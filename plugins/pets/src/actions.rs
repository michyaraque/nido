//! What Nido's Atajos can do with the pets (an NFC tag next to the bowl, by the lead…), through
//! the actions protocol in plugins/README.md: `GET /actions` lists them, `POST /actions/run` does
//! one as whoever scanned, `POST /actions/undo` takes it back.
//!
//! Before writing, each action checks whether someone already did it a moment ago (two people
//! feeding the dog). Then it answers `already` and writes nothing, unless Nido sends `force=1`.
use crate::{
    act::{add_minutes, last_done, take_back, RECENT},
    data,
    kit::*,
    lock, me, now, App, Me,
};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Form, Json, Router,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub fn routes() -> Router<App> {
    Router::new().route("/actions", get(list)).route("/actions/run", post(run)).route("/actions/undo", post(undo))
}

type F = Form<Vec<(String, String)>>;

fn field<'a>(f: &'a [(String, String)], k: &str) -> &'a str {
    f.iter().find(|p| p.0 == k).map_or("", |p| p.1.as_str())
}

fn action(id: String, group: &str, name: String, icon: &str) -> Value {
    json!({ "id": id, "group": group, "name": name, "icon": icon })
}

fn catalog(c: &Connection, today: &str) -> rusqlite::Result<Vec<Value>> {
    let mut out = Vec::new();
    let pets = data::pets(c)?;
    let meds = data::meds(c, None)?;
    let tasks = data::tasks(c, None)?;
    for p in &pets {
        out.push(action(format!("meal:{}", p.id), &p.name, format!("Comida de {}", p.name), "utensils"));
        if data::walks(&p.kind) {
            out.push(action(format!("walk:{}", p.id), &p.name, format!("Paseo con {}", p.name), "footprints"));
        }
        let mut treat = action(format!("treat:{}", p.id), &p.name, format!("Premio a {}", p.name), if p.kind == "dog" { "bone" } else { "cookie" });
        treat["ask"] = "Qué premio, como Galleta".into();
        out.push(treat);
        for m in meds.iter().filter(|m| m.pet_id == p.id && !m.over(today)) {
            out.push(action(format!("dose:{}", m.id), &p.name, format!("{} a {}", m.title, p.name), "pill"));
        }
        for t in tasks.iter().filter(|t| t.pet_id == p.id) {
            out.push(action(format!("task:{}", t.id), &p.name, format!("{} de {}", t.title, p.name), &t.icon));
        }
    }
    Ok(out)
}

async fn list(State(app): State<App>) -> Response {
    let c = lock(&app.db);
    match now(&c).and_then(|(today, _)| catalog(&c, &today)) {
        Ok(list) => Json(list).into_response(),
        Err(e) => {
            eprintln!("acciones: {e}");
            Json(Vec::<Value>::new()).into_response()
        }
    }
}

/// What came of an action, as Nido reads it.
pub struct Out {
    status: &'static str,
    text: String,
    undo: Option<String>,
    href: String,
}

fn done(text: String, undo: String, href: String) -> Out {
    Out { status: "done", text, undo: Some(undo), href }
}

fn already(text: String, href: String) -> Out {
    Out { status: "already", text, undo: None, href }
}

fn error(text: &str) -> Out {
    Out { status: "error", text: text.into(), undo: None, href: String::new() }
}

/// "tú" or the person's name, for "ya está apuntada: Ana, a las 08:02".
fn who(me: &Me, id: i64) -> String {
    if id == me.id {
        return "tú".into();
    }
    me.people.iter().find(|p| p.id == id).map_or_else(|| "alguien".into(), |p| p.name.clone())
}

fn hm(t: &str) -> String {
    t.get(..5).unwrap_or(t).to_string()
}

/// The time a scan is about: the latest one due within two hours (a missed breakfast does not
/// turn tonight's dinner into breakfast), or the first of the day before any is due.
/// `Err(slot)` when that one is given already, so the person hears who gave it.
fn next_slot(slots: &[String], given: &dyn Fn(&str) -> bool, time: &str) -> Result<Option<String>, String> {
    let soon = add_minutes(time, 120);
    match slots.iter().rev().find(|s| s.as_str() <= soon.as_str()) {
        Some(s) if given(s) => Err(s.clone()),
        Some(s) => Ok(Some(s.clone())),
        None => Ok(slots.iter().find(|s| !given(s)).cloned()),
    }
}

