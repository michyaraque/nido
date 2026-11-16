//! Forms: every action redirects back to the view it came from (`ret`) with a toast (`ok`).
use crate::{data, kit::*, lock, me, month, now, App, R};
use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
    routing::{get, post},
    Form, Router,
};
use image::{imageops::FilterType, DynamicImage, ImageDecoder, ImageReader};
use rusqlite::{params, Connection, OptionalExtension};

pub const FLASHES: &[(&str, &str)] = &[
    ("pet_added", "Mascota añadida"),
    ("pet_saved", "Ficha guardada"),
    ("pet_deleted", "Mascota borrada"),
    ("weight_saved", "Peso apuntado"),
    ("med_added", "Medicación añadida"),
    ("med_saved", "Medicación guardada"),
    ("med_stopped", "Tratamiento terminado"),
    ("dose_given", "Toma apuntada"),
    ("dose_undone", "Toma desmarcada"),
    ("health_saved", "Apuntado en su salud"),
    ("walk_saved", "Paseo apuntado"),
    ("walk_updated", "Paseo guardado"),
    ("health_updated", "Guardado"),
    ("weight_updated", "Peso guardado"),
    ("visit_added", "Cita guardada: os avisaremos antes"),
    ("visit_saved", "Cita guardada"),
    ("task_added", "Tarea añadida"),
    ("task_saved", "Tarea guardada"),
    ("task_done", "Hecho. Apuntado"),
    ("fed", "Comida apuntada"),
    ("fed_undone", "Comida desmarcada"),
    ("record_added", "Apuntado en la cartilla"),
    ("record_saved", "Cartilla guardada"),
    ("deleted", "Borrado"),
];

type F = Form<Vec<(String, String)>>;

pub fn routes() -> Router<App> {
    let big = || DefaultBodyLimit::max(25 * 1024 * 1024);
    Router::new()
        .route("/pet", post(pet_new).layer(big()))
        .route("/pet/{id}", post(pet_edit).layer(big()))
        .route("/pet/{id}/delete", post(pet_delete))
        .route("/pet/{id}/photo", get(photo))
        .route("/weight", post(weight))
        .route("/weight/{id}", post(weight_edit))
        .route("/weight/{id}/delete", post(weight_delete))
        .route("/med", post(med_new))
        .route("/med/{id}", post(med_edit))
        .route("/med/{id}/stop", post(med_stop))
        .route("/med/{id}/delete", post(med_delete))
        .route("/dose", post(dose))
        .route("/health", post(health))
        .route("/health/{id}", post(health_edit))
        .route("/health/{id}/delete", post(health_delete))
        .route("/walk", post(walk))
        .route("/walk/{id}", post(walk_edit))
        .route("/walk/{id}/delete", post(walk_delete))
        .route("/visit", post(visit_new))
        .route("/visit/{id}", post(visit_edit))
        .route("/visit/{id}/delete", post(visit_delete))
        .route("/task", post(task_new))
        .route("/task/{id}", post(task_edit))
        .route("/task/{id}/done", post(task_done))
        .route("/task/{id}/delete", post(task_delete))
        .route("/fed", post(fed))
        .route("/assets/pets.js", get(crate::view::script))
        .route("/record", post(record_new))
        .route("/record/{id}", post(record_edit))
        .route("/record/{id}/delete", post(record_delete))
}

fn field<'a>(f: &'a [(String, String)], k: &str) -> &'a str {
    f.iter().find(|p| p.0 == k).map_or("", |p| p.1.as_str())
}

fn bad(msg: &str) -> R {
    Ok((StatusCode::BAD_REQUEST, msg.to_string()).into_response())
}

/// `ret` (our own query, `p=3&s=med`) ends up in a Location header: keep it to safe characters.
fn back(c: &Connection, f: &[(String, String)], ret: Option<String>, ok: &str) -> R {
    let (today, _) = now(c)?;
    let m = month(Some(field(f, "month")), &today);
    let ret = ret.unwrap_or_else(|| field(f, "ret").to_string());
    let ret: String = ret.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '=' | '&' | '_' | '-')).take(80).collect();
    let ret = if ret.is_empty() { String::new() } else { format!("&{ret}") };
    Ok(Redirect::to(&format!("/?month={m}&tab=pets{ret}&ok={ok}")).into_response())
}

