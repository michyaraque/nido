//! Fixing any entry after the fact. The diary, the agenda and the undo bar link to `x=<key>` and
//! the page renders that entry's dialog filled in. Keys: `w` walk, `h` health note, `t` treat,
//! `k` weight, `r` booklet entry and `c` chore done, followed by the id; `f<pet>.<day>.<slot>` a
//! meal and `d<med>.<day>.<slot>` a dose (see `data::feed`).
use super::*;

fn dialog_id(key: &str) -> Option<&'static str> {
    Some(match key.get(..1)? {
        "w" => "walk-edit",
        "h" => "health-edit",
        "t" => "treat-edit",
        "k" => "weight-edit",
        "r" => "record-edit",
        "c" => "done-edit",
        "f" => "fed-edit",
        "d" => "dose-edit",
        _ => return None,
    })
}

/// Opens the edit dialog of `key` without leaving the view `ret`.
pub(super) fn edit_href(v: &V, ret: &str, key: &str) -> Option<String> {
    let id = dialog_id(key)?;
    let q = if ret.is_empty() { format!("x={key}") } else { format!("{ret}&x={key}") };
    Some(format!("{}#{id}", v.link(&q)))
}

fn three(rest: &str) -> Option<(i64, &str, &str)> {
    let mut it = rest.splitn(3, '.');
    Some((it.next()?.parse().ok()?, it.next()?, it.next()?))
}

/// The dialog `x` asks for, or nothing when the entry is gone.
pub(super) fn entry_modal(c: &Connection, v: &V, x: Option<&String>, ret: &str) -> rusqlite::Result<Markup> {
    let Some((kind, rest)) = x.and_then(|k| k.split_at_checked(1)) else { return Ok(html! {}) };
    let id: i64 = rest.parse().unwrap_or(0);
    let pet = |pid: i64| data::pet(c, pid);
    let out = match kind {
        "w" => match data::walk(c, id)? {
            Some(w) => pet(w.pet_id)?.map(|p| walk_modal(v, &p, ret, Some(&w))),
            None => None,
        },
        "h" => match data::health_one(c, id)? {
            Some((pid, h)) => pet(pid)?.map(|p| health_modal(v, &p, ret, Some(&h))),
            None => None,
        },
        "t" => match data::treat(c, id)? {
            Some((pid, t)) => pet(pid)?.map(|p| treat_modal(v, &p, ret, Some(&t), &[])),
            None => None,
        },
        "k" => match data::weight(c, id)? {
            Some((pid, w)) => pet(pid)?.map(|p| weight_modal(v, &p, ret, None, Some(&w))),
            None => None,
        },
        "r" => match data::record(c, id)? {
            Some(r) => pet(r.pet_id)?.map(|p| record_modal(v, &p, ret, Some(&r))),
            None => None,
        },
        "c" => data::done(c, id)?.map(|d| done_modal(v, ret, &d)),
        "f" => match three(rest) {
            Some((pid, day, slot)) => match (pet(pid)?, data::fed_one(c, pid, day, slot)?) {
                (Some(p), Some(g)) => Some(fed_modal(v, &p, ret, Some(&g))),
                _ => None,
            },
            None => None,
        },
        "d" => match three(rest) {
            Some((mid, day, slot)) => match (data::med(c, mid)?, data::dose_one(c, mid, day, slot)?) {
                (Some(m), Some(g)) => Some(dose_modal(v, &m, ret, &g)),
                _ => None,
            },
            None => None,
        },
        _ => None,
    };
    Ok(out.unwrap_or_else(|| html! {}))
}

/// Who did it; the person writing it down unless they say otherwise.
fn who_field(v: &V, legend: &str, by: i64) -> Markup {
    html! {
        @if v.me.people.len() > 1 {
            fieldset.field.pt-fieldset {
                legend { (legend) }
                div.pt-chips {
                    @for p in &v.me.people {
                        label.pt-chip.pt-who { input type="radio" name="by" value=(p.id) checked[p.id == by]; span { (v.avatar(p.id)) (if p.id == v.me.id { "Yo" } else { p.name.as_str() }) } }
                    }
                }
            }
        } @else {
            input type="hidden" name="by" value=(by);
        }
    }
}

fn day_time(v: &V, id: &str, day: Option<&str>, at: &str) -> Markup {
    html! {
        div.two {
            @if let Some(day) = day {
                div.field {
                    label for={ (id) "-day" } { "Día" }
                    input.input type="date" id={ (id) "-day" } name="day" max=(v.today) required value=(day);
                }
            }
            div.field {
                label for={ (id) "-at" } { "Hora" }
                input.input type="time" id={ (id) "-at" } name="at" required value=(at);
            }
        }
    }
}

