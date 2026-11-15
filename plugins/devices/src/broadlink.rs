//! Broadlink IR blasters (RM mini 3, RM4 mini…) on the LAN, as python-broadlink does it: a hello
//! over UDP port 80 finds them, an auth gives a per-device AES key, and then the device learns
//! an IR code from a real remote and plays it back. Everything is local; blocking std sockets,
//! called from `spawn_blocking`.
use aes::cipher::{block_padding::NoPadding, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use std::net::UdpSocket;
use std::time::Duration;

type Enc = cbc::Encryptor<aes::Aes128>;
type Dec = cbc::Decryptor<aes::Aes128>;

const KEY: [u8; 16] = [0x09, 0x76, 0x28, 0x34, 0x3f, 0xe9, 0x9e, 0x23, 0x76, 0x5c, 0x15, 0x13, 0xac, 0xcf, 0x8b, 0x02];
const IV: [u8; 16] = [0x56, 0x2e, 0x17, 0x99, 0x6d, 0x09, 0x3d, 0x28, 0xdd, 0xb3, 0xba, 0x69, 0x5a, 0x2e, 0x6f, 0x58];

#[derive(Clone, Debug)]
pub struct Found {
    pub ip: String,
    pub mac: String,
    pub devtype: u16,
    pub locked: bool,
}

fn checksum(b: &[u8]) -> u16 {
    b.iter().fold(0xBEAFu32, |s, &x| s + x as u32) as u16
}

fn hello(local: std::net::SocketAddr) -> Vec<u8> {
    let mut p = vec![0u8; 0x30];
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    p[0x0c..0x0e].copy_from_slice(&(2026u16).to_le_bytes());
    p[0x0e] = ((now / 60) % 60) as u8;
    p[0x0f] = ((now / 3600) % 24) as u8;
    if let std::net::SocketAddr::V4(a) = local {
        let mut ip = a.ip().octets();
        ip.reverse();
        p[0x18..0x1c].copy_from_slice(&ip);
        p[0x1c..0x1e].copy_from_slice(&a.port().to_le_bytes());
    }
    p[0x26] = 6;
    let c = checksum(&p);
    p[0x20..0x22].copy_from_slice(&c.to_le_bytes());
    p
}

fn parse_hello(r: &[u8], ip: String) -> Option<Found> {
    if r.len() < 0x40 {
        return None;
    }
    Some(Found {
        ip,
        mac: r[0x3a..0x40].iter().rev().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":"),
        devtype: u16::from_le_bytes([r[0x34], r[0x35]]),
        locked: r.get(0x7f).is_some_and(|l| *l != 0),
    })
}

fn local_ip() -> Option<std::net::Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?;
    match s.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(v) => Some(v),
        _ => None,
    }
}

/// Every Broadlink on our /24. Broadcast first, then each address one by one, because many
/// routers drop broadcasts between Wi-Fi clients.
pub fn discover() -> Vec<Found> {
    let Some(me) = local_ip() else { return vec![] };
    let Ok(s) = UdpSocket::bind((me, 0)) else { return vec![] };
    let _ = s.set_broadcast(true);
    let p = hello(s.local_addr().unwrap());
    let o = me.octets();
    let _ = s.send_to(&p, "255.255.255.255:80");
    for i in 1..255u8 {
        if i != o[3] {
            let _ = s.send_to(&p, (std::net::Ipv4Addr::new(o[0], o[1], o[2], i), 80));
            std::thread::sleep(Duration::from_millis(4));
        }
    }
    let _ = s.set_read_timeout(Some(Duration::from_millis(2500)));
    let mut out: Vec<Found> = Vec::new();
    let mut buf = [0u8; 1024];
    while let Ok((n, a)) = s.recv_from(&mut buf) {
        if let Some(f) = parse_hello(&buf[..n], a.ip().to_string()) {
            if !out.iter().any(|x| x.mac == f.mac) {
                out.push(f);
            }
        }
    }
    out
}

/// One device, asked directly (to check it is still there and still unlocked).
pub fn probe(ip: &str) -> Option<Found> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_millis(1000)));
    let local = std::net::SocketAddr::new(std::net::IpAddr::V4(local_ip()?), s.local_addr().ok()?.port());
    let mut buf = [0u8; 1024];
    for _ in 0..3 {
        s.send_to(&hello(local), (ip, 80)).ok()?;
        if let Ok((n, _)) = s.recv_from(&mut buf) {
            return parse_hello(&buf[..n], ip.to_string());
        }
    }
    None
}

/// RM4 models frame their commands with a length; the older RM mini 3 (0x27xx) does not.
fn is_rm4(devtype: u16) -> bool {
    devtype >= 0x5000
}