fn who(h: &HeaderMap) -> Option<i64> {
    me(h).map(|m| m.id)
}

macro_rules! user {
    ($h:expr) => {
        match who(&$h) {
            Some(u) => u,
            None => return Ok(StatusCode::FORBIDDEN.into_response()),
        }
    };
}

fn pet_of(c: &Connection, f: &[(String, String)]) -> rusqlite::Result<Option<i64>> {
    let Ok(id) = field(f, "pet").parse::<i64>() else { return Ok(None) };
    c.query_row("SELECT id FROM pet WHERE id=?1", [id], |r| r.get(0)).optional()
}

fn date(s: &str) -> Option<String> {
    let s = s.trim();
    (s.len() == 10 && days(s).is_some()).then(|| s.to_string())
}

fn datetime(s: &str) -> Option<String> {
    let s = s.trim();
    let d = date(s.get(..10)?)?;
    let t = hhmm(s.get(11..)?)?;
    Some(format!("{d} {t}"))
}

fn when(c: &Connection, f: &[(String, String)]) -> rusqlite::Result<String> {
    let (d, t) = now(c)?;
    Ok(datetime(field(f, "at")).unwrap_or(format!("{d} {t}")))
}

// ---------- pets ----------

/// A photo in any format and orientation → 480×480 WebP: sharp on a phone, light on a small server.
fn to_webp(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoder = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .into_decoder()
        .map_err(|_| "La foto no es una imagen que podamos leer. Prueba con otra (JPG, PNG o WebP).".to_string())?;
    let orientation = decoder.orientation().map_err(|e| e.to_string())?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);
    let rgb = img.resize_to_fill(480, 480, FilterType::Triangle).to_rgb8();
    Ok(webp::Encoder::from_rgb(&rgb, rgb.width(), rgb.height()).encode(70.0).to_vec())
}

async fn read_form(mut m: Multipart) -> Result<(Vec<(String, String)>, Option<Vec<u8>>), String> {
    let lost = |e: axum::extract::multipart::MultipartError| {
        eprintln!("formulario: {e}");
        "No ha llegado bien el formulario. Vuelve a intentarlo.".to_string()
    };
    let mut fields = Vec::new();
    let mut photo = Vec::new();
    while let Some(f) = m.next_field().await.map_err(lost)? {
        let name = f.name().unwrap_or("").to_string();
        if name == "photo" {
            photo = f.bytes().await.map_err(lost)?.to_vec();
        } else {
            fields.push((name, f.text().await.map_err(lost)?));
        }
    }
    if photo.is_empty() {
        return Ok((fields, None));
    }
    match tokio::task::spawn_blocking(move || to_webp(&photo)).await {
        Ok(r) => r.map(|w| (fields, Some(w))),
        Err(e) => Err(e.to_string()),
    }
}

struct PetData {
    name: String,
    kind: &'static str,
    color: String,
    breed: String,
    birth: String,
    note: String,
    kg: Option<f64>,
    sex: &'static str,
    neutered: bool,
    chip: String,
    allergies: String,
    food: String,
    portion: String,
    meals: String,
    insurance: String,
    vet_name: String,
    vet_phone: String,
}

fn times(f: &[(String, String)], name: &str) -> String {
    let mut t: Vec<String> = f.iter().filter(|p| p.0 == name).filter_map(|p| hhmm(&p.1)).collect();
    t.sort();
    t.dedup();
    t.truncate(6);
    t.join(",")
}

fn phone(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit() || matches!(c, '+' | ' ')).take(20).collect::<String>().trim().to_string()
}

