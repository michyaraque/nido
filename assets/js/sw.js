const V = "__VERSION__";
const PAGES = "pages";
const FILES = "files-" + V;

self.addEventListener("install", (e) => {
  e.waitUntil(
    caches.open(FILES).then((c) =>
      c.addAll(["/style.css?v=" + V, "/app.js?v=" + V, "/fonts/inter.woff2", "/fonts/bricolage.woff2", "/icon.png"]),
    ),
  );
  self.skipWaiting();
});

self.addEventListener("activate", (e) => {
  e.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== PAGES && k !== FILES).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  );
});

const FILE = /^\/(style\.css|app\.js|fonts\/|icon\.png|manifest|logos\/|avatars\/|tickets\/\d+\/photo)/;

self.addEventListener("fetch", (e) => {
  const url = new URL(e.request.url);
  if (e.request.method !== "GET" || url.origin !== location.origin) return;
  if (url.pathname === "/ping" || url.pathname === "/sw.js") return;
  e.respondWith(FILE.test(url.pathname) ? file(e) : page(e.request, url));
});

async function file(e) {
  const c = await caches.open(FILES);
  const hit = await c.match(e.request);
  const net = fetch(e.request).then((r) => {
    if (r.ok) c.put(e.request, r.clone());
    return r;
  });
  if (!hit) return net;
  e.waitUntil(net.catch(() => {}));
  return hit;
}

function key(url) {
  const u = new URL(url);
  u.searchParams.delete("ok");
  return u.href;
}

async function page(req, url) {
  const c = await caches.open(PAGES);
  try {
    const res = await fetch(req, { signal: AbortSignal.timeout(6000) });
    if (res.ok && !res.redirected && res.type === "basic") c.put(key(url), res.clone());
    return res;
  } catch {
    const hit = (await c.match(key(url))) || (await sameTab(c, url));
    if (hit) return hit;
    const msg = "Sin conexión con Nido, y esta página todavía no estaba guardada en el móvil.";
    return req.mode === "navigate"
      ? new Response(`<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Nido</title><p style="font:16px system-ui;padding:24px">${msg} <a href="/">Volver al inicio</a></p>`, { status: 503, headers: { "content-type": "text/html; charset=utf-8" } })
      : new Response(msg, { status: 503, headers: { "content-type": "text/plain; charset=utf-8" } });
  }
}

async function sameTab(c, url) {
  const tab = url.searchParams.get("tab") || "home";
  const keys = (await c.keys()).filter((r) => {
    const u = new URL(r.url);
    return u.pathname === url.pathname && (u.searchParams.get("tab") || "home") === tab;
  });
  return keys.length ? c.match(keys[keys.length - 1]) : undefined;
}
