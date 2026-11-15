//! Updating Nido itself from Ajustes. Nido downloads the release, checks that it runs and saves a
//! copy of the database; `/opt/nido/upgrade.sh` (root, started by `nido-upgrade.path` when the
//! request file appears) swaps the binary, restarts Nido and goes back if the new one does not start.
//! Both sides append to the same log, which the update page follows.

use super::*;
use crate::plugins::{latest_tag, semver, ARCH, REPO};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

const INSTALLER: &str = "/etc/systemd/system/nido-upgrade.path";

static RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
pub struct Release {
    pub tag: String,
    pub size: u64,
    pub date: String,
}

#[derive(Clone)]
pub struct Change {
    pub kind: &'static str,
    pub scope: String,
    pub text: String,
}

/// The latest release as last asked (when, what). Renewed in the background after an hour.
static LATEST: Mutex<(Option<Instant>, Option<Release>)> = Mutex::new((None, None));
/// What changed between two versions ("v0.18.0..v0.18.2"), None while it is being fetched.
static CHANGES: Mutex<Option<HashMap<String, Option<Vec<Change>>>>> = Mutex::new(None);

fn dir() -> PathBuf {
    std::env::var("NIDO_UPDATE").map(PathBuf::from).unwrap_or_else(|_| data_dir().join("update"))
}

pub fn installer_ready() -> bool {
    std::path::Path::new(INSTALLER).exists()
}

fn curl(url: &str, api: bool) -> Option<Vec<u8>> {
    let mut c = std::process::Command::new("curl");
    c.args(["-sfL", "--max-time", "20", "--proto", "=https", "--proto-redir", "=https"]);
    if api {
        c.args(["-H", "Accept: application/vnd.github+json"]);
    }
    let out = c.arg(url).output().ok().filter(|o| o.status.success())?;
    Some(out.stdout)
}

fn fetch_latest() -> Option<Release> {
    let tag = latest_tag()?;
    let mut rel = Release { tag: tag.clone(), size: 0, date: String::new() };
    if let Some(j) = curl(&format!("https://api.github.com/repos/{REPO}/releases/tags/{tag}"), true)
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
    {
        rel.date = j["published_at"].as_str().unwrap_or("").chars().take(10).collect();
        let name = format!("nido-{ARCH}");
        rel.size = j["assets"].as_array().into_iter().flatten().find(|a| a["name"] == name.as_str()).and_then(|a| a["size"].as_u64()).unwrap_or(0);
    }
    Some(rel)
}

fn refresh() {
    let rel = fetch_latest();
    let mut l = LATEST.lock().unwrap_or_else(|e| e.into_inner());
    if rel.is_some() {
        l.1 = rel;
    }
}

/// The latest release as known now; asks GitHub again in the background once an hour.
pub fn latest() -> Option<Release> {
    let mut l = LATEST.lock().unwrap_or_else(|e| e.into_inner());
    if l.0.is_none_or(|t| t.elapsed() > Duration::from_secs(3600)) {
        l.0 = Some(Instant::now());
        std::thread::spawn(refresh);
    }
    l.1.clone()
}

/// Asks GitHub right now (the "Buscar ahora" button).
pub fn check_now() -> Option<Release> {
    LATEST.lock().unwrap_or_else(|e| e.into_inner()).0 = Some(Instant::now());
    refresh();
    LATEST.lock().unwrap_or_else(|e| e.into_inner()).1.clone()
}

fn checked_ago() -> Option<Duration> {
    LATEST.lock().unwrap_or_else(|e| e.into_inner()).0.map(|t| t.elapsed())
}

/// A newer Nido than this one, if GitHub has one.
pub fn available() -> Option<Release> {
    latest().filter(|r| semver(&r.tag) > semver(VERSION))
}