fn pet_data(f: &[(String, String)]) -> Result<PetData, &'static str> {
    let name = clean(field(f, "name"), 40);
    if name.is_empty() {
        return Err("Ponle un nombre");
    }
    let kg = match field(f, "kg").trim() {
        "" => None,
        s => Some(kilos(s).ok_or("Escribe el peso en kilos, por ejemplo 4,5")?),
    };
    Ok(PetData {
        name,
        kind: data::kind(field(f, "kind")).0,
        color: data::color(field(f, "color")).to_string(),
        breed: clean(field(f, "breed"), 40),
        birth: date(field(f, "birth")).unwrap_or_default(),
        note: note(field(f, "note")),
        kg,
        sex: match field(f, "sex") {
            "m" => "m",
            "f" => "f",
            _ => "",
        },
        neutered: !field(f, "neutered").is_empty(),
        chip: clean(field(f, "chip"), 30),
        allergies: clean(field(f, "allergies"), 120),
        food: clean(field(f, "food"), 60),
        portion: clean(field(f, "portion"), 40),
        meals: times(f, "meal"),
        insurance: clean(field(f, "insurance"), 80),
        vet_name: clean(field(f, "vet_name"), 60),
        vet_phone: phone(field(f, "vet_phone")),
    })
}

const PET_COLS: &str = "name,kind,color,breed,birth,note,sex,neutered,chip,allergies,food,portion,meals,insurance,vet_name,vet_phone";

fn pet_values(d: &PetData) -> [&dyn rusqlite::ToSql; 16] {
    [&d.name, &d.kind, &d.color, &d.breed, &d.birth, &d.note, &d.sex, &d.neutered, &d.chip, &d.allergies, &d.food, &d.portion, &d.meals, &d.insurance, &d.vet_name, &d.vet_phone]
}

fn set_photo(c: &Connection, id: i64, webp: Option<&[u8]>) -> rusqlite::Result<()> {
    match webp {
        Some(b) => {
            c.execute("INSERT INTO pet_photo(pet_id,webp) VALUES(?1,?2) ON CONFLICT(pet_id) DO UPDATE SET webp=excluded.webp", params![id, b])?;
            c.execute("UPDATE pet SET photo=CAST(strftime('%s','now') AS INTEGER) WHERE id=?1", [id])?;
        }
        None => {
            c.execute("DELETE FROM pet_photo WHERE pet_id=?1", [id])?;
            c.execute("UPDATE pet SET photo=0 WHERE id=?1", [id])?;
        }
    }
    Ok(())
}

/// The weight from the pet's card, only when it is new: the history keeps one point per change.
fn card_weight(c: &Connection, pet: i64, kg: Option<f64>, by: i64) -> rusqlite::Result<()> {
    let Some(kg) = kg else { return Ok(()) };
    let last: Option<f64> = c.query_row("SELECT kg FROM weight WHERE pet_id=?1 ORDER BY day DESC,id DESC LIMIT 1", [pet], |r| r.get(0)).optional()?;
    if last.is_none_or(|l| (l - kg).abs() > 0.0005) {
        c.execute("INSERT INTO weight(pet_id,day,kg,by) VALUES(?1,date('now','localtime'),?2,?3)", params![pet, kg, by])?;
    }
    Ok(())
}

async fn pet_new(State(app): State<App>, h: HeaderMap, m: Multipart) -> R {
    let by = user!(h);
    let (f, photo) = match read_form(m).await {
        Ok(x) => x,
        Err(e) => return bad(&e),
    };
    let d = match pet_data(&f) {
        Ok(d) => d,
        Err(e) => return bad(e),
    };
    let c = lock(&app.db);
    let tx = c.unchecked_transaction()?;
    let marks: Vec<String> = (1..=16).map(|i| format!("?{i}")).collect();
    tx.execute(&format!("INSERT INTO pet({PET_COLS}) VALUES({})", marks.join(",")), &pet_values(&d)[..])?;
    let id = tx.last_insert_rowid();
    card_weight(&tx, id, d.kg, by)?;
    if let Some(p) = photo {
        set_photo(&tx, id, Some(&p))?;
    }
    tx.commit()?;
    back(&c, &f, Some(format!("p={id}")), "pet_added")
}

