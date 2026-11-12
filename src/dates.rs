use super::*;

pub const MONTHS: [&str; 12] = [
    "Enero", "Febrero", "Marzo", "Abril", "Mayo", "Junio", "Julio", "Agosto", "Septiembre",
    "Octubre", "Noviembre", "Diciembre",
];
pub const MON: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic"];
pub const WEEKDAYS: [&str; 7] = ["domingo", "lunes", "martes", "miércoles", "jueves", "viernes", "sábado"];

pub fn valid_month(m: &str) -> bool {
    m.len() == 7
        && m.is_ascii()
        && m.as_bytes()[4] == b'-'
        && m[..4].parse::<u32>().is_ok()
        && matches!(m[5..].parse::<u32>(), Ok(1..=12))
}

pub fn shift(m: &str, d: i32) -> String {
    let t = m[..4].parse::<i32>().unwrap() * 12 + m[5..].parse::<i32>().unwrap() - 1 + d;
    format!("{:04}-{:02}", t.div_euclid(12), t.rem_euclid(12) + 1)
}

pub fn today(c: &Connection) -> rusqlite::Result<String> {
    c.query_row("SELECT date('now','localtime')", [], |r| r.get(0))
}

pub fn month(c: &Connection, m: Option<&str>) -> rusqlite::Result<String> {
    match m {
        Some(m) if valid_month(m) => Ok(m.to_string()),
        _ => Ok(today(c)?[..7].to_string()),
    }
}

pub fn days_in(c: &Connection, m: &str) -> rusqlite::Result<i64> {
    c.query_row("SELECT CAST(strftime('%d', ?1 || '-01', '+1 month', '-1 day') AS INTEGER)", [m], |r| r.get(0))
}

pub fn weekday(date: &str) -> usize {
    let (mut y, m, d): (i64, usize, i64) = (date[..4].parse().unwrap_or(2000), date[5..7].parse().unwrap_or(1), date[8..].parse().unwrap_or(1));
    const T: [i64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    if m < 3 {
        y -= 1;
    }
    ((y + y / 4 - y / 100 + y / 400 + T[(m + 11) % 12] + d) % 7) as usize
}

pub fn day_label(date: &str, weekday: usize, today: &str, yesterday: &str) -> String {
    if date == today {
        return "Hoy".into();
    }
    if date == yesterday {
        return "Ayer".into();
    }
    let d: u32 = date[8..].parse().unwrap_or(1);
    let mi: usize = date[5..7].parse().unwrap_or(1);
    format!("{} {d} {}", WEEKDAYS[weekday % 7], MON[(mi + 11) % 12])
}