/// `feat(games): add Memory` → Novedades · games · Add Memory. Releases and docs are left out.
fn change(msg: &str) -> Option<Change> {
    let line = msg.lines().next()?.trim();
    let (head, text) = line.split_once(": ")?;
    let head = head.trim_end_matches('!');
    let (kind, scope) = match head.split_once('(') {
        Some((k, s)) => (k, s.trim_end_matches(')').to_string()),
        None => (head, String::new()),
    };
    let kind = match kind {
        "feat" => "Novedades",
        "fix" => "Arreglos",
        "perf" => "Más rápido",
        _ => return None,
    };
    let mut c = text.chars();
    let text = c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
    Some(Change { kind, scope, text })
}

fn fetch_changes(from: &str, to: &str) -> Option<Vec<Change>> {
    let b = curl(&format!("https://api.github.com/repos/{REPO}/compare/{from}...{to}"), true)?;
    let j: serde_json::Value = serde_json::from_slice(&b).ok()?;
    let mut out: Vec<Change> = j["commits"].as_array()?.iter().filter_map(|c| change(c["commit"]["message"].as_str()?)).collect();
    out.reverse();
    Some(out)
}

/// What changed from one version to another, newest first; None while it is still being asked.
pub fn changes(from: &str, to: &str) -> Option<Vec<Change>> {
    let key = format!("{from}..{to}");
    let mut m = CHANGES.lock().unwrap_or_else(|e| e.into_inner());
    let m = m.get_or_insert_with(HashMap::new);
    if let Some(c) = m.get(&key) {
        return c.clone();
    }
    m.insert(key.clone(), None);
    let (from, to) = (from.to_string(), to.to_string());
    std::thread::spawn(move || {
        let got = fetch_changes(&from, &to);
        let mut m = CHANGES.lock().unwrap_or_else(|e| e.into_inner());
        let m = m.get_or_insert_with(HashMap::new);
        match got {
            Some(c) => { m.insert(key, Some(c)); }
            None => { m.remove(&key); }
        }
    });
    None
}

// ---------- the job ----------

fn say(kind: &str, text: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(dir().join("log")) {
        let _ = writeln!(f, "{kind}\t{}", text.replace(['\t', '\n'], " "));
    }
}

pub struct Step {
    kind: String,
    text: String,
}

pub struct Job {
    pub from: String,
    pub to: String,
    /// running, ok or fail
    pub state: &'static str,
    pub steps: Vec<Step>,
    pub size: u64,
    pub got: u64,
}

/// The last update, as its log tells it.
pub fn job() -> Option<Job> {
    let path = dir().join("log");
    let log = std::fs::read_to_string(&path).ok()?;
    let mut j = Job { from: String::new(), to: String::new(), state: "running", steps: Vec::new(), size: 0, got: 0 };
    for line in log.lines() {
        let Some((kind, text)) = line.split_once('\t') else { continue };
        match kind {
            "start" => {
                let (f, t) = text.split_once(' ').unwrap_or((text, ""));
                (j.from, j.to) = (f.to_string(), t.to_string());
            }
            "size" => j.size = text.parse().unwrap_or(0),
            _ => j.steps.push(Step { kind: kind.into(), text: text.into() }),
        }
    }
    if j.to.is_empty() {
        return None;
    }
    let last = j.steps.last().map(|s| s.kind.as_str()).unwrap_or("start");
    let quiet = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok()).unwrap_or_default();
    j.state = match last {
        "ok" => "ok",
        "fail" => "fail",
        "sys" if quiet > Duration::from_secs(120) => {
            j.steps.push(Step { kind: "fail".into(), text: "El instalador del sistema no ha terminado. Mira «journalctl -u nido-upgrade» en la Raspberry.".into() });
            "fail"
        }
        "sys" => "running",
        _ if RUNNING.load(Ordering::SeqCst) => "running",
        _ => {
            j.steps.push(Step { kind: "fail".into(), text: "Se interrumpió a medias (¿se reinició Nido?). No se ha cambiado nada.".into() });
            "fail"
        }
    };
    let part = dir().join("nido.part");
    j.got = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(if j.steps.len() > 1 { j.size } else { 0 });
    Some(j)
}

