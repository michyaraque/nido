//! Infrared remotes through a Broadlink: the house teaches Nido each button of its real remotes
//! (TV, air conditioning, fan…) and then presses them from Nido or from a scene.
use super::*;
use crate::broadlink;

#[derive(Default, Clone)]
pub struct Learn {
    pub device: i64,
    pub button: i64,
    pub name: String,
    pub state: LearnState,
}

#[derive(Default, Clone)]
pub enum LearnState {
    #[default]
    Idle,
    Waiting,
    Done,
    Failed(String),
}

pub struct Button {
    pub id: i64,
    pub device: i64,
    pub remote: String,
    pub name: String,
    pub icon: String,
}

pub fn buttons(c: &Connection, device: i64) -> rusqlite::Result<Vec<Button>> {
    c.prepare("SELECT id,device,remote,name,icon FROM ir_button WHERE ?1=0 OR device=?1 ORDER BY remote, id")?
        .query_map([device], |r| Ok(Button { id: r.get(0)?, device: r.get(1)?, remote: r.get(2)?, name: r.get(3)?, icon: r.get(4)? }))?
        .collect()
}

fn devtype(d: &Device) -> u16 {
    d.version.parse().unwrap_or(0x2737)
}

/// Streaming services get their own colour and mark on the key.
const BRANDS: &[(&str, &str, &str, &str)] = &[
    ("netflix", "Netflix", "N", "#e50914"),
    ("youtube", "YouTube", "", "#ff0000"),
    ("prime", "Prime Video", "prime", "#1399ff"),
    ("disney", "Disney+", "D+", "#113ccf"),
    ("hbo", "Max", "max", "#002be7"),
    ("spotify", "Spotify", "S", "#1db954"),
    ("twitch", "Twitch", "T", "#9146ff"),
];

/// Icons a button can wear, in the picker's order.
const ICONS: &[(&str, &str)] = &[
    ("power", "Encender"), ("volume-2", "Subir volumen"), ("volume-1", "Bajar volumen"), ("volume-x", "Silencio"),
    ("chevron-up", "Arriba"), ("chevron-down", "Abajo"), ("chevron-left", "Izquierda"), ("chevron-right", "Derecha"),
    ("circle-dot", "OK"), ("house", "Inicio"), ("arrow-left", "Atrás"), ("log-out", "Salir"),
    ("settings", "Ajustes"), ("tv", "Entrada"), ("play", "Reproducir"), ("pause", "Pausa"),
    ("square", "Parar"), ("rewind", "Retroceder"), ("fast-forward", "Avanzar"), ("plus", "Más"),
    ("minus", "Menos"), ("thermometer", "Temperatura"), ("fan", "Ventilador"), ("wind", "Modo"),
    ("lightbulb", "Luz"), ("sun", "Día"), ("moon", "Noche"), ("timer", "Temporizador"),
];

/// The icon a button gets from its name when nobody picked one: "Volumen arriba" is a speaker,
/// "Ir a Netflix" is Netflix.
pub fn auto_icon(name: &str) -> &'static str {
    let n = fold(name);
    let has = |w: &[&str]| w.iter().any(|k| n.contains(k));
    for (id, _, _, _) in BRANDS {
        if n.contains(id) || (*id == "hbo" && n.contains(" max ")) {
            return id;
        }
    }
    if has(&["silencio", "mute"]) {
        "volume-x"
    } else if has(&["volumen arriba", "subir volumen", "vol +", "vol+", "volumen +", "volumen mas"]) {
        "volume-2"
    } else if has(&["volumen abajo", "bajar volumen", "vol -", "vol-", "volumen -", "volumen menos"]) {
        "volume-1"
    } else if has(&["encender", "apagar", "power", "on off"]) {
        "power"
    } else if has(&["canal arriba", "canal +", "canal mas"]) {
        "chevron-up"
    } else if has(&["canal abajo", "canal -", "canal menos"]) {
        "chevron-down"
    } else if has(&["home", "inicio"]) {
        "house"
    } else if has(&["atras", "volver", "back"]) {
        "arrow-left"
    } else if has(&["salir", "exit"]) {
        "log-out"
    } else if has(&[" ok ", "aceptar", "enter", "intro"]) {
        "circle-dot"
    } else if has(&["arriba"]) {
        "chevron-up"
    } else if has(&["abajo"]) {
        "chevron-down"
    } else if has(&["izquierda"]) {
        "chevron-left"
    } else if has(&["derecha"]) {
        "chevron-right"
    } else if has(&["menu", "ajustes", "config"]) {
        "settings"
    } else if has(&["hdmi", "entrada", "input", "fuente"]) {
        "tv"
    } else if has(&["pausa", "pause"]) {
        "pause"
    } else if has(&["play", "reproducir"]) {
        "play"
    } else if has(&["stop", "parar"]) {
        "square"
    } else if has(&["rebobinar", "retroceder"]) {
        "rewind"
    } else if has(&["avanzar", "adelantar"]) {
        "fast-forward"
    } else if has(&["temperatura", "grados", "frio", "calor"]) {
        "thermometer"
    } else if has(&["ventilador", "velocidad"]) {
        "fan"
    } else if has(&["luz", "lampara"]) {
        "lightbulb"
    } else if has(&["temporizador", "timer", "sleep"]) {
        "timer"
    } else if has(&["modo"]) {
        "wind"
    } else {
        ""
    }
}

