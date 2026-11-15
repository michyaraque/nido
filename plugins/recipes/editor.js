/* Recetas · block editor (loaded only on the Recetas page).
   Turns textarea[data-blocks] into Notion-style lines: Enter splits, Backspace merges, "/" opens the
   block types, "## " starts a section, "> " a tip, Alt+↑/↓ or the handle reorders, a pasted list
   becomes one block per line. The textarea keeps the value, so without JS the form still works. */
(() => {
  const SECTION = "## ", TIP = "> ";
  const TYPES = {
    ing: [["", "Ingrediente", "«200 g de harina»"], ["h", "Sección", "«Para la salsa»"]],
    steps: [["", "Paso", "Lo que hay que hacer"], ["h", "Sección", "Un título entre pasos"], ["tip", "Truco", "Un consejo destacado"]],
  };
  const HINT = { ing: "Ingrediente, o / para más", steps: "Paso, o / para más", h: "Título de la sección", tip: "Un truco de la casa" };
  const ADD = { ing: "Añadir ingrediente", steps: "Añadir paso" };
  const fold = (s) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
  const unmark = (s) => s.trim().replace(/^([-•*·]|\d+[.)])\s+/, "");

  function caret(el) {
    const s = getSelection();
    if (!s.rangeCount || !el.contains(s.anchorNode)) return 0;
    const r = s.getRangeAt(0), pre = r.cloneRange();
    pre.selectNodeContents(el);
    pre.setEnd(r.startContainer, r.startOffset);
    return pre.toString().length;
  }

  function place(el, at) {
    el.focus();
    const r = document.createRange(), w = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    let n, left = at;
    while ((n = w.nextNode())) {
      if (left <= n.length) break;
      left -= n.length;
    }
    if (n) r.setStart(n, left);
    else r.setStart(el, el.childNodes.length);
    r.collapse(true);
    const s = getSelection();
    s.removeAllRanges();
    s.addRange(r);
  }

  /// Is the caret on the first (dir < 0) or last (dir > 0) visual line of the block?
  function atEdge(el, dir) {
    const s = getSelection();
    if (!s.rangeCount) return true;
    const r = s.getRangeAt(0).getBoundingClientRect(), box = el.getBoundingClientRect();
    if (!r.height) return true;
    const lh = parseFloat(getComputedStyle(el).lineHeight) || 22;
    return dir < 0 ? r.top - box.top < lh * 0.8 : box.bottom - r.bottom < lh * 0.8;
  }

  function upgrade(ta) {
    ta.classList.add("rc-up");
    const kind = ta.dataset.blocks;
    const list = document.createElement("div");
    list.className = "rc-blocks rc-blocks-" + kind;
    list.setAttribute("role", "list");
    list.setAttribute("aria-label", ta.getAttribute("aria-label") || "");
    const add = document.createElement("button");
    add.type = "button";
    add.className = "rc-add";
    add.textContent = "+ " + ADD[kind];
    ta.hidden = true;
    ta.after(list, add);

    const typeOf = (l) => (l.startsWith(SECTION) ? ["h", l.slice(3)] : kind === "steps" && l.startsWith(TIP) ? ["tip", l.slice(2)] : ["", l]);
    const save = () => {
      ta.value = [...list.children]
        .map((b) => {
          const t = b.line.textContent.replace(/\s+/g, " ").trim();
          return !t ? "" : b.dataset.t === "h" ? SECTION + t : b.dataset.t === "tip" ? TIP + t : t;
        })
        .filter(Boolean)
        .join("\n");
    };
    const retype = (b, t) => {
      b.dataset.t = t;
      b.line.dataset.ph = HINT[t || kind];
    };
    const block = (t, text) => {
      const b = document.createElement("div");
      b.className = "rc-block";
      b.setAttribute("role", "listitem");
      b.innerHTML = '<span class="rc-grip" title="Arrastra para mover" aria-hidden="true"></span><div class="rc-line" contenteditable="true" spellcheck="true"></div>';
      b.line = b.lastChild;
      // plaintext-only where supported: no stray formatting from the keyboard or the clipboard.
      try { b.line.contentEditable = "plaintext-only"; } catch {}
      b.line.textContent = text;
      retype(b, t);
      return b;
    };
    const rows = ta.value.split("\n").map((s) => s.trim()).filter(Boolean);
    for (const r of rows.length ? rows : [""]) list.append(block(...typeOf(r)));

    // ---- "/" menu ----
    let menu = null;
    const closeMenu = () => {
      menu?.el.remove();
      menu = null;
    };
    const drawMenu = () => {
      menu.el.innerHTML = menu.items
        .map(([t, name, desc], i) => `<button type="button" role="option" class="rc-opt${i === menu.i ? " on" : ""}" data-t="${t}" aria-selected="${i === menu.i}"><span class="rc-opt-ico" data-k="${t || kind}"></span><span><b>${name}</b><small>${desc}</small></span></button>`)
        .join("");
    };
    const openMenu = (b, q) => {
      const items = TYPES[kind].filter(([, name]) => fold(name).startsWith(fold(q)));
      if (!items.length) return closeMenu();
      if (!menu || menu.b !== b) {
        closeMenu();
        const el = document.createElement("div");
        el.className = "rc-menu";
        el.setAttribute("role", "listbox");
        b.append(el);
        menu = { el, b, i: 0 };
      }
      menu.items = items;
      menu.i = Math.min(menu.i, items.length - 1);
      drawMenu();
    };
    const pick = (t) => {
      const b = menu.b;
      closeMenu();
      retype(b, t);
      b.line.replaceChildren();
      place(b.line, 0);
      save();
    };
    list.addEventListener("mousedown", (e) => {
      const o = e.target.closest(".rc-opt");
      if (!o) return;
      e.preventDefault(); // keep the caret in the line
      pick(o.dataset.t);
    });

    // ---- typing ----
    list.addEventListener("input", (e) => {
      const b = e.target.closest(".rc-block");
      if (!b) return;
      const l = b.line;
      if (!l.textContent) l.replaceChildren(); // a lone <br> would hide the placeholder
      const x = l.textContent;
      const md = /^#{1,2} /.test(x) ? "h" : kind === "steps" && x.startsWith(TIP) ? "tip" : null;
      if (md && !b.dataset.t) {
        retype(b, md);
        l.textContent = x.replace(/^(#{1,2}|>) /, "");
        place(l, 0);
      }
      if (x.startsWith("/") && !x.includes(" ")) openMenu(b, x.slice(1));
      else closeMenu();
      save();
    });

    list.addEventListener("keydown", (e) => {
      const b = e.target.closest(".rc-block");
      if (!b || e.isComposing) return;
      const l = b.line, sel = getSelection();
      if (menu) {
        if (e.key === "ArrowDown" || e.key === "ArrowUp") {
          e.preventDefault();
          menu.i = (menu.i + (e.key === "ArrowDown" ? 1 : menu.items.length - 1)) % menu.items.length;
          return drawMenu();
        }
        if (e.key === "Enter" || e.key === "Tab") {
          e.preventDefault();
          return pick(menu.items[menu.i][0]);
        }
        if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          return closeMenu();
        }
      }
      const at = caret(l), len = l.textContent.length;
      if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
        e.preventDefault();
        const o = e.key === "ArrowUp" ? b.previousElementSibling : b.nextElementSibling;
        if (o) {
          e.key === "ArrowUp" ? o.before(b) : o.after(b);
          place(l, at);
          save();
        }
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        if (!len && b.dataset.t) return retype(b, ""), save(); // Enter on an empty section/tip: back to a plain line
        const x = l.textContent;
        l.textContent = x.slice(0, at);
        const n = block("", x.slice(at));
        b.after(n);
        place(n.line, 0);
        return save();
      }
      if (e.key === "Backspace" && at === 0 && sel.isCollapsed) {
        if (b.dataset.t) {
          e.preventDefault();
          return retype(b, ""), save();
        }
        const p = b.previousElementSibling;
        if (!p) return;
        e.preventDefault();
        const join = p.line.textContent.length;
        p.line.textContent += l.textContent;
        b.remove();
        place(p.line, join);
        return save();
      }
      if (e.key === "Delete" && at === len && sel.isCollapsed) {
        const n = b.nextElementSibling;
        if (!n) return;
        e.preventDefault();
        l.textContent += n.line.textContent;
        n.remove();
        place(l, len);
        return save();
      }
      if ((e.key === "ArrowUp" || e.key === "ArrowDown") && !e.shiftKey && sel.isCollapsed) {
        const up = e.key === "ArrowUp", o = up ? b.previousElementSibling : b.nextElementSibling;
        if (o && atEdge(l, up ? -1 : 1)) {
          e.preventDefault();
          place(o.line, up ? o.line.textContent.length : Math.min(at, o.line.textContent.length));
        }
      }
    });

    list.addEventListener("paste", (e) => {
      const b = e.target.closest(".rc-block");
      if (!b) return;
      e.preventDefault();
      const rows = (e.clipboardData?.getData("text/plain") || "").split(/\r?\n/).map(unmark).filter(Boolean);
      if (!rows.length) return;
      const sel = getSelection();
      if (!sel.isCollapsed) sel.deleteFromDocument();
      const l = b.line, at = caret(l), x = l.textContent, tail = x.slice(at);
      l.textContent = x.slice(0, at) + rows[0];
      let last = b, end = l.textContent.length;
      for (const r of rows.slice(1)) {
        const [t, v] = typeOf(r);
        const n = block(t, v);
        last.after(n);
        last = n;
        end = v.length;
      }
      last.line.textContent += tail;
      place(last.line, end);
      save();
    });

    list.addEventListener("focusout", (e) => {
      if (menu && !menu.b.contains(e.relatedTarget)) closeMenu();
    });

    // ---- drag to reorder (mouse and touch) ----
    list.addEventListener("pointerdown", (e) => {
      const g = e.target.closest(".rc-grip");
      if (!g) return;
      e.preventDefault();
      const b = g.parentElement;
      b.classList.add("drag");
      const id = e.pointerId;
      const move = (ev) => {
        if (ev.pointerId !== id) return;
        for (const o of list.children) {
          if (o === b) continue;
          const r = o.getBoundingClientRect();
          if (ev.clientY < r.top || ev.clientY > r.bottom) continue;
          if (ev.clientY < r.top + r.height / 2) {
            if (o.previousElementSibling !== b) o.before(b);
          } else if (o.nextElementSibling !== b) o.after(b);
          break;
        }
      };
      const end = (ev) => {
        if (ev.pointerId !== id) return;
        b.classList.remove("drag");
        removeEventListener("pointermove", move);
        removeEventListener("pointerup", end);
        removeEventListener("pointercancel", end);
        save();
      };
      addEventListener("pointermove", move);
      addEventListener("pointerup", end);
      addEventListener("pointercancel", end);
    });

    add.addEventListener("click", () => {
      const last = list.lastElementChild;
      if (last && !last.line.textContent) return place(last.line, 0);
      const n = block("", "");
      list.append(n);
      place(n.line, 0);
    });
  }

  function init(root) {
    root.querySelectorAll("textarea[data-blocks]:not(.rc-up)").forEach(upgrade);
  }

  // Once per page load (the script itself loads once): save shortcut and cover preview.
  document.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
      const f = document.querySelector("form.rc-editor");
      if (f) {
        e.preventDefault();
        f.requestSubmit();
      }
    }
  });
  document.addEventListener("change", (e) => {
    const input = e.target;
    if (!input.matches?.(".rc-editor input[name=photo]") || !input.files?.[0]) return;
    const cover = input.closest(".rc-ed-cover");
    const img = Object.assign(document.createElement("img"), { className: "rc-cover", alt: "", src: URL.createObjectURL(input.files[0]) });
    cover.querySelector(".rc-cover").replaceWith(img);
  });

  // ---- search as you type: no request, no page swap ----
  const keyOf = (s) => fold(s).replace(/[^\p{L}\p{N}]+/gu, " ").trim();
  let urlTimer = 0;
  const calm = matchMedia("(prefers-reduced-motion: reduce)");
  const moving = new Set();
  const EASE = "cubic-bezier(.22,1,.36,1)";

  /// FLIP: remember where the cards on screen are, change the list, then slide each card from its old
  /// place to the new one. Only transform/opacity (the GPU does it), only cards in view (max 24),
  /// and a new keystroke cancels what is still moving instead of piling up.
  function glide(cards, change) {
    if (calm.matches) return change();
    for (const c of moving) c.getAnimations().forEach((a) => a.cancel());
    moving.clear();
    const top = -80, bottom = innerHeight + 80;
    const before = new Map();
    for (const c of cards) {
      if (c.hidden) continue;
      const r = c.getBoundingClientRect();
      if (r.bottom > top && r.top < bottom) before.set(c, r);
    }
    change();
    let n = 0;
    for (const c of cards) {
      if (c.hidden) continue;
      const r = c.getBoundingClientRect();
      if (r.bottom < top || r.top > bottom) continue;
      if (++n > 24) break;
      const was = before.get(c);
      if (!was) {
        c.animate([{ opacity: 0, transform: "scale(.97)" }, { opacity: 1, transform: "none" }], { duration: 180, easing: EASE });
      } else if (was.left !== r.left || was.top !== r.top) {
        c.animate([{ transform: `translate(${was.left - r.left}px,${was.top - r.top}px)` }, { transform: "none" }], { duration: 200, easing: EASE });
      } else continue;
      moving.add(c);
    }
  }

  function search(input) {
    const words = keyOf(input.value).split(" ").filter(Boolean);
    const grid = document.querySelector(".rc-grid");
    if (!grid) return;
    // Scrolled down while typing: the page keeps its height, so nothing jumps under the finger.
    if (scrollY > 0 && !grid.style.minHeight) grid.style.minHeight = grid.offsetHeight + "px";
    const cards = [...grid.children];
    let shown = 0;
    glide(cards, () => {
      for (const card of cards) {
        const ok = words.every((w) => card.dataset.k.includes(w)); // same rule as `matches` in recipes.rs
        if (card.hidden === ok) card.hidden = !ok;
        shown += ok;
      }
    });
    document.querySelector(".rc-none")?.toggleAttribute("hidden", shown > 0);
    // The address and the category chips keep the search; Safari dislikes many replaceState calls, so later.
    clearTimeout(urlTimer);
    urlTimer = setTimeout(() => {
      const q = input.value.trim();
      const withQ = (href) => {
        const u = new URL(href, location.href);
        q ? u.searchParams.set("q", q) : u.searchParams.delete("q");
        return u.pathname + u.search;
      };
      history.replaceState(history.state, "", withQ(location.href));
      for (const a of document.querySelectorAll(".rc-cat")) a.setAttribute("href", withQ(a.href));
    }, 300);
  }
  document.addEventListener("input", (e) => {
    if (e.target.matches?.(".rc-search input[name=q]")) search(e.target);
  });
  document.addEventListener("focusout", (e) => {
    if (!e.target.matches?.(".rc-search input[name=q]")) return;
    const grid = document.querySelector(".rc-grid");
    if (grid) grid.style.minHeight = ""; // done typing: the page can take its real height again
  });
  document.addEventListener("submit", (e) => {
    const input = e.target.matches?.(".rc-search") && e.target.querySelector("input[name=q]");
    if (!input) return;
    e.preventDefault(); // Enter just closes the phone keyboard: the results are already there
    input.blur();
    search(input);
  });

  (window.nidoPlugins ||= {}).recipes = init;
  init(document);
})();
