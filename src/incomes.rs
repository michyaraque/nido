//! Incomes: the fixed monthly income (optionally confirmed each month when it arrives)
//! and one-off incomes (side jobs, invoices), which can also be pending.
use super::*;

/// Date stored when ticking something of month `m` without saying when: the usual pay day of that
/// month if given, today if it is this month, otherwise the last day of `m`. Never in the future.
fn default_day(c: &Connection, m: &str, pay_day: Option<i64>) -> rusqlite::Result<String> {
    let today = today(c)?;
    let last = days_in(c, m)?;
    let day = match pay_day {
        Some(d) => format!("{m}-{:02}", d.clamp(1, last)),
        None if today.starts_with(m) => today.clone(),
        None => format!("{m}-{last:02}"),
    };
    Ok(if day > today { today } else { day })
}

/// A real date (YYYY-MM-DD) that is not in the future.
fn valid_day(c: &Connection, d: &str) -> rusqlite::Result<bool> {
    let ok: Option<String> = c.query_row("SELECT date(?1)", [d], |r| r.get(0))?;
    Ok(ok.as_deref() == Some(d) && d <= today(c)?.as_str())
}

fn pay_day(c: &Connection, user: i64) -> rusqlite::Result<Option<i64>> {
    c.query_row("SELECT pay_day FROM users WHERE id=?1", [user], |r| r.get(0))
}

#[derive(Deserialize)]
pub struct IncomeF {
    concept: String,
    amount: String,
    received: Option<String>,
    received_on: Option<String>,
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
    let received = match (f.received.is_some(), f.received_on.as_deref().map(str::trim)) {
        (false, _) => None,
        (true, Some(d)) if !d.is_empty() => {
            if !valid_day(&c, d)? {
                return bad("Fecha de cobro no válida (no puede ser futura).");
            }
            Some(d.to_string())
        }
        (true, _) => Some(default_day(&c, &m, None)?),
    };
    c.execute("INSERT INTO incomes(user_id,month,concept,amount,received) VALUES(?1,?2,?3,?4,?5)", params![me.id, m, concept, amount, received])?;
    back(&m, f.tab.as_deref(), "income_added")
}

/// Ticks / unticks a one-off income as received today.
pub async fn toggle_income(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Ctx>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let m: Option<String> = c.query_row("SELECT month FROM incomes WHERE id=?1 AND user_id=?2", params![id, me.id], |r| r.get(0)).optional()?;
    let Some(m) = m else { return bad("Ingreso no encontrado") };
    let day = default_day(&c, &m, None)?;
    c.execute("UPDATE incomes SET received=CASE WHEN received IS NULL THEN ?3 ELSE NULL END WHERE id=?1 AND user_id=?2", params![id, me.id, day])?;
    back(&month(&c, Some(&f.month))?, f.tab.as_deref(), "income_saved")
}

pub async fn delete_income(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Ctx>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    c.execute("DELETE FROM incomes WHERE id=?1 AND user_id=?2", params![id, me.id])?;
    back(&month(&c, Some(&f.month))?, f.tab.as_deref(), "income_deleted")
}

/// Ticks / unticks the fixed income of `m` (dated on the usual pay day), with today's amount.
pub async fn toggle_salary(State(db): State<Db>, h: HeaderMap, Path(m): Path<String>, F(f): F<Ctx>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    if !valid_month(&m) {
        return bad("Mes no válido");
    }
    let removed = c.execute("DELETE FROM salary_received WHERE user_id=?1 AND month=?2", params![me.id, m])?;
    if removed == 0 {
        let day = default_day(&c, &m, pay_day(&c, me.id)?)?;
        c.execute("INSERT INTO salary_received(user_id,month,day,amount) VALUES(?1,?2,?3,?4)", params![me.id, m, day, me.salary])?;
    }
    back(&month(&c, Some(&f.month))?, f.tab.as_deref(), if removed == 0 { "salary_got" } else { "income_saved" })
}

#[derive(Deserialize)]
pub struct DayF {
    day: String,
    month: String,
    tab: Option<String>,
}

