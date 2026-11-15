//! nido-devices: the "Domótica" plugin of Nido, as a program of its own (see plugins/README.md).
//!
//! Plugs, lights and switches of the house, controlled on the LAN without any vendor app. Every
//! device is described by what it can do (switches, power meter), not by its brand; the drivers
//! (`tuya.rs` for now) turn that into each protocol. A poller keeps the state fresh and adds up
//! the energy each device uses.
use axum::{
    extract::{Form, Path, Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use maud::{html, Markup, PreEscaped};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

mod broadlink;
mod ir;
mod scenes;
mod sharing;
mod tuya;

const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));
const CSS: &str = include_str!(concat!(env!("OUT_DIR"), "/style.css"));
const FLASHES: &[(&str, &str)] = &[
    ("device_added", "Dispositivo añadido"),
    ("device_saved", "Guardado"),
    ("device_deleted", "Dispositivo quitado"),
    ("tuya_imported", "Aparatos traídos de tu cuenta"),
    ("scene_run", "Escena en marcha"),
    ("scene_saved", "Escena guardada"),
    ("scene_deleted", "Escena borrada"),
    ("price_saved", "Precio guardado"),
    ("ir_sent", "Enviado"),
    ("ir_added", "Botón añadido"),
];

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

fn icon(name: &str) -> Markup {
    let inner = ICONS.iter().find(|(n, _)| *n == name).map(|(_, s)| *s).unwrap_or("");
    html! {
        svg.i viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
            stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" { (PreEscaped(inner)) }
    }
}

type Db = Arc<Mutex<Connection>>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Default)]
struct Live {
    online: bool,
    dps: Map<String, Value>,
    err: String,
    at: Option<Instant>,
    hist: std::collections::VecDeque<f32>,
    misses: u8,
}

#[derive(Clone)]
struct App {
    db: Db,
    live: Arc<Mutex<HashMap<i64, Live>>>,
    seen: Arc<Mutex<HashMap<String, tuya::Seen>>>,
    qr: Arc<Mutex<Qr>>,
    people: Arc<Mutex<Vec<i64>>>,
    notes: Arc<Mutex<Vec<Note>>>,
    want_sync: Arc<AtomicBool>,
    bl: Arc<Mutex<Vec<broadlink::Found>>>,
    learn: Arc<Mutex<ir::Learn>>,
}

/// A notice from a scene, still to be seen by `pending`.
struct Note {
    text: String,
    pending: Vec<i64>,
    at: Instant,
}

/// The QR sign-in in progress (one at a time: a new one replaces it).
#[derive(Default, Clone)]
struct Qr {
    token: String,
    state: QrState,
}

#[derive(Default, Clone)]
enum QrState {
    #[default]
    Waiting,
    Done(usize),
    Failed(String),
}

#[derive(Clone)]
struct Device {
    id: i64,
    driver: String,
    uid: String,
    name: String,
    room: String,
    category: String,
    ip: String,
    secret: String,
    version: String,
}

fn devices(c: &Connection) -> rusqlite::Result<Vec<Device>> {
    c.prepare("SELECT id,uid,name,room,category,ip,secret,version,driver FROM device ORDER BY room<>'' DESC, room, name")?
        .query_map([], |r| {
            Ok(Device {
                id: r.get(0)?,
                uid: r.get(1)?,
                name: r.get(2)?,
                room: r.get(3)?,
                category: r.get(4)?,
                ip: r.get(5)?,
                secret: r.get(6)?,
                version: r.get(7)?,
                driver: r.get(8)?,
            })
        })?
        .collect()
}

fn one(c: &Connection, id: i64) -> rusqlite::Result<Option<Device>> {
    Ok(devices(c)?.into_iter().find(|d| d.id == id))
}

fn is_light(cat: &str) -> bool {
    matches!(cat, "dj" | "dd" | "fwd" | "xdd" | "dc" | "tgq" | "tgkg")
}

/// The data points that turn something on and off. Plugs and switches use 1…6; lights use 20 (or 1
/// on old firmware). A device of an unknown kind exposes whatever boolean it reports.
fn switches(d: &Device, dps: &Map<String, Value>) -> Vec<String> {
    let bools = |ids: &[&str]| -> Vec<String> {
        ids.iter().filter(|k| dps.get(**k).is_some_and(Value::is_boolean)).map(|k| k.to_string()).collect()
    };
    if is_light(&d.category) {
        return bools(&["20", "1"]).into_iter().take(1).collect();
    }
    let s = bools(&["1", "2", "3", "4", "5", "6"]);
    if !s.is_empty() {
        return s;
    }
    let mut all: Vec<String> = dps.iter().filter(|(_, v)| v.is_boolean()).map(|(k, _)| k.clone()).collect();
    all.sort_by_key(|k| k.parse::<u32>().unwrap_or(u32::MAX));
    all
}

/// Watts now, from the standard Tuya plug data point 19 (tenths of a watt).
fn watts(d: &Device, dps: &Map<String, Value>) -> Option<f64> {
    (!is_light(&d.category)).then(|| dps.get("19")?.as_f64().map(|v| v / 10.0)).flatten()
}

fn tuya_dev(d: &Device) -> Option<tuya::Dev> {
    Some(tuya::Dev { id: d.uid.clone(), ip: d.ip.clone(), key: tuya::key(&d.secret)?, ver: tuya::Ver::parse(&d.version) })
}

/// Asks the device for its state, keeps it, and adds the energy used since the last reading.
async fn refresh(app: &App, d: &Device) {
    if d.driver == "broadlink" {
        return ir::refresh(app, d).await;
    }
    let Some(dev) = tuya_dev(d) else {
        lock(&app.live).entry(d.id).or_default().err = "La clave local debe tener 16 caracteres".into();
        return;
    };
    let r = tuya::status(&dev).await;
    let now = Instant::now();
    let mut live = lock(&app.live);
    let l = live.entry(d.id).or_default();
    match r {
        Ok(dps) => {
            if let (true, Some(at), Some(w)) = (l.online, l.at, watts(d, &dps)) {
                let secs = now.duration_since(at).as_secs_f64().min(60.0);
                let _ = lock(&app.db).execute(
                    "INSERT INTO energy_hour(device,hour,wh) VALUES(?1,strftime('%Y-%m-%d %H','now','localtime'),?2)
                     ON CONFLICT(device,hour) DO UPDATE SET wh=wh+excluded.wh",
                    params![d.id, w * secs / 3600.0],
                );
            }
            if let Some(w) = watts(d, &dps) {
                l.hist.push_back(w as f32);
                if l.hist.len() > 72 {
                    l.hist.pop_front();
                }
            }
            l.dps.extend(dps);
            l.online = true;
            l.err.clear();
        }
        Err(e) => {
            l.online = false;
            l.err = e;
        }
    }
    l.at = Some(now);
}

async fn poll(app: App) {
    loop {
        let list = devices(&lock(&app.db)).unwrap_or_default();
        let mut set = tokio::task::JoinSet::new();
        for d in list {
            let app = app.clone();
            set.spawn_local(async move { refresh(&app, &d).await });
        }
        while set.join_next().await.is_some() {}
        scenes::tick(&app);
        tokio::time::sleep(Duration::from_secs(4)).await;
    }
}

/// Devices without an IP (their announcements never arrived): scan the LAN and try their key on
/// every Tuya port found, like tinytuya's force scan.
async fn hunt(app: App) {
    loop {
        tokio::time::sleep(Duration::from_secs(20)).await;
        let all = devices(&lock(&app.db)).unwrap_or_default();
        let lost: Vec<&Device> = all.iter().filter(|d| d.ip.is_empty() && tuya::key(&d.secret).is_some()).collect();
        if lost.is_empty() {
            continue;
        }
        let mut free: Vec<String> = tuya::scan().await.into_iter().filter(|ip| !all.iter().any(|d| &d.ip == ip)).collect();
        for d in lost {
            let Some(key) = tuya::key(&d.secret) else { continue };
            if let Some((ip, ver)) = tuya::locate(&d.uid, key, tuya::Ver::parse(&d.version), &free).await {
                let _ = lock(&app.db).execute("UPDATE device SET ip=?2, version=?3 WHERE id=?1", params![d.id, ip, ver.as_str()]);
                free.retain(|f| *f != ip);
            }
        }
        tokio::time::sleep(Duration::from_secs(40)).await;
    }
}

fn found(app: &App, s: tuya::Seen) {
    let c = lock(&app.db);
    let _ = c.execute(
        "UPDATE device SET ip=?2, version=?3 WHERE driver='tuya' AND uid=?1 AND (ip<>?2 OR version<>?3)",
        params![s.id, s.ip, s.ver],
    );
    if c.query_row("SELECT 1 FROM device WHERE uid=?1", [&s.id], |_| Ok(())).is_err() {
        app.want_sync.store(true, Ordering::Relaxed);
    }
    drop(c);
    lock(&app.seen).insert(s.id.clone(), s);
}

