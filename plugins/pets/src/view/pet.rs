//! One pet's page: its header, quick logs, today, and every section listed with a summary.
use super::*;

pub(super) const SECTIONS: &[(&str, &str, &str)] = &[
    ("", "Hoy", "sun"),
    ("med", "Medicación", "pill"),
    ("walks", "Paseos", "footprints"),
    ("treats", "Premios", "cookie"),
    ("health", "Salud", "heart-pulse"),
    ("weight", "Peso", "weight"),
    ("vet", "Veterinario", "stethoscope"),
    ("tasks", "Tareas", "list-todo"),
    ("diary", "Diario", "history"),
    ("profile", "Ficha", "id-card"),
];

pub(super) fn pet_page(c: &Connection, v: &V, pet: &Pet, p: &Params) -> rusqlite::Result<Markup> {
    let s = p.get("s").map(String::as_str).unwrap_or("");
    let (s, label, sic) = SECTIONS.iter().find(|x| x.0 == s).copied().unwrap_or(SECTIONS[0]);
    let ret = if s.is_empty() { format!("p={}", pet.id) } else { format!("p={}&s={s}", pet.id) };
    let weights = data::weights(c, pet.id)?;
    let walker = data::walks(&pet.kind);
    let tasks = data::tasks(c, Some(pet.id))?;
    let edit = num(p, "e");
    let idea = num(p, "nt").filter(|_| s == "tasks").and_then(|i| data::suggestions(&pet.kind).get(i as usize));
    let treat_names = data::treat_names(c, pet, &add_days(&v.today, -90))?;

    let body = match s {
        "diary" => diary_section(c, v, pet, &ret)?,
        "med" => med_section(c, v, pet, &ret, edit)?,
        "health" => health_section(c, v, pet, &ret, edit)?,
        "walks" => walks_section(c, v, pet, &ret, edit)?,
        "treats" => treats_section(c, v, pet, &ret, edit)?,
        "weight" => weight_section(v, pet, &weights, &ret, edit),
        "vet" => vet_section(c, v, pet, &ret, edit, num(p, "rec"))?,
        "tasks" => tasks_section(v, pet, &tasks, &ret, edit),
        "profile" => profile_section(v, pet, weights.last().map(|w| w.kg)),
        _ => overview(c, v, pet, &ret, &weights, &tasks)?,
    };

    Ok(html! {
        @if s.is_empty() {
            a.back.pt-back href=(v.link("")) { (icon("chevron-left")) "Mascotas" }
            (hero(v, pet, &weights))
            (quick(v, pet, &ret, walker))
        } @else {
            a.back.pt-back href=(v.link(&format!("p={}", pet.id))) { (icon("chevron-left")) (pet.name) }
            div.pt-sec-page style=(tint(pet)) { span.pt-sec-ico { (icon(sic)) } h1 { (label) } }
        }
        @if let Some(u) = p.get("undo") { (undo_bar(v, u, &ret)) }

        div style=(tint(pet)) { (body) }

        (pet_modal(v, Some(pet), weights.last().map(|w| w.kg)))
        (weight_modal(v, pet, &ret, weights.last().map(|w| w.kg), None))
        (health_modal(v, pet, &ret, None))
        @if walker { (walk_modal(v, pet, &ret, None)) }
        (treat_modal(v, pet, &ret, None, &treat_names))
        (more_modal(v, pet, &ret, walker))
        (med_modal(v, pet, &ret, None))
        (visit_modal(v, pet, &ret, None))
        (record_modal(v, pet, &ret, None))
        (task_modal(v, pet, &ret, None, idea))
        (fed_modal(v, pet, &ret, None))
        (entry_modal(c, v, p.get("x"), &ret)?)
    })
}

fn hero(v: &V, pet: &Pet, weights: &[data::Weight]) -> Markup {
    let (_, kname, kic) = data::kind(&pet.kind);
    let birthday = next_birthday(&pet.birth, &v.today).filter(|b| *b <= add_days(&v.today, 30));
    html! {
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
                        @if let Some(a) = age(&pet.birth, &v.today) { span.pt-tag { (a) } }
                        @if let Some(w) = weights.last() { span.pt-tag { (kg(w.kg)) } }
                        @if let Some(sx) = pet.sex_label() { span.pt-tag { (sx) } }
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
        }
    }
}

