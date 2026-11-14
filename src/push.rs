//! Web Push: notifications on the phones that turned them on, even with Nido closed. Each browser
//! gives us an address at its push service (Apple, Google, Mozilla); we send there, encrypted for
//! that phone (RFC 8291) and signed with the house's key (VAPID, RFC 8292). On iPhone it works
//! from the icon on the home screen (iOS 16.4+), over https.
use super::*;
use aes_gcm::{aead::Aead, Aes128Gcm, KeyInit, Nonce};
use hkdf::Hkdf;
use p256::{
    ecdsa::{signature::Signer, Signature, SigningKey},
    elliptic_curve::sec1::ToEncodedPoint,
    PublicKey, SecretKey,
};
use sha2::Sha256;
use std::io::Write;
use std::process::{Command, Stdio};

const SUB: &str = "https://github.com/michyaraque/nido";

/// Phones that said "gone" (unsubscribed or uninstalled): forgotten on the next send.
static GONE: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn b64(b: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(b.len() * 4 / 3 + 3);
    for ch in b.chunks(3) {
        let n = (ch[0] as u32) << 16 | (*ch.get(1).unwrap_or(&0) as u32) << 8 | *ch.get(2).unwrap_or(&0) as u32;
        for i in 0..=ch.len() {
            out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

pub fn unb64(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'-' | b'+' => Some(62),
        b'_' | b'/' => Some(63),
        _ => None,
    };
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for ch in s.as_bytes().chunks(4) {
        let mut n = 0u32;
        for (i, &c) in ch.iter().enumerate() {
            n |= (val(c)? as u32) << (18 - 6 * i);
        }
        out.extend_from_slice(&n.to_be_bytes()[1..ch.len()]);
    }
    Some(out)
}

fn new_key() -> SecretKey {
    loop {
        let mut b = [0u8; 32];
        getrandom::fill(&mut b).expect("rng");
        if let Ok(k) = SecretKey::from_slice(&b) {
            return k;
        }
    }
}

/// The house's signing key, made the first time it is needed.
fn vapid(c: &Connection) -> SecretKey {
    let stored = crate::notify::setting(c, "push_key").and_then(|h| (0..h.len()).step_by(2).map(|i| u8::from_str_radix(h.get(i..i + 2)?, 16).ok()).collect::<Option<Vec<u8>>>());
    if let Some(k) = stored.and_then(|b| SecretKey::from_slice(&b).ok()) {
        return k;
    }
    let k = new_key();
    let hex: String = k.to_bytes().iter().map(|x| format!("{x:02x}")).collect();
    let _ = crate::notify::set_setting(c, "push_key", &hex);
    k
}

/// What the browser needs to subscribe (`applicationServerKey`).
pub fn public_key(c: &Connection) -> String {
    b64(vapid(c).public_key().to_encoded_point(false).as_bytes())
}

/// The message for one phone, encrypted with its keys (`aes128gcm`, one record).
fn encrypt(p256dh: &[u8], auth: &[u8], payload: &[u8]) -> Option<Vec<u8>> {
    let phone = PublicKey::from_sec1_bytes(p256dh).ok()?;
    let ours = new_key();
    let ours_pub = ours.public_key().to_encoded_point(false);
    let shared = p256::ecdh::diffie_hellman(ours.to_nonzero_scalar(), phone.as_affine());
    let mut info = b"WebPush: info\0".to_vec();
    info.extend_from_slice(p256dh);
    info.extend_from_slice(ours_pub.as_bytes());
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes()).expand(&info, &mut ikm).ok()?;
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).ok()?;
    let hk = Hkdf::<Sha256>::new(Some(&salt), &ikm);
    let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
    hk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).ok()?;
    hk.expand(b"Content-Encoding: nonce\0", &mut nonce).ok()?;
    let mut plain = payload.to_vec();
    plain.push(2);
    let sealed = Aes128Gcm::new_from_slice(&cek).ok()?.encrypt(Nonce::from_slice(&nonce), plain.as_ref()).ok()?;
    let mut out = salt.to_vec();
    out.extend_from_slice(&4096u32.to_be_bytes());
    out.push(65);
    out.extend_from_slice(ours_pub.as_bytes());
    out.extend(sealed);
    Some(out)
}

