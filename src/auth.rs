use super::*;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub struct User {
    pub id: i64,
    pub name: String,
    pub salary: i64,
    pub token: String,
}

pub fn rand_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).expect("rng");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn hash(p: &str) -> String {
    use argon2::password_hash::{PasswordHasher, SaltString};
    let mut s = [0u8; 16];
    getrandom::fill(&mut s).expect("rng");
    let salt = SaltString::encode_b64(&s).expect("salt");
    argon2::Argon2::default().hash_password(p.as_bytes(), &salt).expect("hash").to_string()
}

pub fn verify(p: &str, h: &str) -> bool {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    PasswordHash::new(h)
        .map(|h| argon2::Argon2::default().verify_password(p.as_bytes(), &h).is_ok())
        .unwrap_or(false)
}

pub fn token_hash(t: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(t.as_bytes()).iter().map(|x| format!("{x:02x}")).collect()
}

pub fn sid(h: &HeaderMap) -> Option<&str> {
    h.get(header::COOKIE)?.to_str().ok()?.split(';').find_map(|p| p.trim().strip_prefix("sid="))
}

pub fn current_user(c: &Connection, h: &HeaderMap) -> Option<User> {
    c.query_row(
        "SELECT u.id,u.name,u.salary,u.token FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token=?1",
        [token_hash(sid(h)?)],
        |r| Ok(User { id: r.get(0)?, name: r.get(1)?, salary: r.get(2)?, token: r.get(3)? }),
    )
    .ok()
}

macro_rules! require_user {
    ($c:expr, $h:expr) => {
        match $crate::auth::current_user(&$c, &$h) {
            Some(u) => u,
            None => return Ok(axum::response::IntoResponse::into_response(axum::response::Redirect::to("/login"))),
        }
    };
}

const FREE_TRIES: u32 = 5;
const MAX_WAIT: u64 = 15 * 60;

struct Strike {
    fails: u32,
    until: Instant,
}

fn strikes() -> MutexGuard<'static, HashMap<String, Strike>> {
    static S: OnceLock<Mutex<HashMap<String, Strike>>> = OnceLock::new();
    S.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

fn wait_secs(keys: &[String]) -> u64 {
    let now = Instant::now();
    let s = strikes();
    keys.iter().filter_map(|k| s.get(k)).map(|s| s.until.saturating_duration_since(now).as_secs_f64().ceil() as u64).max().unwrap_or(0)
}

fn strike(keys: &[String]) {
    let now = Instant::now();
    let mut s = strikes();
    if s.len() > 1024 {
        s.retain(|_, v| v.until > now);
    }
    for k in keys {
        let e = s.entry(k.clone()).or_insert(Strike { fails: 0, until: now });
        e.fails += 1;
        if e.fails >= FREE_TRIES {
            e.until = now + Duration::from_secs((30u64 << (e.fails - FREE_TRIES).min(5)).min(MAX_WAIT));
        }
    }
}

fn forgive(keys: &[String]) {
    let mut s = strikes();
    for k in keys {
        s.remove(k);
    }
}

fn client_ip(h: &HeaderMap) -> String {
    h.get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "local".into())
}

fn dummy_hash() -> &'static str {
    static H: OnceLock<String> = OnceLock::new();
    H.get_or_init(|| hash(&rand_hex(16)))
}

fn wait_text(s: u64) -> String {
    if s < 60 { format!("{s} s") } else { format!("{} min", s.div_ceil(60)) }
}

struct Face {
    id: i64,
    name: String,
    photo: i64,
    color: String,
}

fn face(u: &Face, class: &str) -> Markup {
    html! {
        @if u.photo > 0 {
            img class={ "face " (class) } src={ "/avatars/" (u.id) "?v=" (u.photo) } alt="" width="88" height="88";
        } @else {
            span class={ "face " (class) } style={ "background:" (u.color) } aria-hidden="true" { (initial(&u.name)) }
        }
    }
}