/// New: a meal nobody ticked at the time, on any past day. Edit: which meal, when and who.
pub(super) fn fed_modal(v: &V, pet: &Pet, ret: &str, g: Option<&data::GivenOne>) -> Markup {
    let (id, title) = match g {
        Some(_) => ("fed-edit", "Editar comida".to_string()),
        None => ("fed-new", format!("Comida de {}", pet.name)),
    };
    let slots = pet.meal_slots();
    let slot = match g {
        Some(g) => g.slot.as_str(),
        None => slots.iter().rev().find(|s| s.as_str() <= v.time.as_str()).or(slots.first()).map_or("", String::as_str),
    };
    let off = !slots.iter().any(|s| s == slot);
    modal(id, &title, html! {
        form.form method="post" action="/plugins/pets/fed/save" {
            (v.ctx(ret))
            input type="hidden" name="pet" value=(pet.id);
            @if let Some(g) = g { input type="hidden" name="oday" value=(g.day); input type="hidden" name="oslot" value=(g.slot); }
            @if !slots.is_empty() {
                div.field {
                    label for={ (id) "-slot" } { "Qué comida" }
                    select.input id={ (id) "-slot" } name="slot" {
                        @for s in &slots { option value=(s) selected[s == slot] { "La de las " (s) } }
                        option value="" selected[off] { "Fuera de horario" }
                    }
                }
            }
            (day_time(v, id, Some(g.map_or(v.today.as_str(), |g| g.day.as_str())), g.map_or(v.time.as_str(), |g| g.at.as_str())))
            @if g.is_none() { p.hint { "Para cuando se os olvidó marcarla: pon el día y la hora en que comió." } }
            (who_field(v, "Quién se la dio", g.map_or(v.me.id, |g| g.by)))
            button.btn.primary.wide { (icon("check")) @if g.is_some() { "Guardar" } @else { "Apuntar" } }
        }
        @if let Some(g) = g {
            div.pt-danger { (delete_form_with(v, "/plugins/pets/fed/delete", ret, "Borrar comida", &[("pet", pet.id.to_string()), ("day", g.day.clone()), ("slot", g.slot.clone())])) }
        }
    })
}

/// A dose stays on its day and time of the course; what can change is when it was given and by whom.
pub(super) fn dose_modal(v: &V, m: &Med, ret: &str, g: &data::GivenOne) -> Markup {
    let key = [("med", m.id.to_string()), ("day", g.day.clone()), ("slot", g.slot.clone())];
    modal("dose-edit", &format!("Toma de {}", m.title), html! {
        form.form method="post" action="/plugins/pets/dose/save" {
            (v.ctx(ret))
            @for (k, val) in &key { input type="hidden" name=(k) value=(val); }
            p.hint.pt-lead { (m.pet) " · " (day_label(&g.day, &v.today)) @if !g.slot.is_empty() { " · la de las " (g.slot) } }
            (day_time(v, "dose-edit", None, &g.at))
            (who_field(v, "Quién se la dio", g.by))
            button.btn.primary.wide { (icon("check")) "Guardar" }
        }
        div.pt-danger { (delete_form_with(v, "/plugins/pets/dose/delete", ret, "Borrar toma", &key)) }
    })
}

pub(super) fn done_modal(v: &V, ret: &str, d: &data::Done) -> Markup {
    modal("done-edit", &d.title, html! {
        form.form method="post" action={ "/plugins/pets/done/" (d.id) } {
            (v.ctx(ret))
            (day_time(v, "done-edit", Some(&d.day), &d.at))
            p.hint { "Si cambias el día, la próxima vez se cuenta desde ahí." }
            (who_field(v, "Quién lo hizo", d.by))
            button.btn.primary.wide { (icon("check")) "Guardar" }
        }
        div.pt-danger { (delete_form(v, &format!("/plugins/pets/done/{}/delete", d.id), ret, "Borrar")) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_name_their_entry() {
        assert_eq!(three("3.2026-11-18.08:00"), Some((3, "2026-11-18", "08:00")));
        assert_eq!(three("3.2026-11-18."), Some((3, "2026-11-18", "")), "a dose with no time");
        assert_eq!(three("x.2026-11-18.08:00"), None);
        assert_eq!(dialog_id("f1.2026-11-18.08:00"), Some("fed-edit"));
        assert_eq!(dialog_id("z9"), None);
    }
}
