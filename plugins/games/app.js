/* Juegos: matches in real time over a WebSocket with small binary packets (see src/net.rs),
   moves without reloading, reactions, synthesised sound effects and a bit of game feel.
   Without this script every form still works over plain HTTP. */
(() => {
  // ---------- the protocol: same packets as src/net.rs ----------
  const OP = { HELLO: 0x01, MOVE: 0x02, REACT: 0x03, PING: 0x04, VIEW: 0x81, REACTION: 0x82, ERROR: 0x83, PONG: 0x84, STATE: 0x85 };
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
  // ---------- built from the physics of the thing: a card is paper on felt, a reward is a struck bell ----------
  // Every layer has an attack, a body and a tail; highs die first; only tonal layers reach the little
  // room, never the hits; each play varies a little so a sound heard a hundred times does not tire.
  let noiseBuf = null, room = null;
  const vary = (x, amt = 0.06) => x * (1 + (Math.random() * 2 - 1) * amt);
  function noiseOf(a) {
    if (noiseBuf) return noiseBuf;
    const n = a.sampleRate * 2, b = a.createBuffer(1, n, a.sampleRate), d = b.getChannelData(0);
    for (let i = 0; i < n; i++) d[i] = Math.random() * 2 - 1;
    return (noiseBuf = b);
  }
  /// A small, dark room for bells and pads: a short tail that darkens as it fades.
  function roomOf(a) {
    if (room) return room;
    const len = Math.floor(a.sampleRate * 1.1), ir = a.createBuffer(2, len, a.sampleRate);
    for (let ch = 0; ch < 2; ch++) {
      const d = ir.getChannelData(ch);
      let lp = 0;
      for (let i = 0; i < len; i++) {
        const k = i / len;
        lp += (Math.random() * 2 - 1 - lp) * (0.9 - 0.75 * k); // later = darker
        d[i] = lp * (1 - k) ** 2.4;
      }
    }
    const c = a.createConvolver(), g = a.createGain();
    c.buffer = ir;
    g.gain.value = 0.5;
    c.connect(g).connect(a.destination);
    return (room = c);
  }
  /// Filtered noise: the paper, the felt, the friction of a card sliding.
  function grain({ at = 0, dur = 0.05, f = 2000, to = f, q = 0.8, vol = 0.2, attack = 0.002, type = "bandpass", pan = 0, panTo = pan }) {
    const a = audio(); if (!a) return;
    const t = a.currentTime + at, s = a.createBufferSource(), fl = a.createBiquadFilter(), g = a.createGain(), p = a.createStereoPanner();
    s.buffer = noiseOf(a);
    fl.type = type; fl.Q.value = q;
    fl.frequency.setValueAtTime(vary(f, 0.08), t);
    fl.frequency.exponentialRampToValueAtTime(vary(to, 0.08), t + dur);
    g.gain.setValueAtTime(0.0001, t);
    g.gain.exponentialRampToValueAtTime(vary(vol, 0.1), t + attack);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    p.pan.setValueAtTime(pan, t);
    p.pan.linearRampToValueAtTime(panTo, t + dur);
    s.connect(fl).connect(g).connect(p).connect(a.destination);
    s.start(t, Math.random() * 1.5); s.stop(t + dur + 0.03);
  }
  /// A low body that drops in pitch: the felt table taking the weight.
  const thump = (at, f = 150, vol = 0.12, dur = 0.08) => tone(vary(f, 0.05), dur, { vol: vary(vol, 0.1), at, slide: 0.55 });
  /// A struck bar: near-harmonic partials as beating pairs (never a clean test tone), the strike
  /// tick on top, the high partials dying first, a little room.
  function bell(f, { at = 0, dur = 1, vol = 0.06, send = 0.35 } = {}) {
    const a = audio(); if (!a) return;
    const t = a.currentTime + at, out = a.createGain();
    out.gain.value = 1;
    out.connect(a.destination);
    if (send) { const s = a.createGain(); s.gain.value = send; out.connect(s).connect(roomOf(a)); }
    for (const [r, lv, life] of [[1, 1, 1], [2.01, 0.42, 0.55], [3.02, 0.18, 0.32], [4.23, 0.07, 0.18]]) {
      for (const beat of [-0.7, 0.7]) {
        const o = a.createOscillator(), g = a.createGain();
        o.frequency.value = f * r + beat * r;
        g.gain.setValueAtTime(0.0001, t);
        g.gain.exponentialRampToValueAtTime(vol * lv / 2, t + 0.004);
        g.gain.exponentialRampToValueAtTime(0.0001, t + dur * life);
        o.connect(g).connect(out);
        o.start(t); o.stop(t + dur * life + 0.05);
      }
    }
    grain({ at, dur: 0.014, f: 6500, q: 0.6, vol: vol * 0.9 });
  }
  /// A soft held chord under a reward: slow attack, slow release.
  function pad(freqs, { at = 0, dur = 1.4, vol = 0.03 } = {}) {
    const a = audio(); if (!a) return;
    const t = a.currentTime + at, s = a.createGain();
    s.gain.value = 0.6;
    s.connect(roomOf(a));
    for (const f of freqs) {
      for (const beat of [-1.1, 1.1]) {
        const o = a.createOscillator(), g = a.createGain();
        o.type = "triangle";
        o.frequency.value = f + beat;
        g.gain.setValueAtTime(0.0001, t);
        g.gain.exponentialRampToValueAtTime(vol / freqs.length, t + 0.12);
        g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
        o.connect(g); g.connect(a.destination); g.connect(s);
        o.start(t); o.stop(t + dur + 0.05);
      }
    }
  }
  /// When a thrown card lands on the pile (the toss animation reaches it about here).
  const LAND = 0.32;
  // Cozy: round tones (a wooden marimba, bubbles), soft attacks, a little bounce, a small room, and
  // every random note taken from a major pentatonic scale so the variety always sounds in tune.
  const PENTA = [523.25, 587.33, 659.25, 783.99, 880, 1046.5, 1174.66, 1318.5]; // C D E G A C D E
  const pick = (from = 0, to = PENTA.length) => PENTA[from + Math.floor(Math.random() * (to - from))];
  /// A wooden bar struck softly: partials 1, 3.9 and 9.2 (a marimba), the upper ones gone first.
  function marimba(f, { at = 0, dur = 0.45, vol = 0.12, send = 0.25 } = {}) {
    const a = audio(); if (!a) return;
    const t = a.currentTime + at, out = a.createGain();
    out.connect(a.destination);
    if (send) { const s = a.createGain(); s.gain.value = send; out.connect(s).connect(roomOf(a)); }
    for (const [r, lv, life] of [[1, 1, 1], [3.9, 0.22, 0.25], [9.2, 0.06, 0.1]]) {
      const o = a.createOscillator(), g = a.createGain();
      o.frequency.value = f * r;
      g.gain.setValueAtTime(0.0001, t);
      g.gain.exponentialRampToValueAtTime(vol * lv, t + 0.003);
      g.gain.exponentialRampToValueAtTime(0.0001, t + dur * life);
      o.connect(g).connect(out);
      o.start(t); o.stop(t + dur * life + 0.03);
    }
    grain({ at, dur: 0.01, f: 1800, q: 0.6, vol: vol * 0.25, attack: 0.001, type: "lowpass" }); // the soft mallet
  }
  /// A bubbly pop: a round tone whose pitch jumps up as it fades.
  function bubble(f = 420, { at = 0, vol = 0.12, up = 2.2, dur = 0.07 } = {}) {
    const a = audio(); if (!a) return;
    const t = a.currentTime + at, o = a.createOscillator(), g = a.createGain();
    o.frequency.setValueAtTime(vary(f, 0.06), t);
    o.frequency.exponentialRampToValueAtTime(f * up, t + dur);
    g.gain.setValueAtTime(0.0001, t);
    g.gain.exponentialRampToValueAtTime(vol, t + 0.004);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    o.connect(g).connect(a.destination);
    o.start(t); o.stop(t + dur + 0.02);
  }
  /// A bouncy boop: a soft rounded tone that overshoots its pitch a little and settles (or sinks).
  function boop(f, { at = 0, dur = 0.12, vol = 0.1, end = 1, send = 0.15 } = {}) {
    const a = audio(); if (!a) return;
    const t = a.currentTime + at, o = a.createOscillator(), lp = a.createBiquadFilter(), g = a.createGain();
    o.type = "triangle";
    o.frequency.setValueAtTime(f * 1.12, t);
    o.frequency.exponentialRampToValueAtTime(f, t + 0.03);
    o.frequency.exponentialRampToValueAtTime(f * end, t + dur);
    lp.type = "lowpass"; lp.frequency.value = Math.min(4000, f * 4);
    g.gain.setValueAtTime(0.0001, t);
    g.gain.exponentialRampToValueAtTime(vol, t + 0.006);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    o.connect(lp).connect(g).connect(a.destination);
    if (send) { const s = a.createGain(); s.gain.value = send; g.connect(s).connect(roomOf(a)); }
    o.start(t); o.stop(t + dur + 0.03);
  }
  /// A card meeting the pile: a soft paper tap (no harsh highs), the round pop and a wooden note.
  function cozyCard(at = 0, vol = 1) {
    grain({ at, dur: 0.03, f: 2200, to: 1400, q: 0.9, vol: 0.14 * vol, attack: 0.002 });
    bubble(380, { at, vol: 0.09 * vol, up: 1.9, dur: 0.06 });
    marimba(pick(0, 5), { at: at + 0.01, dur: 0.35, vol: 0.09 * vol });
  }
  /// A card pulled off the deck: a gentle swish and a little rising blip.
  function cozyDraw(at = 0, vol = 1) {
    grain({ at, dur: 0.12, f: 900, to: 2000, q: 0.8, vol: 0.05 * vol, attack: 0.08, type: "bandpass" });
    boop(659.25, { at: at + 0.06, dur: 0.07, vol: 0.06 * vol, end: 1.12 });
    boop(987.77, { at: at + 0.11, dur: 0.09, vol: 0.06 * vol, end: 1.05 });
  }
  /// A card settling into a hand: a soft low pop.
  const settle = (at = 0, vol = 1) => bubble(220, { at, vol: 0.12 * vol, up: 1.6, dur: 0.08 });
  /// A riffle: clickety soft clicks, then the stack settles with a pop.
  function riffle(at = 0) {
    for (let i = 0; i < 12; i++) grain({ at: at + i * 0.028 + Math.random() * 0.006, dur: 0.012, f: 2200 + Math.random() * 1600, q: 1.4, vol: 0.05 * Math.sin(Math.PI * (i + 0.5) / 12), attack: 0.001 });
    settle(at + 0.36);
    marimba(783.99, { at: at + 0.38, dur: 0.3, vol: 0.06 });
  }
  const NOTE = { G3: 196, A3: 220, C4: 261.63, D4: 293.66, E4: 329.63, F4: 349.23, G4: 392, A4: 440, B4: 493.88, C5: 523.25, D5: 587.33, E5: 659.25, G5: 783.99, A5: 880, B5: 987.77, C6: 1046.5, D6: 1174.66, E6: 1318.5, G6: 1567.98 };
  const buzz = (ms) => { try { navigator.vibrate?.(ms); } catch {} };
  let stepN = 0, stepAt = 0;

  const SFX = {
    tap: () => { grain({ dur: 0.015, f: 1600, q: 1, vol: 0.08, attack: 0.001, type: "lowpass" }); bubble(700, { vol: 0.05, up: 1.4, dur: 0.04 }); },
    // Pressing a card I can play: a clickety click-in with a tiny pop.
    select: () => { grain({ dur: 0.012, f: 1900, q: 1.2, vol: 0.09, attack: 0.001 }); bubble(520, { vol: 0.07, up: 1.5, dur: 0.045 }); buzz(8); },
    card: () => cozyCard(LAND),
    // The server said no: a boing that sinks, back to the hand.
    return: () => { boop(440, { dur: 0.22, vol: 0.09, end: 0.62 }); settle(0.2, 0.6); },
    // You lose your turn: a cute "bonk" — two boops falling.
    skip: () => { cozyCard(LAND); boop(NOTE.G4, { at: LAND + 0.1, dur: 0.12, vol: 0.11 }); boop(NOTE.C4, { at: LAND + 0.23, dur: 0.2, vol: 0.11, end: 0.94 }); buzz([20, 40, 20]); },
    // Someone else loses theirs: the same two boops, lighter and quicker.
    "skip-other": () => { cozyCard(LAND); boop(NOTE.E5, { at: LAND + 0.1, dur: 0.09, vol: 0.07 }); boop(NOTE.A4, { at: LAND + 0.19, dur: 0.14, vol: 0.07, end: 0.95 }); },
    // +2: the card lands, two little cards hop off the deck and pop into the victim's hand.
    two: () => { cozyCard(LAND); [0, 1].forEach((i) => { boop(NOTE.G5 + i * 200, { at: 0.42 + i * 0.14, dur: 0.07, vol: 0.06, end: 1.15 }); settle(0.68 + i * 0.14, 0.8); }); },
    // Gira: the card lands and a little whirl goes up and down, with a twinkle at the top.
    "turn-around": () => { cozyCard(LAND); boop(NOTE.C5, { at: LAND + 0.08, dur: 0.18, vol: 0.06, end: 1.5 }); boop(NOTE.G5, { at: LAND + 0.24, dur: 0.18, vol: 0.06, end: 0.67 }); bell(NOTE.E6, { at: LAND + 0.2, dur: 0.5, vol: 0.025 }); },
    // Comodín: the card lands with a sparkle (the colour was chosen with its own notes).
    wild: () => { cozyCard(LAND); bell(NOTE.G6, { at: LAND + 0.05, dur: 0.6, vol: 0.025 }); },
    // Choosing a colour: three bubbly blips stepping up.
    "picker-select": () => [NOTE.C6, NOTE.E6, NOTE.G6].forEach((f, i) => { marimba(f, { at: i * 0.07, dur: 0.3, vol: 0.07 }); bubble(f / 2, { at: i * 0.07, vol: 0.04, up: 1.6, dur: 0.05 }); }),
    "picker-show": () => { bubble(300, { vol: 0.11, up: 2.4, dur: 0.09 }); boop(NOTE.G5, { at: 0.05, dur: 0.1, vol: 0.05, end: 1.1 }); },
    "picker-hide": () => { boop(NOTE.G5, { dur: 0.1, vol: 0.05, end: 0.75 }); bubble(500, { at: 0.06, vol: 0.06, up: 0.6, dur: 0.07 }); },
    // "¡Le queda una!": two high, cheeky boops.
    one: () => { boop(NOTE.A5, { dur: 0.08, vol: 0.09 }); boop(NOTE.A5, { at: 0.13, dur: 0.12, vol: 0.09, end: 1.06 }); },
    // A card from the deck to a hand: the swish and blip, then it pops into place.
    draw: () => { cozyDraw(); settle(0.38, 0.7); },
    deal: () => settle(),
    shuffle: () => riffle(),
    // Wooden dice tumbling: knocks on pentatonic notes, closer and softer as they settle (a bounce).
    dice: () => { let t = 0, gap = 0.11; for (let i = 0; i < 6; i++) { marimba(pick(3), { at: t, dur: 0.12, vol: 0.065 * (1 - i * 0.1), send: 0.1 }); grain({ at: t, dur: 0.01, f: 2400, q: 1, vol: 0.05, attack: 0.001 }); t += gap; gap *= 0.72; } bubble(260, { at: t + 0.02, vol: 0.06, up: 1.6 }); },
    // C4: the disc slides down the column and lands with a wooden tok.
    drop: () => { boop(NOTE.C6, { dur: 0.18, vol: 0.05, end: 0.45 }); marimba(NOTE.G4, { at: 0.16, dur: 0.25, vol: 0.1 }); bubble(200, { at: 0.16, vol: 0.08, up: 1.6 }); },
    // Fleet, water: a little string of bubbles.
    splash: () => { for (let i = 0; i < 5; i++) bubble(300 + Math.random() * 400, { at: i * 0.045, vol: 0.08, up: 2.4, dur: 0.06 }); grain({ dur: 0.25, f: 1200, to: 500, q: 0.6, vol: 0.04, attack: 0.02, type: "lowpass" }); },
    // Fleet, hit: a soft "poof" and a boop that sinks a little.
    hit: () => { grain({ dur: 0.18, f: 900, to: 380, q: 0.6, vol: 0.13, attack: 0.005, type: "lowpass" }); boop(NOTE.E5, { at: 0.02, dur: 0.16, vol: 0.08, end: 0.6 }); bubble(250, { at: 0.05, vol: 0.08, up: 2 }); },
    // Fleet, sunk: the poof, bubbles rising and a gentle line going down.
    sink: () => { SFX.hit(); for (let i = 0; i < 6; i++) bubble(220 + Math.random() * 300, { at: 0.12 + i * 0.06, vol: 0.06, up: 2.6, dur: 0.07 }); [NOTE.G5, NOTE.E5, NOTE.C5, NOTE.G4].forEach((f, i) => marimba(f, { at: 0.18 + i * 0.1, dur: 0.35, vol: 0.06 })); },
    // Captured (parchís) or a bad square (la oca): a cute "nom".
    eat: () => { boop(NOTE.A5, { dur: 0.06, vol: 0.08, end: 0.7 }); boop(NOTE.D5, { at: 0.08, dur: 0.1, vol: 0.08, end: 0.6 }); bubble(300, { at: 0.16, vol: 0.08, up: 0.5 }); },
    // A pair found, a round won: a marimba third and a sparkle.
    reveal: () => { marimba(NOTE.E5, { dur: 0.35, vol: 0.08 }); marimba(NOTE.G5, { at: 0.09, dur: 0.5, vol: 0.08 }); bell(NOTE.C6 * 2, { at: 0.12, dur: 0.6, vol: 0.02, send: 0.5 }); },
    // No pair: a soft "uh-uh".
    miss: () => { boop(NOTE.D5, { dur: 0.1, vol: 0.07 }); boop(NOTE.A4, { at: 0.13, dur: 0.16, vol: 0.07, end: 0.94 }); },
    // Memory: a card turned over.
    flip: () => { grain({ dur: 0.04, f: 2500, to: 1500, q: 0.8, vol: 0.07, attack: 0.003 }); bubble(500, { at: 0.02, vol: 0.06, up: 1.5, dur: 0.05 }); },
    // A mark or a piece put down (3 en raya, a pawn out of home): a marimba note and a pop.
    place: () => { marimba(pick(0, 5), { dur: 0.3, vol: 0.09 }); bubble(350, { vol: 0.06, up: 1.8, dur: 0.05 }); },
    // Dominó: a wooden tile laid on the table, a little clack.
    tile: () => { marimba(pick(0, 4) / 2, { dur: 0.18, vol: 0.11, send: 0.1 }); grain({ dur: 0.015, f: 1800, q: 1, vol: 0.1, attack: 0.001 }); marimba(pick(2, 6) / 2, { at: 0.035, dur: 0.12, vol: 0.05, send: 0 }); },
    // Dominó: a tile slid off the stock.
    "tile-draw": () => { grain({ dur: 0.15, f: 700, to: 1400, q: 0.8, vol: 0.05, attack: 0.1 }); marimba(NOTE.G4, { at: 0.15, dur: 0.15, vol: 0.06, send: 0 }); },
    // Pass, or a turn lost: one soft boop that sinks.
    pass: () => boop(NOTE.G4, { dur: 0.18, vol: 0.07, end: 0.85 }),
    // Fleet placed and ready: two bright notes up.
    ready: () => { marimba(NOTE.C5, { dur: 0.3, vol: 0.08 }); marimba(NOTE.G5, { at: 0.1, dur: 0.45, vol: 0.08 }); bubble(400, { at: 0.1, vol: 0.05, up: 1.7 }); },
    // Parchís, a pawn home: a little run up with a sparkle.
    home: () => { [NOTE.E5, NOTE.G5, NOTE.C6].forEach((f, i) => marimba(f, { at: i * 0.07, dur: 0.4, vol: 0.08 })); bubble(330, { at: 0.14, vol: 0.06, up: 1.8 }); bell(NOTE.E6, { at: 0.16, dur: 0.6, vol: 0.022, send: 0.5 }); },
    // Your turn: a friendly marimba "ding-ding" rising a fifth.
    turn: () => { marimba(NOTE.C5, { dur: 0.4, vol: 0.09 }); marimba(NOTE.G5, { at: 0.12, dur: 0.55, vol: 0.09 }); },
    // Win: a bubbly marimba run up the pentatonic scale, pops sprinkled in, a warm chord and a sparkle.
    win: () => {
      [NOTE.C5, NOTE.D5, NOTE.E5, NOTE.G5, NOTE.A5, NOTE.C6, NOTE.E6].forEach((f, i) => { marimba(f, { at: i * 0.07, dur: 0.5, vol: 0.085 }); if (i % 2) bubble(f / 2, { at: i * 0.07 + 0.03, vol: 0.05, up: 1.8 }); });
      marimba(NOTE.G6, { at: 0.56, dur: 1.1, vol: 0.07, send: 0.45 });
      pad([NOTE.C5, NOTE.E5, NOTE.G5], { at: 0.5, dur: 1.6, vol: 0.045 });
      [0.7, 0.85, 1.05].forEach((at) => bell(NOTE.C6 * 2 * (1 + Math.random() * 0.12), { at, dur: 0.6, vol: 0.016, send: 0.5 }));
    },
    // Lose: a gentle "aww" — a soft marimba line falling, and a low boop to close; sad but kind.
    lose: () => {
      [NOTE.G5, NOTE.E5, NOTE.D5, NOTE.C5].forEach((f, i) => marimba(f, { at: i * 0.16, dur: 0.55, vol: 0.075, send: 0.35 }));
      boop(NOTE.C4, { at: 0.66, dur: 0.45, vol: 0.08, end: 0.9, send: 0.3 });
      pad([NOTE.A3, NOTE.C4, NOTE.E4], { at: 0.66, dur: 1.4, vol: 0.035 });
    },
    pop: () => bubble(450, { vol: 0.11, up: 2.3 }),
    // A call that is good news for the room: three bubbly blips up.
    good: () => [NOTE.E5, NOTE.G5, NOTE.C6].forEach((f, i) => { marimba(f, { at: i * 0.08, dur: 0.35, vol: 0.06 }); if (i === 2) bubble(f / 2, { at: 0.17, vol: 0.05 }); }),
    // Bad news for someone: an "uh-oh", two soft boops falling a minor third.
    bad: () => { boop(NOTE.E5, { dur: 0.14, vol: 0.08 }); boop(NOTE.C5, { at: 0.17, dur: 0.24, vol: 0.08, end: 0.94 }); },
    // A pawn hopping square by square: each hop a bubble and a note one step higher up the scale.
    step: () => { const now = performance.now(); if (now - stepAt > 450) stepN = 0; stepAt = now; const f = PENTA[Math.min(stepN++, PENTA.length - 1)]; bubble(f / 2, { vol: 0.06, up: 1.5, dur: 0.05 }); marimba(f, { dur: 0.12, vol: 0.035, send: 0 }); },
    error: () => { boop(NOTE.D4, { dur: 0.1, vol: 0.08 }); boop(NOTE.A3, { at: 0.12, dur: 0.16, vol: 0.08, end: 0.92 }); },
  };
  // The same list, in the same order, as app.rs SOUNDS: the server sends the index.
  const SOUNDS = ["tap", "card", "draw", "dice", "drop", "splash", "hit", "sink", "eat", "reveal", "skip", "two", "turn-around", "wild", "one",
    "skip-other", "select", "return", "shuffle", "deal", "picker-select", "picker-show", "picker-hide",
    "tile", "tile-draw", "pass", "miss", "flip", "place", "ready", "home"];
  const play = (name) => { if (!muted() && SFX[name]) try { SFX[name](); } catch {} };
  addEventListener("pointerdown", () => { if (document.getElementById("gm-live")) audio(); }, { capture: true, passive: true });

  // ---------- game feel ----------
  const calm = matchMedia("(prefers-reduced-motion: reduce)");
  function shake() {
    const el = document.querySelector("#gm-live .gm-move");
    if (!el || calm.matches) return;
    el.classList.remove("gm-shake"); void el.offsetWidth; el.classList.add("gm-shake");
  }
  function streamers() {
    if (calm.matches) return;
    const box = document.createElement("div");
    box.className = "gm-confetti";
    const colors = ["#e53935", "#f2b705", "#43a047", "#1e6fd9", "#7c3aed", "#ec4899"];
    for (let i = 0; i < 16; i++) {
      const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
      s.setAttribute("viewBox", "0 0 20 120");
      s.setAttribute("class", "gm-streamer");
      s.style.left = Math.random() * 100 + "vw";
      s.innerHTML = `<path d="M10 0q9 10 0 20t0 20 0 20 0 20 0 20 0 20" fill="none" stroke="${colors[i % colors.length]}" stroke-width="4" stroke-linecap="round"/>`;
      box.append(s);
      const sway = (Math.random() - 0.5) * 160;
      s.animate(
        [
          { transform: `translate(0, -140px) rotate(${(Math.random() - 0.5) * 40}deg)` },
          { transform: `translate(${sway}px, ${innerHeight + 140}px) rotate(${(Math.random() - 0.5) * 120}deg)` },
        ],
        { duration: 2600 + Math.random() * 1600, delay: Math.random() * 500, easing: "cubic-bezier(.25,.5,.45,1)", fill: "forwards" },
      );
    }
    document.body.append(box);
    setTimeout(() => box.remove(), 5000);
  }

  function celebrate() {
    const win = document.querySelector("#gm-live .gm-win");
    if (!win) return;
    win.classList.add("show");
    streamers();
    confetti();
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
  const stepMs = (n) => Math.min(260, 2400 / n);
  const pathOf = (g) => g.dataset.steps.trim().split(" ").map((p) => p.split(",").map(Number));

  function walkTime(root) {
    if (calm.matches) return 0;
    let longest = 0;
    for (const g of root.querySelectorAll("[data-steps]")) {
      const n = pathOf(g).length - 1;
      if (n > 0) longest = Math.max(longest, stepMs(n) * n);
    }
    return longest;
  }

  const ROLL_MS = 1000;
  let rolled = false; // the view just swapped in comes from a roll of the dice
  const TOP = { 1: [90, 0], 6: [-90, 0], 2: [0, -90], 5: [0, 90], 3: [0, 0], 4: [180, 0] };

  /// The dice jump, tumble over in 3D and land with the rolled value on top.
  function rollDice() {
    const dice = [...document.querySelectorAll("#gm-live .gm-dice-bar .gm-die")];
    if (!dice.length || calm.matches) return 0;
    for (const [k, die] of dice.entries()) {
      const cube = die.querySelector(".gm-cube");
      const [a, c] = TOP[cube?.dataset.v] || [0, 0];
      const way = k % 2 ? -1 : 1;
      const at = (y, x, z, spin) => `translateY(${y}px) rotateX(-58deg) rotateY(-45deg) rotateX(${a + x}deg) rotateZ(${c + z}deg) rotateY(${spin}deg)`;
      const slot = die.closest(".gm-die-slot.used");
      slot?.classList.remove("used");
      if (slot) setTimeout(() => slot.classList.add("used"), ROLL_MS + k * 80);
      const timing = { duration: ROLL_MS, delay: k * 80, easing: "cubic-bezier(.33,.7,.4,1)", fill: "backwards" };
      cube?.animate(
        [
          { transform: at(0, way * -540, 360, way * 90) },
          { transform: at(-58, way * -200, 140, way * 40), offset: 0.42 },
          { transform: at(0, 0, 0, 0), offset: 0.74 },
          { transform: at(-9, way * 8, 0, 0), offset: 0.86 },
          { transform: at(0, 0, 0, 0) },
        ],
        timing,
      );
      die.animate(
        [
          { transform: "scale(1)", opacity: 1 },
          { transform: "scale(.55)", opacity: 0.45, offset: 0.42 },
          { transform: "scale(1)", opacity: 1, offset: 0.74 },
          { transform: "scale(.85)", opacity: 0.8, offset: 0.86 },
          { transform: "scale(1)", opacity: 1 },
        ],
        { ...timing, pseudoElement: "::after" },
      );
    }
    return ROLL_MS + (dice.length - 1) * 80;
  }

  /// The moved pawn walks its path square by square with a little hop (parchís).
  function walk(delay = 0) {
    if (calm.matches) return 0;
    let longest = 0;
    for (const g of document.querySelectorAll("#gm-live [data-steps]")) {
      const pts = pathOf(g);
      if (pts.length < 2) continue;
      const n = pts.length - 1, each = stepMs(n), frames = [];
      pts.forEach(([x, y], i) => {
        if (i > 0) {
          const [px, py] = pts[i - 1];
          frames.push({ transform: `translate(${(px + x) / 2}px, ${(py + y) / 2 - 7}px)`, offset: (i - 0.5) / n });
        }
        frames.push({ transform: `translate(${x}px, ${y}px)`, offset: i / n });
      });
      g.animate(frames, { duration: each * n, delay, easing: "linear", fill: "backwards" });
      for (let i = 1; i <= n; i++) setTimeout(() => play("step"), delay + each * i);
      longest = Math.max(longest, each * n);
    }
    return longest;
  }

  function feel(sound, flags, mineBefore) {
    const rolling = rolled ? rollDice() : 0;
    if (rolling) play("dice");
    const walking = walk(rolling);
    const wait = rolling + walking;
    if (wait) return setTimeout(() => feelNow(sound, flags, mineBefore, true), wait);
    feelNow(sound, flags, mineBefore, false);
  }

  const CALL_MS = 2400;
  function announce() {
    const c = document.querySelector("#gm-live .gm-call");
    if (!c) return;
    c.classList.add("show");
    play(c.classList.contains("bad") ? "bad" : "good");
    setTimeout(() => c.classList.remove("show"), CALL_MS);
  }

  function feelNow(sound, flags, mineBefore, walked) {
    const name = SOUNDS[sound] || "tap";
    announce();
    if (flags & FLAG.WIN) {
      play("win");
      return celebrate();
    }
    if (flags & FLAG.LOSE) {
      play("lose");
      return celebrate();
    }
    if (box()?.dataset.end) return celebrate();
    if (!(walked && name === "dice")) play(name);
    if (name === "hit" || name === "sink" || name === "eat") shake();
    const called = document.querySelector("#gm-live .gm-call") ? CALL_MS : 0;
    if (flags & FLAG.MY_TURN && (!mineBefore || called)) setTimeout(() => play("turn"), 280 + called);
  }

  // ---------- Colors: cards fly from where they come to where they go ----------
  let clicked = null; // the card I just tapped, so mine leaves from my hand
  const rect = (el) => el?.getBoundingClientRect();
  /// Animations end on the card's resting transform (its tilt on the pile), or it snaps when they end.
  const restOf = (el) => getComputedStyle(el).transform;

  function fly(el, from, { rotate = 0, delay = 0 } = {}) {
    const to = rect(el);
    if (!el || !from || !to || calm.matches) return;
    const dx = from.left + from.width / 2 - (to.left + to.width / 2), dy = from.top + from.height / 2 - (to.top + to.height / 2);
    const s = from.width / to.width || 1;
    el.animate(
      [{ transform: `translate(${dx}px, ${dy}px) scale(${s}) rotate(${rotate}deg)`, opacity: 0.9 }, { transform: restOf(el), opacity: 1 }],
      { duration: 420, delay, easing: "cubic-bezier(.2,.9,.25,1.15)", fill: "backwards" },
    );
  }

  function toss(el, from, spin) {
    const to = rect(el);
    if (!el || !from || !to || calm.matches) return;
    const dx = from.left + from.width / 2 - (to.left + to.width / 2), dy = from.top + from.height / 2 - (to.top + to.height / 2);
    const s = from.width / to.width || 1;
    const lift = Math.min(90, Math.abs(dy) * 0.45 + 30);
    el.animate(
      [
        { transform: `translate(${dx}px, ${dy}px) scale(${s}) rotate(0deg)`, offset: 0 },
        { transform: `translate(${dx * 0.45}px, ${dy * 0.45 - lift}px) scale(1.15) rotate(${spin}deg)`, offset: 0.5 },
        { transform: restOf(el), offset: 1 },
      ],
      { duration: 520, easing: "cubic-bezier(.3,.7,.3,1)", fill: "backwards" },
    );
  }
  /// Measured before the old view leaves the page: a detached element measures 0,0.
  function tablePlaces(view) {
    const at = new Map();
    const deck = view.querySelector(".gm-deck");
    if (deck) at.set(".gm-deck", rect(deck));
    for (const o of view.querySelectorAll(".gm-other[data-seat]")) {
      const backs = o.querySelector(".gm-backs");
      if (backs) at.set(`.gm-other[data-seat="${o.dataset.seat}"] .gm-backs`, rect(backs));
    }
    return at;
  }

  function cardsMoved(old, next, places) {
    const t = next.querySelector(".gm-colores");
    if (!t || !old.querySelector(".gm-colores")) return;
    const act = t.dataset.act, seat = t.dataset.seat, me = t.dataset.me;
    const was = (sel) => places.get(sel);
    if (act === "play") {
      const from = seat === me ? clicked : was(`.gm-other[data-seat="${seat}"] .gm-backs`);
      toss(next.querySelector(".gm-pile-card.top"), from || was(".gm-deck"), seat === me ? -16 : 16);
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
    if (a0 && a1 && a0 !== a1 && !calm.matches) next.querySelector(".gm-arrows")?.animate([{ rotate: "0deg" }, { rotate: a1 < 0 ? "-180deg" : "180deg" }], { duration: 600, easing: "cubic-bezier(.3,1.4,.5,1)" });
    clicked = null;
  }
  document.addEventListener("pointerdown", (e) => {
    const c = e.target.closest?.(".gm-hand .gm-card-btn");
    if (c) clicked = rect(c);
    if (c?.classList.contains("can") || e.target.closest?.("#gm-live button.gm-deck")) play("select");
  }, true);
  // The comodín's colour picker opens and closes with its own sounds.
  document.addEventListener("toggle", (e) => {
    if (e.target.matches?.("#gm-live details.gm-wild")) play(e.target.open ? "picker-show" : "picker-hide");
  }, true);

  // ---------- the live part ----------
  const box = () => document.getElementById("gm-live");
  function swap(html) {
    const old = box(); if (!old) return null;
    Colors.forget();
    const t = document.createElement("template");
    t.innerHTML = html.trim();
    const next = t.content.firstElementChild;
    if (next?.id !== "gm-live" || +next.dataset.v < +old.dataset.v) return null;
    const picked = old.querySelector("input[name=k]:checked")?.value;
    const places = tablePlaces(old);
    rolled = next.dataset.roll !== old.dataset.roll && next.dataset.roll !== "0";
    old.replaceWith(next);
    if (picked) {
      const r = next.querySelector(`input[name=k][value="${picked}"]`);
      if (r) r.checked = true;
    }
    cardsMoved(old, next, places);
    if (next.dataset.turn) document.title = document.hidden ? "• " + document.title.replace(/^• /, "") : document.title;
    init(true);
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
  function seatFace(seat) {
    const live = box();
    const chip = live?.querySelectorAll(".gm-player")[seat];
    if (!chip) return null;
    const table = live.querySelector(".gm-colores");
    const at = chip.classList.contains("me")
      ? table?.querySelector(".gm-mine .gm-face")
      : table?.querySelector(`.gm-other[data-seat="${seat}"] .gm-face`);
    return { el: at || chip.querySelector(".avatar") || chip, color: chip.style.getPropertyValue("--c") };
  }

  /// In the stage, which outlives the view swaps, so a move right after does not wipe it.
  function bubble(seat, html) {
    const stage = document.querySelector(".gm-stage");
    const face = seatFace(seat);
    if (!stage || !face) return;
    stage.querySelector(`.gm-said[data-seat="${seat}"]`)?.remove();
    const r = face.el.getBoundingClientRect(), s = stage.getBoundingClientRect();
    const b = document.createElement("div");
    b.className = "gm-said";
    b.dataset.seat = seat;
    b.style.setProperty("--c", face.color);
    b.style.left = r.left - s.left + stage.scrollLeft + r.width / 2 + "px";
    b.style.top = r.top - s.top + stage.scrollTop + "px";
    const what = document.createElement("span");
    what.className = "gm-said-what" + (html.startsWith("<") ? "" : " emoji");
    if (html.startsWith("<")) what.innerHTML = html;
    else what.textContent = html;
    b.append(what);
    stage.append(b);
    setTimeout(() => b.remove(), 3300);
  }

  // ---------- Colors draws itself: the server sends what this seat sees (STATE), the page patches it ----------
  /// My own moves show at once, before the server answers; its answer then confirms them, or
  /// puts things back if it says no. The server still decides everything and hides the others' cards.
  const Colors = (() => {
    const SKIP = 10, TURN = 11, TWO = 12, WILD = 13, ANY = 4;
    const NAMES = ["rojo", "amarillo", "verde", "azul"];
    const PALETTE = ["#ef4444", "#eab308", "#22c55e", "#3b82f6"]; // colors.rs PALETTE
    const TILT = [[-9, -6, 3], [7, 4, -2], [-4, 2, 1], [-4, 0, 0]]; // colors.rs TILT
    let base = null; // the server's latest word: { s, v }
    let pending = null; // my move on screen that the server has not confirmed yet

    const kit = (name) => document.querySelector(`#gm-kit`)?.content.querySelector(`[data-i="${name}"] svg`)?.cloneNode(true) || document.createTextNode("");
    function el(tag, cls, attrs = {}, ...kids) {
      const e = document.createElement(tag);
      if (cls) e.className = cls;
      for (const [k, v] of Object.entries(attrs)) if (v != null) e.setAttribute(k, v);
      e.append(...kids.filter((k) => k != null));
      return e;
    }
    const named = ([c, v]) => v === WILD ? "Comodín" : `${v === SKIP ? "Salta" : v === TURN ? "Gira" : v === TWO ? "+2" : v} ${NAMES[c]}`;
    function face([c, v], shown) {
      const k = v === WILD ? shown : c;
      const mid = el("span", "gm-card-mid"), tab = el("span", "gm-card-tab");
      if (v === SKIP || v === TURN) { tab.append(kit(v === SKIP ? "ban" : "refresh-cw")); mid.append(kit(v === SKIP ? "ban" : "refresh-cw")); }
      else if (v === TWO) { tab.append("+2"); mid.append(el("b", "", {}, "+2")); }
      else if (v === WILD) { tab.append("?"); mid.append(el("span", "gm-wild-face", {}, ...[0, 1, 2, 3].map((q) => el("i", `q${q}${q === shown ? " on" : ""}`)))); }
      else { tab.append(String(v)); mid.append(el("b", "", {}, String(v))); }
      return el("span", `gm-card k${k}${v === WILD ? " wild" + (shown < ANY ? " chosen" : "") : ""}`, {}, tab, mid);
    }
    const back = () => el("span", "gm-card back");
    const top = (g) => g.pile[g.pile.length - 1] || [0, 1];
    const fits = (g, [c, v]) => v === WILD || c === g.color || v === top(g)[1];
    const step = (g, from, n) => (((from + g.dir * n) % g.counts.length) + g.counts.length) % g.counts.length;
    const count = (n) => el("span", `gm-count${n === 2 ? " warn" : ""}${n === 1 ? " bad" : ""}`, {}, String(n), kit("layers"));
    /// Name, colour, robot or not and face of each seat, from the players' chips the server drew.
    function seat(i) {
      const chip = box()?.querySelectorAll(".gm-player")[i];
      return {
        name: chip?.querySelector(".gm-player-name")?.textContent || "?",
        color: chip?.style.getPropertyValue("--c") || "#64748b",
        bot: chip?.dataset.bot === "1",
        face: () => chip?.querySelector(".avatar")?.cloneNode(true) || el("span"),
      };
    }

    /// What my move does, worked out here so it shows at once (colors.rs `play`, without the luck).
    function ahead(s, mv) {
      const g = structuredClone(s.g), me = s.me;
      if (me == null || g.turn !== me) return null;
      g.skipped = null; g.victim = null;
      let said = "";
      if (mv === "draw") {
        if (g.drew) return null;
        g.hand.push(null); // which card comes is only known to the server
        g.counts[me]++; g.deck = Math.max(0, g.deck - 1); g.drew = true; g.act = [me, "draw"];
        g.waiting = true; // "Pasar" waits for the server: if the card does not fit, the turn passes by itself
        said = "roba una carta";
      } else if (mv === "pass") {
        if (!g.drew) return null;
        g.act = [me, "pass"]; g.turn = step(g, me, 1); g.drew = false;
        said = "pasa";
      } else {
        const [, i, chosen] = mv.split(":");
        const card = g.hand[+i];
        if (!card || !fits(g, card)) return null;
        const played = card[1] === WILD ? [+chosen, WILD] : card;
        g.hand.splice(+i, 1); g.counts[me]--;
        g.pile = [...g.pile, played].slice(-4); g.color = played[0]; g.act = [me, "play"]; g.drew = false;
        said = card[1] === WILD ? `echa un comodín: ${NAMES[played[0]]}` : `echa ${named(card)}`;
        if (!g.hand.length) return null; // the end comes with the server's trophy
        const two = g.counts.length === 2, next = step(g, me, 1);
        if (card[1] === SKIP) { g.skipped = next; g.turn = step(g, me, 2); }
        else if (card[1] === TURN) { g.dir = -g.dir; g.turn = two ? me : step(g, me, 1); if (two) g.skipped = next; }
        else if (card[1] === TWO) { g.skipped = next; g.victim = next; g.counts[next] += 2; g.deck = Math.max(0, g.deck - 2); g.turn = step(g, me, 2); }
        else g.turn = step(g, me, 1);
      }
      // The same last line and hint the server will send (colors.rs `play` and `hint`), so its
      // confirmation finds nothing to redraw.
      let last = [me, said];
      if (g.act[1] === "play" && g.hand.length === 1) last = [me, "¡le queda una!"];
      if (g.victim != null) last = [g.victim, "roba dos y pierde el turno"];
      const again = g.turn === me && g.act[1] === "play" && g.skipped != null;
      return { ...s, g, last, call: null, again, hint: g.turn === me ? (again ? "El otro pierde el turno: juegas otra vez" : s.hint) : "" };
    }
    const sound = (mv, s) => mv === "draw" ? "draw" : mv === "pass" ? "tap" : ({ [SKIP]: "skip-other", [TURN]: "turn-around", [TWO]: "two", [WILD]: "wild" })[s.g.hand[+mv.split(":")[1]]?.[1]] || "card";

    // ---------- drawing ----------
    function bar(s, L) {
      const g = s.g, mine = s.me != null && g.turn === s.me;
      const b = L.querySelector(".gm-bar");
      if (!b) return;
      b.classList.toggle("mine", mine);
      // Each half is rebuilt only when what it says changes: rebuilding replays its entrance, and
      // the server's confirmation of my own move usually says the same thing again.
      const turnKey = mine ? `me:${s.again}:${s.hint || ""}` : `them:${g.turn}`;
      let turn = b.querySelector(":scope > .gm-turn");
      if (turn?.dataset.k !== turnKey) {
        const next = mine
          ? el("span", "gm-turn you", {}, el("span", "gm-turn-txt", {}, el("b", "", {}, s.again ? "¡Otra vez tú!" : "¡Te toca!"), el("small", "", {}, s.hint || "")))
          : (() => { const t = seat(g.turn); return el("span", "gm-turn them", { style: `--c:${t.color}` }, t.face(),
              el("span", "gm-turn-txt", {}, el("b", "", {}, `Turno de ${t.name}`), el("small", "gm-think", {}, t.bot ? "pensando" : "esperando su jugada", el("i"), el("i"), el("i")))); })();
        next.dataset.k = turnKey;
        if (turn) turn.replaceWith(next); else b.prepend(next);
        turn = next;
      }
      const lastKey = s.last ? `${s.last[0]}:${s.last[1]}` : "";
      const last = b.querySelector(":scope > .gm-last");
      if ((last?.dataset.k ?? "") !== lastKey) {
        if (!s.last) last?.remove();
        else {
          const w = seat(s.last[0]), next = el("span", "gm-last", { style: `--c:${w.color}` }, el("i"), el("b", "", {}, w.name), ` ${s.last[1]}`);
          next.dataset.k = lastKey;
          if (last) last.replaceWith(next); else turn.after(next);
        }
      }
    }
    function extras(s, L, fresh) {
      const g = s.g, mine = s.me != null && g.turn === s.me, form = L.querySelector("form.gm-move");
      L.querySelector(".gm-call")?.remove();
      if (s.call && fresh) {
        const [who, text, good] = s.call, w = seat(who);
        form?.before(el("div", `gm-call${good ? "" : " bad"}`, { style: `--c:${w.color}`, "aria-hidden": "true" }, el("span", "gm-call-who", {}, w.face(), w.name), el("b", "gm-call-text", {}, text)));
      }
      L.querySelector(".gm-pop")?.remove();
      if (mine) {
        const cue = s.again ? "¡Otra vez tú!" : "¡Te toca!";
        form?.before(el("div", `gm-pop${s.again ? " again" : ""}`, { "aria-hidden": "true" },
          el("b", "gm-pop-title", {}, ...[...cue].map((ch, i) => el("span", "", { style: `--i:${i}` }, ch))), el("span", "gm-pop-hint", {}, s.hint || "")));
      }
    }
    function table(s, T) {
      const g = s.g, me = s.me, playing = true, mine = me != null && g.turn === me;
      const data = { act: g.act?.[1] ?? "", seat: g.act?.[0], me, victim: g.victim, dir: g.dir };
      for (const [k, v] of Object.entries(data)) if (v == null) delete T.dataset[k]; else T.dataset[k] = v;
      for (const o of T.querySelectorAll(".gm-other[data-seat]")) {
        const i = +o.dataset.seat, n = g.counts[i] ?? 0;
        o.classList.toggle("on", playing && g.turn === i);
        o.classList.toggle("skipped", g.skipped === i);
        o.querySelector(".gm-backs")?.replaceChildren(...Array.from({ length: Math.min(n, 9) }, () => el("i")));
        o.querySelector(".gm-count")?.replaceWith(count(n));
        ban(o.querySelector(".gm-face"), g.skipped === i);
      }
      T.querySelector(".gm-arrows")?.classList.toggle("back", g.dir < 0);
      const under = g.pile.slice(-4);
      morph(T.querySelector(".gm-pile"), under.map((c, i) => {
        const [r, x, y] = TILT[TILT.length - under.length + i];
        return keyed(el("span", `gm-pile-card${i + 1 === under.length ? " top" : ""}`, { style: `--r:${r}deg;--x:${x}px;--y:${y}px` }, face(c, c[0])), `${i}:${c}:${under.length}`);
      }));
      const deck = mine && !g.drew
        ? el("button", "gm-deck glow", { type: "submit", name: "m", value: "draw", "aria-label": "Robar una carta", title: "Robar" }, back(), el("span", "gm-card back b2"))
        : el("span", "gm-deck", { title: `${g.deck} en el mazo` }, back(), el("span", "gm-card back b2"));
      T.querySelector(".gm-deck")?.replaceWith(deck);
      T.querySelector(".gm-now")?.replaceChildren(
        el("span", "gm-color-pill", { style: `background:${PALETTE[g.color]}` }, `Toca ${NAMES[g.color]}`),
        ...(mine && g.drew && !g.waiting ? [el("button", "btn outline sm", { type: "submit", name: "m", value: "pass" }, "Pasar")] : mine ? [el("span", "hint", {}, "Echa una carta o roba del mazo")] : []));
      const area = T.querySelector(".gm-mine");
      if (!area || me == null) return;
      area.classList.toggle("on", mine);
      area.classList.toggle("skipped", g.skipped === me);
      const hand = area.querySelector(".gm-hand");
      hand.style.setProperty("--n", g.hand.length);
      morph(hand, g.hand.map((card, i) => {
        if (!card) return keyed(el("span", "gm-card-btn off gm-coming", {}, back()), "coming");
        const ok = mine && fits(g, card), k = `${i}:${card}:${ok}`;
        if (card[1] === WILD && ok) {
          return keyed(el("details", "gm-wild", {}, el("summary", "gm-card-btn can", { "aria-label": "Echar el comodín" }, face(card, ANY)),
            el("div", "gm-picker", { role: "dialog", "aria-label": "Elige un color" }, el("div", "gm-diamond", {},
              ...[0, 1, 2, 3].map((c) => el("button", `q q${c}`, { type: "submit", name: "m", value: `play:${i}:${c}`, "aria-label": `Comodín: ${NAMES[c]}` }))))), k);
        }
        return keyed(ok
          ? el("button", "gm-card-btn can", { type: "submit", name: "m", value: `play:${i}`, "aria-label": `Echar ${named(card)}` }, face(card, card[0]))
          : el("span", "gm-card-btn off", {}, face(card, card[0])), k);
      }));
      area.querySelector(".gm-me-who .gm-count")?.replaceWith(count(g.counts[me] ?? 0));
      ban(area.querySelector(".gm-me-who .gm-face"), g.skipped === me);
    }
    const keyed = (e, k) => { e.dataset.k = k; return e; };
    /// Keeps the cards that did not change, so a card in flight is not cut short when the server
    /// confirms my move; the rest are swapped, taking over any flight still going on.
    function morph(into, kids) {
      if (!into) return;
      const old = [...into.children];
      kids.forEach((k, i) => {
        const o = old[i];
        if (o && o.dataset.k === k.dataset.k) return;
        if (!o) return into.append(k);
        for (const a of o.getAnimations({ subtree: true }).filter((a) => a.effect?.target === o)) {
          const t = k.animate(a.effect.getKeyframes(), a.effect.getTiming());
          t.currentTime = a.currentTime;
        }
        o.replaceWith(k);
        // The card I drew arrives face down and turns over once the server says which it is.
        if (o.dataset.k === "coming" && !calm.matches) k.querySelector(".gm-card")?.animate([{ transform: "rotateY(90deg)" }, { transform: "none" }], { duration: 240, delay: 120, easing: "cubic-bezier(.2,.8,.3,1)", fill: "backwards" });
      });
      old.slice(kids.length).forEach((o) => o.remove());
    }
    function ban(at, on) {
      if (!at) return;
      const b = at.querySelector(".gm-ban");
      if (on && !b) at.append(el("span", "gm-ban", {}, kit("ban")));
      if (!on) b?.remove();
    }

    /// Patches the page to `s`, flying the cards that moved unless `quiet`.
    function draw(s, v, { quiet = false, fresh = true } = {}) {
      const L = box(), T = L?.querySelector(".gm-colores");
      if (!L || !T) return false;
      const was = { deck: rect(T.querySelector(".gm-deck")), backs: new Map([...T.querySelectorAll(".gm-other[data-seat]")].map((o) => [o.dataset.seat, rect(o.querySelector(".gm-backs"))])) };
      const dirBefore = T.dataset.dir, mine = s.me != null && s.g.turn === s.me;
      L.dataset.v = v;
      if (mine) L.dataset.turn = "1"; else delete L.dataset.turn;
      if (s.again) L.dataset.again = "1"; else delete L.dataset.again;
      L.querySelectorAll(".gm-player").forEach((p, i) => p.classList.toggle("on", s.g.turn === i));
      bar(s, L);
      extras(s, L, fresh);
      table(s, T);
      if (!quiet && !calm.matches) {
        const [who, act] = s.g.act || [];
        const me = String(s.me);
        if (act === "play") toss(T.querySelector(".gm-pile-card.top"), String(who) === me ? clicked : was.backs.get(String(who)) || was.deck, String(who) === me ? -16 : 16);
        else if (act === "draw") {
          if (String(who) === me) fly(T.querySelector(".gm-hand > :last-child"), was.deck, { rotate: 10 });
          else fly(T.querySelector(`.gm-other[data-seat="${who}"] .gm-backs i:last-child`), was.deck, { rotate: -10 });
        }
        if (s.g.victim != null) {
          const target = String(s.g.victim) === me ? T.querySelectorAll(".gm-hand > *") : T.querySelectorAll(`.gm-other[data-seat="${s.g.victim}"] .gm-backs i`);
          [...target].slice(-2).forEach((e, i) => fly(e, was.deck, { rotate: 8, delay: 260 + i * 140 }));
        }
        if (dirBefore && dirBefore !== String(s.g.dir)) T.querySelector(".gm-arrows")?.animate([{ rotate: "0deg" }, { rotate: s.g.dir < 0 ? "-180deg" : "180deg" }], { duration: 600, easing: "cubic-bezier(.3,1.4,.5,1)" });
      }
      clicked = null;
      return true;
    }

    return {
      version: () => base?.v ?? -1,
      /// The server's word: confirms my move (no second animation), or shows what the others did.
      server(s, v, snd, flags) {
        const mineBefore = !!box()?.dataset.turn;
        const mv = pending;
        pending = null;
        const first = !base;
        const reshuffled = base && s.g.deck > base.s.g.deck + 1;
        base = { s, v };
        const own = mv && s.g.act && s.g.act[0] === s.me && s.g.act[1] === mv.kind;
        if (!draw(s, v, { quiet: own || first, fresh: !first })) return;
        init(!first);
        if (first) return;
        if (reshuffled) play("shuffle");
        if (own) {
          announce();
          if (flags & FLAG.MY_TURN && !mineBefore) setTimeout(() => play("turn"), 280);
        } else {
          // The same Salta sounds different to the one who loses the turn and to the others.
          if (SOUNDS[snd] === "skip" && s.g.skipped !== s.me) snd = SOUNDS.indexOf("skip-other");
          feel(snd, flags, mineBefore);
        }
      },
      /// My move, shown before the server answers. False when it cannot be guessed here.
      ahead(mv) {
        if (!base || pending) return false;
        const next = ahead(base.s, mv);
        if (!next) return false;
        pending = { kind: mv === "draw" ? "draw" : mv === "pass" ? "pass" : "play" };
        if (mv.split(":").length === 3) play("picker-select");
        play(sound(mv, base.s));
        draw(next, base.v, { fresh: false });
        return true;
      },
      /// The server said no: back to what it last said.
      undo() {
        if (!pending || !base) return;
        pending = null;
        play("return");
        draw(base.s, base.v, { quiet: true, fresh: false });
      },
      busy: () => !!pending,
      forget() { base = null; pending = null; },
    };
  })();

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
      s.send(new Out().packet(OP.HELLO, (w) => { w.u32(id); w.u32(v); w.u8(1); }).bytes());
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
        } else if (op === OP.STATE) {
          const match = m.u32(), version = m.u32(), sound = m.u8(), flags = m.u8(), json = m.text();
          if (+box()?.dataset.id === match && version > Colors.version()) {
            try { Colors.server(JSON.parse(json), version, sound, flags); } catch { location.reload(); }
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
          Colors.undo();
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
      if (!react && Colors.busy()) return; // one move at a time: the last one is still on its way
      if (!react && !Colors.ahead(value)) {
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
      if (next && react) {
        const seat = [...next.querySelectorAll(".gm-player")].findIndex((p) => p.classList.contains("me"));
        const tpl = next.querySelector(`.gm-emojis button[value="${CSS.escape(value)}"]`);
        if (seat >= 0) bubble(seat, tpl ? tpl.innerHTML : value);
      } else if (next) feel(SOUNDS.indexOf(next.dataset.sfx), flagsOf(next), mine);
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
    if (e.target.closest?.(".gm-win-close")) e.target.closest(".gm-win").classList.remove("show");
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
  let poppedAt = null; // version that showed it: playing again after a +2 shows it once more
  function announceTurn(fresh) {
    const el = box();
    if (!el || el.dataset.live !== "1") return;
    if (!el.dataset.turn) {
      if (poppedFor === el.dataset.id) poppedFor = null;
      return;
    }
    const again = el.dataset.again && poppedAt !== el.dataset.v;
    if (poppedFor === el.dataset.id && !again) return;
    poppedFor = el.dataset.id;
    poppedAt = el.dataset.v;
    const pop = el.querySelector(".gm-pop");
    if (!pop) return;
    const roll = rolled && el.querySelector(".gm-dice-bar .gm-die") ? ROLL_MS : 0;
    const wait = fresh ? roll + walkTime(el) + (el.querySelector(".gm-call") ? CALL_MS : 0) : 0;
    setTimeout(() => {
      pop.classList.add("show");
      setTimeout(() => pop.classList.remove("show"), calm.matches ? 1400 : 1900);
    }, wait);
  }

  function init(fresh = false) {
    announceTurn(fresh);
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