/// `Authorization` for one push service: a short-lived token signed with the house's key.
fn authorization(key: &SecretKey, endpoint: &str) -> Option<String> {
    let aud: String = endpoint.splitn(4, '/').take(3).collect::<Vec<_>>().join("/");
    let exp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() + 12 * 3600;
    let head = b64(br#"{"typ":"JWT","alg":"ES256"}"#);
    let claims = b64(serde_json::json!({ "aud": aud, "exp": exp, "sub": SUB }).to_string().as_bytes());
    let msg = format!("{head}.{claims}");
    let sig: Signature = SigningKey::from(key).sign(msg.as_bytes());
    Some(format!("vapid t={msg}.{}, k={}", b64(&sig.to_bytes()), b64(key.public_key().to_encoded_point(false).as_bytes())))
}

fn deliver(endpoint: &str, auth: &str, body: &[u8]) -> Result<u16, String> {
    let mut child = Command::new("curl")
        .args(["-s", "-o", if cfg!(windows) { "NUL" } else { "/dev/null" }, "-w", "%{http_code}", "--max-time", "20", "-X", "POST", "--data-binary", "@-"])
        .args(["-H", &format!("Authorization: {auth}"), "-H", "Content-Encoding: aes128gcm", "-H", "Content-Type: application/octet-stream", "-H", "TTL: 86400", "-H", "Urgency: high"])
        .arg(endpoint)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    child.stdin.take().ok_or("curl")?.write_all(body).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    String::from_utf8_lossy(&out.stdout).trim().parse().map_err(|_| "sin respuesta".into())
}

/// Sends a notification to every phone of `user` that turned them on. Returns whether they have
/// any (if not, the caller may try Telegram). `url` opens when they tap it.
pub fn send(c: &Connection, user: i64, title: &str, body: &str, url: &str) -> bool {
    for gone in std::mem::take(&mut *GONE.lock().unwrap_or_else(|e| e.into_inner())) {
        let _ = c.execute("DELETE FROM push_subs WHERE endpoint=?1", [gone]);
    }
    let subs: Vec<(String, String, String)> = c
        .prepare("SELECT endpoint, p256dh, auth FROM push_subs WHERE user_id=?1")
        .and_then(|mut s| s.query_map([user], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect())
        .unwrap_or_default();
    if subs.is_empty() {
        return false;
    }
    let key = vapid(c);
    let payload = serde_json::json!({ "title": title, "body": body, "url": url }).to_string();
    let jobs: Vec<(String, String, Vec<u8>)> = subs
        .into_iter()
        .filter_map(|(ep, p, a)| {
            let body = encrypt(&unb64(&p)?, &unb64(&a)?, payload.as_bytes())?;
            Some((authorization(&key, &ep)?, ep, body))
        })
        .collect();
    std::thread::spawn(move || {
        for (auth, ep, body) in jobs {
            match deliver(&ep, &auth, &body) {
                Ok(404 | 410) => GONE.lock().unwrap_or_else(|e| e.into_inner()).push(ep),
                Ok(s) if s >= 300 => eprintln!("push: {s} desde {}", ep.split('/').nth(2).unwrap_or("?")),
                Err(e) => eprintln!("push: {e}"),
                _ => {}
            }
        }
    });
    true
}

// ---------- the browser side: subscribe, unsubscribe, try ----------

#[derive(Deserialize)]
pub struct Sub {
    endpoint: String,
    keys: Option<Keys>,
}
#[derive(Deserialize)]
struct Keys {
    p256dh: String,
    auth: String,
}

pub async fn key(State(db): State<Db>, h: HeaderMap) -> R {
    let c = lock(&db);
    require_user!(c, h);
    Ok(([(header::CACHE_CONTROL, "no-store")], public_key(&c)).into_response())
}

pub async fn subscribe(State(db): State<Db>, h: HeaderMap, axum::Json(s): axum::Json<Sub>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let Some(k) = s.keys else { return Ok((StatusCode::BAD_REQUEST, "faltan las claves").into_response()) };
    let ok = s.endpoint.starts_with("https://") && s.endpoint.len() < 1024 && !s.endpoint.contains(char::is_whitespace)
        && unb64(&k.p256dh).is_some_and(|b| b.len() == 65) && unb64(&k.auth).is_some_and(|b| b.len() == 16);
    if !ok {
        return Ok((StatusCode::BAD_REQUEST, "suscripción no válida").into_response());
    }
    c.execute(
        "INSERT INTO push_subs(endpoint,user_id,p256dh,auth) VALUES(?1,?2,?3,?4)
         ON CONFLICT(endpoint) DO UPDATE SET user_id=excluded.user_id, p256dh=excluded.p256dh, auth=excluded.auth",
        params![s.endpoint, me.id, k.p256dh, k.auth],
    )?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn unsubscribe(State(db): State<Db>, h: HeaderMap, axum::Json(s): axum::Json<Sub>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    c.execute("DELETE FROM push_subs WHERE endpoint=?1 AND user_id=?2", params![s.endpoint, me.id])?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn test(State(db): State<Db>, h: HeaderMap) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    if !send(&c, me.id, "Nido", "Así te llegarán los avisos.", "/?tab=settings") {
        return Ok((StatusCode::CONFLICT, "Este móvil aún no tiene los avisos activados").into_response());
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub fn routes() -> Router<Db> {
    Router::new()
        .route("/push/key", get(key))
        .route("/push/subscribe", post(subscribe))
        .route("/push/unsubscribe", post(unsubscribe))
        .route("/push/test", post(test))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_round_trip() {
        for n in 0..70 {
            let b: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(unb64(&b64(&b)).unwrap(), b);
        }
        assert_eq!(b64(b"hi?"), "aGk_");
    }

    /// The phone can open what we send: decrypt it the way the browser does (RFC 8291).
    #[test]
    fn a_phone_can_read_it() {
        let phone = new_key();
        let phone_pub = phone.public_key().to_encoded_point(false);
        let auth = [7u8; 16];
        let body = encrypt(phone_pub.as_bytes(), &auth, b"hola").unwrap();
        let (salt, rs, idlen) = (&body[..16], u32::from_be_bytes(body[16..20].try_into().unwrap()), body[20] as usize);
        assert_eq!((rs, idlen), (4096, 65));
        let server = PublicKey::from_sec1_bytes(&body[21..21 + idlen]).unwrap();
        let shared = p256::ecdh::diffie_hellman(phone.to_nonzero_scalar(), server.as_affine());
        let mut info = b"WebPush: info\0".to_vec();
        info.extend_from_slice(phone_pub.as_bytes());
        info.extend_from_slice(&body[21..21 + idlen]);
        let mut ikm = [0u8; 32];
        Hkdf::<Sha256>::new(Some(&auth), shared.raw_secret_bytes()).expand(&info, &mut ikm).unwrap();
        let hk = Hkdf::<Sha256>::new(Some(salt), &ikm);
        let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
        hk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).unwrap();
        hk.expand(b"Content-Encoding: nonce\0", &mut nonce).unwrap();
        let plain = Aes128Gcm::new_from_slice(&cek).unwrap().decrypt(Nonce::from_slice(&nonce), &body[21 + idlen..]).unwrap();
        assert_eq!(plain, b"hola\x02");
    }

    #[test]
    fn signed_for_the_push_service() {
        let key = new_key();
        let a = authorization(&key, "https://web.push.apple.com/QGuQyavXutnMH").unwrap();
        let t = a.strip_prefix("vapid t=").unwrap().split(", k=").next().unwrap();
        let claims: serde_json::Value = serde_json::from_slice(&unb64(t.split('.').nth(1).unwrap()).unwrap()).unwrap();
        assert_eq!(claims["aud"], "https://web.push.apple.com");
        use p256::ecdsa::{signature::Verifier, VerifyingKey};
        let (msg, sig) = t.rsplit_once('.').unwrap();
        let sig = Signature::from_slice(&unb64(sig).unwrap()).unwrap();
        assert!(VerifyingKey::from(key.public_key()).verify(msg.as_bytes(), &sig).is_ok());
    }
}
