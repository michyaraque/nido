//! The pet's card: what to keep at hand, and the form that edits it.
use super::*;

pub(super) fn fact(ic: &str, label: &str, value: Markup) -> Markup {
    html! { div.pt-fact { span.pt-fact-ico { (icon(ic)) } div { dt { (label) } dd { (value) } } } }
}

pub(super) fn profile_section(v: &V, pet: &Pet, weight: Option<f64>) -> Markup {
    let none = || html! { span.pt-opt { "Sin apuntar" } };
    let ret = format!("p={}&s=profile", pet.id);
    let (_, kname, kic) = data::kind(&pet.kind);
    html! {
        div.pt-bar {
            p.hint { "Lo que conviene tener a mano: chip, alergias, comida, seguro y su veterinario." }
            a.btn.outline href="#pet-edit" { (icon("pencil")) "Editar ficha" }
        }
        div.pt-home {
            section.card.pt-card {
                (sec_head("id-card", "Datos", None))
                dl.pt-facts {
                    (fact(kic, "Especie", html! { (kname) @if !pet.breed.is_empty() { " · " (pet.breed) } }))
                    (fact("paw-print", "Sexo", pet.sex_label().map_or_else(none, |s| html! { (s) })))
                    (fact("cake", "Nació", if pet.birth.is_empty() { none() } else { html! { (day_label(&pet.birth, &v.today)) @if let Some(a) = age(&pet.birth, &v.today) { " · " (a) } } }))
                    (fact("weight", "Peso", weight.map_or_else(none, |w| html! { (kg(w)) })))
                    (fact("scan-line", "Microchip", if pet.chip.is_empty() { none() } else { html! { span.pt-mono { (pet.chip) } } }))
                    (fact("circle-alert", "Alergias", if pet.allergies.is_empty() { html! { "Ninguna conocida" } } else { html! { b.pt-warn-text { (pet.allergies) } } }))
                    (fact("shield-plus", "Seguro", if pet.insurance.is_empty() { none() } else { html! { (pet.insurance) } }))
                }
            }
            div.pt-side {
                section.card.pt-card {
                    (sec_head("utensils", "Comida", None))
                    @if pet.food.is_empty() && pet.meals.is_empty() {
                        p.pt-none { "Apunta qué come y a qué horas: saldrán en «Hoy» para marcar quién se lo ha dado. " a href="#pet-edit" { "Ponerlo" } }
                    } @else {
                        dl.pt-facts.one {
                            @if !pet.food.is_empty() { (fact("shopping-bag", "Come", html! { (pet.food) @if !pet.portion.is_empty() { " · " (pet.portion) } })) }
                            @if !pet.meal_slots().is_empty() { (fact("clock", "A las", html! { (pet.meal_slots().join(" · ")) })) }
                        }
                        @if !pet.food.is_empty() {
                            form.pt-shop method="post" action="/shopping/many" {
                                input type="hidden" name="item" value=(pet.food);
                                input type="hidden" name="next" value=(v.link(&ret));
                                button.btn.outline.sm { (icon("shopping-cart")) "Se acaba: a la compra" }
                            }
                        }
                    }
                }
                section.card.pt-card {
                    (sec_head("stethoscope", "Veterinario", None))
                    @if pet.vet_name.is_empty() && pet.vet_phone.is_empty() {
                        p.pt-none { "Guarda su clínica y su teléfono para llamar con un toque. " a href="#pet-edit" { "Ponerlo" } }
                    } @else {
                        div.pt-vetcard.flat {
                            div.pt-row-main {
                                span.pt-row-title { @if pet.vet_name.is_empty() { "Su veterinario" } @else { (pet.vet_name) } }
                                @if !pet.vet_phone.is_empty() { span.pt-row-sub { (pet.vet_phone) } }
                            }
                            @if !pet.vet_phone.is_empty() { a.btn.primary.sm href={ "tel:" (pet.vet_phone.replace(' ', "")) } { (icon("phone")) "Llamar" } }
                        }
                    }
                }
                @if !pet.note.is_empty() {
                    section.card.pt-card {
                        (sec_head("pencil", "Notas", None))
                        p.pt-note { (pet.note) }
                    }
                }
            }
        }
    }
}

