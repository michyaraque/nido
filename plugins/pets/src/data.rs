use crate::kit::days;
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::collections::HashMap;

/// `AND <col>=<id>` for one pet, nothing for all. Inlined rather than `?1 IS NULL OR col=?1`,
/// which keeps SQLite from using the pet's index.
fn of_pet(col: &str, pet: Option<i64>) -> String {
    pet.map_or_else(String::new, |id| format!(" AND {col}={id}"))
}

/// (med or pet id, day, slot) → (who, at what time).
pub type Given = HashMap<(i64, String, String), (i64, String)>;

/// (id, name, icon).
pub const KINDS: &[(&str, &str, &str)] = &[
    ("dog", "Perro", "dog"),
    ("cat", "Gato", "cat"),
    ("bird", "Pájaro", "bird"),
    ("fish", "Pez", "fish"),
    ("rabbit", "Conejo", "rabbit"),
    ("rodent", "Roedor", "rat"),
    ("turtle", "Tortuga", "turtle"),
    ("other", "Otro", "paw-print"),
];

pub fn kind(id: &str) -> &'static (&'static str, &'static str, &'static str) {
    KINDS.iter().find(|k| k.0 == id).unwrap_or(&KINDS[KINDS.len() - 1])
}

/// (name, hex).
pub const COLORS: &[(&str, &str)] = &[
    ("Calabaza", "#e07b39"),
    ("Miel", "#c9971c"),
    ("Salvia", "#5f9a5a"),
    ("Mar", "#2f8f8b"),
    ("Cielo", "#4d7cc7"),
    ("Lavanda", "#9165c4"),
    ("Rosa", "#cf5f7d"),
    ("Cacao", "#8a6347"),
];

pub fn color(c: &str) -> &str {
    COLORS.iter().find(|x| x.1 == c).map_or(COLORS[0].1, |x| x.1)
}

/// What a health note is about (id, label, icon).
pub const HEALTH: &[(&str, &str, &str)] = &[
    ("poop", "Caca", "circle-dot"),
    ("pee", "Pis", "droplets"),
    ("food", "Comida", "utensils"),
    ("vomit", "Vómito", "circle-alert"),
    ("mood", "Ánimo", "heart"),
    ("symptom", "Síntoma", "thermometer"),
    ("other", "Otro", "pencil"),
];

/// How it went (id, label, icon, tone).
pub const MOODS: &[(&str, &str, &str, &str)] = &[
    ("good", "Bien", "smile", "ok"),
    ("so", "Regular", "meh", "warn"),
    ("bad", "Mal", "frown", "bad"),
];

pub fn health_kind(id: &str) -> &'static (&'static str, &'static str, &'static str) {
    HEALTH.iter().find(|k| k.0 == id).unwrap_or(&HEALTH[HEALTH.len() - 1])
}

pub fn mood(id: &str) -> &'static (&'static str, &'static str, &'static str, &'static str) {
    MOODS.iter().find(|k| k.0 == id).unwrap_or(&MOODS[0])
}

pub const TASK_ICONS: &[&str] = &["bath", "sparkles", "scissors", "brush", "pill", "syringe", "droplets", "bone", "spray-can", "washing-machine", "shopping-bag", "calendar"];

/// Chores that usually come with each kind of pet: (title, icon, every n days).
pub fn suggestions(kind: &str) -> &'static [(&'static str, &'static str, i64)] {
    match kind {
        "dog" => &[("Baño", "bath", 30), ("Cortar uñas", "scissors", 30), ("Cepillar", "brush", 7), ("Desparasitar", "pill", 90), ("Vacuna anual", "syringe", 365)],
        "cat" => &[("Limpiar arenero", "sparkles", 1), ("Cambiar la arena", "spray-can", 14), ("Cepillar", "brush", 7), ("Desparasitar", "pill", 90), ("Cortar uñas", "scissors", 30)],
        "bird" => &[("Limpiar la jaula", "sparkles", 7), ("Agua fresca", "droplets", 1), ("Bañera", "bath", 7)],
        "fish" => &[("Cambiar agua", "droplets", 14), ("Limpiar el filtro", "sparkles", 30)],
        "rabbit" | "rodent" => &[("Limpiar la jaula", "sparkles", 7), ("Cortar uñas", "scissors", 60), ("Cepillar", "brush", 7)],
        "turtle" => &[("Cambiar agua", "droplets", 7), ("Limpiar el terrario", "sparkles", 30)],
        _ => &[("Baño", "bath", 30), ("Limpiar su sitio", "sparkles", 7)],
    }
}

