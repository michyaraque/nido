//! Dates travel as text ("YYYY-MM-DD", "HH:MM") in the server's local time, as SQLite gives them, so the
//! plugin needs no date crate.
use maud::{html, Markup, PreEscaped};
use std::cell::RefCell;

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

thread_local! {
    static USED: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// A reference to the icon's symbol: a page repeats the same few icons dozens of times, so each
/// one's paths travel once, in `icon_sprite`.
pub fn icon(name: &str) -> Markup {
    let Some(i) = ICONS.iter().position(|(n, _)| *n == name) else { return html! {} };
    USED.with(|u| {
        let mut u = u.borrow_mut();
        if !u.contains(&i) {
            u.push(i);
        }
    });
    PreEscaped(format!(r##"<svg class="i pt-i" aria-hidden="true"><use href="#pt-i-{name}"/></svg>"##))
}

/// Starts a page: forgets the icons of the previous one (a failed render may have left some).
pub fn reset_icons() {
    USED.with(|u| u.borrow_mut().clear());
}

/// The symbols of every icon used since `reset_icons`.
pub fn icon_sprite() -> Markup {
    let used = USED.with(|u| std::mem::take(&mut *u.borrow_mut()));
    let mut s = String::from(r#"<svg width="0" height="0" style="position:absolute" aria-hidden="true">"#);
    for i in used {
        let (name, inner) = ICONS[i];
        s += &format!(r#"<symbol id="pt-i-{name}" viewBox="0 0 24 24">{inner}</symbol>"#);
    }
    PreEscaped(s + "</svg>")
}

pub fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn clean(s: &str, max: usize) -> String {
    s.trim().chars().filter(|c| !c.is_control()).take(max).collect()
}

pub fn note(s: &str) -> String {
    s.trim().chars().filter(|c| *c == '\n' || !c.is_control()).take(1000).collect()
}

pub fn initial(name: &str) -> String {
    name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into())
}

/// Days since 1970-01-01 for "YYYY-MM-DD" (also accepts a longer "YYYY-MM-DD HH:MM").
pub fn days(ymd: &str) -> Option<i64> {
    let s = ymd.get(..10)?;
    let (y, m, d) = (s.get(..4)?.parse::<i64>().ok()?, s.get(5..7)?.parse::<i64>().ok()?, s.get(8..10)?.parse::<i64>().ok()?);
    if s.as_bytes()[4] != b'-' || s.as_bytes()[7] != b'-' || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

pub fn ymd(z: i64) -> String {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn add_days(day: &str, n: i64) -> String {
    days(day).map(|d| ymd(d + n)).unwrap_or_else(|| day.to_string())
}

const WEEK: [&str; 7] = ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"];
const MONTHS: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic"];

/// "Hoy", "Mañana", "Ayer", or "jue 21 nov" (with the year when it is not this one).
pub fn day_label(day: &str, today: &str) -> String {
    let (Some(d), Some(t)) = (days(day), days(today)) else { return day.to_string() };
    match d - t {
        0 => "Hoy".into(),
        1 => "Mañana".into(),
        -1 => "Ayer".into(),
        _ => {
            let s = ymd(d);
            let wd = WEEK[(d + 3).rem_euclid(7) as usize];
            let m = MONTHS[s[5..7].parse::<usize>().unwrap_or(1) - 1];
            let dd = s[8..10].trim_start_matches('0');
            if s[..4] == today[..4] {
                format!("{wd} {dd} {m}")
            } else {
                format!("{dd} {m} {}", &s[..4])
            }
        }
    }
}

pub fn long_day(day: &str) -> String {
    const NAMES: [&str; 7] = ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"];
    const FULL: [&str; 12] = ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];
    let Some(d) = days(day) else { return day.to_string() };
    let m = FULL[day[5..7].parse::<usize>().unwrap_or(1) - 1];
    format!("{}, {} de {m}", NAMES[(d + 3).rem_euclid(7) as usize], day[8..10].trim_start_matches('0'))
}

pub fn rel(day: &str, today: &str) -> String {
    let (Some(d), Some(t)) = (days(day), days(today)) else { return String::new() };
    let n = d - t;
    let span = |n: i64| match n {
        1 => "1 día".to_string(),
        2..=44 => format!("{n} días"),
        45..=364 => format!("{} meses", (n + 15) / 30),
        _ => format!("{} años", (n + 182) / 365).replace("1 años", "1 año"),
    };
    match n {
        0 => "hoy".into(),
        1 => "mañana".into(),
        -1 => "ayer".into(),
        n if n > 0 => format!("en {}", span(n)),
        n => format!("hace {}", span(-n)),
    }
}

pub fn age(birth: &str, today: &str) -> Option<String> {
    let (b, t) = (days(birth)?, days(today)?);
    if b > t {
        return None;
    }
    let months = (today[..4].parse::<i64>().ok()? - birth[..4].parse::<i64>().ok()?) * 12
        + today[5..7].parse::<i64>().ok()?
        - birth[5..7].parse::<i64>().ok()?
        - i64::from(today[8..10] < birth[8..10]);
    Some(match months {
        m if m >= 24 => format!("{} años", m / 12),
        m if m >= 12 => "1 año".into(),
        m if m >= 2 => format!("{m} meses"),
        _ => match (t - b) / 7 {
            0 | 1 => format!("{} días", t - b),
            w => format!("{w} semanas"),
        },
    })
}

pub fn hhmm(s: &str) -> Option<String> {
    let s = s.trim();
    let (h, m) = s.split_once(':')?;
    let (h, m) = (h.parse::<u8>().ok()?, m.get(..2)?.parse::<u8>().ok()?);
    (h < 24 && m < 60).then(|| format!("{h:02}:{m:02}"))
}

pub fn kilos(s: &str) -> Option<f64> {
    let v: f64 = s.trim().trim_end_matches("kg").trim().replace(',', ".").parse().ok()?;
    (v > 0.0 && v < 500.0).then_some((v * 1000.0).round() / 1000.0)
}

/// "4,2 kg", "350 g", with a no-break space so the number never parts from its unit.
pub fn kg(v: f64) -> String {
    if v < 1.0 {
        format!("{}\u{a0}g", (v * 1000.0).round())
    } else {
        let s = format!("{:.2}", v);
        let s = s.trim_end_matches('0').trim_end_matches('.');
        format!("{}\u{a0}kg", s.replace('.', ","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar() {
        assert_eq!(days("1970-01-01"), Some(0));
        assert_eq!(ymd(days("2024-02-29").unwrap() + 1), "2024-03-01");
        assert_eq!(add_days("2026-12-31", 1), "2027-01-01");
        assert_eq!(day_label("2026-11-19", "2026-11-16"), "jue 19 nov");
        assert_eq!(day_label("2026-11-17", "2026-11-16"), "Mañana");
        assert_eq!(rel("2026-11-21", "2026-11-16"), "en 5 días");
        assert_eq!(rel("2026-09-16", "2026-11-16"), "hace 2 meses");
        assert_eq!(age("2023-11-20", "2026-11-16").as_deref(), Some("2 años"));
        assert_eq!(age("2026-10-01", "2026-11-16").as_deref(), Some("6 semanas"));
        assert_eq!(days("2026-13-01"), None);
        assert_eq!(long_day("2026-11-16"), "lunes, 16 de noviembre");
    }

    #[test]
    fn icons_travel_once() {
        reset_icons();
        let a = icon("dog").into_string();
        icon("dog");
        icon("cat");
        assert!(a.contains("#pt-i-dog") && !a.contains("<path"));
        let sprite = icon_sprite().into_string();
        assert_eq!(sprite.matches("<symbol").count(), 2);
        assert_eq!(icon_sprite().into_string().matches("<symbol").count(), 0);
    }

    #[test]
    fn text() {
        assert_eq!(pct_decode("Ana%20Mar%C3%ADa"), "Ana María");
        assert_eq!(pct_decode("100%"), "100%");
        assert_eq!(kilos("4,25 kg"), Some(4.25));
        assert_eq!(kg(4.25), "4,25\u{a0}kg");
        assert_eq!(kg(12.0), "12\u{a0}kg");
        assert_eq!(kg(0.35), "350\u{a0}g");
        assert_eq!(hhmm("8:05"), Some("08:05".into()));
        assert_eq!(hhmm("25:00"), None);
    }
}