/// Lower case, no accents, padded with spaces: " ir a netflix ".
fn fold(s: &str) -> String {
    let t: String = s
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' => 'a',
            'é' | 'è' => 'e',
            'í' => 'i',
            'ó' => 'o',
            'ú' | 'ü' => 'u',
            'ñ' => 'n',
            c => c,
        })
        .collect();
    format!(" {} ", t.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// The mark on a key: a brand, an icon, or nothing.
fn mark(icon_id: &str) -> Markup {
    if let Some((_, _, txt, color)) = BRANDS.iter().find(|b| b.0 == icon_id) {
        return html! {
            span.dv-brand style={ "--b:" (color) } aria-hidden="true" {
                @if txt.is_empty() { (super::icon("play")) } @else { (txt) }
            }
        };
    }
    html! { @if !icon_id.is_empty() { span.dv-key-ico { (super::icon(icon_id)) } } }
}

fn effective(b: &Button) -> String {
    if b.icon.is_empty() { auto_icon(&b.name).to_string() } else if b.icon == "none" { String::new() } else { b.icon.clone() }
}

fn picker(current: &str, name: &str) -> Markup {
    html! {
        fieldset.dv-pick {
            legend.label-sm { "Icono" }
            div.dv-pick-grid {
                label.dv-pick-opt title="Según el nombre" {
                    input type="radio" name="icon" value="" checked[current.is_empty()];
                    span { (super::icon("wand-sparkles")) small { "Auto" } }
                }
                label.dv-pick-opt title="Sin icono" {
                    input type="radio" name="icon" value="none" checked[current == "none"];
                    span { (super::icon("ban")) small { "Ninguno" } }
                }
                @for (id, label, _, _) in BRANDS {
                    label.dv-pick-opt title=(label) {
                        input type="radio" name="icon" value=(id) checked[current == *id];
                        span { (mark(id)) }
                    }
                }
                @for (id, label) in ICONS {
                    label.dv-pick-opt title=(label) {
                        input type="radio" name="icon" value=(id) checked[current == *id];
                        span { (super::icon(id)) }
                    }
                }
            }
            @if !name.is_empty() && current.is_empty() {
                @let a = auto_icon(name);
                p.hint { @if a.is_empty() { "Con «Auto» no lleva icono: su nombre no se parece a ninguno." } @else { "Con «Auto» lleva el que pega con su nombre." } }
            }
        }
    }
}

fn valid_icon(s: &str) -> String {
    let ok = s.is_empty() || s == "none" || BRANDS.iter().any(|b| b.0 == s) || ICONS.iter().any(|i| i.0 == s);
    if ok { s.to_string() } else { String::new() }
}

pub async fn refresh(app: &App, d: &Device) {
    let ip = d.ip.clone();
    let r = tokio::task::spawn_blocking(move || broadlink::probe(&ip)).await.ok().flatten();
    let mut live = lock(&app.live);
    let l = live.entry(d.id).or_default();
    match r {
        Some(f) if !f.locked => {
            l.online = true;
            l.misses = 0;
            l.err.clear();
        }
        Some(_) => {
            l.online = false;
            l.err = "bloqueado".into();
        }
        None => {
            l.misses = l.misses.saturating_add(1);
            if l.misses >= 3 {
                l.online = false;
                l.err = "no responde".into();
            }
        }
    }
    l.at = Some(Instant::now());
}

/// Plays a learnt button. Blocking: call it from `spawn_blocking`.
pub fn press(app: &App, button: i64) -> Result<(), String> {
    let (d, code): (Device, String) = {
        let c = lock(&app.db);
        let (dev, code): (i64, String) = c
            .query_row("SELECT device, code FROM ir_button WHERE id=?1", [button], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|_| "ese botón ya no existe")?;
        (super::one(&c, dev).ok().flatten().ok_or("el mando ya no existe")?, code)
    };
    let bytes: Vec<u8> = (0..code.len() / 2).filter_map(|i| u8::from_str_radix(&code[i * 2..i * 2 + 2], 16).ok()).collect();
    broadlink::Conn::open(&d.ip, &d.uid, devtype(&d))?.send(&bytes)
}

pub async fn press_now(State(app): State<App>, Path(id): Path<i64>) -> Response {
    let dev: i64 = lock(&app.db).query_row("SELECT device FROM ir_button WHERE id=?1", [id], |r| r.get(0)).unwrap_or(0);
    let a = app.clone();
    match tokio::task::spawn_blocking(move || press(&a, id)).await {
        Ok(Ok(())) => back(&format!("&d={dev}&ok=ir_sent")),
        Ok(Err(e)) => fail(&format!("&d={dev}"), &format!("No se ha podido enviar: {}", plain_err(&e))),
        Err(e) => fail(&format!("&d={dev}"), &e.to_string()),
    }
}

pub async fn delete_button(State(app): State<App>, Path(id): Path<i64>) -> Response {
    let c = lock(&app.db);
    let dev: i64 = c.query_row("SELECT device FROM ir_button WHERE id=?1", [id], |r| r.get(0)).unwrap_or(0);
    let _ = c.execute("DELETE FROM ir_button WHERE id=?1", [id]);
    back(&format!("&d={dev}&edit=1"))
}

pub async fn edit_button(State(app): State<App>, Path(id): Path<i64>, Form(f): Form<Params>) -> Response {
    let (name, remote) = (clean(f.get("name")), clean(f.get("remote")));
    let icon = valid_icon(&clean(f.get("icon")));
    let c = lock(&app.db);
    let dev: i64 = c.query_row("SELECT device FROM ir_button WHERE id=?1", [id], |r| r.get(0)).unwrap_or(0);
    if !name.is_empty() && !remote.is_empty() {
        let _ = c.execute("UPDATE ir_button SET name=?2, remote=?3, icon=?4 WHERE id=?1", params![id, name, remote, icon]);
    }
    back(&format!("&d={dev}&edit=1&ok=device_saved"))
}

/// The NEC address of a learnt code ("04" for LG), so the advanced form can suggest it.
fn nec_address(hex: &str) -> Option<String> {
    let b: Vec<u8> = (0..hex.len() / 2).filter_map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()).collect();
    let mut v = Vec::new();
    let mut j = 4;
    while j < b.len() && v.len() < 20 {
        let x = if b[j] == 0 && j + 2 < b.len() {
            j += 2;
            (b[j - 1] as u32) << 8 | b[j] as u32
        } else {
            b[j] as u32
        };
        v.push(x * 8192 / 269);
        j += 1;
    }
    if v.len() < 18 || !(8000..10000).contains(&v[0]) {
        return None;
    }
    let bits: Vec<u8> = v[2..18].chunks(2).map(|p| (p.get(1).copied().unwrap_or(0) > 1100) as u8).collect();
    let addr = bits.iter().enumerate().fold(0u8, |a, (i, bit)| a | bit << i);
    Some(format!("{addr:02X}"))
}

