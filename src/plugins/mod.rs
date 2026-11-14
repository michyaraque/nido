//! Extras: optional features that live in programs of their own, so the core stays light.
//!
//! The core only knows the catalog below (a few lines of text per extra). Installing one downloads
//! its binary from the GitHub release into `<data dir>/plugins/`; activating it starts that program
//! on 127.0.0.1 with a secret, and from then on the core:
//! - draws its section in the sidebar and puts the HTML of `GET /page` into the content area;
//! - forwards `/x/<id>/<path>` from the browser to the program as `/<path>`, after checking the
//!   session, telling it who asks in `X-Nido-User` / `X-Nido-Name`;
//! - shows its toasts (from its `/manifest`) and offers its own services, like `/shopping/many`.
//!
//! Each extra keeps its own database (`extras/<id>.db`, in backups). The protocol and a guide to write one
//! are in plugins/README.md.
use super::*;
use axum::{extract::RawQuery, http::Method, routing::any};
use std::{path::PathBuf, process::Child, time::Duration};

/// An extra the core can offer in the marketplace.
pub struct Known {
    pub id: &'static str,  // its tab, its `/x/<id>/` prefix and its settings key `plugin:<id>`
    pub bin: &'static str, // program name; release asset `<bin>-<arch>`
    pub name: &'static str,
    pub icon: &'static str,
    pub tagline: &'static str,
    pub blurb: &'static str,
    pub features: &'static [&'static str],
    pub accent: &'static str,
    pub size: &'static str,                        // download size, roughly
    pub fab: Option<(&'static str, &'static str)>, // mobile `+` on its page: (href, label)
}

pub static ALL: &[Known] = &[Known {
    id: "recipes",
    bin: "nido-recetas",
    name: "Recetas",
    icon: "chef-hat",
    tagline: "El recetario de casa",
    blurb: "Recetas con sus ingredientes y sus pasos, que se ajustan a las raciones y mandan lo que falta a la lista de la compra.",
    features: &[
        "134 recetas españolas, con fotos si las quieres",
        "Importa recetas desde el enlace de casi cualquier web",
        "Editor por bloques para escribir las vuestras",
        "Lo que falta, a Compra con un toque",
        "Cantidades que se ajustan a las raciones",
    ],
    accent: "#ea580c",
    size: "≈ 4 MB",
    fab: Some(("/?tab=recipes&new=1", "Nueva receta")),
}];

pub fn find(id: &str) -> Option<&'static Known> {
    ALL.iter().find(|k| k.id == id)
}

/// Repository whose releases carry the extras (set at build time for forks).
const REPO: &str = match option_env!("NIDO_REPO") {
    Some(r) => r,
    None => "michyaraque/nido",
};

/// Board family of this build, as release assets name it (aarch64, armv7, armv6).
const ARCH: &str = env!("NIDO_ARCH");

fn dir() -> PathBuf {
    std::env::var("NIDO_PLUGINS").map(PathBuf::from).unwrap_or_else(|_| data_dir().join("plugins"))
}

/// Where extras keep their databases: next to nido.db (writable by Nido, included in backups).
pub fn data() -> PathBuf {
    data_dir().join("extras")
}

fn bin_path(k: &Known) -> PathBuf {
    dir().join(format!("{}{}", k.bin, std::env::consts::EXE_SUFFIX))
}

pub fn installed(k: &Known) -> bool {
    bin_path(k).is_file()
}

/// Extras installed and switched on in this house, in catalog order.
pub fn enabled(c: &Connection) -> rusqlite::Result<Vec<&'static Known>> {
    let on: Vec<String> = c
        .prepare("SELECT substr(key,8) FROM settings WHERE key LIKE 'plugin:%' AND value='1'")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(ALL.iter().filter(|k| on.iter().any(|id| id == k.id) && installed(k)).collect())
}

fn set_on(c: &Connection, k: &Known, on: bool) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![format!("plugin:{}", k.id), if on { "1" } else { "0" }],
    )?;
    Ok(())
}

// ---------- running extras ----------

struct Proc {
    id: &'static str,
    child: Child,
    port: u16,
    key: String,
}

static PROCS: Mutex<Vec<Proc>> = Mutex::new(Vec::new());
/// Toasts of every extra that has run, as (code, text). Leaked once per extra: they live as long as Nido.
static FLASHES: Mutex<Vec<(&'static str, &'static str)>> = Mutex::new(Vec::new());
static LOADED: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

fn procs() -> MutexGuard<'static, Vec<Proc>> {
    PROCS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Port and secret of the running extra, starting it if needed (and if it died).
