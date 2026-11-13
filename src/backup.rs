use super::*;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path as FsPath, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

static RUNNING: AtomicBool = AtomicBool::new(false);
const SUFFIX: &str = ".tar.gz.enc";

pub struct Target {
    pub id: i64,
    pub kind: String,
    pub name: String,
    cfg: Value,
    pub keep: i64,
    pub last_at: Option<String>,
    pub last_ok: Option<bool>,
    pub last_error: Option<String>,
    pub last_size: Option<i64>,
}

impl Target {
    pub fn kind_label(&self) -> &'static str {
        match self.kind.as_str() {
            "local" if self.uuid().is_some() => "Disco USB",
            "local" => "Carpeta",
            "s3" => "Nube S3",
            _ => "WebDAV",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self.kind.as_str() {
            "local" if self.uuid().is_some() => "hard-drive",
            "local" => "folder",
            "s3" => "cloud",
            _ => "server",
        }
    }

    pub fn uuid(&self) -> Option<&str> {
        self.cfg["uuid"].as_str()
    }

}

pub struct Panel {
    pub now: String,
    pub today: String,
    pub yesterday: String,
    pub disks: Vec<Disk>,
    pub has_pass: bool,
    pub at: String,
    pub targets: Vec<Target>,
    pub running: bool,
}

