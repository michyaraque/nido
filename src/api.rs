use super::*;

#[derive(Deserialize)]
#[serde(untagged)]
pub enum AmountIn {
    Text(String),
    Number(f64),
}

#[derive(Deserialize)]
pub struct CreateExpense {
    amount: AmountIn,
    #[serde(alias = "merchant")]
    description: Option<String>,
}

#[derive(Serialize)]
pub struct ExpenseOut {
    id: i64,
    description: String,
    merchant: Option<&'static str>,
    category: &'static str,
    amount_cents: i64,
    amount_display: String,
    month: String,
}

pub fn api_error(status: StatusCode, msg: &str) -> R {
    Ok((status, Json(serde_json::json!({ "error": msg }))).into_response())
}

pub async fn api_create_expense(State(db): State<Db>, h: HeaderMap, body: Bytes) -> R {
    let c = lock(&db);
    let tok = h
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let uid: Option<i64> = c.query_row("SELECT id FROM users WHERE token=?1", [tok], |r| r.get(0)).optional()?;
    let Some(uid) = uid else { return api_error(StatusCode::UNAUTHORIZED, "invalid_token") };
    let Ok(body) = serde_json::from_str::<CreateExpense>(&to_text(body.to_vec())) else {
        return api_error(StatusCode::BAD_REQUEST, "invalid_json");
    };
    let cents = match body.amount {
        AmountIn::Text(s) => parse_cents(&s),
        AmountIn::Number(n) if n.is_finite() && n.abs() < 1e12 => Some((n * 100.0).round() as i64),
        _ => None,
    };
    let Some(cents) = cents.filter(|c| *c != 0) else { return api_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_amount") };
    let desc = body.description.map(|d| clean(&d)).filter(|d| !d.is_empty()).unwrap_or_else(|| "Gasto".into());
    let m = month(&c, None)?;
    c.execute(
        "INSERT INTO expenses(user_id,month,description,amount,source) VALUES(?1,?2,?3,?4,'shortcut')",
        params![uid, m, desc, cents],
    )?;
    let l = look(&desc, None);
    let out = ExpenseOut {
        id: c.last_insert_rowid(),
        merchant: l.brand,
        category: l.cat.id,
        description: desc,
        amount_cents: cents,
        amount_display: eur(cents),
        month: m,
    };
    Ok((StatusCode::CREATED, Json(out)).into_response())
}
