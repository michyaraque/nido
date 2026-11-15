//! Scenes: several things at once, by hand or at a time of day. "Salgo de casa" turns everything
//! off; "Buenas noches" waits ten minutes and turns off the heater; a step can also send a notice
//! to everyone in the house (Nido shows it, or pushes it to the phone).
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "k", rename_all = "lowercase")]
pub enum Step {
    Set { d: i64, p: String, v: String },
    Wait {
        #[serde(default)]
        m: u32,
        #[serde(default)]
        s: u32,
    },
    Notify { t: String },
    Press { b: i64 },
}

pub struct Scene {
    pub id: i64,
    pub name: String,
    pub icon: String,
    pub at: String,
    pub days: String,
    pub steps: Vec<Step>,
}

pub const ICONS_CHOICE: &[&str] = &["sparkles", "house", "log-out", "moon", "sun", "bed", "flame", "coffee", "tv", "lamp", "washing-machine", "power"];
const DAYS: &[(&str, &str)] = &[("1", "L"), ("2", "M"), ("3", "X"), ("4", "J"), ("5", "V"), ("6", "S"), ("0", "D")];

pub fn all(c: &Connection) -> rusqlite::Result<Vec<Scene>> {
    c.prepare("SELECT id,name,icon,at,days,steps FROM scene ORDER BY at='' DESC, at, name")?
        .query_map([], |r| {
            Ok(Scene {
                id: r.get(0)?,
                name: r.get(1)?,
                icon: r.get(2)?,
                at: r.get(3)?,
                days: r.get(4)?,
                steps: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
            })
        })?
        .collect()
}

fn one(c: &Connection, id: i64) -> Option<Scene> {
    all(c).ok()?.into_iter().find(|s| s.id == id)
}

/// "A las 23:00 · L-V", "Todos los días a las 7:30", or how many steps it has.
pub fn when(s: &Scene) -> String {
    if s.at.is_empty() {
        return match s.steps.len() {
            0 => "Sin acciones".into(),
            1 => "1 acción".into(),
            n => format!("{n} acciones"),
        };
    }
    let days = match s.days.as_str() {
        d if d.len() == 7 => "todos los días".to_string(),
        "12345" => "de lunes a viernes".into(),
        "60" | "06" => "el fin de semana".into(),
        d => DAYS.iter().filter(|(k, _)| d.contains(k)).map(|(_, l)| *l).collect::<Vec<_>>().join(" "),
    };
    format!("{} · {days}", s.at)
}

/// Runs the steps in order, in the background (a wait can last minutes).
pub fn run(app: &App, steps: Vec<Step>) {
    let app = app.clone();
    tokio::spawn(async move {
        for st in steps {
            match st {
                Step::Set { d, p, v } => {
                    let dev = one_device(&app, d);
                    if let Some(dev) = dev {
                        let want = match v.as_str() {
                            "on" => Some(true),
                            "off" => Some(false),
                            _ => None,
                        };
                        let _ = switch(&app, &dev, &p, want).await;
                    }
                }
                Step::Wait { m, s } => tokio::time::sleep(Duration::from_secs(m as u64 * 60 + s as u64)).await,
                Step::Notify { t } => notify(&app, &t),
                Step::Press { b } => {
                    let a = app.clone();
                    let _ = tokio::task::spawn_blocking(move || ir::press(&a, b)).await;
                }
            }
        }
    });
}

fn one_device(app: &App, id: i64) -> Option<Device> {
    super::one(&lock(&app.db), id).ok().flatten()
}

/// Scenes due this minute. Called every 20 s; `last_run` stops a second run in the same minute.
pub fn tick(app: &App) {
    let due: Vec<Vec<Step>> = {
        let c = lock(&app.db);
        let now: (String, String, String) = match c.query_row(
            "SELECT strftime('%H:%M','now','localtime'), strftime('%w','now','localtime'), strftime('%Y-%m-%d %H:%M','now','localtime')",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ) {
            Ok(n) => n,
            Err(_) => return,
        };
        let list = all(&c).unwrap_or_default();
        let mut due = Vec::new();
        for s in list.into_iter().filter(|s| s.at == now.0 && s.days.contains(&now.1)) {
            let fresh = c.execute("UPDATE scene SET last_run=?2 WHERE id=?1 AND last_run<>?2", params![s.id, now.2]).unwrap_or(0);
            if fresh > 0 {
                due.push(s.steps);
            }
        }
        due
    };
    for steps in due {
        run(app, steps);
    }
}