pub fn panel(c: &Connection) -> rusqlite::Result<Panel> {
    let (now, today, yesterday) = c.query_row(
        "SELECT strftime('%H:%M','now','localtime'), date('now','localtime'), date('now','localtime','-1 day')",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok(Panel {
        now,
        today,
        yesterday,
        disks: disks(),
        has_pass: notify::setting(c, "backup_pass").is_some(),
        at: notify::setting(c, "backup_at").unwrap_or_else(|| "04:00".into()),
        targets: targets(c)?,
        running: RUNNING.load(Ordering::Relaxed),
    })
}

pub fn targets(c: &Connection) -> rusqlite::Result<Vec<Target>> {
    c.prepare("SELECT id,kind,name,config,keep,last_at,last_ok,last_error,last_size FROM backup_targets ORDER BY id")?
        .query_map([], |r| {
            Ok(Target {
                id: r.get(0)?,
                kind: r.get(1)?,
                name: r.get(2)?,
                cfg: serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default(),
                keep: r.get(4)?,
                last_at: r.get(5)?,
                last_ok: r.get(6)?,
                last_error: r.get(7)?,
                last_size: r.get(8)?,
            })
        })?
        .collect()
}


/// A USB disk plugged into the Pi, mounted or not.
pub struct Disk {
    pub uuid: String,
    pub label: String,
    pub model: String,
    pub size: i64,
    pub avail: Option<i64>,
    pub mountpoint: Option<String>,
}

pub fn disks() -> Vec<Disk> {
    let cols = "PATH,PKNAME,UUID,LABEL,FSTYPE,SIZE,FSAVAIL,MOUNTPOINT,TRAN,MODEL,TYPE";
    match Command::new("lsblk").args(["-J", "-b", "-o", cols]).output() {
        Ok(out) => parse_disks(&String::from_utf8_lossy(&out.stdout)),
        Err(_) => vec![],
    }
}

/// Partitions with a file system on a USB disk; the SD card and swap never show up.
/// lsblk may nest partitions under their disk or list them flat with PKNAME: the bus and model come from the disk.
fn parse_disks(json: &str) -> Vec<Disk> {
    let v: Value = serde_json::from_str(json).unwrap_or_default();
    let parents: std::collections::HashMap<String, (Option<String>, Option<String>)> = v["blockdevices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| Some((d["path"].as_str()?.trim_start_matches("/dev/").to_string(), (d["tran"].as_str().map(str::to_string), d["model"].as_str().map(str::to_string)))))
        .collect();
    let walk = |list: &Value, out: &mut Vec<Disk>| {
        fn go(list: &Value, tran: Option<&str>, model: Option<&str>, parents: &std::collections::HashMap<String, (Option<String>, Option<String>)>, out: &mut Vec<Disk>) {
            for d in list.as_array().into_iter().flatten() {
                let parent = d["pkname"].as_str().and_then(|p| parents.get(p));
                let tran = d["tran"].as_str().or(tran).or(parent.and_then(|p| p.0.as_deref()));
                let model = d["model"].as_str().or(model).or(parent.and_then(|p| p.1.as_deref()));
            let fs = d["fstype"].as_str().unwrap_or_default();
            if let (Some("usb"), Some(uuid)) = (tran, d["uuid"].as_str()) {
                if ["vfat", "exfat", "ntfs", "ext4", "ext3", "btrfs", "xfs", "f2fs"].contains(&fs) {
                    let model = model.unwrap_or_default().trim().to_string();
                    out.push(Disk {
                        uuid: uuid.to_string(),
                        label: d["label"].as_str().map(str::to_string).unwrap_or_else(|| if model.is_empty() { "Disco USB".into() } else { model.clone() }),
                        model,
                        size: d["size"].as_i64().unwrap_or(0),
                        avail: d["fsavail"].as_i64(),
                        mountpoint: d["mountpoint"].as_str().map(str::to_string),
                    });
                }
            }
                go(&d["children"], tran, model, parents, out);
            }
        }
        go(list, None, None, &parents, out)
    };
    let mut out = Vec::new();
    walk(&v["blockdevices"], &mut out);
    out
}

fn writable(dir: &FsPath) -> bool {
    std::fs::create_dir_all(dir).is_ok() && std::fs::write(dir.join(".nido-prueba"), b"ok").is_ok() && std::fs::remove_file(dir.join(".nido-prueba")).is_ok()
}

/// Folder for the copies on a USB disk. Nido mounts the disk itself (udisks2) when it is not mounted, or
/// mounted by someone else without letting Nido write, so it works again after a reboot or a replug.
fn usb_dir(uuid: &str) -> Result<PathBuf, String> {
    let disk = disks().into_iter().find(|d| d.uuid == uuid).ok_or("El disco no está conectado a la Pi.")?;
    let dev = format!("/dev/disk/by-uuid/{uuid}");
    if let Some(mp) = &disk.mountpoint {
        let dir = PathBuf::from(mp).join("Nido copias");
        if writable(&dir) {
            return Ok(dir);
        }
        run(Command::new("udisksctl").args(["unmount", "--no-user-interaction", "-b", &dev]))?;
    }
    let out = Command::new("udisksctl").args(["mount", "--no-user-interaction", "-b", &dev]).output().map_err(|e| format!("udisksctl: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mp = text.split(" at ").nth(1).map(|m| m.trim().trim_end_matches('.').to_string());
    let Some(mp) = mp.filter(|_| out.status.success()) else {
        return Err(format!("No se ha podido montar el disco: {}", String::from_utf8_lossy(&out.stderr).trim()));
    };
    let dir = PathBuf::from(mp).join("Nido copias");
    if writable(&dir) { Ok(dir) } else { Err("Nido no puede escribir en este disco. Formatéalo como exFAT y vuelve a probar.".into()) }
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let out = cmd.stdin(Stdio::null()).output().map_err(|e| format!("{:?}: {e}", cmd.get_program()))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().chars().take(200).collect())
    }
}

/// Database (consistent hot copy), receipt and profile photos, compressed and encrypted with the household password.
fn archive(db: &Db, pass: &str) -> Result<(PathBuf, String), String> {
    let dir = data_dir();
    let work = dir.join(".backup");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let snap = work.join("nido.db");
    let stamp: String = {
        let c = lock(db);
        c.execute("VACUUM INTO ?1", [snap.to_string_lossy()]).map_err(|e| e.to_string())?;
        c.query_row("SELECT strftime('%Y%m%d-%H%M%S','now','localtime')", [], |r| r.get(0)).map_err(|e| e.to_string())?
    };
    let tgz = work.join("nido.tar.gz");
    let mut tar = Command::new("tar");
    tar.arg("-czf").arg(&tgz).arg("-C").arg(&work).arg("nido.db");
    let extra: Vec<&str> = ["tickets", "avatars"].into_iter().filter(|d| dir.join(d).is_dir()).collect();
    if !extra.is_empty() {
        tar.arg("-C").arg(&dir).args(&extra);
    }
    run(&mut tar)?;
    let name = format!("nido-{stamp}{SUFFIX}");
    let out = work.join(&name);
    run(Command::new("openssl")
        .args(["enc", "-aes-256-cbc", "-pbkdf2", "-iter", "200000", "-salt", "-pass", "env:NIDO_BACKUP_PASS", "-in"])
        .arg(&tgz)
        .arg("-out")
        .arg(&out)
        .env("NIDO_BACKUP_PASS", pass))?;
    let _ = std::fs::remove_file(&tgz);
    let _ = std::fs::remove_file(&snap);
    Ok((out, name))
}


/// Runs curl with its options on stdin, so credentials never show in the process list. Returns (HTTP code, body).
fn curl(opts: &[(&str, &str)]) -> Result<(u16, String), String> {
    let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let mut cfg = String::from("silent\nshow-error\nmax-time = 900\nwrite-out = \"\\n%{http_code}\"\n");
    for (k, v) in opts {
        cfg += &format!("{k} = \"{}\"\n", q(v));
    }
    let mut child = Command::new("curl")
        .args(["-K", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    child.stdin.take().ok_or("curl")?.write_all(cfg.as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
    match code.trim().parse::<u16>() {
        Ok(c) if c > 0 => Ok((c, body.to_string())),
        _ => Err(String::from_utf8_lossy(&out.stderr).trim().trim_start_matches("curl: ").chars().take(200).collect()),
    }
}

fn http_ok(code: u16, body: &str, what: &str) -> Result<(), String> {
    if (200..300).contains(&code) {
        return Ok(());
    }
    let detail = between(body, "<Message>", "</Message>").or_else(|| between(body, "<s:message>", "</s:message>")).unwrap_or_default();
    Err(match code {
        401 | 403 => format!("{what}: acceso denegado ({code}). Revisa el usuario y la clave. {detail}"),
        404 => format!("{what}: no existe ({code}). Revisa la dirección, el bucket o la carpeta. {detail}"),
        _ => format!("{what}: el servidor respondió {code}. {detail}"),
    }
    .trim()
    .to_string())
}

/// A copy stored at a destination.
pub struct Copy {
    pub name: String,
    pub size: Option<i64>,
}

impl Copy {
    /// "13/11/2026 04:00" from "nido-20261113-040000.tar.gz.enc".
    pub fn when(&self) -> String {
        let n = self.name.as_bytes();
        if n.len() < 20 {
            return self.name.clone();
        }
        let p = |a: usize, b: usize| &self.name[a..b];
        format!("{}/{}/{} {}:{}", p(11, 13), p(9, 11), p(5, 9), p(14, 16), p(16, 18))
    }
}

/// Text of the first `<name>` element, whatever namespace prefix the server uses (d:, D:, ns0:…).
fn tag(xml: &str, name: &str) -> Option<String> {
    let mut rest = xml;
    while let Some(i) = rest.find(&format!("{name}>")) {
        let before = rest[..i].chars().last();
        let opening = matches!(before, Some('<') | Some(':')) && !rest[..i].ends_with("</") && !rest[..i].rsplit('<').next().unwrap_or_default().starts_with('/');
        rest = &rest[i + name.len() + 1..];
        if opening {
            return rest.split('<').next().map(|t| t.trim().to_string());
        }
    }
    None
}

fn s3_copies(body: &str) -> Vec<Copy> {
    body.split("<Contents>").skip(1).filter_map(|b| {
        let key = tag(b, "Key")?;
        Some(Copy { name: key.rsplit('/').next().unwrap_or(&key).to_string(), size: tag(b, "Size").and_then(|s| s.parse().ok()) })
    }).collect()
}

fn dav_copies(body: &str) -> Vec<Copy> {
    body.split("response>").filter_map(|b| {
        let href = tag(b, "href")?;
        let name = href.trim_end_matches('/').rsplit('/').next()?.to_string();
        Some(Copy { name, size: tag(b, "getcontentlength").and_then(|s| s.parse().ok()) })
    }).collect()
}

fn between(s: &str, a: &str, b: &str) -> Option<String> {
    let i = s.find(a)? + a.len();
    Some(s[i..i + s[i..].find(b)?].to_string())
}

struct Dest<'a>(&'a Target);

impl Dest<'_> {
    fn s(&self, k: &str) -> String {
        self.0.cfg[k].as_str().unwrap_or_default().trim().to_string()
    }

    fn local_dir(&self) -> Result<PathBuf, String> {
        match self.0.uuid() {
            Some(uuid) => usb_dir(uuid),
            None => Ok(PathBuf::from(self.s("path"))),
        }
    }

    fn s3_url(&self, key: &str) -> String {
        format!("{}/{}/{}", self.s("endpoint").trim_end_matches('/'), self.s("bucket"), key)
    }

    fn s3_auth(&self) -> (String, String) {
        let region = Some(self.s("region")).filter(|r| !r.is_empty()).unwrap_or_else(|| "auto".into());
        (format!("aws:amz:{region}:s3"), format!("{}:{}", self.s("key"), self.s("secret")))
    }

    fn s3_key(&self, name: &str) -> String {
        let prefix = self.s("prefix");
        let prefix = prefix.trim_matches('/');
        if prefix.is_empty() { name.to_string() } else { format!("{prefix}/{name}") }
    }

    fn dav_url(&self, name: &str) -> String {
        format!("{}/{name}", self.s("url").trim_end_matches('/'))
    }

    fn put(&self, file: &FsPath, name: &str) -> Result<(), String> {
        let path = file.to_string_lossy().to_string();
        match self.0.kind.as_str() {
            "local" => {
                let dir = self.local_dir()?;
                std::fs::create_dir_all(&dir).map_err(|e| format!("No se puede usar la carpeta: {e}"))?;
                std::fs::copy(file, dir.join(name)).map_err(|e| format!("No se puede escribir en la carpeta: {e}"))?;
                Ok(())
            }
            "s3" => {
                let (sig, user) = self.s3_auth();
                let (code, body) = curl(&[
                    ("url", &self.s3_url(&self.s3_key(name))),
                    ("aws-sigv4", &sig),
                    ("user", &user),
                    ("header", "x-amz-content-sha256: UNSIGNED-PAYLOAD"),
                    ("upload-file", &path),
                ])?;
                http_ok(code, &body, "Subir la copia")
            }
            _ => {
                let user = format!("{}:{}", self.s("user"), self.s("pass"));
                let _ = curl(&[("url", &self.dav_url("")), ("request", "MKCOL"), ("user", &user)]);
                let (code, body) = curl(&[("url", &self.dav_url(name)), ("user", &user), ("upload-file", &path)])?;
                http_ok(code, &body, "Subir la copia")
            }
        }
    }

    fn list(&self) -> Result<Vec<Copy>, String> {
        let mut copies: Vec<Copy> = match self.0.kind.as_str() {
            "local" => std::fs::read_dir(self.local_dir()?)
                .map_err(|e| e.to_string())?
                .filter_map(|e| {
                    let e = e.ok()?;
                    Some(Copy { name: e.file_name().into_string().ok()?, size: e.metadata().ok().map(|m| m.len() as i64) })
                })
                .collect(),
            "s3" => {
                let (sig, user) = self.s3_auth();
                let prefix = self.s3_key("nido-");
                let url = format!("{}/{}?list-type=2&prefix={}", self.s("endpoint").trim_end_matches('/'), self.s("bucket"), prefix.replace('/', "%2F"));
                let (code, body) = curl(&[("url", &url), ("aws-sigv4", &sig), ("user", &user), ("header", "x-amz-content-sha256: UNSIGNED-PAYLOAD")])?;
                http_ok(code, &body, "Ver las copias")?;
                s3_copies(&body)
            }
            _ => {
                let user = format!("{}:{}", self.s("user"), self.s("pass"));
                let (code, body) = curl(&[("url", &self.dav_url("")), ("request", "PROPFIND"), ("header", "Depth: 1"), ("user", &user)])?;
                http_ok(code, &body, "Ver las copias")?;
                dav_copies(&body)
            }
        };
        copies.retain(|c| valid_name(&c.name) || c.name == format!("nido-prueba{SUFFIX}"));
        copies.sort_unstable_by(|a, b| b.name.cmp(&a.name));
        Ok(copies)
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        match self.0.kind.as_str() {
            "local" => std::fs::remove_file(self.local_dir()?.join(name)).map_err(|e| e.to_string()),
            "s3" => {
                let (sig, user) = self.s3_auth();
                let (code, body) = curl(&[("url", &self.s3_url(&self.s3_key(name))), ("request", "DELETE"), ("aws-sigv4", &sig), ("user", &user), ("header", "x-amz-content-sha256: UNSIGNED-PAYLOAD")])?;
                http_ok(code, &body, "Borrar una copia antigua")
            }
            _ => {
                let user = format!("{}:{}", self.s("user"), self.s("pass"));
                let (code, body) = curl(&[("url", &self.dav_url(name)), ("request", "DELETE"), ("user", &user)])?;
                http_ok(code, &body, "Borrar una copia antigua")
            }
        }
    }

    fn fetch(&self, name: &str) -> Result<Vec<u8>, String> {
        if self.0.kind == "local" {
            return std::fs::read(self.local_dir()?.join(name)).map_err(|e| e.to_string());
        }
        let tmp = data_dir().join(".backup").join("descarga");
        std::fs::create_dir_all(tmp.parent().unwrap_or(&tmp)).map_err(|e| e.to_string())?;
        let out = tmp.to_string_lossy().to_string();
        let (code, body) = if self.0.kind == "s3" {
            let (sig, user) = self.s3_auth();
            curl(&[("url", &self.s3_url(&self.s3_key(name))), ("aws-sigv4", &sig), ("user", &user), ("header", "x-amz-content-sha256: UNSIGNED-PAYLOAD"), ("output", &out)])?
        } else {
            curl(&[("url", &self.dav_url(name)), ("user", &format!("{}:{}", self.s("user"), self.s("pass"))), ("output", &out)])?
        };
        let bytes = std::fs::read(&tmp).unwrap_or_default();
        let _ = std::fs::remove_file(&tmp);
        http_ok(code, &body, "Descargar la copia")?;
        Ok(bytes)
    }

    /// Uploads the copy, then removes the oldest ones beyond `keep`.
    fn store(&self, file: &FsPath, name: &str) -> Result<(), String> {
        self.put(file, name)?;
        for old in self.list()?.into_iter().filter(|c| valid_name(&c.name)).skip(self.0.keep.max(1) as usize) {
            self.delete(&old.name)?;
        }
        Ok(())
    }

    /// Writes, lists and deletes a small file: proves the destination works without waiting for the night.
    fn check(&self) -> Result<(), String> {
        let dir = data_dir().join(".backup");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let name = format!("nido-prueba{SUFFIX}");
        let file = dir.join(&name);
        std::fs::write(&file, b"nido").map_err(|e| e.to_string())?;
        let result = self.put(&file, &name).and_then(|_| self.list()).and_then(|copies| {
            if copies.iter().any(|c| c.name == name) { self.delete(&name) } else { Err("La copia de prueba se subió pero no aparece al listar.".into()) }
        });
        let _ = std::fs::remove_file(&file);
        result
    }
}


/// Makes one copy and sends it to every destination. Returns the failures as "destination: reason".
/// "nido-AAAAMMDD-HHMMSS.tar.gz.enc" and nothing else: names arrive from URLs and remote listings.
fn valid_name(n: &str) -> bool {
    n.len() == 5 + 15 + SUFFIX.len() && n.starts_with("nido-") && n.ends_with(SUFFIX) && n[5..20].bytes().enumerate().all(|(i, b)| if i == 8 { b == b'-' } else { b.is_ascii_digit() })
}

/// The copies stored at a destination, newest first (contacts it: S3 and WebDAV go over the network).
pub fn copies(t: &Target) -> Result<Vec<Copy>, String> {
    Ok(Dest(t).list()?.into_iter().filter(|c| valid_name(&c.name)).collect())
}

pub fn run_now(db: &Db) -> Result<Vec<String>, String> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("Ya hay una copia en marcha.".into());
    }
    let result: Result<Vec<String>, String> = (|| {
        let (pass, list) = {
            let c = lock(db);
            (notify::setting(&c, "backup_pass").ok_or("Falta la contraseña de las copias.")?, targets(&c).map_err(|e| e.to_string())?)
        };
        if list.is_empty() {
            return Ok(vec![]);
        }
        let (file, name) = archive(db, &pass)?;
        let size = std::fs::metadata(&file).map(|m| m.len() as i64).unwrap_or(0);
        let mut failed = Vec::new();
        for t in &list {
            let res = Dest(t).store(&file, &name);
            let _ = lock(db).execute(
                "UPDATE backup_targets SET last_at=datetime('now','localtime'), last_ok=?2, last_error=?3, last_size=?4 WHERE id=?1",
                params![t.id, res.is_ok(), res.as_ref().err(), size],
            );
            if let Err(e) = res {
                failed.push(format!("{}: {e}", t.name));
            }
        }
        let _ = std::fs::remove_file(&file);
        Ok(failed)
    })();
    RUNNING.store(false, Ordering::SeqCst);
    let failures = match &result {
        Ok(f) => f.clone(),
        Err(e) => vec![e.clone()],
    };
    if !failures.is_empty() {
        notify::tell_all(&lock(db), &format!("<b>La copia de seguridad de Nido ha fallado</b>\n{}", failures.join("\n")));
    }
    result
}

/// Once a day at the chosen time (or as soon after it as Nido is running).
pub fn start(db: Db) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
        let due = {
            let c = lock(&db);
            let Ok((now, day)) = c.query_row("SELECT strftime('%H:%M','now','localtime'), date('now','localtime')", [], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) else { continue };
            let at = notify::setting(&c, "backup_at").unwrap_or_else(|| "04:00".into());
            let has = c.query_row("SELECT EXISTS(SELECT 1 FROM backup_targets)", [], |r| r.get::<_, bool>(0)).unwrap_or(false);
            let due = has && now >= at && notify::setting(&c, "backup_last").as_deref() != Some(day.as_str());
            if due {
                let _ = notify::set_setting(&c, "backup_last", &day);
            }
            due
        };
        if due {
            if let Err(e) = run_now(&db) {
                eprintln!("backup: {e}");
            }
        }
    });
}


