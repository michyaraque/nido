//! Tuya devices on the LAN (Smart Life, Woox, Lidl, Nedis… all white labels of Tuya), the same way
//! tinytuya does it: the local key comes once from the Tuya cloud (or pasted by hand), and from then
//! on everything goes straight to the device on port 6668, protocols 3.3, 3.4 and 3.5. Devices
//! announce themselves by UDP broadcast on 6666/6667/7000, which gives their IP and version.
use aes::cipher::{generic_array::GenericArray, BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes128;
use aes_gcm::aead::{Aead, Payload};
use aes_gcm::Aes128Gcm;
use hmac::{Hmac, Mac};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SESS_START: u32 = 3;
const SESS_RESP: u32 = 4;
const SESS_FINISH: u32 = 5;
const CONTROL: u32 = 7;
const DP_QUERY: u32 = 10;
const CONTROL_NEW: u32 = 13;
const DP_QUERY_NEW: u32 = 16;
const NO_HEADER: &[u32] = &[DP_QUERY, DP_QUERY_NEW, 9, 18, SESS_START, SESS_RESP, SESS_FINISH, 64];

/// md5("yGAdlopoPVldABfn"): the key every Tuya device uses for its UDP announcements.
const UDP_KEY: [u8; 16] = [0x6c, 0x1e, 0xc8, 0xe2, 0xbb, 0x9b, 0xb5, 0x9a, 0xb5, 0x0b, 0x0d, 0xaf, 0x64, 0x9b, 0x41, 0x0a];

#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub enum Ver {
    V33,
    V34,
    V35,
}

impl Ver {
    pub fn parse(s: &str) -> Ver {
        match s.trim() {
            "3.4" => Ver::V34,
            "3.5" => Ver::V35,
            _ => Ver::V33,
        }
    }
    fn header(self) -> [u8; 15] {
        let mut h = [0u8; 15];
        h[..3].copy_from_slice(match self {
            Ver::V33 => b"3.3",
            Ver::V34 => b"3.4",
            Ver::V35 => b"3.5",
        });
        h
    }
}

pub struct Dev {
    pub id: String,
    pub ip: String,
    pub key: [u8; 16],
    pub ver: Ver,
}

pub fn key(s: &str) -> Option<[u8; 16]> {
    s.trim().as_bytes().try_into().ok()
}

fn hmac(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("any key length");
    m.update(data);
    m.finalize().into_bytes().into()
}

