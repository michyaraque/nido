//! What the core itself can do from a tag: the shopping list, house chores, an expense, or going
//! somewhere in Nido. Same shape as a plugin's actions, without the HTTP.
use super::*;

const RECENT: &str = "datetime('now','localtime','-30 minutes')";

/// Sections a tag can open, besides the active plugins.
const PLACES: [(&str, &str, &str); 4] = [
    ("home", "Resumen", "house"),
    ("expenses", "Gastos", "receipt"),
    ("shopping", "Compra", "shopping-basket"),
    ("tasks", "Tareas", "calendar-check"),
];

pub fn catalog(c: &Connection, on: &[&'static plugins::Known]) -> rusqlite::Result<Vec<Action>> {
    let a = |id: String, group: &str, name: String, icon: &str, ask: Option<&str>| Action {
        source: "core",
        id,
        group: group.into(),
        name,
        icon: known_icon(icon),
        confirm: false,
        ask: ask.map(Into::into),
    };
    let mut out = vec![
        a("shop".into(), "Compra", "Añadir a la lista".into(), "shopping-basket", Some("Qué añadir, como Leche")),
        a("expense".into(), "Gastos", "Apuntar un gasto".into(), "receipt", Some("Importe y concepto, como 1,20 Pan")),
    ];
    let chores: Vec<(i64, String, String)> = c
        .prepare("SELECT id,name,icon FROM chores WHERE archived=0 ORDER BY name")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    for (id, name, icon) in chores {
        out.push(a(format!("chore:{id}"), "Tareas de casa", format!("Hecho: {name}"), &icon, None));
    }
    for (tab, label, icon) in PLACES {
        out.push(a(format!("open:{tab}"), "Abrir en Nido", format!("Abrir {label}"), icon, None));
    }
    for k in on {
        out.push(a(format!("open:{}", k.id), "Abrir en Nido", format!("Abrir {}", k.name), k.icon, None));
    }
    Ok(out)
}

fn name_of(c: &Connection, id: i64) -> rusqlite::Result<String> {
    Ok(c.query_row("SELECT name FROM users WHERE id=?1", [id], |r| r.get(0)).optional()?.unwrap_or_else(|| "Alguien".into()))
}

/// "1,20 Pan" or "Pan 1,20": the amount may come first or last.
pub fn amount_and_concept(s: &str) -> Option<(i64, String)> {
    let words: Vec<&str> = s.split_whitespace().collect();
    let (first, last) = (words.first()?, words.last()?);
    let (cents, rest) = match parse_cents(first) {
        Some(v) => (v, &words[1..]),
        None => (parse_cents(last)?, &words[..words.len() - 1]),
    };
    let concept = clean(&rest.join(" "));
    (cents > 0).then(|| (cents, if concept.is_empty() { "Gasto".into() } else { concept }))
}

pub fn run(c: &Connection, me: i64, action: &str, arg: &str, force: bool) -> rusqlite::Result<Outcome> {
    let (kind, rest) = action.split_once(':').unwrap_or((action, ""));
    Ok(match kind {
        "shop" => {
            let item = clean(arg);
            if item.is_empty() {
                return Ok(Outcome::failed("Este atajo no dice qué añadir. Escríbelo en Ajustes → Atajos."));
            }
            if add_to_list(c, &item, me)? {
                Outcome::done(format!("{item}, a la lista de la compra")).undo(format!("s{}", c.last_insert_rowid())).href("/?tab=shopping")
            } else {
                Outcome::done(format!("{item} ya estaba en la lista")).href("/?tab=shopping")
            }
        }
        "chore" => {
            let id: i64 = rest.parse().unwrap_or(0);
            let Some(name): Option<String> = c.query_row("SELECT name FROM chores WHERE id=?1 AND archived=0", [id], |r| r.get(0)).optional()? else {
                return Ok(Outcome::failed("Esa tarea ya no está. Cambia este atajo en Ajustes → Atajos."));
            };
            let day = today(c)?;
            let by: Option<i64> = c.query_row("SELECT user_id FROM chore_logs WHERE chore_id=?1 AND day=?2 ORDER BY id DESC LIMIT 1", params![id, day], |r| r.get(0)).optional()?;
            match by {
                Some(u) if !force && u == me => Outcome::already(format!("Ya apuntaste «{name}» hoy")),
                Some(u) if !force => Outcome::already(format!("{} ya hizo «{name}» hoy", name_of(c, u)?)),
                _ => {
                    c.execute("INSERT INTO chore_logs(chore_id,user_id,day) VALUES(?1,?2,?3)", params![id, me, day])?;
                    Outcome::done(format!("«{name}», hecho hoy")).undo(format!("c{}", c.last_insert_rowid())).href("/?tab=tasks")
                }
            }
        }
        "expense" => {
            let Some((cents, concept)) = amount_and_concept(arg) else {
                return Ok(Outcome::failed("Este atajo no tiene un importe válido. Escríbelo en Ajustes → Atajos, como 1,20 Pan."));
            };
            let m = month(c, None)?;
            c.execute("INSERT INTO expenses(user_id,month,description,amount,source) VALUES(?1,?2,?3,?4,'tag')", params![me, m, concept, cents])?;
            Outcome::done(format!("{} · {concept}, en tus gastos", eur(cents).replace(' ', "\u{a0}"))).undo(format!("e{}", c.last_insert_rowid())).href("/?tab=expenses")
        }
        "open" => Outcome::open(format!("/?tab={}", tab(Some(rest)))),
        _ => Outcome::failed("Esta acción ya no existe. Elige otra en Ajustes → Atajos."),
    })
}

/// Only my own entry, only while it is recent: an old link cannot erase what someone else wrote.
pub fn undo(c: &Connection, me: i64, token: &str) -> rusqlite::Result<bool> {
    let Some((kind, id)) = token.split_at_checked(1) else { return Ok(false) };
    let Ok(id) = id.parse::<i64>() else { return Ok(false) };
    let sql = match kind {
        "s" => format!("DELETE FROM shopping WHERE id=?1 AND added_by=?2 AND done=0 AND cleared=0 AND created>={RECENT}"),
        "c" => format!("DELETE FROM chore_logs WHERE id=?1 AND user_id=?2 AND created>={RECENT}"),
        "e" => format!("DELETE FROM expenses WHERE id=?1 AND user_id=?2 AND created>={RECENT} AND NOT EXISTS (SELECT 1 FROM tickets WHERE expense_id=expenses.id)"),
        _ => return Ok(false),
    };
    Ok(c.execute(&sql, params![id, me])? > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts() {
        assert_eq!(amount_and_concept("1,20 Pan de leña"), Some((120, "Pan de leña".into())));
        assert_eq!(amount_and_concept("Café 1.5"), Some((150, "Café".into())));
        assert_eq!(amount_and_concept("3"), Some((300, "Gasto".into())));
        assert_eq!(amount_and_concept("Pan"), None);
        assert_eq!(amount_and_concept(""), None);
        assert_eq!(amount_and_concept("-2 Pan"), None);
    }

    #[test]
    fn runs_and_takes_back() {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c);
        c.execute("INSERT INTO users(id,name,pass,token) VALUES(1,'Ana','x','a'),(2,'Leo','x','b')", []).unwrap();
        c.execute("INSERT INTO chores(id,name,icon,color) VALUES(7,'Fregar','sparkles','#2f6bff')", []).unwrap();

        let o = run(&c, 1, "shop", "leche", false).unwrap();
        assert_eq!((o.status, o.text.as_str()), (Status::Done, "leche, a la lista de la compra"));
        assert_eq!(run(&c, 2, "shop", "Leche", false).unwrap().text, "Leche ya estaba en la lista");
        assert!(!undo(&c, 2, o.undo.as_deref().unwrap()).unwrap(), "not Leo's");
        assert!(undo(&c, 1, o.undo.as_deref().unwrap()).unwrap());

        let o = run(&c, 1, "chore:7", "", false).unwrap();
        assert_eq!(o.status, Status::Done);
        let again = run(&c, 2, "chore:7", "", false).unwrap();
        assert_eq!((again.status, again.text.as_str()), (Status::Already, "Ana ya hizo «Fregar» hoy"));
        assert_eq!(run(&c, 1, "chore:7", "", false).unwrap().text, "Ya apuntaste «Fregar» hoy");
        assert_eq!(run(&c, 2, "chore:7", "", true).unwrap().status, Status::Done);
        assert_eq!(run(&c, 1, "chore:99", "", false).unwrap().status, Status::Failed);

        let o = run(&c, 1, "expense", "1,20 Pan", false).unwrap();
        assert_eq!(o.text, "1,20\u{a0}€ · Pan, en tus gastos");
        assert!(undo(&c, 1, o.undo.as_deref().unwrap()).unwrap());
        assert_eq!(run(&c, 1, "expense", "Pan", false).unwrap().status, Status::Failed);

        let o = run(&c, 1, "open:tasks", "", false).unwrap();
        assert_eq!((o.status, o.href.as_deref()), (Status::Open, Some("/?tab=tasks")));
        assert_eq!(run(&c, 1, "open:<x>", "", false).unwrap().href.as_deref(), Some("/?tab=home"));
        assert!(!undo(&c, 1, "x1").unwrap() && !undo(&c, 1, "").unwrap() && !undo(&c, 1, "sabc").unwrap());
    }
}
