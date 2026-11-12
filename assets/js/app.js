document.documentElement.classList.add("js");

/* ---------- in-place navigation ---------- */

const loading = (on) => document.documentElement.classList.toggle("loading", on);

function toast(message, error = false) {
  document.querySelector(".toast")?.remove();
  const t = document.createElement("div");
  t.className = "toast" + (error ? " error" : "");
  t.setAttribute("role", error ? "alert" : "status");
  t.textContent = message;
  document.body.append(t);
}

const OFFLINE = Symbol("offline");

const tabOf = (u) => new URL(u, location.href).searchParams.get("tab") || "home";

const early = new Map();
function prefetch(url) {
  if (early.has(url)) return;
  early.set(url, fetch(url, { credentials: "same-origin" }).catch(() => null));
  setTimeout(() => early.delete(url), 8000);
}

const calm = matchMedia("(prefers-reduced-motion: reduce)");

async function visit(url, init = {}, { keepScroll = false, push = true, y = null } = {}) {
  loading(true);
  try {
    let res = !init.method && (await early.get(url));
    early.delete(url);
    if (!res || !res.ok) res = await fetch(url, { ...init, credentials: "same-origin" });
    if (!res.ok) {
      toast((await res.text()).slice(0, 200) || "No se ha podido completar. Inténtalo de nuevo.", true);
      document.querySelectorAll("button[disabled]").forEach((b) => (b.disabled = false));
      return;
    }
    const final = new URL(res.url);
    if (final.pathname === "/login" && location.pathname !== "/login") {
      location.href = final.href;
      return;
    }
    const doc = new DOMParser().parseFromString(await res.text(), "text/html");
    const before = scrollY;
    // The flash code is for this view only; reloading should not show it again.
    final.searchParams.delete("ok");
    const hash = new URL(url, location.href).hash;
    const next = final.pathname + final.search + (hash && !keepScroll ? hash : "");
    if (push) {
      history.replaceState({ y: before }, "");
      history.pushState({}, "", next);
    } else history.replaceState(history.state, "", next);
    const swap = () => {
      document.title = doc.title;
      document.body.className = doc.body.className;
      document.body.replaceChildren(...[...doc.body.childNodes].map((n) => document.importNode(n, true)));
      const target = hash && !keepScroll && document.querySelector(hash);
      if (target) target.scrollIntoView();
      else scrollTo(0, y ?? (keepScroll ? before : 0));
    };
    if (document.startViewTransition && !calm.matches) await document.startViewTransition(swap).updateCallbackDone;
    else swap();
    ready();
  } catch {
    if (init.method === "POST") return OFFLINE;
    location.href = url;
  } finally {
    loading(false);
  }
}

const internal = (a) =>
  a.origin === location.origin && !a.target && !a.hasAttribute("download") &&
  !(a.pathname === location.pathname && a.search === location.search && a.hash);

const linkOf = (e) => {
  const a = e.target.closest?.("a[href]");
  return a && internal(a) ? a : null;
};

document.addEventListener("pointerdown", (e) => {
  const a = !e.button && linkOf(e);
  if (a) prefetch(a.href);
}, { passive: true });

document.addEventListener("mouseover", (e) => {
  const a = linkOf(e);
  if (a?.closest("nav")) prefetch(a.href);
}, { passive: true });

document.addEventListener("click", (e) => {
  const a = e.target.closest("a[href]");
  if (!a || e.defaultPrevented || e.button || e.metaKey || e.ctrlKey || e.shiftKey || !internal(a)) return;
  e.preventDefault();
  visit(a.href, {}, { keepScroll: tabOf(a.href) === tabOf(location.href) && !a.hash });
});

document.addEventListener("submit", async (e) => {
  const form = e.target;
  if (e.defaultPrevented || form.method.toLowerCase() !== "post") return;
  e.preventDefault();
  const data = new FormData(form, e.submitter);
  const multipart = form.enctype === "multipart/form-data";
  form.querySelectorAll("button").forEach((b) => (b.disabled = true));
  const path = new URL(form.action).pathname;
  const sameView = !(path === "/login" || path === "/logout" || path === "/tickets");
  if (path === "/logout") self.caches?.delete("pages"); // what was saved for offline belongs to this person
  const body = multipart ? data : new URLSearchParams(data);
  const done = await visit(form.action, { method: "POST", body }, { keepScroll: sameView, push: !sameView });
  if (done === OFFLINE) later(form, data, multipart);
});

