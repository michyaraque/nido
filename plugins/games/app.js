/* Juegos: matches in real time over a WebSocket with small binary packets (see src/net.rs),
   moves without reloading, reactions, synthesised sound effects and a bit of game feel.
   Without this script every form still works over plain HTTP. */
(() => {
  // ---------- the protocol: same packets as src/net.rs ----------
  const OP = { HELLO: 0x01, MOVE: 0x02, REACT: 0x03, PING: 0x04, VIEW: 0x81, REACTION: 0x82, ERROR: 0x83, PONG: 0x84 };
  const FLAG = { MY_TURN: 1, WIN: 4, LOSE: 8 };
  const enc = new TextEncoder(), dec = new TextDecoder();

  /// Builds packets: u32 length (LE) · u8 opcode · payload.
  class Out {
    constructor() { this.parts = []; }
    packet(op, write) {
      const body = [];
      const w = {
        u8: (v) => body.push(Uint8Array.of(v)),
        u32: (v) => {
          const b = new Uint8Array(4);
          new DataView(b.buffer).setUint32(0, v >>> 0, true);
          body.push(b);
        },
        str: (s) => {
          const t = enc.encode(s).slice(0, 65535);
          const b = new Uint8Array(2);
          new DataView(b.buffer).setUint16(0, t.length, true);
          body.push(b, t);
        },
      };
      w.u8(op);
      write?.(w);
      const len = body.reduce((n, b) => n + b.length, 0);
      const head = new Uint8Array(4);
      new DataView(head.buffer).setUint32(0, len, true);
      this.parts.push(head, ...body);
      return this;
    }
    bytes() {
      const all = new Uint8Array(this.parts.reduce((n, b) => n + b.length, 0));
      let o = 0;
      for (const b of this.parts) { all.set(b, o); o += b.length; }
      return all;
    }
  }

  /// Reads one message: a NetworkMessage with a position.
  class In {
    constructor(buf) { this.v = new DataView(buf); this.p = 0; }
    left() { return this.v.byteLength - this.p; }
    u8() { return this.v.getUint8(this.p++); }
    u16() { const x = this.v.getUint16(this.p, true); this.p += 2; return x; }
    u32() { const x = this.v.getUint32(this.p, true); this.p += 4; return x; }
    bytes(n) { const b = new Uint8Array(this.v.buffer, this.v.byteOffset + this.p, n); this.p += n; return b; }
    str() { return dec.decode(this.bytes(this.u16())); }
    text() { return dec.decode(this.bytes(this.u32())); }
  }

  // ---------- sounds: synthesised, nothing to download ----------
  let ac = null;
  const muted = () => { try { return localStorage.getItem("gm-mute") === "1"; } catch { return false; } };
  const audio = () => {
    if (!ac) {
      const A = window.AudioContext || window.webkitAudioContext;
      if (!A) return null;
      ac = new A();
    }
    if (ac.state === "suspended") ac.resume();
    return ac;
  };
  function tone(freq, dur, { type = "sine", vol = 0.16, at = 0, slide = 0 } = {}) {
    const a = audio(); if (!a) return;
    const t = a.currentTime + at, o = a.createOscillator(), g = a.createGain();
    o.type = type;
    o.frequency.setValueAtTime(freq, t);
    if (slide) o.frequency.exponentialRampToValueAtTime(Math.max(30, freq * slide), t + dur);
    g.gain.setValueAtTime(0.0001, t);
    g.gain.exponentialRampToValueAtTime(vol, t + 0.012);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    o.connect(g).connect(a.destination);
    o.start(t); o.stop(t + dur + 0.03);
  }
  function noise(dur, { vol = 0.2, at = 0, freq = 1200, q = 1 } = {}) {
    const a = audio(); if (!a) return;
    const t = a.currentTime + at, n = Math.ceil(a.sampleRate * dur), buf = a.createBuffer(1, n, a.sampleRate), d = buf.getChannelData(0);
    for (let i = 0; i < n; i++) d[i] = (Math.random() * 2 - 1) * (1 - i / n) ** 2;
    const s = a.createBufferSource(), f = a.createBiquadFilter(), g = a.createGain();
    s.buffer = buf; f.type = "bandpass"; f.frequency.value = freq; f.Q.value = q; g.gain.value = vol;
    s.connect(f).connect(g).connect(a.destination);
    s.start(t);
  }
  const SFX = {
    tap: () => tone(660, 0.07, { type: "triangle", vol: 0.1 }),
    // A card slapped on the table: a crisp broadband snap and a tiny body.
    card: () => { noise(0.05, { freq: 3800, q: 0.5, vol: 0.38 }); noise(0.07, { freq: 900, q: 0.8, vol: 0.12, at: 0.01 }); },
    skip: () => { noise(0.05, { freq: 3500, q: 0.5, vol: 0.3 }); tone(180, 0.22, { type: "square", vol: 0.07, at: 0.04, slide: 0.6 }); },
    two: () => { SFX.card(); noise(0.05, { freq: 3500, q: 0.5, vol: 0.3, at: 0.12 }); tone(110, 0.25, { vol: 0.2, at: 0.1, slide: 0.7 }); },
    "turn-around": () => { noise(0.05, { freq: 3500, q: 0.5, vol: 0.3 }); tone(500, 0.12, { type: "triangle", vol: 0.1, at: 0.04, slide: 1.6 }); tone(800, 0.12, { type: "triangle", vol: 0.1, at: 0.16, slide: 0.6 }); },
    wild: () => { noise(0.05, { freq: 3500, q: 0.5, vol: 0.3 }); [880, 1109, 1319, 1760].forEach((f, i) => tone(f, 0.12, { type: "triangle", vol: 0.08, at: 0.05 + i * 0.06 })); },
    one: () => { tone(988, 0.1, { vol: 0.14 }); tone(988, 0.16, { vol: 0.14, at: 0.14 }); },
    // A card sliding off the deck.
    draw: () => { noise(0.16, { freq: 1500, q: 0.4, vol: 0.18 }); tone(140, 0.12, { vol: 0.1, at: 0.08, slide: 0.8 }); },
    dice: () => { for (let i = 0; i < 6; i++) noise(0.035, { freq: 900 + i * 260, q: 4, vol: 0.25, at: i * 0.05 }); },
    drop: () => { tone(320, 0.16, { vol: 0.2, slide: 0.45 }); noise(0.05, { freq: 600, vol: 0.2, at: 0.13 }); },
    splash: () => noise(0.35, { freq: 650, q: 0.5, vol: 0.22 }),
    hit: () => { noise(0.25, { freq: 220, q: 0.6, vol: 0.4 }); tone(120, 0.22, { type: "square", vol: 0.05, slide: 0.5 }); },
    sink: () => { noise(0.6, { freq: 150, q: 0.5, vol: 0.45 }); tone(95, 0.6, { type: "sawtooth", vol: 0.06, slide: 0.35 }); },
    eat: () => { tone(520, 0.07, { type: "square", vol: 0.07 }); tone(260, 0.18, { type: "square", vol: 0.07, at: 0.07, slide: 0.6 }); },
    reveal: () => { noise(0.05, { freq: 1500, vol: 0.2 }); tone(660, 0.12, { vol: 0.12, at: 0.06 }); tone(990, 0.16, { vol: 0.12, at: 0.14 }); },
    turn: () => { tone(784, 0.12, { vol: 0.12 }); tone(1175, 0.2, { vol: 0.12, at: 0.1 }); },
    win: () => [523, 659, 784, 1047, 1319].forEach((f, i) => tone(f, 0.24, { type: "triangle", vol: 0.14, at: i * 0.1 })),
    lose: () => [392, 330, 262].forEach((f, i) => tone(f, 0.3, { type: "triangle", vol: 0.12, at: i * 0.15 })),
    pop: () => tone(880, 0.08, { vol: 0.12, slide: 1.5 }),
    step: () => tone(1250, 0.035, { type: "triangle", vol: 0.05 }),
    error: () => tone(160, 0.18, { type: "square", vol: 0.06 }),
  };
  const SOUNDS = ["tap", "card", "draw", "dice", "drop", "splash", "hit", "sink", "eat", "reveal", "skip", "two", "turn-around", "wild", "one"]; // app.rs SOUNDS
  const play = (name) => { if (!muted() && SFX[name]) try { SFX[name](); } catch {} };
  addEventListener("pointerdown", () => { if (document.getElementById("gm-live")) audio(); }, { capture: true, passive: true });

  // ---------- game feel ----------
  const calm = matchMedia("(prefers-reduced-motion: reduce)");
  function shake() {
    const el = document.querySelector("#gm-live .gm-move");
    if (!el || calm.matches) return;
    el.classList.remove("gm-shake"); void el.offsetWidth; el.classList.add("gm-shake");
  }
  function confetti() {
    if (calm.matches) return;
    const box = document.createElement("div");
    box.className = "gm-confetti";
    const colors = ["#e53935", "#f2b705", "#43a047", "#1e6fd9", "#7c3aed", "#ec4899"];
    for (let i = 0; i < 70; i++) {
      const p = document.createElement("i");
      p.style.left = Math.random() * 100 + "vw";
      p.style.background = colors[i % colors.length];
      box.append(p);
      p.animate(
        [{ transform: `translate(0,0) rotate(0)` }, { transform: `translate(${(Math.random() - 0.5) * 220}px, ${innerHeight + 40}px) rotate(${Math.random() * 900}deg)` }],
        { duration: 1600 + Math.random() * 1300, delay: Math.random() * 300, easing: "cubic-bezier(.2,.6,.4,1)", fill: "forwards" },
      );
    }
    document.body.append(box);
    setTimeout(() => box.remove(), 3400);
  }
  /// The moved pawn walks its path square by square with a little hop (parchís).
  function walk() {
    if (calm.matches) return 0;
    let longest = 0;
    for (const g of document.querySelectorAll("#gm-live [data-steps]")) {
      const pts = g.dataset.steps.trim().split(" ").map((p) => p.split(",").map(Number));
      if (pts.length < 2) continue;
      const n = pts.length - 1, each = Math.min(180, 1000 / n), frames = [];
      pts.forEach(([x, y], i) => {
        if (i > 0) {
          const [px, py] = pts[i - 1];
          frames.push({ transform: `translate(${(px + x) / 2}px, ${(py + y) / 2 - 7}px)`, offset: (i - 0.5) / n });
        }
        frames.push({ transform: `translate(${x}px, ${y}px)`, offset: i / n });
      });
      g.animate(frames, { duration: each * n, easing: "linear", fill: "backwards" });
      for (let i = 1; i <= n; i++) setTimeout(() => play("step"), each * i);
      longest = Math.max(longest, each * n);
    }
    return longest;
  }

  function feel(sound, flags, mineBefore) {
    const walking = walk();
    if (walking) {
      // Let the pawn arrive before the capture or the win sounds.
      return setTimeout(() => feelNow(sound, flags, mineBefore, true), walking);
    }
    feelNow(sound, flags, mineBefore, false);
  }

  function feelNow(sound, flags, mineBefore, walked) {
    const name = SOUNDS[sound] || "tap";
    if (flags & FLAG.WIN) {
      play("win");
      confetti();
      return;
    }
    if (flags & FLAG.LOSE) return play("lose");
    if (!(walked && name === "dice")) play(name);
    if (name === "hit" || name === "sink" || name === "eat") shake();
    if (flags & FLAG.MY_TURN && !mineBefore) setTimeout(() => play("turn"), 280);
  }

  // ---------- Colors: cards fly from where they come to where they go ----------
  let clicked = null; // the card I just tapped, so mine leaves from my hand
  const rect = (el) => el?.getBoundingClientRect();
  function fly(el, from, { rotate = 0, delay = 0 } = {}) {
    const to = rect(el);
    if (!el || !from || !to || calm.matches) return;
    const dx = from.left + from.width / 2 - (to.left + to.width / 2), dy = from.top + from.height / 2 - (to.top + to.height / 2);
    const s = from.width / to.width || 1;
    el.animate(
      [{ transform: `translate(${dx}px, ${dy}px) scale(${s}) rotate(${rotate}deg)`, opacity: 0.9 }, { transform: "none", opacity: 1 }],
      { duration: 420, delay, easing: "cubic-bezier(.2,.9,.25,1.15)", fill: "backwards" },
    );
  }
  function cardsMoved(old, next) {
    const t = next.querySelector(".gm-colores");
    if (!t || !old.querySelector(".gm-colores")) return;
    const act = t.dataset.act, seat = t.dataset.seat, me = t.dataset.me;
    const was = (sel) => rect(old.querySelector(sel));
    if (act === "play") {
      const from = seat === me ? clicked : was(`.gm-other[data-seat="${seat}"] .gm-backs`);
      fly(next.querySelector(".gm-pile-card.top"), from || was(".gm-deck"), { rotate: seat === me ? -12 : 18 });
    } else if (act === "draw") {
      const deck = was(".gm-deck");
      if (seat === me) fly(next.querySelector(".gm-hand > :last-child"), deck, { rotate: 10 });
      else fly(next.querySelector(`.gm-other[data-seat="${seat}"] .gm-backs i:last-child`), deck, { rotate: -10 });
    }
    // A +2: two cards slide off the deck towards whoever has to take them.
    const victim = t.dataset.victim;
    if (victim !== undefined) {
      const deck = was(".gm-deck");
      const target = victim === me ? next.querySelectorAll(".gm-hand > *") : next.querySelectorAll(`.gm-other[data-seat="${victim}"] .gm-backs i`);
      [...target].slice(-2).forEach((el, i) => fly(el, deck, { rotate: 8, delay: 260 + i * 140 }));
    }
    // The arrows spin round when the direction changes.
    const a0 = old.querySelector(".gm-colores")?.dataset.dir, a1 = t.dataset.dir;
    if (a0 && a1 && a0 !== a1 && !calm.matches) next.querySelector(".gm-arrows")?.animate([{ rotate: "0deg" }, { rotate: "180deg" }], { duration: 600, easing: "cubic-bezier(.3,1.4,.5,1)" });
    clicked = null;
  }
  document.addEventListener("pointerdown", (e) => {
    const c = e.target.closest?.(".gm-hand .gm-card-btn");
    if (c) clicked = rect(c);
  }, true);

  // ---------- the live part ----------
  const box = () => document.getElementById("gm-live");
  function swap(html) {
    const old = box(); if (!old) return null;
    const t = document.createElement("template");
    t.innerHTML = html.trim();
    const next = t.content.firstElementChild;
    if (next?.id !== "gm-live" || +next.dataset.v < +old.dataset.v) return null;
    const picked = old.querySelector("input[name=k]:checked")?.value;
    old.replaceWith(next);
    if (picked) {
      const r = next.querySelector(`input[name=k][value="${picked}"]`);
      if (r) r.checked = true;
    }
    cardsMoved(old, next);
    if (next.dataset.turn) document.title = document.hidden ? "• " + document.title.replace(/^• /, "") : document.title;
    init();
    return next;
  }
  function toast(msg) {
    document.querySelector(".toast")?.remove();
    const t = document.createElement("div");
    t.className = "toast error";
    t.setAttribute("role", "alert");
    t.textContent = String(msg).slice(0, 160);
    document.body.append(t);
  }
  function bubble(seat, html) {
    const p = document.querySelectorAll("#gm-live .gm-player")[seat];
    if (!p) return;
    p.querySelector(".gm-bubble")?.remove();
    const b = document.createElement("span");
    b.className = "gm-bubble" + (html.startsWith("<") ? "" : " emoji");
    if (html.startsWith("<")) b.innerHTML = html;
    else b.textContent = html;
    p.append(b);
    setTimeout(() => b.remove(), 3300);
  }

  // ---------- the socket ----------
  let ws = null, wsFor = null, retry = 0, ping = 0;
  function connect() {
    const el = box();
    const id = el?.dataset.live ? +el.dataset.id : null;
    if (id === wsFor && ws && ws.readyState <= 1) return;
    ws?.close();
    ws = null;
    wsFor = id;
    clearInterval(ping);
    if (!id || !("WebSocket" in window)) return;
    const month = new URL(location.href).searchParams.get("month") || "";
    const url = `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/plugins/games/ws?month=${encodeURIComponent(month)}`;
    const s = new WebSocket(url);
    s.binaryType = "arraybuffer";
    ws = s;
    s.onopen = () => {
      retry = 0;
      const v = +(box()?.dataset.v || 0);
      s.send(new Out().packet(OP.HELLO, (w) => { w.u32(id); w.u32(v); }).bytes());
      ping = setInterval(() => s.readyState === 1 && s.send(new Out().packet(OP.PING).bytes()), 25000);
    };
    s.onmessage = (e) => {
      const m = new In(e.data);
      while (m.left() >= 5) {
        const len = m.u32();
        if (len < 1 || len > m.left()) break; // a malformed packet ends the message
        const end = m.p + len, op = m.u8();
        if (op === OP.VIEW) {
          const match = m.u32(), version = m.u32(), sound = m.u8(), flags = m.u8(), html = m.text();
          const old = box();
          if (old && +old.dataset.id === match && version > +old.dataset.v) {
            const mineBefore = !!old.dataset.turn;
            if (swap(html)) feel(sound, flags, mineBefore);
          }
        } else if (op === OP.REACTION) {
          const match = m.u32(), seat = m.u8(), r = m.str();
          if (+box()?.dataset.id === match) {
            const tpl = document.querySelector(`#gm-live .gm-emojis button[value="${CSS.escape(r)}"]`);
            bubble(seat, tpl ? tpl.innerHTML : r);
            play("pop");
          }
        } else if (op === OP.ERROR) {
          toast(m.str());
          play("error");
          unlock();
        }
        m.p = end;
      }
    };
    s.onclose = () => {
      clearInterval(ping);
      if (ws !== s || !box()?.dataset.live) return;
      ws = null;
      setTimeout(connect, Math.min(8000, 400 * 2 ** retry++)); // back off, then reconnect and catch up with HELLO
    };
  }
  const unlock = () => document.querySelectorAll("#gm-live form.gm-move button").forEach((b) => (b.disabled = false));

  // ---------- moves and reactions: over the socket, or over HTTP if it is not open ----------
  /// The HTTP answer has no packet header: the sound and the outcome come as data-* on the new view.
  function flagsOf(view) {
    if (view.dataset.end === "win") return FLAG.WIN;
    if (view.dataset.end === "lose") return FLAG.LOSE;
    return view.dataset.turn ? FLAG.MY_TURN : 0;
  }
  document.addEventListener("submit", async (e) => {
    const f = e.target;
    if (!f.matches?.("form.gm-live-form")) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    audio();
    const id = +box()?.dataset.id;
    let value = e.submitter?.value ?? "";
    if (value.includes("{k}")) value = value.replace("{k}", f.querySelector("input[name=k]:checked")?.value ?? "0");
    f.closest("details")?.removeAttribute("open");
    const action = new URL(f.getAttribute("action"), location.href).href;
    const react = action.endsWith("/react");
    if (ws?.readyState === 1 && id) {
      if (!react) {
        play("tap");
        document.querySelectorAll("#gm-live form.gm-move button").forEach((b) => (b.disabled = true));
      }
      ws.send(new Out().packet(react ? OP.REACT : OP.MOVE, (w) => { w.u32(id); w.str(value); }).bytes());
      return;
    }
    const body = new URLSearchParams(new FormData(f, e.submitter));
    body.set("m", value);
    try {
      const r = await fetch(action + "?partial=1", { method: "POST", body, credentials: "same-origin" });
      const html = await r.text();
      if (!r.ok) {
        toast(html || "No se ha podido hacer");
        play("error");
        return;
      }
      const mine = !!box()?.dataset.turn;
      const next = swap(html);
      if (next) feel(SOUNDS.indexOf(next.dataset.sfx), flagsOf(next), mine);
    } catch {
      location.reload();
    }
  }, true);

  // ---------- dominó: drag a tile from the hand to the end of the line where it goes ----------
  let tile = null;
  document.addEventListener("pointerdown", (e) => {
    const pick = e.target.closest?.("#gm-live .gm-dom-pick");
    if (!pick || e.button > 0) return;
    tile = { pick, btn: e.target.closest("button"), id: e.pointerId, x: e.clientX, y: e.clientY, ghost: null, over: null };
    // With a mouse the browser would start selecting or its own drag and cancel ours; taps are pressed in dropTile.
    e.preventDefault();
    try { pick.setPointerCapture(e.pointerId); } catch {}
  }, true);
  document.addEventListener("dragstart", (e) => { if (e.target.closest?.("#gm-live .gm-dom-pick")) e.preventDefault(); }, true);
  document.addEventListener("pointermove", (e) => {
    if (!tile || e.pointerId !== tile.id) return;
    if (!tile.ghost) {
      if (Math.hypot(e.clientX - tile.x, e.clientY - tile.y) < 8) return;
      const svg = tile.pick.querySelector(".gm-tile"), r = svg.getBoundingClientRect();
      const g = svg.cloneNode(true);
      g.classList.add("gm-dom-ghost");
      g.style.width = r.width + "px";
      g.style.height = r.height + "px";
      tile.dx = e.clientX - r.left;
      tile.dy = e.clientY - r.top;
      document.body.append(g);
      tile.ghost = g;
      tile.pick.classList.add("lifted");
      box()?.classList.add("gm-dom-dragging");
      box()?.querySelectorAll(".gm-dom-drop").forEach((z) => z.classList.toggle("ok", tile.pick.dataset[z.dataset.side] === "1"));
    }
    e.preventDefault();
    tile.ghost.style.transform = `translate(${e.clientX - tile.dx}px, ${e.clientY - tile.dy}px) rotate(-6deg)`;
    const z = document.elementFromPoint(e.clientX, e.clientY)?.closest?.(".gm-dom-drop.ok") || null;
    if (z !== tile.over) { tile.over?.classList.remove("hot"); z?.classList.add("hot"); tile.over = z; }
  }, { passive: false });
  function dropTile(e) {
    if (!tile || e.pointerId !== tile.id) return;
    const t = tile;
    tile = null;
    if (!t.ghost) {
      // A tap. The captured pointer lands on the tile, not on its button: press that button here.
      if (e.type === "pointerup" && t.btn && !t.btn.disabled) t.btn.form?.requestSubmit(t.btn);
      return;
    }
    t.ghost.remove();
    t.pick.classList.remove("lifted");
    t.over?.classList.remove("hot");
    box()?.classList.remove("gm-dom-dragging");
    if (e.type !== "pointerup" || !t.over) return;
    const form = box()?.querySelector("form.gm-move");
    if (!form) return;
    const b = Object.assign(document.createElement("button"), { type: "submit", name: "m", value: `lay:${t.pick.dataset.i}:${t.over.dataset.side}`, hidden: true });
    form.append(b);
    form.requestSubmit(b);
    b.remove();
  }
  document.addEventListener("pointerup", dropTile);
  document.addEventListener("pointercancel", dropTile);
  document.addEventListener("click", (e) => {
    if (e.target.closest?.("#gm-live .gm-dom-pick") && e.detail > 0) { e.preventDefault(); e.stopPropagation(); }
  }, true);

  // ---------- hundir la flota: drag a ship with the finger (or mouse); tap a picked ship to turn it ----------
  let drag = null;
  document.addEventListener("pointerdown", (e) => {
    const b = e.target.closest?.(".gm-ocean.placing .gm-boat");
    if (!b || e.button > 0) return;
    const input = b.querySelector("input"), r = b.getBoundingClientRect();
    drag = { b, picked: input.checked, x: e.clientX, y: e.clientY, ox: e.clientX - r.left, oy: e.clientY - r.top, moved: false };
    input.checked = true;
    b.setPointerCapture(e.pointerId);
    e.preventDefault();
  });
  document.addEventListener("pointermove", (e) => {
    if (!drag) return;
    const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
    if (!drag.moved && Math.hypot(dx, dy) < 8) return;
    drag.moved = true;
    drag.b.classList.add("dragging");
    drag.b.style.transform = `translate(${dx}px, ${dy}px)`;
  });
  const drop = (e) => {
    if (!drag) return;
    const { b, picked, moved, ox, oy } = drag;
    drag = null;
    const f = b.closest("form");
    const back = () => setTimeout(() => { if (b.isConnected) { b.style.transform = ""; b.classList.remove("dragging"); } }, 900);
    if (!moved) {
      if (picked && e.type === "pointerup") f.requestSubmit(f.querySelector('button[value^="turn:"]'));
      return;
    }
    // The square under the ship's first square is where it starts.
    const cell = f.querySelector(".gm-cells button").getBoundingClientRect();
    const x = e.clientX - ox + cell.width / 2, y = e.clientY - oy + cell.height / 2;
    const to = document.elementsFromPoint(x, y).find((el) => el.matches(".gm-ocean.placing .gm-cells button"));
    if (to) f.requestSubmit(to);
    back();
  };
  document.addEventListener("pointerup", drop);
  document.addEventListener("pointercancel", drop);
  document.addEventListener("click", (e) => { if (e.target.closest?.(".gm-ocean.placing .gm-boat")) e.preventDefault(); }, true);

  // The comodín picker closes when you tap outside the diamond.
  document.addEventListener("click", (e) => {
    const picker = e.target.closest?.(".gm-picker");
    if (picker && !e.target.closest(".gm-diamond")) picker.closest("details")?.removeAttribute("open");
    const mute = e.target.closest?.(".gm-mute");
    if (mute) {
      e.preventDefault();
      const m = !muted();
      try { localStorage.setItem("gm-mute", m ? "1" : "0"); } catch {}
      mute.classList.toggle("off", m);
      mute.setAttribute("aria-pressed", String(m));
      if (!m) play("pop");
    }
  });
  document.addEventListener("visibilitychange", () => { if (!document.hidden) document.title = document.title.replace(/^• /, ""); });

  // ---------- full screen: the real one where the browser allows it, a covering layer elsewhere (iPhone) ----------
  let lock = null;
  async function full(on) {
    const stage = document.querySelector(".gm-stage");
    if (!stage) return;
    stage.classList.toggle("full", on);
    document.documentElement.classList.toggle("gm-noscroll", on);
    if (on) {
      try { await stage.requestFullscreen?.({ navigationUI: "hide" }); } catch {}
      try { lock = await navigator.wakeLock?.request("screen"); } catch {} // the screen stays on while playing
    } else {
      if (document.fullscreenElement) try { await document.exitFullscreen(); } catch {}
      lock?.release?.();
      lock = null;
    }
  }
  document.addEventListener("click", (e) => {
    if (e.target.closest?.(".gm-full")) { e.preventDefault(); full(!document.querySelector(".gm-stage.full")); }
  });
  document.addEventListener("fullscreenchange", () => { if (!document.fullscreenElement && document.querySelector(".gm-stage.full")) full(false); });
  document.addEventListener("keydown", (e) => { if (e.key === "Escape" && document.querySelector(".gm-stage.full")) full(false); });

  // ---------- "¡Te toca!": a big card for a moment when the turn becomes mine ----------
  let poppedFor = null; // the match whose current turn of mine was already announced
  function announceTurn() {
    const el = box();
    if (!el || el.dataset.live !== "1") return;
    if (!el.dataset.turn) {
      if (poppedFor === el.dataset.id) poppedFor = null;
      return;
    }
    if (poppedFor === el.dataset.id) return;
    poppedFor = el.dataset.id;
    const pop = el.querySelector(".gm-pop");
    if (!pop) return;
    pop.classList.add("show");
    setTimeout(() => pop.classList.remove("show"), calm.matches ? 1400 : 1900);
  }

  function init() {
    announceTurn();
    if (!document.querySelector(".gm-stage.full")) document.documentElement.classList.remove("gm-noscroll");
    const off = muted();
    for (const b of document.querySelectorAll(".gm-mute")) {
      b.classList.toggle("off", off);
      b.setAttribute("aria-pressed", String(off));
    }
    connect();
  }
  (window.nidoPlugins ||= {}).games = init;
  init();
})();
