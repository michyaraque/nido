//! Atajos: NFC tags, QR codes and links that do things in Nido as whoever scans them.
//!
//! A tag only holds a random code (`/t/<code>`); what it does is a list of actions kept in the
//! core database, so it can change without writing the tag again. Actions come from the core
//! (`builtin.rs`) and from plugins, which list theirs at `GET /actions` and run them at
//! `POST /actions/run` (protocol in plugins/README.md).
use super::*;

mod builtin;
mod scan;
pub mod store;

pub use scan::{api_run, run_page, scan_get, scan_post, undo_run};
use store::{Line, Step};

/// Most actions a tag can do: each one may be a call to a plugin, and a phone is waiting.
pub const MAX_STEPS: usize = 5;
/// How long the person who scanned can take a scan back.
pub const UNDO_MINUTES: i64 = 30;
/// A second scan of the same tag by the same person this soon is most likely the phone reading twice.
pub const REPEAT_SECONDS: i64 = 120;

/// Something a tag can do, as its source offers it.
#[derive(Clone)]
pub struct Action {
    pub source: &'static str, // "core" or the plugin id
    pub id: String,
    pub group: String, // "Luca", "Compra"…: options are grouped by it
    pub name: String,
    pub icon: String,
    pub confirm: bool,       // changes something in the real world: ask before doing it
    pub ask: Option<String>, // it needs a short text, written once when setting up the tag
}

impl Action {
    pub fn key(&self) -> String {
        format!("{}|{}", self.source, self.id)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status {
    Done,
    Already, // someone did it a moment ago: nothing was written, the person may do it anyway
    Failed,
    Open, // a place to go, not something to write
}

impl Status {
    pub fn id(self) -> &'static str {
        match self {
            Status::Done => "done",
            Status::Already => "already",
            Status::Failed => "failed",
            Status::Open => "open",
        }
    }
    pub fn from_id(s: &str) -> Status {
        match s {
            "done" => Status::Done,
            "already" => Status::Already,
            "open" => Status::Open,
            _ => Status::Failed,
        }
    }
}

pub struct Outcome {
    pub status: Status,
    pub text: String,
    pub undo: Option<String>,
    pub href: Option<String>,
}

impl Outcome {
    pub fn done(text: impl Into<String>) -> Outcome {
        Outcome { status: Status::Done, text: text.into(), undo: None, href: None }
    }
    pub fn already(text: impl Into<String>) -> Outcome {
        Outcome { status: Status::Already, text: text.into(), undo: None, href: None }
    }
    pub fn failed(text: impl Into<String>) -> Outcome {
        Outcome { status: Status::Failed, text: text.into(), undo: None, href: None }
    }
    pub fn open(href: String) -> Outcome {
        Outcome { status: Status::Open, text: String::new(), undo: None, href: Some(href) }
    }
    pub fn undo(mut self, token: String) -> Outcome {
        self.undo = Some(token);
        self
    }
    pub fn href(mut self, href: &str) -> Outcome {
        self.href = Some(href.to_string());
        self
    }
}

/// Plugins write these strings; they end up in pages and in the database.
fn short(s: &str, max: usize) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

pub fn known_icon(name: &str) -> String {
    if merchants::ICONS.iter().any(|(n, _)| *n == name) { name.to_string() } else { "zap".to_string() }
}

/// Links stay inside Nido, as with notices.
pub fn safe_href(h: &str) -> Option<String> {
    (h.starts_with("/?") && h.len() <= 300 && !h.contains(['\r', '\n', '"', '<', '>'])).then(|| h.to_string())
}

/// `GET /actions` of a plugin: `[{"id","name","group","icon","confirm","ask"}]`. Bad entries are dropped.
pub fn parse_actions(source: &'static str, body: &[u8]) -> Vec<Action> {
    let list: Vec<serde_json::Value> = serde_json::from_slice(body).unwrap_or_default();
    list.iter()
        .filter_map(|a| {
            let id = a["id"].as_str()?.trim();
            let name = short(a["name"].as_str()?, 80);
            if id.is_empty() || id.len() > 80 || id.contains(['|', '\r', '\n']) || name.is_empty() {
                return None;
            }
            Some(Action {
                source,
                id: id.to_string(),
                group: short(a["group"].as_str().unwrap_or(""), 40),
                name,
                icon: known_icon(a["icon"].as_str().unwrap_or("")),
                confirm: a["confirm"].as_bool().unwrap_or(false),
                ask: a["ask"].as_str().map(|s| short(s, 60)).filter(|s| !s.is_empty()),
            })
        })
        .take(200)
        .collect()
}

/// `POST /actions/run` of a plugin: `{"status":"done"|"already"|"error","text","undo","href"}`.
/// A plain-text 4xx is read as the error to show, as plugins already answer their forms.
pub fn parse_outcome(r: Option<plugins::Reply>, plugin: &str) -> Outcome {
    let silent = || Outcome::failed(format!("{plugin} no ha contestado. Prueba otra vez en unos segundos."));
    let Some(r) = r else { return silent() };
    if r.status == 404 {
        return Outcome::failed(format!("{plugin} ya no tiene esta acción. Elige otra en Ajustes → Atajos."));
    }
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&r.body) else {
        let text = short(&String::from_utf8_lossy(&r.body), 200);
        return if (400..500).contains(&r.status) && !text.is_empty() { Outcome::failed(text) } else { silent() };
    };
    let status = match v["status"].as_str() {
        Some("done") if r.status == 200 => Status::Done,
        Some("already") if r.status == 200 => Status::Already,
        _ => Status::Failed,
    };
    let text = short(v["text"].as_str().unwrap_or(""), 200);
    if status == Status::Failed && text.is_empty() {
        return silent();
    }
    Outcome {
        status,
        text,
        undo: v["undo"].as_str().filter(|u| !u.is_empty() && u.len() <= 200 && status == Status::Done).map(String::from),
        href: v["href"].as_str().and_then(safe_href),
    }
}