pub fn walks(kind: &str) -> bool {
    matches!(kind, "dog" | "other")
}

#[derive(Clone)]
pub struct Pet {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub color: String,
    pub breed: String,
    pub birth: String,
    pub note: String,
    pub photo: i64,
    pub sex: String,
    pub neutered: bool,
    pub chip: String,
    pub allergies: String,
    pub food: String,
    pub portion: String,
    /// Meal times, "08:00,20:00".
    pub meals: String,
    pub insurance: String,
    pub vet_name: String,
    pub vet_phone: String,
}

impl Pet {
    pub fn meal_slots(&self) -> Vec<String> {
        self.meals.split(',').filter(|t| !t.is_empty()).map(str::to_string).collect()
    }
    pub fn sex_label(&self) -> Option<String> {
        let (word, fem) = match self.sex.as_str() {
            "m" => ("Macho", false),
            "f" => ("Hembra", true),
            _ => return self.neutered.then(|| "Esterilizado".into()),
        };
        Some(if self.neutered { format!("{word} {}", if fem { "esterilizada" } else { "esterilizado" }) } else { word.into() })
    }
}

fn pet_row(r: &Row) -> rusqlite::Result<Pet> {
    Ok(Pet {
        id: r.get(0)?,
        name: r.get(1)?,
        kind: r.get(2)?,
        color: r.get(3)?,
        breed: r.get(4)?,
        birth: r.get(5)?,
        note: r.get(6)?,
        photo: r.get(7)?,
        sex: r.get(8)?,
        neutered: r.get(9)?,
        chip: r.get(10)?,
        allergies: r.get(11)?,
        food: r.get(12)?,
        portion: r.get(13)?,
        meals: r.get(14)?,
        insurance: r.get(15)?,
        vet_name: r.get(16)?,
        vet_phone: r.get(17)?,
    })
}

const PET: &str = "SELECT id,name,kind,color,breed,birth,note,photo,sex,neutered,chip,allergies,food,portion,meals,insurance,vet_name,vet_phone FROM pet";

pub fn pets(c: &Connection) -> rusqlite::Result<Vec<Pet>> {
    c.prepare(&format!("{PET} ORDER BY id"))?.query_map([], pet_row)?.collect()
}

pub fn pet(c: &Connection, id: i64) -> rusqlite::Result<Option<Pet>> {
    c.query_row(&format!("{PET} WHERE id=?1"), [id], pet_row).optional()
}

pub struct Weight {
    pub id: i64,
    pub day: String,
    pub kg: f64,
    pub by: i64,
}

/// Oldest first.
pub fn weights(c: &Connection, pet: i64) -> rusqlite::Result<Vec<Weight>> {
    c.prepare("SELECT id,day,kg,by FROM weight WHERE pet_id=?1 ORDER BY day,id")?
        .query_map([pet], |r| Ok(Weight { id: r.get(0)?, day: r.get(1)?, kg: r.get(2)?, by: r.get(3)? }))?
        .collect()
}

pub fn last_weights(c: &Connection) -> rusqlite::Result<HashMap<i64, f64>> {
    c.prepare("SELECT pet_id,kg FROM weight w WHERE id=(SELECT id FROM weight WHERE pet_id=w.pet_id ORDER BY day DESC,id DESC LIMIT 1)")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}

pub struct Med {
    pub id: i64,
    pub pet_id: i64,
    pub pet: String,
    pub title: String,
    pub dose: String,
    pub times: String,
    pub start: String,
    pub days: Option<i64>,
    pub note: String,
    pub stopped: String,
}

impl Med {
    /// The times of day of its doses; one untimed dose when it has none.
    pub fn slots(&self) -> Vec<String> {
        let s: Vec<String> = self.times.split(',').filter(|t| !t.is_empty()).map(str::to_string).collect();
        if s.is_empty() {
            vec![String::new()]
        } else {
            s
        }
    }
    pub fn active_on(&self, day: &str) -> bool {
        let (Some(d), Some(s)) = (days(day), days(&self.start)) else { return false };
        d >= s && self.days.is_none_or(|n| d - s < n) && (self.stopped.is_empty() || day < self.stopped.as_str())
    }
    pub fn last_day(&self) -> Option<String> {
        let end = self.days.map(|n| crate::kit::add_days(&self.start, n - 1));
        match (end, self.stopped.is_empty()) {
            (e, true) => e,
            (Some(e), false) if e < self.stopped => Some(e),
            (_, false) => Some(crate::kit::add_days(&self.stopped, -1)),
        }
    }
    pub fn over(&self, today: &str) -> bool {
        self.last_day().is_some_and(|l| l.as_str() < today)
    }
}

