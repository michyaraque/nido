//! Today: the agenda of things to tick, what is coming up and the house's diary.
use super::*;

pub(super) struct Ag {
    pub(super) time: String,
    pub(super) pet: i64,
    pub(super) icon: &'static str,
    pub(super) title: String,
    pub(super) sub: String,
    pub(super) done: Option<(i64, String)>,
    pub(super) late: bool,
    pub(super) post: Option<(String, Vec<(&'static str, String)>)>,
    pub(super) href: String,
}

pub(super) fn agenda_items(c: &Connection, v: &V, pets: &[Pet]) -> rusqlite::Result<Vec<Ag>> {
    let ids: Vec<i64> = pets.iter().map(|p| p.id).collect();
    let fed = data::fed(c, &v.today)?;
    let doses = data::doses(c, &v.today)?;
    let mut out = Vec::new();
    let link = |p: i64, s: &str| v.link(&format!("p={p}{s}"));
    for p in pets {
        if next_birthday(&p.birth, &v.today).as_deref() == Some(v.today.as_str()) {
            out.push(Ag { time: String::new(), pet: p.id, icon: "cake", title: format!("Cumple {}", age(&p.birth, &v.today).unwrap_or_default()), sub: "Su cumpleaños".into(), done: None, late: false, post: None, href: link(p.id, "") });
        }
        for slot in p.meal_slots() {
            let done = fed.get(&(p.id, v.today.clone(), slot.clone())).cloned();
            let sub = match (p.portion.is_empty(), p.food.is_empty()) {
                (true, _) => p.food.clone(),
                (false, true) => p.portion.clone(),
                (false, false) => format!("{} de {}", p.portion, p.food),
            };
            out.push(Ag {
                late: done.is_none() && slot < v.time,
                time: slot.clone(),
                pet: p.id,
                icon: "utensils",
                title: "Comida".into(),
                sub,
                done,
                post: Some(("/plugins/pets/fed".into(), vec![("pet", p.id.to_string()), ("day", v.today.clone()), ("slot", slot)])),
                href: link(p.id, ""),
            });
        }
    }
    for m in data::meds(c, None)?.into_iter().filter(|m| ids.contains(&m.pet_id) && m.active_on(&v.today)) {
        for slot in m.slots() {
            let done = doses.get(&(m.id, v.today.clone(), slot.clone())).cloned();
            out.push(Ag {
                late: done.is_none() && !slot.is_empty() && slot < v.time,
                time: slot.clone(),
                pet: m.pet_id,
                icon: "pill",
                title: m.title.clone(),
                sub: course(&m, &v.today),
                done,
                post: Some(("/plugins/pets/dose".into(), vec![("med", m.id.to_string()), ("day", v.today.clone()), ("slot", slot)])),
                href: link(m.pet_id, "&s=med"),
            });
        }
    }
    for t in data::tasks(c, None)?.into_iter().filter(|t| ids.contains(&t.pet_id) && !t.due.is_empty() && t.due <= v.today) {
        let late = t.due < v.today;
        out.push(Ag {
            time: String::new(),
            pet: t.pet_id,
            icon: data::TASK_ICONS.iter().find(|i| **i == t.icon).copied().unwrap_or("sparkles"),
            title: t.title.clone(),
            sub: if late { format!("Tocaba {}", rel(&t.due, &v.today)) } else { "Toca hoy".into() },
            done: None,
            late,
            post: Some((format!("/plugins/pets/task/{}/done", t.id), vec![])),
            href: link(t.pet_id, &format!("&s=tasks&e={}#task-edit", t.id)),
        });
    }
    for x in data::visits(c, None)?.into_iter().filter(|x| ids.contains(&x.pet_id) && x.at.starts_with(v.today.as_str())) {
        out.push(Ag {
            time: x.at[11..16].to_string(),
            pet: x.pet_id,
            icon: "stethoscope",
            title: "Veterinario".into(),
            sub: [x.title.as_str(), x.place.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · "),
            done: (x.at < v.now_full()).then(|| (0, String::new())),
            late: false,
            post: None,
            href: link(x.pet_id, &format!("&s=vet&e={}#visit-edit", x.id)),
        });
    }
    out.sort_by(|a, b| a.time.cmp(&b.time));
    Ok(out)
}

pub(super) fn agenda(v: &V, items: &[Ag], pets: &HashMap<i64, &Pet>, ret: &str, show_pet: bool) -> Markup {
    html! {
        ol.pt-agenda {
            @for a in items {
                @if let Some(pet) = pets.get(&a.pet) {
                    li.pt-ag.done[a.done.is_some()].late[a.late] style=(tint(pet)) {
                        span.pt-ag-time { @if a.time.is_empty() { (icon("sun")) } @else { (a.time) } }
                        @if show_pet { (face(pet, "xs")) } @else { span.pt-ag-ico { (icon(a.icon)) } }
                        div.pt-row-main {
                            a.pt-row-title href=(a.href) { @if show_pet { span.pt-ag-kind { (icon(a.icon)) } } (a.title) @if show_pet { span.pt-ev-pet { (pet.name) } } }
                            span.pt-row-sub {
                                @match &a.done {
                                    Some((by, at)) if *by > 0 => { (v.name(*by)) " · a las " (at) }
                                    Some(_) => "Hecho",
                                    None if a.late => span.pt-late { @if a.time.is_empty() { (a.sub) } @else { "Pendiente desde las " (a.time) } },
                                    None => (a.sub),
                                }
                            }
                        }
                        @if let Some((action, fields)) = &a.post {
                            form method="post" action=(action) {
                                (v.ctx(ret))
                                @for (k, val) in fields { input type="hidden" name=(k) value=(val); }
                                @match &a.done {
                                    Some((by, _)) => button.pt-tick.on aria-pressed="true" title="Hecho. Toca para desmarcar" aria-label={ "Desmarcar: " (a.title) } { (v.avatar(*by)) },
                                    None => button.pt-tick aria-pressed="false" aria-label={ "Marcar como hecho: " (a.title) } title="Marcar como hecho" { (icon("check")) },
                                }
                            }
                        } @else if a.done.is_some() {
                            span.pt-tick.on.pt-static aria-hidden="true" { (icon("check")) }
                        }
                    }
                }
            }
        }
    }
}

pub(super) fn agenda_card(v: &V, items: &[Ag], pets: &HashMap<i64, &Pet>, ret: &str, show_pet: bool, empty_text: Markup) -> Markup {
    let done = items.iter().filter(|a| a.done.is_some()).count();
    let total = items.len();
    html! {
        section.card.pt-card.pt-today {
            div.pt-today-head {
                div {
                    h2 { "Hoy" }
                    p { (long_day(&v.today)) }
                }
                @if total > 0 {
                    div.pt-ring style={ "--p:" (done * 100 / total) } role="img" aria-label={ (done) " de " (total) " hechas" } {
                        span { b { (done) } "/" (total) }
                    }
                }
            }
            @if items.is_empty() {
                div.pt-calm { span.pt-calm-ico { (icon("sun")) } p { (empty_text) } }
            } @else {
                @let fold = done > 0 && done < total;
                @let id = if show_pet { "ag-done-all" } else { "ag-done-pet" };
                @let later = (total - done).saturating_sub(4);
                @if fold { input.pt-done-in type="checkbox" id=(id); }
                @if later > 0 { input.pt-later-in type="checkbox" id={ (id) "-later" }; }
                (agenda(v, items, pets, ret, show_pet))
                @if later > 0 {
                    label.pt-later-btn for={ (id) "-later" } {
                        span.pt-more-m { "Ver " (later) " más pendientes" } span.pt-less { "Ver menos" } (icon("chevron-down"))
                    }
                }
                @if fold {
                    label.pt-done-btn for=(id) {
                        span.pt-more-m { (icon("circle-check")) "Ver " (done) @if done == 1 { " hecha" } @else { " hechas" } }
                        span.pt-less { "Ocultar lo hecho" }
                    }
                }
            }
        }
    }
}

// ---------- coming up ----------

pub(super) struct Up {
    pub(super) day: String,
    pub(super) time: String,
    pub(super) pet: i64,
    pub(super) icon: &'static str,
    pub(super) title: String,
    pub(super) href: String,
}

/// Soonest first, from tomorrow to `horizon` days ahead.
pub(super) fn upcoming(c: &Connection, v: &V, pets: &[Pet], horizon: i64) -> rusqlite::Result<Vec<Up>> {
    let ids: Vec<i64> = pets.iter().map(|p| p.id).collect();
    let one = (pets.len() == 1).then(|| pets[0].id);
    let tomorrow = add_days(&v.today, 1);
    let end = add_days(&v.today, horizon);
    let mut out = Vec::new();
    for x in data::visits(c, one)? {
        if ids.contains(&x.pet_id) && x.at[..10] >= *tomorrow && x.at[..10] <= *end {
            let what = if x.title.is_empty() { "Veterinario".to_string() } else { x.title.clone() };
            out.push(Up { day: x.at[..10].into(), time: x.at[11..16].into(), pet: x.pet_id, icon: "stethoscope", title: what, href: v.link(&format!("p={}&s=vet&e={}#visit-edit", x.pet_id, x.id)) });
        }
    }
    for r in data::records(c, one)? {
        if ids.contains(&r.pet_id) && !r.next.is_empty() && r.next >= tomorrow && r.next <= end {
            out.push(Up { day: r.next.clone(), time: String::new(), pet: r.pet_id, icon: data::record_kind(&r.kind).2, title: r.title(), href: v.link(&format!("p={}&s=vet", r.pet_id)) });
        }
    }
    for t in data::tasks(c, one)? {
        if ids.contains(&t.pet_id) && t.due >= tomorrow && t.due <= end {
            let ic = data::TASK_ICONS.iter().find(|i| **i == t.icon).copied().unwrap_or("sparkles");
            out.push(Up { day: t.due.clone(), time: String::new(), pet: t.pet_id, icon: ic, title: t.title.clone(), href: v.link(&format!("p={}&s=tasks", t.pet_id)) });
        }
    }
    for p in pets {
        if let Some(b) = next_birthday(&p.birth, &v.today).filter(|b| *b >= tomorrow && *b <= end) {
            out.push(Up { day: b.clone(), time: String::new(), pet: p.id, icon: "cake", title: format!("Cumple {}", age(&p.birth, &b).unwrap_or_default()), href: v.link(&format!("p={}", p.id)) });
        }
    }
    out.sort_by(|a, b| (&a.day, &a.time).cmp(&(&b.day, &b.time)));
    Ok(out)
}

pub(super) fn upcoming_list<'a>(v: &V, items: impl IntoIterator<Item = &'a Up>, pets: &HashMap<i64, &Pet>, show_pet: bool) -> Markup {
    html! {
        div.pt-list.pt-carousel {
            @for u in items {
                @if let Some(pet) = pets.get(&u.pet) {
                    a.pt-up href=(u.href) style=(tint(pet)) {
                        (cal(&u.day))
                        div.pt-row-main {
                            span.pt-row-title { span.pt-ag-kind { (icon(u.icon)) } (u.title) @if show_pet { span.pt-ev-pet { (pet.name) } } }
                            span.pt-row-sub { (day_label(&u.day, &v.today)) @if !u.time.is_empty() { " a las " (u.time) } }
                        }
                        span.pt-when { (rel(&u.day, &v.today)) }
                    }
                }
            }
        }
    }
}

// ---------- the diary ----------

pub(super) fn event_row(v: &V, e: &data::Event, pet: &Pet, show_pet: bool) -> Markup {
    let time = e.at.get(11..16).unwrap_or("");
    let (ic, title, extra, tone): (&str, String, String, &str) = match e.kind.as_str() {
        "walk" => ("footprints", "Paseo".into(), if e.a.is_empty() { e.b.clone() } else { format!("{} min", e.a) }, ""),
        "fed" => ("utensils", "Comida".into(), if e.a.as_str() != time && !e.a.is_empty() { format!("la de las {}", e.a) } else { String::new() }, ""),
        "dose" => ("pill", e.a.clone(), if e.b.is_empty() { String::new() } else { format!("toma de las {}", e.b) }, ""),
        "health" => {
            let (_, k, kic) = data::health_kind(&e.a);
            let (_, m, _, tone) = data::mood(&e.b);
            (kic, format!("{k} · {}", m.to_lowercase()), String::new(), tone)
        }
        "task" => (data::TASK_ICONS.iter().find(|i| **i == e.b).copied().unwrap_or("sparkles"), e.a.clone(), String::new(), ""),
        "weight" => ("weight", format!("Peso: {}", e.a.parse().map(kg).unwrap_or_default()), String::new(), ""),
        _ => {
            let (_, k, kic) = data::record_kind(&e.a);
            (kic, if e.b.is_empty() { k.to_string() } else { format!("{k}: {}", e.b) }, String::new(), "")
        }
    };
    let mut sub: Vec<String> = Vec::new();
    if e.people.len() > 1 {
        sub.push(v.names(&e.people));
    } else if e.by > 0 {
        sub.push(v.name(e.by));
    }
    if !time.is_empty() {
        sub.push(time.to_string());
    }
    if !extra.is_empty() {
        sub.push(extra);
    }
    html! {
        div.pt-event style=(tint(pet)) {
            span class={ "pt-ev-ico " (tone) } { (icon(ic)) }
            div.pt-row-main {
                span.pt-row-title { (title) @if show_pet { span.pt-ev-pet { (pet.name) } } }
                @if !sub.is_empty() { span.pt-row-sub { (sub.join(" · ")) } }
            }
        }
    }
}

pub(super) fn diary(v: &V, events: &[data::Event], pets: &HashMap<i64, &Pet>, show_pet: bool, by_day: bool, id: &str) -> Markup {
    let row = |e: &data::Event| pets.get(&e.pet_id).map_or_else(|| html! {}, |pet| event_row(v, e, pet, show_pet));
    let rows = if by_day { day_groups(v, events, |e| e.at.as_str(), row) } else { events.iter().map(row).collect() };
    clamp(id, rows, by_day)
}
