//! nido-recipes: the "Recetas" plugin of Nido, as a program of its own.
//!
//! Nido starts it when the house activates the plugin, and talks to it over HTTP on 127.0.0.1
//! (see plugins/README.md): `GET /page` returns the HTML for the content area, and every
//! `/plugins/recipes/<path>` request from the browser reaches this program as `/<path>`. Nido checks the
//! session first and says who is asking in `X-Nido-*` headers. The recipes live in their own
//! database (`recipes.db`), so the core never carries a byte of this plugin.
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
/// Query string of the page (`r`, `q`, `cat`, `serves`, `new`, `edit`…).
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

/// The person of the house who is asking, as Nido tells it (it has already checked the session).
pub struct User {
    pub id: i64,
    pub name: String,
}

pub fn me(h: &HeaderMap) -> Option<User> {
    let get = |k: &str| h.get(k).and_then(|v| v.to_str().ok());
    Some(User {
        id: get("x-nido-user")?.parse().ok()?,
        name: pct_decode(get("x-nido-name").unwrap_or("")),
    })
}

macro_rules! require_user {
    ($c:expr, $h:expr) => {
        match $crate::me(&$h) {
            Some(u) => u,
            None => {
                return Ok(axum::response::IntoResponse::into_response(
                    axum::http::StatusCode::FORBIDDEN,
                ))
            }
        }
    };
}

mod recipes;

/// Where the page is shown: the month on screen (Nido keeps it in every link) and who looks.
pub struct View {
    pub m: String,
    pub me: User,
}

impl View {
    pub fn link(&self, tab: &str) -> String {
        format!("/?month={}&tab={tab}", self.m)
    }
    /// Hidden fields every form carries back, like Nido's own forms.
    pub fn ctx(&self) -> Markup {
        html! { input type="hidden" name="month" value=(self.m); input type="hidden" name="tab" value="recipes"; }
    }
}

/// The month in `m` if it looks like "YYYY-MM", else this month.
pub fn month(c: &Connection, m: Option<&str>) -> rusqlite::Result<String> {
    match m {
        Some(m)
            if m.len() == 7
                && m.as_bytes()[4] == b'-'
                && m[..4].parse::<u16>().is_ok()
                && m[5..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)) =>
        {
            Ok(m.to_string())
        }
        _ => c.query_row("SELECT strftime('%Y-%m','now','localtime')", [], |r| {
            r.get(0)
        }),
    }
}

/// Only Nido may talk to this program: it passes a secret it made up when it started us.
async fn guard(State(key): State<Arc<String>>, req: Request, next: Next) -> Response {
    if req
        .headers()
        .get("x-nido-key")
        .and_then(|v| v.to_str().ok())
        == Some(key.as_str())
    {
        next.run(req).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}

/// What Nido needs to know once it has started us: our toasts (Nido shows them after a redirect).
async fn manifest() -> Response {
    let flashes: serde_json::Map<String, serde_json::Value> = recipes::FLASHES
        .iter()
        .map(|(k, v)| (k.to_string(), (*v).into()))
        .collect();
    let body = serde_json::json!({ "id": "recipes", "version": VERSION, "flashes": flashes });
    (
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

async fn page(State(db): State<Db>, h: HeaderMap, Query(p): Query<Params>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let v = View {
        m: month(&c, p.get("month").map(String::as_str))?,
        me,
    };
    Ok(Html(recipes::page(&c, &v, &p)?.into_string()).into_response())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("{VERSION}");
        return;
    }
    let env = |k: &str, default: &str| std::env::var(k).unwrap_or_else(|_| default.to_string());
    let key = std::env::var("NIDO_PLUGIN_KEY").unwrap_or_else(|_| {
        eprintln!("nido-recipes lo arranca Nido (Plugins); falta NIDO_PLUGIN_KEY");
        std::process::exit(2);
    });
    let conn =
        Connection::open(env("NIDO_PLUGIN_DB", "recipes.db")).expect("no se pudo abrir recipes.db");
    recipes::prepare(&conn, std::env::var("NIDO_CORE_DB").ok().as_deref())
        .expect("preparar el recetario");
    let db: Db = Arc::new(Mutex::new(conn));
    let app = recipes::routes()
        .route("/manifest", get(manifest))
        .route("/page", get(page))
        .layer(middleware::from_fn_with_state(Arc::new(key), guard))
        .with_state(db);
    let addr = env("NIDO_PLUGIN_ADDR", "127.0.0.1:8181");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("no se pudo abrir el puerto");
    println!("nido-recipes {VERSION} en http://{addr}");
    axum::serve(listener, app).await.expect("servidor");
}
