//! The plugin's front page: a card per pet in its own colour, and today's agenda for the whole house.
use super::*;

pub(super) fn home(c: &Connection, v: &V, p: &Params) -> rusqlite::Result<Markup> {
    let all = data::pets(c)?;
    let filter = p.get("kind").map(String::as_str).filter(|k| all.iter().any(|x| x.kind == *k));
    let pets: Vec<Pet> = all.iter().filter(|x| filter.is_none_or(|k| x.kind == k)).map(Pet::clone).collect();
    let ids: Vec<i64> = pets.iter().map(|x| x.id).collect();
    let by_pet: HashMap<i64, &Pet> = pets.iter().map(|x| (x.id, x)).collect();
    let weights = data::last_weights(c)?;
    let items = agenda_items(c, v, &pets)?;
    let ahead = upcoming(c, v, &pets, 60)?;
    let soon = add_days(&v.today, 21);
    let coming: Vec<&Up> = ahead.iter().filter(|u| u.day <= soon).take(8).collect();
    let events: Vec<data::Event> = data::feed(c, None, 40)?.into_iter().filter(|e| ids.contains(&e.pet_id)).take(20).collect();
    let walks = data::walks_since(c, None, &v.today)?;
    let left_today = items.iter().filter(|a| a.post.is_some() && a.done.is_none()).count();

    Ok(html! {
        div.page-head.pt-greet {
            div {
                p.pt-eyebrow { (greeting(&v.time)) ", " (first_name(&v.me.name)) }
                h1 { "Mascotas" }
                p {
                    @if all.is_empty() { "Sus comidas, medicinas, paseos y citas, para toda la casa." }
                    @else if left_today == 0 { (summary(&all)) " Hoy está todo hecho." }
                    @else { (summary(&all)) " Quedan " b { (left_today) } @if left_today == 1 { " cosa" } @else { " cosas" } " por hacer hoy." }
                }
            }
        }
        @if all.is_empty() {
            section.pt-welcome {
                div.pt-welcome-art aria-hidden="true" { @for k in ["dog", "cat", "bird", "rabbit", "fish"] { span { (icon(k)) } } }
                h2 { "¿Quién vive en casa?" }
                p { "Añade a vuestra mascota con su foto. Después podréis apuntar quién le ha dado de comer o la ha paseado, sus medicinas, su salud, sus vacunas y cuándo le toca el veterinario o el baño." }
                a.btn.primary href="#pet-new" { (icon("paw-print")) "Añadir la primera" }
                ul.pt-welcome-list {
                    li { (icon("utensils")) "Comidas y paseos, con quién lo hizo" }
                    li { (icon("pill")) "Medicinas por días, toma a toma" }
                    li { (icon("bell-ring")) "Avisos de citas, vacunas y tareas" }
                }
            }
        } @else {
            (kind_filter(v, &all, filter))
            div.pt-home {
                (agenda_card(v, &items, &by_pet, "", true, html! { "Nada pendiente hoy. Pon las horas de comer en la ficha de cada mascota para verlas aquí." }))
                div.pt-side {
                    div.pt-pets {
                        @for pet in &pets {
                            (pet_card(v, pet, &items, walks.iter().filter(|w| w.pet_id == pet.id).count(), weights.get(&pet.id).copied(), ahead.iter().find(|u| u.pet == pet.id)))
                        }
                        a.pt-pet-add href="#pet-new" {
                            span.pt-pet-add-ico { (icon("plus")) }
                            b { "Añadir mascota" }
                            small { "Perro, gato, pájaro, pez…" }
                        }
                    }
                    section.card.pt-card {
                        (sec_head("calendar-clock", "Próximamente", None))
                        @if coming.is_empty() { p.pt-none { "Nada en las próximas tres semanas." } }
                        @else { (upcoming_list(v, coming.iter().copied(), &by_pet, true)) }
                    }
                    section.card.pt-card {
                        (sec_head("history", "Lo último en casa", None))
                        @if events.is_empty() { p.pt-none { "Aquí saldrá lo que vaya apuntando cada uno." } }
                        @else { (diary(v, &events, &by_pet, true, false, "feed-home", "")) }
                    }
                }
            }
        }
        (pet_modal(v, None, None))
        (entry_modal(c, v, p.get("x"), "")?)
    })
}

