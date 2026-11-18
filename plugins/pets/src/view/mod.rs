//! The pages: the house's pets with today's agenda, and each pet's own page by sections.
use crate::{
    act::REMIND,
    data::{self, Given, Med, Pet, Record, Task, Visit},
    kit::*,
    month, now, Me, Params, Person,
};
use maud::{html, Markup, PreEscaped};
use rusqlite::Connection;
use std::collections::HashMap;

mod today;
mod home;
mod pet;
mod care;
mod vet;
mod tasks;
mod card;
mod rows;
mod treats;
mod entries;
use {today::*, home::*, pet::*, care::*, vet::*, tasks::*, card::*, rows::*, treats::*, entries::*};

const CSS: &str = include_str!(concat!(env!("OUT_DIR"), "/style.css"));
const JS: &str = include_str!("../../pets.js");

/// Long cache: the URL carries a hash of the script, so a new version is a new URL.
pub async fn script() -> axum::response::Response {
    use axum::{http::header, response::IntoResponse};
    ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8"), (header::CACHE_CONTROL, "private, max-age=31536000")], JS).into_response()
}

fn js_url() -> String {
    let h = JS.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    format!("/plugins/pets/assets/pets.js?v={h:x}")
}
const MONTHS: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic"];
const WEEK: [&str; 7] = ["L", "M", "X", "J", "V", "S", "D"];

struct V {
    m: String,
    today: String,
    time: String,
    me: Me,
}

impl V {
    fn link(&self, q: &str) -> String {
        if q.is_empty() {
            format!("/?month={}&tab=pets", self.m)
        } else {
            format!("/?month={}&tab=pets&{q}", self.m)
        }
    }
    /// Hidden fields every form carries: the month and the view to come back to.
    fn ctx(&self, ret: &str) -> Markup {
        html! { input type="hidden" name="month" value=(self.m); input type="hidden" name="ret" value=(ret); }
    }
    fn person(&self, id: i64) -> Option<(usize, &Person)> {
        self.me.people.iter().enumerate().find(|(_, p)| p.id == id)
    }
    fn name(&self, id: i64) -> String {
        self.person(id).map_or_else(|| "Alguien".into(), |(_, p)| if p.id == self.me.id { "Tú".into() } else { p.name.clone() })
    }
    fn names(&self, ids: &[i64]) -> String {
        join_names(&ids.iter().map(|id| self.name(*id)).collect::<Vec<_>>())
    }
    fn faces(&self, ids: &[i64]) -> Markup {
        html! { span.avatars { @for id in ids { (self.avatar(*id)) } } }
    }
    fn avatar(&self, id: i64) -> Markup {
        match self.person(id) {
            Some((_, p)) if p.photo > 0 => html! { img.avatar src={ "/avatars/" (p.id) "?v=" (p.photo) } alt="" title=(p.name) width="30" height="30"; },
            Some((i, p)) => {
                let bg = if p.color.starts_with('#') { format!("background:{}", p.color) } else { String::new() };
                html! { span class={ "avatar p" (i % 4) } style=(bg) title=(p.name) { (initial(&p.name)) } }
            }
            None => html! { span.avatar.pt-nobody { "?" } },
        }
    }
    fn now_local(&self) -> String {
        format!("{}T{}", self.today, self.time)
    }
    fn now_full(&self) -> String {
        format!("{} {}", self.today, self.time)
    }
}

fn join_names(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} y {last}", rest.join(", ")),
    }
}

fn num(p: &Params, k: &str) -> Option<i64> {
    p.get(k).and_then(|s| s.parse().ok())
}

fn weekday(day: &str) -> &'static str {
    WEEK[(days(day).unwrap_or(0) + 3).rem_euclid(7) as usize]
}

pub fn page(c: &Connection, me: Me, p: &Params) -> rusqlite::Result<Markup> {
    let (today, time) = now(c)?;
    let v = V { m: month(p.get("month").map(String::as_str), &today), today, time, me };
    reset_icons();
    let body = match num(p, "p").map(|id| data::pet(c, id)).transpose()?.flatten() {
        Some(pet) => pet_page(c, &v, &pet, p)?,
        None => home(c, &v, p)?,
    };
    Ok(html! { style { (PreEscaped(CSS)) } span hidden data-plugin="pets" data-plugin-js=(js_url()) {} (icon_sprite()) div.pt { (body) } })
}

// ---------- small pieces ----------

fn face(pet: &Pet, size: &str) -> Markup {
    let (_, _, ic) = data::kind(&pet.kind);
    html! {
        @if pet.photo > 0 {
            img class={ "pt-face " (size) } src={ "/plugins/pets/pet/" (pet.id) "/photo?v=" (pet.photo) } alt={ "Foto de " (pet.name) } width="480" height="480" loading="lazy" decoding="async";
        } @else {
            span class={ "pt-face pt-art " (size) } aria-hidden="true" { (icon(ic)) }
        }
    }
}

fn tint(pet: &Pet) -> String {
    format!("--pc:{}", data::color(&pet.color))
}