fn back(q: &str) -> Response {
    Redirect::to(&format!("/?tab=devices{q}")).into_response()
}

fn fail(q: &str, msg: &str) -> Response {
    back(&format!("{q}&{}", serde_urlencoded::to_string([("err", msg)]).unwrap_or_default()))
}

type Params = HashMap<String, String>;

fn clean(s: Option<&String>) -> String {
    s.map(|s| s.trim().chars().filter(|c| !c.is_control()).take(80).collect()).unwrap_or_default()
}

fn default_dp(d: &Device) -> String {
    if is_light(&d.category) { "20".into() } else { "1".into() }
}

/// Turns one switch on (`Some(true)`), off, or the other way round (`None`), then reads it back.
async fn switch(app: &App, d: &Device, dp: &str, want: Option<bool>) -> Result<(), String> {
    let dev = tuya_dev(d).ok_or("sin clave")?;
    let cur = lock(&app.live).get(&d.id).and_then(|l| l.dps.get(dp)?.as_bool()).unwrap_or(false);
    let to = want.unwrap_or(!cur);
    let mut dps = Map::new();
    dps.insert(dp.to_string(), Value::Bool(to));
    tuya::set(&dev, Value::Object(dps)).await?;
    lock(&app.live).entry(d.id).or_default().dps.insert(dp.to_string(), Value::Bool(to));
    refresh(app, d).await;
    Ok(())
}

async fn toggle(State(app): State<App>, Path(id): Path<i64>, Form(f): Form<Params>) -> Response {
    let Some(d) = one(&lock(&app.db), id).ok().flatten() else { return back("") };
    match switch(&app, &d, &clean(f.get("dp")), None).await {
        Ok(()) => back(""),
        Err(e) => fail("", &format!("{}: {}", d.name, plain_err(&e))),
    }
}

fn notify(app: &App, text: &str) {
    let people = lock(&app.people).clone();
    if !text.trim().is_empty() && !people.is_empty() {
        lock(&app.notes).push(Note { text: text.trim().to_string(), pending: people, at: Instant::now() });
    }
}

fn remember_people(app: &App, h: &HeaderMap) {
    let ids: Vec<i64> = h
        .get("x-nido-people")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(',')
        .filter_map(|p| p.split('=').next()?.parse().ok())
        .collect();
    if !ids.is_empty() {
        *lock(&app.people) = ids;
    }
}

/// In-app notices for whoever has Nido open (Nido asks every 3 s).
async fn notices(State(app): State<App>, h: HeaderMap) -> Response {
    remember_people(&app, &h);
    let me: i64 = h.get("x-nido-user").and_then(|v| v.to_str().ok()?.parse().ok()).unwrap_or(0);
    let mut notes = lock(&app.notes);
    let mut out = Vec::new();
    for n in notes.iter_mut() {
        if let Some(i) = n.pending.iter().position(|u| *u == me) {
            n.pending.remove(i);
            out.push(serde_json::json!({ "text": n.text, "href": "/?tab=devices", "action": "Ver" }));
        }
    }
    notes.retain(|n| !n.pending.is_empty());
    axum::Json(out).into_response()
}

/// Notices nobody saw in 30 s: Nido sends them to the phone.
async fn notices_late(State(app): State<App>) -> Response {
    let mut notes = lock(&app.notes);
    let mut out = Vec::new();
    for n in notes.iter_mut().filter(|n| n.at.elapsed() > Duration::from_secs(30)) {
        for u in n.pending.drain(..) {
            out.push(serde_json::json!({ "user": u, "text": n.text, "href": "/?tab=devices" }));
        }
    }
    notes.retain(|n| !n.pending.is_empty());
    axum::Json(out).into_response()
}

async fn set_price(State(app): State<App>, Form(f): Form<Params>) -> Response {
    let p: f64 = clean(f.get("price")).replace(',', ".").parse().unwrap_or(0.0);
    if !(0.01..=2.0).contains(&p) {
        return fail("&energy=1", "Pon un precio entre 0,01 y 2 € por kWh");
    }
    let _ = lock(&app.db).execute("INSERT OR REPLACE INTO setting(k,v) VALUES('kwh_price',?1)", [p.to_string()]);
    back("&energy=1&ok=price_saved")
}

fn upsert(c: &Connection, seen: &HashMap<String, tuya::Seen>, uid: &str, name: &str, room: &str, cat: &str, key: &str, ip: &str, ver: &str) -> rusqlite::Result<()> {
    let s = seen.get(uid);
    let ip = if ip.is_empty() { s.map(|s| s.ip.as_str()).unwrap_or("") } else { ip };
    let ver = if ver.is_empty() { s.map(|s| s.ver.as_str()).unwrap_or("3.3") } else { ver };
    c.execute(
        "INSERT INTO device(driver,uid,name,room,category,ip,secret,version) VALUES('tuya',?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(driver,uid) DO UPDATE SET secret=excluded.secret, category=excluded.category,
           ip=CASE WHEN excluded.ip<>'' THEN excluded.ip ELSE ip END, version=excluded.version",
        params![uid, name, room, cat, ip, key, ver],
    )?;
    Ok(())
}

async fn add(State(app): State<App>, Form(f): Form<Params>) -> Response {
    let (uid, key) = (clean(f.get("uid")), clean(f.get("key")));
    if uid.is_empty() || tuya::key(&key).is_none() {
        return fail("&add=manual", "Revisa el ID y la clave: la clave tiene 16 caracteres");
    }
    let name = Some(clean(f.get("name"))).filter(|n| !n.is_empty()).unwrap_or_else(|| "Enchufe".into());
    let seen = lock(&app.seen).clone();
    let r = upsert(&lock(&app.db), &seen, &uid, &name, &clean(f.get("room")), "", &key, &clean(f.get("ip")), &clean(f.get("version")));
    match r {
        Ok(()) => back("&ok=device_added"),
        Err(e) => fail("&add=manual", &e.to_string()),
    }
}

async fn cloud(State(app): State<App>, Form(f): Form<Params>) -> Response {
    let from = Some(clean(f.get("from"))).filter(|s| s.starts_with("&add=")).unwrap_or("&add=1".into());
    let saved = |k: &str| setting(&lock(&app.db), k);
    let pick = |form: &str, k: &str| Some(clean(f.get(form))).filter(|v| !v.is_empty()).unwrap_or_else(|| saved(k));
    let (region, id, secret) = (pick("region", "tuya_region"), pick("access_id", "tuya_id"), pick("access_secret", "tuya_secret"));
    if id.is_empty() || secret.is_empty() {
        return fail(&from, "Faltan el Access ID y el Access Secret");
    }
    let region = if region.is_empty() { "eu".to_string() } else { region };
    let (r2, i2, s2) = (region.clone(), id.clone(), secret.clone());
    let list = match tokio::task::spawn_blocking(move || tuya::cloud_devices(&r2, &i2, &s2)).await {
        Ok(Ok(l)) => l,
        Ok(Err(e)) => return fail(&from, &format!("No hemos podido conectar ({e}). Revisa las llaves y el centro de datos.")),
        Err(e) => return fail(&from, &e.to_string()),
    };
    if list.is_empty() {
        return fail(&from, "La cuenta está bien, pero no tiene aparatos: enlaza tu app (paso 2)");
    }
    let seen = lock(&app.seen).clone();
    let c = lock(&app.db);
    for (k, v) in [("tuya_region", &region), ("tuya_id", &id), ("tuya_secret", &secret)] {
        let _ = c.execute("INSERT OR REPLACE INTO setting(k,v) VALUES(?1,?2)", params![k, v]);
    }
    for d in &list {
        if let Err(e) = upsert(&c, &seen, &d.id, &d.name, "", &d.category, &d.key, "", "") {
            return fail(&from, &e.to_string());
        }
    }
    back("&ok=tuya_imported")
}

/// Brings every device of the Smart Life account in, and keeps the session for later syncs.
fn import(app: &App, mut s: sharing::Session) -> Result<usize, String> {
    sharing::refresh(&mut s)?;
    let list = sharing::devices(&s)?;
    let seen = lock(&app.seen).clone();
    let c = lock(&app.db);
    let json = serde_json::to_string(&s).unwrap_or_default();
    c.execute("INSERT OR REPLACE INTO setting(k,v) VALUES('tuya_session',?1)", [json]).map_err(|e| e.to_string())?;
    for d in &list {
        upsert(&c, &seen, &d.id, &d.name, "", &d.category, &d.key, "", "").map_err(|e| e.to_string())?;
    }
    Ok(list.len())
}