#[derive(Deserialize)]
pub struct BackupF {
    month: String,
    kind: Option<String>,
    name: Option<String>,
    keep: Option<String>,
    path: Option<String>,
    uuid: Option<String>,
    endpoint: Option<String>,
    region: Option<String>,
    bucket: Option<String>,
    prefix: Option<String>,
    key: Option<String>,
    secret: Option<String>,
    url: Option<String>,
    user: Option<String>,
    pass: Option<String>,
    pass2: Option<String>,
    at: Option<String>,
}

fn to_house(c: &Connection, f: &BackupF, ok: &str) -> R {
    back(&month(c, Some(&f.month))?, Some("household"), ok)
}

fn refuse(msg: String) -> R {
    Ok((StatusCode::BAD_REQUEST, msg).into_response())
}

pub async fn save_password(State(db): State<Db>, h: HeaderMap, F(f): F<BackupF>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    let pass = f.pass.as_deref().unwrap_or_default();
    if pass.chars().count() < 8 {
        return bad("La contraseña de las copias debe tener al menos 8 caracteres.");
    }
    if f.pass2.as_deref() != Some(pass) {
        return bad("Las dos contraseñas no coinciden.");
    }
    notify::set_setting(&c, "backup_pass", pass)?;
    to_house(&c, &f, "backup_pass")
}