fn modal(id: &str, title: &str, body: Markup) -> Markup {
    html! {
        div.modal id=(id) role="dialog" aria-modal="true" aria-labelledby={ (id) "-t" } {
            a.scrim href="#_" aria-label="Cerrar" tabindex="-1" {}
            div.dialog.dialog-wide.pt-dialog {
                div.dialog-top {
                    h2 id={ (id) "-t" } { (title) }
                    a.icon-btn href="#_" aria-label="Cerrar" { (icon("x")) }
                }
                (body)
            }
        }
    }
}

fn sec_head(ic: &str, title: &str, more: Option<(String, &str)>) -> Markup {
    html! {
        div.pt-sec-head {
            h2 { span.pt-sec-ico { (icon(ic)) } (title) }
            @if let Some((href, label)) = more { a.pt-more href=(href) { (label) (icon("chevron-right")) } }
        }
    }
}

fn empty(ic: &str, title: &str, body: &str) -> Markup {
    html! { div.empty { span.empty-ico { (icon(ic)) } h3 { (title) } p { (body) } } }
}

fn every(n: i64) -> String {
    match n {
        1 => "cada día".into(),
        7 => "cada semana".into(),
        14 => "cada 2 semanas".into(),
        30 | 31 => "cada mes".into(),
        365 => "cada año".into(),
        n if n % 30 == 0 => format!("cada {} meses", n / 30),
        n if n % 7 == 0 => format!("cada {} semanas", n / 7),
        n => format!("cada {n} días"),
    }
}

/// The next time `birth` comes round, from `today` (today included).
fn next_birthday(birth: &str, today: &str) -> Option<String> {
    if birth.len() != 10 || birth[..4] >= today[..4] {
        return None;
    }
    let this = format!("{}{}", &today[..4], &birth[4..]);
    let d = if this.as_str() >= today { this } else { format!("{:04}{}", today[..4].parse::<i64>().ok()? + 1, &birth[4..]) };
    days(&d).map(ymd)
}

fn cal(day: &str) -> Markup {
    html! {
        span.pt-cal aria-hidden="true" {
            b { (day[8..10].trim_start_matches('0')) }
            small { (MONTHS[day[5..7].parse::<usize>().unwrap_or(1) - 1]) }
        }
    }
}

/// Shows 4 rows on a phone and 8 on wider screens (2 and 4 days when `by_day`) and folds the rest
/// behind "Ver N más". A checkbox and its label do the folding, so it works without JavaScript.
fn clamp(id: &str, rows: Vec<Markup>, by_day: bool) -> Markup {
    let n = rows.len();
    let (phone, wide) = if by_day { (2, 4) } else { (4, 8) };
    let more = |k: usize| match (by_day, k) {
        (true, 1) => "Ver 1 día más".to_string(),
        (true, k) => format!("Ver {k} días más"),
        (false, k) => format!("Ver {k} más"),
    };
    html! {
        div class={ "pt-clamp" @if by_day { " days" } @if n <= wide { " few" } } {
            @if n > phone { input.pt-clamp-in type="checkbox" id=(id); }
            div.pt-list { @for r in rows { (r) } }
            @if n > phone {
                label.pt-clamp-btn for=(id) {
                    span.pt-more-m { (more(n - phone)) }
                    @if n > wide { span.pt-more-d { (more(n - wide)) } }
                    span.pt-less { "Ver menos" }
                    (icon("chevron-down"))
                }
            }
        }
    }
}

fn day_groups<T>(v: &V, items: &[T], at: impl Fn(&T) -> &str, row: impl Fn(&T) -> Markup) -> Vec<Markup> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < items.len() {
        let day = at(&items[i])[..10].to_string();
        let j = items[i..].iter().position(|x| at(x)[..10] != *day).map_or(items.len(), |k| i + k);
        out.push(html! { div.pt-dgroup { div.pt-daysep { (day_label(&day, &v.today)) } @for x in &items[i..j] { (row(x)) } } });
        i = j;
    }
    out
}

fn delete_form(v: &V, action: &str, ret: &str, label: &str) -> Markup {
    delete_form_with(v, action, ret, label, &[])
}

/// For entries named by several fields instead of an id in the path (a meal, a dose).
fn delete_form_with(v: &V, action: &str, ret: &str, label: &str, fields: &[(&str, String)]) -> Markup {
    html! {
        details.pt-confirm {
            summary.btn.ghost.sm { (icon("trash-2")) (label) }
            form method="post" action=(action) {
                (v.ctx(ret))
                @for (k, val) in fields { input type="hidden" name=(k) value=(val); }
                span { "¿Seguro? No se puede deshacer." }
                button.btn.danger.sm { "Sí, borrar" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_naturally() {
        let n = |v: &[&str]| join_names(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(n(&["Ana"]), "Ana");
        assert_eq!(n(&["Tú", "Leo"]), "Tú y Leo");
        assert_eq!(n(&["Ana", "Leo", "Sara"]), "Ana, Leo y Sara");
    }

    #[test]
    fn birthdays() {
        assert_eq!(next_birthday("2021-11-20", "2026-11-16").as_deref(), Some("2026-11-20"));
        assert_eq!(next_birthday("2021-03-01", "2026-11-16").as_deref(), Some("2027-03-01"));
        assert_eq!(next_birthday("2021-11-16", "2026-11-16").as_deref(), Some("2026-11-16"));
        assert_eq!(next_birthday("2026-11-01", "2026-11-16"), None);
        assert_eq!(next_birthday("", "2026-11-16"), None);
    }
}