pub(super) fn pet_modal(v: &V, p: Option<&Pet>, weight: Option<f64>) -> Markup {
    let (id, title, action) = match p {
        Some(p) => ("pet-edit", "Editar ficha", format!("/plugins/pets/pet/{}", p.id)),
        None => ("pet-new", "Nueva mascota", "/plugins/pets/pet".to_string()),
    };
    let kind = p.map_or("dog", |p| p.kind.as_str());
    let color = p.map_or(data::COLORS[0].1, |p| data::color(&p.color));
    let sex = p.map_or("", |p| p.sex.as_str());
    let ret = p.map(|p| format!("p={}", p.id)).unwrap_or_default();
    let meals: Vec<String> = p.map(|p| p.meal_slots()).unwrap_or_default();
    let val = |f: fn(&Pet) -> &String| p.map(f).filter(|s| !s.is_empty()).cloned();
    modal(id, title, html! {
        form.form method="post" action=(action) enctype="multipart/form-data" accept-charset="utf-8" {
            (v.ctx(&ret))
            div.pt-photo-pick style={ "--pc:" (color) } {
                @match p {
                    Some(p) => (face(p, "md")),
                    None => span.pt-face.pt-art.md aria-hidden="true" { (icon("camera")) },
                }
                div {
                    label.btn.outline.sm.pt-file {
                        input type="file" name="photo" accept="image/*";
                        (icon("camera")) @if p.is_some_and(|p| p.photo > 0) { "Cambiar foto" } @else { "Poner foto" }
                    }
                    @if p.is_some_and(|p| p.photo > 0) {
                        label.pt-check { input type="checkbox" name="remove_photo" value="1"; "Quitar la foto" }
                    }
                }
            }
            div.field {
                label for={ (id) "-name" } { "Nombre" }
                input.input id={ (id) "-name" } name="name" required maxlength="40" autocomplete="off" placeholder="Luna" value=[p.map(|p| &p.name)];
            }
            fieldset.field.pt-fieldset {
                legend { "Qué es" }
                div.pt-kinds {
                    @for (k, name, ic) in data::KINDS {
                        label { input type="radio" name="kind" value=(k) checked[*k == kind]; span { (icon(ic)) (name) } }
                    }
                }
            }
            div.two {
                div.field {
                    label for={ (id) "-kg" } { "Peso (kg) " span.pt-opt { "(opcional)" } }
                    input.input id={ (id) "-kg" } name="kg" inputmode="decimal" autocomplete="off" placeholder="4,5" pattern="[0-9]+([.,][0-9]+)?" value=[weight.map(|k| format!("{k}").replace('.', ","))];
                }
                div.field {
                    label for={ (id) "-birth" } { "Nació " span.pt-opt { "(opcional)" } }
                    input.input type="date" id={ (id) "-birth" } name="birth" max=(v.today) value=[val(|p| &p.birth)];
                }
            }
            fieldset.field.pt-fieldset {
                legend { "Color de su ficha" }
                div.swatches {
                    @for (name, hex) in data::COLORS {
                        label.swatch title=(name) { input type="radio" name="color" value=(hex) checked[*hex == color] aria-label=(name); span style={ "--c:" (hex) } {} }
                    }
                }
            }

            details.pt-more-fields open[p.is_some_and(|p| !p.meals.is_empty() || !p.food.is_empty())] {
                summary { span.pt-more-ico { (icon("utensils")) } span { "Comida" small { "Sus horas salen en «Hoy» para marcarlas" } } }
                div.pt-more-body {
                    div.two {
                        div.field {
                            label for={ (id) "-food" } { "Qué come" }
                            input.input id={ (id) "-food" } name="food" maxlength="60" autocomplete="off" placeholder="Pienso de salmón" value=[val(|p| &p.food)];
                        }
                        div.field {
                            label for={ (id) "-portion" } { "Ración" }
                            input.input id={ (id) "-portion" } name="portion" maxlength="40" autocomplete="off" placeholder="1 cacito, 120 g…" value=[val(|p| &p.portion)];
                        }
                    }
                    fieldset.field.pt-fieldset {
                        legend { "A qué horas come" }
                        div.pt-times { @for i in 0..4 { input.input type="time" name="meal" aria-label={ "Comida " (i + 1) } value=[meals.get(i)]; } }
                        p.hint { "Si en una hora nadie marca una comida, avisamos a la casa." }
                    }
                }
            }
            details.pt-more-fields open[p.is_some_and(|p| !p.sex.is_empty() || !p.chip.is_empty() || !p.allergies.is_empty() || !p.breed.is_empty())] {
                summary { span.pt-more-ico { (icon("id-card")) } span { "Más datos" small { "Sexo, raza, chip, alergias, seguro" } } }
                div.pt-more-body {
                    fieldset.field.pt-fieldset {
                        legend { "Sexo" }
                        div.pt-chips {
                            @for (k, name) in [("", "Sin decir"), ("m", "Macho"), ("f", "Hembra")] {
                                label.pt-chip { input type="radio" name="sex" value=(k) checked[k == sex]; span { (name) } }
                            }
                        }
                        label.pt-check { input type="checkbox" name="neutered" value="1" checked[p.is_some_and(|p| p.neutered)]; "Está esterilizado" }
                    }
                    div.two {
                        div.field {
                            label for={ (id) "-breed" } { "Raza" }
                            input.input id={ (id) "-breed" } name="breed" maxlength="40" autocomplete="off" placeholder="Mestizo, siamés…" value=[val(|p| &p.breed)];
                        }
                        div.field {
                            label for={ (id) "-chip" } { "Microchip" }
                            input.input id={ (id) "-chip" } name="chip" maxlength="30" autocomplete="off" inputmode="numeric" placeholder="941000…" value=[val(|p| &p.chip)];
                        }
                    }
                    div.field {
                        label for={ (id) "-allergies" } { "Alergias" }
                        input.input id={ (id) "-allergies" } name="allergies" maxlength="120" autocomplete="off" placeholder="Pollo, picadura de pulga…" value=[val(|p| &p.allergies)];
                        p.hint { "Salen destacadas arriba de su página." }
                    }
                    div.field {
                        label for={ (id) "-insurance" } { "Seguro" }
                        input.input id={ (id) "-insurance" } name="insurance" maxlength="80" autocomplete="off" placeholder="Compañía y número de póliza" value=[val(|p| &p.insurance)];
                    }
                }
            }
            details.pt-more-fields open[p.is_some_and(|p| !p.vet_name.is_empty() || !p.vet_phone.is_empty())] {
                summary { span.pt-more-ico { (icon("stethoscope")) } span { "Su veterinario" small { "Para llamar con un toque" } } }
                div.pt-more-body {
                    div.two {
                        div.field {
                            label for={ (id) "-vet" } { "Clínica o nombre" }
                            input.input id={ (id) "-vet" } name="vet_name" maxlength="60" autocomplete="off" placeholder="Clínica del barrio" value=[val(|p| &p.vet_name)];
                        }
                        div.field {
                            label for={ (id) "-phone" } { "Teléfono" }
                            input.input type="tel" id={ (id) "-phone" } name="vet_phone" maxlength="20" autocomplete="off" inputmode="tel" placeholder="600 000 000" value=[val(|p| &p.vet_phone)];
                        }
                    }
                }
            }
            div.field {
                label for={ (id) "-note" } { "Notas " span.pt-opt { "(opcional)" } }
                textarea.input id={ (id) "-note" } name="note" rows="2" maxlength="1000" placeholder="Lo que le gusta, lo que le da miedo, trucos…" { (p.map_or("", |p| p.note.as_str())) }
            }
            button.btn.primary.wide { (icon("check")) "Guardar" }
        }
        @if let Some(p) = p {
            div.pt-danger { (delete_form(v, &format!("/plugins/pets/pet/{}/delete", p.id), "", &format!("Borrar a {}", p.name))) }
        }
    })
}