fn meal(c: &Connection, me: &Me, pet: i64, force: bool) -> rusqlite::Result<Out> {
    let Some(p) = data::pet(c, pet)? else { return Ok(error("Esa mascota ya no está. Cambia este atajo en Nido.")) };
    let href = format!("/?tab=pets&p={}", p.id);
    let (today, time) = now(c)?;
    let slots = p.meal_slots();
    let fed_one = |s: &str| data::fed_one(c, p.id, &today, s);
    let slot = if slots.is_empty() {
        let last: Option<(i64, String)> = c
            .query_row(&format!("SELECT by,at FROM fed WHERE pet_id=?1 AND day||' '||at>={RECENT} ORDER BY day DESC, at DESC LIMIT 1"), [p.id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        if let Some((by, at)) = last.filter(|_| !force) {
            return Ok(already(format!("{} ya ha comido hace poco: {}, a las {}", p.name, who(me, by), hm(&at)), href));
        }
        time.clone()
    } else {
        let given = |s: &str| fed_one(s).ok().flatten().is_some();
        match next_slot(&slots, &given, &time) {
            Ok(Some(s)) => s,
            Err(s) if !force => {
                let g = fed_one(&s)?;
                let (by, at) = g.map_or((0, String::new()), |g| (g.by, g.at));
                return Ok(already(format!("La comida de las {s} ya está apuntada: {}, a las {}", who(me, by), hm(&at)), href));
            }
            // Given anyway, or every meal of the day is given: an extra one at this time.
            _ => time.clone(),
        }
    };
    c.execute("INSERT OR IGNORE INTO fed(pet_id,day,slot,by) VALUES(?1,?2,?3,?4)", params![p.id, today, slot, me.id])?;
    let text = if slots.contains(&slot) { format!("Comida de las {slot} apuntada") } else { "Comida apuntada".to_string() };
    Ok(done(text, format!("f{}-{}", p.id, slot.replace(':', "")), href))
}

fn walk(c: &Connection, me: &Me, pet: i64, force: bool) -> rusqlite::Result<Out> {
    let Some(p) = data::pet(c, pet)? else { return Ok(error("Esa mascota ya no está. Cambia este atajo en Nido.")) };
    let href = format!("/?tab=pets&p={}", p.id);
    if !force {
        let last: Option<(i64, String)> = c.query_row(&format!("SELECT by,at FROM walk WHERE pet_id=?1 AND at>={RECENT} ORDER BY at DESC LIMIT 1"), [p.id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        if let Some((by, at)) = last {
            return Ok(already(format!("{} ya ha salido hace poco: {}, a las {}", p.name, who(me, by), at.get(11..16).unwrap_or("")), href));
        }
    }
    let (day, time) = now(c)?;
    c.execute("INSERT INTO walk(pet_id,at,by) VALUES(?1,?2,?3)", params![p.id, format!("{day} {time}"), me.id])?;
    Ok(done(format!("Paseo con {} apuntado", p.name), format!("w{}", c.last_insert_rowid()), href))
}

fn treat(c: &Connection, me: &Me, pet: i64, what: &str) -> rusqlite::Result<Out> {
    let Some(p) = data::pet(c, pet)? else { return Ok(error("Esa mascota ya no está. Cambia este atajo en Nido.")) };
    let what = clean(what, 60);
    if what.is_empty() {
        return Ok(error("Este atajo no dice qué premio es. Escríbelo en Ajustes → Atajos."));
    }
    let (day, time) = now(c)?;
    c.execute("INSERT INTO treat(pet_id,at,what,by) VALUES(?1,?2,?3,?4)", params![p.id, format!("{day} {time}"), what, me.id])?;
    Ok(done(format!("{what} para {}", p.name), format!("t{}", c.last_insert_rowid()), format!("/?tab=pets&p={}", p.id)))
}

fn dose(c: &Connection, me: &Me, med: i64, force: bool) -> rusqlite::Result<Out> {
    let Some(m) = data::med(c, med)? else { return Ok(error("Esa medicación ya no está. Cambia este atajo en Nido.")) };
    let href = format!("/?tab=pets&p={}&s=med", m.pet_id);
    let (today, time) = now(c)?;
    if !m.active_on(&today) {
        return Ok(error(&format!("Hoy no toca {}: el tratamiento no está en marcha.", m.title)));
    }
    let slots = m.slots();
    let given = |s: &str| data::dose_one(c, m.id, &today, s).ok().flatten();
    let slot = match next_slot(&slots, &|s| given(s).is_some(), &time) {
        Ok(Some(s)) => s,
        Err(s) if !force => {
            let (by, at) = given(&s).map_or((0, String::new()), |g| (g.by, g.at));
            let which = if s.is_empty() { "La toma de hoy".to_string() } else { format!("La toma de las {s}") };
            return Ok(already(format!("{which} ya está apuntada: {}, a las {}", who(me, by), hm(&at)), href));
        }
        _ => match slots.iter().find(|s| given(s).is_none()) {
            Some(s) => s.clone(),
            None => return Ok(error(&format!("Ya están apuntadas todas las tomas de hoy de {}.", m.title))),
        },
    };
    c.execute("INSERT INTO dose(med_id,day,slot,by) VALUES(?1,?2,?3,?4)", params![m.id, today, slot, me.id])?;
    let text = if slot.is_empty() { format!("{} a {}, apuntado", m.title, m.pet) } else { format!("{} de las {slot}, apuntado", m.title) };
    Ok(done(text, format!("d{}-{}", m.id, slot.replace(':', "")), href))
}

fn task(c: &Connection, me: &Me, id: i64, force: bool) -> rusqlite::Result<Out> {
    let row: Option<(i64, String, i64)> = c.query_row("SELECT pet_id,title,every FROM task WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
    let Some((pet, title, every)) = row else { return Ok(error("Esa tarea ya no está. Cambia este atajo en Nido.")) };
    let href = format!("/?tab=pets&p={pet}&s=tasks");
    let (today, _) = now(c)?;
    if !force {
        let last: Option<(i64, String)> = c.query_row("SELECT by,at FROM task_done WHERE task_id=?1 AND day=?2 ORDER BY id DESC LIMIT 1", params![id, today], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        if let Some((by, at)) = last {
            return Ok(already(format!("«{title}» ya está hecho hoy: {}, a las {}", who(me, by), hm(&at)), href));
        }
    }
    let tx = c.unchecked_transaction()?;
    tx.execute("INSERT INTO task_done(task_id,day,by) VALUES(?1,?2,?3)", params![id, today, me.id])?;
    let done_id = tx.last_insert_rowid();
    let before = last_done(&tx, id)?;
    let due = if every > 0 { add_days(before.as_deref().unwrap_or(&today), every) } else { String::new() };
    tx.execute("UPDATE task SET due=?1 WHERE id=?2", params![due, id])?;
    tx.commit()?;
    Ok(done(format!("«{title}», hecho"), format!("k{done_id}"), href))
}

pub fn perform(c: &Connection, me: &Me, id: &str, arg: &str, force: bool) -> rusqlite::Result<Out> {
    let (kind, n) = id.split_once(':').unwrap_or((id, ""));
    let Ok(n) = n.parse::<i64>() else { return Ok(error("Esta acción ya no existe. Elige otra en Ajustes → Atajos.")) };
    match kind {
        "meal" => meal(c, me, n, force),
        "walk" => walk(c, me, n, force),
        "treat" => treat(c, me, n, arg),
        "dose" => dose(c, me, n, force),
        "task" => task(c, me, n, force),
        _ => Ok(error("Esta acción ya no existe. Elige otra en Ajustes → Atajos.")),
    }
}

async fn run(State(app): State<App>, h: HeaderMap, Form(f): F) -> Response {
    let Some(me) = me(&h) else { return StatusCode::FORBIDDEN.into_response() };
    let c = lock(&app.db);
    let out = perform(&c, &me, field(&f, "id"), field(&f, "arg"), field(&f, "force") == "1").unwrap_or_else(|e| {
        eprintln!("acciones: {e}");
        error("No se ha podido apuntar. Prueba otra vez.")
    });
    Json(json!({ "status": out.status, "text": out.text, "undo": out.undo, "href": out.href })).into_response()
}

async fn undo(State(app): State<App>, h: HeaderMap, Form(f): F) -> Response {
    let Some(me) = me(&h) else { return StatusCode::FORBIDDEN.into_response() };
    let ok = take_back(&lock(&app.db), field(&f, "undo"), me.id).unwrap_or(false);
    Json(json!({ "ok": ok })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Person;

    fn person(id: i64, name: &str) -> Person {
        Person { id, name: name.into(), color: String::new(), photo: 0 }
    }

    fn me(id: i64) -> Me {
        Me { id, name: String::new(), people: vec![person(1, "Ana"), person(2, "Leo")] }
    }

    #[test]
    fn picks_the_meal_that_is_due() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let slots = s(&["08:00", "14:00", "20:00"]);
        let none = |_: &str| false;
        let morning = |x: &str| x == "08:00";
        assert_eq!(next_slot(&slots, &none, "07:50"), Ok(Some("08:00".into())));
        assert_eq!(next_slot(&slots, &morning, "08:30"), Err("08:00".into()), "the morning one was given");
        assert_eq!(next_slot(&slots, &morning, "12:30"), Ok(Some("14:00".into())));
        assert_eq!(next_slot(&slots, &none, "05:00"), Ok(Some("08:00".into())), "early: the first of the day");
        assert_eq!(next_slot(&s(&["20:00"]), &none, "09:00"), Ok(Some("20:00".into())));
        assert_eq!(next_slot(&slots, &none, "21:50"), Ok(Some("20:00".into())), "breakfast was missed: this is dinner");
        assert_eq!(next_slot(&slots, &|x| x == "20:00", "21:50"), Err("20:00".into()));
    }

    #[test]
    fn two_people_do_not_feed_the_dog_twice() {
        let c = crate::open(":memory:").unwrap();
        c.execute_batch("INSERT INTO pet(id,name,kind,meals) VALUES(1,'Luca','dog','00:00'); INSERT INTO task(id,pet_id,title,every,due) VALUES(4,1,'Baño',30,'');").unwrap();
        let (ana, leo) = (me(1), me(2));

        let first = perform(&c, &ana, "meal:1", "", false).unwrap();
        assert_eq!((first.status, first.text.as_str()), ("done", "Comida de las 00:00 apuntada"));
        let second = perform(&c, &leo, "meal:1", "", false).unwrap();
        assert_eq!(second.status, "already");
        assert!(second.text.starts_with("La comida de las 00:00 ya está apuntada: Ana, a las "), "{}", second.text);
        assert!(perform(&c, &ana, "meal:1", "", false).unwrap().text.contains(": tú, a las "));
        let anyway = perform(&c, &leo, "meal:1", "", true).unwrap();
        assert_eq!((anyway.status, anyway.text.as_str()), ("done", "Comida apuntada"), "an extra meal at this time");

        assert!(take_back(&c, first.undo.as_deref().unwrap(), 1).unwrap());

        let w = perform(&c, &ana, "walk:1", "", false).unwrap();
        assert_eq!(w.status, "done");
        assert_eq!(perform(&c, &leo, "walk:1", "", false).unwrap().status, "already");
        assert!(take_back(&c, w.undo.as_deref().unwrap(), 1).unwrap());
        assert_eq!(perform(&c, &leo, "walk:1", "", false).unwrap().status, "done");

        assert_eq!(perform(&c, &ana, "treat:1", "", false).unwrap().status, "error");
        assert_eq!(perform(&c, &ana, "treat:1", "Galleta", false).unwrap().text, "Galleta para Luca");

        let t = perform(&c, &ana, "task:4", "", false).unwrap();
        assert_eq!(t.status, "done");
        let due: String = c.query_row("SELECT due FROM task WHERE id=4", [], |r| r.get(0)).unwrap();
        assert!(!due.is_empty());
        assert_eq!(perform(&c, &leo, "task:4", "", false).unwrap().status, "already");
        assert!(take_back(&c, t.undo.as_deref().unwrap(), 1).unwrap());
        let n: i64 = c.query_row("SELECT count(*) FROM task_done", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);

        assert_eq!(perform(&c, &ana, "meal:9", "", false).unwrap().status, "error");
        assert_eq!(perform(&c, &ana, "nope:1", "", false).unwrap().status, "error");
    }

    #[test]
    fn doses_follow_the_treatment() {
        let c = crate::open(":memory:").unwrap();
        c.execute_batch(
            "INSERT INTO pet(id,name,kind) VALUES(1,'Luca','cat');
             INSERT INTO med(id,pet_id,title,times,start) VALUES(1,1,'Apoquel','',date('now','localtime')),(2,1,'Viejo','08:00','2020-01-01');
             UPDATE med SET days=3 WHERE id=2;",
        )
        .unwrap();
        let ana = me(1);
        let d = perform(&c, &ana, "dose:1", "", false).unwrap();
        assert_eq!((d.status, d.undo.as_deref()), ("done", Some("d1-")));
        assert_eq!(perform(&c, &me(2), "dose:1", "", false).unwrap().status, "already");
        assert_eq!(perform(&c, &me(2), "dose:1", "", true).unwrap().status, "error", "one dose a day, already given");
        assert!(take_back(&c, "d1-", 1).unwrap());
        assert_eq!(perform(&c, &ana, "dose:2", "", false).unwrap().status, "error", "the course is over");
        let (today, _) = now(&c).unwrap();
        let ids: Vec<String> = catalog(&c, &today).unwrap().iter().map(|a| a["id"].as_str().unwrap().to_string()).collect();
        assert_eq!(ids, ["meal:1", "treat:1", "dose:1"], "cats do not walk; finished courses are not offered");
    }
}