addEventListener("popstate", (e) => visit(location.href, {}, { push: false, y: e.state?.y ?? 0 }));
history.scrollRestoration = "manual";

/* ---------- menus and dialogs ---------- */

const openPopovers = () => document.querySelectorAll("details.select[open], details.menu[open]");

document.addEventListener("click", (e) => {
  for (const d of openPopovers()) if (!d.contains(e.target)) d.open = false;
});

document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  for (const d of openPopovers()) d.open = false;
  if (document.querySelector(".modal:target")) location.hash = "_";
});

/* ---------- forms that send themselves ---------- */

document.addEventListener("change", (e) => {
  const el = e.target;
  if (el.matches(".select-pop input")) el.closest("details").open = false;
  if (el.closest("form.auto-submit")) el.form.requestSubmit();
  if (el.matches(".photo-form input[type=file]") && el.files?.length) el.form.requestSubmit();
  if (el.matches("form.scan input[type=file]") && el.files?.length) {
    el.form.classList.add("busy");
    el.form.querySelector(".drop-txt b").textContent = "Subiendo el ticket…";
    el.form.requestSubmit();
  }
});

/* ---------- receipt review: product lines and their sum ---------- */

const euros = (s) => {
  const t = String(s).replace(/[^\d,.-]/g, "");
  const i = Math.max(t.lastIndexOf(","), t.lastIndexOf("."));
  const n = i >= 0 && t.length - i - 1 <= 2 ? t.slice(0, i).replace(/[.,]/g, "") + "." + t.slice(i + 1) : t.replace(/[.,]/g, "");
  return Math.round(parseFloat(n || "0") * 100) || 0;
};
const fmt = (c) => (c / 100).toLocaleString("es-ES", { minimumFractionDigits: 2, maximumFractionDigits: 2 }) + " €";

function updateSum() {
  const badge = document.querySelector(".items-sum");
  if (!badge) return;
  const prices = [...document.querySelectorAll(".items .item-price")].map((i) => euros(i.value));
  const sum = prices.reduce((a, b) => a + b, 0);
  const total = euros(document.getElementById(badge.dataset.totalOf)?.value || "");
  badge.className = "items-sum pill " + (sum === total ? "ok" : "warn");
  badge.textContent = prices.length ? (sum === total ? "Cuadra con el total" : "Suman " + fmt(sum)) : "";
}

document.addEventListener("click", (e) => {
  const del = e.target.closest(".item-del");
  if (del) {
    del.closest(".item").remove();
    updateSum();
  }
  if (e.target.closest(".item-add")) {
    const row = document.getElementById("item-template").content.firstElementChild.cloneNode(true);
    document.querySelector(".items").append(row);
    row.querySelector("input").focus();
    updateSum();
  }
});

document.addEventListener("input", (e) => {
  if (e.target.matches(".item-price, #tt")) updateSum();
});

/* ---------- away from home: changes wait on the phone until the Pi answers again ---------- */

const idb = new Promise((ok, ko) => {
  const r = indexedDB.open("nido", 1);
  r.onupgradeneeded = () => r.result.createObjectStore("outbox", { autoIncrement: true });
  r.onsuccess = () => ok(r.result);
  r.onerror = () => ko(r.error);
});
const outbox = (mode, fn) =>
  idb.then((d) => new Promise((ok, ko) => {
    const t = d.transaction("outbox", mode);
    const r = fn(t.objectStore("outbox"));
    t.oncomplete = () => ok(r.result);
    t.onerror = () => ko(t.error);
  }));
const queued = async () => {
  const [keys, items] = await Promise.all([outbox("readonly", (s) => s.getAllKeys()), outbox("readonly", (s) => s.getAll())]);
  return keys.map((k, i) => [k, items[i]]);
};

function labelOf(form) {
  if (form.querySelector("input[type=file]")) return form.matches(".scan") ? "Foto de un ticket" : "Foto de perfil";
  const what = form.closest(".row")?.querySelector(".row-title")?.textContent ||
    form.querySelector("[name=description],[name=name],[name=names]")?.value ||
    form.querySelector("button")?.textContent || "Cambio";
  const amount = form.elements.amount?.value;
  return what.trim() + (amount ? " · " + amount + " €" : "");
}