fn mb(bytes: u64) -> String {
    format!("{},{} MB", bytes / 1_000_000, bytes % 1_000_000 / 100_000)
}

fn run(db: Db, rel: Release) -> Result<(), String> {
    let d = dir();
    let tag = &rel.tag;
    let part = d.join("nido.part");
    say("step", &format!("Descargando Nido {tag} para esta placa ({ARCH})"));
    let url = format!("https://github.com/{REPO}/releases/download/{tag}/nido-{ARCH}");
    let ok = std::process::Command::new("curl")
        .args(["-sfL", "--max-time", "900", "--proto", "=https", "--proto-redir", "=https", "-o"])
        .arg(&part)
        .arg(&url)
        .status()
        .map_err(|e| format!("curl: {e}"))?
        .success();
    if !ok {
        let _ = std::fs::remove_file(&part);
        return Err("No se ha podido descargar. Revisa que Nido tenga internet.".into());
    }
    let size = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    say("step", &format!("Descargado ({}). Comprobando que funciona en esta Raspberry", mb(size)));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    let got = std::process::Command::new(&part)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let Some(got) = got else {
        let _ = std::fs::remove_file(&part);
        return Err("Lo descargado no arranca en esta Raspberry. No se ha cambiado nada.".into());
    };
    if got != *tag {
        let _ = std::fs::remove_file(&part);
        return Err(format!("GitHub ha servido la versión {got} en vez de la {tag}. No se ha cambiado nada; prueba de nuevo en unos minutos."));
    }
    say("step", &format!("Funciona: es la {got}. Guardando una copia de la base de datos"));
    for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
        if e.file_name().to_string_lossy().starts_with("nido-before-") {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let copy = d.join(format!("nido-before-{tag}.db"));
    lock(&db).execute("VACUUM INTO ?1", [copy.to_string_lossy()]).map_err(|e| format!("No se ha podido copiar la base de datos: {e}"))?;
    let copied = std::fs::metadata(&copy).map(|m| m.len()).unwrap_or(0);
    say("step", &format!("Copia guardada ({}) en {}. Pasando al instalador del sistema", mb(copied), copy.display()));
    std::fs::rename(&part, d.join("nido")).map_err(|e| e.to_string())?;
    let request = d.join("request");
    std::fs::write(&request, tag).map_err(|e| e.to_string())?;
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(500));
        if !request.exists() {
            return Ok(());
        }
    }
    let _ = std::fs::remove_file(&request);
    Err("El instalador del sistema no ha respondido. En la Raspberry, ejecuta una vez: sudo sh install.sh".into())
}

#[derive(Deserialize)]
pub struct UpdateF {
    month: Option<String>,
}

/// `POST /update`: starts updating to the latest release.
pub async fn start(State(db): State<Db>, h: HeaderMap, F(f): F<UpdateF>) -> R {
    let m = {
        let c = lock(&db);
        require_user!(c, h);
        month(&c, f.month.as_deref())?
    };
    let to_page = Redirect::to(&format!("/?month={m}&tab=update")).into_response();
    if RUNNING.load(Ordering::SeqCst) || job().is_some_and(|j| j.state == "running") {
        return Ok(to_page);
    }
    if !installer_ready() {
        return bad("Falta el instalador del sistema. En la Raspberry, ejecuta una vez: sudo sh install.sh");
    }
    let Some(rel) = available() else { return bad("Ya tienes la última versión") };
    let d = dir();
    let _ = std::fs::create_dir_all(&d);
    let _ = std::fs::remove_file(d.join("request"));
    if std::fs::write(d.join("log"), format!("start\t{VERSION} {}\nsize\t{}\n", rel.tag, rel.size)).is_err() {
        let msg = format!("Nido no puede escribir en {}. En la Raspberry, ejecuta una vez: sudo sh install.sh", d.display());
        return Ok((StatusCode::BAD_REQUEST, msg).into_response());
    }
    RUNNING.store(true, Ordering::SeqCst);
    std::thread::spawn(move || {
        if let Err(e) = run(db, rel) {
            say("fail", &e);
        }
        RUNNING.store(false, Ordering::SeqCst);
    });
    Ok(to_page)
}

