/// The last separator is decimal only when 1-2 digits follow it, so "1.234" is one thousand two hundred.
pub fn parse_cents(s: &str) -> Option<i64> {
    let s: String = s.chars().filter(|c| c.is_ascii_digit() || matches!(c, ',' | '.' | '-')).collect();
    let neg = s.starts_with('-');
    let s = s.trim_start_matches('-');
    let (int, frac) = match s.rfind([',', '.']) {
        Some(i) if s.len() - i - 1 <= 2 => (&s[..i], &s[i + 1..]),
        _ => (s, ""),
    };
    let int: String = int.chars().filter(|c| c.is_ascii_digit()).collect();
    if int.is_empty() && frac.is_empty() {
        return None;
    }
    let i: i64 = if int.is_empty() { 0 } else { int.parse().ok()? };
    let f: i64 = match frac.len() {
        0 => 0,
        1 => frac.parse::<i64>().ok()? * 10,
        _ => frac.parse().ok()?,
    };
    let v = i.checked_mul(100)?.checked_add(f)?;
    Some(if neg { -v } else { v })
}

pub fn eur_parts(c: i64) -> (String, String) {
    let a = c.unsigned_abs();
    let digits = (a / 100).to_string();
    let mut int = String::from(if c < 0 { "-" } else { "" });
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            int.push('.');
        }
        int.push(ch);
    }
    (int, format!(",{:02}", a % 100))
}

pub fn eur(c: i64) -> String {
    let (a, b) = eur_parts(c);
    format!("{a}{b} €")
}

pub fn eur_input(c: i64) -> String {
    format!("{}{},{:02}", if c < 0 { "-" } else { "" }, c.unsigned_abs() / 100, c.unsigned_abs() % 100)
}