pub async fn run_now(State(app): State<App>, Path(id): Path<i64>) -> Response {
    let s = one(&lock(&app.db), id);
    match s {
        Some(s) => {
            run(&app, s.steps);
            back("&ok=scene_run")
        }
        None => back(""),
    }
}

pub async fn delete(State(app): State<App>, Path(id): Path<i64>) -> Response {
    let _ = lock(&app.db).execute("DELETE FROM scene WHERE id=?1", [id]);
    back("&ok=scene_deleted")
}

/// The editor posts everything each time: the button pressed (`op`) says what to do with it.
pub async fn save(State(app): State<App>, Form(f): Form<Vec<(String, String)>>) -> Response {
    let get = |k: &str| f.iter().find(|(n, _)| n == k).map(|(_, v)| v.trim().to_string()).unwrap_or_default();
    let n: usize = get("n").parse().unwrap_or(0).min(40);
    let mut steps: Vec<Step> = (0..n)
        .filter_map(|i| {
            let g = |k: &str| get(&format!("{k}{i}"));
            match g("k").as_str() {
                "set" => {
                    let (d, p) = g("d").split_once(':').map(|(a, b)| (a.parse().ok(), b.to_string()))?;
                    Some(Step::Set { d: d?, p, v: Some(g("v")).filter(|v| ["on", "off", "toggle"].contains(&v.as_str())).unwrap_or("off".into()) })
                }
                "wait" => {
                    let n: u32 = g("m").parse().unwrap_or(1);
                    let secs = if g("u") == "min" { n.saturating_mul(60) } else { n };
                    Some(Step::Wait { m: 0, s: secs.clamp(1, 24 * 3600) })
                }
                "notify" => Some(Step::Notify { t: g("t").chars().take(140).collect() }),
                "press" => g("b").parse().ok().map(|b| Step::Press { b }),
                _ => None,
            }
        })
        .collect();
    let op = get("op");
    let (default_dev, first_button) = {
        let c = lock(&app.db);
        (
            devices(&c).ok().and_then(|l| l.into_iter().find(|d| d.driver != "broadlink")).map(|d| (d.id, default_dp(&d))),
            ir::buttons(&c, 0).ok().and_then(|b| b.into_iter().next()).map(|b| b.id),
        )
    };
    match op.as_str() {
        "add_set" => {
            if let Some((d, p)) = default_dev {
                steps.push(Step::Set { d, p, v: "off".into() })
            }
        }
        "add_wait" => steps.push(Step::Wait { m: 0, s: 5 }),
        "add_press" => {
            if let Some(b) = first_button {
                steps.push(Step::Press { b })
            }
        }
        "add_notify" => steps.push(Step::Notify { t: String::new() }),
        o => {
            if let Some(i) = o.strip_prefix("del_").and_then(|i| i.parse::<usize>().ok()).filter(|i| *i < steps.len()) {
                steps.remove(i);
            }
            if let Some(i) = o.strip_prefix("up_").and_then(|i| i.parse::<usize>().ok()).filter(|i| *i > 0 && *i < steps.len()) {
                steps.swap(i, i - 1);
            }
        }
    }
    let name = Some(clean(Some(&get("name")))).filter(|s| !s.is_empty()).unwrap_or("Escena".into());
    let icon = Some(get("icon")).filter(|i| ICONS_CHOICE.contains(&i.as_str())).unwrap_or("sparkles".into());
    let at = Some(get("at")).filter(|a| get("timed") == "1" && a.len() == 5 && a.as_bytes()[2] == b':').unwrap_or_default();
    let mut days: String = f.iter().filter(|(k, v)| k == "day" && DAYS.iter().any(|(d, _)| d == v)).map(|(_, v)| v.as_str()).collect();
    if days.is_empty() {
        days = "0123456".into();
    }
    let json = serde_json::to_string(&steps).unwrap_or("[]".into());
    let c = lock(&app.db);
    let id: i64 = match get("id").parse::<i64>() {
        Ok(id) => {
            let _ = c.execute("UPDATE scene SET name=?2, icon=?3, at=?4, days=?5, steps=?6 WHERE id=?1", params![id, name, icon, at, days, json]);
            id
        }
        Err(_) => {
            let _ = c.execute("INSERT INTO scene(name,icon,at,days,steps) VALUES(?1,?2,?3,?4,?5)", params![name, icon, at, days, json]);
            c.last_insert_rowid()
        }
    };
    if op == "save" {
        back("&ok=scene_saved")
    } else {
        back(&format!("&scene={id}#steps"))
    }
}