async fn pet_edit(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, m: Multipart) -> R {
    let by = user!(h);
    let (f, photo) = match read_form(m).await {
        Ok(x) => x,
        Err(e) => return bad(&e),
    };
    let d = match pet_data(&f) {
        Ok(d) => d,
        Err(e) => return bad(e),
    };
    let c = lock(&app.db);
    let tx = c.unchecked_transaction()?;
    let set: Vec<String> = PET_COLS.split(',').enumerate().map(|(i, c)| format!("{c}=?{}", i + 1)).collect();
    let mut values = pet_values(&d).to_vec();
    values.push(&id);
    if tx.execute(&format!("UPDATE pet SET {} WHERE id=?17", set.join(",")), &values[..])? == 0 {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    card_weight(&tx, id, d.kg, by)?;
    if let Some(p) = photo {
        set_photo(&tx, id, Some(&p))?;
    } else if !field(&f, "remove_photo").is_empty() {
        set_photo(&tx, id, None)?;
    }
    tx.commit()?;
    back(&c, &f, Some(format!("p={id}")), "pet_saved")
}

async fn pet_delete(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    c.execute("DELETE FROM pet WHERE id=?1", [id])?;
    back(&c, &f, Some(String::new()), "pet_deleted")
}

async fn photo(State(app): State<App>, Path(id): Path<i64>) -> R {
    let webp: Option<Vec<u8>> = lock(&app.db).query_row("SELECT webp FROM pet_photo WHERE pet_id=?1", [id], |r| r.get(0)).optional()?;
    Ok(match webp {
        Some(b) => ([(header::CONTENT_TYPE, "image/webp"), (header::CACHE_CONTROL, "private, max-age=31536000, immutable")], b).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    })
}

// ---------- weight ----------

async fn weight(State(app): State<App>, h: HeaderMap, Form(f): F) -> R {
    let by = user!(h);
    let c = lock(&app.db);
    let Some(pet) = pet_of(&c, &f)? else { return bad("Esa mascota ya no está") };
    let Some(kg) = kilos(field(&f, "kg")) else { return bad("Escribe el peso en kilos, por ejemplo 4,5") };
    let (today, _) = now(&c)?;
    let day = date(field(&f, "day")).filter(|d| *d <= today).unwrap_or(today);
    c.execute("INSERT INTO weight(pet_id,day,kg,by) VALUES(?1,?2,?3,?4)", params![pet, day, kg, by])?;
    back(&c, &f, None, "weight_saved")
}

async fn weight_edit(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    let Some(kg) = kilos(field(&f, "kg")) else { return bad("Escribe el peso en kilos, por ejemplo 4,5") };
    let (today, _) = now(&c)?;
    let day = date(field(&f, "day")).filter(|d| *d <= today).unwrap_or(today);
    c.execute("UPDATE weight SET kg=?1,day=?2 WHERE id=?3", params![kg, day, id])?;
    back(&c, &f, None, "weight_updated")
}

async fn weight_delete(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    c.execute("DELETE FROM weight WHERE id=?1", [id])?;
    back(&c, &f, None, "deleted")
}

// ---------- medication ----------

struct MedData {
    title: String,
    dose: String,
    times: String,
    start: String,
    days: Option<i64>,
    note: String,
}

fn med_data(c: &Connection, f: &[(String, String)]) -> Result<MedData, String> {
    let title = clean(field(f, "title"), 60);
    if title.is_empty() {
        return Err("Escribe qué medicina es".into());
    }
    let mut times: Vec<String> = f.iter().filter(|p| p.0 == "time").filter_map(|p| hhmm(&p.1)).collect();
    times.sort();
    times.dedup();
    times.truncate(6);
    let days = match field(f, "days").trim() {
        "" => None,
        s => Some(s.parse::<i64>().ok().filter(|n| (1..=365).contains(n)).ok_or("Los días van de 1 a 365, o déjalo vacío si no tiene fin")?),
    };
    let today = now(c).map_err(|e| e.to_string())?.0;
    Ok(MedData { title, dose: clean(field(f, "dose"), 60), times: times.join(","), start: date(field(f, "start")).unwrap_or(today), days, note: note(field(f, "note")) })
}

async fn med_new(State(app): State<App>, h: HeaderMap, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    let Some(pet) = pet_of(&c, &f)? else { return bad("Esa mascota ya no está") };
    let d = match med_data(&c, &f) {
        Ok(d) => d,
        Err(e) => return bad(&e),
    };
    c.execute("INSERT INTO med(pet_id,title,dose,times,start,days,note) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![pet, d.title, d.dose, d.times, d.start, d.days, d.note])?;
    back(&c, &f, None, "med_added")
}

async fn med_edit(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    let d = match med_data(&c, &f) {
        Ok(d) => d,
        Err(e) => return bad(&e),
    };
    c.execute("UPDATE med SET title=?1,dose=?2,times=?3,start=?4,days=?5,note=?6 WHERE id=?7", params![d.title, d.dose, d.times, d.start, d.days, d.note, id])?;
    back(&c, &f, None, "med_saved")
}

async fn med_stop(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    let (today, _) = now(&c)?;
    c.execute("UPDATE med SET stopped=max(?1,start) WHERE id=?2 AND stopped=''", params![add_days(&today, 1), id])?;
    back(&c, &f, None, "med_stopped")
}

async fn med_delete(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    c.execute("DELETE FROM med WHERE id=?1", [id])?;
    back(&c, &f, None, "deleted")
}

async fn dose(State(app): State<App>, h: HeaderMap, Form(f): F) -> R {
    let by = user!(h);
    let c = lock(&app.db);
    let Some(m) = field(&f, "med").parse().ok().map(|id| data::med(&c, id)).transpose()?.flatten() else { return bad("Esa medicación ya no está") };
    let (today, _) = now(&c)?;
    let day = date(field(&f, "day")).unwrap_or(today.clone());
    let slot = field(&f, "slot");
    if day > today || !m.active_on(&day) || !m.slots().iter().any(|s| s == slot) {
        return bad("Esa toma no está en el tratamiento");
    }
    let removed = c.execute("DELETE FROM dose WHERE med_id=?1 AND day=?2 AND slot=?3", params![m.id, day, slot])?;
    if removed == 0 {
        c.execute("INSERT INTO dose(med_id,day,slot,by) VALUES(?1,?2,?3,?4)", params![m.id, day, slot, by])?;
    }
    back(&c, &f, None, if removed == 0 { "dose_given" } else { "dose_undone" })
}

// ---------- health and walks ----------

struct HealthData {
    at: String,
    kind: &'static str,
    mood: &'static str,
    note: String,
}

fn health_data(c: &Connection, f: &[(String, String)]) -> rusqlite::Result<HealthData> {
    Ok(HealthData { at: when(c, f)?, kind: data::health_kind(field(f, "kind")).0, mood: data::mood(field(f, "mood")).0, note: note(field(f, "note")) })
}

async fn health(State(app): State<App>, h: HeaderMap, Form(f): F) -> R {
    let by = user!(h);
    let c = lock(&app.db);
    let Some(pet) = pet_of(&c, &f)? else { return bad("Esa mascota ya no está") };
    let d = health_data(&c, &f)?;
    c.execute("INSERT INTO health(pet_id,at,kind,mood,note,by) VALUES(?1,?2,?3,?4,?5,?6)", params![pet, d.at, d.kind, d.mood, d.note, by])?;
    back(&c, &f, None, "health_saved")
}

async fn health_edit(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    let d = health_data(&c, &f)?;
    c.execute("UPDATE health SET at=?1,kind=?2,mood=?3,note=?4 WHERE id=?5", params![d.at, d.kind, d.mood, d.note, id])?;
    back(&c, &f, None, "health_updated")
}

async fn health_delete(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    c.execute("DELETE FROM health WHERE id=?1", [id])?;
    back(&c, &f, None, "deleted")
}

struct WalkData {
    people: Vec<i64>,
    at: String,
    minutes: Option<i64>,
    note: String,
}

/// Several people can take a dog out together: every ticked "by", or whoever writes it down.
fn walk_data(c: &Connection, f: &[(String, String)], me: i64) -> rusqlite::Result<WalkData> {
    let mut people: Vec<i64> = Vec::new();
    for id in f.iter().filter(|p| p.0 == "by").filter_map(|p| p.1.parse().ok()) {
        if !people.contains(&id) {
            people.push(id);
        }
    }
    if people.is_empty() {
        people.push(me);
    }
    Ok(WalkData {
        people,
        at: when(c, f)?,
        minutes: field(f, "minutes").trim().parse::<i64>().ok().filter(|m| (1..=600).contains(m)),
        note: clean(field(f, "note"), 200),
    })
}

fn set_walkers(c: &Connection, walk: i64, people: &[i64]) -> rusqlite::Result<()> {
    c.execute("DELETE FROM walk_people WHERE walk_id=?1", [walk])?;
    for p in people {
        c.execute("INSERT OR IGNORE INTO walk_people(walk_id,user_id) VALUES(?1,?2)", params![walk, p])?;
    }
    Ok(())
}

async fn walk(State(app): State<App>, h: HeaderMap, Form(f): F) -> R {
    let me = user!(h);
    let c = lock(&app.db);
    let Some(pet) = pet_of(&c, &f)? else { return bad("Esa mascota ya no está") };
    let d = walk_data(&c, &f, me)?;
    let tx = c.unchecked_transaction()?;
    tx.execute("INSERT INTO walk(pet_id,at,minutes,by,note) VALUES(?1,?2,?3,?4,?5)", params![pet, d.at, d.minutes, d.people[0], d.note])?;
    set_walkers(&tx, tx.last_insert_rowid(), &d.people)?;
    tx.commit()?;
    back(&c, &f, None, "walk_saved")
}

async fn walk_edit(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): F) -> R {
    let me = user!(h);
    let c = lock(&app.db);
    let d = walk_data(&c, &f, me)?;
    let tx = c.unchecked_transaction()?;
    if tx.execute("UPDATE walk SET at=?1,minutes=?2,by=?3,note=?4 WHERE id=?5", params![d.at, d.minutes, d.people[0], d.note, id])? == 0 {
        return bad("Ese paseo ya no está");
    }
    set_walkers(&tx, id, &d.people)?;
    tx.commit()?;
    back(&c, &f, None, "walk_updated")
}

async fn walk_delete(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    c.execute("DELETE FROM walk WHERE id=?1", [id])?;
    back(&c, &f, None, "deleted")
}

// ---------- vet ----------

/// (minutes before the visit, label); 0 sends no reminder.
pub const REMIND: &[(i64, &str)] = &[(10080, "Una semana antes"), (2880, "Dos días antes"), (1440, "El día antes"), (180, "Tres horas antes"), (60, "Una hora antes"), (0, "Sin aviso")];

struct VisitData {
    at: String,
    title: String,
    place: String,
    note: String,
    remind: i64,
}

fn visit_data(f: &[(String, String)]) -> Result<VisitData, &'static str> {
    Ok(VisitData {
        at: datetime(field(f, "at")).ok_or("Elige el día y la hora de la cita")?,
        title: clean(field(f, "title"), 60),
        place: clean(field(f, "place"), 80),
        note: note(field(f, "note")),
        remind: field(f, "remind").parse().ok().filter(|r| REMIND.iter().any(|x| x.0 == *r)).unwrap_or(1440),
    })
}