/// The few things people log every day, one tap each; the rest waits in «Más».
fn quick(v: &V, pet: &Pet, ret: &str, walker: bool) -> Markup {
    html! {
        div.pt-quick style=(tint(pet)) {
            form method="post" action="/plugins/pets/fed" {
                (v.ctx(ret)) input type="hidden" name="pet" value=(pet.id); input type="hidden" name="quick" value="1";
                button.pt-q { span.pt-q-ico { (icon("utensils")) } span { "Ha comido" } }
            }
            @if walker {
                form method="post" action="/plugins/pets/walk" {
                    (v.ctx(ret)) input type="hidden" name="pet" value=(pet.id); input type="hidden" name="quick" value="1";
                    button.pt-q { span.pt-q-ico { (icon("footprints")) } span { "Paseado" } }
                }
            } @else {
                a.pt-q href="#health" { span.pt-q-ico { (icon("heart-pulse")) } span { "Salud" } }
            }
            a.pt-q href="#treat" { span.pt-q-ico { (icon("cookie")) } span { "Premio" } }
            a.pt-q href="#log-more" { span.pt-q-ico { (icon("ellipsis")) } span { "Más" } }
        }
    }
}

fn more_modal(v: &V, pet: &Pet, ret: &str, walker: bool) -> Markup {
    let poop = |mood: &str, tone: &str, ic: &str, label: &str| html! {
        form method="post" action="/plugins/pets/health" {
            (v.ctx(ret)) input type="hidden" name="pet" value=(pet.id); input type="hidden" name="quick" value="1";
            input type="hidden" name="kind" value="poop"; input type="hidden" name="mood" value=(mood);
            button class={ "pt-q " (tone) } { span.pt-q-ico { (icon(ic)) } span { (label) } }
        }
    };
    modal("log-more", &format!("Apuntar de {}", pet.name), html! {
        div.pt-more-grid style=(tint(pet)) {
            (poop("good", "ok", "smile", "Caca bien"))
            (poop("bad", "bad", "frown", "Caca mal"))
            @if walker { a.pt-q href="#health" { span.pt-q-ico { (icon("heart-pulse")) } span { "Salud" } } }
            a.pt-q href="#weight" { span.pt-q-ico { (icon("weight")) } span { "Peso" } }
            @if walker { a.pt-q href="#walk" { span.pt-q-ico { (icon("footprints")) } span { "Paseo con detalles" } } }
            a.pt-q href="#fed-new" { span.pt-q-ico { (icon("utensils")) } span { "Comida con detalles" } }
            a.pt-q href="#visit-new" { span.pt-q-ico { (icon("stethoscope")) } span { "Cita" } }
            a.pt-q href="#med-new" { span.pt-q-ico { (icon("pill")) } span { "Medicina" } }
            a.pt-q href="#task-new" { span.pt-q-ico { (icon("list-todo")) } span { "Tarea" } }
        }
    })
}

/// What a quick log saved, with the way to take it back.
fn undo_bar(v: &V, code: &str, ret: &str) -> Markup {
    let what = match code.get(..1) {
        Some("w") => "Paseo apuntado",
        Some("h") => "Apuntado en su salud",
        Some("t") => "Premio apuntado",
        Some("f") => "Comida apuntada",
        _ => return html! {},
    };
    let key = match code.split_at_checked(1) {
        Some(("f", rest)) => rest.split_once('-').map(|(pet, hm)| format!("f{pet}.{}.{}:{}", v.today, hm.get(..2).unwrap_or(""), hm.get(2..).unwrap_or(""))),
        _ => Some(code.to_string()),
    };
    html! {
        div.pt-undo role="status" {
            span { (icon("circle-check")) (what) }
            div.pt-undo-acts {
                @if let Some(href) = key.and_then(|k| edit_href(v, ret, &k)) { a.btn.ghost.sm href=(href) { "Cambiar hora" } }
                form method="post" action="/plugins/pets/undo" {
                    (v.ctx(ret)) input type="hidden" name="what" value=(code);
                    button.btn.ghost.sm { "Deshacer" }
                }
            }
        }
    }
}

struct HubRow {
    id: &'static str,
    note: String,
}