/// The "Añadir botón" window: learn it from the real remote, or type its code.
pub async fn new_button(State(app): State<App>, Path(id): Path<i64>, Form(f): Form<Params>) -> Response {
    let here = format!("&d={id}");
    let (remote, name) = (clean(f.get("remote")), clean(f.get("name")));
    if remote.is_empty() || name.is_empty() {
        return fail(&here, "Pon el mando y el nombre del botón");
    }
    let icon = valid_icon(&clean(f.get("icon")));
    let kind = clean(f.get("kind"));
    if kind != "nec" && kind != "raw" {
        return start_learn(&app, id, remote, name, icon, 0);
    }
    let hexnum = |k: &str| u32::from_str_radix(clean(f.get(k)).trim_start_matches("0x").trim_start_matches("0X"), 16).ok();
    let code = if kind == "raw" {
        broadlink::parse_code(f.get("raw").map(String::as_str).unwrap_or(""))
    } else {
        match (hexnum("addr"), hexnum("cmd")) {
            (Some(a), Some(c)) if a <= 0xffff && c <= 0xff => Some(broadlink::nec(a as u16, c as u8)),
            _ => None,
        }
    };
    let Some(code) = code else {
        return fail(&here, "Ese código no es válido. NEC: dirección y comando en hexadecimal (04 y CE). Broadlink: el código entero.");
    };
    let hex: String = code.iter().map(|b| format!("{b:02x}")).collect();
    let _ = lock(&app.db).execute("INSERT INTO ir_button(device,remote,name,code,icon) VALUES(?1,?2,?3,?4,?5)", params![id, remote, name, hex, icon]);
    back(&format!("{here}&ok=ir_added"))
}

