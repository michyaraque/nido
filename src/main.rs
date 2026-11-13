#[macro_use]
mod auth; // users, sessions, login (first: defines require_user!)
mod chores;
mod actions;
mod api;
mod assets;
mod pages;
mod profile;
mod products;
mod dates;
mod db;
mod errors;
mod merchants;
mod money;
mod nav;
mod backup;
mod notify;
mod ocr;
mod receipt;
mod tickets;
mod text;
mod ui;

use actions::*;
use api::*;
use assets::*;
use auth::*;
use chores::*;
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, FromRequest, Path, Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use pages::*;
use profile::*;
use dates::*;
use db::*;
use errors::*;
use maud::{html, Markup, PreEscaped, DOCTYPE};
use merchants::{Cat, CATS};
use money::*;
use nav::*;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::sync::{Arc, Mutex, MutexGuard};
use text::*;
use tickets::*;
use ui::*;

const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

fn create_user(c: &Connection, name: &str) {
    let mut pass = String::new();
    std::io::stdin().read_line(&mut pass).expect("leer contraseña");
    let pass = pass.trim_end_matches(['\r', '\n']);
    if pass.len() < 4 {
        eprintln!("La contraseña debe tener al menos 4 caracteres");
        std::process::exit(1);
    }
    let n = c.execute("UPDATE users SET pass=?1 WHERE name=?2", params![hash(pass), name]).expect("db");
    if n == 0 {
        c.execute("INSERT INTO users(name,pass,token) VALUES(?1,?2,?3)", params![name, hash(pass), rand_hex(24)])
            .expect("db");
        println!("Usuario «{name}» creado");
    } else {
        println!("Contraseña de «{name}» cambiada");
    }
}

