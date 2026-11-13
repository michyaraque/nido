//! Incomes: the fixed monthly income (optionally confirmed each month when it arrives)
//! and one-off incomes (side jobs, invoices), which can also be pending.
use super::*;

#[derive(Deserialize)]
pub struct IncomeF {
    concept: String,
    amount: String,
    received: Option<String>,
    month: String,
    tab: Option<String>,
}

pub async fn add_income(State(db): State<Db>, h: HeaderMap, F(f): F<IncomeF>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let m = month(&c, Some(&f.month))?;
    let concept = f.concept.trim();
    if concept.is_empty() || concept.chars().count() > 60 {
        return bad("Escribe un concepto (hasta 60 letras).");
    }
    let Some(amount) = parse_cents(&f.amount).filter(|a| *a > 0) else { return bad("Importe no válido") };
    let received = if f.received.is_some() { Some(today(&c)?) } else { None };
    c.execute("INSERT INTO incomes(user_id,month,concept,amount,received) VALUES(?1,?2,?3,?4,?5)", params![me.id, m, concept, amount, received])?;
    back(&m, f.tab.as_deref(), "income_added")
}

/// Ticks / unticks a one-off income as received today.
pub async fn toggle_income(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Ctx>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let day = today(&c)?;
    c.execute("UPDATE incomes SET received=CASE WHEN received IS NULL THEN ?3 ELSE NULL END WHERE id=?1 AND user_id=?2", params![id, me.id, day])?;
    back(&month(&c, Some(&f.month))?, f.tab.as_deref(), "income_saved")
}

pub async fn delete_income(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Ctx>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    c.execute("DELETE FROM incomes WHERE id=?1 AND user_id=?2", params![id, me.id])?;
    back(&month(&c, Some(&f.month))?, f.tab.as_deref(), "income_deleted")
}

/// Ticks / unticks the fixed income of `m` as received today, with today's salary as the amount.
pub async fn toggle_salary(State(db): State<Db>, h: HeaderMap, Path(m): Path<String>, F(f): F<Ctx>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    if !valid_month(&m) {
        return bad("Mes no válido");
    }
    let removed = c.execute("DELETE FROM salary_received WHERE user_id=?1 AND month=?2", params![me.id, m])?;
    if removed == 0 {
        c.execute("INSERT INTO salary_received(user_id,month,day,amount) VALUES(?1,?2,?3,?4)", params![me.id, m, today(&c)?, me.salary])?;
    }
    back(&month(&c, Some(&f.month))?, f.tab.as_deref(), if removed == 0 { "salary_got" } else { "income_saved" })
}

#[derive(Deserialize)]
pub struct TrackF {
    track: Option<String>,
    month: String,
    tab: Option<String>,
}

/// Personal choice: confirm the fixed income every month (late or missing shows as pending),
/// or assume it arrives. Turning it on starts counting from this month.
pub async fn set_track_pay(State(db): State<Db>, h: HeaderMap, F(f): F<TrackF>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    if f.track.is_some() {
        c.execute("UPDATE users SET track_pay=1, pay_since=strftime('%Y-%m','now','localtime') WHERE id=?1 AND track_pay=0", [me.id])?;
    } else {
        c.execute("UPDATE users SET track_pay=0 WHERE id=?1", [me.id])?;
    }
    back(&month(&c, Some(&f.month))?, f.tab.as_deref(), "track_pay")
}