async fn visit_new(State(app): State<App>, h: HeaderMap, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    let Some(pet) = pet_of(&c, &f)? else { return bad("Esa mascota ya no está") };
    let d = match visit_data(&f) {
        Ok(d) => d,
        Err(e) => return bad(e),
    };
    c.execute("INSERT INTO visit(pet_id,at,title,place,note,remind) VALUES(?1,?2,?3,?4,?5,?6)", params![pet, d.at, d.title, d.place, d.note, d.remind])?;
    back(&c, &f, None, "visit_added")
}

async fn visit_edit(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    let d = match visit_data(&f) {
        Ok(d) => d,
        Err(e) => return bad(e),
    };
    c.execute(
        "UPDATE visit SET sent=CASE WHEN at<>?1 OR remind<>?5 THEN 0 ELSE sent END, at=?1,title=?2,place=?3,note=?4,remind=?5 WHERE id=?6",
        params![d.at, d.title, d.place, d.note, d.remind, id],
    )?;
    back(&c, &f, None, "visit_saved")
}

async fn visit_delete(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    c.execute("DELETE FROM visit WHERE id=?1", [id])?;
    back(&c, &f, None, "deleted")
}

// ---------- chores ----------

struct TaskData {
    title: String,
    icon: &'static str,
    every: i64,
    due: String,
    note: String,
}

