use super::*;
use axum::extract::Multipart;
use std::path::PathBuf;

fn photo_path(id: i64) -> PathBuf {
    data_dir().join("tickets").join(format!("{id}.webp"))
}

fn read_ticket(db: Db, id: i64, photo: Vec<u8>) {
    let result = (|| -> Result<(receipt::Receipt, String), String> {
        let (img, stored) = ocr::normalize(&photo)?;
        let path = photo_path(id);
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&path, stored).map_err(|e| e.to_string())?;
        let lines = ocr::read_lines(&img)?;
        Ok((receipt::parse(&lines), lines.join("\n")))
    })();
    let c = lock(&db);
    let saved = match result {
        Ok((r, text)) => c.execute(
            "UPDATE tickets SET status='ready', merchant=?2, date=?3, total=?4, items=?5, text=?6 WHERE id=?1",
            params![id, r.merchant, r.date, r.total, serde_json::to_string(&r.items).unwrap_or_default(), text],
        ),
        Err(e) => c.execute("UPDATE tickets SET status='failed', error=?2 WHERE id=?1", params![id, e]),
    };
    if let Err(e) = saved {
        eprintln!("ticket {id}: {e}");
    }
}

pub async fn upload_ticket(State(db): State<Db>, h: HeaderMap, mut form: Multipart) -> R {
    let me = {
        let c = lock(&db);
        require_user!(c, h)
    };
    let mut photo = Vec::new();
    while let Ok(Some(field)) = form.next_field().await {
        if field.name() == Some("photo") {
            photo = field.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
        }
    }
    if photo.is_empty() {
        return bad("Elige una foto del ticket.");
    }
    let id = {
        let c = lock(&db);
        c.execute("INSERT INTO tickets(user_id) VALUES(?1)", [me.id])?;
        c.last_insert_rowid()
    };
    let db = db.clone();
    tokio::task::spawn_blocking(move || read_ticket(db, id, photo));
    Ok(Redirect::to(&format!("/tickets/{id}")).into_response())
}

pub async fn ticket_photo(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>) -> R {
    {
        let c = lock(&db);
        require_user!(c, h);
    }
    // Receipts read before photos were stored as WebP are still JPEG files.
    let found = std::fs::read(photo_path(id)).map(|b| (b, "image/webp")).or_else(|_| std::fs::read(photo_path(id).with_extension("jpg")).map(|b| (b, "image/jpeg")));
    match found {
        Ok((b, mime)) => Ok(([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "private, max-age=86400")], b).into_response()),
        Err(_) => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

pub async fn save_ticket(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(form): F<Vec<(String, String)>>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    // Anyone at home can look at a receipt, but only whoever uploaded it saves it (once).
    let mine: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM tickets WHERE id=?1 AND user_id=?2 AND status!='done')", params![id, me.id], |r| r.get(0))?;
    if !mine {
        return bad("Este ticket no es tuyo o ya está guardado.");
    }
    let field = |k: &str| form.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str()).unwrap_or("");
    let desc = clean(field("description"));
    let Some(amount) = parse_cents(field("amount")).filter(|a| *a != 0) else { return bad("Escribe el importe del ticket.") };
    if desc.is_empty() {
        return bad("Escribe dónde fue la compra.");
    }
    let date: String = c.query_row("SELECT date(?1)", [field("date")], |r| r.get::<_, Option<String>>(0))?.unwrap_or(today(&c)?);
    let cat = CATS.iter().find(|c| c.id == field("category")).map(|c| c.id);
    let note = Some(field("note").trim().chars().take(300).collect::<String>()).filter(|n| !n.is_empty());
    let names = form.iter().filter(|(k, _)| k == "item_name").map(|(_, v)| clean(v));
    let prices = form.iter().filter(|(k, _)| k == "item_price").map(|(_, v)| parse_cents(v));
    let items: Vec<(String, i64)> = names.zip(prices).filter_map(|(n, p)| Some((n, p?))).filter(|(n, _)| !n.is_empty()).take(100).collect();

    c.execute(
        "INSERT INTO expenses(user_id,month,description,amount,category,note,source,created)
         VALUES(?1,?2,?3,?4,?5,?6,'ticket',?7 || ' ' || time('now','localtime'))",
        params![me.id, &date[..7], desc, amount, cat, note, date],
    )?;
    c.execute(
        "UPDATE tickets SET status='done', expense_id=?2, merchant=?3, total=?4, date=?5, items=?6 WHERE id=?1 AND user_id=?7",
        params![id, c.last_insert_rowid(), desc, amount, date, serde_json::to_string(&items).unwrap_or_default(), me.id],
    )?;
    back(&date[..7], Some("expenses"), "ticket_saved")
}

pub async fn delete_ticket(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    if c.execute("DELETE FROM tickets WHERE id=?1 AND user_id=?2", params![id, me.id])? > 0 {
        let _ = std::fs::remove_file(photo_path(id));
        let _ = std::fs::remove_file(photo_path(id).with_extension("jpg"));
    }
    back(&month(&c, None)?, Some("expenses"), "ticket_deleted")
}
