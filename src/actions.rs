use super::*;

#[derive(Deserialize)]
pub struct Ctx {
    pub month: String,
    pub tab: Option<String>,
}

#[derive(Deserialize)]
pub struct BillF {
    name: String,
    amount: String,
    due_day: i64,
    mode: Option<String>, // "split" | "personal" | "user:<id>"
    month: String,
    tab: Option<String>,
}

pub fn bill_fields(f: &BillF) -> Result<(String, i64), &'static str> {
    let name = clean(&f.name);
    let amount = parse_cents(&f.amount).filter(|a| *a > 0).ok_or("Importe no válido")?;
    if name.is_empty() || !(1..=31).contains(&f.due_day) {
        return Err("Datos no válidos");
    }
    Ok((name, amount))
}

fn payer_mode(c: &Connection, mode: Option<&str>, me: i64) -> rusqlite::Result<(Option<i64>, Option<i64>)> {
    Ok(match mode {
        Some("personal") => (Some(me), None),
        Some(m) if m.starts_with("user:") => {
            let id: i64 = m[5..].parse().unwrap_or(0);
            let exists: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM users WHERE id=?1)", [id], |r| r.get(0))?;
            (None, exists.then_some(id))
        }
        _ => (None, None),
    })
}

pub async fn add_bill(State(db): State<Db>, h: HeaderMap, F(f): F<BillF>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let m = month(&c, Some(&f.month))?;
    let (name, amount) = match bill_fields(&f) {
        Ok(v) => v,
        Err(e) => return bad(e),
    };
    let (owner, covered_by) = payer_mode(&c, f.mode.as_deref(), me.id)?;
    c.execute(
        "INSERT INTO bills(name,amount,due_day,owner_id,covered_by,start_month) VALUES(?1,?2,?3,?4,?5,?6)",
        params![name, amount, f.due_day, owner, covered_by, m],
    )?;
    back(&m, f.tab.as_deref(), "bill_added")
}

