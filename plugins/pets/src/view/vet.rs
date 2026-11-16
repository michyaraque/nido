//! The vet: appointments and the health booklet (vaccines, deworming, tests).
use super::*;

pub(super) fn vet_section(c: &Connection, v: &V, pet: &Pet, ret: &str, edit: Option<i64>, rec: Option<i64>) -> rusqlite::Result<Markup> {
    let all = data::visits(c, Some(pet.id))?;
    let records = data::records(c, Some(pet.id))?;
    let now_s = v.now_full();
    let (coming, past): (Vec<&Visit>, Vec<&Visit>) = all.iter().partition(|x| x.at >= now_s);
    let editing = edit.and_then(|id| all.iter().find(|x| x.id == id));
    let editing_rec = rec.and_then(|id| records.iter().find(|r| r.id == id));
    Ok(html! {
        @if !pet.vet_name.is_empty() || !pet.vet_phone.is_empty() {
            div.pt-vetcard {
                span.pt-task-ico { (icon("stethoscope")) }
                div.pt-row-main {
                    span.pt-row-title { @if pet.vet_name.is_empty() { "Su veterinario" } @else { (pet.vet_name) } }
                    @if !pet.vet_phone.is_empty() { span.pt-row-sub { (pet.vet_phone) } }
                }
                @if !pet.vet_phone.is_empty() { a.btn.outline.sm href={ "tel:" (pet.vet_phone.replace(' ', "")) } { (icon("phone")) "Llamar" } }
            }
        }
        div.pt-home {
            section.card.pt-card {
                div.pt-sec-head {
                    h2 { span.pt-sec-ico { (icon("calendar-clock")) } "Citas" }
                    a.btn.primary.sm href="#visit-new" { (icon("calendar-plus")) "Nueva cita" }
                }
                @if coming.is_empty() { p.pt-none { "No hay ninguna cita apuntada. Os avisaremos antes de cada una." } }
                @else {
                    div.pt-list {
                        @for x in &coming {
                            (visit_row(v, x, pet))
                            @if x.remind > 0 {
                                p.pt-remind { (icon("bell-ring")) (REMIND.iter().find(|r| r.0 == x.remind).map_or("", |r| r.1)) }
                            }
                        }
                    }
                }
                @if !past.is_empty() {
                    details.pt-history {
                        summary { "Historial · " (past.len()) }
                        div.pt-list {
                            @for x in past.iter().rev() {
                                (visit_row(v, x, pet))
                                @if !x.note.is_empty() { p.pt-note { (x.note) } }
                            }
                        }
                    }
                }
            }
            section.card.pt-card {
                div.pt-sec-head {
                    h2 { span.pt-sec-ico { (icon("shield-plus")) } "Cartilla" }
                    a.btn.outline.sm href="#record-new" { (icon("plus")) "Añadir" }
                }
                @if records.is_empty() {
                    p.pt-none { "Vacunas, desparasitaciones y pruebas, con la fecha de la próxima. Os avisaremos una semana antes." }
                } @else {
                    (clamp("records", records.iter().map(|r| record_row(v, r, pet, ret)).collect(), false))
                }
            }
        }
        @if let Some(x) = editing { (visit_modal(v, pet, ret, Some(x))) }
        @if let Some(r) = editing_rec { (record_modal(v, pet, ret, Some(r))) }
    })
}

pub(super) fn record_row(v: &V, r: &Record, pet: &Pet, ret: &str) -> Markup {
    let (_, kname, kic) = data::record_kind(&r.kind);
    let soon = !r.next.is_empty() && r.next <= add_days(&v.today, 7);
    html! {
        a.pt-record href=(v.link(&format!("{ret}&rec={}#record-edit", r.id))) style=(tint(pet)) {
            span.pt-task-ico { (icon(kic)) }
            div.pt-row-main {
                span.pt-row-title { (r.title()) }
                span.pt-row-sub { @if !r.name.is_empty() { (kname) " · " } (day_label(&r.day, &v.today)) }
            }
            @if !r.next.is_empty() {
                span.pt-when.warn[soon] title={ "Próxima: " (day_label(&r.next, &v.today)) } {
                    @if r.next < v.today { "Vencida" } @else { "Próxima " (rel(&r.next, &v.today)) }
                }
            }
        }
    }
}