pub async fn save_time(State(db): State<Db>, h: HeaderMap, F(f): F<BackupF>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    let at = f.at.as_deref().unwrap_or_default();
    let valid = at.len() == 5 && at.as_bytes()[2] == b':' && at[..2].parse::<u8>().is_ok_and(|h| h < 24) && at[3..].parse::<u8>().is_ok_and(|m| m < 60);
    if !valid {
        return bad("Hora no válida");
    }
    notify::set_setting(&c, "backup_at", at)?;
    to_house(&c, &f, "backup_time")
}

pub async fn add_target(State(db): State<Db>, h: HeaderMap, F(f): F<BackupF>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    let v = |o: &Option<String>| o.as_deref().unwrap_or_default().trim().to_string();
    let kind = v(&f.kind);
    let cfg = match kind.as_str() {
        "local" if !v(&f.uuid).is_empty() => {
            let uuid = v(&f.uuid);
            let Some(disk) = disks().into_iter().find(|d| d.uuid == uuid) else { return bad("Ese disco ya no está conectado. Pulsa «Buscar de nuevo».") };
            json!({ "uuid": uuid, "label": disk.label })
        }
        "local" => {
            let path = v(&f.path);
            if !FsPath::new(&path).is_absolute() {
                return bad("Escribe la ruta completa de la carpeta, por ejemplo /media/usb/nido.");
            }
            json!({ "path": path })
        }
        "s3" => {
            let endpoint = v(&f.endpoint);
            if !(endpoint.starts_with("https://") || endpoint.starts_with("http://")) || v(&f.bucket).is_empty() || v(&f.key).is_empty() || v(&f.secret).is_empty() {
                return bad("Faltan datos: dirección (https://…), bucket, access key y secret key.");
            }
            json!({ "endpoint": endpoint, "region": v(&f.region), "bucket": v(&f.bucket), "prefix": v(&f.prefix), "key": v(&f.key), "secret": v(&f.secret) })
        }
        "webdav" => {
            let url = v(&f.url);
            if !(url.starts_with("https://") || url.starts_with("http://")) || v(&f.user).is_empty() {
                return bad("Faltan datos: dirección de la carpeta (https://…) y usuario.");
            }
            json!({ "url": url, "user": v(&f.user), "pass": f.pass.clone().unwrap_or_default() })
        }
        _ => return bad("Tipo de destino desconocido"),
    };
    let name = Some(v(&f.name)).filter(|n| !n.is_empty()).or_else(|| cfg["label"].as_str().map(str::to_string)).unwrap_or_else(|| match kind.as_str() {
        "local" => "Disco".into(),
        "s3" => "S3".into(),
        _ => "WebDAV".into(),
    });
    let keep = v(&f.keep).parse::<i64>().unwrap_or(14).clamp(1, 365);
    c.execute("INSERT INTO backup_targets(kind,name,config,keep) VALUES(?1,?2,?3,?4)", params![kind, name, cfg.to_string(), keep])?;
    to_house(&c, &f, "backup_target_added")
}

