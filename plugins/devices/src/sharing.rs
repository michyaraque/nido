//! Smart Life / Tuya Smart sign-in by QR, the way Home Assistant does it: the person types the user
//! code their app shows, scans a QR with the app, and Tuya hands over the devices of that account
//! with their local keys. No developer account. Only the listing goes through the cloud; control
//! stays on the LAN (tuya.rs).
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::Aes128Gcm;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use hmac::{Hmac, Mac};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;

const CLIENT_ID: &str = "HA_3y9q4ak7g4ephrvke";
const SCHEMA: &str = "haauthorize";
const LOGIN: &str = "https://apigw.iotbing.com/v1.0/m/life/home-assistant/qrcode/tokens";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Session {
    pub user_code: String,
    pub endpoint: String,
    pub access: String,
    pub refresh: String,
    pub expires_ms: i64,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn hmac_hex(key: &[u8], msg: &[u8]) -> String {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("any key length");
    m.update(msg);
    hex(&m.finalize().into_bytes())
}

fn pct(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn curl(method: &str, url: &str, headers: &[String]) -> Result<Value, String> {
    let mut cmd = std::process::Command::new("curl");
    cmd.args(["-sS", "--max-time", "20", "-X", method]);
    for h in headers {
        cmd.args(["-H", h]);
    }
    let out = cmd.arg(url).output().map_err(|e| format!("curl: {e}"))?;
    serde_json::from_slice(&out.stdout).map_err(|_| "Tuya no contesta; ¿hay internet?".to_string())
}

fn tuya_err(v: &Value) -> String {
    v["msg"].as_str().unwrap_or("error de Tuya").to_string()
}

/// The token to put in the QR, for the user code the person typed.
pub fn qr_token(user_code: &str) -> Result<String, String> {
    let url = format!("{LOGIN}?clientid={CLIENT_ID}&usercode={}&schema={SCHEMA}", pct(user_code));
    let v = curl("POST", &url, &[])?;
    if v["success"].as_bool() != Some(true) {
        return Err(tuya_err(&v));
    }
    v["result"]["qrcode"].as_str().map(str::to_string).ok_or_else(|| "Tuya no ha dado el código QR".into())
}

pub fn qr_text(token: &str) -> String {
    format!("tuyaSmart--qrLogin?token={token}")
}

/// `None` while the QR has not been scanned and confirmed in the app yet.
pub fn login_result(token: &str, user_code: &str) -> Result<Option<Session>, String> {
    let url = format!("{LOGIN}/{}?clientid={CLIENT_ID}&usercode={}", pct(token), pct(user_code));
    let v = curl("GET", &url, &[])?;
    if v["success"].as_bool() != Some(true) {
        return Ok(None);
    }
    let r = &v["result"];
    let s = |k: &str| r[k].as_str().unwrap_or("").to_string();
    if s("access_token").is_empty() || s("endpoint").is_empty() {
        return Ok(None);
    }
    Ok(Some(Session {
        user_code: user_code.to_string(),
        endpoint: s("endpoint").trim_end_matches('/').to_string(),
        access: s("access_token"),
        refresh: s("refresh_token"),
        expires_ms: v["t"].as_i64().unwrap_or_else(now_ms) + r["expire_time"].as_i64().unwrap_or(0) * 1000,
    }))
}

fn uuid4() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("random");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex(&b);
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

fn seal(secret: &[u8], plain: &[u8]) -> String {
    const ABC: &[u8] = b"ABCDEFGHJKMNPQRSTWXYZabcdefhijkmnprstwxyz2345678";
    let mut r = [0u8; 12];
    getrandom::fill(&mut r).expect("random");
    let nonce: Vec<u8> = r.iter().map(|b| ABC[*b as usize % ABC.len()]).collect();
    let ct = Aes128Gcm::new(secret.into()).encrypt(nonce.as_slice().into(), plain).expect("gcm");
    B64.encode([nonce, ct].concat())
}

fn unseal(secret: &[u8], blob: &str) -> Option<Vec<u8>> {
    let raw = B64.decode(blob).ok()?;
    if raw.len() < 28 {
        return None;
    }
    Aes128Gcm::new(secret.into()).decrypt(raw[..12].into(), &raw[12..]).ok()
}

/// One signed and encrypted GET to the Smart Life API (Tuya's "customer" API).
fn get(s: &Session, path: &str, params: Option<Value>) -> Result<Value, String> {
    let rid = uuid4();
    let hash_key = hex(&Md5::digest(format!("{rid}{}", s.refresh)));
    let secret = hmac_hex(rid.as_bytes(), hash_key.as_bytes())[..16].to_string();
    let qenc = params.map(|p| seal(secret.as_bytes(), p.to_string().as_bytes())).unwrap_or_default();
    let t = now_ms().to_string();
    let mut sign = format!("X-appKey={CLIENT_ID}||X-requestId={rid}||X-time={t}");
    if !s.access.is_empty() {
        sign += &format!("||X-token={}", s.access);
    }
    sign += &qenc;
    let headers = vec![
        format!("X-appKey: {CLIENT_ID}"),
        format!("X-requestId: {rid}"),
        "X-sid;".to_string(),
        format!("X-time: {t}"),
        format!("X-token: {}", s.access),
        format!("X-sign: {}", hmac_hex(hash_key.as_bytes(), sign.as_bytes())),
    ];
    let url = if qenc.is_empty() { format!("{}{path}", s.endpoint) } else { format!("{}{path}?encdata={}", s.endpoint, pct(&qenc)) };
    let v = curl("GET", &url, &headers)?;
    if v["success"].as_bool() != Some(true) {
        return Err(tuya_err(&v));
    }
    let plain = v["result"].as_str().and_then(|b| unseal(secret.as_bytes(), b)).ok_or("respuesta de Tuya ilegible")?;
    let out: Value = serde_json::from_slice(&plain).unwrap_or(Value::Null);
    Ok(serde_json::json!({ "t": v["t"], "result": out }))
}

/// Renews the access token when it is about to expire. Returns whether the session changed.
pub fn refresh(s: &mut Session) -> Result<bool, String> {
    if s.expires_ms - 60_000 > now_ms() {
        return Ok(false);
    }
    let v = get(s, &format!("/v1.0/m/token/{}", s.refresh), None)?;
    let r = &v["result"];
    let k = |n: &str| r[n].as_str().map(str::to_string).ok_or_else(|| "Tuya no ha renovado la sesión".to_string());
    s.access = k("accessToken")?;
    s.refresh = k("refreshToken")?;
    s.expires_ms = v["t"].as_i64().unwrap_or_else(now_ms) + r["expireTime"].as_i64().unwrap_or(0) * 1000;
    Ok(true)
}

/// Every device in every home of the account, with its local key.
pub fn devices(s: &Session) -> Result<Vec<crate::tuya::CloudDev>, String> {
    let homes = get(s, "/v1.0/m/life/users/homes", None)?;
    let mut all = Vec::new();
    for h in homes["result"].as_array().into_iter().flatten() {
        let id = match &h["ownerId"] {
            Value::String(s) => s.clone(),
            v => v.to_string(),
        };
        let list = get(s, "/v1.0/m/life/ha/home/devices", Some(serde_json::json!({ "homeId": id })))?;
        for d in list["result"].as_array().into_iter().flatten() {
            let f = |k: &str| d[k].as_str().unwrap_or("").to_string();
            if !f("local_key").is_empty() && !all.iter().any(|c: &crate::tuya::CloudDev| c.id == f("id")) {
                all.push(crate::tuya::CloudDev { id: f("id"), name: f("name"), key: f("local_key"), category: f("category") });
            }
        }
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crypto_round_trip() {
        let secret = b"0123456789abcdef";
        let blob = seal(secret, br#"{"homeId":"1"}"#);
        assert_eq!(unseal(secret, &blob).unwrap(), br#"{"homeId":"1"}"#);
        let u = uuid4();
        assert_eq!((u.len(), &u[14..15]), (36, "4"));
        assert_eq!(pct("a+b/c="), "a%2Bb%2Fc%3D");
        assert_eq!(qr_text("abc"), "tuyaSmart--qrLogin?token=abc");
    }
}
