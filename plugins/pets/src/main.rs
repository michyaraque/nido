//! nido-pets: the "Mascotas" plugin of Nido, as a program of its own.
//!
//! Nido starts it when the house activates the plugin and talks to it over HTTP on 127.0.0.1
//! (see plugins/README.md). It keeps each pet's weight, medication, health log, walks, vet visits
//! and chores in its own database, and reminds the house of doses, visits and chores through
//! `/notices` (banner while Nido is open) and `/notices/late` (push or Telegram).
use axum::{
    extract::{Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

mod act;
mod data;
mod kit;
mod view;

use kit::*;

pub const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

pub type Db = Arc<Mutex<Connection>>;
pub type Params = std::collections::HashMap<String, String>;

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone)]
pub struct App {
    pub db: Db,
    notes: Arc<Mutex<Vec<Note>>>,
}

struct Note {
    text: String,
    href: String,
    pending: Vec<i64>,
    at: Instant,
}

pub struct AppErr(pub String);
impl<E: std::fmt::Display> From<E> for AppErr {
    fn from(e: E) -> Self {
        AppErr(e.to_string())
    }
}
impl IntoResponse for AppErr {
    fn into_response(self) -> Response {
        eprintln!("error: {}", self.0);
        (StatusCode::INTERNAL_SERVER_ERROR, "Algo ha fallado. Vuelve a intentarlo.").into_response()
    }
}
pub type R = Result<Response, AppErr>;

#[derive(Clone)]
pub struct Person {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub photo: i64,
}

/// Who is asking and everyone in the house, as Nido tells it (it has already checked the session).
pub struct Me {
    pub id: i64,
    pub name: String,
    pub people: Vec<Person>,
}

pub fn me(h: &HeaderMap) -> Option<Me> {
    let get = |k: &str| h.get(k).and_then(|v| v.to_str().ok());
    let id = get("x-nido-user")?.parse().ok()?;
    let people = get("x-nido-people")
        .unwrap_or("")
        .split(',')
        .filter_map(|p| {
            let mut it = p.splitn(4, '=');
            Some(Person {
                id: it.next()?.parse().ok()?,
                name: pct_decode(it.next()?),
                color: pct_decode(it.next().unwrap_or("")),
                photo: it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            })
        })
        .collect();
    Some(Me { id, name: pct_decode(get("x-nido-name").unwrap_or("")), people })
}

/// Background reminders need the household, which only `/page` requests carry: keep the ids.
fn remember_people(c: &Connection, me: &Me) {
    let ids: Vec<String> = me.people.iter().map(|p| p.id.to_string()).collect();
    if !ids.is_empty() {
        let _ = c.execute("INSERT OR REPLACE INTO setting(k,v) VALUES('people',?1)", [ids.join(",")]);
    }
}

fn people_ids(c: &Connection) -> Vec<i64> {
    c.query_row("SELECT v FROM setting WHERE k='people'", [], |r| r.get::<_, String>(0))
        .optional()
        .ok()
        .flatten()
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| s.parse().ok())
        .collect()
}

/// Now on the server's clock, local time: ("YYYY-MM-DD", "HH:MM").
pub fn now(c: &Connection) -> rusqlite::Result<(String, String)> {
    c.query_row("SELECT date('now','localtime'), strftime('%H:%M','now','localtime')", [], |r| Ok((r.get(0)?, r.get(1)?)))
}

/// The month in `m` if it looks like "YYYY-MM", else this month (Nido keeps it in every link).
pub fn month(m: Option<&str>, today: &str) -> String {
    match m {
        Some(m) if m.len() == 7 && days(&format!("{m}-01")).is_some() => m.to_string(),
        _ => today[..7].to_string(),
    }
}