async fn qr_start(State(app): State<App>, Form(f): Form<Params>) -> Response {
    let id = Some(clean(f.get("app"))).filter(|a| APPS.iter().any(|x| x.0 == a)).unwrap_or("smartlife".into());
    let code = clean(f.get("user_code"));
    let here = format!("&add=tuya&app={id}");
    if code.is_empty() {
        return fail(&here, "Pega tu código de usuario");
    }
    let c2 = code.clone();
    let token = match tokio::task::spawn_blocking(move || sharing::qr_token(&c2)).await {
        Ok(Ok(t)) => t,
        Ok(Err(e)) => return fail(&here, &format!("Ese código no funciona ({e}). Revisa que esté copiado tal cual.")),
        Err(e) => return fail(&here, &e.to_string()),
    };
    *lock(&app.qr) = Qr { token: token.clone(), state: QrState::Waiting };
    let a = app.clone();
    std::thread::spawn(move || {
        let set = |st: QrState| {
            let mut q = lock(&a.qr);
            if q.token == token {
                q.state = st;
            }
        };
        for _ in 0..150 {
            std::thread::sleep(Duration::from_secs(2));
            if lock(&a.qr).token != token {
                return;
            }
            if let Ok(Some(s)) = sharing::login_result(&token, &code) {
                return set(match import(&a, s) {
                    Ok(n) => QrState::Done(n),
                    Err(e) => QrState::Failed(format!("Has entrado, pero no hemos podido traer tus aparatos: {e}")),
                });
            }
        }
        set(QrState::Failed("El código QR ha caducado sin escanearse.".into()));
    });
    back(&format!("{here}&step=2"))
}

/// The account's device list again (new devices, new keys after a re-pair), with whichever
/// account the house connected. `Ok(None)` when none is.
fn resync(app: &App) -> Result<usize, String> {
    let (session, region, id, secret) = {
        let c = lock(&app.db);
        (setting(&c, "tuya_session"), setting(&c, "tuya_region"), setting(&c, "tuya_id"), setting(&c, "tuya_secret"))
    };
    if let Ok(s) = serde_json::from_str::<sharing::Session>(&session) {
        return import(app, s);
    }
    if id.is_empty() {
        return Err("no hay ninguna cuenta conectada".into());
    }
    let list = tuya::cloud_devices(if region.is_empty() { "eu" } else { &region }, &id, &secret)?;
    let seen = lock(&app.seen).clone();
    let c = lock(&app.db);
    for d in &list {
        upsert(&c, &seen, &d.id, &d.name, "", &d.category, &d.key, "", "").map_err(|e| e.to_string())?;
    }
    Ok(list.len())
}

/// Syncs by itself: every 15 minutes, and soon after an unknown device shows up on the LAN
/// (just paired in the app), never more than once every 2 minutes.
async fn autosync(app: App) {
    let mut last: Option<Instant> = None;
    loop {
        tokio::time::sleep(Duration::from_secs(20)).await;
        let due = match last {
            None => true,
            Some(t) => t.elapsed() > Duration::from_secs(15 * 60) || (app.want_sync.load(Ordering::Relaxed) && t.elapsed() > Duration::from_secs(120)),
        };
        if !due {
            continue;
        }
        app.want_sync.store(false, Ordering::Relaxed);
        last = Some(Instant::now());
        let a = app.clone();
        if let Ok(Err(e)) = tokio::task::spawn_blocking(move || resync(&a)).await {
            if !e.starts_with("no hay") {
                eprintln!("devices: sincronizar: {e}");
            }
        }
    }
}

/// "Sincronizar ahora": the same listing again with the saved Smart Life session, or the developer keys.
async fn sync(State(app): State<App>) -> Response {
    let a = app.clone();
    match tokio::task::spawn_blocking(move || resync(&a)).await {
        Ok(Ok(_)) => back("&ok=tuya_imported"),
        Ok(Err(e)) => fail("&add=1", &format!("No hemos podido sincronizar ({e}). Vuelve a conectar tu app.")),
        Err(e) => fail("&add=1", &e.to_string()),
    }
}

async fn save(State(app): State<App>, Path(id): Path<i64>, Form(f): Form<Params>) -> Response {
    let key = clean(f.get("key"));
    if !f.contains_key("key") {
        let _ = lock(&app.db).execute("UPDATE device SET name=?2, room=?3 WHERE id=?1", params![id, clean(f.get("name")), clean(f.get("room"))]);
        return back(&format!("&d={id}&ok=device_saved"));
    }
    if tuya::key(&key).is_none() {
        return fail(&format!("&d={id}"), "La clave local debe tener 16 caracteres");
    }
    let r = lock(&app.db).execute(
        "UPDATE device SET name=?2, room=?3, ip=?4, secret=?5, version=?6 WHERE id=?1",
        params![id, clean(f.get("name")), clean(f.get("room")), clean(f.get("ip")), key, clean(f.get("version"))],
    );
    match r {
        Ok(_) => back("&ok=device_saved"),
        Err(e) => fail(&format!("&d={id}"), &e.to_string()),
    }
}

async fn delete(State(app): State<App>, Path(id): Path<i64>) -> Response {
    let c = lock(&app.db);
    let _ = c.execute("DELETE FROM device WHERE id=?1", [id]);
    let _ = c.execute("DELETE FROM energy_hour WHERE device=?1", [id]);
    let _ = c.execute("DELETE FROM ir_button WHERE device=?1", [id]);
    lock(&app.live).remove(&id);
    back("&ok=device_deleted")
}

fn kwh(wh: f64) -> String {
    if wh < 10.0 { format!("{wh:.1} Wh").replace('.', ",") } else if wh < 1000.0 { format!("{wh:.0} Wh") } else { format!("{:.2} kWh", wh / 1000.0).replace('.', ",") }
}

fn w(v: f64) -> String {
    if v < 10.0 { format!("{v:.1} W").replace('.', ",") } else { format!("{v:.0} W") }
}

fn eur(x: f64) -> String {
    format!("{x:.2} €").replace('.', ",")
}

/// €/kWh the house pays; 0.15 until someone sets it.
fn price(c: &Connection) -> f64 {
    setting(c, "kwh_price").parse().ok().filter(|p: &f64| *p > 0.0 && *p < 5.0).unwrap_or(0.15)
}

fn ver_select(cur: &str) -> Markup {
    html! {
        select.input name="version" {
            @for v in ["3.3", "3.4", "3.5"] { option value=(v) selected[cur == v] { (v) } }
        }
    }
}

fn alert(p: &Params) -> Markup {
    html! { @if let Some(e) = p.get("err") { div.alert.bad.dv-err { (icon("circle-alert")) span { (e) } } } }
}

/// An icon from the name people gave it ("Calefactor salón" is a flame), else by kind.
fn dev_icon(d: &Device) -> &'static str {
    if d.driver == "broadlink" {
        return "tv";
    }
    let n = d.name.to_lowercase();
    const KW: &[(&str, &str)] = &[
        ("calefac", "flame"), ("estufa", "flame"), ("radiador", "flame"), ("brasero", "flame"),
        ("ventilador", "fan"), ("aire", "wind"), ("humidif", "droplets"), ("termo", "droplets"),
        ("lámpara", "lamp"), ("lampara", "lamp"), ("flexo", "lamp"), ("luz", "lightbulb"), ("bombilla", "lightbulb"),
        ("tele", "tv"), ("tv", "tv"), ("lavadora", "washing-machine"), ("secadora", "washing-machine"),
        ("cafe", "coffee"), ("café", "coffee"), ("arrocera", "chef-hat"), ("tostadora", "chef-hat"), ("horno", "chef-hat"),
        ("freidora", "chef-hat"), ("ordenador", "laptop"), ("setup", "laptop"), ("portátil", "laptop"), ("cama", "bed"),
    ];
    KW.iter().find(|(k, _)| n.contains(k)).map(|(_, i)| *i).unwrap_or(if is_light(&d.category) { "lightbulb" } else { "plug" })
}

fn spark(hist: &[f32]) -> Markup {
    if hist.len() < 2 || hist.iter().all(|v| *v <= 0.0) {
        return html! {};
    }
    let max = hist.iter().cloned().fold(1.0f32, f32::max);
    let step = 100.0 / (hist.len() - 1) as f32;
    let pts: Vec<String> = hist.iter().enumerate().map(|(i, v)| format!("{:.1},{:.1}", i as f32 * step, 30.0 - v / max * 27.0)).collect();
    let line = pts.join(" ");
    html! {
        svg.dv-spark viewBox="0 0 100 30" preserveAspectRatio="none" aria-hidden="true" {
            polygon points={ "0,30 " (line) " 100,30" } {}
            polyline points=(line) {}
        }
    }
}