/// `POST /update/check`: asks GitHub now instead of waiting for the hourly look.
pub async fn check(State(db): State<Db>, h: HeaderMap, F(f): F<UpdateF>) -> R {
    let m = {
        let c = lock(&db);
        require_user!(c, h);
        month(&c, f.month.as_deref())?
    };
    let rel = tokio::task::spawn_blocking(check_now).await?;
    let ok = match rel {
        None => "update_offline",
        Some(r) if semver(&r.tag) > semver(VERSION) => "update_found",
        Some(_) => "update_none",
    };
    back(&m, Some("update"), ok)
}

pub fn routes() -> Router<Db> {
    Router::new()
        .route("/update", post(start))
        .route("/update/check", post(check))
}

// ---------- the page ----------

fn date(d: &str) -> String {
    let m: usize = d.get(5..7).and_then(|m| m.parse().ok()).unwrap_or(0);
    let day: u32 = d.get(8..10).and_then(|d| d.parse().ok()).unwrap_or(0);
    match MONTHS.get(m.wrapping_sub(1)) {
        Some(name) if day > 0 => format!("{day} de {}", name.to_lowercase()),
        _ => String::new(),
    }
}

fn changelog(from: &str, to: &str) -> Markup {
    let Some(list) = changes(from, to) else {
        return html! { p.upd-muted { (icon("loader-circle")) "Buscando qué ha cambiado…" } };
    };
    if list.is_empty() {
        return html! { p.upd-muted { "Cambios internos, sin novedades visibles." } };
    }
    html! {
        div.upd-log {
            @for kind in ["Novedades", "Arreglos", "Más rápido"] {
                @let items: Vec<&Change> = list.iter().filter(|c| c.kind == kind).collect();
                @if !items.is_empty() {
                    h3 { (kind) }
                    ul {
                        @for c in items {
                            li { @if !c.scope.is_empty() { span.upd-scope { (c.scope) } } (c.text) }
                        }
                    }
                }
            }
        }
    }
}

fn step_icon(kind: &str, last: bool, state: &str) -> &'static str {
    match kind {
        "ok" => "circle-check",
        "fail" => "circle-alert",
        _ if last && state == "running" => "loader-circle",
        _ => "check",
    }
}

fn progress(j: &Job) -> Markup {
    let n = j.steps.len();
    html! {
        div.card.upd-job data-update=(j.state) data-to=(j.to) {
            div.upd-job-head {
                @match j.state {
                    "ok" => { (icon("party-popper")) div { b { "Nido actualizado a " (j.to) } span { "Antes tenías la " (j.from) "." } } }
                    "fail" => { (icon("circle-alert")) div { b { "No se ha podido actualizar" } span { "Sigues en la " (VERSION) ", que funciona como antes." } } }
                    _ => { (icon("loader-circle")) div { b { "Actualizando de " (j.from) " a " (j.to) } span { "Puedes cerrar esta página: sigue sola. Nido se reiniciará unos segundos." } } }
                }
            }
            @if j.state == "running" && j.size > 0 && j.got < j.size {
                div.meter.upd-meter { span style={ "width:" (j.got * 100 / j.size) "%" } {} }
                p.upd-muted.upd-bytes { (mb(j.got)) " de " (mb(j.size)) }
            }
            ol.upd-steps aria-live="polite" {
                @for (i, s) in j.steps.iter().enumerate() {
                    li class=(s.kind) { (icon(step_icon(&s.kind, i + 1 == n, j.state))) span { (s.text) } }
                }
            }
        }
    }
}