pub async fn update_bill(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<BillF>) -> R {
    let mut c = lock(&db);
    let me = require_user!(c, h);
    let m = month(&c, Some(&f.month))?;
    let (name, amount) = match bill_fields(&f) {
        Ok(v) => v,
        Err(e) => return bad(e),
    };
    let (owner, covered_by) = payer_mode(&c, f.mode.as_deref(), me.id)?;
    let tx = c.transaction()?;
    let old: Option<(String, Option<String>)> = tx
        .query_row(
            "SELECT start_month,end_month FROM bills WHERE id=?1 AND (owner_id IS NULL OR owner_id=?2)",
            params![id, me.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((start, end)) = old else { return bad("Cuenta no encontrada") };
    let target = if start >= m {
        tx.execute(
            "UPDATE bills SET name=?1,amount=?2,due_day=?3,owner_id=?4,covered_by=?5 WHERE id=?6",
            params![name, amount, f.due_day, owner, covered_by, id],
        )?;
        id
    } else {
        tx.execute(
            "INSERT INTO bills(name,amount,due_day,owner_id,covered_by,start_month,end_month) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![name, amount, f.due_day, owner, covered_by, m, end],
        )?;
        let new_id = tx.last_insert_rowid();
        tx.execute("UPDATE bills SET end_month=?1 WHERE id=?2", params![m, id])?;
        tx.execute("UPDATE payments SET bill_id=?1 WHERE bill_id=?2 AND month>=?3", params![new_id, id, m])?;
        tx.execute("UPDATE bill_amounts SET bill_id=?1 WHERE bill_id=?2 AND month>=?3", params![new_id, id, m])?;
        new_id
    };
    tx.execute("DELETE FROM bill_amounts WHERE bill_id=?1 AND month=?2", params![target, m])?;
    tx.commit()?;
    back(&m, f.tab.as_deref(), "bill_updated")
}

#[derive(Deserialize)]
pub struct AmountF {
    amount: String,
    month: String,
    tab: Option<String>,
}

pub async fn set_bill_amount(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<AmountF>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let m = month(&c, Some(&f.month))?;
    let Some(amount) = parse_cents(&f.amount).filter(|a| *a > 0) else { return bad("Importe no válido") };
    c.execute(
        "INSERT INTO bill_amounts(bill_id,month,amount)
         SELECT id,?2,?3 FROM bills WHERE id=?1 AND (owner_id IS NULL OR owner_id=?4)
         ON CONFLICT(bill_id,month) DO UPDATE SET amount=excluded.amount",
        params![id, m, amount, me.id],
    )?;
    c.execute(
        "DELETE FROM bill_amounts WHERE bill_id=?1 AND month=?2 AND amount=(SELECT amount FROM bills WHERE id=?1)",
        params![id, m],
    )?;
    back(&m, f.tab.as_deref(), "bill_updated")
}

pub async fn end_bill(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Ctx>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let m = month(&c, Some(&f.month))?;
    c.execute(
        "UPDATE bills SET end_month=?1 WHERE id=?2 AND (owner_id IS NULL OR owner_id=?3)",
        params![m, id, me.id],
    )?;
    back(&m, f.tab.as_deref(), "bill_ended")
}

pub async fn toggle_payment(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Ctx>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let m = month(&c, Some(&f.month))?;
    if c.execute("DELETE FROM payments WHERE bill_id=?1 AND month=?2", params![id, m])? == 0 {
        c.execute(
            "INSERT INTO payments(bill_id,month,payer_id)
             SELECT id,?2,?3 FROM bills WHERE id=?1 AND (owner_id IS NULL OR owner_id=?3)",
            params![id, m, me.id],
        )?;
    }
    back(&m, f.tab.as_deref(), "paid")
}

#[derive(Deserialize)]
pub struct ExpF {
    description: String,
    amount: String,
    date: Option<String>,
    category: Option<String>,
    note: Option<String>,
    month: Option<String>,
    tab: Option<String>,
}

fn expense_fields(c: &Connection, f: &ExpF) -> Result<Result<(String, i64, String, Option<&'static str>, Option<String>), &'static str>, rusqlite::Error> {
    let desc = clean(&f.description);
    let Some(amount) = parse_cents(&f.amount).filter(|a| *a != 0) else { return Ok(Err("Importe no válido")) };
    if desc.is_empty() {
        return Ok(Err("Falta el concepto"));
    }
    let date: String = match f.date.as_deref().filter(|d| d.len() == 10) {
        Some(d) => c.query_row("SELECT date(?1)", [d], |r| r.get::<_, Option<String>>(0))?.unwrap_or(today(c)?),
        None => today(c)?,
    };
    let cat = f.category.as_deref().and_then(|id| CATS.iter().find(|c| c.id == id)).map(|c| c.id);
    let note = f.note.as_deref().map(|n| n.trim().chars().take(300).collect::<String>()).filter(|n| !n.is_empty());
    Ok(Ok((desc, amount, date, cat, note)))
}

pub async fn add_expense(State(db): State<Db>, h: HeaderMap, F(f): F<ExpF>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let (desc, amount, date, cat, note) = match expense_fields(&c, &f)? {
        Ok(v) => v,
        Err(e) => return bad(e),
    };
    c.execute(
        "INSERT INTO expenses(user_id,month,description,amount,category,note,created)
         VALUES(?1,?2,?3,?4,?5,?6,?7 || ' ' || time('now','localtime'))",
        params![me.id, &date[..7], desc, amount, cat, note, date],
    )?;
    back(&date[..7], f.tab.as_deref(), "expense_added")
}

pub async fn update_expense(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<ExpF>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let (desc, amount, date, cat, note) = match expense_fields(&c, &f)? {
        Ok(v) => v,
        Err(e) => return bad(e),
    };
    c.execute(
        "UPDATE expenses SET description=?3, amount=?4, category=?5, note=?6, month=?7, created=?8 || ' ' || time(created)
         WHERE id=?1 AND user_id=?2",
        params![id, me.id, desc, amount, cat, note, &date[..7], date],
    )?;
    back(&month(&c, f.month.as_deref())?, f.tab.as_deref(), "expense_updated")
}

pub async fn delete_expense(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Ctx>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let m = month(&c, Some(&f.month))?;
    c.execute("DELETE FROM expenses WHERE id=?1 AND user_id=?2", params![id, me.id])?;
    back(&m, f.tab.as_deref(), "expense_deleted")
}

#[derive(Deserialize)]
pub struct SalaryF {
    salary: String,
    month: String,
    tab: Option<String>,
}

pub async fn set_salary(State(db): State<Db>, h: HeaderMap, F(f): F<SalaryF>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let m = month(&c, Some(&f.month))?;
    let Some(s) = parse_cents(&f.salary).filter(|s| *s >= 0) else { return bad("Sueldo no válido") };
    c.execute("UPDATE users SET salary=?1 WHERE id=?2", params![s, me.id])?;
    back(&m, f.tab.as_deref(), "salary")
}

#[derive(Deserialize)]
pub struct SettleF {
    other_id: i64,
    amount: i64, // cents, signed from my point of view: > 0 the other pays me
    month: String,
    tab: Option<String>,
}

pub async fn add_settlement(State(db): State<Db>, h: HeaderMap, F(f): F<SettleF>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let m = month(&c, Some(&f.month))?;
    let exists: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM users WHERE id=?1)", [f.other_id], |r| r.get(0))?;
    if !exists || f.other_id == me.id || f.amount == 0 {
        return bad("Datos no válidos");
    }
    let (from, to) = if f.amount > 0 { (f.other_id, me.id) } else { (me.id, f.other_id) };
    c.execute(
        "INSERT INTO settlements(month,from_id,to_id,amount) VALUES(?1,?2,?3,?4)",
        params![m, from, to, f.amount.abs()],
    )?;
    back(&m, f.tab.as_deref(), "settled")
}

pub async fn delete_settlement(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Ctx>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let m = month(&c, Some(&f.month))?;
    c.execute("DELETE FROM settlements WHERE id=?1 AND (from_id=?2 OR to_id=?2)", params![id, me.id])?;
    back(&m, f.tab.as_deref(), "unsettled")
}

#[derive(Deserialize)]
pub struct ItemsF {
    names: String,
    month: String,
}

pub async fn add_items(State(db): State<Db>, h: HeaderMap, F(f): F<ItemsF>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let m = month(&c, Some(&f.month))?;
    for name in f.names.split([',', ';', '\n']).map(clean).filter(|n| !n.is_empty()).take(30) {
        let mut chars = name.chars();
        let name: String = chars.next().map(|f| f.to_uppercase().chain(chars).collect()).unwrap_or_default();
        c.execute(
            "INSERT INTO shopping(name,added_by) SELECT ?1,?2
             WHERE NOT EXISTS (SELECT 1 FROM shopping WHERE cleared=0 AND done=0 AND lower(name)=lower(?1))",
            params![name, me.id],
        )?;
    }
    back(&m, Some("shopping"), "items_added")
}

pub async fn toggle_item(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Ctx>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    c.execute("UPDATE shopping SET done=1-done WHERE id=?1", [id])?;
    back(&month(&c, Some(&f.month))?, Some("shopping"), "")
}

pub async fn delete_item(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Ctx>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    c.execute("DELETE FROM shopping WHERE id=?1", [id])?;
    back(&month(&c, Some(&f.month))?, Some("shopping"), "item_deleted")
}

pub async fn clear_items(State(db): State<Db>, h: HeaderMap, F(f): F<Ctx>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    c.execute("UPDATE shopping SET cleared=1 WHERE done=1 AND cleared=0", [])?;
    back(&month(&c, Some(&f.month))?, Some("shopping"), "list_cleared")
}

#[derive(Deserialize)]
pub struct HouseF {
    share_expenses: Option<String>,
    month: String,
}

pub async fn save_house(State(db): State<Db>, h: HeaderMap, F(f): F<HouseF>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    let value = if f.share_expenses.is_some() { "1" } else { "0" };
    c.execute(
        "INSERT INTO settings(key,value) VALUES('share_expenses',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [value],
    )?;
    back(&month(&c, Some(&f.month))?, Some("household"), "house_saved")
}

#[derive(Deserialize)]
pub struct ThemeF {
    theme: String,
    month: String,
}

pub async fn save_theme(State(db): State<Db>, h: HeaderMap, F(f): F<ThemeF>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    c.execute(
        "INSERT INTO settings(key,value) VALUES('theme',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [theme_id(&f.theme)],
    )?;
    back(&month(&c, Some(&f.month))?, Some("household"), "house_saved")
}

#[derive(Deserialize)]
pub struct MemberF {
    name: String,
    password: String,
    color: Option<String>,
    month: String,
}

/// New person in the house, with their own login. Anyone in the house can add one.
pub async fn add_member(State(db): State<Db>, h: HeaderMap, F(f): F<MemberF>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    let name = f.name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return bad("Escribe un nombre (hasta 40 letras).");
    }
    if f.password.chars().count() < 4 {
        return bad("La contraseña necesita al menos 4 caracteres.");
    }
    let taken: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM users WHERE lower(name)=lower(?1))", [name], |r| r.get(0))?;
    if taken {
        return bad("Ya hay alguien con ese nombre en casa.");
    }
    let color = f.color.as_deref().filter(|c| PALETTE.contains(c));
    c.execute("INSERT INTO users(name,pass,token,color) VALUES(?1,?2,?3,?4)", params![name, hash(&f.password), rand_hex(24), color])?;
    back(&month(&c, Some(&f.month))?, Some("household"), "member_added")
}