pub async fn update_target(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<BackupF>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    let name = f.name.as_deref().unwrap_or_default().trim().chars().take(60).collect::<String>();
    let keep = f.keep.as_deref().unwrap_or_default().trim().parse::<i64>().unwrap_or(14).clamp(1, 365);
    if name.is_empty() {
        return bad("Ponle un nombre al destino.");
    }
    c.execute("UPDATE backup_targets SET name=?2, keep=?3 WHERE id=?1", params![id, name, keep])?;
    to_house(&c, &f, "backup_target_saved")
}

pub async fn download(State(db): State<Db>, h: HeaderMap, Path((id, name)): Path<(i64, String)>) -> R {
    let target = {
        let c = lock(&db);
        require_user!(c, h);
        targets(&c)?.into_iter().find(|t| t.id == id)
    };
    let (Some(target), true) = (target, valid_name(&name)) else { return Ok(StatusCode::NOT_FOUND.into_response()) };
    let file = name.clone();
    match tokio::task::spawn_blocking(move || Dest(&target).fetch(&file)).await? {
        Ok(bytes) => Ok((
            [(header::CONTENT_TYPE, "application/octet-stream".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))],
            bytes,
        )
            .into_response()),
        Err(e) => refuse(format!("No se ha podido descargar: {e}")),
    }
}

