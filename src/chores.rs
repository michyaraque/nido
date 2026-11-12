use super::*;

pub const ICONS: &[(&str, &str)] = &[
    ("utensils", "Platos"),
    ("brush", "Barrer"),
    ("droplets", "Fregar"),
    ("washing-machine", "Lavadora"),
    ("shirt", "Ropa"),
    ("trash-2", "Basura"),
    ("bath", "Baño"),
    ("chef-hat", "Cocinar"),
    ("shopping-cart", "Compra"),
    ("sprout", "Plantas"),
    ("dog", "Perro"),
    ("cat", "Gato"),
    ("bed", "Cama"),
    ("wind", "Aspirar"),
    ("sparkles", "Limpiar"),
    ("sofa", "Salón"),
    ("car", "Coche"),
    ("baby", "Peques"),
];

pub const COLORS: &[&str] = &["#2f6bff", "#e1166b", "#7c3aed", "#d97706", "#0891b2", "#17803d", "#db2777", "#52525b"];

pub const FREQUENCIES: &[(i64, &str)] = &[(0, "Sin fecha"), (1, "Cada día"), (2, "Cada 2 días"), (7, "Cada semana"), (14, "Cada 2 semanas"), (30, "Cada mes")];

pub const STARTERS: &[(&str, &str, &str, i64)] = &[
    ("Fregar los platos", "utensils", "#2f6bff", 1),
    ("Barrer", "brush", "#d97706", 2),
    ("Fregar el suelo", "droplets", "#0891b2", 7),
    ("Poner lavadora", "washing-machine", "#7c3aed", 3),
    ("Sacar la basura", "trash-2", "#17803d", 2),
    ("Limpiar el baño", "bath", "#e1166b", 7),
    ("Cocinar", "chef-hat", "#d97706", 1),
    ("Cambiar las sábanas", "bed", "#52525b", 14),
];

#[derive(Deserialize)]
pub struct ChoreF {
    name: String,
    icon: String,
    color: String,
    every_days: Option<i64>,
    month: String,
}

fn chore_fields(f: &ChoreF) -> Option<(String, &'static str, &'static str, Option<i64>)> {
    let name = clean(&f.name);
    let icon = ICONS.iter().find(|(i, _)| *i == f.icon)?.0;
    let color = *COLORS.iter().find(|c| **c == f.color)?;
    let every = f.every_days.filter(|d| (1..=365).contains(d));
    (!name.is_empty()).then_some((name, icon, color, every))
}

pub async fn add_chore(State(db): State<Db>, h: HeaderMap, F(f): F<ChoreF>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    let Some((name, icon, color, every)) = chore_fields(&f) else { return bad("Revisa el nombre, el icono y el color.") };
    c.execute("INSERT INTO chores(name,icon,color,every_days) VALUES(?1,?2,?3,?4)", params![name, icon, color, every])?;
    back(&month(&c, Some(&f.month))?, Some("tasks"), "chore_added")
}

pub async fn update_chore(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<ChoreF>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    let Some((name, icon, color, every)) = chore_fields(&f) else { return bad("Revisa el nombre, el icono y el color.") };
    c.execute("UPDATE chores SET name=?2,icon=?3,color=?4,every_days=?5 WHERE id=?1", params![id, name, icon, color, every])?;
    back(&month(&c, Some(&f.month))?, Some("tasks"), "chore_updated")
}

pub async fn archive_chore(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Ctx>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    c.execute("UPDATE chores SET archived=1 WHERE id=?1", [id])?;
    back(&month(&c, Some(&f.month))?, Some("tasks"), "chore_archived")
}

#[derive(Deserialize)]
pub struct LogF {
    day: Option<String>,
    user_id: Option<i64>,
    month: String,
}