pub async fn relearn(State(app): State<App>, Path(id): Path<i64>) -> Response {
    let b: Option<(i64, String, String, String)> = lock(&app.db)
        .query_row("SELECT device, remote, name, icon FROM ir_button WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .ok();
    match b {
        Some((dev, remote, name, icon)) => start_learn(&app, dev, remote, name, icon, id),
        None => back(""),
    }
}

/// Puts the Broadlink in learning mode and waits (in the background) for the real remote's
/// button; `button` > 0 replaces that button's code in place.
fn start_learn(app: &App, id: i64, remote: String, name: String, icon: String, button: i64) -> Response {
    let Some(d) = super::one(&lock(&app.db), id).ok().flatten() else { return back("") };
    *lock(&app.learn) = Learn { device: id, button, name: name.clone(), state: LearnState::Waiting };
    let a = app.clone();
    std::thread::spawn(move || {
        let r = (|| -> Result<(), String> {
            let mut c = broadlink::Conn::open(&d.ip, &d.uid, devtype(&d))?;
            // Some firmware hands back the previous code as soon as learning starts: wait for a different one.
            let stale = c.learnt();
            c.learn_start()?;
            for _ in 0..60 {
                std::thread::sleep(Duration::from_millis(500));
                if let Some(code) = c.learnt().filter(|code| Some(code) != stale.as_ref()) {
                    let hex: String = code.iter().map(|b| format!("{b:02x}")).collect();
                    let db = lock(&a.db);
                    let updated = db.execute("UPDATE ir_button SET code=?3 WHERE id=?1 AND device=?2", params![button, id, hex]).map_err(|e| e.to_string())?;
                    if updated == 0 {
                        db.execute("INSERT INTO ir_button(device,remote,name,code,icon) VALUES(?1,?2,?3,?4,?5)", params![id, remote, name, hex, icon])
                            .map_err(|e| e.to_string())?;
                    }
                    return Ok(());
                }
            }
            Err("No ha llegado ninguna señal en 30 segundos.".into())
        })();
        lock(&a.learn).state = match r {
            Ok(()) => LearnState::Done,
            Err(e) if e.starts_with("No ha llegado") => LearnState::Failed(format!("{e} Acércalo más y prueba otra vez.")),
            Err(e) => LearnState::Failed(plain_err(&e).to_string()),
        };
    });
    back(&format!("&d={id}{}", if button > 0 { "&edit=1" } else { "" }))
}

pub async fn scan(State(app): State<App>) -> Response {
    let found = tokio::task::spawn_blocking(broadlink::discover).await.unwrap_or_default();
    *lock(&app.bl) = found;
    back("&add=broadlink&scanned=1")
}

pub async fn add(State(app): State<App>, Form(f): Form<Params>) -> Response {
    let mac = clean(f.get("mac"));
    let Some(found) = lock(&app.bl).iter().find(|x| x.mac == mac).cloned() else { return back("&add=broadlink") };
    let c = lock(&app.db);
    let r = c.execute(
        "INSERT INTO device(driver,uid,name,category,ip,version) VALUES('broadlink',?1,'Mando IR','ir',?2,?3)
         ON CONFLICT(driver,uid) DO UPDATE SET ip=excluded.ip, version=excluded.version",
        params![found.mac, found.ip, found.devtype.to_string()],
    );
    let id: i64 = c.query_row("SELECT id FROM device WHERE driver='broadlink' AND uid=?1", [&found.mac], |r| r.get(0)).unwrap_or(0);
    match r {
        Ok(_) => back(&format!("&d={id}&ok=device_added")),
        Err(e) => fail("&add=broadlink", &e.to_string()),
    }
}

pub fn add_page(app: &App, c: &Connection, p: &Params) -> rusqlite::Result<Markup> {
    let have: Vec<String> = devices(c)?.into_iter().map(|d| d.uid).collect();
    let found = lock(&app.bl).clone();
    let scanned = p.contains_key("scanned");
    Ok(html! {
        div.page-head {
            div {
                a.back href="/?tab=devices&add=1" { (super::icon("chevron-left")) "Añadir dispositivos" }
                h1.dv-title { (logo("broadlink")) "Broadlink" }
                p { "Mandos por infrarrojos: tele, aire acondicionado, ventilador…" }
            }
        }
        (alert(p))
        section.card.dv-step {
            h2 { "Antes de buscar" }
            ol.dv-todo {
                li { span { "Tenlo emparejado en la app " b { "Broadlink" } " y conectado a la misma WiFi que Nido." } }
                li { span { "En la app, entra en el mando → " b { "⋯" } " → " b { "Propiedades" } " y desactiva " b { "Bloquear dispositivo" } "." } }
            }
            form method="post" action="/plugins/devices/broadlink/scan" {
                button.btn.primary type="submit" { (super::icon("search")) "Buscar en mi red" }
            }
        }
        @if scanned {
            section.card.dv-step {
                h2 { @if found.is_empty() { "No hemos encontrado ninguno" } @else { "Encontrados" } }
                @if found.is_empty() { p.dv-lead { "Comprueba que está enchufado y en la misma WiFi, y vuelve a buscar." } }
                div.rows {
                    @for f in &found {
                        div.row.dv-found {
                            (logo("broadlink"))
                            div.row-main { div.row-title { (broadlink::model(f.devtype)) } div.row-sub { (f.ip) } }
                            @if have.contains(&f.mac) { span.pill.ok { (super::icon("circle-check")) "Ya está en Nido" } }
                            @else if f.locked { span.pill.bad { (super::icon("lock")) "Bloqueado" } }
                            @else {
                                form method="post" action="/plugins/devices/broadlink/add" {
                                    input type="hidden" name="mac" value=(f.mac);
                                    button.btn.primary.sm type="submit" { (super::icon("plus")) "Añadir" }
                                }
                            }
                        }
                    }
                }
                @if found.iter().any(|f| f.locked) {
                    div.dv-tip { (super::icon("info")) p { "Los bloqueados solo obedecen a su app. Desbloquéalos en Propiedades y vuelve a buscar." } }
                }
            }
        }
    })
}

fn modal(id: &str, title: &str, body: Markup) -> Markup {
    html! {
        div.modal id=(id) role="dialog" aria-modal="true" aria-labelledby={ (id) "-t" } {
            a.scrim href="#_" aria-label="Cerrar" tabindex="-1" {}
            div.dialog.dialog-wide.dv-dialog {
                div.dialog-top {
                    h2 id={ (id) "-t" } { (title) }
                    a.icon-btn href="#_" aria-label="Cerrar" { (super::icon("x")) }
                }
                (body)
            }
        }
    }
}

fn remote_field(id: &str, value: &str) -> Markup {
    html! {
        div.field { label for=(id) { "Mando" } input.input id=(id) name="remote" list="dv-remotes" value=(value) placeholder="Tele, aire, ventilador…" required; }
    }
}

pub fn detail(c: &Connection, app: &App, d: &Device, p: &Params) -> rusqlite::Result<Markup> {
    let live = lock(&app.live);
    let l = live.get(&d.id);
    let all = buttons(c, d.id)?;
    let mut remotes: Vec<(&str, Vec<&Button>)> = Vec::new();
    for b in &all {
        match remotes.iter_mut().find(|(r, _)| *r == b.remote) {
            Some((_, v)) => v.push(b),
            None => remotes.push((&b.remote, vec![b])),
        }
    }
    let learn = lock(&app.learn).clone();
    let mine = learn.device == d.id;
    let editing = p.contains_key("edit");
    let nec_addr: Option<String> = c
        .query_row("SELECT code FROM ir_button WHERE device=?1 ORDER BY id DESC LIMIT 1", [d.id], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|code| nec_address(&code));
    let last_remote = remotes.last().map(|r| r.0).unwrap_or("");
    Ok(html! {
        @if mine && matches!(learn.state, LearnState::Waiting) { span hidden data-refresh="2" {} }
        div.page-head {
            div {
                a.back href="/?tab=devices" { (super::icon("chevron-left")) "Domótica" }
                h1 { (d.name) }
                p { "Broadlink " (broadlink::model(devtype(d))) @if !d.room.is_empty() { " · " (d.room) } }
            }
            a.btn.primary href="#teach" { (super::icon("plus")) "Añadir botón" }
        }
        (alert(p))
        @if let Some(l) = l.filter(|l| !l.online && !l.err.is_empty()) {
            div.alert.bad.dv-err { (super::icon("cloud-off")) span { (plain_err(&l.err)) } }
        }
        @if mine {
            @match &learn.state {
                LearnState::Waiting => {
                    section.card.dv-learning {
                        span.dv-radar aria-hidden="true" { span {} span {} (super::icon("tv")) }
                        div { h2 { "Pulsa «" (learn.name) "» en tu mando" } p { "Apúntalo al Broadlink, a un palmo, y pulsa el botón una vez." } }
                    }
                }
                LearnState::Done => { div.alert.dv-ok-msg { (super::icon("circle-check")) span { "Botón «" (learn.name) "» guardado." } } }
                LearnState::Failed(e) => { div.alert.bad.dv-err { (super::icon("circle-alert")) span { (e) } } }
                LearnState::Idle => {}
            }
        }
        @if remotes.is_empty() {
            section.card { div.empty {
                span.empty-ico { (super::icon("tv")) }
                h3 { "Aún no tiene botones" }
                p { "Enséñale los botones de tu mando: la tele, el aire, el ventilador…" }
                a.btn.primary href="#teach" { (super::icon("plus")) "Añadir el primero" }
            } }
        }
        @for (remote, list) in &remotes {
            section.card {
                div.dv-remote-head {
                    h2.dv-card-t { (remote) }
                    @if editing { a.btn.primary.sm href={ "/?tab=devices&d=" (d.id) } { (super::icon("check")) "Listo" } }
                    @else { a.btn.ghost.sm href={ "/?tab=devices&d=" (d.id) "&edit=1" } { (super::icon("pencil")) "Editar" } }
                }
                @if editing { p.hint.dv-hint-top { "Toca un botón para cambiarlo." } }
                div.dv-remote.editing[editing] {
                    @for b in list {
                        @let ic = effective(b);
                        @if editing {
                            a.dv-key href={ "#btn-" (b.id) } {
                                (mark(&ic)) span.dv-key-l { (b.name) }
                                span.dv-key-edit aria-hidden="true" { (super::icon("pencil")) }
                                @if mine && learn.button == b.id && matches!(learn.state, LearnState::Waiting) { span.dv-key-wait { (super::icon("loader-circle")) } }
                            }
                        } @else {
                            form method="post" action={ "/plugins/devices/ir/press/" (b.id) } {
                                button.dv-key type="submit" { (mark(&ic)) span.dv-key-l { (b.name) } }
                            }
                        }
                    }
                }
            }
        }
        datalist #dv-remotes { @for (r, _) in &remotes { option value=(r) {} } }
        details.card.dv-settings {
            summary { (super::icon("settings")) "Ajustes del mando" (super::icon("chevron-down")) }
            form.form method="post" action={ "/plugins/devices/" (d.id) } {
                div.two {
                    div.field { label for="name" { "Nombre" } input #name.input name="name" value=(d.name) required; }
                    div.field { label for="room" { "Habitación" } input #room.input name="room" value=(d.room) placeholder="Salón, cocina…" list="dv-rooms"; }
                }
                (rooms_list(c)?)
                div { button.btn.primary type="submit" { "Guardar" } }
            }
            form.dv-del method="post" action={ "/plugins/devices/" (d.id) "/delete" } {
                button.btn.danger type="submit" { (super::icon("trash-2")) "Quitar de Nido" }
            }
        }
        (modal("teach", "Añadir botón", html! {
            form.form method="post" action={ "/plugins/devices/ir/" (d.id) "/new" } {
                div.two {
                    (remote_field("tr", last_remote))
                    div.field { label for="tn" { "Botón" } input #tn.input name="name" placeholder="Encender" required; }
                }
                div.dv-when.three {
                    label.dv-seg { input type="radio" name="kind" value="learn" checked; span { (super::icon("tv")) "Con el mando" } }
                    label.dv-seg { input type="radio" name="kind" value="nec"; span { "Código NEC" } }
                    label.dv-seg { input type="radio" name="kind" value="raw"; span { "Código Broadlink" } }
                }
                p.hint.dv-k-learn { "Al guardar, apunta tu mando al Broadlink y pulsa ese botón una vez." }
                div.two.dv-nec {
                    div.field { label for="ca" { "Dirección" } input #ca.input name="addr" placeholder="04" value=(nec_addr.as_deref().unwrap_or("")) spellcheck="false" autocapitalize="none"; }
                    div.field { label for="cc" { "Comando" } input #cc.input name="cmd" placeholder="CE" spellcheck="false" autocapitalize="none"; }
                }
                div.field.dv-raw {
                    label for="cw" { "Código" }
                    textarea #cw.input name="raw" rows="3" placeholder="JgBQAAAB… o 260050…" spellcheck="false" {}
                }
                p.hint.dv-k-code {
                    "En hexadecimal. "
                    @if let Some(a) = &nec_addr { "Tus botones usan la dirección " b { (a) } ". " } @else { "La dirección es la de la marca: LG usa 04. " }
                    "Los comandos de cada marca están en listas como LIRC o Flipper IRDB."
                }
                (picker("", ""))
                div.dialog-actions {
                    a.btn.ghost href="#_" { "Cancelar" }
                    button.btn.primary type="submit" { "Guardar" }
                }
            }
        }))
        @if editing {
            @for b in &all {
                (modal(&format!("btn-{}", b.id), &b.name, html! {
                    form.form method="post" action={ "/plugins/devices/ir/button/" (b.id) } {
                        div.two {
                            div.field { label for={ "en" (b.id) } { "Nombre" } input.input id={ "en" (b.id) } name="name" value=(b.name) required; }
                            (remote_field(&format!("er{}", b.id), &b.remote))
                        }
                        (picker(&b.icon, &b.name))
                        div.dialog-actions { a.btn.ghost href="#_" { "Cancelar" } button.btn.primary type="submit" { "Guardar" } }
                    }
                    div.dv-dialog-more {
                        form method="post" action={ "/plugins/devices/ir/button/" (b.id) "/relearn" } {
                            button.btn.outline.sm type="submit" { (super::icon("refresh-cw")) "Volver a enseñar" }
                        }
                        form method="post" action={ "/plugins/devices/ir/button/" (b.id) "/delete" } {
                            button.btn.danger.sm type="submit" { (super::icon("trash-2")) "Eliminar" }
                        }
                    }
                }))
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_from_names() {
        assert_eq!(auto_icon("Ir a Netflix"), "netflix");
        assert_eq!(auto_icon("Volumen Arriba"), "volume-2");
        assert_eq!(auto_icon("Volumen abajo"), "volume-1");
        assert_eq!(auto_icon("Atrás"), "arrow-left");
        assert_eq!(auto_icon("Encender"), "power");
        assert_eq!(auto_icon("OK"), "circle-dot");
        assert_eq!(auto_icon("HDMI 1"), "tv");
        assert_eq!(auto_icon("Ir a PrimeVideo"), "prime");
        assert_eq!(auto_icon("Ir a Youtube"), "youtube");
        assert_eq!(auto_icon("Cosa rara"), "");
    }
}