const MED: &str = "SELECT m.id,m.pet_id,p.name,m.title,m.dose,m.times,m.start,m.days,m.note,m.stopped FROM med m JOIN pet p ON p.id=m.pet_id";

fn med_row(r: &Row) -> rusqlite::Result<Med> {
    Ok(Med { id: r.get(0)?, pet_id: r.get(1)?, pet: r.get(2)?, title: r.get(3)?, dose: r.get(4)?, times: r.get(5)?, start: r.get(6)?, days: r.get(7)?, note: r.get(8)?, stopped: r.get(9)? })
}

/// Newest first; every pet when `pet` is None.
pub fn meds(c: &Connection, pet: Option<i64>) -> rusqlite::Result<Vec<Med>> {
    c.prepare(&format!("{MED} WHERE 1{} ORDER BY m.start DESC,m.id DESC", of_pet("m.pet_id", pet)))?.query_map([], med_row)?.collect()
}

pub fn med(c: &Connection, id: i64) -> rusqlite::Result<Option<Med>> {
    c.query_row(&format!("{MED} WHERE m.id=?1"), [id], med_row).optional()
}

pub fn doses(c: &Connection, from: &str) -> rusqlite::Result<Given> {
    c.prepare_cached("SELECT med_id,day,slot,by,at FROM dose WHERE day>=?1")?
        .query_map([from], |r| Ok(((r.get(0)?, r.get(1)?, r.get(2)?), (r.get(3)?, r.get(4)?))))?
        .collect()
}

pub struct Health {
    pub id: i64,
    pub at: String,
    pub kind: String,
    pub mood: String,
    pub note: String,
    pub by: i64,
}

pub fn health(c: &Connection, pet: i64, limit: i64) -> rusqlite::Result<Vec<Health>> {
    c.prepare_cached("SELECT id,at,kind,mood,note,by FROM health WHERE pet_id=?1 ORDER BY at DESC,id DESC LIMIT ?2")?
        .query_map(params![pet, limit], |r| Ok(Health { id: r.get(0)?, at: r.get(1)?, kind: r.get(2)?, mood: r.get(3)?, note: r.get(4)?, by: r.get(5)? }))?
        .collect()
}

pub struct Walk {
    pub id: i64,
    pub pet_id: i64,
    pub at: String,
    pub minutes: Option<i64>,
    pub people: Vec<i64>,
    pub note: String,
}

/// Who went on a walk: its `walk_people` ids ("2,3"), or the one who wrote it down.
const WALKERS: &str = "COALESCE((SELECT group_concat(user_id) FROM walk_people WHERE walk_id=walk.id), by)";

pub fn ids(s: &str) -> Vec<i64> {
    s.split(',').filter_map(|x| x.trim().parse().ok()).collect()
}

/// Newest first, since `from` ("YYYY-MM-DD"); every pet when `pet` is None.
pub fn walks_since(c: &Connection, pet: Option<i64>, from: &str) -> rusqlite::Result<Vec<Walk>> {
    c.prepare(&format!("SELECT id,pet_id,at,minutes,CAST({WALKERS} AS TEXT),note FROM walk WHERE at>=?1{} ORDER BY at DESC,id DESC", of_pet("pet_id", pet)))?
        .query_map([from], |r| {
            Ok(Walk { id: r.get(0)?, pet_id: r.get(1)?, at: r.get(2)?, minutes: r.get(3)?, people: ids(&r.get::<_, String>(4)?), note: r.get(5)? })
        })?
        .collect()
}

pub struct Visit {
    pub id: i64,
    pub pet_id: i64,
    pub at: String,
    pub title: String,
    pub place: String,
    pub note: String,
    pub remind: i64,
}

/// Every pet when `pet` is None; in date order.
pub fn visits(c: &Connection, pet: Option<i64>) -> rusqlite::Result<Vec<Visit>> {
    c.prepare(&format!("SELECT id,pet_id,at,title,place,note,remind FROM visit WHERE 1{} ORDER BY at", of_pet("pet_id", pet)))?
        .query_map([], |r| Ok(Visit { id: r.get(0)?, pet_id: r.get(1)?, at: r.get(2)?, title: r.get(3)?, place: r.get(4)?, note: r.get(5)?, remind: r.get(6)? }))?
        .collect()
}