#[derive(Deserialize)]
pub struct ColorF {
    color: String,
    month: String,
    tab: Option<String>,
}

pub async fn set_color(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<ColorF>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    if !PALETTE.contains(&f.color.as_str()) {
        return bad("Color no válido");
    }
    c.execute("UPDATE users SET color=?2 WHERE id=?1", params![id, f.color])?;
    back(&month(&c, Some(&f.month))?, Some(nav::tab(f.tab.as_deref())), "color_saved")
}

pub async fn toggle_whole(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Ctx>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let m = month(&c, Some(&f.month))?;
    let mine_whole: bool = c
        .query_row("SELECT EXISTS(SELECT 1 FROM payments WHERE bill_id=?1 AND month=?2 AND payer_id=?3 AND whole=1)", params![id, m, me.id], |r| r.get(0))?;
    if mine_whole {
        c.execute("UPDATE payments SET whole=0 WHERE bill_id=?1 AND month=?2", params![id, m])?;
    } else {
        c.execute(
            "INSERT INTO payments(bill_id,month,payer_id,whole) SELECT id,?2,?3,1 FROM bills WHERE id=?1 AND owner_id IS NULL
             ON CONFLICT(bill_id,month) DO UPDATE SET payer_id=excluded.payer_id, whole=1",
            params![id, m, me.id],
        )?;
    }
    back(&m, f.tab.as_deref(), if mine_whole { "bill_split_again" } else { "bill_paid_whole" })
}