fn crc32(d: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in d {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

fn ecb_enc(key: &[u8; 16], data: &[u8], pad: bool) -> Vec<u8> {
    let c = Aes128::new(key.into());
    let mut v = data.to_vec();
    if pad {
        let n = 16 - v.len() % 16;
        v.extend(std::iter::repeat(n as u8).take(n));
    }
    for b in v.chunks_mut(16) {
        c.encrypt_block(GenericArray::from_mut_slice(b));
    }
    v
}

fn ecb_dec(key: &[u8; 16], data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() || data.len() % 16 != 0 {
        return None;
    }
    let c = Aes128::new(key.into());
    let mut v = data.to_vec();
    for b in v.chunks_mut(16) {
        c.decrypt_block(GenericArray::from_mut_slice(b));
    }
    let n = *v.last()? as usize;
    if (1..=16).contains(&n) && v[v.len() - n..].iter().all(|&b| b as usize == n) {
        v.truncate(v.len() - n);
    }
    Some(v)
}

fn gcm_seal(key: &[u8; 16], iv: &[u8], msg: &[u8], aad: &[u8]) -> Vec<u8> {
    Aes128Gcm::new(key.into())
        .encrypt(iv.into(), Payload { msg, aad })
        .expect("gcm")
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("random");
    b
}

/// A 55AA frame (3.3 with CRC, 3.4 with HMAC) or a 6699 frame (3.5, AES-GCM).
fn pack(seq: u32, cmd: u32, payload: &[u8], ver: Ver, key: &[u8; 16]) -> Vec<u8> {
    if ver == Ver::V35 {
        let mut f = 0x6699u32.to_be_bytes().to_vec();
        f.extend(0u16.to_be_bytes());
        f.extend(seq.to_be_bytes());
        f.extend(cmd.to_be_bytes());
        f.extend((payload.len() as u32 + 28).to_be_bytes());
        let iv: [u8; 12] = random();
        let sealed = gcm_seal(key, &iv, payload, &f[4..18]);
        f.extend(iv);
        f.extend(sealed);
        f.extend(0x9966u32.to_be_bytes());
        return f;
    }
    let tail = if ver == Ver::V34 { 32 } else { 4 };
    let mut f = 0x55AAu32.to_be_bytes().to_vec();
    f.extend(seq.to_be_bytes());
    f.extend(cmd.to_be_bytes());
    f.extend((payload.len() as u32 + tail + 4).to_be_bytes());
    f.extend(payload);
    if ver == Ver::V34 {
        let h = hmac(key, &f);
        f.extend(h);
    } else {
        let c = crc32(&f);
        f.extend(c.to_be_bytes());
    }
    f.extend(0xAA55u32.to_be_bytes());
    f
}

/// The command and the decrypted payload of a frame. 3.3/3.4 payloads may start with a 4-byte
/// return code and 3.3 ones with a clear "3.3" header: their length modulo 16 tells which.
fn open(f: &[u8], ver: Ver, key: &[u8; 16]) -> Option<(u32, Vec<u8>)> {
    let be = |i: usize| u32::from_be_bytes(f[i..i + 4].try_into().unwrap());
    if f.len() >= 22 && be(0) == 0x6699 {
        let (cmd, len) = (be(10), be(14) as usize);
        if len < 28 || f.len() < 18 + len {
            return None;
        }
        let p = Aes128Gcm::new(key.into())
            .decrypt(f[18..30].into(), Payload { msg: &f[30..18 + len], aad: &f[4..18] })
            .ok()?;
        return Some((cmd, p));
    }
    if f.len() < 24 || be(0) != 0x55AA {
        return None;
    }
    let (cmd, len) = (be(8), be(12) as usize);
    let tail = if ver == Ver::V34 { 36 } else { 8 };
    if len < tail || f.len() < 16 + len {
        return None;
    }
    let mut body = &f[16..16 + len - tail];
    let plain = |b: &[u8]| b.is_empty() || serde_json::from_slice::<Value>(b).is_ok();
    if plain(body) {
        return Some((cmd, body.to_vec()));
    }
    if body.len() > 4 && plain(&body[4..]) {
        return Some((cmd, body[4..].to_vec()));
    }
    if matches!(body.len() % 16, 3 | 4) {
        body = &body[4..];
    }
    if body.len() % 16 == 15 {
        body = &body[15..];
    }
    if plain(body) {
        return Some((cmd, body.to_vec()));
    }
    Some((cmd, ecb_dec(key, body)?))
}

pub fn json_in(p: &[u8]) -> Option<Value> {
    let a = p.iter().position(|&b| b == b'{')?;
    let z = p.iter().rposition(|&b| b == b'}')?;
    serde_json::from_slice(p.get(a..=z)?).ok()
}

fn dps_in(v: &Value) -> Option<Map<String, Value>> {
    v.get("dps")
        .or_else(|| v.get("data").and_then(|d| d.get("dps")))
        .and_then(|d| d.as_object())
        .cloned()
}

async fn read_frame(s: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut f = vec![0u8; 4];
    s.read_exact(&mut f).await?;
    let (head, at) = match u32::from_be_bytes(f[..4].try_into().unwrap()) {
        0x55AA => (16, 12),
        0x6699 => (18, 14),
        _ => return Err(std::io::Error::other("trama desconocida")),
    };
    f.resize(head, 0);
    s.read_exact(&mut f[4..]).await?;
    let len = u32::from_be_bytes(f[at..at + 4].try_into().unwrap()) as usize;
    if len > 64 * 1024 {
        return Err(std::io::Error::other("trama enorme"));
    }
    let extra = if head == 18 { 4 } else { 0 };
    f.resize(head + len + extra, 0);
    s.read_exact(&mut f[head..]).await?;
    Ok(f)
}

struct Conn<'a> {
    s: TcpStream,
    d: &'a Dev,
    key: [u8; 16],
    seq: u32,
}

impl<'a> Conn<'a> {
    async fn open(d: &'a Dev) -> Result<Conn<'a>, String> {
        if d.ip.is_empty() {
            return Err("sin IP: aún no ha aparecido en la red".into());
        }
        let s = TcpStream::connect((d.ip.as_str(), 6668)).await.map_err(|e| e.to_string())?;
        let mut c = Conn { s, d, key: d.key, seq: 0 };
        if d.ver >= Ver::V34 {
            c.negotiate().await?;
        }
        Ok(c)
    }

    async fn raw(&mut self, cmd: u32, payload: &[u8]) -> Result<(), String> {
        self.seq += 1;
        let f = pack(self.seq, cmd, payload, self.d.ver, &self.key);
        self.s.write_all(&f).await.map_err(|e| e.to_string())
    }

    async fn send(&mut self, cmd: u32, msg: &[u8]) -> Result<(), String> {
        let ver = self.d.ver;
        let hdr = !NO_HEADER.contains(&cmd);
        let payload = match ver {
            Ver::V33 => {
                let enc = ecb_enc(&self.key, msg, true);
                if hdr { [&ver.header()[..], &enc].concat() } else { enc }
            }
            _ => {
                let plain = if hdr { [&ver.header()[..], msg].concat() } else { msg.to_vec() };
                if ver == Ver::V34 { ecb_enc(&self.key, &plain, true) } else { plain }
            }
        };
        self.raw(cmd, &payload).await
    }

    async fn recv(&mut self) -> Result<(u32, Vec<u8>), String> {
        let f = read_frame(&mut self.s).await.map_err(|e| e.to_string())?;
        open(&f, self.d.ver, &self.key).ok_or_else(|| "no se pudo descifrar: ¿clave o versión?".into())
    }

    async fn negotiate(&mut self) -> Result<(), String> {
        let lk = self.d.key;
        let local: [u8; 16] = random();
        let p = if self.d.ver == Ver::V34 { ecb_enc(&lk, &local, true) } else { local.to_vec() };
        self.raw(SESS_START, &p).await?;
        let (cmd, r) = self.recv().await?;
        let r = if r.len() % 16 == 4 { &r[4..] } else { &r[..] };
        if cmd != SESS_RESP || r.len() < 48 || r[16..48] != hmac(&lk, &local) {
            return Err("la clave local no es la buena".into());
        }
        let remote: [u8; 16] = r[..16].try_into().unwrap();
        let fin = hmac(&lk, &remote);
        let p = if self.d.ver == Ver::V34 { ecb_enc(&lk, &fin, true) } else { fin.to_vec() };
        self.raw(SESS_FINISH, &p).await?;
        let x: Vec<u8> = local.iter().zip(remote).map(|(a, b)| a ^ b).collect();
        let sk = match self.d.ver {
            Ver::V34 => ecb_enc(&lk, &x, false),
            _ => gcm_seal(&lk, &local[..12], &x, &[])[..16].to_vec(),
        };
        self.key = sk.try_into().unwrap();
        Ok(())
    }

    /// Frames until one carries data points (devices also send acks and heartbeats).
    async fn dps(&mut self) -> Result<Map<String, Value>, String> {
        for _ in 0..5 {
            let (_, p) = self.recv().await?;
            if p.windows(12).any(|w| w == b"data unvalid") {
                return Err("device22".into());
            }
            if let Some(d) = json_in(&p).as_ref().and_then(dps_in) {
                return Ok(d);
            }
        }
        Err("no contesta con su estado".into())
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

async fn timed<T>(f: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
    tokio::time::timeout(Duration::from_secs(5), f).await.unwrap_or(Err("no responde".into()))
}

/// Everything the device reports now: `{"1": true, "19": 1234, …}`.
pub async fn status(d: &Dev) -> Result<Map<String, Value>, String> {
    timed(async {
        let mut c = Conn::open(d).await?;
        if d.ver == Ver::V33 {
            let q = json!({"gwId": d.id, "devId": d.id, "uid": d.id, "t": now().to_string()});
            c.send(DP_QUERY, q.to_string().as_bytes()).await?;
            match c.dps().await {
                Err(e) if e == "device22" => {
                    let dps = json!({"1": null, "2": null, "3": null, "18": null, "19": null, "20": null});
                    let q = json!({"devId": d.id, "uid": d.id, "t": now().to_string(), "dps": dps});
                    c.send(CONTROL_NEW, q.to_string().as_bytes()).await?;
                    c.dps().await
                }
                r => r,
            }
        } else {
            c.send(DP_QUERY_NEW, b"{}").await?;
            c.dps().await
        }
    })
    .await
}

pub async fn set(d: &Dev, dps: Value) -> Result<(), String> {
    timed(async {
        let mut c = Conn::open(d).await?;
        if d.ver == Ver::V33 {
            let m = json!({"devId": d.id, "uid": d.id, "t": now().to_string(), "dps": dps});
            c.send(CONTROL, m.to_string().as_bytes()).await?;
        } else {
            let m = json!({"protocol": 5, "t": now(), "data": {"dps": dps}});
            c.send(CONTROL_NEW, m.to_string().as_bytes()).await?;
        }
        c.recv().await.map(|_| ())
    })
    .await
}

/// Every address of our /24 with the Tuya port open, for routers that drop the UDP announcements.
pub async fn scan() -> Vec<String> {
    let me = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("8.8.8.8:80").and_then(|_| s.local_addr()))
        .map(|a| a.ip().to_string());
    let Ok(me) = me else { return Vec::new() };
    let Some(base) = me.rsplit_once('.').map(|(b, _)| b.to_string()) else { return Vec::new() };
    let mut set = tokio::task::JoinSet::new();
    for i in 1..255 {
        let ip = format!("{base}.{i}");
        set.spawn(async move {
            let ok = tokio::time::timeout(Duration::from_millis(600), TcpStream::connect((ip.as_str(), 6668))).await;
            matches!(ok, Ok(Ok(_))).then_some(ip)
        });
    }
    let mut out = Vec::new();
    while let Some(r) = set.join_next().await {
        out.extend(r.ok().flatten());
    }
    out
}

/// Which of `ips` answers to this device's key, and with which protocol version.
pub async fn locate(id: &str, key: [u8; 16], first: Ver, ips: &[String]) -> Option<(String, Ver)> {
    let mut vers = vec![first];
    vers.extend([Ver::V33, Ver::V34, Ver::V35].into_iter().filter(|v| *v != first));
    for ip in ips {
        for &ver in &vers {
            let d = Dev { id: id.to_string(), ip: ip.clone(), key, ver };
            if status(&d).await.is_ok() {
                return Some((ip.clone(), ver));
            }
        }
    }
    None
}

impl Ver {
    pub fn as_str(self) -> &'static str {
        match self {
            Ver::V33 => "3.3",
            Ver::V34 => "3.4",
            Ver::V35 => "3.5",
        }
    }
}

/// A device announcing itself on the LAN.
#[derive(Clone, Debug)]
pub struct Seen {
    pub id: String,
    pub ip: String,
    pub ver: String,
}

pub fn announcement(packet: &[u8]) -> Option<Seen> {
    let (_, p) = open(packet, Ver::V33, &UDP_KEY)?;
    let v = json_in(&p)?;
    Some(Seen {
        id: v["gwId"].as_str()?.to_string(),
        ip: v["ip"].as_str()?.to_string(),
        ver: v["version"].as_str().unwrap_or("3.3").to_string(),
    })
}

/// Listens for announcements forever and hands each one to `found`.
pub async fn discover(found: impl Fn(Seen) + Clone + Send + 'static) {
    for port in [6666, 6667, 7000] {
        let found = found.clone();
        tokio::spawn(async move {
            let sock = match tokio::net::UdpSocket::bind(("0.0.0.0", port)).await {
                Ok(s) => s,
                Err(e) => return eprintln!("tuya: no se puede escuchar UDP {port}: {e}"),
            };
            let mut buf = [0u8; 2048];
            while let Ok((n, _)) = sock.recv_from(&mut buf).await {
                if let Some(s) = announcement(&buf[..n]) {
                    found(s);
                }
            }
        });
    }
}

/// A device as the Tuya cloud lists it, with the local key that unlocks it on the LAN.
pub struct CloudDev {
    pub id: String,
    pub name: String,
    pub key: String,
    pub category: String,
}

pub const REGIONS: &[(&str, &str, &str)] = &[
    ("eu", "Europa central", "openapi.tuyaeu.com"),
    ("eu-w", "Europa occidental", "openapi-weaz.tuyaeu.com"),
    ("us", "EE. UU. oeste", "openapi.tuyaus.com"),
    ("us-e", "EE. UU. este", "openapi-ueaz.tuyaus.com"),
    ("cn", "China", "openapi.tuyacn.com"),
    ("in", "India", "openapi.tuyain.com"),
    ("sg", "Singapur", "openapi-sg.iotbing.com"),
];

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn sign(id: &str, secret: &str, token: &str, t: &str, path: &str) -> String {
    let to_sign = format!("GET\n{}\n\n{path}", hex(&Sha256::digest(b"")));
    hex(&hmac(secret.as_bytes(), format!("{id}{token}{t}{to_sign}").as_bytes())).to_uppercase()
}

fn cloud_get(host: &str, id: &str, secret: &str, token: &str, path: &str) -> Result<Value, String> {
    let t = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()).to_string();
    let mut cmd = std::process::Command::new("curl");
    cmd.args(["-sS", "--max-time", "20"])
        .args(["-H", &format!("client_id: {id}")])
        .args(["-H", &format!("sign: {}", sign(id, secret, token, &t, path))])
        .args(["-H", &format!("t: {t}")])
        .args(["-H", "sign_method: HMAC-SHA256"]);
    if !token.is_empty() {
        cmd.args(["-H", &format!("access_token: {token}")]);
    }
    let out = cmd.arg(format!("https://{host}{path}")).output().map_err(|e| format!("curl: {e}"))?;
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|_| "la nube de Tuya no contesta".to_string())?;
    if v["success"].as_bool() != Some(true) {
        return Err(format!("Tuya: {}", v["msg"].as_str().unwrap_or("error")));
    }
    Ok(v["result"].clone())
}