/// Tab "update": the version you have, the one there is, what changes, and the update while it runs.
pub fn render(v: &View) -> Markup {
    let job = job();
    let running = job.as_ref().is_some_and(|j| j.state == "running");
    let next = available();
    let ready = installer_ready();
    html! {
        div.set-page.upd {
            div.page-head { div { h1 { "Actualizar Nido" } p { "Tienes la " b { (VERSION) } "." } } }
            div.set-list {
                @if let Some(j) = &job {
                    @if match j.state { "ok" => j.to == VERSION, "fail" => semver(&j.to) > semver(VERSION), _ => true } {
                        section.set-group {
                            (progress(j))
                            @if j.state == "ok" {
                                div.card.upd-card {
                                    h2 { "Qué trae" }
                                    (changelog(&j.from, &j.to))
                                    p.upd-muted { "Los plugins se actualizan aparte, desde " a href=(v.link("plugins")) { "Plugins" } "." }
                                }
                            }
                        }
                    }
                }
                @if !running {
                    @if let Some(r) = &next {
                        section.set-group {
                            div.card.upd-card {
                                div.upd-new {
                                    span.upd-ico { (icon("sparkles")) }
                                    div {
                                        h2 { "Nido " (r.tag) " disponible" }
                                        p.upd-muted {
                                            @if !r.date.is_empty() { "Publicada el " (date(&r.date)) }
                                            @if r.size > 0 { " · " (mb(r.size)) }
                                        }
                                    }
                                }
                                (changelog(VERSION, &r.tag))
                                @if ready {
                                    form.upd-go method="post" action="/update" {
                                        (v.ctx())
                                        button.btn.primary { (icon("download")) "Actualizar a " (r.tag) }
                                    }
                                    ul.upd-how {
                                        li { (icon("database-backup")) "Antes se guarda una copia de la base de datos." }
                                        li { (icon("power")) "Nido se reinicia: unos segundos sin servicio." }
                                        li { (icon("refresh-cw")) "Si la nueva versión no arranca, vuelve sola a la " (VERSION) "." }
                                    }
                                } @else {
                                    div.backup-status.bad {
                                        span.backup-status-ico { (icon("circle-alert")) }
                                        div {
                                            b { "Falta el instalador del sistema" }
                                            span { "Nido no puede cambiar su propio programa sin él. Instálalo una vez en la Raspberry con " code { "sudo sh install.sh" } " y vuelve aquí." }
                                        }
                                    }
                                }
                            }
                        }
                    } @else {
                        section.set-group {
                            div.card.upd-card {
                                div.upd-new {
                                    span.upd-ico.ok { (icon("circle-check")) }
                                    div {
                                        h2 { "Tienes la última versión" }
                                        p.upd-muted {
                                            @match checked_ago() {
                                                Some(t) if t.as_secs() < 60 => "Comprobado hace un momento",
                                                Some(t) => { "Comprobado hace " (t.as_secs() / 60) " min" },
                                                None => "Sin comprobar todavía",
                                            }
                                        }
                                    }
                                    form.upd-check method="post" action="/update/check" {
                                        (v.ctx())
                                        button.btn.outline.sm { (icon("refresh-cw")) "Buscar ahora" }
                                    }
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
    fn commits_to_changes() {
        let c = change("feat(games): add Memory and Domino\n\nbody").unwrap();
        assert_eq!((c.kind, c.scope.as_str(), c.text.as_str()), ("Novedades", "games", "Add Memory and Domino"));
        let c = change("fix: download the exact release").unwrap();
        assert_eq!((c.kind, c.scope.as_str(), c.text.as_str()), ("Arreglos", "", "Download the exact release"));
        assert!(change("chore: release 0.18.1").is_none());
        assert!(change("docs: count the new games").is_none());
        assert!(change("Merge branch").is_none());
        assert_eq!(date("2026-11-15T03:10:00Z"), "15 de noviembre");
        assert_eq!(mb(9_085_392), "9,0 MB");
    }
}