/// Every section in sight with what it holds now, instead of tabs that hide half of them.
fn hub(c: &Connection, v: &V, pet: &Pet, weights: &[data::Weight], tasks: &[Task]) -> rusqlite::Result<Markup> {
    let mut rows: Vec<HubRow> = Vec::new();
    let meds: Vec<Med> = data::meds(c, Some(pet.id))?.into_iter().filter(|m| m.active_on(&v.today) && !m.over(&v.today)).collect();
    let given = data::doses(c, &v.today)?;
    let next = meds.iter().flat_map(|m| m.slots().into_iter().filter(|s| !s.is_empty() && !given.contains_key(&(m.id, v.today.clone(), s.clone())))).min();
    let names = meds.iter().map(|m| m.title.as_str()).collect::<Vec<_>>().join(", ");
    rows.push(HubRow {
        id: "med",
        note: match (meds.len(), next) {
            (0, _) => "Sin medicación ahora".into(),
            (_, Some(t)) => format!("{names} · toca a las {t}"),
            _ => format!("{names} · hoy, todo dado"),
        },
    });
    if data::walks(&pet.kind) {
        let walks = data::walks_since(c, Some(pet.id), &v.today)?;
        rows.push(HubRow {
            id: "walks",
            note: match walks.first() {
                None => "Ninguno hoy".into(),
                Some(w) => format!("{} hoy · el último a las {}", walks.len(), &w.at[11..16]),
            },
        });
    }
    let treats = data::treats_since(c, pet.id, &v.today)?;
    rows.push(HubRow {
        id: "treats",
        note: match treats.first() {
            None => "Ninguno hoy".into(),
            Some(t) => format!("{} hoy · el último: {}", treats.len(), t.what.to_lowercase()),
        },
    });
    let health = data::health(c, pet.id, 1)?;
    rows.push(HubRow {
        id: "health",
        note: match health.first() {
            None => "Nada apuntado".into(),
            Some(h) => {
                let (_, k, _) = data::health_kind(&h.kind);
                let (_, m, _, _) = data::mood(&h.mood);
                format!("{k} · {}, {} a las {}", m.to_lowercase(), day_label(&h.at[..10], &v.today).to_lowercase(), &h.at[11..16])
            }
        },
    });
    rows.push(HubRow {
        id: "weight",
        note: match weights.last() {
            None => "Sin pesar todavía".into(),
            Some(w) => format!("{} · {}", kg(w.kg), rel(&w.day, &v.today)),
        },
    });
    let now = v.now_full();
    let visit = data::visits(c, Some(pet.id))?.into_iter().find(|x| x.at >= now);
    rows.push(HubRow {
        id: "vet",
        note: match visit {
            None => "Ninguna cita a la vista".into(),
            Some(x) => format!("{} · {} a las {}", x.title, day_label(&x.at[..10], &v.today).to_lowercase(), &x.at[11..16]),
        },
    });
    let due = tasks.iter().filter(|t| !t.due.is_empty()).min_by(|a, b| a.due.cmp(&b.due));
    rows.push(HubRow {
        id: "tasks",
        note: match due {
            None if tasks.is_empty() => "Baño, uñas, arenero… cuando toque".into(),
            None => format!("{} sin fecha", tasks.len()),
            Some(t) if t.due <= v.today => format!("{} · toca {}", t.title, rel(&t.due, &v.today)),
            Some(t) => format!("{} · {}", t.title, rel(&t.due, &v.today)),
        },
    });
    rows.push(HubRow { id: "diary", note: "Todo lo apuntado, día a día".into() });
    rows.push(HubRow { id: "profile", note: "Chip, seguro, comida y veterinario".into() });

    Ok(html! {
        section.card.pt-card {
            (sec_head("layout-grid", &format!("Todo de {}", pet.name), None))
            nav.pt-hub aria-label={ "Secciones de " (pet.name) } {
                @for r in &rows {
                    @let (_, label, ic) = SECTIONS.iter().find(|x| x.0 == r.id).copied().unwrap_or(SECTIONS[0]);
                    a.pt-hub-row href=(v.link(&format!("p={}&s={}", pet.id, r.id))) {
                        span.pt-hub-ico { (icon(ic)) }
                        span.pt-row-main { span.pt-row-title { (label) } span.pt-row-sub { (r.note) } }
                        span.pt-row-go aria-hidden="true" { (icon("chevron-right")) }
                    }
                }
            }
        }
    })
}

pub(super) fn overview(c: &Connection, v: &V, pet: &Pet, ret: &str, weights: &[data::Weight], tasks: &[Task]) -> rusqlite::Result<Markup> {
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
                @if !coming.is_empty() {
                    section.card.pt-card {
                        (sec_head("calendar-clock", "Próximamente", None))
                        (upcoming_list(v, &coming, &by_pet, false))
                    }
                }
                (hub(c, v, pet, weights, tasks)?)
                section.card.pt-card {
                    (sec_head("history", "Lo último", Some((link("diary"), "Todo"))))
                    @if events.is_empty() { p.pt-none { "Usa los botones de arriba para apuntar lo de cada día." } }
                    @else { (diary(v, &events, &by_pet, false, false, "feed-pet", ret)) }
                }
            }
        }
    })
}

pub(super) fn diary_section(c: &Connection, v: &V, pet: &Pet, ret: &str) -> rusqlite::Result<Markup> {
    let events = data::feed(c, Some(pet.id), 80)?;
    let by_pet: HashMap<i64, &Pet> = [(pet.id, pet)].into_iter().collect();
    Ok(html! {
        p.hint.pt-lead { "Todo lo que habéis apuntado de " (pet.name) ", y quién lo hizo. Toca una línea para cambiarla." }
        section.card.pt-card {
            @if events.is_empty() { (empty("history", "El diario está vacío", "Cada comida, paseo, toma o visita que apuntéis aparecerá aquí.")) }
            @else { (diary(v, &events, &by_pet, false, true, "feed-all", ret)) }
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