fn ocr_cli(path: &str) {
    let t = std::time::Instant::now();
    assert!(ocr::warm_up(), "no se encontraron los modelos (NIDO_MODELS)");
    let t_models = t.elapsed();
    let photo = std::fs::read(path).expect("no se pudo leer la foto");
    let (img, stored) = ocr::normalize(&photo).expect("imagen");
    let t_img = t.elapsed() - t_models;
    let lines = ocr::read_lines(&img).expect("ocr");
    for l in &lines {
        println!("{l}");
    }
    println!("---\n{:?}", receipt::parse(&lines));
    let t_text = t.elapsed() - t_models - t_img;
    println!("modelos {t_models:.1?} · imagen {t_img:.1?} ({} KB) · lectura {t_text:.1?}", stored.len() / 1024);
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--version") {
        println!("{VERSION}");
        return;
    }
    let path = std::env::var("NIDO_DB").unwrap_or_else(|_| "nido.db".into());
    let conn = Connection::open(&path).expect("no se pudo abrir la base de datos");
    migrate(&conn);

    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => {}
        ["user", name] => return create_user(&conn, name),
        ["ocr", path] => return ocr_cli(path),
        ["avisos", name] => {
            let db: Db = Arc::new(Mutex::new(conn));
            return match notify::send_now(&db, name) {
                Ok(()) => println!("Avisos enviados a {name}"),
                Err(e) => eprintln!("{e}"),
            };
        }
        _ => {
            eprintln!("Uso:\n  nido                 arranca el servidor\n  nido user <nombre>   crea un usuario o cambia su contraseña (la lee de stdin)\n  nido ocr <foto>      lee un ticket y muestra lo que entiende\n  nido --version");
            std::process::exit(2);
        }
    }

    let db: Db = Arc::new(Mutex::new(conn));
    notify::start(db.clone());
    backup::start(db.clone());
    let app = Router::new()
        .route("/", get(dashboard))
        .route("/login", get(login_page).post(login))
        .route("/logout", post(logout))
        .route("/bills", post(add_bill))
        .route("/bills/{id}", post(update_bill))
        .route("/bills/{id}/amount", post(set_bill_amount))
        .route("/bills/{id}/end", post(end_bill))
        .route("/bills/{id}/payment", post(toggle_payment))
        .route("/bills/{id}/whole", post(toggle_whole))
        .route("/expenses", post(add_expense))
        .route("/expenses/{id}", post(update_expense))
        .route("/expenses/{id}/delete", post(delete_expense))
        .route("/me/avatar", post(upload_avatar).layer(DefaultBodyLimit::max(25 * 1024 * 1024)))
        .route("/me/avatar/delete", post(delete_avatar))
        .route("/avatars/{id}", get(avatar))
        .route("/settlements", post(add_settlement))
        .route("/settlements/{id}/delete", post(delete_settlement))
        .route("/me/salary", post(set_salary))
        .route("/shopping", post(add_items))
        .route("/shopping/clear", post(clear_items))
        .route("/shopping/{id}/toggle", post(toggle_item))
        .route("/shopping/{id}/delete", post(delete_item))
        .route("/tickets", post(upload_ticket).layer(DefaultBodyLimit::max(25 * 1024 * 1024)))
        .route("/tickets/{id}", get(ticket_page))
        .route("/tickets/{id}/photo", get(ticket_photo))
        .route("/tickets/{id}/expense", post(save_ticket))
        .route("/tickets/{id}/delete", post(delete_ticket))
        .route("/chores", post(add_chore))
        .route("/chores/starter", post(add_starter))
        .route("/chores/{id}", post(update_chore))
        .route("/chores/{id}/archive", post(archive_chore))
        .route("/chores/{id}/log", post(log_chore))
        .route("/chores/logs/{id}/delete", post(delete_log))
        .route("/chores/{id}/plan", post(plan_chore))
        .route("/plans/{id}/take", post(take_plan))
        .route("/plans/{id}/done", post(complete_plan))
        .route("/plans/{id}/delete", post(delete_plan))
        .route("/house/settings", post(save_house))
        .route("/house/theme", post(save_theme))
        .route("/backups/password", post(backup::save_password))
        .route("/backups/time", post(backup::save_time))
        .route("/backups/run", post(backup::run_backup))
        .route("/backups/targets", post(backup::add_target))
        .route("/backups/targets/{id}/test", post(backup::test_target))
        .route("/backups/targets/{id}/delete", post(backup::delete_target))
        .route("/backups/targets/{id}", get(backups_page).post(backup::update_target))
        .route("/backups/targets/{id}/files/{name}", get(backup::download))
        .route("/backups/targets/{id}/files/{name}/delete", post(backup::delete_copy))
        .route("/telegram/bot", post(notify::connect_bot))
        .route("/telegram/remove", post(notify::remove_bot))
        .route("/telegram/photo", get(notify::bot_photo))
        .route("/telegram/time", post(notify::save_time))
        .route("/telegram/test", post(notify::test))
        .route("/telegram/unlink", post(notify::unlink))
        .route("/api/expenses", post(api_create_expense))
        .merge(assets::routes())
        .with_state(db);

    let addr = std::env::var("NIDO_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("no se pudo abrir el puerto");
    println!("nido {VERSION} escuchando en http://{addr}");
    axum::serve(listener, app).await.expect("servidor");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_and_months() {
        assert_eq!(parse_cents("12,50 €"), Some(1250));
        assert_eq!(parse_cents("1.234,56"), Some(123456));
        assert_eq!(parse_cents("1,234.56"), Some(123456));
        assert_eq!(parse_cents("1.234"), Some(123400));
        assert_eq!(parse_cents("€7.5"), Some(750));
        assert_eq!(parse_cents("-3"), Some(-300));
        assert_eq!(parse_cents("abc"), None);
        assert_eq!(parse_cents("-"), None);
        assert_eq!(eur(123456), "1.234,56 €");
        assert_eq!(eur(-5), "-0,05 €");
        assert_eq!(eur(100000000), "1.000.000,00 €");
        assert_eq!(eur_input(123456), "1234,56");
        assert_eq!(parse_cents(&eur_input(123456)), Some(123456));
        assert_eq!(shift("2026-01", -1), "2025-12");
        assert_eq!(shift("2026-12", 1), "2027-01");
        assert!(valid_month("2026-11") && !valid_month("2026-13") && !valid_month("2026-1x") && !valid_month("ñ"));
        assert_eq!(tab(Some("house")), "house");
        assert_eq!(tab(Some("<script>")), "home");
        assert_eq!(day_label("2026-11-10", 2, "2026-11-12", "2026-11-11"), "martes 10 nov");
        assert_eq!(day_label("2026-11-11", 3, "2026-11-12", "2026-11-11"), "Ayer");
    }

    #[test]
    fn accents_survive_any_encoding() {
        assert_eq!(to_text(pct_decode(b"Caf%C3%A9+con+le%C3%B1a")), "Café con leña");
        assert_eq!(to_text(pct_decode(b"Caf%E9+m%F3vil")), "Café móvil");
        assert_eq!(to_text(vec![b'5', 0x80]), "5€");
        assert_eq!(to_text(pct_decode(b"100%")), "100%");
        assert_eq!(to_text(pct_decode(b"%zz")), "%zz");
    }
}