fn task_data(c: &Connection, f: &[(String, String)]) -> Result<TaskData, String> {
    let title = clean(field(f, "title"), 60);
    if title.is_empty() {
        return Err("Escribe qué hay que hacer".into());
    }
    let icon = data::TASK_ICONS.iter().find(|i| **i == field(f, "icon")).copied().unwrap_or("sparkles");
    let every = match field(f, "every").trim() {
        "" | "0" => 0,
        s => s.parse::<i64>().ok().filter(|n| (1..=730).contains(n)).ok_or("Repite cada 1 a 730 días, o déjalo vacío para una sola vez")?,
    };
    let today = now(c).map_err(|e| e.to_string())?.0;
    Ok(TaskData { title, icon, every, due: date(field(f, "due")).unwrap_or(today), note: note(field(f, "note")) })
}

async fn task_new(State(app): State<App>, h: HeaderMap, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    let Some(pet) = pet_of(&c, &f)? else { return bad("Esa mascota ya no está") };
    let d = match task_data(&c, &f) {
        Ok(d) => d,
        Err(e) => return bad(&e),
    };
    c.execute("INSERT INTO task(pet_id,title,icon,every,due,note) VALUES(?1,?2,?3,?4,?5,?6)", params![pet, d.title, d.icon, d.every, d.due, d.note])?;
    back(&c, &f, None, "task_added")
}