// ponytail: extras end with Nido because systemd stops the whole service; on a dev machine an
// extra can outlive a killed Nido until its port is reused.
async fn ensure(k: &'static Known) -> Option<(u16, String)> {
    {
        let mut ps = procs();
        if let Some(i) = ps.iter().position(|p| p.id == k.id) {
            if matches!(ps[i].child.try_wait(), Ok(None)) {
                return Some((ps[i].port, ps[i].key.clone()));
            }
            ps.remove(i);
        }
    }
    let port = std::net::TcpListener::bind("127.0.0.1:0").ok()?.local_addr().ok()?.port();
    let key = rand_hex(16);
    let core_db = std::env::var("NIDO_DB").unwrap_or_else(|_| "nido.db".into());
    let _ = std::fs::create_dir_all(data());
    let mut child = std::process::Command::new(bin_path(k))
        .env("NIDO_PLUGIN_ADDR", format!("127.0.0.1:{port}"))
        .env("NIDO_PLUGIN_KEY", &key)
        .env("NIDO_PLUGIN_DB", data().join(format!("{}.db", k.id)))
        .env("NIDO_CORE_DB", core_db)
        .spawn()
        .map_err(|e| eprintln!("extra {}: {e}", k.id))
        .ok()?;
    let mut ready = false;
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    if !ready {
        let _ = child.kill();
        eprintln!("extra {}: no arranca", k.id);
        return None;
    }
    let first = {
        let mut l = LOADED.lock().unwrap_or_else(|e| e.into_inner());
        let first = !l.contains(&k.id);
        if first {
            l.push(k.id);
        }
        first
    };
    if first {
        load_flashes(port, &key).await;
    }
    let mut ps = procs();
    if let Some(p) = ps.iter().find(|p| p.id == k.id) {
        let _ = child.kill(); // another request started it meanwhile: keep that one
        return Some((p.port, p.key.clone()));
    }
    ps.push(Proc { id: k.id, child, port, key: key.clone() });
    Some((port, key))
}

async fn load_flashes(port: u16, key: &str) {
    let Ok(r) = call(port, key, "GET", "/manifest", None, None, &[]).await else { return };
    let Ok(m) = serde_json::from_slice::<serde_json::Value>(&r.body) else { return };
    let mut f = FLASHES.lock().unwrap_or_else(|e| e.into_inner());
    for (code, text) in m["flashes"].as_object().into_iter().flatten() {
        if let Some(t) = text.as_str() {
            f.push((Box::leak(code.clone().into_boxed_str()), Box::leak(t.to_string().into_boxed_str())));
        }
    }
}

fn stop(id: &str) {
    let mut ps = procs();
    if let Some(i) = ps.iter().position(|p| p.id == id) {
        let mut p = ps.remove(i);
        let _ = p.child.kill();
        let _ = p.child.wait();
    }
}

/// Starts the extras the house has on, in the background, so their first page is quick.
pub fn start(db: &Db) {
    let on = enabled(&lock(db)).unwrap_or_default();
    tokio::spawn(async move {
        for k in on {
            ensure(k).await;
        }
    });
}

pub fn flash(code: &str) -> Option<&'static str> {
    FLASHES.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|f| f.0 == code).map(|f| f.1)
}

// ---------- talking to an extra: plain HTTP/1.1 on loopback ----------

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Who is asking, for the extra.
pub struct Who<'a> {
    pub id: i64,
    pub name: &'a str,
}

fn pct(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

async fn call(port: u16, key: &str, method: &str, path: &str, who: Option<&Who<'_>>, ctype: Option<&str>, body: &[u8]) -> std::io::Result<Reply> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await?;
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nX-Nido-Key: {key}\r\nContent-Length: {}\r\n", body.len());
    if let Some(w) = who {
        head += &format!("X-Nido-User: {}\r\nX-Nido-Name: {}\r\n", w.id, pct(w.name));
    }
    if let Some(ct) = ctype.filter(|c| !c.contains(['\r', '\n'])) {
        head += &format!("Content-Type: {ct}\r\n");
    }
    s.write_all(format!("{head}\r\n").as_bytes()).await?;
    s.write_all(body).await?;
    let mut buf = Vec::new();
    // Importing a recipe from another site can take a while; nothing legitimate takes two minutes.
    tokio::time::timeout(Duration::from_secs(120), s.read_to_end(&mut buf))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "el extra tarda demasiado"))??;
    parse(&buf).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "respuesta no válida del extra"))
}

fn parse(buf: &[u8]) -> Option<Reply> {
    let end = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&buf[..end]).ok()?;
    let mut lines = head.split("\r\n");
    let status = lines.next()?.split(' ').nth(1)?.parse().ok()?;
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    let mut body = buf[end + 4..].to_vec();
    if headers.iter().any(|(k, v)| k == "transfer-encoding" && v.eq_ignore_ascii_case("chunked")) {
        body = dechunk(&body)?;
    }
    Some(Reply { status, headers, body })
}