/// The scenes strip on the main page.
pub fn strip(c: &Connection) -> rusqlite::Result<Markup> {
    let list = all(c)?;
    Ok(html! {
        div.dv-sec-head {
            h2 { "Escenas" }
            a.btn.ghost.sm href="/?tab=devices&scene=new" { (icon("plus")) "Nueva" }
        }
        @if list.is_empty() {
            a.dv-scene-new href="/?tab=devices&scene=new" {
                span.dv-scene-ico { (icon("sparkles")) }
                span { b { "Crea tu primera escena" } small { "Varias cosas de golpe: «Salgo de casa» lo apaga todo, «Buenas noches» apaga el calefactor a los 10 minutos…" } }
            }
        } @else {
            div.dv-scenes {
                @for s in &list {
                    div.dv-scene {
                        a.dv-scene-body href={ "/?tab=devices&scene=" (s.id) } {
                            span.dv-scene-ico { (icon(&s.icon)) }
                            span.dv-scene-txt { b { (s.name) } small { @if !s.at.is_empty() { (icon("clock")) } (when(s)) } }
                        }
                        form method="post" action={ "/plugins/devices/scenes/" (s.id) "/run" } {
                            button.dv-play type="submit" aria-label={ "Ejecutar " (s.name) } { (icon("play")) }
                        }
                    }
                }
            }
        }
    })
}