/// Bars for a series of (tooltip, Wh); the last one is "now".
fn bars(vals: &[(String, f64)], ticks: [&str; 3], label: &str) -> Markup {
    let max = vals.iter().map(|v| v.1).fold(0.0, f64::max);
    let n = vals.len().max(1) as f64;
    html! {
        div.dv-chart {
            @if max > 0.0 { span.dv-chart-max { (kwh(max)) } }
            svg viewBox={ "0 0 " (n * 10.0) " 100" } preserveAspectRatio="none" role="img" aria-label=(label) {
                @for (i, (t, v)) in vals.iter().enumerate() {
                    @let h = if max > 0.0 { (v / max * 94.0).max(if *v > 0.0 { 2.5 } else { 1.0 }) } else { 1.0 };
                    rect.now[i + 1 == vals.len()].zero[*v <= 0.0] x=(i as f64 * 10.0 + 1.5) y=(100.0 - h) width="7" height=(h) rx="1.2" {
                        title { (t) ": " (kwh(*v)) }
                    }
                }
            }
            div.dv-ticks { span { (ticks[0]) } span { (ticks[1]) } span { (ticks[2]) } }
        }
    }
}

/// Last 30 days of energy, of one device (`dev` > 0) or of all.
fn days30(c: &Connection, dev: i64) -> rusqlite::Result<Vec<(String, f64)>> {
    c.prepare(
        "WITH RECURSIVE d(x) AS (SELECT date('now','localtime','-29 days') UNION ALL SELECT date(x,'+1 day') FROM d WHERE x < date('now','localtime'))
         SELECT strftime('%d/%m', x), coalesce((SELECT sum(wh) FROM energy_hour WHERE substr(hour,1,10)=x AND (?1=0 OR device=?1)),0) FROM d",
    )?
    .query_map([dev], |r| Ok((r.get(0)?, r.get(1)?)))?
    .collect()
}

fn hours_today(c: &Connection, dev: i64) -> rusqlite::Result<Vec<(String, f64)>> {
    c.prepare(
        "WITH RECURSIVE h(n) AS (SELECT 0 UNION ALL SELECT n+1 FROM h WHERE n<23)
         SELECT printf('%02d:00',n), coalesce((SELECT sum(wh) FROM energy_hour WHERE hour=date('now','localtime')||' '||printf('%02d',n) AND (?1=0 OR device=?1)),0) FROM h",
    )?
    .query_map([dev], |r| Ok((r.get(0)?, r.get(1)?)))?
    .collect()
}

