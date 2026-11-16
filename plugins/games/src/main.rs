//! nido-games: the "Juegos" plugin of Nido, as a program of its own (see plugins/README.md).
//!
//! Board games for the house: everyone plays from their own phone against the others or against
//! a robot. Nido starts this program, forwards `/plugins/games/<path>` to it as `/<path>`, and asks
//! `GET /page` for the content area. Matches and the ranking live in its own database.
use axum::{
    extract::{Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use maud::{html, Markup, PreEscaped};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use std::sync::{Arc, Mutex, MutexGuard};

mod kit;
use kit::*;

pub const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

pub type Db = Arc<Mutex<Connection>>;
pub type Params = std::collections::HashMap<String, String>;

pub fn lock(db: &Db) -> MutexGuard<'_, Connection> {
    db.lock().unwrap_or_else(|e| e.into_inner())
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
        (StatusCode::INTERNAL_SERVER_ERROR, "Error interno").into_response()
    }
}
pub type R = Result<Response, AppErr>;

pub fn bad(msg: &'static str) -> R {
    Ok((StatusCode::BAD_REQUEST, msg).into_response())
}

/// Someone in the house, as Nido tells it.
#[derive(Clone)]
pub struct Person {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub photo: i64, // version of their Nido profile photo, 0 = none
}

impl Person {
    /// Their face: the profile photo from Nido, or a letter in their colour.
    pub fn face(&self) -> Markup {
        face(&self.name, &self.color, self.photo_url().as_deref())
    }
    pub fn photo_url(&self) -> Option<String> {
        (self.photo > 0).then(|| format!("/avatars/{}?v={}", self.id, self.photo))
    }
}

/// Who asks and everyone in the house (`X-Nido-User`, `X-Nido-Name`, `X-Nido-People`).
#[derive(Clone)]
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

macro_rules! require_user {
    ($h:expr) => {
        match $crate::me(&$h) {
            Some(u) => u,
            None => return Ok(axum::response::IntoResponse::into_response(axum::http::StatusCode::FORBIDDEN)),
        }
    };
}

mod app;
mod games;
mod net;

/// The month on screen (Nido keeps it in every link) and who looks.
pub struct View {
    pub m: String,
    pub me: Me,
}

impl View {
    pub fn link(&self, extra: &str) -> String {
        if extra.is_empty() { format!("/?month={}&tab=games", self.m) } else { format!("/?month={}&tab=games&{extra}", self.m) }
    }
    pub fn ctx(&self) -> Markup {
        html! { input type="hidden" name="month" value=(self.m); input type="hidden" name="tab" value="games"; }
    }
    pub fn person(&self, id: i64) -> Option<&Person> {
        self.me.people.iter().find(|p| p.id == id)
    }
}

pub fn month(c: &Connection, m: Option<&str>) -> rusqlite::Result<String> {
    match m {
        Some(m) if m.len() == 7 && m.as_bytes()[4] == b'-' && m[..4].parse::<u16>().is_ok() && m[5..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)) => {
            Ok(m.to_string())
        }
        _ => c.query_row("SELECT strftime('%Y-%m','now','localtime')", [], |r| r.get(0)),
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
    let flashes: serde_json::Map<String, serde_json::Value> = app::FLASHES.iter().map(|(k, v)| (k.to_string(), (*v).into())).collect();
    let body = serde_json::json!({ "id": "games", "version": VERSION, "flashes": flashes });
    ([(header::CONTENT_TYPE, "application/json")], body.to_string()).into_response()
}

async fn page(State(db): State<Db>, h: HeaderMap, Query(p): Query<Params>) -> R {
    let me = require_user!(h);
    let c = lock(&db);
    let v = View { m: month(&c, p.get("month").map(String::as_str))?, me };
    Ok(Html(app::page(&c, &v, &p)?.into_string()).into_response())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("{VERSION}");
        return;
    }
    let env = |k: &str, default: &str| std::env::var(k).unwrap_or_else(|_| default.to_string());
    let key = std::env::var("NIDO_PLUGIN_KEY").unwrap_or_else(|_| {
        eprintln!("nido-games lo arranca Nido (Plugins); falta NIDO_PLUGIN_KEY");
        std::process::exit(2);
    });
    let conn = Connection::open(env("NIDO_PLUGIN_DB", "games.db")).expect("no se pudo abrir la base de datos");
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY;").expect("ajustes de la base de datos");
    conn.execute_batch(include_str!("../schema.sql")).expect("tablas");
    let db: Db = Arc::new(Mutex::new(conn));
    let app = app::routes()
        .route("/manifest", get(manifest))
        .route("/page", get(page))
        .route("/ws", get(net::ws))
        .layer(middleware::from_fn_with_state(Arc::new(key), guard))
        .with_state(db);
    let addr = env("NIDO_PLUGIN_ADDR", "127.0.0.1:8182");
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("no se pudo abrir el puerto");
    println!("nido-games {VERSION} en http://{addr}");
    axum::serve(listener, app).await.expect("servidor");
}