fn pet_card(v: &V, pet: &Pet, items: &[Ag], walked: usize, weight: Option<f64>, next: Option<&Up>) -> Markup {
    let (_, kname, kic) = data::kind(&pet.kind);
    let mine: Vec<&Ag> = items.iter().filter(|a| a.pet == pet.id && a.post.is_some()).collect();
    let left = mine.iter().filter(|a| a.done.is_none()).count();
    let birthday = next_birthday(&pet.birth, &v.today).as_deref() == Some(v.today.as_str());
    html! {
        a.pt-pet href=(v.link(&format!("p={}", pet.id))) style=(tint(pet)) {
            div.pt-pet-body {
                div {
                    h3 { (pet.name) @if birthday { span.pt-cake title="Hoy es su cumpleaños" { (icon("cake")) } } }
                    p.pt-pet-meta {
                        (kname)
                        @if let Some(a) = age(&pet.birth, &v.today) { " · " (a) }
                        @if let Some(w) = weight { " · " (kg(w)) }
                    }
                }
                div.pt-meters {
                    @if !mine.is_empty() {
                        span.pt-meter.full[left == 0] {
                            (icon(if left == 0 { "circle-check" } else { "list-todo" }))
                            @if left == 0 { "Todo hecho" } @else { (left) @if left == 1 { " pendiente" } @else { " pendientes" } }
                        }
                    }
                    @if data::walks(&pet.kind) {
                        span.pt-meter.full[walked > 0] { (icon("footprints")) (walked) @if walked == 1 { " paseo" } @else { " paseos" } }
                    }
                    @if mine.is_empty() && !data::walks(&pet.kind) {
                        span.pt-meter.full { (icon("check")) "Nada para hoy" }
                    }
                }
                @if let Some(u) = next {
                    p.pt-pet-next { (icon(u.icon)) span { (u.title) " · " b { (rel(&u.day, &v.today)) } } }
                }
            }
            div.pt-pet-pic {
                (face(pet, "fill"))
                span.pt-kind-badge title=(kname) { (icon(kic)) }
            }
        }
    }
}

/// Filter by kind of animal, only when the house has more than one kind.
fn kind_filter(v: &V, pets: &[Pet], current: Option<&str>) -> Markup {
    let kinds: Vec<(&str, &str, &str, usize)> = data::KINDS
        .iter()
        .map(|(k, name, ic)| (*k, *name, *ic, pets.iter().filter(|p| p.kind == *k).count()))
        .filter(|k| k.3 > 0)
        .collect();
    if kinds.len() < 2 {
        return html! {};
    }
    html! {
        nav.pt-kind-filter aria-label="Filtrar por animal" {
            a.on[current.is_none()] href=(v.link("")) aria-current=[current.is_none().then_some("page")] {
                span.pt-kf-ico { (icon("paw-print")) } span { "Todas" }
            }
            @for (k, name, ic, n) in &kinds {
                a.on[current == Some(*k)] href=(v.link(&format!("kind={k}"))) aria-current=[(current == Some(*k)).then_some("page")] {
                    span.pt-kf-ico { (icon(ic)) } span { (plural(name, *n)) }
                }
            }
        }
    }
}

fn plural(name: &str, n: usize) -> String {
    match (name, n) {
        (_, 1) => name.to_string(),
        ("Pez", _) => "Peces".into(),
        ("Roedor", _) => "Roedores".into(),
        ("Otro", _) => "Otros".into(),
        _ => format!("{name}s"),
    }
}

fn greeting(time: &str) -> &'static str {
    match time {
        t if ("06:00".."13:00").contains(&t) => "Buenos días",
        t if ("13:00".."21:00").contains(&t) => "Buenas tardes",
        _ => "Buenas noches",
    }
}

fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

pub(super) fn summary(pets: &[Pet]) -> String {
    let names: Vec<&str> = pets.iter().map(|p| p.name.as_str()).collect();
    match names.as_slice() {
        [one] => format!("Todo lo de {one}, para toda la casa."),
        [rest @ .., last] if rest.len() < 4 => format!("{} y {last}, cuidados entre todos.", rest.join(", ")),
        _ => format!("{} mascotas, cuidadas entre todos.", names.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words() {
        assert_eq!(greeting("08:30"), "Buenos días");
        assert_eq!(greeting("13:00"), "Buenas tardes");
        assert_eq!(greeting("02:00"), "Buenas noches");
        assert_eq!(plural("Pez", 2), "Peces");
        assert_eq!(plural("Gato", 3), "Gatos");
        assert_eq!(plural("Perro", 1), "Perro");
        assert_eq!(first_name("Ana María"), "Ana");
    }
}