pub struct Task {
    pub id: i64,
    pub pet_id: i64,
    pub title: String,
    pub icon: String,
    pub every: i64,
    pub due: String,
    pub note: String,
    /// When it was last done and by whom.
    pub last: Option<(String, i64)>,
}

/// Pending first, by date; every pet when `pet` is None.
pub fn tasks(c: &Connection, pet: Option<i64>) -> rusqlite::Result<Vec<Task>> {
    c.prepare(&format!(
        "SELECT t.id,t.pet_id,t.title,t.icon,t.every,t.due,t.note,
                (SELECT day||' '||by FROM task_done WHERE task_id=t.id ORDER BY day DESC,id DESC LIMIT 1)
         FROM task t WHERE 1{}
         ORDER BY t.due='', t.due, t.id",
        of_pet("t.pet_id", pet)
    ))?
    .query_map([], |r| {
        let last = r.get::<_, Option<String>>(7)?.and_then(|s| s.split_once(' ').map(|(d, by)| (d.to_string(), by.parse().unwrap_or(0))));
        Ok(Task { id: r.get(0)?, pet_id: r.get(1)?, title: r.get(2)?, icon: r.get(3)?, every: r.get(4)?, due: r.get(5)?, note: r.get(6)?, last })
    })?
    .collect()
}

pub fn fed(c: &Connection, from: &str) -> rusqlite::Result<Given> {
    c.prepare_cached("SELECT pet_id,day,slot,by,at FROM fed WHERE day>=?1")?
        .query_map([from], |r| Ok(((r.get(0)?, r.get(1)?, r.get(2)?), (r.get(3)?, r.get(4)?))))?
        .collect()
}

/// Kinds of entries in the health booklet (id, label, icon).
pub const RECORDS: &[(&str, &str, &str)] = &[
    ("vaccine", "Vacuna", "syringe"),
    ("deworm_in", "Desparasitación interna", "pill"),
    ("deworm_out", "Desparasitación externa", "shield-plus"),
    ("test", "Análisis o prueba", "stethoscope"),
    ("other", "Otro", "pencil"),
];

pub fn record_kind(id: &str) -> &'static (&'static str, &'static str, &'static str) {
    RECORDS.iter().find(|k| k.0 == id).unwrap_or(&RECORDS[RECORDS.len() - 1])
}

pub struct Record {
    pub id: i64,
    pub pet_id: i64,
    pub kind: String,
    pub name: String,
    pub day: String,
    pub next: String,
    pub note: String,
}

impl Record {
    pub fn title(&self) -> String {
        if self.name.is_empty() {
            record_kind(&self.kind).1.to_string()
        } else {
            self.name.clone()
        }
    }
}

/// Newest first; every pet when `pet` is None.
pub fn records(c: &Connection, pet: Option<i64>) -> rusqlite::Result<Vec<Record>> {
    c.prepare(&format!("SELECT id,pet_id,kind,name,day,next,note FROM record WHERE 1{} ORDER BY day DESC,id DESC", of_pet("pet_id", pet)))?
        .query_map([], |r| Ok(Record { id: r.get(0)?, pet_id: r.get(1)?, kind: r.get(2)?, name: r.get(3)?, day: r.get(4)?, next: r.get(5)?, note: r.get(6)? }))?
        .collect()
}

pub struct Event {
    /// "YYYY-MM-DD HH:MM", or only the day when there is no time.
    pub at: String,
    pub pet_id: i64,
    pub kind: String,
    pub by: i64,
    /// Everyone who did it, when it can be several people (walks).
    pub people: Vec<i64>,
    pub a: String,
    pub b: String,
}