async fn task_edit(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    let d = match task_data(&c, &f) {
        Ok(d) => d,
        Err(e) => return bad(&e),
    };
    c.execute("UPDATE task SET title=?1,icon=?2,every=?3,due=?4,note=?5 WHERE id=?6", params![d.title, d.icon, d.every, d.due, d.note, id])?;
    back(&c, &f, None, "task_saved")
}

/// Done today: a repeating chore comes back `every` days from now, a one-off one is put away.
async fn task_done(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): F) -> R {
    let by = user!(h);
    let c = lock(&app.db);
    let (today, _) = now(&c)?;
    let Some(every): Option<i64> = c.query_row("SELECT every FROM task WHERE id=?1", [id], |r| r.get(0)).optional()? else { return bad("Esa tarea ya no está") };
    let tx = c.unchecked_transaction()?;
    tx.execute("INSERT INTO task_done(task_id,day,by) VALUES(?1,?2,?3)", params![id, today, by])?;
    let due = if every > 0 { add_days(&today, every) } else { String::new() };
    tx.execute("UPDATE task SET due=?1 WHERE id=?2", params![due, id])?;
    tx.commit()?;
    back(&c, &f, None, "task_done")
}

async fn task_delete(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    c.execute("DELETE FROM task WHERE id=?1", [id])?;
    back(&c, &f, None, "deleted")
}

// ---------- meals ----------

/// Ticks a meal as given, or unticks it. Without a `slot`, it is the next meal still to give
/// today; a pet with no meal times gets one at the current time.
async fn fed(State(app): State<App>, h: HeaderMap, Form(f): F) -> R {
    let by = user!(h);
    let c = lock(&app.db);
    let Some(pet) = pet_of(&c, &f)?.map(|id| data::pet(&c, id)).transpose()?.flatten() else { return bad("Esa mascota ya no está") };
    let (today, time) = now(&c)?;
    let day = date(field(&f, "day")).filter(|d| *d <= today).unwrap_or(today.clone());
    let slots = pet.meal_slots();
    let given = data::fed(&c, &day)?;
    let is_given = |s: &str| given.contains_key(&(pet.id, day.clone(), s.to_string()));
    let asked = field(&f, "slot");
    let slot = match asked {
        "" if slots.is_empty() => time,
        "" => {
            let soon = add_minutes(&time, 120);
            match slots.iter().find(|s| !is_given(s) && **s <= soon).or_else(|| slots.iter().find(|s| !is_given(s))) {
                Some(s) => s.clone(),
                None => return bad("Ya ha comido todo lo de hoy"),
            }
        }
        s if slots.iter().any(|x| x == s) || hhmm(s).as_deref() == Some(s) => s.to_string(),
        _ => return bad("Esa comida no está en su horario"),
    };
    let removed = if asked.is_empty() { 0 } else { c.execute("DELETE FROM fed WHERE pet_id=?1 AND day=?2 AND slot=?3", params![pet.id, day, slot])? };
    if removed == 0 {
        c.execute("INSERT OR IGNORE INTO fed(pet_id,day,slot,by) VALUES(?1,?2,?3,?4)", params![pet.id, day, slot, by])?;
    }
    back(&c, &f, None, if removed == 0 { "fed" } else { "fed_undone" })
}