fn dechunk(mut b: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let eol = b.windows(2).position(|w| w == b"\r\n")?;
        let size = usize::from_str_radix(std::str::from_utf8(&b[..eol]).ok()?.split(';').next()?.trim(), 16).ok()?;
        if size == 0 {
            return Some(out);
        }
        out.extend_from_slice(b.get(eol + 2..eol + 2 + size)?);
        b = b.get(eol + 4 + size..)?;
    }
}

/// The extra's page for the content area, or a calm card if it does not answer.
pub async fn page(k: &'static Known, who: &Who<'_>, query: &str) -> Markup {
    let reply = match ensure(k).await {
        Some((port, key)) => call(port, &key, "GET", &format!("/page?{query}"), Some(who), None, &[]).await.ok(),
        None => None,
    };
    match reply {
        Some(r) if r.status == 200 => PreEscaped(String::from_utf8_lossy(&r.body).into_owned()),
        _ => {
            stop(k.id);
            html! {
                section.card { (empty(k.icon, &format!("{} no responde", k.name), "Recarga la página en unos segundos. Si sigue igual, desactívalo y vuelve a activarlo en Extras.", None)) }
            }
        }
    }
}

/// `/x/<id>/<path>`: what the browser asks the extra (forms, photos, its script).
async fn proxy(State(db): State<Db>, h: HeaderMap, method: Method, Path((id, rest)): Path<(String, String)>, RawQuery(q): RawQuery, body: Bytes) -> R {
    let (me, k) = {
        let c = lock(&db);
        let me = require_user!(c, h);
        match enabled(&c)?.into_iter().find(|k| k.id == id) {
            Some(k) => (me, k),
            None => return Ok(StatusCode::NOT_FOUND.into_response()),
        }
    };
    let Some((port, key)) = ensure(k).await else { return Ok((StatusCode::SERVICE_UNAVAILABLE, "El extra no responde. Inténtalo en unos segundos.").into_response()) };
    let path = format!("/{rest}{}", q.map(|q| format!("?{q}")).unwrap_or_default());
    let ctype = h.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok());
    let who = Who { id: me.id, name: &me.name };
    let r = match call(port, &key, method.as_str(), &path, Some(&who), ctype, &body).await {
        Ok(r) => r,
        Err(e) => {
            stop(k.id);
            return Ok((StatusCode::BAD_GATEWAY, format!("El extra no ha contestado: {e}")).into_response());
        }
    };
    let mut res = Response::builder().status(r.status);
    for (name, value) in &r.headers {
        if matches!(name.as_str(), "content-type" | "location" | "cache-control") {
            res = res.header(name.as_str(), value.as_str());
        }
    }
    Ok(res.body(axum::body::Body::from(r.body)).unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response()))
}

// ---------- installing ----------