/// What tinytuya's wizard does: log in with the project's Access ID/Secret and list every device
/// of the app accounts linked to that project, with its local key.
pub fn cloud_devices(region: &str, id: &str, secret: &str) -> Result<Vec<CloudDev>, String> {
    let host = REGIONS.iter().find(|r| r.0 == region).map(|r| r.2).ok_or("región desconocida")?;
    let token = cloud_get(host, id, secret, "", "/v1.0/token?grant_type=1")?["access_token"]
        .as_str()
        .ok_or("Tuya no ha dado token")?
        .to_string();
    let mut all = Vec::new();
    let mut row = String::new();
    loop {
        let path = if row.is_empty() {
            "/v1.0/iot-01/associated-users/devices?size=50".to_string()
        } else {
            format!("/v1.0/iot-01/associated-users/devices?last_row_key={row}&size=50")
        };
        let r = cloud_get(host, id, secret, &token, &path)?;
        for d in r["devices"].as_array().into_iter().flatten() {
            let s = |k: &str| d[k].as_str().unwrap_or("").to_string();
            if !s("local_key").is_empty() {
                all.push(CloudDev { id: s("id"), name: s("name"), key: s("local_key"), category: s("category") });
            }
        }
        row = r["last_row_key"].as_str().unwrap_or("").to_string();
        if r["has_more"].as_bool() != Some(true) || row.is_empty() || all.len() > 1000 {
            return Ok(all);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn frames() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        let k = *b"0123456789abcdef";
        assert_eq!(ecb_dec(&k, &ecb_enc(&k, b"hola", true)).unwrap(), b"hola");
        for ver in [Ver::V33, Ver::V34, Ver::V35] {
            let enc = if ver == Ver::V35 { b"{\"a\":1}".to_vec() } else { ecb_enc(&k, b"{\"a\":1}", true) };
            let with_rc = [&[0u8, 0, 0, 0][..], &enc].concat();
            let (cmd, p) = open(&pack(1, 8, if ver == Ver::V35 { &enc } else { &with_rc }, ver, &k), ver, &k).unwrap();
            assert_eq!((cmd, json_in(&p).unwrap()["a"].as_i64()), (8, Some(1)), "{ver:?}");
        }
        let ann = br#"{"ip":"192.168.1.50","gwId":"bf123","version":"3.4","productKey":"x"}"#;
        let s = announcement(&pack(0, 19, &ecb_enc(&UDP_KEY, ann, true), Ver::V33, &UDP_KEY)).unwrap();
        assert_eq!((s.id.as_str(), s.ip.as_str(), s.ver.as_str()), ("bf123", "192.168.1.50", "3.4"));
    }

    /// A fake plug that speaks the device side of each protocol, session negotiation included.
    async fn fake_plug(ver: Ver, lk: [u8; 16]) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut key = lk;
            let mut on = false;
            loop {
                let Ok(f) = read_frame(&mut s).await else { return };
                let (cmd, p) = open(&f, ver, &key).unwrap();
                let reply = |cmd: u32, body: &[u8], key: &[u8; 16]| {
                    let payload = match ver {
                        Ver::V33 => [&[0u8; 4][..], &ecb_enc(key, body, true)].concat(),
                        Ver::V34 => [&[0u8; 4][..], &ecb_enc(key, body, true)].concat(),
                        Ver::V35 => [&[0u8; 4][..], body].concat(),
                    };
                    pack(1, cmd, &payload, ver, key)
                };
                let out = match cmd {
                    SESS_START => {
                        let remote = *b"fedcba9876543210";
                        let r = reply(SESS_RESP, &[&remote[..], &hmac(&lk, &p)].concat(), &lk);
                        let local: [u8; 16] = p[..16].try_into().unwrap();
                        let x: Vec<u8> = local.iter().zip(remote).map(|(a, b)| a ^ b).collect();
                        s.write_all(&r).await.unwrap();
                        let f = read_frame(&mut s).await.unwrap();
                        let (c, fin) = open(&f, ver, &lk).unwrap();
                        assert_eq!((c, &fin[..32]), (SESS_FINISH, &hmac(&lk, &remote)[..]));
                        key = match ver {
                            Ver::V34 => ecb_enc(&lk, &x, false),
                            _ => gcm_seal(&lk, &local[..12], &x, &[])[..16].to_vec(),
                        }
                        .try_into()
                        .unwrap();
                        continue;
                    }
                    CONTROL | CONTROL_NEW => {
                        on = dps_in(&json_in(&p).unwrap()).unwrap()["1"].as_bool().unwrap();
                        reply(cmd, b"", &key)
                    }
                    _ => reply(cmd, format!(r#"{{"dps":{{"1":{on},"19":1234}}}}"#).as_bytes(), &key),
                };
                s.write_all(&out).await.unwrap();
            }
        });
        port
    }

    #[tokio::test]
    async fn talks_to_a_plug() {
        let lk = *b"0123456789abcdef";
        for ver in [Ver::V33, Ver::V34, Ver::V35] {
            let port = fake_plug(ver, lk).await;
            let d = Dev { id: "bf1".into(), ip: "127.0.0.1".into(), key: lk, ver };
            // the fake plug listens on a random port; point the connection there
            let mut c = Conn { s: TcpStream::connect(("127.0.0.1", port)).await.unwrap(), d: &d, key: lk, seq: 0 };
            if ver >= Ver::V34 {
                c.negotiate().await.unwrap();
            }
            let set = if ver == Ver::V33 { (CONTROL, json!({"dps": {"1": true}})) } else { (CONTROL_NEW, json!({"data": {"dps": {"1": true}}})) };
            c.send(set.0, set.1.to_string().as_bytes()).await.unwrap();
            c.recv().await.unwrap();
            c.send(if ver == Ver::V33 { DP_QUERY } else { DP_QUERY_NEW }, b"{}").await.unwrap();
            let dps = c.dps().await.unwrap();
            assert_eq!((dps["1"].as_bool(), dps["19"].as_i64()), (Some(true), Some(1234)), "{ver:?}");
        }
    }
}