fn who<'a>(db: &Db, me: &'a User) -> plugins::Who<'a> {
    let people = plugins::house(&lock(db)).unwrap_or_default();
    plugins::Who { id: me.id, name: &me.name, people }
}

/// Everything a tag can do in this house right now: the core's actions, then each active plugin's.
pub async fn catalog(db: &Db, me: &User) -> Vec<Action> {
    let (mut all, on) = {
        let c = lock(db);
        let on = plugins::enabled(&c).unwrap_or_default();
        (builtin::catalog(&c, &on).unwrap_or_else(|e| {
            eprintln!("atajos: {e}");
            Vec::new()
        }), on)
    };
    for k in on {
        let who = who(db, me);
        if let Some(r) = plugins::ask(db, k.id, &who, "GET", "/actions", "").await.filter(|r| r.status == 200) {
            all.extend(parse_actions(k.id, &r.body));
        }
    }
    // Tags mostly write things down: places to open go last in the list.
    all.sort_by_key(|a| a.source == "core" && a.id.starts_with("open:"));
    all
}

/// The name people know a source by.
pub fn source_name(source: &str) -> &'static str {
    if source == "core" { "Casa" } else { plugins::find(source).map_or("Plugin", |k| k.name) }
}

/// Does one action of a tag, as `me`. `force` does it even if someone just did it.
pub async fn run(db: &Db, me: &User, step: &Step, force: bool) -> Outcome {
    if step.source == "core" {
        let c = lock(db);
        return builtin::run(&c, me.id, &step.action, &step.arg, force).unwrap_or_else(|e| {
            eprintln!("atajos: {e}");
            Outcome::failed("No se ha podido apuntar. Prueba otra vez.")
        });
    }
    let Some(k) = plugins::find(&step.source) else { return Outcome::failed("Ese plugin ya no existe. Cambia este atajo en Ajustes → Atajos.") };
    let on = plugins::enabled(&lock(db)).unwrap_or_default().iter().any(|p| p.id == k.id);
    if !on {
        return Outcome::failed(format!("{} está desactivado. Actívalo en Plugins o cambia este atajo.", k.name));
    }
    let form = serde_urlencoded::to_string([("id", step.action.as_str()), ("arg", step.arg.as_str()), ("force", if force { "1" } else { "0" })]).unwrap_or_default();
    let who = who(db, me);
    parse_outcome(plugins::ask(db, k.id, &who, "POST", "/actions/run", &form).await, k.name)
}

