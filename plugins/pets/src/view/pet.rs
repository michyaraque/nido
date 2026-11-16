//! One pet's page: its header, quick actions, section tabs and its own day.
use super::*;

pub(super) const SECTIONS: &[(&str, &str, &str)] = &[
    ("", "Hoy", "sun"),
    ("diary", "Diario", "history"),
    ("med", "Medicación", "pill"),
    ("health", "Salud", "heart-pulse"),
    ("walks", "Paseos", "footprints"),
    ("weight", "Peso", "weight"),
    ("vet", "Veterinario", "stethoscope"),
    ("tasks", "Tareas", "list-todo"),
    ("profile", "Ficha", "id-card"),
];

pub(super) fn pet_page(c: &Connection, v: &V, pet: &Pet, p: &Params) -> rusqlite::Result<Markup> {
    let s = p.get("s").map(String::as_str).unwrap_or("");
    let s = SECTIONS.iter().find(|x| x.0 == s).map_or("", |x| x.0);
    let ret = if s.is_empty() { format!("p={}", pet.id) } else { format!("p={}&s={s}", pet.id) };
    let (_, kname, kic) = data::kind(&pet.kind);
    let weights = data::weights(c, pet.id)?;
    let walker = data::walks(&pet.kind);
    let walks_today = if walker { data::walks_since(c, Some(pet.id), &v.today)?.len() } else { 0 };
    let tasks = data::tasks(c, Some(pet.id))?;
    let bath = tasks.iter().find(|t| t.icon == "bath" && !t.due.is_empty());
    let slots = pet.meal_slots();
    let fed = data::fed(c, &v.today)?;
    let fed_today = slots.iter().filter(|s| fed.contains_key(&(pet.id, v.today.clone(), (*s).clone()))).count();
    let birthday = next_birthday(&pet.birth, &v.today).filter(|b| *b <= add_days(&v.today, 30));
    let edit = num(p, "e");
    let idea = num(p, "nt").filter(|_| s == "tasks").and_then(|i| data::suggestions(&pet.kind).get(i as usize));
    let link = |sec: &str| v.link(&format!("p={}&s={sec}", pet.id));
    let trend = match weights.as_slice() {
        [.., a, b] if (b.kg - a.kg).abs() >= 0.0005 => Some(b.kg > a.kg),
        _ => None,
    };

    let body = match s {
        "diary" => diary_section(c, v, pet)?,
        "med" => med_section(c, v, pet, &ret, edit)?,
        "health" => health_section(c, v, pet, &ret, edit)?,
        "walks" => walks_section(c, v, pet, &ret, edit)?,
        "weight" => weight_section(v, pet, &weights, &ret, edit),
        "vet" => vet_section(c, v, pet, &ret, edit, num(p, "rec"))?,
        "tasks" => tasks_section(v, pet, &tasks, &ret, edit),
        "profile" => profile_section(v, pet, weights.last().map(|w| w.kg)),
        _ => overview(c, v, pet, &ret)?,
    };

    Ok(html! {
        a.back.pt-back href=(v.link("")) { (icon("chevron-left")) "Mascotas" }
        section.pt-hero style=(tint(pet)) {
            div.pt-hero-top {
                div.pt-hero-pic {
                    (face(pet, "lg"))
                    span.pt-kind-badge title=(kname) { (icon(kic)) }
                }
                div.pt-hero-body {
                    h1 { (pet.name) }
                    div.pt-tags {
                        span.pt-tag { (kname) }
                        @if let Some(sx) = pet.sex_label() { span.pt-tag { (sx) } }
                        @if let Some(a) = age(&pet.birth, &v.today) { span.pt-tag { (a) } }
                        @if !pet.breed.is_empty() { span.pt-tag { (pet.breed) } }
                        @if let Some(b) = &birthday {
                            span.pt-tag.pt-tag-cake { (icon("cake")) @if *b == v.today { "Cumple hoy" } @else { "Cumple " (rel(b, &v.today)) } }
                        }
                    }
                    @if !pet.allergies.is_empty() {
                        p.pt-allergy { (icon("circle-alert")) span { b { "Alergias: " } (pet.allergies) } }
                    }
                }
                a.icon-btn.pt-edit href="#pet-edit" aria-label="Editar la ficha" title="Editar la ficha" { (icon("pencil")) }
            }
            div.pt-stats {
                a.pt-stat href=(link("weight")) {
                    span { (icon("weight")) "Peso" }
                    b {
                        @if let Some(w) = weights.last() { (kg(w.kg)) } @else { "—" }
                        @if let Some(up) = trend { span.pt-trend-ico title=(if up { "Ha subido" } else { "Ha bajado" }) { (icon(if up { "trending-up" } else { "trending-down" })) } }
                    }
                }
                @if !slots.is_empty() {
                    a.pt-stat href=(v.link(&format!("p={}", pet.id))) {
                        span { (icon("utensils")) "Comidas" }
                        b { (fed_today) small { " de " (slots.len()) } }
                    }
                }
                @if walker {
                    a.pt-stat href=(link("walks")) {
                        span { (icon("footprints")) "Paseos hoy" }
                        b { (walks_today) }
                    }
                }
                @if let Some(b) = bath {
                    a.pt-stat href=(link("tasks")) {
                        span { (icon("bath")) "Baño" }
                        b { @if b.due <= v.today { "Hoy" } @else { (rel(&b.due, &v.today)) } }
                    }
                }
            }
        }

        div.pt-quick style=(tint(pet)) {
            form method="post" action="/plugins/pets/fed" {
                (v.ctx(&ret)) input type="hidden" name="pet" value=(pet.id);
                button.pt-q { span.pt-q-ico { (icon("utensils")) } span { "Ha comido" } }
            }
            @if walker {
                form method="post" action="/plugins/pets/walk" {
                    (v.ctx(&ret)) input type="hidden" name="pet" value=(pet.id);
                    button.pt-q { span.pt-q-ico { (icon("footprints")) } span { "Paseado" } }
                }
            }
            form method="post" action="/plugins/pets/health" {
                (v.ctx(&ret)) input type="hidden" name="pet" value=(pet.id); input type="hidden" name="kind" value="poop"; input type="hidden" name="mood" value="good";
                button.pt-q.ok { span.pt-q-ico { (icon("smile")) } span { "Caca bien" } }
            }
            form method="post" action="/plugins/pets/health" {
                (v.ctx(&ret)) input type="hidden" name="pet" value=(pet.id); input type="hidden" name="kind" value="poop"; input type="hidden" name="mood" value="bad";
                button.pt-q.bad { span.pt-q-ico { (icon("frown")) } span { "Caca mal" } }
            }
            a.pt-q href="#health" { span.pt-q-ico { (icon("heart-pulse")) } span { "Salud" } }
            @if !walker { a.pt-q href="#weight" { span.pt-q-ico { (icon("weight")) } span { "Peso" } } }
        }

        nav.pt-tabs aria-label="Secciones" style=(tint(pet)) {
            @for (id, label, ic) in SECTIONS {
                @if *id != "walks" || walker {
                    a.on[*id == s] href=(if id.is_empty() { v.link(&format!("p={}", pet.id)) } else { link(id) }) aria-current=[(*id == s).then_some("page")] {
                        (icon(ic)) (label)
                    }
                }
            }
        }

        div style=(tint(pet)) { (body) }

        (pet_modal(v, Some(pet), weights.last().map(|w| w.kg)))
        (weight_modal(v, pet, &ret, weights.last().map(|w| w.kg), None))
        (health_modal(v, pet, &ret, None))
        @if walker { (walk_modal(v, pet, &ret, None)) }
        (med_modal(v, pet, &ret, None))
        (visit_modal(v, pet, &ret, None))
        (record_modal(v, pet, &ret, None))
        (task_modal(v, pet, &ret, None, idea))
    })
}