async fn guard(State(key): State<Arc<String>>, req: Request, next: Next) -> Response {
    if req.headers().get("x-nido-key").and_then(|v| v.to_str().ok()) == Some(key.as_str()) {
        next.run(req).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}

async fn manifest() -> Response {
    let flashes: serde_json::Map<String, serde_json::Value> =
        act::FLASHES.iter().map(|(k, v)| (k.to_string(), (*v).into())).collect();
    let body = serde_json::json!({ "id": "pets", "version": VERSION, "flashes": flashes });
    ([(header::CONTENT_TYPE, "application/json")], body.to_string()).into_response()
}

async fn page(State(app): State<App>, h: HeaderMap, Query(p): Query<Params>) -> R {
    let Some(me) = me(&h) else { return Ok(StatusCode::FORBIDDEN.into_response()) };
    let c = lock(&app.db);
    remember_people(&c, &me);
    Ok(Html(view::page(&c, me, &p)?.into_string()).into_response())
}

/// In-app notices for whoever has Nido open (Nido asks every 3 s).
async fn notices(State(app): State<App>, h: HeaderMap) -> Response {
    let me: i64 = h.get("x-nido-user").and_then(|v| v.to_str().ok()?.parse().ok()).unwrap_or(0);
    let mut notes = lock(&app.notes);
    let mut out = Vec::new();
    for n in notes.iter_mut() {
        if let Some(i) = n.pending.iter().position(|u| *u == me) {
            n.pending.remove(i);
            out.push(serde_json::json!({ "text": n.text, "href": n.href, "action": "Ver" }));
        }
    }
    notes.retain(|n| !n.pending.is_empty());
    axum::Json(out).into_response()
}

/// Notices nobody saw in 30 s: Nido sends them to each phone.
async fn notices_late(State(app): State<App>) -> Response {
    let mut notes = lock(&app.notes);
    let mut out = Vec::new();
    for n in notes.iter_mut().filter(|n| n.at.elapsed() > Duration::from_secs(30)) {
        for u in n.pending.drain(..) {
            out.push(serde_json::json!({ "user": u, "text": n.text, "href": n.href, "action": "Ver" }));
        }
    }
    notes.retain(|n| !n.pending.is_empty());
    axum::Json(out).into_response()
}

fn minutes(dt: &str) -> Option<i64> {
    let t = hhmm(dt.get(11..16)?)?;
    Some(days(dt)? * 1440 + t[..2].parse::<i64>().ok()? * 60 + t[3..].parse::<i64>().ok()?)
}

/// Reminders that are due at `day` `time`: vet visits inside their warning window, doses whose time
/// passed in the last three hours without anyone ticking them, meals nobody gave within an hour,
/// and from 9:00 chores due, booklet doses within a week and birthdays. Each one is marked as sent
/// here, so it goes out once. Returns (text, href).
fn due(c: &Connection, day: &str, time: &str) -> rusqlite::Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    let now = minutes(&format!("{day} {time}")).unwrap_or(0);

    let visits: Vec<(i64, String, String, String, i64, i64)> = c
        .prepare("SELECT v.id,v.at,v.title,p.name,p.id,v.remind FROM visit v JOIN pet p ON p.id=v.pet_id WHERE v.sent=0 AND v.remind>0")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?
        .collect::<Result<_, _>>()?;
    for (id, at, title, pet, pid, remind) in visits {
        let Some(m) = minutes(&at) else { continue };
        if m > now && m - now <= remind {
            let when = day_label(&at, day);
            let what = if title.is_empty() { String::new() } else { format!(" ({title})") };
            out.push((format!("{when} a las {}: veterinario de {pet}{what}", &at[11..16]), format!("/?tab=pets&p={pid}&s=vet")));
            c.execute("UPDATE visit SET sent=1 WHERE id=?1", [id])?;
        }
    }

    for m in data::meds(c, None)?.into_iter().filter(|m| m.active_on(day)) {
        for slot in m.slots() {
            let Some(at) = minutes(&format!("{day} {slot}")) else { continue };
            if at > now || now - at > 180 {
                continue;
            }
            let key = format!("d:{}:{day}:{slot}", m.id);
            let given: bool = c.query_row("SELECT count(*) FROM dose WHERE med_id=?1 AND day=?2 AND slot=?3", params![m.id, day, slot], |r| r.get::<_, i64>(0))? > 0;
            if !given && c.execute("INSERT OR IGNORE INTO sent(k,day) VALUES(?1,?2)", params![key, day])? == 1 {
                out.push((format!("Toca {} a {} ({slot})", m.title, m.pet), format!("/?tab=pets&p={}&s=med", m.pet_id)));
            }
        }
    }

    let pets = data::pets(c)?;
    for p in &pets {
        for slot in p.meal_slots() {
            let Some(at) = minutes(&format!("{day} {slot}")) else { continue };
            if now - at < 60 || now - at > 240 {
                continue;
            }
            let given: bool = c.query_row("SELECT count(*) FROM fed WHERE pet_id=?1 AND day=?2 AND slot=?3", params![p.id, day, slot], |r| r.get::<_, i64>(0))? > 0;
            if !given && c.execute("INSERT OR IGNORE INTO sent(k,day) VALUES(?1,?2)", params![format!("f:{}:{day}:{slot}", p.id), day])? == 1 {
                out.push((format!("¿Alguien le ha dado de comer a {}? Tocaba a las {slot}", p.name), format!("/?tab=pets&p={}", p.id)));
            }
        }
    }

    if time >= "09:00" {
        for p in pets.iter().filter(|p| p.birth.len() == 10 && p.birth[5..] == day[5..] && p.birth[..4] < day[..4]) {
            if c.execute("INSERT OR IGNORE INTO sent(k,day) VALUES(?1,?2)", params![format!("b:{}:{}", p.id, &day[..4]), day])? == 1 {
                let years = age(&p.birth, day).unwrap_or_default();
                out.push((format!("Hoy {} cumple {years}", p.name), format!("/?tab=pets&p={}", p.id)));
            }
        }
        let records: Vec<(i64, String, String, String, String, i64)> = c
            .prepare("SELECT r.id,r.kind,r.name,r.next,p.name,p.id FROM record r JOIN pet p ON p.id=r.pet_id WHERE r.next<>'' AND r.next<=?1 AND r.next>=?2 AND r.sent<>r.next")?
            .query_map([add_days(day, 7), day.to_string()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?
            .collect::<Result<_, _>>()?;
        for (id, kind, name, next, pet, pid) in records {
            let what = if name.is_empty() { data::record_kind(&kind).1.to_string() } else { name };
            out.push((format!("{}: {what} de {pet}", day_label(&next, day)), format!("/?tab=pets&p={pid}&s=vet")));
            c.execute("UPDATE record SET sent=next WHERE id=?1", [id])?;
        }
        let tasks: Vec<(i64, String, String, String, i64)> = c
            .prepare("SELECT t.id,t.title,t.due,p.name,p.id FROM task t JOIN pet p ON p.id=t.pet_id WHERE t.due<>'' AND t.due<=?1 AND t.sent<>t.due")?
            .query_map([day], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
            .collect::<Result<_, _>>()?;
        for (id, title, due, pet, pid) in tasks {
            let text = if due == day { format!("Hoy le toca a {pet}: {title}") } else { format!("Pendiente con {pet}: {title}, desde {}", day_label(&due, day).to_lowercase()) };
            out.push((text, format!("/?tab=pets&p={pid}&s=tasks")));
            c.execute("UPDATE task SET sent=due WHERE id=?1", [id])?;
        }
    }
    c.execute("DELETE FROM sent WHERE day<?1", [add_days(day, -7)])?;
    Ok(out)
}

async fn remind(app: App) {
    let mut tick = tokio::time::interval(Duration::from_secs(60));
    loop {
        tick.tick().await;
        let c = lock(&app.db);
        let people = people_ids(&c);
        if people.is_empty() {
            continue;
        }
        let found = now(&c).and_then(|(d, t)| due(&c, &d, &t));
        drop(c);
        match found {
            Ok(list) => {
                let mut notes = lock(&app.notes);
                for (text, href) in list {
                    notes.push(Note { text, href, pending: people.clone(), at: Instant::now() });
                }
            }
            Err(e) => eprintln!("avisos: {e}"),
        }
    }
}

pub fn open(path: &str) -> rusqlite::Result<Connection> {
    let c = Connection::open(path)?;
    c.busy_timeout(Duration::from_secs(5))?;
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY; PRAGMA foreign_keys=ON;")?;
    c.execute_batch(include_str!("../schema.sql"))?;
    Ok(c)
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("{VERSION}");
        return;
    }
    let env = |k: &str, default: &str| std::env::var(k).unwrap_or_else(|_| default.to_string());
    let key = std::env::var("NIDO_PLUGIN_KEY").unwrap_or_else(|_| {
        eprintln!("nido-pets lo arranca Nido (Plugins); falta NIDO_PLUGIN_KEY");
        std::process::exit(2);
    });
    let conn = open(&env("NIDO_PLUGIN_DB", "pets.db")).expect("no se pudo abrir la base de datos");
    let app = App { db: Arc::new(Mutex::new(conn)), notes: Default::default() };
    let router = act::routes()
        .route("/manifest", get(manifest))
        .route("/page", get(page))
        .route("/notices", get(notices))
        .route("/notices/late", get(notices_late))
        .layer(middleware::from_fn_with_state(Arc::new(key), guard))
        .with_state(app.clone());
    let addr = env("NIDO_PLUGIN_ADDR", "127.0.0.1:8184");
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("no se pudo abrir el puerto");
    println!("nido-pets {VERSION} en http://{addr}");
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            tokio::task::spawn_local(remind(app));
            axum::serve(axum::serve::ListenerExt::tap_io(listener, |tcp| { let _ = tcp.set_nodelay(true); }), router).await.expect("servidor");
        })
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminders_go_out_once() {
        let c = open(":memory:").unwrap();
        c.execute("INSERT INTO pet(id,name,kind) VALUES(1,'Luna','dog')", []).unwrap();
        c.execute("INSERT INTO visit(pet_id,at,title,remind) VALUES(1,'2026-11-17 10:30','Vacuna',1440)", []).unwrap();
        c.execute("INSERT INTO visit(pet_id,at,title,remind) VALUES(1,'2026-11-30 10:30','Revisión',1440)", []).unwrap();
        c.execute("INSERT INTO med(pet_id,title,times,start,days) VALUES(1,'Apoquel','08:00,20:00','2026-11-10',10)", []).unwrap();
        c.execute("INSERT INTO task(pet_id,title,every,due) VALUES(1,'Baño',30,'2026-11-16')", []).unwrap();
        c.execute("UPDATE pet SET meals='09:30,20:00', birth='2021-11-16' WHERE id=1", []).unwrap();
        c.execute("INSERT INTO record(pet_id,kind,name,day,next) VALUES(1,'vaccine','Rabia','2025-11-20','2026-11-20')", []).unwrap();

        let first = due(&c, "2026-11-16", "11:00").unwrap();
        let texts: Vec<&str> = first.iter().map(|n| n.0.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Mañana a las 10:30: veterinario de Luna (Vacuna)",
                "Toca Apoquel a Luna (08:00)",
                "¿Alguien le ha dado de comer a Luna? Tocaba a las 09:30",
                "Hoy Luna cumple 5 años",
                "vie 20 nov: Rabia de Luna",
                "Hoy le toca a Luna: Baño"
            ]
        );
        assert!(due(&c, "2026-11-16", "11:01").unwrap().is_empty());

        c.execute("INSERT INTO dose(med_id,day,slot) VALUES(1,'2026-11-16','20:00')", []).unwrap();
        assert!(due(&c, "2026-11-16", "20:30").unwrap().is_empty());
        assert!(due(&c, "2026-11-25", "08:10").unwrap().is_empty(), "the course of 10 days is over");
    }
}