pub struct Conn {
    s: UdpSocket,
    ip: String,
    mac: [u8; 6],
    devtype: u16,
    key: [u8; 16],
    id: u32,
    count: u16,
}

fn encrypt(key: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let mut v = data.to_vec();
    v.resize(data.len().div_ceil(16) * 16, 0);
    let n = v.len();
    Enc::new(key.into(), &IV.into()).encrypt_padded_mut::<NoPadding>(&mut v, n).expect("aligned").to_vec()
}

fn decrypt(key: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let mut v = data[..data.len() / 16 * 16].to_vec();
    Dec::new(key.into(), &IV.into()).decrypt_padded_mut::<NoPadding>(&mut v).map(|p| p.to_vec()).unwrap_or_default()
}

impl Conn {
    pub fn open(ip: &str, mac: &str, devtype: u16) -> Result<Conn, String> {
        let m: Vec<u8> = mac.split(':').filter_map(|h| u8::from_str_radix(h, 16).ok()).collect();
        let mac: [u8; 6] = m.try_into().map_err(|_| "MAC rara")?;
        let s = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
        s.set_read_timeout(Some(Duration::from_millis(1200))).map_err(|e| e.to_string())?;
        let mut r = [0u8; 2];
        getrandom::fill(&mut r).map_err(|e| e.to_string())?;
        // The device silently ignores a packet number it has already seen: start somewhere new each time.
        let mut c = Conn { s, ip: ip.to_string(), mac, devtype, key: KEY, id: 0, count: u16::from_le_bytes(r) | 0x8000 };
        let mut auth = vec![0u8; 0x50];
        auth[0x04..0x14].fill(0x31);
        auth[0x1e] = 1;
        auth[0x2d] = 1;
        auth[0x30..0x36].copy_from_slice(b"Nido 1");
        let r = c.packet(0x65, &auth)?;
        if r.len() < 0x14 {
            return Err("el mando no acepta la conexión: ¿está bloqueado en la app Broadlink?".into());
        }
        c.id = u32::from_le_bytes(r[..4].try_into().unwrap());
        c.key = r[0x04..0x14].try_into().unwrap();
        Ok(c)
    }

    fn packet(&mut self, kind: u16, payload: &[u8]) -> Result<Vec<u8>, String> {
        self.count = (self.count.wrapping_add(1)) | 0x8000;
        let mut p = vec![0u8; 0x38];
        p[..8].copy_from_slice(&[0x5a, 0xa5, 0xaa, 0x55, 0x5a, 0xa5, 0xaa, 0x55]);
        p[0x24..0x26].copy_from_slice(&self.devtype.to_le_bytes());
        p[0x26..0x28].copy_from_slice(&kind.to_le_bytes());
        p[0x28..0x2a].copy_from_slice(&self.count.to_le_bytes());
        let mut mac = self.mac;
        mac.reverse();
        p[0x2a..0x30].copy_from_slice(&mac);
        p[0x30..0x34].copy_from_slice(&self.id.to_le_bytes());
        p[0x34..0x36].copy_from_slice(&checksum(payload).to_le_bytes());
        p.extend(encrypt(&self.key, payload));
        let c = checksum(&p);
        p[0x20..0x22].copy_from_slice(&c.to_le_bytes());
        // UDP over Wi-Fi drops packets now and then: send the same packet again, like the Broadlink app.
        let mut buf = [0u8; 2048];
        let mut got = None;
        for _ in 0..3 {
            self.s.send_to(&p, (self.ip.as_str(), 80)).map_err(|e| e.to_string())?;
            if let Ok((n, _)) = self.s.recv_from(&mut buf) {
                got = Some(n);
                break;
            }
        }
        let n = got.ok_or("el mando no contesta")?;
        if n < 0x38 {
            return Err("respuesta corta del mando".into());
        }
        let err = u16::from_le_bytes([buf[0x22], buf[0x23]]);
        if err != 0 {
            return Err(format!("error {err:#06x}"));
        }
        Ok(decrypt(&self.key, &buf[0x38..n]))
    }

    fn cmd(&mut self, command: u32, data: &[u8]) -> Result<Vec<u8>, String> {
        let rm4 = is_rm4(self.devtype);
        let mut p = Vec::new();
        if rm4 {
            p.extend(((data.len() + 4) as u16).to_le_bytes());
        }
        p.extend(command.to_le_bytes());
        p.extend(data);
        let r = self.packet(0x6a, &p)?;
        if rm4 {
            let len = r.get(..2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize).unwrap_or(0);
            Ok(r.get(6..(len + 2).min(r.len())).unwrap_or_default().to_vec())
        } else {
            Ok(r.get(4..).unwrap_or_default().to_vec())
        }
    }