/// Takes back what one action wrote. Sources only accept their own recent entries by the same person.
pub async fn undo(db: &Db, me: &User, source: &str, token: &str) -> bool {
    if source == "core" {
        return builtin::undo(&lock(db), me.id, token).unwrap_or(false);
    }
    let form = serde_urlencoded::to_string([("undo", token)]).unwrap_or_default();
    let who = who(db, me);
    let Some(r) = plugins::ask(db, source, &who, "POST", "/actions/undo", &form).await else { return false };
    r.status == 200 && serde_json::from_slice::<serde_json::Value>(&r.body).is_ok_and(|v| v["ok"].as_bool() == Some(true))
}

/// Runs `steps` in order and keeps the scan: what happened and how to take it back.
pub async fn perform(db: &Db, me: &User, tag: i64, steps: &[&Step], force: bool) -> Result<i64, AppErr> {
    let mut lines = Vec::new();
    let mut undo = Vec::new();
    for s in steps {
        let out = run(db, me, s, force).await;
        if let Some(t) = &out.undo {
            undo.push((s.source.clone(), t.clone()));
        }
        lines.push(Line { pos: s.pos, label: s.label.clone(), icon: s.icon.clone(), status: out.status.id().into(), text: out.text, href: out.href });
    }
    Ok(store::record(&lock(db), tag, me.id, &lines, &undo)?)
}

/// How phones reach this Nido: what goes on the tag. Caddy says when it came over https.
pub fn origin(h: &HeaderMap) -> String {
    let host = h.get(header::HOST).and_then(|v| v.to_str().ok()).filter(|v| v.chars().all(|c| c.is_ascii_alphanumeric() || ".-:[]".contains(c))).unwrap_or("nido.local");
    let https = h.get("x-forwarded-proto").and_then(|v| v.to_str().ok()) == Some("https");
    format!("{}://{host}", if https { "https" } else { "http" })
}

// ---------- managing tags (Ajustes → Atajos) ----------

fn field<'a>(f: &'a [(String, String)], k: &str) -> &'a str {
    f.iter().find(|p| p.0 == k).map_or("", |p| p.1.as_str())
}

/// The label a step shows: the action's name, plus what was written for it.
pub fn step_label(a: &Action, arg: &str) -> String {
    if arg.is_empty() { a.name.clone() } else { format!("{} · {arg}", a.name) }
}

/// The form's `act`/`arg` pairs (in order) turned into steps, checked against what exists now.
/// An action that is gone for now (its plugin is off) keeps its saved step, so renaming a tag
/// does not silently drop it.
fn steps_from(f: &[(String, String)], catalog: &[Action], kept: &[Step]) -> Result<Vec<Step>, &'static str> {
    let acts = f.iter().filter(|p| p.0 == "act").map(|p| p.1.as_str());
    let args = f.iter().filter(|p| p.0 == "arg").map(|p| p.1.as_str()).chain(std::iter::repeat(""));
    let mut out: Vec<Step> = Vec::new();
    for (key, arg) in acts.zip(args) {
        if key.is_empty() {
            continue;
        }
        if out.len() == MAX_STEPS {
            return Err("Un atajo puede hacer 5 cosas como mucho");
        }
        let arg = short(arg, 60);
        let pos = out.len() as i64;
        if let Some(a) = catalog.iter().find(|a| a.key() == key) {
            if a.ask.is_some() && arg.is_empty() {
                return Err("Falta escribir el detalle de una acción (qué añadir, el importe…)");
            }
            let arg = if a.ask.is_some() { arg } else { String::new() };
            out.push(Step { pos, source: a.source.to_string(), action: a.id.clone(), label: step_label(a, &arg), arg, icon: a.icon.clone(), confirm: a.confirm });
        } else if let Some(s) = kept.iter().find(|s| format!("{}|{}", s.source, s.action) == key) {
            out.push(Step { pos, ..s.clone() });
        } else {
            return Err("Una de las acciones ya no existe. Elige otra.");
        }
    }
    if out.is_empty() {
        return Err("Elige al menos una cosa que haga el atajo");
    }
    Ok(out)
}

