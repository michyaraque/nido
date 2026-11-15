use super::*;

pub type Db = Arc<Mutex<Connection>>;

const SCHEMA: &str = include_str!("schema.sql");

pub fn data_dir() -> std::path::PathBuf {
    let db = std::env::var("NIDO_DB").unwrap_or_else(|_| "nido.db".into());
    std::path::Path::new(&db).parent().map(|p| p.to_path_buf()).filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| ".".into())
}

pub fn lock(db: &Db) -> MutexGuard<'_, Connection> {
    db.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn migrate(c: &Connection) {
    c.execute_batch(SCHEMA).expect("schema");
    // Columns added after the first release (CREATE TABLE IF NOT EXISTS won't add them).
    add_column(c, "expenses", "category", "TEXT");
    add_column(c, "expenses", "note", "TEXT");
    add_column(c, "users", "photo", "INTEGER NOT NULL DEFAULT 0");
    add_column(c, "bills", "covered_by", "INTEGER REFERENCES users(id)");
    add_column(c, "payments", "whole", "INTEGER NOT NULL DEFAULT 0");
    add_column(c, "users", "tg_chat", "INTEGER");
    add_column(c, "users", "tg_code", "TEXT");
    add_column(c, "users", "color", "TEXT");
    add_column(c, "users", "pay_since", "TEXT"); // month the person started confirming their fixed income
    add_column(c, "users", "track_pay", "INTEGER NOT NULL DEFAULT 0");
    add_column(c, "users", "share", "INTEGER NOT NULL DEFAULT 0");
    add_column(c, "users", "pay_day", "INTEGER"); // usual day the fixed income arrives (optional)
    add_column(c, "users", "carry", "INTEGER NOT NULL DEFAULT 0"); // add up what is left from month to month
    add_column(c, "users", "carry_since", "TEXT");
    add_column(c, "users", "carry_start", "INTEGER NOT NULL DEFAULT 0"); // cents I had when carrying started // % of shared bills, for the "custom" split
    hash_sessions(c);
}

fn hash_sessions(c: &Connection) {
    let done: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM settings WHERE key='sessions_hashed')", [], |r| r.get(0)).expect("schema");
    if done {
        return;
    }
    let tokens: Vec<String> = c
        .prepare("SELECT token FROM sessions")
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
        .expect("sessions");
    for t in tokens {
        c.execute("UPDATE sessions SET token=?2 WHERE token=?1", params![t, auth::token_hash(&t)]).expect("migration");
    }
    c.execute("INSERT INTO settings(key,value) VALUES('sessions_hashed','1')", []).expect("migration");
}

fn add_column(c: &Connection, table: &str, column: &str, decl: &str) {
    let exists: bool = c
        .query_row(&format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name=?1)"), [column], |r| r.get(0))
        .expect("schema");
    if !exists {
        c.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"), []).expect("migration");
    }
}