    pub fn learn_start(&mut self) -> Result<(), String> {
        self.cmd(3, &[]).map(|_| ())
    }

    /// The code learnt so far, if the button was pressed (errors mean "nothing yet").
    pub fn learnt(&mut self) -> Option<Vec<u8>> {
        let code = self.cmd(4, &[]).ok()?;
        let end = code.iter().rposition(|b| *b != 0).map(|i| i + 1).unwrap_or(0);
        (end > 4).then(|| code[..end].to_vec())
    }

    pub fn send(&mut self, code: &[u8]) -> Result<(), String> {
        self.cmd(2, code).map(|_| ())
    }
}

fn ticks(out: &mut Vec<u8>, us: u32) {
    let n = (us as u64 * 269 / 8192) as u32;
    if n > 255 {
        out.extend([0, (n >> 8) as u8, n as u8]);
    } else {
        out.push(n.max(1) as u8);
    }
}

/// A Broadlink IR code for an NEC button (LG, many TVs and air conditioners): 16-bit address
/// (8-bit ones are sent with their complement) and 8-bit command.
pub fn nec(addr: u16, cmd: u8) -> Vec<u8> {
    let a = if addr <= 0xff { [addr as u8, !(addr as u8)] } else { [addr as u8, (addr >> 8) as u8] };
    let mut d = Vec::new();
    ticks(&mut d, 9000);
    ticks(&mut d, 4500);
    for byte in [a[0], a[1], cmd, !cmd] {
        for bit in 0..8 {
            ticks(&mut d, 560);
            ticks(&mut d, if byte >> bit & 1 == 1 { 1690 } else { 560 });
        }
    }
    ticks(&mut d, 560);
    ticks(&mut d, 108_000);
    let mut code = vec![0x26, 0x00];
    code.extend((d.len() as u16).to_le_bytes());
    code.extend(d);
    code
}

/// A pasted Broadlink IR code, in hex or in the base64 that Home Assistant and SmartIR share.
pub fn parse_code(s: &str) -> Option<Vec<u8>> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let hex = s.len() % 2 == 0 && s.chars().all(|c| c.is_ascii_hexdigit());
    let bytes = if hex {
        (0..s.len() / 2).map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()).collect::<Option<Vec<u8>>>()?
    } else {
        STANDARD.decode(&s).ok()?
    };
    (bytes.len() > 8 && bytes[0] == 0x26).then_some(bytes)
}

pub fn model(devtype: u16) -> &'static str {
    match devtype {
        0x2737 | 0x273d | 0x277c | 0x278f | 0x27c2 | 0x27c7 | 0x27cc | 0x27cd | 0x27d0 | 0x27d1 | 0x27d3 | 0x27de => "RM mini 3",
        0x2712 | 0x272a | 0x2787 | 0x278b | 0x279d | 0x27a9 | 0x27a1 | 0x27a6 | 0x27c3 => "RM Pro",
        t if is_rm4(t) => "RM4",
        _ => "Broadlink",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packets() {
        let p = hello("192.168.1.106:5000".parse().unwrap());
        assert_eq!((p.len(), p[0x26], &p[0x18..0x1c]), (0x30, 6, &[106u8, 1, 168, 192][..]));
        let mut z = p.clone();
        z[0x20] = 0;
        z[0x21] = 0;
        assert_eq!(u16::from_le_bytes([p[0x20], p[0x21]]), checksum(&z));
        let e = encrypt(&KEY, b"hola");
        assert_eq!((e.len(), &decrypt(&KEY, &e)[..4]), (16, &b"hola"[..]));
        let mut r = vec![0u8; 0x80];
        r[0x34] = 0x37;
        r[0x35] = 0x27;
        r[0x3a..0x40].copy_from_slice(&[0xe0, 0xd4, 0x27, 0xa7, 0xdf, 0x24]);
        r[0x7f] = 1;
        let code = nec(0x04, 0x08);
        assert_eq!((code[0], code[4], code[5], code[6]), (0x26, 0, 0x01, 0x27));
        assert_eq!(parse_code(&code.iter().map(|b| format!("{b:02x}")).collect::<String>()), Some(code.clone()));
        assert_eq!(parse_code("JgAGAAEBAQEBAQ=="), Some(vec![0x26, 0, 6, 0, 1, 1, 1, 1, 1, 1]));
        assert_eq!(parse_code("hola"), None);
        let f = parse_hello(&r, "1.2.3.4".into()).unwrap();
        assert_eq!((f.mac.as_str(), f.devtype, f.locked, model(f.devtype)), ("24:df:a7:27:d4:e0", 0x2737, true, "RM mini 3"));
    }
}