pub async fn delete_copy(State(db): State<Db>, h: HeaderMap, Path((id, name)): Path<(i64, String)>) -> R {
    let target = {
        let c = lock(&db);
        require_user!(c, h);
        targets(&c)?.into_iter().find(|t| t.id == id)
    };
    let (Some(target), true) = (target, valid_name(&name)) else { return Ok(StatusCode::NOT_FOUND.into_response()) };
    if let Err(e) = tokio::task::spawn_blocking(move || Dest(&target).delete(&name)).await? {
        return refuse(format!("No se ha podido borrar: {e}"));
    }
    Ok(Redirect::to(&format!("/backups/targets/{id}?ok=backup_copy_deleted")).into_response())
}

pub async fn delete_target(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<BackupF>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    c.execute("DELETE FROM backup_targets WHERE id=?1", [id])?;
    to_house(&c, &f, "backup_target_removed")
}

pub async fn test_target(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<BackupF>) -> R {
    let target = {
        let c = lock(&db);
        require_user!(c, h);
        targets(&c)?.into_iter().find(|t| t.id == id)
    };
    let Some(target) = target else { return bad("Ese destino ya no existe.") };
    if let Err(e) = tokio::task::spawn_blocking(move || Dest(&target).check()).await? {
        return refuse(format!("No funciona: {e}"));
    }
    to_house(&lock(&db), &f, "backup_target_ok")
}