/// Corrects when an income actually arrived.
pub async fn set_income_day(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<DayF>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    if !valid_day(&c, &f.day)? {
        return bad("Fecha de cobro no válida (no puede ser futura).");
    }
    c.execute("UPDATE incomes SET received=?3 WHERE id=?1 AND user_id=?2 AND received IS NOT NULL", params![id, me.id, f.day])?;
    back(&month(&c, Some(&f.month))?, f.tab.as_deref(), "income_saved")
}

pub async fn set_salary_day(State(db): State<Db>, h: HeaderMap, Path(m): Path<String>, F(f): F<DayF>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    if !valid_day(&c, &f.day)? {
        return bad("Fecha de cobro no válida (no puede ser futura).");
    }
    c.execute("UPDATE salary_received SET day=?3 WHERE user_id=?1 AND month=?2", params![me.id, m, f.day])?;
    back(&month(&c, Some(&f.month))?, f.tab.as_deref(), "income_saved")
}

#[derive(Deserialize)]
pub struct FinanceF {
    salary: String,
    pay_day: String,
    track: Option<String>,
    pay_since: Option<String>,
    carry: Option<String>,
    carry_since: Option<String>,
    carry_start: Option<String>,
    month: String,
    tab: Option<String>,
}

/// Mis ajustes → Tus finanzas, saved in one go. Options under a switch only apply when it is on.
pub async fn save_finance(State(db): State<Db>, h: HeaderMap, F(f): F<FinanceF>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let now = today(&c)?[..7].to_string();
    let past_month = |m: Option<&str>| m.map(str::trim).filter(|m| valid_month(m) && *m <= now.as_str()).map(str::to_string);
    let Some(salary) = parse_cents(&f.salary).filter(|s| *s >= 0) else { return bad("Ingreso fijo no válido") };
    let pay_day = match f.pay_day.trim() {
        "" => None,
        d => match d.parse::<i64>() {
            Ok(n @ 1..=31) => Some(n),
            _ => return bad("El día de cobro va del 1 al 31, o vacío."),
        },
    };
    c.execute("UPDATE users SET salary=?2, pay_day=?3, track_pay=?4 WHERE id=?1", params![me.id, salary, pay_day, f.track.is_some()])?;
    if f.track.is_some() {
        let Some(since) = past_month(f.pay_since.as_deref()) else { return bad("«Confirmar desde» tiene que ser un mes que no sea futuro.") };
        c.execute("UPDATE users SET pay_since=?2 WHERE id=?1", params![me.id, since])?;
    }
    c.execute("UPDATE users SET carry=?2 WHERE id=?1", params![me.id, f.carry.is_some()])?;
    if f.carry.is_some() {
        let Some(since) = past_month(f.carry_since.as_deref()) else { return bad("«Desde» tiene que ser un mes que no sea futuro.") };
        let start = match f.carry_start.as_deref().map(str::trim).unwrap_or("") {
            "" => Some(0),
            v => parse_cents(v),
        };
        let Some(start) = start else { return bad("Saldo inicial no válido") };
        c.execute("UPDATE users SET carry_since=?2, carry_start=?3 WHERE id=?1", params![me.id, since, start])?;
    }
    back(&month(&c, Some(&f.month))?, f.tab.as_deref(), "finance_saved")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pay_dates() {
        let c = Connection::open_in_memory().unwrap();
        let today = today(&c).unwrap();
        let now = &today[..7];
        let past = shift(now, -4);
        let last = days_in(&c, &past).unwrap();
        // Past month: the usual pay day of that month, clamped to its length.
        assert_eq!(default_day(&c, &past, Some(3)).unwrap(), format!("{past}-03"));
        assert_eq!(default_day(&c, &past, Some(31)).unwrap(), format!("{past}-{last:02}"));
        // Past month without a pay day: its last day.
        assert_eq!(default_day(&c, &past, None).unwrap(), format!("{past}-{last:02}"));
        // This month without a pay day: today. Never a future date.
        assert_eq!(default_day(&c, now, None).unwrap(), today);
        assert!(default_day(&c, now, Some(31)).unwrap() <= today);
        assert!(default_day(&c, &shift(now, 2), None).unwrap() <= today);
        // Validation: real dates only, none in the future.
        assert!(valid_day(&c, &format!("{past}-03")).unwrap());
        assert!(!valid_day(&c, "2026-02-30").unwrap());
        assert!(!valid_day(&c, &format!("{}-01", shift(now, 1))).unwrap());
    }
}