/// "HH:MM" plus `n` minutes, capped at the end of the day.
fn add_minutes(t: &str, n: i64) -> String {
    let m = (t[..2].parse::<i64>().unwrap_or(0) * 60 + t[3..5].parse::<i64>().unwrap_or(0) + n).min(23 * 60 + 59);
    format!("{:02}:{:02}", m / 60, m % 60)
}

// ---------- booklet ----------

struct RecordData {
    kind: &'static str,
    name: String,
    day: String,
    next: String,
    note: String,
}

fn record_data(c: &Connection, f: &[(String, String)]) -> Result<RecordData, String> {
    let today = now(c).map_err(|e| e.to_string())?.0;
    let day = date(field(f, "day")).unwrap_or(today);
    let next = match date(field(f, "next")) {
        Some(n) => n,
        None => match field(f, "every").parse::<i64>() {
            Ok(n) if (1..=1095).contains(&n) => add_days(&day, n),
            _ => String::new(),
        },
    };
    if !next.is_empty() && next <= day {
        return Err("La próxima vez tiene que ser después de esta".into());
    }
    Ok(RecordData { kind: data::record_kind(field(f, "kind")).0, name: clean(field(f, "name"), 60), day, next, note: note(field(f, "note")) })
}

async fn record_new(State(app): State<App>, h: HeaderMap, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    let Some(pet) = pet_of(&c, &f)? else { return bad("Esa mascota ya no está") };
    let d = match record_data(&c, &f) {
        Ok(d) => d,
        Err(e) => return bad(&e),
    };
    c.execute("INSERT INTO record(pet_id,kind,name,day,next,note) VALUES(?1,?2,?3,?4,?5,?6)", params![pet, d.kind, d.name, d.day, d.next, d.note])?;
    back(&c, &f, None, "record_added")
}

async fn record_edit(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    let d = match record_data(&c, &f) {
        Ok(d) => d,
        Err(e) => return bad(&e),
    };
    c.execute(
        "UPDATE record SET sent=CASE WHEN next<>?4 THEN '' ELSE sent END, kind=?1,name=?2,day=?3,next=?4,note=?5 WHERE id=?6",
        params![d.kind, d.name, d.day, d.next, d.note, id],
    )?;
    back(&c, &f, None, "record_saved")
}

async fn record_delete(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): F) -> R {
    user!(h);
    let c = lock(&app.db);
    c.execute("DELETE FROM record WHERE id=?1", [id])?;
    back(&c, &f, None, "deleted")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_values() {
        assert_eq!(datetime("2026-11-20T10:30").as_deref(), Some("2026-11-20 10:30"));
        assert_eq!(datetime("2026-11-20"), None);
        assert_eq!(date("2026-02-30x"), None);
        assert_eq!(add_minutes("23:30", 120), "23:59");
        assert_eq!(add_minutes("08:15", 120), "10:15");
        assert_eq!(phone("+34 600-12-34-56"), "+34 600123456");
    }

    #[test]
    fn photos_become_small_webp() {
        let mut png = Vec::new();
        image::RgbImage::from_pixel(900, 600, image::Rgb([200, 120, 40])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let img = image::load_from_memory(&to_webp(&png).unwrap()).unwrap();
        assert_eq!((img.width(), img.height()), (480, 480));
    }
}