/// Newest first.
pub fn feed(c: &Connection, pet: Option<i64>, limit: i64) -> rusqlite::Result<Vec<Event>> {
    c.prepare(
        &format!(
            "SELECT * FROM (SELECT at, pet_id, 'walk', by, COALESCE(minutes,''), note, CAST({walkers} AS TEXT) FROM walk WHERE 1{p} ORDER BY at DESC LIMIT ?1)
             UNION ALL SELECT * FROM (SELECT day||' '||at, pet_id, 'fed', by, slot, '', by FROM fed WHERE 1{p} ORDER BY day DESC, at DESC LIMIT ?1)
             UNION ALL SELECT * FROM (SELECT d.day||' '||d.at, m.pet_id, 'dose', d.by, m.title, d.slot, d.by FROM dose d JOIN med m ON m.id=d.med_id WHERE 1{mp} ORDER BY d.day DESC, d.at DESC LIMIT ?1)
             UNION ALL SELECT * FROM (SELECT at, pet_id, 'health', by, kind, mood, by FROM health WHERE 1{p} ORDER BY at DESC LIMIT ?1)
             UNION ALL SELECT * FROM (SELECT x.day||' '||x.at, t.pet_id, 'task', x.by, t.title, t.icon, x.by FROM task_done x JOIN task t ON t.id=x.task_id WHERE 1{tp} ORDER BY x.day DESC, x.at DESC LIMIT ?1)
             UNION ALL SELECT * FROM (SELECT day, pet_id, 'weight', by, kg, '', by FROM weight WHERE 1{p} ORDER BY day DESC LIMIT ?1)
             UNION ALL SELECT * FROM (SELECT day, pet_id, 'record', 0, kind, name, '' FROM record WHERE 1{p} ORDER BY day DESC LIMIT ?1)
             ORDER BY 1 DESC LIMIT ?1",
            walkers = WALKERS,
            p = of_pet("pet_id", pet),
            mp = of_pet("m.pet_id", pet),
            tp = of_pet("t.pet_id", pet),
        ),
    )?
    .query_map([limit], |r| {
        let a: rusqlite::types::Value = r.get(4)?;
        let a = match a {
            rusqlite::types::Value::Text(s) => s,
            rusqlite::types::Value::Integer(n) => n.to_string(),
            rusqlite::types::Value::Real(f) => f.to_string(),
            _ => String::new(),
        };
        let who: rusqlite::types::Value = r.get(6)?;
        let people = match who {
            rusqlite::types::Value::Text(s) => ids(&s),
            rusqlite::types::Value::Integer(n) if n > 0 => vec![n],
            _ => Vec::new(),
        };
        Ok(Event { at: r.get(0)?, pet_id: r.get(1)?, kind: r.get(2)?, by: r.get(3)?, people, a, b: r.get(5)? })
    })?
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn med(times: &str, start: &str, days: Option<i64>, stopped: &str) -> Med {
        Med { id: 1, pet_id: 1, pet: "Luna".into(), title: "x".into(), dose: String::new(), times: times.into(), start: start.into(), days, note: String::new(), stopped: stopped.into() }
    }

    #[test]
    fn courses() {
        let m = med("08:00,20:00", "2026-11-10", Some(10), "");
        assert!(!m.active_on("2026-11-09"));
        assert!(m.active_on("2026-11-19"));
        assert!(!m.active_on("2026-11-20"));
        assert_eq!(m.last_day().as_deref(), Some("2026-11-19"));
        assert_eq!(m.slots(), ["08:00", "20:00"]);
        let open = med("", "2026-11-10", None, "2026-11-15");
        assert_eq!(open.slots(), [""]);
        assert!(open.active_on("2026-11-14") && !open.active_on("2026-11-15"));
        assert!(open.over("2026-11-15"));
    }

    #[test]
    fn diary_mixes_everything() {
        let c = crate::open(":memory:").unwrap();
        c.execute_batch(
            "INSERT INTO pet(id,name) VALUES(1,'Toby');
             INSERT INTO walk(pet_id,at,minutes,by) VALUES(1,'2026-11-16 08:00',30,2);
             INSERT INTO walk_people(walk_id,user_id) VALUES(1,2),(1,3);
             INSERT INTO fed(pet_id,day,slot,by,at) VALUES(1,'2026-11-16','20:00',1,'20:05');
             INSERT INTO weight(pet_id,day,kg) VALUES(1,'2026-11-10',12.5);",
        )
        .unwrap();
        let f = feed(&c, Some(1), 10).unwrap();
        let kinds: Vec<&str> = f.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, ["fed", "walk", "weight"]);
        assert_eq!((f[1].a.as_str(), f[2].a.as_str()), ("30", "12.5"));
        assert_eq!(f[1].people, [2, 3], "a walk lists everyone who went");
        assert_eq!(f[0].people, [1]);
        let w = walks_since(&c, Some(1), "2026-11-01").unwrap();
        assert_eq!(w[0].people, [2, 3]);
    }
}