/// Downloads the extra's program for this board, checks that it runs, and puts it in place.
fn download(k: &Known) -> Result<(), String> {
    let dir = dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let part = dir.join(format!("{}.part", k.bin));
    let url = format!("https://github.com/{REPO}/releases/latest/download/{}-{ARCH}", k.bin);
    let ok = std::process::Command::new("curl")
        .args(["-sfL", "--max-time", "300", "--proto", "=https", "--proto-redir", "=https", "-o"])
        .arg(&part)
        .arg(&url)
        .status()
        .map_err(|e| format!("curl: {e}"))?
        .success();
    if !ok {
        let _ = std::fs::remove_file(&part);
        return Err("No se ha podido descargar. Revisa que Nido tenga internet.".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    let runs = std::process::Command::new(&part).arg("--version").output().is_ok_and(|o| o.status.success());
    if !runs {
        let _ = std::fs::remove_file(&part);
        return Err("Lo descargado no funciona en este equipo".into());
    }
    std::fs::rename(&part, bin_path(k)).map_err(|e| e.to_string())
}

#[derive(Deserialize)]
pub struct ExtraF {
    on: Option<String>,
    month: String,
    tab: Option<String>, // where the button was: Extras or settings
}

fn back_to(c: &Connection, f: &ExtraF, ok: &str) -> R {
    back(&month(c, Some(&f.month))?, Some(f.tab.as_deref().unwrap_or("extras")), ok)
}

async fn toggle(State(db): State<Db>, h: HeaderMap, Path(id): Path<String>, F(f): F<ExtraF>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    let Some(k) = find(&id).filter(|k| installed(k)) else { return bad("Ese extra no está instalado") };
    let on = f.on.is_some();
    set_on(&c, k, on)?;
    if !on {
        stop(k.id);
    }
    back_to(&c, &f, if on { "plugin_on" } else { "plugin_off" })
}

async fn install(State(db): State<Db>, h: HeaderMap, Path(id): Path<String>, F(f): F<ExtraF>) -> R {
    {
        let c = lock(&db);
        require_user!(c, h);
    }
    let Some(k) = find(&id) else { return bad("Ese extra no existe") };
    if !installed(k) {
        if let Err(e) = tokio::task::spawn_blocking(move || download(k)).await? {
            return Ok((StatusCode::BAD_REQUEST, e).into_response());
        }
    }
    let c = lock(&db);
    set_on(&c, k, true)?;
    back_to(&c, &f, "plugin_installed")
}

async fn uninstall(State(db): State<Db>, h: HeaderMap, Path(id): Path<String>, F(f): F<ExtraF>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    let Some(k) = find(&id) else { return bad("Ese extra no existe") };
    stop(k.id);
    set_on(&c, k, false)?;
    let _ = std::fs::remove_file(bin_path(k)); // its database stays: reinstalling brings everything back
    back_to(&c, &f, "plugin_removed")
}

pub fn routes() -> Router<Db> {
    Router::new()
        .route("/plugins/{id}/toggle", post(toggle))
        .route("/plugins/{id}/install", post(install))
        .route("/plugins/{id}/uninstall", post(uninstall))
        .route("/x/{id}/{*rest}", any(proxy).layer(DefaultBodyLimit::max(25 * 1024 * 1024)))
}

// ---------- the marketplace ----------

/// Tab "extras": every extra as a card, to install, switch on or off, or remove.
pub fn marketplace(v: &View) -> Markup {
    html! {
        div.page-head {
            div {
                h1 { "Extras" }
                p { "Funciones opcionales. Se instalan aparte y, si quitas una, sus datos se guardan." }
            }
        }
        div.mk-grid {
            @for k in ALL {
                @let has = installed(k);
                @let on = v.plugins.iter().any(|p| p.id == k.id);
                section.mk-card.on[on] style={ "--c:" (k.accent) } {
                    div.mk-top {
                        span.mk-ico { (icon(k.icon)) }
                        div.mk-title { h2 { (k.name) } p { (k.tagline) } }
                        @if on { (pill("ok", Some("circle-check"), "Activado")) }
                        @else if has { (pill("", None, "Instalado")) }
                        @else { (pill("", Some("download"), k.size)) }
                    }
                    p.mk-desc { (k.blurb) }
                    ul.mk-feats { @for f in k.features { li { (icon("check")) (f) } } }
                    div.mk-foot {
                        div.mk-actions {
                            @if on {
                                a.btn.primary.sm href=(v.link(k.id)) { "Abrir" (icon("arrow-right")) }
                                form.inline method="post" action={ "/plugins/" (k.id) "/toggle" } { (v.ctx()) button.btn.ghost.sm { "Desactivar" } }
                            } @else if has {
                                form.inline method="post" action={ "/plugins/" (k.id) "/uninstall" } { (v.ctx()) button.btn.ghost.sm { "Desinstalar" } }
                                form.inline method="post" action={ "/plugins/" (k.id) "/toggle" } {
                                    (v.ctx()) input type="hidden" name="on" value="1";
                                    button.btn.primary.sm { (icon("plus")) "Activar" }
                                }
                            } @else {
                                form.inline method="post" action={ "/plugins/" (k.id) "/install" } {
                                    (v.ctx())
                                    button.btn.primary.sm { (icon("download")) "Instalar" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_replies_and_catalog() {
        let r = parse(b"HTTP/1.1 303 See Other\r\nLocation: /?tab=recipes&r=3\r\ncontent-length: 0\r\n\r\n").unwrap();
        assert_eq!((r.status, r.headers[0].1.as_str()), (303, "/?tab=recipes&r=3"));
        let r = parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nHola \r\n6\r\nmundo!\r\n0\r\n\r\n").unwrap();
        assert_eq!(r.body, b"Hola mundo!");
        assert!(parse(b"basura").is_none());
        assert_eq!(pct("Ana María"), "Ana%20Mar%C3%ADa");
        assert_eq!(tab(Some("recipes")), "recipes");

        let c = Connection::open_in_memory().unwrap();
        crate::db::migrate(&c);
        // Switched on but not installed: nothing shows up.
        c.execute("INSERT INTO settings(key,value) VALUES('plugin:recipes','1')", []).unwrap();
        std::env::set_var("NIDO_PLUGINS", std::env::temp_dir().join("nido-no-extras"));
        assert!(enabled(&c).unwrap().is_empty());
    }
}