pub async fn run_backup(State(db): State<Db>, h: HeaderMap, F(f): F<BackupF>) -> R {
    {
        let c = lock(&db);
        require_user!(c, h);
        if notify::setting(&c, "backup_pass").is_none() {
            return bad("Primero elige la contraseña de las copias.");
        }
    }
    if RUNNING.load(Ordering::Relaxed) {
        return bad("Ya hay una copia en marcha.");
    }
    let bg = db.clone();
    std::thread::spawn(move || {
        if let Err(e) = run_now(&bg) {
            eprintln!("backup: {e}");
        }
    });
    to_house(&lock(&db), &f, "backup_started")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usb_disks_only() {
        let json = r#"{"blockdevices":[
            {"path":"/dev/mmcblk0p2","uuid":"a368","label":"rootfs","fstype":"ext4","size":14956888064,"fsavail":8362377216,"mountpoint":"/","tran":"mmc","model":null,"type":"part"},
            {"path":"/dev/zram0","uuid":"ca66","label":"zram0","fstype":"swap","size":948961280,"fsavail":null,"mountpoint":"[SWAP]","tran":null,"model":null,"type":"disk"},
            {"path":"/dev/sda","uuid":null,"label":null,"fstype":null,"size":32010928128,"fsavail":null,"mountpoint":null,"tran":"usb","model":"SanDisk Ultra","type":"disk","children":[
                {"path":"/dev/sda1","uuid":"7A3B-11F0","label":"COPIAS","fstype":"exfat","size":32008830976,"fsavail":30000000000,"mountpoint":null,"tran":null,"model":null,"type":"part"}]},
            {"path":"/dev/sdb1","uuid":"9f1c","label":null,"fstype":"ext4","size":500107862016,"fsavail":null,"mountpoint":null,"tran":"usb","model":"WD Elements","type":"part"}]}"#;
        let d = parse_disks(json);
        assert_eq!(d.iter().map(|d| (d.uuid.as_str(), d.label.as_str())).collect::<Vec<_>>(), [("7A3B-11F0", "COPIAS"), ("9f1c", "WD Elements")]);
        assert_eq!(d[0].model, "SanDisk Ultra");
        let flat = r#"{"blockdevices":[
            {"path":"/dev/sda","pkname":null,"uuid":null,"label":null,"fstype":null,"size":4173332480,"fsavail":null,"mountpoint":null,"tran":"usb","model":"Flash Disk","type":"disk"},
            {"path":"/dev/sda1","pkname":"sda","uuid":"AE73-DAAB","label":"NIDO BACKS","fstype":"vfat","size":4172283904,"fsavail":4163878912,"mountpoint":"/media/pi/NIDO BACKS","tran":null,"model":null,"type":"part"},
            {"path":"/dev/mmcblk0p1","pkname":"mmcblk0","uuid":"1080-F633","label":"bootfs","fstype":"vfat","size":536870912,"fsavail":437190144,"mountpoint":"/boot/firmware","tran":"mmc","model":null,"type":"part"}]}"#;
        let d = parse_disks(flat);
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].label.as_str(), d[0].model.as_str(), d[0].mountpoint.as_deref()), ("NIDO BACKS", "Flash Disk", Some("/media/pi/NIDO BACKS")));
    }

    #[test]
    fn listings_from_any_server() {
        let s3 = "<ListBucketResult><Contents><Key>casa/nido-20261113-040000.tar.gz.enc</Key></Contents><Contents><Key>nido-20261112-040000.tar.gz.enc</Key></Contents></ListBucketResult>";
        let names = |c: Vec<Copy>| c.into_iter().map(|c| (c.name, c.size)).collect::<Vec<_>>();
        let s3 = s3.replace("</Key>", "</Key><Size>1234</Size>");
        assert_eq!(names(s3_copies(&s3)), [("nido-20261113-040000.tar.gz.enc".to_string(), Some(1234)), ("nido-20261112-040000.tar.gz.enc".to_string(), Some(1234))]);
        for ns in ["d:", "D:", "ns0:", ""] {
            let dav = format!("<{ns}multistatus><{ns}response><{ns}href>/remote.php/dav/files/ana/Nido/</{ns}href></{ns}response><{ns}response><{ns}href>/remote.php/dav/files/ana/Nido/nido-20261113-040000.tar.gz.enc</{ns}href><{ns}propstat><{ns}prop><{ns}getcontentlength>153600</{ns}getcontentlength></{ns}prop></{ns}propstat></{ns}response></{ns}multistatus>");
            assert_eq!(names(dav_copies(&dav)), [("Nido".to_string(), None), ("nido-20261113-040000.tar.gz.enc".to_string(), Some(153600))], "prefix {ns}");
        }
        assert!(valid_name("nido-20261113-040000.tar.gz.enc") && !valid_name("nido-prueba.tar.gz.enc") && !valid_name("../nido-20261113-040000.tar.gz.enc"));
        assert_eq!(Copy { name: "nido-20261113-040512.tar.gz.enc".into(), size: None }.when(), "13/11/2026 04:05");
        assert_eq!(between("<Error><Message>Access Denied</Message></Error>", "<Message>", "</Message>").as_deref(), Some("Access Denied"));
    }
}