fn wallpaper() -> Markup {
    static SVG: OnceLock<String> = OnceLock::new();
    let svg = SVG.get_or_init(|| {
        const ROWS: [[&str; 4]; 3] = [
            ["house", "coffee", "sprout", "key-round"],
            ["sofa", "shopping-cart", "lamp", "utensils"],
            ["heart", "washing-machine", "carrot", "bed"],
        ];
        const TILT: [i32; 6] = [-12, 8, -4, 14, -8, 4];
        let mut tile = String::new();
        for (r, row) in ROWS.iter().enumerate() {
            for (k, name) in row.iter().enumerate() {
                let inner = merchants::ICONS.iter().find(|(n, _)| n == name).map_or("", |(_, s)| *s);
                let x = k * 72 + if r % 2 == 1 { 36 } else { 0 } + 24;
                let y = r * 72 + 24;
                let t = TILT[(r * 4 + k) % TILT.len()];
                tile.push_str(&format!(r#"<g transform="translate({x} {y}) rotate({t} 12 12)">{inner}</g>"#));
            }
        }
        format!(
            r#"<svg class="login-bg" aria-hidden="true" focusable="false"><defs><pattern id="login-tile" width="288" height="216" patternUnits="userSpaceOnUse" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">{tile}</pattern></defs><rect width="100%" height="100%" fill="url(#login-tile)"/></svg>"#
        )
    });
    PreEscaped(svg.clone())
}

#[derive(Deserialize)]
pub struct LoginQ {
    u: Option<i64>,
    error: Option<String>,
    wait: Option<u64>,
}

pub async fn login_page(State(db): State<Db>, Query(q): Query<LoginQ>) -> R {
    let c = lock(&db);
    let mut people: Vec<Face> = c
        .prepare("SELECT id,name,photo,color FROM users ORDER BY id")?
        .query_map([], |r| {
            Ok(Face { id: r.get(0)?, name: r.get(1)?, photo: r.get(2)?, color: r.get::<_, Option<String>>(3)?.unwrap_or_default() })
        })?
        .collect::<Result<_, _>>()?;
    for (i, u) in people.iter_mut().enumerate() {
        if !PALETTE.contains(&u.color.as_str()) {
            u.color = PALETTE[i % PALETTE.len()].to_string();
        }
    }
    let theme: Option<String> = c.query_row("SELECT value FROM settings WHERE key='theme'", [], |r| r.get(0)).optional()?;
    let hour: i64 = c.query_row("SELECT CAST(strftime('%H','now','localtime') AS INTEGER)", [], |r| r.get(0))?;
    drop(c);
    let greeting = match hour {
        6..=13 => "Buenos días",
        14..=20 => "Buenas tardes",
        _ => "Buenas noches",
    };
    let only = (people.len() == 1).then_some(0);
    let me = q.u.and_then(|id| people.iter().position(|u| u.id == id)).or(only);
    let wait = q.wait.filter(|w| *w > 0);
    let body = html! {
        (wallpaper())
        main.login {
            header.login-brand {
                img src="/icon.png" alt="" width="72" height="72";
                div { h2 { "Nido" } p { "Lo de casa, en orden" } }
            }
            @if let Some(i) = me {
                @let u = &people[i];
                form.card.login-card.login-me method="post" action="/login" accept-charset="utf-8" style={ "--pc:" (u.color) } {
                    (face(u, "lg"))
                    div.login-head {
                        h1 { "Hola, " (u.name) }
                        p.muted { "Escribe tu contraseña para entrar." }
                    }
                    input.vh type="text" name="name" value=(u.name) autocomplete="username" readonly tabindex="-1" aria-hidden="true";
                    div.field {
                        label for="password" { "Contraseña" }
                        input #password.input type="password" name="password" autocomplete="current-password" required autofocus
                            aria-invalid=[(q.error.is_some() || wait.is_some()).then_some("true")] aria-describedby=[(q.error.is_some() || wait.is_some()).then_some("pw-err")];
                        @if let Some(w) = wait {
                            p #pw-err.field-error role="alert" { (icon("clock")) "Demasiados intentos. Espera " (wait_text(w)) " y vuelve a probar." }
                        } @else if q.error.is_some() {
                            p #pw-err.field-error role="alert" { (icon("info")) "No es esa contraseña. Revísala e inténtalo otra vez." }
                        }
                    }
                    button.btn.primary.wide.lg { "Entrar" }
                    @if people.len() > 1 {
                        a.not-me href="/login" { (icon("arrow-left")) "No soy " (u.name) }
                    }
                }
            } @else {
                section.card.login-card aria-labelledby="who-title" {
                    div.login-head {
                        h1 #who-title { (greeting) }
                        p.muted { "¿Quién entra?" }
                    }
                    @if people.is_empty() {
                        p.muted { "Todavía no hay nadie en esta casa. Crea el primer usuario desde la Raspberry, como explica la guía de instalación." }
                    } @else {
                        ul.who-list role="list" {
                            @for u in &people {
                                li { a.login-who href={ "/login?u=" (u.id) } style={ "--pc:" (u.color) } { (face(u, "md")) span.who-name { (u.name) } } }
                            }
                        }
                    }
                }
            }
        }
    };
    let class = format!("login-body theme-{}", theme_id(theme.as_deref().unwrap_or("")));
    Ok(page("Entrar · Nido", &class, body))
}

#[derive(Deserialize)]
pub struct LoginF {
    name: String,
    password: String,
}

pub async fn login(State(db): State<Db>, h: HeaderMap, F(f): F<LoginF>) -> R {
    let row: Option<(i64, String)> = lock(&db)
        .query_row("SELECT id,pass FROM users WHERE name=?1 COLLATE NOCASE", [f.name.trim()], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let ip = format!("ip:{}", client_ip(&h));
    let keys = match &row {
        Some((id, _)) => vec![ip, format!("user:{id}")],
        None => vec![ip],
    };
    let back = |q: &str| match &row {
        Some((id, _)) => Redirect::to(&format!("/login?u={id}&{q}")).into_response(),
        None => Redirect::to(&format!("/login?{q}")).into_response(),
    };
    let wait = wait_secs(&keys);
    if wait > 0 {
        return Ok(back(&format!("wait={wait}")));
    }
    let stored = row.as_ref().map_or_else(|| dummy_hash().to_string(), |r| r.1.clone());
    let ok = tokio::task::spawn_blocking(move || verify(&f.password, &stored)).await?;
    match row {
        Some((id, _)) if ok => {
            forgive(&keys);
            let sid = rand_hex(32);
            lock(&db).execute("INSERT INTO sessions(token,user_id) VALUES(?1,?2)", params![token_hash(&sid), id])?;
            let cookie = format!("sid={sid}; Path=/; HttpOnly; SameSite=Lax; Max-Age=31536000");
            Ok(([(header::SET_COOKIE, cookie)], Redirect::to("/")).into_response())
        }
        _ => {
            strike(&keys);
            let wait = wait_secs(&keys);
            Ok(back(&if wait > 0 { format!("wait={wait}") } else { "error=1".into() }))
        }
    }
}

pub async fn logout(State(db): State<Db>, h: HeaderMap) -> R {
    if let Some(s) = sid(&h) {
        lock(&db).execute("DELETE FROM sessions WHERE token=?1", [token_hash(s)])?;
    }
    Ok(([(header::SET_COOKIE, "sid=; Path=/; Max-Age=0")], Redirect::to("/login")).into_response())
}

#[derive(Deserialize)]
pub struct PasswordF {
    current: String,
    new: String,
    repeat: String,
    month: String,
}

/// Mis ajustes → Contraseña. Signs out every other device of this user.
pub async fn change_password(State(db): State<Db>, h: HeaderMap, F(f): F<PasswordF>) -> R {
    let (me, stored) = {
        let c = lock(&db);
        let me = require_user!(c, h);
        let stored: String = c.query_row("SELECT pass FROM users WHERE id=?1", [me.id], |r| r.get(0))?;
        (me, stored)
    };
    let current = f.current.clone();
    if !tokio::task::spawn_blocking(move || verify(&current, &stored)).await? {
        return bad("La contraseña actual no es correcta.");
    }
    if f.new.chars().count() < 4 {
        return bad("La nueva contraseña necesita al menos 4 caracteres.");
    }
    if f.new != f.repeat {
        return bad("Las dos contraseñas nuevas no coinciden.");
    }
    let new = f.new.clone();
    let hashed = tokio::task::spawn_blocking(move || hash(&new)).await?;
    let c = lock(&db);
    c.execute("UPDATE users SET pass=?2 WHERE id=?1", params![me.id, hashed])?;
    c.execute("DELETE FROM sessions WHERE user_id=?1 AND token!=?2", params![me.id, token_hash(sid(&h).unwrap_or_default())])?;
    back(&month(&c, Some(&f.month))?, Some("settings"), "password_saved")
}