pub(super) fn overview(c: &Connection, v: &V, pet: &Pet, ret: &str) -> rusqlite::Result<Markup> {
    let one = std::slice::from_ref(pet);
    let by_pet: HashMap<i64, &Pet> = [(pet.id, pet)].into_iter().collect();
    let items = agenda_items(c, v, one)?;
    let mut coming = upcoming(c, v, one, 45)?;
    coming.truncate(8);
    let events = data::feed(c, Some(pet.id), 12)?;
    let link = |s: &str| v.link(&format!("p={}&s={s}", pet.id));
    let hint = if pet.meal_slots().is_empty() {
        html! { "Nada pendiente hoy. " a href="#pet-edit" { "Pon sus horas de comer" } " y saldrán aquí para marcarlas." }
    } else {
        html! { "Nada pendiente hoy." }
    };
    Ok(html! {
        div.pt-home {
            (agenda_card(v, &items, &by_pet, ret, false, hint))
            div.pt-side {
                section.card.pt-card {
                    (sec_head("calendar-clock", "Próximamente", Some((link("vet"), "Citas"))))
                    @if coming.is_empty() {
                        p.pt-none { "Nada a la vista. " a href="#visit-new" { "Apuntar una cita" } }
                    } @else { (upcoming_list(v, &coming, &by_pet, false)) }
                }
                section.card.pt-card {
                    (sec_head("history", "Diario", Some((link("diary"), "Todo"))))
                    @if events.is_empty() { p.pt-none { "Usa los botones de arriba para apuntar lo de cada día." } }
                    @else { (diary(v, &events, &by_pet, false, false, "feed-pet")) }
                }
            }
        }
    })
}

pub(super) fn diary_section(c: &Connection, v: &V, pet: &Pet) -> rusqlite::Result<Markup> {
    let events = data::feed(c, Some(pet.id), 80)?;
    let by_pet: HashMap<i64, &Pet> = [(pet.id, pet)].into_iter().collect();
    Ok(html! {
        p.hint.pt-lead { "Todo lo que habéis apuntado de " (pet.name) ", y quién lo hizo." }
        section.card.pt-card {
            @if events.is_empty() { (empty("history", "El diario está vacío", "Cada comida, paseo, toma o visita que apuntéis aparecerá aquí.")) }
            @else { (diary(v, &events, &by_pet, false, true, "feed-all")) }
        }
    })
}

pub(super) fn course(m: &Med, today: &str) -> String {
    let mut s = String::new();
    if !m.dose.is_empty() {
        s += &m.dose;
        s += " · ";
    }
    let day = days(today).zip(days(&m.start)).map(|(t, st)| t - st + 1).unwrap_or(1);
    if m.start.as_str() > today {
        s += &format!("Empieza {}", rel(&m.start, today));
    } else if let (Some(n), true) = (m.days, m.stopped.is_empty()) {
        s += &format!("Día {} de {n}", day.min(n));
    } else {
        s += &format!("Desde el {}", day_label(&m.start, today).to_lowercase());
    }
    match m.last_day() {
        Some(l) if l.as_str() > today => s += &format!(" · hasta el {}", day_label(&l, today).to_lowercase()),
        Some(l) if l.as_str() == today => s += " · último día",
        Some(_) => {}
        None => s += " · sin fecha de fin",
    }
    s
}