/// (today, this month) in Wh.
fn totals(c: &Connection, dev: i64) -> rusqlite::Result<(f64, f64)> {
    c.query_row(
        "SELECT coalesce(sum(CASE WHEN substr(hour,1,10)=date('now','localtime') THEN wh END),0),
                coalesce(sum(CASE WHEN hour>=date('now','localtime','start of month') THEN wh END),0)
         FROM energy_hour WHERE ?1=0 OR device=?1",
        [dev],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
}

fn power_form(d: &Device, dp: &str, on: bool, class: &str, label: Option<String>) -> Markup {
    html! {
        form method="post" action={ "/plugins/devices/" (d.id) "/toggle" } {
            input type="hidden" name="dp" value=(dp);
            button class={ (class) @if on { " on" } } type="submit" aria-pressed=(on) aria-label={ (if on { "Apagar " } else { "Encender " }) (d.name) } {
                (icon("power"))
                @if let Some(l) = label { span { (l) } }
            }
        }
    }
}

fn list(c: &Connection, app: &App, p: &Params) -> rusqlite::Result<Markup> {
    let all = devices(c)?;
    let live = lock(&app.live);
    let now_w: f64 = all.iter().filter_map(|d| watts(d, &live.get(&d.id)?.dps)).sum();
    let on_count = all.iter().filter(|d| live.get(&d.id).is_some_and(|l| l.online && switches(d, &l.dps).iter().any(|k| l.dps[k] == Value::Bool(true)))).count();
    let (today, month) = totals(c, 0)?;
    let pr = price(c);
    let ir_count: Vec<i64> = ir::buttons(c, 0)?.into_iter().map(|b| b.device).collect();
    let metered = all.iter().any(|d| live.get(&d.id).is_some_and(|l| watts(d, &l.dps).is_some())) || month > 0.0;
    let mut rooms: Vec<(&str, Vec<&Device>)> = Vec::new();
    for d in &all {
        match rooms.iter_mut().find(|(r, _)| *r == d.room) {
            Some((_, v)) => v.push(d),
            None => rooms.push((&d.room, vec![d])),
        }
    }
    Ok(html! {
        span hidden data-refresh="5" {}
        div.page-head {
            div {
                h1 { "Domótica" }
                p {
                    @if all.is_empty() { "Los aparatos de casa, sin la app de cada marca" }
                    @else {
                        b { (all.len()) } " aparato" @if all.len() != 1 { "s" }
                        @if on_count > 0 { " · " b { (on_count) } " encendido" @if on_count != 1 { "s" } }
                    }
                }
            }
            a.btn.primary href="/?tab=devices&add=1" { (icon("plus")) "Añadir" }
        }
        (alert(p))
        @if all.is_empty() {
            section.card { div.empty {
                span.empty-ico { (icon("plug")) }
                h3 { "Aún no hay aparatos" }
                p { "Enchufes, bombillas e interruptores de Smart Life, Gosund, Teckin, Woox… se controlan aquí sin salir de casa." }
                a.btn.outline href="/?tab=devices&add=1" { (icon("search")) "Buscar aparatos" }
            } }
        } @else {
            @if metered {
                a.dv-energy href="/?tab=devices&energy=1" {
                    div.dv-en-now {
                        span.dv-en-ico { (icon("zap")) }
                        div { small { "Ahora" } b { (w(now_w)) } }
                    }
                    div.dv-en-m { small { "Hoy" } b { (kwh(today)) } span { (eur(today / 1000.0 * pr)) } }
                    div.dv-en-m { small { "Este mes" } b { (kwh(month)) } span { (eur(month / 1000.0 * pr)) } }
                    span.dv-en-go { "Consumo" (icon("chevron-right")) }
                }
            }
            (scenes::strip(c)?)
            @for (room, list) in &rooms {
                div.dv-sec-head { h2 { @if room.is_empty() { @if rooms.len() > 1 { "Sin habitación" } @else { "Aparatos" } } @else { (room) } } }
                div.dv-grid {
                    @for d in list {
                        @let l = live.get(&d.id);
                        @let dps = l.map(|l| l.dps.clone()).unwrap_or_default();
                        @let online = l.is_some_and(|l| l.online);
                        @let sw = switches(d, &dps);
                        @let any_on = online && sw.iter().any(|k| dps[k] == Value::Bool(true));
                        @let watt = online.then(|| watts(d, &dps)).flatten();
                        article.dv-card.on[any_on].off[!online] {
                            div.dv-card-top {
                                a.dv-ico href={ "/?tab=devices&d=" (d.id) } aria-hidden="true" tabindex="-1" { (icon(dev_icon(d))) }
                                @if online && sw.len() == 1 { (power_form(d, &sw[0], any_on, "dv-power", None)) }
                            }
                            a.dv-card-main href={ "/?tab=devices&d=" (d.id) } {
                                h3 { (d.name) }
                                p.dv-sub {
                                    @if !online { span.dv-dot-off {} @if l.is_none() { "Conectando…" } @else { "Sin conexión" } }
                                    @else if d.driver == "broadlink" { @let n = ir_count.iter().filter(|x| **x == d.id).count(); (n) " botón" @if n != 1 { "es" } }
                                    @else if let Some(v) = watt.filter(|_| any_on) { b { (w(v)) } }
                                    @else if any_on { "Encendido" } @else { "Apagado" }
                                }
                            }
                            @if any_on { @if let Some(l) = l { (spark(&l.hist.iter().cloned().collect::<Vec<_>>())) } }
                            @if online && sw.len() > 1 {
                                div.dv-sws {
                                    @for (i, k) in sw.iter().enumerate() {
                                        (power_form(d, k, dps[k] == Value::Bool(true), "dv-sw", Some((i + 1).to_string())))
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    })
}

/// The driver's error in words of the house, and what to do about it.
fn plain_err(e: &str) -> &'static str {
    if e == "bloqueado" || e.contains("bloqueado en la app") {
        "Está bloqueado en la app Broadlink: entra en sus Propiedades y desactiva «Bloquear dispositivo»."
    } else if e.starts_with("sin IP") {
        "Aún no lo hemos encontrado en tu red. Si está enchufado, aparecerá en un minuto."
    } else if e.contains("clave") || e.contains("descifrar") {
        "Su llave ya no encaja (cambia si lo vuelves a emparejar). Sincroniza otra vez con tu cuenta."
    } else {
        "No contesta. ¿Está enchufado y conectado a la misma WiFi que Nido?"
    }
}

fn detail(c: &Connection, app: &App, d: &Device, p: &Params) -> rusqlite::Result<Markup> {
    let (today, month) = totals(c, d.id)?;
    let pr = price(c);
    let live = lock(&app.live);
    let l = live.get(&d.id);
    let online = l.is_some_and(|l| l.online);
    let dps = l.map(|l| l.dps.clone()).unwrap_or_default();
    let sw = switches(d, &dps);
    let any_on = online && sw.iter().any(|k| dps[k] == Value::Bool(true));
    let watt = online.then(|| watts(d, &dps)).flatten();
    let metered = watt.is_some() || month > 0.0;
    Ok(html! {
        span hidden data-refresh="5" {}
        div.page-head {
            div {
                a.back href="/?tab=devices" { (icon("chevron-left")) "Domótica" }
                h1 { (d.name) }
                p { @if d.room.is_empty() { "Sin habitación" } @else { (d.room) } }
            }
        }
        (alert(p))
        section.card.dv-hero.on[any_on].off[!online] {
            div.dv-hero-top {
                span.dv-ico.big { (icon(dev_icon(d))) }
                div.dv-hero-txt {
                    b { @if !online { "Sin conexión" } @else if any_on { "Encendido" } @else { "Apagado" } }
                    @if let Some(v) = watt { span { "Consume " b { (w(v)) } " ahora" } }
                    @else if let Some(l) = l.filter(|l| !l.online && !l.err.is_empty()) { span { (plain_err(&l.err)) } }
                }
                @if online && sw.len() == 1 { (power_form(d, &sw[0], any_on, "dv-power big", None)) }
            }
            @if online && sw.len() > 1 {
                div.dv-sws {
                    @for (i, k) in sw.iter().enumerate() { (power_form(d, k, dps[k] == Value::Bool(true), "dv-sw", Some(format!("Toma {}", i + 1)))) }
                }
            }
            @if let Some(l) = l { (spark(&l.hist.iter().cloned().collect::<Vec<_>>())) }
        }
        @if metered {
            div.dv-stats {
                div.card { span.label-sm { "Hoy" } b { (kwh(today)) } small { (eur(today / 1000.0 * pr)) } }
                div.card { span.label-sm { "Este mes" } b { (kwh(month)) } small { (eur(month / 1000.0 * pr)) } }
            }
            section.card.dv-chart-card {
                h2.dv-card-t { "Hoy, por horas" }
                (bars(&hours_today(c, d.id)?, ["00:00", "12:00", "23:00"], "Consumo de hoy por horas"))
            }
            section.card.dv-chart-card {
                h2.dv-card-t { "Últimos 30 días" }
                @let days = days30(c, d.id)?;
                (bars(&days, [days[0].0.as_str(), days[15].0.as_str(), "Hoy"], "Consumo de los últimos 30 días"))
            }
        }
        details.card.dv-settings {
            summary { (icon("settings")) "Ajustes del aparato" (icon("chevron-down")) }
            form.form method="post" action={ "/plugins/devices/" (d.id) } {
                div.two {
                    div.field { label for="name" { "Nombre" } input #name.input name="name" value=(d.name) required; }
                    div.field { label for="room" { "Habitación" } input #room.input name="room" value=(d.room) placeholder="Salón, cocina…" list="dv-rooms"; }
                }
                (rooms_list(c)?)
                (advanced(&d.ip, &d.version, Some((&d.uid, &d.secret))))
                div { button.btn.primary type="submit" { "Guardar" } }
            }
            form.dv-del method="post" action={ "/plugins/devices/" (d.id) "/delete" } {
                button.btn.danger type="submit" { (icon("trash-2")) "Quitar de Nido" }
            }
        }
    })
}

const MONTHS: [&str; 12] = ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];

fn month_name(m: &str) -> String {
    let i: usize = m.get(5..7).and_then(|s| s.parse().ok()).unwrap_or(1);
    format!("{} {}", MONTHS[(i.clamp(1, 12)) - 1], &m[..4.min(m.len())])
}

/// Energy history, month by month: `m=YYYY-MM` picks the month (this one by default).
fn energy(c: &Connection, app: &App, p: &Params) -> rusqlite::Result<Markup> {
    let this: String = c.query_row("SELECT strftime('%Y-%m','now','localtime')", [], |r| r.get(0))?;
    let m = p
        .get("m")
        .filter(|m| m.len() == 7 && m.as_bytes()[4] == b'-' && m[..4].parse::<u16>().is_ok() && m[5..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)) && **m <= this)
        .cloned()
        .unwrap_or(this.clone());
    let current = m == this;
    let (prev, next): (String, String) = c.query_row(
        "SELECT strftime('%Y-%m',date(?1||'-01','-1 month')), strftime('%Y-%m',date(?1||'-01','+1 month'))",
        [&m],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let first: Option<String> = c.query_row("SELECT min(substr(hour,1,7)) FROM energy_hour", [], |r| r.get(0))?;
    let has_prev = first.is_some_and(|f| f < m);
    let pr = price(c);
    let month_wh = |dev: i64| -> rusqlite::Result<f64> {
        c.query_row("SELECT coalesce(sum(wh),0) FROM energy_hour WHERE substr(hour,1,7)=?1 AND (?2=0 OR device=?2)", params![m, dev], |r| r.get(0))
    };
    let total = month_wh(0)?;
    let days: Vec<(String, f64)> = c
        .prepare(
            "WITH RECURSIVE d(x) AS (SELECT ?1||'-01' UNION ALL SELECT date(x,'+1 day') FROM d WHERE x < date(?1||'-01','+1 month','-1 day') AND x < date('now','localtime'))
             SELECT strftime('%d/%m',x), coalesce((SELECT sum(wh) FROM energy_hour WHERE substr(hour,1,10)=x),0) FROM d",
        )?
        .query_map([&m], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let year: Vec<(String, f64)> = c
        .prepare(
            "WITH RECURSIVE mm(x) AS (SELECT date(?1||'-01','-11 months') UNION ALL SELECT date(x,'+1 month') FROM mm WHERE x < ?1||'-01')
             SELECT strftime('%Y-%m',x), coalesce((SELECT sum(wh) FROM energy_hour WHERE substr(hour,1,7)=strftime('%Y-%m',x)),0) FROM mm",
        )?
        .query_map([&m], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let year_named: Vec<(String, f64)> = year.iter().map(|(k, v)| (month_name(k), *v)).collect();
    let all = devices(c)?;
    let mut rank: Vec<(&Device, f64)> = all.iter().map(|d| Ok((d, month_wh(d.id)?))).collect::<rusqlite::Result<_>>()?;
    rank.retain(|(_, v)| *v > 0.0);
    rank.sort_by(|a, b| b.1.total_cmp(&a.1));
    let top = rank.first().map(|r| r.1).unwrap_or(1.0);
    let live = lock(&app.live);
    let now_w: f64 = all.iter().filter_map(|d| watts(d, &live.get(&d.id)?.dps)).sum();
    let today = totals(c, 0)?.0;
    let n_days = days.len().max(1) as f64;
    let in_month: f64 = c.query_row("SELECT CAST(strftime('%d',date(?1||'-01','+1 month','-1 day')) AS REAL)", [&m], |r| r.get(0))?;
    let forecast = if n_days > 1.0 { (total - today) / (n_days - 1.0) * in_month } else { total * in_month };
    let tick = |i: usize| days.get(i).map(|d| d.0.as_str()).unwrap_or("");
    let short = |k: &str| month_name(k).chars().take(3).collect::<String>();
    Ok(html! {
        @if current { span hidden data-refresh="10" {} }
        div.page-head {
            div {
                a.back href="/?tab=devices" { (icon("chevron-left")) "Domótica" }
                h1 { "Consumo" }
                p { "Lo que gastan los aparatos que miden la luz" }
            }
        }
        (alert(p))
        nav.dv-month aria-label="Mes" {
            @if has_prev { a.icon-btn href={ "/?tab=devices&energy=1&m=" (prev) } aria-label="Mes anterior" { (icon("chevron-left")) } }
            @else { span.icon-btn.dis aria-hidden="true" { (icon("chevron-left")) } }
            b { (month_name(&m)) }
            @if !current { a.icon-btn href={ "/?tab=devices&energy=1&m=" (next) } aria-label="Mes siguiente" { (icon("chevron-right")) } }
            @else { span.icon-btn.dis aria-hidden="true" { (icon("chevron-right")) } }
        }
        div.dv-stats.four {
            @if current {
                div.card { span.label-sm { "Ahora" } b { (w(now_w)) } small { "entre todos" } }
                div.card { span.label-sm { "Hoy" } b { (kwh(today)) } small { (eur(today / 1000.0 * pr)) } }
                div.card { span.label-sm { "Este mes" } b { (kwh(total)) } small { (eur(total / 1000.0 * pr)) } }
                div.card { span.label-sm { "Previsión del mes" } b { (eur(forecast / 1000.0 * pr)) } small { "≈ " (kwh(forecast)) } }
            } @else {
                div.card { span.label-sm { "Total" } b { (kwh(total)) } small { "en " (MONTHS[m[5..].parse::<usize>().unwrap_or(1) - 1]) } }
                div.card { span.label-sm { "Coste" } b { (eur(total / 1000.0 * pr)) } small { "a " (eur(pr)) "/kWh" } }
                div.card { span.label-sm { "Media diaria" } b { (kwh(total / n_days)) } small { (eur(total / n_days / 1000.0 * pr)) } }
                div.card { span.label-sm { "Aparatos" } b { (rank.len()) } small { "con consumo" } }
            }
        }
        section.card.dv-chart-card {
            h2.dv-card-t { "Día a día" }
            (bars(&days, [tick(0), tick(days.len() / 2), if current { "Hoy" } else { tick(days.len().saturating_sub(1)) }], "Consumo diario del mes"))
        }
        section.card.dv-chart-card {
            h2.dv-card-t { "Últimos 12 meses" }
            (bars(&year_named, [short(&year[0].0).as_str(), short(&year[6].0).as_str(), short(&year[11].0).as_str()], "Consumo de los últimos 12 meses"))
            div.dv-months {
                @for (k, v) in year.iter().rev().filter(|(_, v)| *v > 0.0).take(6) {
                    a.dv-month-row.on[*k == m] href={ "/?tab=devices&energy=1&m=" (k) } {
                        span { (month_name(k)) } span { (kwh(*v)) } b { (eur(v / 1000.0 * pr)) }
                    }
                }
            }
        }
        section.card {
            h2.dv-card-t { "Quién gasta más" @if !current { " en " (MONTHS[m[5..].parse::<usize>().unwrap_or(1) - 1]) } }
            @if rank.is_empty() { p.hint { "Aún no hay datos: se van sumando mientras los aparatos están encendidos." } }
            div.dv-rank {
                @for (d, v) in &rank {
                    a.dv-rank-row href={ "/?tab=devices&d=" (d.id) } {
                        span.dv-ico { (icon(dev_icon(d))) }
                        div.dv-rank-main {
                            div.dv-rank-top { b { (d.name) } span { (kwh(*v)) " · " (eur(v / 1000.0 * pr)) } }
                            div.dv-rank-bar { span style={ "width:" (format!("{:.1}", v / top * 100.0)) "%" } {} }
                        }
                    }
                }
            }
        }
        section.card {
            h2.dv-card-t { "Precio de la luz" }
            form.dv-price method="post" action="/plugins/devices/price" {
                div.field {
                    label for="kp" { "Euros por kWh" }
                    input #kp.input type="number" name="price" value=(format!("{pr:.3}")) step="0.001" min="0.01" max="2" inputmode="decimal";
                }
                button.btn.outline type="submit" { "Guardar" }
            }
            p.hint { "Lo pone en tu factura de la luz; suele estar entre 0,10 y 0,25 €." }
        }
    })
}

fn rooms_list(c: &Connection) -> rusqlite::Result<Markup> {
    let rooms: Vec<String> = c
        .prepare("SELECT DISTINCT room FROM device WHERE room<>'' ORDER BY room")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(html! { datalist #dv-rooms { @for r in &rooms { option value=(r) {} } } })
}

/// What only someone who knows Tuya needs: folded away.
fn advanced(ip: &str, ver: &str, id_key: Option<(&str, &str)>) -> Markup {
    html! {
        details.steps.dv-adv {
            summary { (icon("chevron-right")) "Opciones avanzadas" }
            div.form {
                @if let Some((uid, key)) = id_key {
                    div.field { label for="key" { "Clave local" } input #key.input name="key" value=(key) autocomplete="off" spellcheck="false" minlength="16" maxlength="16"; }
                    p.hint { "ID del dispositivo: " (uid) }
                }
                div.two {
                    div.field { label for="ip" { "Dirección IP" } input #ip.input name="ip" value=(ip) inputmode="decimal" placeholder="Automática"; }
                    div.field { label for="version" { "Protocolo" } (ver_select(ver)) }
                }
            }
        }
    }
}

/// The apps people know their plugs by. All are Tuya inside; only Smart Life and Tuya Smart can be
/// linked to a Tuya developer project, so the others go through Smart Life.
const APPS: &[(&str, &str, &str, bool)] = &[
    ("smartlife", "Smart Life", "La más habitual", true),
    ("tuya", "Tuya Smart", "La app de Tuya", true),
    ("teckin", "Teckin", "Enchufes Teckin", true),
    ("gosund", "Gosund", "App GHome o Gosund", false),
    ("woox", "Woox Home", "Enchufes y bombillas Woox", false),
    ("lidl", "Lidl Home", "Silvercrest, Livarno", false),
    ("other", "Otra app", "Nedis, LSC, Blitzwolf…", false),
];
const SOON: &[(&str, &str)] = &[("shelly", "Shelly"), ("hue", "Philips Hue"), ("zigbee", "Zigbee"), ("matter", "Matter")];

/// App icons (from each app's store page, 96 px), served by this plugin: the core carries none of them.
const LOGOS: &[(&str, &[u8])] = &[
    ("smartlife", include_bytes!("../logos/smartlife.webp")),
    ("tuya", include_bytes!("../logos/tuya.webp")),
    ("teckin", include_bytes!("../logos/teckin.webp")),
    ("gosund", include_bytes!("../logos/gosund.webp")),
    ("woox", include_bytes!("../logos/woox.webp")),
    ("lidl", include_bytes!("../logos/lidl.webp")),
    ("shelly", include_bytes!("../logos/shelly.webp")),
    ("hue", include_bytes!("../logos/hue.webp")),
    ("broadlink", include_bytes!("../logos/broadlink.webp")),
];

fn logo(id: &str) -> Markup {
    html! {
        @if LOGOS.iter().any(|(n, _)| *n == id) {
            img.dv-logo src={ "/plugins/devices/logos/" (id) ".webp" } alt="" width="44" height="44" loading="lazy" decoding="async";
        } @else {
            span.dv-logo.none { (icon(if id == "other" { "plug" } else { "layers" })) }
        }
    }
}

async fn logo_file(Path(f): Path<String>) -> Response {
    match LOGOS.iter().find(|(n, _)| Some(*n) == f.strip_suffix(".webp")) {
        Some((_, b)) => ([(header::CONTENT_TYPE, "image/webp"), (header::CACHE_CONTROL, "public, max-age=604800")], *b).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn setting(c: &Connection, k: &str) -> String {
    c.query_row("SELECT v FROM setting WHERE k=?1", [k], |r| r.get(0)).optional().ok().flatten().unwrap_or_default()
}

fn add_view(c: &Connection, app: &App, p: &Params) -> rusqlite::Result<Markup> {
    match p.get("add").map(String::as_str) {
        Some("tuya") => return Ok(qr_wizard(app, p)),
        Some("dev") => return Ok(dev_wizard(c, p)),
        Some("manual") => return manual(c, p),
        Some("broadlink") => return ir::add_page(app, c, p),
        _ => {}
    }
    let have: Vec<String> = devices(c)?.into_iter().map(|d| d.uid).collect();
    let n = lock(&app.seen).values().filter(|s| !have.contains(&s.id)).count();
    let linked = !setting(c, "tuya_session").is_empty() || !setting(c, "tuya_id").is_empty();
    Ok(html! {
        span hidden data-refresh="5" {}
        div.page-head {
            div {
                a.back href="/?tab=devices" { (icon("chevron-left")) "Domótica" }
                h1 { "Añadir dispositivos" }
                p { "Enchufes, bombillas e interruptores de casa" }
            }
        }
        (alert(p))
        section.card.dv-scan {
            div.dv-radar.found[n > 0] aria-hidden="true" { span {} span {} (icon("plug")) }
            div.dv-scan-txt {
                h2 {
                    @if n == 0 { "Buscando en tu red…" }
                    @else if n == 1 { "Hay un aparato nuevo en tu red" }
                    @else { "Hay " (n) " aparatos nuevos en tu red" }
                }
                p {
                    @if linked { "Tu cuenta está conectada: los aparatos nuevos aparecen solos en unos minutos." }
                    @else if n == 0 { "Los aparatos encendidos aparecen aquí solos. Mientras, dinos con qué app los usas." }
                    @else { "Para controlarlo, Nido necesita su llave. Elige abajo la app donde lo tienes y te guiamos." }
                }
            }
            @if linked {
                form method="post" action="/plugins/devices/sync" {
                    button.btn.outline.sm type="submit" { (icon("refresh-cw")) "Sincronizar ahora" }
                }
            }
        }
        h2.dv-section { @if linked { "Conectar otra cuenta" } @else { "¿Con qué app los usas ahora?" } }
        div.dv-apps {
            @for (id, name, sub, _) in APPS {
                a.dv-app href={ "/?tab=devices&add=tuya&app=" (id) } {
                    (logo(id))
                    span.dv-app-txt { b { (name) } small { (sub) } }
                    (icon("chevron-right"))
                }
            }
        }
        h2.dv-section { "Mandos por infrarrojos" }
        div.dv-apps {
            a.dv-app href="/?tab=devices&add=broadlink" {
                (logo("broadlink"))
                span.dv-app-txt { b { "Broadlink" } small { "RM mini, RM4: tele, aire, ventilador…" } }
                (icon("chevron-right"))
            }
        }
        h2.dv-section { "Muy pronto" }
        div.dv-apps.soon {
            @for (id, name) in SOON {
                div.dv-app aria-disabled="true" {
                    (logo(id))
                    span.dv-app-txt { b { (name) } small { "En camino" } }
                }
            }
        }
        p.dv-foot { a href="/?tab=devices&add=manual" { (icon("key-round")) "Ya tengo la clave local de un aparato" } }
    })
}

fn qr_svg(text: &str) -> String {
    use qrcode::{render::svg, EcLevel, QrCode};
    let Ok(code) = QrCode::with_error_correction_level(text, EcLevel::Q) else { return String::new() };
    let s = code.render::<svg::Color>().min_dimensions(240, 240).dark_color(svg::Color("#000")).light_color(svg::Color("#fff")).build();
    s.find("<svg").map(|i| s[i..].to_string()).unwrap_or(s)
}

/// The easy way: the user code from the app, then a QR scanned with the app. Two screens.
fn qr_wizard(app: &App, p: &Params) -> Markup {
    let a = APPS.iter().find(|a| Some(a.0) == p.get("app").map(String::as_str)).unwrap_or(&APPS[0]);
    let (id, name, _, _) = *a;
    let scan_with = match id { "other" => "tu app", "teckin" => "Teckin o Smart Life", _ => name };
    let qr = lock(&app.qr).clone();
    let step2 = p.get("step").is_some_and(|s| s == "2") && !qr.token.is_empty();
    html! {
        div.page-head {
            div {
                a.back href="/?tab=devices&add=1" { (icon("chevron-left")) "Añadir dispositivos" }
                h1.dv-title { (logo(id)) "Conectar con " (name) }
                p { "Menos de un minuto, solo la primera vez" }
            }
        }
        ol.dv-stepper aria-label="Pasos" {
            li.done[step2].now[!step2] aria-current=[(!step2).then_some("step")] {
                span.dv-dot { @if step2 { (icon("check")) } @else { "1" } } span.dv-step-l { "Tu código" }
            }
            li.now[step2] aria-current=[step2.then_some("step")] { span.dv-dot { "2" } span.dv-step-l { "Escanear" } }
        }
        @if !step2 {
            section.card.dv-step {
                h2 { "Copia tu código de usuario" }
                p.dv-lead { "Búscalo en " b { (scan_with) } ":" }
                ol.dv-path aria-label="Dónde encontrarlo" {
                    li { "Yo" } li { (icon("settings")) "Ajustes" } li { "Cuenta y seguridad" } li.on { "Código de usuario" }
                }
                (alert(p))
                form.form method="post" action="/plugins/devices/qr" {
                    input type="hidden" name="app" value=(id);
                    div.field {
                        label for="uc" { "Código de usuario" }
                        input #uc.input.dv-code name="user_code" required autocomplete="off" spellcheck="false" autocapitalize="none" placeholder="Pégalo aquí";
                        p.hint { "Cópialo tal cual: distingue mayúsculas y minúsculas." }
                    }
                    div.dv-actions { button.btn.primary type="submit" { "Continuar" (icon("chevron-right")) } }
                }
            }
            p.dv-foot { a href={ "/?tab=devices&add=dev&app=" (id) } { "¿No te funciona? Usa una cuenta de desarrollador de Tuya" } }
        } @else {
            section.card.dv-step.dv-scan-step {
                @match &qr.state {
                    QrState::Done(n) => {
                        div.dv-ok { (icon("circle-check")) }
                        h2 { "¡Conectado!" }
                        p.dv-lead { @if *n == 1 { "Hemos añadido 1 aparato." } @else { "Hemos añadido " (n) " aparatos." } " A partir de ahora se controlan desde casa, sin internet." }
                        a.btn.primary href="/?tab=devices" { "Ver mis aparatos" }
                    }
                    QrState::Failed(e) => {
                        h2 { "No ha salido bien" }
                        div.alert.bad.dv-err { (icon("circle-alert")) span { (e) } }
                        a.btn.primary href={ "/?tab=devices&add=tuya&app=" (id) } { (icon("refresh-cw")) "Probar otra vez" }
                    }
                    QrState::Waiting => {
                        span hidden data-refresh="3" {}
                        h2 { "Escanéalo con " (scan_with) }
                        div.dv-qr role="img" aria-label="Código QR para iniciar sesión" { (PreEscaped(qr_svg(&sharing::qr_text(&qr.token)))) }
                        ol.dv-todo {
                            li { span { "Abre " b { (scan_with) } " y ve a " b { "Yo" } "." } }
                            li { span { "Toca el icono de escanear, arriba a la derecha, y apunta a este código." } }
                            li { span { "Confirma el inicio de sesión en el móvil." } }
                        }
                        p.dv-wait { (icon("loader-circle")) "Esperando a que lo escanees…" }
                        p.hint.dv-same { "¿Estás viendo esto en el mismo móvil? Abre Nido en otra pantalla para escanearlo." }
                    }
                }
            }
        }
    }
}

fn dev_wizard(c: &Connection, p: &Params) -> Markup {
    let app = APPS.iter().find(|a| Some(a.0) == p.get("app").map(String::as_str)).unwrap_or(&APPS[0]);
    let (id, name, _, linkable) = *app;
    let scan_with = if id == "smartlife" || id == "tuya" { name } else { "Smart Life" };
    let step: u8 = p.get("step").and_then(|s| s.parse().ok()).filter(|s| (1..=3).contains(s)).unwrap_or(1);
    let go = |s: u8| format!("/?tab=devices&add=dev&app={id}&step={s}");
    let region = Some(setting(c, "tuya_region")).filter(|r| !r.is_empty()).unwrap_or("eu".into());
    let labels = ["Crear cuenta", "Enlazar la app", "Copiar llaves"];
    html! {
        div.page-head {
            div {
                a.back href={ "/?tab=devices&add=tuya&app=" (id) } { (icon("chevron-left")) "Conectar con " (name) }
                h1 { "Cuenta de desarrollador de Tuya" }
                p { "La vía larga, por si la del código QR no te funciona" }
            }
        }
        ol.dv-stepper aria-label="Pasos" {
            @for (i, l) in labels.iter().enumerate() {
                @let n = i as u8 + 1;
                li.done[n < step].now[n == step] aria-current=[(n == step).then_some("step")] {
                    span.dv-dot { @if n < step { (icon("check")) } @else { (n) } }
                    span.dv-step-l { (l) }
                }
            }
        }
        section.card.dv-step {
            @match step {
                1 => {
                    h2 { "Crea una cuenta gratis en Tuya" }
                    p.dv-lead { "Tuya es la empresa que hay detrás de " (name) ". Desde su web se consiguen las llaves con las que Nido habla con tus aparatos directamente, sin pasar por internet." }
                    ol.dv-todo {
                        li { "Regístrate en " b { "iot.tuya.com" } "." }
                        li { "Entra en " b { "Cloud → Development" } " y pulsa " b { "Create Cloud Project" } "." }
                        li { "Ponle cualquier nombre. En los desplegables elige " b { "Smart Home" } " y, como centro de datos, " b { "Central Europe" } "." }
                    }
                    div.dv-actions {
                        a.btn.outline href="https://iot.tuya.com" target="_blank" rel="noopener" { (icon("external-link")) "Abrir iot.tuya.com" }
                        a.btn.primary href=(go(2)) { "Ya lo tengo" (icon("chevron-right")) }
                    }
                }
                2 => {
                    h2 { "Enlaza tu app" }
                    @if !linkable {
                        div.dv-tip {
                            (icon("info"))
                            p { "La app " (name) " no se puede enlazar. Instala " b { "Smart Life" } " y añade allí tus aparatos: son compatibles y seguirán funcionando igual." }
                        }
                    }
                    ol.dv-todo {
                        li { "En tu proyecto de la web, abre " b { "Devices → Link App Account" } " y pulsa " b { "Add App Account" } ". Aparecerá un código QR." }
                        li { "En " b { (scan_with) } ", ve a " b { "Yo" } " y toca el icono de escanear, arriba a la derecha." }
                        li { "Escanea el código y confirma. Tus aparatos aparecerán en la web." }
                    }
                    div.dv-actions {
                        a.btn.ghost href=(go(1)) { (icon("chevron-left")) "Atrás" }
                        a.btn.primary href=(go(3)) { "Hecho" (icon("chevron-right")) }
                    }
                }
                _ => {
                    h2 { "Copia tus dos llaves" }
                    p.dv-lead { "En la pestaña " b { "Overview" } " del proyecto verás el Access ID y el Access Secret. Pégalos aquí." }
                    (alert(p))
                    form.form method="post" action="/plugins/devices/cloud" {
                        input type="hidden" name="from" value={ "&add=dev&app=" (id) "&step=3" };
                        div.field { label for="aid" { "Access ID" } input #aid.input name="access_id" autocomplete="off" spellcheck="false" autocapitalize="none" required; }
                        div.field { label for="asec" { "Access Secret" } input #asec.input type="password" name="access_secret" autocomplete="off" required; }
                        details.steps.dv-adv {
                            summary { (icon("chevron-right")) "¿Tu cuenta no es de Europa?" }
                            div.field {
                                label for="region" { "Centro de datos" }
                                select #region.input name="region" {
                                    @for (k, n, _) in tuya::REGIONS { option value=(k) selected[region == *k] { (n) } }
                                }
                            }
                        }
                        p.hint.dv-safe { (icon("lock")) "Solo se usan para leer tus aparatos y sus llaves. Después todo funciona en casa, sin internet." }
                        div.dv-actions {
                            a.btn.ghost href=(go(2)) { (icon("chevron-left")) "Atrás" }
                            button.btn.primary type="submit" { (icon("download")) "Conectar y traer aparatos" }
                        }
                    }
                }
            }
        }
    }
}

fn manual(c: &Connection, p: &Params) -> rusqlite::Result<Markup> {
    let pre = |k: &str| p.get(k).cloned().unwrap_or_default();
    Ok(html! {
        div.page-head {
            div {
                a.back href="/?tab=devices&add=1" { (icon("chevron-left")) "Añadir dispositivos" }
                h1 { "Añadir a mano" }
                p { "Si ya tienes el ID y la clave local del aparato" }
            }
        }
        (alert(p))
        section.card.dv-step {
            form.form method="post" action="/plugins/devices/add" {
                div.two {
                    div.field { label for="mname" { "Nombre" } input #mname.input name="name" placeholder="Enchufe del salón"; }
                    div.field { label for="mroom" { "Habitación" } input #mroom.input name="room" list="dv-rooms"; }
                }
                (rooms_list(c)?)
                div.field { label for="muid" { "ID del dispositivo" } input #muid.input name="uid" value=(pre("uid")) required spellcheck="false" autocapitalize="none"; }
                div.field {
                    label for="mkey" { "Clave local" }
                    input #mkey.input name="key" required autocomplete="off" spellcheck="false" autocapitalize="none" minlength="16" maxlength="16";
                    p.hint { "16 caracteres." }
                }
                (advanced(&pre("ip"), &Some(pre("ver")).filter(|v| !v.is_empty()).unwrap_or("3.3".into()), None))
                div { button.btn.primary type="submit" { (icon("plus")) "Añadir" } }
            }
        }
    })
}

async fn page(State(app): State<App>, h: HeaderMap, Query(p): Query<Params>) -> Response {
    remember_people(&app, &h);
    let c = lock(&app.db);
    let body = if p.contains_key("add") {
        add_view(&c, &app, &p)
    } else if let Some(d) = p.get("d").and_then(|s| s.parse().ok()).and_then(|id| one(&c, id).ok().flatten()) {
        if d.driver == "broadlink" { ir::detail(&c, &app, &d, &p) } else { detail(&c, &app, &d, &p) }
    } else if let Some(s) = p.get("scene") {
        scenes::editor(&c, &app, s.parse().ok())
    } else if p.contains_key("energy") {
        energy(&c, &app, &p)
    } else {
        list(&c, &app, &p)
    };
    match body {
        Ok(b) => Html(html! { style { (PreEscaped(CSS)) } (b) }.into_string()).into_response(),
        Err(e) => {
            eprintln!("error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "Error interno").into_response()
        }
    }
}

async fn manifest() -> Response {
    let flashes: Map<String, Value> = FLASHES.iter().map(|(k, v)| (k.to_string(), (*v).into())).collect();
    let body = serde_json::json!({ "id": "devices", "version": VERSION, "flashes": flashes });
    ([(header::CONTENT_TYPE, "application/json")], body.to_string()).into_response()
}

async fn guard(State(key): State<Arc<String>>, req: Request, next: Next) -> Response {
    if req.headers().get("x-nido-key").and_then(|v| v.to_str().ok()) == Some(key.as_str()) {
        next.run(req).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        return println!("{VERSION}");
    }
    let env = |k: &str, default: &str| std::env::var(k).unwrap_or_else(|_| default.to_string());
    let key = std::env::var("NIDO_PLUGIN_KEY").unwrap_or_else(|_| {
        eprintln!("nido-devices lo arranca Nido (Plugins); falta NIDO_PLUGIN_KEY");
        std::process::exit(2);
    });
    let conn = Connection::open(env("NIDO_PLUGIN_DB", "devices.db")).expect("no se pudo abrir la base de datos");
    let _ = conn.busy_timeout(Duration::from_secs(5));
    conn.execute_batch(include_str!("../schema.sql")).expect("tablas");
    let _ = conn.execute("ALTER TABLE ir_button ADD COLUMN icon TEXT NOT NULL DEFAULT ''", []);
    let app = App { db: Arc::new(Mutex::new(conn)), live: Default::default(), seen: Default::default(), qr: Default::default(), people: Default::default(), notes: Default::default(), want_sync: Default::default(), bl: Default::default(), learn: Default::default() };
    let router = Router::new()
        .route("/manifest", get(manifest))
        .route("/page", get(page))
        .route("/add", post(add))
        .route("/cloud", post(cloud))
        .route("/qr", post(qr_start))
        .route("/sync", post(sync))
        .route("/logos/{file}", get(logo_file))
        .route("/notices", get(notices))
        .route("/broadlink/scan", post(ir::scan))
        .route("/broadlink/add", post(ir::add))
        .route("/ir/{id}/new", post(ir::new_button))
        .route("/ir/press/{id}", post(ir::press_now))
        .route("/ir/button/{id}/relearn", post(ir::relearn))
        .route("/ir/button/{id}", post(ir::edit_button))
        .route("/ir/button/{id}/delete", post(ir::delete_button))
        .route("/notices/late", get(notices_late))
        .route("/price", post(set_price))
        .route("/scenes", post(scenes::save))
        .route("/scenes/{id}/run", post(scenes::run_now))
        .route("/scenes/{id}/delete", post(scenes::delete))
        .route("/{id}", post(save))
        .route("/{id}/toggle", post(toggle))
        .route("/{id}/delete", post(delete))
        .layer(middleware::from_fn_with_state(Arc::new(key), guard))
        .with_state(app.clone());
    let addr = env("NIDO_PLUGIN_ADDR", "127.0.0.1:8183");
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("no se pudo abrir el puerto");
    println!("nido-devices {VERSION} en http://{addr}");
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let a = app.clone();
            tuya::discover(move |s| found(&a, s)).await;
            tokio::task::spawn_local(hunt(app.clone()));
            tokio::task::spawn_local(autosync(app.clone()));
            tokio::task::spawn_local(poll(app));
            axum::serve(listener, router).await.expect("servidor");
        })
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities() {
        let plug = Device { id: 1, driver: "tuya".into(), uid: "a".into(), name: "E".into(), room: "".into(), category: "cz".into(), ip: "".into(), secret: "".into(), version: "3.3".into() };
        let dps: Map<String, Value> = serde_json::from_str(r#"{"1":true,"9":0,"19":1234,"41":false}"#).unwrap();
        assert_eq!(switches(&plug, &dps), vec!["1"]);
        assert_eq!(watts(&plug, &dps), Some(123.4));
        let bulb = Device { category: "dj".into(), ..plug.clone() };
        let dps: Map<String, Value> = serde_json::from_str(r#"{"20":false,"22":500}"#).unwrap();
        assert_eq!((switches(&bulb, &dps), watts(&bulb, &dps)), (vec!["20".to_string()], None));
        let odd = Device { category: "xyz".into(), ..plug };
        let dps: Map<String, Value> = serde_json::from_str(r#"{"101":true,"7":false}"#).unwrap();
        assert_eq!(switches(&odd, &dps), vec!["7", "101"]);
        assert_eq!(kwh(1500.0), "1,50 kWh");
    }
}