pub(super) fn record_modal(v: &V, pet: &Pet, ret: &str, r: Option<&Record>) -> Markup {
    let (id, title, action) = match r {
        Some(r) => ("record-edit", "Editar en la cartilla", format!("/plugins/pets/record/{}", r.id)),
        None => ("record-new", "Añadir a la cartilla", "/plugins/pets/record".to_string()),
    };
    let kind = r.map_or("vaccine", |r| r.kind.as_str());
    modal(id, title, html! {
        form.form method="post" action=(action) {
            (v.ctx(ret))
            input type="hidden" name="pet" value=(pet.id);
            fieldset.field.pt-fieldset {
                legend { "Qué" }
                div.pt-chips {
                    @for (k, name, ic) in data::RECORDS {
                        label.pt-chip { input type="radio" name="kind" value=(k) checked[*k == kind]; span { (icon(ic)) (name) } }
                    }
                }
            }
            div.field {
                label for={ (id) "-name" } { "Nombre " span.pt-opt { "(opcional)" } }
                input.input id={ (id) "-name" } name="name" maxlength="60" autocomplete="off" placeholder="Rabia, polivalente, pipeta…" value=[r.map(|r| &r.name).filter(|n| !n.is_empty())];
            }
            div.two {
                div.field {
                    label for={ (id) "-day" } { "Cuándo se puso" }
                    input.input type="date" id={ (id) "-day" } name="day" max=(v.today) value=(r.map_or(v.today.as_str(), |r| r.day.as_str()));
                }
                div.field {
                    label for={ (id) "-every" } { "Repetir" }
                    select.input id={ (id) "-every" } name="every" {
                        option value="" { "No se repite" }
                        @for (n, label) in [(30, "Al mes"), (91, "A los 3 meses"), (182, "A los 6 meses"), (365, "Al año")] { option value=(n) { (label) } }
                    }
                }
            }
            div.field {
                label for={ (id) "-next" } { "O la fecha de la próxima " span.pt-opt { "(opcional)" } }
                input.input type="date" id={ (id) "-next" } name="next" value=[r.map(|r| &r.next).filter(|n| !n.is_empty())];
                p.hint { "Os avisamos una semana antes." }
            }
            div.field {
                label for={ (id) "-note" } { "Notas " span.pt-opt { "(opcional)" } }
                textarea.input id={ (id) "-note" } name="note" rows="2" maxlength="1000" placeholder="Lote, clínica, reacción…" { (r.map_or("", |r| r.note.as_str())) }
            }
            button.btn.primary.wide { (icon("check")) "Guardar" }
        }
        @if let Some(r) = r { div.pt-danger { (delete_form(v, &format!("/plugins/pets/record/{}/delete", r.id), ret, "Borrar")) } }
    })
}

pub(super) fn visit_modal(v: &V, pet: &Pet, ret: &str, x: Option<&Visit>) -> Markup {
    let (id, title, action) = match x {
        Some(x) => ("visit-edit", "Editar cita", format!("/plugins/pets/visit/{}", x.id)),
        None => ("visit-new", "Nueva cita en el veterinario", "/plugins/pets/visit".to_string()),
    };
    let at = x.map_or_else(|| format!("{}T10:00", add_days(&v.today, 1)), |x| x.at.replace(' ', "T"));
    let remind = x.map_or(1440, |x| x.remind);
    let place = x.map(|x| &x.place).filter(|t| !t.is_empty()).or((x.is_none() && !pet.vet_name.is_empty()).then_some(&pet.vet_name));
    modal(id, title, html! {
        form.form method="post" action=(action) {
            (v.ctx(ret))
            input type="hidden" name="pet" value=(pet.id);
            div.field {
                label for={ (id) "-at" } { "Día y hora" }
                input.input type="datetime-local" id={ (id) "-at" } name="at" required value=(at);
            }
            div.field {
                label for={ (id) "-title" } { "Motivo " span.pt-opt { "(opcional)" } }
                input.input id={ (id) "-title" } name="title" maxlength="60" autocomplete="off" placeholder="Vacuna, revisión, análisis…" value=[x.map(|x| &x.title).filter(|t| !t.is_empty())];
            }
            div.field {
                label for={ (id) "-place" } { "Dónde " span.pt-opt { "(opcional)" } }
                input.input id={ (id) "-place" } name="place" maxlength="80" autocomplete="off" placeholder="Clínica del barrio" value=[place];
            }
            div.field {
                label for={ (id) "-remind" } { "Avisar" }
                select.input id={ (id) "-remind" } name="remind" {
                    @for (m, label) in REMIND { option value=(m) selected[*m == remind] { (label) } }
                }
            }
            div.field {
                label for={ (id) "-note" } { "Notas " span.pt-opt { "(opcional)" } }
                textarea.input id={ (id) "-note" } name="note" rows="2" maxlength="1000" placeholder="Ir en ayunas, llevar la cartilla… o lo que os dijeron" { (x.map_or("", |x| x.note.as_str())) }
            }
            button.btn.primary.wide { (icon("check")) "Guardar" }
        }
        @if let Some(x) = x { div.pt-danger { (delete_form(v, &format!("/plugins/pets/visit/{}/delete", x.id), ret, "Borrar cita")) } }
    })
}