fn saved(c: &Connection, f: &[(String, String)], id: i64, ok: &str) -> R {
    let m = month(c, Some(field(f, "month")))?;
    Ok(Redirect::to(&format!("/?month={m}&tab=shortcuts&e={id}&ok={ok}#tag-edit")).into_response())
}

/// `POST /shortcuts` (new) and `POST /shortcuts/<id>` (edit): same form, same checks.
async fn save(db: &Db, h: &HeaderMap, id: Option<i64>, f: Vec<(String, String)>) -> R {
    let (me, kept) = {
        let c = lock(db);
        let me = require_user!(c, h);
        let kept = match id {
            Some(id) => match store::by_id(&c, id)? {
                Some(t) => t.steps,
                None => return bad("Ese atajo ya no existe"),
            },
            None => Vec::new(),
        };
        (me, kept)
    };
    let catalog = catalog(db, &me).await;
    let steps = match steps_from(&f, &catalog, &kept) {
        Ok(s) => s,
        Err(e) => return bad(e),
    };
    let name = short(field(&f, "name"), 40);
    let name = if name.is_empty() { steps[0].label.chars().take(40).collect() } else { name };
    let choose = field(&f, "mode") == "choose";
    let confirm = !field(&f, "confirm").is_empty();
    let c = lock(db);
    let id = match id {
        Some(id) => {
            store::update(&c, id, &name, choose, confirm, &steps)?;
            id
        }
        None => {
            // A tag written before its atajo existed (scanned, then "Configurar") keeps its code.
            let code = Some(field(&f, "code")).filter(|s| store::valid_code(s) && store::by_code(&c, s).ok().flatten().is_none());
            let code = code.map(str::to_string).unwrap_or_else(store::new_code);
            store::create(&c, &code, &name, choose, confirm, &steps, me.id)?
        }
    };
    saved(&c, &f, id, "tag_saved")
}

async fn create(State(db): State<Db>, h: HeaderMap, F(f): F<Vec<(String, String)>>) -> R {
    save(&db, &h, None, f).await
}

async fn update(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Vec<(String, String)>>) -> R {
    save(&db, &h, Some(id), f).await
}

async fn delete(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Vec<(String, String)>>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    store::delete(&c, id)?;
    back(&month(&c, Some(field(&f, "month")))?, Some("shortcuts"), "tag_deleted")
}

