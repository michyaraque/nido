//! Development tool: sends test notifications to anyone in the house through Nido's real path
//! (`/notices/late` → Web Push, with Telegram when no phone confirms). It lives under `tools/`
//! so the release workflow, which ships every `plugins/*/`, never publishes it, and the core only
//! lists it in debug builds.
use axum::{
    body::Bytes,
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use maud::html;
use rusqlite::{Connection, OpenFlags};
use std::sync::{Arc, Mutex};

const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Default)]
struct App {
    queue: Arc<Mutex<Vec<(i64, String)>>>,
    core_db: Arc<String>,
}

struct Person {
    id: i64,
    name: String,
    phones: i64,
    telegram: bool,
    waiting: i64,
}

/// Read-only: the core's database belongs to the core.
fn people(core_db: &str) -> rusqlite::Result<Vec<Person>> {
    let c = Connection::open_with_flags(core_db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let pending = c.query_row("SELECT 1 FROM sqlite_master WHERE name='push_pending'", [], |_| Ok(())).is_ok();
    let sql = format!(
        "SELECT u.id, u.name, (SELECT COUNT(*) FROM push_subs s WHERE s.user_id=u.id), u.tg_chat IS NOT NULL, {}
         FROM users u ORDER BY u.id",
        if pending { "(SELECT COUNT(*) FROM push_pending p WHERE p.user_id=u.id)" } else { "0" }
    );
    let mut s = c.prepare(&sql)?;
    let rows = s.query_map([], |r| Ok(Person { id: r.get(0)?, name: r.get(1)?, phones: r.get(2)?, telegram: r.get(3)?, waiting: r.get(4)? }))?;
    rows.collect()
}

fn me(h: &HeaderMap) -> i64 {
    h.get("x-nido-user").and_then(|v| v.to_str().ok()?.parse().ok()).unwrap_or(0)
}

async fn page(State(app): State<App>, h: HeaderMap) -> axum::response::Html<String> {
    let me = me(&h);
    let list = people(&app.core_db);
    let queued = app.queue.lock().unwrap().len();
    axum::response::Html(html! {
        div.page-head { h1 { "Prueba de avisos" } }
        p.hint { "Solo para probar. Nido manda cada aviso al móvil y, si ningún móvil confirma que lo ha mostrado en 3 minutos, también por Telegram. Nido recoge los avisos cada 30 segundos." }
        @match list {
            Err(e) => div.alert.bad { "No se puede leer la casa: " (e) },
            Ok(list) => {
                form.card method="post" action="/plugins/notifytest/send" {
                    fieldset.field {
                        legend { "Para quién" }
                        @for p in &list {
                            label {
                                input type="checkbox" name="to" value=(p.id) checked[p.id == me];
                                " " (p.name) @if p.id == me { " (tú)" }
                                span.hint {
                                    " · " (p.phones) (if p.phones == 1 { "\u{a0}móvil" } else { "\u{a0}móviles" })
                                    " · Telegram " (if p.telegram { "sí" } else { "no" })
                                    @if p.waiting > 0 { " · " (p.waiting) "\u{a0}sin confirmar" }
                                }
                            }
                            br;
                        }
                    }
                    label.field {
                        "Texto"
                        input.input type="text" name="text" maxlength="200" required value="Aviso de prueba";
                    }
                    button.btn.primary type="submit" { "Enviar" }
                    @if queued > 0 { p.hint { (queued) " en cola, salen en menos de 30\u{a0}s." } }
                }
            }
        }
    }.into_string())
}

async fn send(State(app): State<App>, body: Bytes) -> Response {
    let form: Vec<(String, String)> = serde_urlencoded::from_bytes(&body).unwrap_or_default();
    let text = form.iter().find(|(k, _)| k == "text").map(|(_, v)| v.trim().to_string()).unwrap_or_default();
    let to: Vec<i64> = form.iter().filter(|(k, _)| k == "to").filter_map(|(_, v)| v.parse().ok()).collect();
    let code = if text.is_empty() || to.is_empty() {
        "err=Elige a alguien y escribe el texto"
    } else {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() % 1000).unwrap_or(0);
        app.queue.lock().unwrap().extend(to.into_iter().map(|u| (u, format!("{text} · #{stamp}"))));
        "ok=nt_sent"
    };
    (StatusCode::SEE_OTHER, [(header::LOCATION, format!("/?tab=notifytest&{code}"))]).into_response()
}

/// Always `200 []`: a 404 would make Nido stop asking, `/notices/late` included.
async fn notices() -> Response {
    ([(header::CONTENT_TYPE, "application/json")], "[]").into_response()
}

async fn notices_late(State(app): State<App>) -> Response {
    let out: Vec<_> = app.queue.lock().unwrap().drain(..).map(|(u, t)| serde_json::json!({ "user": u, "text": t, "href": "/?tab=notifytest" })).collect();
    ([(header::CONTENT_TYPE, "application/json")], serde_json::Value::from(out).to_string()).into_response()
}

async fn manifest() -> Response {
    let body = serde_json::json!({ "id": "notifytest", "version": VERSION, "flashes": { "nt_sent": "Aviso en cola" } });
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
    let key = std::env::var("NIDO_PLUGIN_KEY").unwrap_or_else(|_| {
        eprintln!("nido-notifytest lo arranca Nido (Plugins); falta NIDO_PLUGIN_KEY");
        std::process::exit(2);
    });
    let app = App { queue: Default::default(), core_db: Arc::new(std::env::var("NIDO_CORE_DB").unwrap_or_else(|_| "nido.db".into())) };
    let router = Router::new()
        .route("/manifest", get(manifest))
        .route("/page", get(page))
        .route("/send", post(send))
        .route("/notices", get(notices))
        .route("/notices/late", get(notices_late))
        .layer(middleware::from_fn_with_state(Arc::new(key), guard))
        .with_state(app);
    let addr = std::env::var("NIDO_PLUGIN_ADDR").unwrap_or_else(|_| "127.0.0.1:8199".into());
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("no se pudo abrir el puerto");
    println!("nido-notifytest {VERSION} en http://{addr}");
    axum::serve(listener, router).await.expect("servidor");
}