pub async fn log_chore(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<LogF>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let day: String = match f.day.as_deref().filter(|d| d.len() == 10) {
        Some(d) => c.query_row("SELECT date(?1)", [d], |r| r.get::<_, Option<String>>(0))?.unwrap_or(today(&c)?),
        None => today(&c)?,
    };
    let who = match f.user_id {
        Some(u) if c.query_row("SELECT EXISTS(SELECT 1 FROM users WHERE id=?1)", [u], |r| r.get::<_, bool>(0))? => u,
        _ => me.id,
    };
    c.execute("INSERT INTO chore_logs(chore_id,user_id,day) SELECT id,?2,?3 FROM chores WHERE id=?1", params![id, who, day])?;
    let m = month(&c, Some(&f.month))?;
    let ok = if f.day.is_some() { "chore_logged_day" } else { "chore_logged" };
    Ok(Redirect::to(&format!("/?month={m}&tab=tasks&day={day}&ok={ok}")).into_response())
}

pub async fn delete_log(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<LogF>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    c.execute("DELETE FROM chore_logs WHERE id=?1", [id])?;
    let m = month(&c, Some(&f.month))?;
    let day = f.day.filter(|d| d.len() == 10).map(|d| format!("&day={d}")).unwrap_or_default();
    Ok(Redirect::to(&format!("/?month={m}&tab=tasks{day}&ok=log_deleted")).into_response())
}

#[derive(Deserialize)]
pub struct StarterF {
    index: usize,
    month: String,
}

pub async fn add_starter(State(db): State<Db>, h: HeaderMap, F(f): F<StarterF>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    if let Some((name, icon, color, every)) = STARTERS.get(f.index) {
        c.execute("INSERT INTO chores(name,icon,color,every_days) VALUES(?1,?2,?3,?4)", params![name, icon, color, every])?;
    }
    back(&month(&c, Some(&f.month))?, Some("tasks"), "chore_added")
}

#[derive(Deserialize)]
pub struct PlanF {
    day: String,
    assignee: Option<String>, // "" = anyone
    month: String,
}

fn to_tasks(m: &str, day: &str, ok: &str) -> R {
    Ok(Redirect::to(&format!("/?month={m}&tab=tasks&day={day}&ok={ok}")).into_response())
}

pub async fn plan_chore(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<PlanF>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let Some(day) = c.query_row("SELECT date(?1)", [&f.day], |r| r.get::<_, Option<String>>(0))? else { return bad("Fecha no válida") };
    let assignee = f.assignee.as_deref().and_then(|a| a.parse::<i64>().ok());
    let added = c.execute(
        "INSERT INTO chore_plans(chore_id,day,assignee_id)
         SELECT id,?2,(SELECT id FROM users WHERE id=?3) FROM chores WHERE id=?1",
        params![id, day, assignee],
    )?;
    if added > 0 {
        notify::planned(&c, &me, id, &day, assignee);
    }
    to_tasks(&month(&c, Some(&f.month))?, &day, "plan_added")
}

#[derive(Deserialize)]
pub struct PlanCtx {
    month: String,
    day: Option<String>,
}

fn plan_back(c: &Connection, f: &PlanCtx, ok: &str) -> R {
    let day = f.day.as_deref().filter(|d| d.len() == 10).unwrap_or("");
    to_tasks(&month(c, Some(&f.month))?, day, ok)
}

pub async fn take_plan(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<PlanCtx>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    c.execute("UPDATE chore_plans SET assignee_id=?2 WHERE id=?1 AND assignee_id IS NULL", params![id, me.id])?;
    plan_back(&c, &f, "plan_taken")
}

pub async fn complete_plan(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<PlanCtx>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let plan: Option<(i64, String)> = c
        .query_row("SELECT chore_id, date('now','localtime') FROM chore_plans WHERE id=?1 AND log_id IS NULL", [id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    if let Some((chore, day)) = plan {
        c.execute("INSERT INTO chore_logs(chore_id,user_id,day) VALUES(?1,?2,?3)", params![chore, me.id, day])?;
        c.execute("UPDATE chore_plans SET log_id=?2, assignee_id=COALESCE(assignee_id,?3) WHERE id=?1", params![id, c.last_insert_rowid(), me.id])?;
    }
    plan_back(&c, &f, "chore_logged")
}

pub async fn delete_plan(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<PlanCtx>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    c.execute("DELETE FROM chore_plans WHERE id=?1", [id])?;
    plan_back(&c, &f, "plan_deleted")
}