pub fn editor(c: &Connection, app: &App, id: Option<i64>) -> rusqlite::Result<Markup> {
    let s = id.and_then(|id| one(c, id)).unwrap_or(Scene { id: 0, name: String::new(), icon: "sparkles".into(), at: String::new(), days: "0123456".into(), steps: vec![] });
    let devs: Vec<Device> = devices(c)?.into_iter().filter(|d| d.driver != "broadlink").collect();
    let keys: Vec<(i64, String)> = {
        let names: Vec<(i64, String)> = devices(c)?.into_iter().map(|d| (d.id, d.name)).collect();
        ir::buttons(c, 0)?
            .into_iter()
            .map(|b| {
                let dn = names.iter().find(|(id, _)| *id == b.device).map(|(_, n)| n.as_str()).unwrap_or("");
                (b.id, format!("{} · {} ({dn})", b.remote, b.name))
            })
            .collect()
    };
    let live = lock(&app.live);
    let options: Vec<(String, String)> = devs
        .iter()
        .flat_map(|d| {
            let sw = live.get(&d.id).map(|l| switches(d, &l.dps)).filter(|s| !s.is_empty()).unwrap_or_else(|| vec![default_dp(d)]);
            let multi = sw.len() > 1;
            sw.into_iter().enumerate().map(move |(i, p)| (format!("{}:{p}", d.id), if multi { format!("{} · {}", d.name, i + 1) } else { d.name.clone() }))
        })
        .collect();
    let timed = !s.at.is_empty();
    Ok(html! {
        div.page-head {
            div {
                a.back href="/?tab=devices" { (icon("chevron-left")) "Domótica" }
                h1 { @if s.id == 0 { "Nueva escena" } @else { (s.name) } }
                p { "Varias cosas a la vez, cuando tú digas" }
            }
        }
        form.dv-editor method="post" action="/plugins/devices/scenes" {
            @if s.id > 0 { input type="hidden" name="id" value=(s.id); }
            input type="hidden" name="n" value=(s.steps.len());
            section.card {
                div.field { label for="sname" { "Nombre" } input #sname.input name="name" value=(s.name) placeholder="Salgo de casa" required; }
                fieldset.dv-icons {
                    legend.label-sm { "Icono" }
                    @for i in ICONS_CHOICE {
                        label.dv-icon-opt { input type="radio" name="icon" value=(i) checked[s.icon == *i]; span { (icon(i)) } }
                    }
                }
            }
            section.card {
                h2.dv-card-t { "Cuándo" }
                div.dv-when {
                    label.dv-seg { input type="radio" name="timed" value="0" checked[!timed]; span { (icon("play")) "Cuando la pulse" } }
                    label.dv-seg { input type="radio" name="timed" value="1" checked[timed]; span { (icon("clock")) "A una hora" } }
                }
                div.dv-time {
                    div.field { label for="sat" { "Hora" } input #sat.input type="time" name="at" value=(if timed { s.at.as_str() } else { "08:00" }); }
                    fieldset.dv-days {
                        legend.label-sm { "Días" }
                        @for (k, l) in DAYS {
                            label.dv-day { input type="checkbox" name="day" value=(k) checked[s.days.contains(k)]; span { (l) } }
                        }
                    }
                }
            }
            section.card #steps {
                h2.dv-card-t { "Qué hace" }
                @if s.steps.is_empty() { p.hint { "Añade lo que quieras que pase, en orden." } }
                ol.dv-steps {
                    @for (i, st) in s.steps.iter().enumerate() {
                        li.dv-stepline {
                            @match st {
                                Step::Set { d, p, v } => {
                                    input type="hidden" name={ "k" (i) } value="set";
                                    span.dv-step-ico { (icon("power")) }
                                    select.input name={ "v" (i) } aria-label="Acción" {
                                        option value="on" selected[v == "on"] { "Encender" }
                                        option value="off" selected[v == "off"] { "Apagar" }
                                        option value="toggle" selected[v == "toggle"] { "Cambiar" }
                                    }
                                    @let cur = format!("{d}:{p}");
                                    select.input name={ "d" (i) } aria-label="Aparato" {
                                        @for (val, label) in &options { option value=(val) selected[*val == cur] { (label) } }
                                    }
                                }
                                Step::Wait { m, s } => {
                                    @let total = m * 60 + s;
                                    @let in_min = total >= 60 && total % 60 == 0;
                                    input type="hidden" name={ "k" (i) } value="wait";
                                    span.dv-step-ico { (icon("timer")) }
                                    span.dv-step-l { "Esperar" }
                                    input.input.dv-min type="number" name={ "m" (i) } value=(if in_min { total / 60 } else { total }) min="1" max="86400" aria-label="Cuánto";
                                    select.input.dv-unit name={ "u" (i) } aria-label="Unidad" {
                                        option value="s" selected[!in_min] { "segundos" }
                                        option value="min" selected[in_min] { "minutos" }
                                    }
                                }
                                Step::Press { b } => {
                                    input type="hidden" name={ "k" (i) } value="press";
                                    span.dv-step-ico { (icon("tv")) }
                                    span.dv-step-l { "Pulsar" }
                                    select.input name={ "b" (i) } aria-label="Botón" {
                                        @for (id, label) in &keys { option value=(id) selected[*id == *b] { (label) } }
                                    }
                                }
                                Step::Notify { t } => {
                                    input type="hidden" name={ "k" (i) } value="notify";
                                    span.dv-step-ico { (icon("bell-ring")) }
                                    input.input type="text" name={ "t" (i) } value=(t) placeholder="Avisar a todos: «La lavadora ha terminado»" maxlength="140";
                                }
                            }
                            div.dv-step-btns {
                                @if i > 0 { button.icon-btn type="submit" name="op" value={ "up_" (i) } aria-label="Subir" { (icon("chevron-left")) } }
                                button.icon-btn type="submit" name="op" value={ "del_" (i) } aria-label="Quitar" { (icon("x")) }
                            }
                        }
                    }
                }
                div.dv-add-step {
                    button.btn.outline.sm type="submit" name="op" value="add_set" disabled[options.is_empty()] { (icon("power")) "Encender o apagar" }
                    button.btn.outline.sm type="submit" name="op" value="add_wait" { (icon("timer")) "Esperar" }
                    @if !keys.is_empty() { button.btn.outline.sm type="submit" name="op" value="add_press" { (icon("tv")) "Pulsar un botón del mando" } }
                    button.btn.outline.sm type="submit" name="op" value="add_notify" { (icon("bell-ring")) "Avisar" }
                }
            }
            div.dv-editor-actions {
                button.btn.primary type="submit" name="op" value="save" { (icon("check")) "Guardar escena" }
            }
        }
        @if s.id > 0 {
            form.dv-del method="post" action={ "/plugins/devices/scenes/" (s.id) "/delete" } {
                button.btn.danger type="submit" { (icon("trash-2")) "Borrar escena" }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_json_and_when() {
        let st = vec![Step::Set { d: 4, p: "1".into(), v: "off".into() }, Step::Wait { m: 0, s: 5 }, Step::Notify { t: "Hola".into() }];
        let j = serde_json::to_string(&st).unwrap();
        assert_eq!(j, r#"[{"k":"set","d":4,"p":"1","v":"off"},{"k":"wait","m":0,"s":5},{"k":"notify","t":"Hola"}]"#);
        assert_eq!(serde_json::from_str::<Vec<Step>>(&j).unwrap(), st);
        let s = Scene { id: 1, name: "x".into(), icon: "moon".into(), at: "23:00".into(), days: "12345".into(), steps: st };
        assert_eq!(when(&s), "23:00 · de lunes a viernes");
        assert_eq!(serde_json::from_str::<Step>(r#"{"k":"wait","m":10}"#).unwrap(), Step::Wait { m: 10, s: 0 });
    }
}
