use super::*;
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

const COLORS: [&str; 4] = ["#2f6bff", "#e1166b", "#7c3aed", "#d97706"];
const DAILY_WIDGET: &str = include_str!("../assets/telegram/daily.html");
const WEEKLY_WIDGET: &str = include_str!("../assets/telegram/weekly.html");

pub fn setting(c: &Connection, key: &str) -> Option<String> {
    c.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| r.get(0)).optional().ok().flatten()
}

pub fn set_setting(c: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    c.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value])?;
    Ok(())
}

pub fn reminder_time(c: &Connection) -> String {
    setting(c, "tg_at").unwrap_or_else(|| "20:00".into())
}

/// Calls a Bot API method. The token goes to curl through stdin (`-K -`), never on the command line.
fn api(token: &str, method: &str, form: &[(&str, &str)], wait: u64) -> Result<Value, String> {
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "").replace('\t', "\\t");
    let mut cfg = format!("url = \"https://api.telegram.org/bot{token}/{method}\"\nsilent\nmax-time = {}\n", wait + 15);
    for (k, v) in form {
        cfg += &format!("data-urlencode = \"{k}={}\"\n", quote(v));
    }
    let mut child = Command::new("curl")
        .args(["-K", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    child.stdin.take().ok_or("curl")?.write_all(cfg.as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|_| "Telegram no responde".to_string())?;
    if v["ok"].as_bool() == Some(true) {
        Ok(v["result"].clone())
    } else {
        Err(v["description"].as_str().unwrap_or("error de Telegram").to_string())
    }
}

/// Downloads a file from Telegram's file storage (token through stdin, like `api`).
fn download(token: &str, file_path: &str) -> Result<Vec<u8>, String> {
    let cfg = format!("url = \"https://api.telegram.org/file/bot{token}/{file_path}\"\nsilent\nfail\nmax-time = 30\n");
    let mut child = Command::new("curl").args(["-K", "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
        .spawn().map_err(|e| format!("curl: {e}"))?;
    child.stdin.take().ok_or("curl")?.write_all(cfg.as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() && !out.stdout.is_empty() { Ok(out.stdout) } else { Err("no se pudo descargar".into()) }
}

fn bot_photo_path() -> std::path::PathBuf {
    data_dir().join("avatars").join("bot.jpg")
}

/// Stores the bot's display name and profile photo, shown in Ajustes → La casa.
pub fn refresh_profile(db: &Db, token: &str) -> Result<(), String> {
    let me = api(token, "getMe", &[], 0)?;
    let id = me["id"].as_i64().ok_or("getMe")?.to_string();
    let title = me["first_name"].as_str().unwrap_or_default().to_string();
    let photos = api(token, "getUserProfilePhotos", &[("user_id", &id), ("limit", "1")], 0)?;
    let sizes = photos["photos"][0].as_array().cloned().unwrap_or_default();
    let pick = sizes.iter().find(|s| s["width"].as_i64().unwrap_or(0) >= 160).or(sizes.last());
    let jpeg = match pick.and_then(|s| s["file_id"].as_str()) {
        Some(file_id) => {
            let f = api(token, "getFile", &[("file_id", file_id)], 0)?;
            let bytes = download(token, f["file_path"].as_str().ok_or("getFile")?)?;
            Some(crate::profile::square_jpeg(&bytes)?)
        }
        None => None,
    };
    let p = bot_photo_path();
    match &jpeg {
        Some(j) => {
            std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&p, j).map_err(|e| e.to_string())?;
        }
        None => { let _ = std::fs::remove_file(&p); }
    }
    let c = lock(db);
    let version = if jpeg.is_some() { setting(&c, "tg_photo").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0) + 1 } else { 0 };
    set_setting(&c, "tg_title", &title).map_err(|e| e.to_string())?;
    set_setting(&c, "tg_photo", &version.to_string()).map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn bot_photo(State(db): State<Db>, h: HeaderMap) -> R {
    require_user!(lock(&db), h);
    match std::fs::read(bot_photo_path()) {
        Ok(b) => Ok(([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "private, max-age=31536000, immutable")], b).into_response()),
        Err(_) => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

pub fn bot_name(token: &str) -> Result<String, String> {
    api(token, "getMe", &[], 0)?["username"].as_str().map(str::to_string).ok_or_else(|| "token no válido".into())
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn send_text(token: &str, chat: i64, html: &str) -> Result<Value, String> {
    api(token, "sendMessage", &[("chat_id", &chat.to_string()), ("text", html), ("parse_mode", "HTML"), ("link_preview_options", r#"{"is_disabled":true}"#)], 0)
}

fn send_later(token: String, chat: i64, html: String) {
    std::thread::spawn(move || {
        if let Err(e) = send_text(&token, chat, &html) {
            eprintln!("telegram: {e}");
        }
    });
}

fn send_widget(token: &str, chat: i64, html: &str, data: &str, caption: &str, queries: bool, fallback: &str) -> Option<i64> {
    let sent = api(token, "sendWidget", &[
        ("chat_id", &chat.to_string()),
        ("html", html),
        ("chat_data", data),
        ("caption", caption),
        ("allow_queries", if queries { "true" } else { "false" }),
    ], 0);
    match sent {
        Ok(m) => m["message_id"].as_i64(),
        Err(e) => {
            eprintln!("telegram widget: {e}");
            send_text(token, chat, fallback).ok().and_then(|m| m["message_id"].as_i64())
        }
    }
}

/// Sends the same notice to everyone who linked Telegram (used when a backup fails).
pub fn tell_all(c: &Connection, html: &str) {
    let Some(token) = setting(c, "tg_token") else { return };
    let chats: Vec<i64> = c
        .prepare("SELECT tg_chat FROM users WHERE tg_chat IS NOT NULL")
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
        .unwrap_or_default();
    for chat in chats {
        send_later(token.clone(), chat, html.to_string());
    }
}

fn chat_of(c: &Connection, user: i64) -> Option<i64> {
    c.query_row("SELECT tg_chat FROM users WHERE id=?1", [user], |r| r.get(0)).ok().flatten()
}

const DAYS: [&str; 7] = ["domingo", "lunes", "martes", "miércoles", "jueves", "viernes", "sábado"];

fn day_words(c: &Connection, day: &str) -> String {
    let (today, tomorrow): (String, String) = c
        .query_row("SELECT date('now','localtime'), date('now','localtime','+1 day')", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap_or_default();
    if day == today {
        "hoy".into()
    } else if day == tomorrow {
        "mañana".into()
    } else {
        format!("el {} {}", DAYS[weekday(day)], day[8..].trim_start_matches('0'))
    }
}

pub fn planned(c: &Connection, by: &User, chore: i64, day: &str, assignee: Option<i64>) {
    let Some(token) = setting(c, "tg_token") else { return };
    let Ok(name) = c.query_row("SELECT name FROM chores WHERE id=?1", [chore], |r| r.get::<_, String>(0)) else { return };
    let when = day_words(c, day);
    let users: Vec<(i64, Option<i64>)> = c
        .prepare("SELECT id, tg_chat FROM users")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
        .unwrap_or_default();
    for (id, chat) in users {
        let Some(chat) = chat else { continue };
        let text = match assignee {
            Some(a) if a == id && a != by.id => format!("<b>{}</b> te ha planificado «{}» para {when}.", esc(&by.name), esc(&name)),
            None if id != by.id => format!("<b>{}</b> ha dejado «{}» libre para {when}: la puede coger quien pueda.", esc(&by.name), esc(&name)),
            _ => continue,
        };
        send_later(token.clone(), chat, text);
    }
}

fn todo(c: &Connection, user: i64) -> rusqlite::Result<Vec<(String, String, String, String)>> {
    let mut items = Vec::new();
    let mut planned_chores = Vec::new();
    let mut s = c.prepare(
        "SELECT p.id, ch.id, ch.name, ch.color, p.day, p.assignee_id FROM chore_plans p JOIN chores ch ON ch.id=p.chore_id
         WHERE p.log_id IS NULL AND ch.archived=0 AND p.day <= date('now','localtime') AND (p.assignee_id=?1 OR p.assignee_id IS NULL)
         ORDER BY p.day",
    )?;
    for r in s.query_map([user], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, Option<i64>>(5)?)))? {
        let (plan, chore, name, color, day, who) = r?;
        planned_chores.push(chore);
        let why = match (who.is_some(), day_words(c, &day) == "hoy") {
            (true, true) => "Te toca hoy".to_string(),
            (false, true) => "Libre: cógela si puedes".to_string(),
            (_, false) => format!("Atrasada: era para {}", day_words(c, &day)),
        };
        items.push((format!("p{plan}"), name, why, color));
    }
    let mut s = c.prepare(
        "SELECT ch.id, ch.name, ch.color, ch.every_days,
                CAST(julianday(date('now','localtime')) - julianday(MAX(l.day)) AS INTEGER)
         FROM chores ch LEFT JOIN chore_logs l ON l.chore_id=ch.id
         WHERE ch.archived=0 AND ch.every_days IS NOT NULL GROUP BY ch.id",
    )?;
    for r in s.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?, r.get::<_, Option<i64>>(4)?)))? {
        let (id, name, color, every, since) = r?;
        if planned_chores.contains(&id) {
            continue;
        }
        let why = match since {
            None => "Sin hacer todavía".to_string(),
            Some(d) if d > every => format!("Atrasada {} día{}", d - every, if d - every == 1 { "" } else { "s" }),
            Some(d) if d == every => "Toca hoy".to_string(),
            _ => continue,
        };
        items.push((format!("c{id}"), name, why, color));
    }
    Ok(items)
}

/// Kept under Telegram's 1 KB by shortening names and, if needed, leaving the last items out.
fn daily_data(title: &str, items: &[(String, String, String, String, bool)]) -> String {
    let short = |s: &str, n: usize| if s.chars().count() > n { s.chars().take(n - 1).collect::<String>() + "…" } else { s.to_string() };
    let mut list: Vec<Value> = items.iter().map(|(k, n, w, col, done)| json!([k, short(n, 28), short(w, 34), col, *done as u8])).collect();
    loop {
        let data = json!({ "h": title, "i": list }).to_string();
        if data.len() <= 1024 || list.is_empty() {
            return data;
        }
        list.pop();
    }
}

fn daily(c: &Connection, user: i64) -> Option<(String, String, String)> {
    let items = todo(c, user).unwrap_or_default();
    if items.is_empty() {
        return None;
    }
    let day = today(c).ok()?;
    let title = format!("Para hoy, {} {}", DAYS[weekday(&day)], day[8..].trim_start_matches('0'));
    let items: Vec<_> = items.into_iter().map(|(k, n, w, col)| (k, n, w, col, false)).collect();
    let fallback = format!("<b>{}</b>\n{}", esc(&title), items.iter().map(|(_, n, w, ..)| format!("• {} · <i>{}</i>", esc(n), esc(w))).collect::<Vec<_>>().join("\n"));
    let caption = format!("{} pendiente{}", items.len(), if items.len() == 1 { "" } else { "s" });
    Some((daily_data(&title, &items), fallback, caption))
}

fn deliver_daily(db: &Db, token: &str, chat: i64, (data, fallback, caption): (String, String, String)) {
    if let Some(msg) = send_widget(token, chat, DAILY_WIDGET, &data, &caption, true, &fallback) {
        // Telegram offers no way to read a widget's state back, so the bot keeps it.
        let _ = set_setting(&lock(db), &format!("tg_daily_{chat}_{msg}"), &data);
    }
}

fn widget_done(db: &Db, token: &str, q: &Value) {
    let qid = q["id"].as_str().unwrap_or_default();
    let (chat, msg) = (q["chat"]["id"].as_i64().unwrap_or(0), q["message_id"].as_i64().unwrap_or(0));
    let key = q["data"].as_str().unwrap_or_default().strip_prefix("done:").unwrap_or_default().to_string();
    let edited = (|| {
        let c = lock(db);
        let user: i64 = c.query_row("SELECT id FROM users WHERE tg_chat=?1", [chat], |r| r.get(0)).ok()?;
        let id: i64 = key.get(1..)?.parse().ok()?;
        match key.as_bytes().first()? {
            b'p' => {
                let chore: i64 = c.query_row("SELECT chore_id FROM chore_plans WHERE id=?1 AND log_id IS NULL", [id], |r| r.get(0)).ok()?;
                c.execute("INSERT INTO chore_logs(chore_id,user_id,day) VALUES(?1,?2,date('now','localtime'))", params![chore, user]).ok()?;
                c.execute("UPDATE chore_plans SET log_id=?2, assignee_id=COALESCE(assignee_id,?3) WHERE id=?1", params![id, c.last_insert_rowid(), user]).ok()?;
            }
            b'c' => {
                c.execute("INSERT INTO chore_logs(chore_id,user_id,day) VALUES(?1,?2,date('now','localtime'))", params![id, user]).ok()?;
            }
            _ => return None,
        }
        let state_key = format!("tg_daily_{chat}_{msg}");
        let mut data: Value = serde_json::from_str(&setting(&c, &state_key)?).ok()?;
        for it in data["i"].as_array_mut()? {
            if it[0] == key {
                it[4] = json!(1);
            }
        }
        let data = data.to_string();
        let _ = set_setting(&c, &state_key, &data);
        Some(data)
    })();
    let _ = api(token, "answerWidgetQuery", &[("widget_query_id", qid), ("data", if edited.is_some() { "ok" } else { "error" })], 0);
    if let Some(data) = edited {
        let _ = api(token, "editMessageWidgetData", &[("chat_id", &chat.to_string()), ("message_id", &msg.to_string()), ("chat_data", &data)], 0);
    }
}

fn weekly(c: &Connection, user: i64) -> rusqlite::Result<(String, String)> {
    const MONTHS: [&str; 12] = ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];
    let (from, to): (String, String) = c.query_row("SELECT date('now','localtime','-6 days'), date('now','localtime')", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let words = |d: &str| (d[8..].trim_start_matches('0').to_string(), MONTHS[d[5..7].parse::<usize>().unwrap_or(1) - 1]);
    let ((d1, m1), (d2, m2)) = (words(&from), words(&to));
    let range = if m1 == m2 { format!("Del {d1} al {d2} de {m2}") } else { format!("Del {d1} de {m1} al {d2} de {m2}") };
    let chores: Vec<(String, i64)> = c
        .prepare(
            "SELECT u.name, COUNT(l.id) FROM users u LEFT JOIN chore_logs l
             ON l.user_id=u.id AND l.day > date('now','localtime','-7 days') GROUP BY u.id ORDER BY u.id",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut by_cat: Vec<(&'static str, i64)> = Vec::new();
    let (mut spent, mut count) = (0i64, 0i64);
    let mut s = c.prepare("SELECT description, amount, category FROM expenses WHERE user_id=?1 AND date(created) > date('now','localtime','-7 days')")?;
    for r in s.query_map([user], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<String>>(2)?)))? {
        let (desc, amount, cat) = r?;
        let label = cat.as_deref().map(merchants::cat).unwrap_or_else(|| merchants::classify(&desc).cat).label;
        match by_cat.iter_mut().find(|(l, _)| *l == label) {
            Some(e) => e.1 += amount,
            None => by_cat.push((label, amount)),
        }
        spent += amount;
        count += 1;
    }
    by_cat.sort_by_key(|(_, a)| -a);
    by_cat.truncate(3);
    let data = json!({
        "w": range,
        "t": chores.iter().enumerate().map(|(i, (n, k))| json!([n, k, COLORS[i % 4]])).collect::<Vec<_>>(),
        "g": eur(spent),
        "n": count,
        "c": by_cat.iter().map(|(l, a)| json!([l, eur(*a)])).collect::<Vec<_>>(),
    })
    .to_string();
    let fallback = format!(
        "<b>Resumen de la semana</b>\nTareas: {}\nTus gastos: {} en {} compra{}{}",
        chores.iter().map(|(n, k)| format!("{} {k}", esc(n))).collect::<Vec<_>>().join(" · "),
        eur(spent),
        count,
        if count == 1 { "" } else { "s" },
        by_cat.iter().map(|(l, a)| format!("\n• {l}: {}", eur(*a))).collect::<String>(),
    );
    Ok((data, fallback))
}

fn tick(db: &Db, token: &str) {
    // Everything is prepared with the database locked, then sent without it.
    let (daily_out, weekly_out) = {
        let c = lock(db);
        let Ok((now, day, sunday)) = c.query_row(
            "SELECT strftime('%H:%M','now','localtime'), date('now','localtime'), strftime('%w','now','localtime')='0'",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, bool>(2)?)),
        ) else {
            return;
        };
        if now < reminder_time(&c) || setting(&c, "tg_last").as_deref() == Some(day.as_str()) {
            return;
        }
        let _ = set_setting(&c, "tg_last", &day);
        let _ = c.execute("DELETE FROM settings WHERE key LIKE 'tg_daily_%'", []);
        let linked: Vec<(i64, i64)> = c
            .prepare("SELECT id, tg_chat FROM users WHERE tg_chat IS NOT NULL")
            .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
            .unwrap_or_default();
        let daily_out: Vec<_> = linked.iter().filter_map(|&(u, chat)| daily(&c, u).map(|d| (chat, d))).collect();
        let weekly_out: Vec<_> = if sunday { linked.iter().filter_map(|&(u, chat)| weekly(&c, u).ok().map(|w| (chat, w))).collect() } else { vec![] };
        (daily_out, weekly_out)
    };
    for (chat, d) in daily_out {
        deliver_daily(db, token, chat, d);
    }
    for (chat, (data, fallback)) in weekly_out {
        send_widget(token, chat, WEEKLY_WIDGET, &data, "Resumen de la semana", false, &fallback);
    }
}

fn on_message(db: &Db, token: &str, m: &Value) {
    let chat = m["chat"]["id"].as_i64().unwrap_or(0);
    if m["chat"]["type"].as_str() != Some("private") {
        return;
    }
    let text = m["text"].as_str().unwrap_or_default().trim();
    if let Some(code) = text.strip_prefix("/start ").map(str::trim).filter(|s| !s.is_empty()) {
        let reply = {
            let c = lock(db);
            let _ = c.execute("UPDATE users SET tg_chat=NULL WHERE tg_chat=?1", [chat]);
            let linked = c.execute("UPDATE users SET tg_chat=?1, tg_code=NULL WHERE tg_code=?2", params![chat, code]).unwrap_or(0);
            let name: Option<String> = c.query_row("SELECT name FROM users WHERE tg_chat=?1", [chat], |r| r.get(0)).ok();
            match (linked, name) {
                (1, Some(n)) => format!("Hola, {}. Aquí te avisaré de las tareas que te planifiquen, de lo pendiente cada día a las {} y, los domingos, del resumen de la semana.\n\nEscríbeme cuando quieras para ver lo que queda para hoy.", esc(&n), reminder_time(&c)),
                _ => "Este enlace ya no vale. Ábrelo de nuevo desde Nido → Ajustes → Avisos por Telegram.".into(),
            }
        };
        let _ = send_text(token, chat, &reply);
        return;
    }
    let user = lock(db).query_row("SELECT id FROM users WHERE tg_chat=?1", [chat], |r| r.get::<_, i64>(0)).ok();
    match user {
        Some(user) => {
            let today_list = daily(&lock(db), user); // the lock ends here, before sending
            match today_list {
                Some(d) => deliver_daily(db, token, chat, d),
                None => {
                    let _ = send_text(token, chat, "Nada pendiente para hoy.");
                }
            }
        }
        None => {
            let _ = send_text(token, chat, "Para recibir avisos, abre Nido → Ajustes → Avisos por Telegram y pulsa «Vincular mi Telegram».");
        }
    }
}

pub fn start(db: Db) {
    std::thread::spawn(move || {
        let mut offset = 0i64;
        let mut profiled = String::new();
        loop {
            let Some(token) = setting(&lock(&db), "tg_token") else {
                std::thread::sleep(std::time::Duration::from_secs(20));
                continue;
            };
            // Once per start (and per token): picks up a new bot name or photo.
            if profiled != token {
                let _ = refresh_profile(&db, &token);
                profiled = token.clone();
            }
            let off = offset.to_string();
            let updates = api(&token, "getUpdates", &[
                ("offset", &off),
                ("timeout", "50"),
                ("allowed_updates", r#"["message","widget_query","shared_widget_query"]"#),
            ], 50);
            match updates {
                Ok(Value::Array(list)) => {
                    for u in list {
                        offset = offset.max(u["update_id"].as_i64().unwrap_or(0) + 1);
                        if u["message"].is_object() {
                            on_message(&db, &token, &u["message"]);
                        } else if u["widget_query"].is_object() {
                            widget_done(&db, &token, &u["widget_query"]);
                        }
                    }
                }
                _ => std::thread::sleep(std::time::Duration::from_secs(30)),
            }
            tick(&db, &token);
        }
    });
}

pub struct Panel {
    pub bot: Option<String>,
    pub title: Option<String>,
    pub photo: i64,
    pub at: String,
    pub linked: Vec<(i64, bool)>,
    pub code: String,
}

pub fn panel(c: &Connection, me: i64) -> rusqlite::Result<Panel> {
    let linked = c.prepare("SELECT id, tg_chat IS NOT NULL FROM users ORDER BY id")?.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    let code = match c.query_row("SELECT tg_code FROM users WHERE id=?1", [me], |r| r.get::<_, Option<String>>(0))? {
        Some(code) => code,
        None => {
            let code = rand_hex(12);
            c.execute("UPDATE users SET tg_code=?2 WHERE id=?1", params![me, code])?;
            code
        }
    };
    let photo = setting(c, "tg_photo").and_then(|v| v.parse().ok()).unwrap_or(0);
    Ok(Panel { bot: setting(c, "tg_bot"), title: setting(c, "tg_title").filter(|t| !t.is_empty()), photo, at: reminder_time(c), linked, code })
}

#[derive(Deserialize)]
pub struct BotF {
    token: Option<String>,
    at: Option<String>,
    month: String,
    tab: Option<String>,
}

fn to_settings(c: &Connection, f: &BotF, ok: &str) -> R {
    back(&month(c, Some(&f.month))?, Some(nav::tab(f.tab.as_deref())), ok)
}

pub async fn connect_bot(State(db): State<Db>, h: HeaderMap, F(f): F<BotF>) -> R {
    require_user!(lock(&db), h);
    let token = f.token.as_deref().unwrap_or_default().trim().to_string();
    if token.is_empty() || !token.contains(':') {
        return bad("Pega el token completo que te dio @BotFather (tiene la forma 123456:ABC…).");
    }
    let check = token.clone();
    let name = match tokio::task::spawn_blocking(move || bot_name(&check)).await? {
        Ok(name) => name,
        Err(_) => return bad("Telegram no reconoce ese token. Cópialo otra vez desde @BotFather."),
    };
    {
        let c = lock(&db);
        set_setting(&c, "tg_token", &token)?;
        set_setting(&c, "tg_bot", &name)?;
    }
    let (db2, t2) = (db.clone(), token.clone());
    let _ = tokio::task::spawn_blocking(move || refresh_profile(&db2, &t2)).await;
    let c = lock(&db);
    to_settings(&c, &f, "tg_connected")
}

pub async fn remove_bot(State(db): State<Db>, h: HeaderMap, F(f): F<BotF>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    c.execute("DELETE FROM settings WHERE key LIKE 'tg_%'", [])?;
    let _ = std::fs::remove_file(bot_photo_path());
    c.execute("UPDATE users SET tg_chat=NULL, tg_code=NULL", [])?;
    to_settings(&c, &f, "tg_removed")
}

pub async fn save_time(State(db): State<Db>, h: HeaderMap, F(f): F<BotF>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    let at = f.at.as_deref().unwrap_or_default();
    let valid = at.len() == 5 && at.as_bytes()[2] == b':' && at[..2].parse::<u8>().is_ok_and(|h| h < 24) && at[3..].parse::<u8>().is_ok_and(|m| m < 60);
    if !valid {
        return bad("Hora no válida");
    }
    set_setting(&c, "tg_at", at)?;
    to_settings(&c, &f, "tg_time")
}

pub async fn test(State(db): State<Db>, h: HeaderMap, F(f): F<BotF>) -> R {
    let (token, chat) = {
        let c = lock(&db);
        let me = require_user!(c, h);
        (setting(&c, "tg_token"), chat_of(&c, me.id))
    };
    let (Some(token), Some(chat)) = (token, chat) else { return bad("Primero vincula tu Telegram.") };
    let sent = tokio::task::spawn_blocking(move || send_text(&token, chat, "<b>Nido</b>\nTodo listo: aquí te llegarán los avisos de casa.")).await?;
    if sent.is_err() {
        return bad("Telegram no ha aceptado el mensaje. ¿Has bloqueado o borrado el chat con el bot?");
    }
    to_settings(&lock(&db), &f, "tg_test")
}

pub async fn unlink(State(db): State<Db>, h: HeaderMap, F(f): F<BotF>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    c.execute("UPDATE users SET tg_chat=NULL WHERE id=?1", [me.id])?;
    to_settings(&c, &f, "tg_unlinked")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_list_and_its_size() {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c);
        c.execute_batch(
            "INSERT INTO users(id,name,pass,token) VALUES(1,'Natalia','x','a'),(2,'Michael','x','b');
             INSERT INTO chores(id,name,icon,color,every_days) VALUES
               (1,'Barrer','brush','#d97706',2),(2,'Fregar','utensils','#2f6bff',1),(3,'Sábanas','bed','#52525b',14),(4,'Cocinar','chef-hat','#d97706',NULL);
             INSERT INTO chore_logs(chore_id,user_id,day) VALUES
               (1,1,date('now','localtime','-5 days')),(2,2,date('now','localtime')),(3,1,date('now','localtime','-1 days'));
             INSERT INTO chore_plans(id,chore_id,day,assignee_id) VALUES
               (7,4,date('now','localtime'),2),(8,3,date('now','localtime'),NULL),(9,4,date('now','localtime','+1 days'),1);",
        )
        .unwrap();
        let keys = |u| todo(&c, u).unwrap().into_iter().map(|(k, ..)| k).collect::<Vec<_>>();
        assert_eq!(keys(1), ["p8", "c1"]);
        assert_eq!(keys(2), ["p7", "p8", "c1"]);
        let many: Vec<_> = (0..60).map(|i| (format!("c{i}"), "Una tarea con un nombre bastante largo".to_string(), "Atrasada 3 días".to_string(), "#2f6bff".to_string(), false)).collect();
        let data = daily_data("Para hoy, jueves 12", &many);
        assert!(data.len() <= 1024 && serde_json::from_str::<Value>(&data).unwrap()["i"].as_array().unwrap().len() > 5);
    }
}

pub fn send_now(db: &Db, name: &str) -> Result<(), String> {
    let (token, chat, d, w) = {
        let c = lock(db);
        let token = setting(&c, "tg_token").ok_or("No hay bot conectado (Ajustes → Control de la casa)")?;
        let (user, chat): (i64, Option<i64>) = c.query_row("SELECT id, tg_chat FROM users WHERE name=?1", [name], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|_| format!("No existe «{name}»"))?;
        let chat = chat.ok_or(format!("«{name}» no ha vinculado su Telegram"))?;
        (token, chat, daily(&c, user), weekly(&c, user).map_err(|e| e.to_string())?)
    };
    match d {
        Some(d) => deliver_daily(db, &token, chat, d),
        None => {
            send_text(&token, chat, "Nada pendiente para hoy.")?;
        }
    }
    send_widget(&token, chat, WEEKLY_WIDGET, &w.0, "Resumen de la semana", false, &w.1);
    Ok(())
}