async function later(form, data, multipart) {
  try {
    await outbox("readwrite", (s) =>
      s.add({ url: form.action, multipart, body: multipart ? [...data] : new URLSearchParams(data).toString(), label: labelOf(form) }));
  } catch {
    toast("Sin conexión y no se ha podido guardar en el móvil. Inténtalo en casa.", true);
    return;
  }
  form.querySelectorAll("button").forEach((b) => (b.disabled = false));
  const row = form.closest(".row");
  if (row) {
    row.classList.add("pending");
    // Bills and the shopping list toggle; a chore only ever gets logged again.
    if (form.querySelector("button.check")) row.classList.toggle("done", row.matches(".chore-row") || !row.classList.contains("done"));
  } else form.reset();
  form.classList.remove("busy");
  form.closest("details")?.removeAttribute("open");
  if (document.querySelector(".modal:target")) location.hash = "_";
  toast("Sin conexión: se enviará al volver a casa");
  offline = true;
  status();
}

let offline = false, sending = false;

async function send() {
  if (sending) return;
  sending = true;
  let sent = 0;
  const failed = [];
  try {
    for (const [k, it] of await queued()) {
      let body;
      if (it.multipart) {
        body = new FormData();
        for (const [name, value] of it.body) body.append(name, value);
      } else body = new URLSearchParams(it.body);
      let res;
      try {
        res = await fetch(it.url, { method: "POST", body, credentials: "same-origin" });
      } catch {
        break;
      }
      if (new URL(res.url).pathname === "/login") break; // signed out: keep them until signing in again
      if (!res.ok) failed.push(it.label);
      await outbox("readwrite", (s) => s.delete(k));
      sent++;
    }
  } finally {
    sending = false;
  }
  if (!sent) return;
  await visit(location.href, {}, { keepScroll: true, push: false });
  if (failed.length) toast("No se pudo enviar: " + failed.join(", "), true);
  else toast(sent === 1 ? "Enviado el cambio hecho sin conexión" : `Enviados ${sent} cambios hechos sin conexión`);
}

const CLOUD_OFF = '<svg class="i" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m2 2 20 20"/><path d="M5.782 5.782A7 7 0 0 0 9 19h8.5a4.5 4.5 0 0 0 1.307-.193"/><path d="M21.532 16.5A4.5 4.5 0 0 0 17.5 10h-1.79A7.008 7.008 0 0 0 10 5.07"/></svg>';

async function status() {
  let items = [];
  try { items = await queued(); } catch {}
  document.querySelector(".offline-bar")?.remove();
  if (!offline && !items.length) return;
  const bar = document.createElement("details");
  bar.className = "offline-bar";
  const s = document.createElement("summary");
  s.innerHTML = CLOUD_OFF;
  s.append(offline ? "Sin conexión con casa · ves lo último guardado" : "Cambios pendientes de enviar");
  if (items.length) {
    const n = document.createElement("b");
    n.textContent = items.length === 1 ? "1 cambio por enviar" : items.length + " cambios por enviar";
    s.append(n);
    const ul = document.createElement("ul");
    for (const [, it] of items) ul.append(Object.assign(document.createElement("li"), { textContent: it.label }));
    bar.append(s, ul);
  } else bar.append(s);
  document.body.prepend(bar);
}

let warmed = false;
async function sync() {
  try {
    offline = !(await fetch("/ping", { cache: "no-store" })).ok;
  } catch {
    offline = true;
  }
  document.documentElement.classList.toggle("offline", offline);
  status();
  if (offline) return;
  await send();
  if (!warmed && navigator.serviceWorker?.controller) {
    warmed = true;
    for (const t of ["home", "house", "expenses", "shopping", "tasks", "settings"]) fetch("/?tab=" + t).catch(() => {});
  }
}

if ("serviceWorker" in navigator && isSecureContext) navigator.serviceWorker.register("/sw.js").catch(() => {});
addEventListener("online", sync);
document.addEventListener("visibilitychange", () => document.visibilityState === "visible" && sync());

/* ---------- after every page (first load or in-place update) ---------- */

let refreshTimer;
function ready() {
  updateSum();
  sync();
  clearTimeout(refreshTimer);
  const reading = document.querySelector("[data-refresh]");
  if (reading) refreshTimer = setTimeout(() => visit(location.href, {}, { keepScroll: true }), reading.dataset.refresh * 1000);
}
ready();