pub fn routes() -> Router<Db> {
    Router::new()
        .route("/shortcuts", post(create))
        .route("/shortcuts/{id}", post(update))
        .route("/shortcuts/{id}/delete", post(delete))
        .route("/t/{code}", get(scan_get).post(scan_post))
        .route("/t/r/{id}", get(run_page))
        .route("/t/r/{id}/undo", post(undo_run))
        .route("/api/t/{code}", post(api_run))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(status: u16, body: &str) -> Option<plugins::Reply> {
        Some(plugins::Reply { status, headers: Vec::new(), body: body.as_bytes().to_vec() })
    }

    #[test]
    fn reads_plugin_actions() {
        let list = parse_actions(
            "pets",
            br#"[{"id":"meal:3","name":"Comida de Luca","group":"Luca","icon":"utensils"},
                 {"id":"bad|id","name":"x"},{"id":"","name":"x"},{"id":"walk:3"},
                 {"id":"treat:3","name":"  Premio   de Luca ","icon":"no-such-icon","ask":"Que premio","confirm":true}]"#,
        );
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].key(), "pets|meal:3");
        assert_eq!((list[0].icon.as_str(), list[0].group.as_str(), list[0].confirm), ("utensils", "Luca", false));
        assert_eq!((list[1].name.as_str(), list[1].icon.as_str()), ("Premio de Luca", "zap"));
        assert_eq!((list[1].ask.as_deref(), list[1].confirm), (Some("Que premio"), true));
        assert!(parse_actions("pets", b"not json").is_empty());
    }

    #[test]
    fn reads_plugin_outcomes() {
        let o = parse_outcome(reply(200, r#"{"status":"done","text":"Comida apuntada","undo":"f3-0800","href":"/?tab=pets&p=3"}"#), "Mascotas");
        assert_eq!((o.status, o.undo.as_deref(), o.href.as_deref()), (Status::Done, Some("f3-0800"), Some("/?tab=pets&p=3")));
        let o = parse_outcome(reply(200, r#"{"status":"already","text":"Ana ya le dio de comer","undo":"x","href":"https://evil"}"#), "Mascotas");
        assert_eq!((o.status, o.undo, o.href), (Status::Already, None, None), "no undo for what was not written; no links out");
        let o = parse_outcome(reply(400, "Esa mascota ya no está"), "Mascotas");
        assert_eq!((o.status, o.text.as_str()), (Status::Failed, "Esa mascota ya no está"));
        assert_eq!(parse_outcome(None, "Mascotas").text, "Mascotas no ha contestado. Prueba otra vez en unos segundos.");
        assert!(parse_outcome(reply(404, ""), "Mascotas").text.contains("ya no tiene esta acción"));
        assert_eq!(parse_outcome(reply(500, "{}"), "Mascotas").status, Status::Failed);
    }

    #[test]
    fn builds_steps_from_the_form() {
        let a = |id: &str, ask: Option<&str>| Action { source: "core", id: id.into(), group: String::new(), name: id.to_uppercase(), icon: "zap".into(), confirm: false, ask: ask.map(Into::into) };
        let catalog = [a("shop", Some("Qué")), a("open:tasks", None)];
        let pairs = |p: &[(&str, &str)]| p.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<Vec<_>>();
        let kept = [Step { pos: 0, source: "pets".into(), action: "meal:3".into(), arg: String::new(), label: "Comida de Luca".into(), icon: "utensils".into(), confirm: false }];

        let f = pairs(&[("act", "pets|meal:3"), ("arg", ""), ("act", ""), ("arg", "x"), ("act", "core|shop"), ("arg", " Leche "), ("act", "core|open:tasks"), ("arg", "ignored")]);
        let s = steps_from(&f, &catalog, &kept).unwrap();
        let got: Vec<_> = s.iter().map(|s| (s.pos, s.label.as_str(), s.arg.as_str())).collect();
        assert_eq!(got, [(0, "Comida de Luca", ""), (1, "SHOP · Leche", "Leche"), (2, "OPEN:TASKS", "")]);

        assert!(steps_from(&pairs(&[("act", "core|shop"), ("arg", "")]), &catalog, &[]).is_err(), "shop needs what to add");
        assert!(steps_from(&pairs(&[("act", "pets|meal:3")]), &catalog, &[]).is_err(), "unknown and not kept");
        assert!(steps_from(&pairs(&[("act", "")]), &catalog, &[]).is_err());
        let six: Vec<_> = (0..6).flat_map(|_| [("act", "core|open:tasks"), ("arg", "")]).collect();
        assert!(steps_from(&pairs(&six), &catalog, &[]).is_err());
    }
}
