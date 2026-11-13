use super::*;

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

pub fn sid(h: &HeaderMap) -> Option<&str> {
    h.get(header::COOKIE)?.to_str().ok()?.split(';').find_map(|p| p.trim().strip_prefix("sid="))
}

pub fn current_user(c: &Connection, h: &HeaderMap) -> Option<User> {
    c.query_row(
        "SELECT u.id,u.name,u.salary,u.token FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token=?1",
        [sid(h)?],
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

#[derive(Deserialize)]
pub struct LoginQ {
    error: Option<String>,
}

pub async fn login_page(Query(q): Query<LoginQ>) -> Response {
    page(
        "Entrar · Nido",
        "login-body",
        html! {
            main.login {
                div.brand.center { (brand()) }
                form.card.login-card method="post" action="/login" accept-charset="utf-8" {
                    h1 { "Entra en Nido" }
                    p.muted { "Usa tu nombre y tu contraseña" }
                    @if q.error.is_some() { p.alert role="alert" { (icon("info")) "El nombre o la contraseña no coinciden. Revísalos e inténtalo de nuevo." } }
                    div.field { label for="name" { "Nombre" } input #name.input type="text" name="name" autocomplete="username" autocapitalize="none" required autofocus; }
                    div.field { label for="password" { "Contraseña" } input #password.input type="password" name="password" autocomplete="current-password" required; }
                    button.btn.primary.wide { "Entrar" }
                }
            }
        },
    )
}

#[derive(Deserialize)]
pub struct LoginF {
    name: String,
    password: String,
}

pub async fn login(State(db): State<Db>, F(f): F<LoginF>) -> R {
    let c = lock(&db);
    let row: Option<(i64, String)> = c
        .query_row("SELECT id,pass FROM users WHERE name=?1 COLLATE NOCASE", [f.name.trim()], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    match row {
        Some((id, h)) if verify(&f.password, &h) => {
            let sid = rand_hex(32);
            c.execute("INSERT INTO sessions(token,user_id) VALUES(?1,?2)", params![sid, id])?;
            let cookie = format!("sid={sid}; Path=/; HttpOnly; SameSite=Lax; Max-Age=31536000");
            Ok(([(header::SET_COOKIE, cookie)], Redirect::to("/")).into_response())
        }
        _ => Ok(Redirect::to("/login?error=1").into_response()),
    }
}

pub async fn logout(State(db): State<Db>, h: HeaderMap) -> R {
    if let Some(s) = sid(&h) {
        lock(&db).execute("DELETE FROM sessions WHERE token=?1", [s])?;
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
    let c = lock(&db);
    let me = require_user!(c, h);
    let stored: String = c.query_row("SELECT pass FROM users WHERE id=?1", [me.id], |r| r.get(0))?;
    if !verify(&f.current, &stored) {
        return bad("La contraseña actual no es correcta.");
    }
    if f.new.chars().count() < 4 {
        return bad("La nueva contraseña necesita al menos 4 caracteres.");
    }
    if f.new != f.repeat {
        return bad("Las dos contraseñas nuevas no coinciden.");
    }
    c.execute("UPDATE users SET pass=?2 WHERE id=?1", params![me.id, hash(&f.new)])?;
    c.execute("DELETE FROM sessions WHERE user_id=?1 AND token!=?2", params![me.id, sid(&h).unwrap_or_default()])?;
    back(&month(&c, Some(&f.month))?, Some("settings"), "password_saved")
}
