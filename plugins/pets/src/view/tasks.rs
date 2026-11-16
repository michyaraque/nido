//! Chores that come back every so many days, with ideas for each kind of pet.
use super::*;

pub(super) fn tasks_section(v: &V, pet: &Pet, tasks: &[Task], ret: &str, edit: Option<i64>) -> Markup {
    let (pending, done): (Vec<&Task>, Vec<&Task>) = tasks.iter().partition(|t| !t.due.is_empty());
    let ideas: Vec<(usize, &(&str, &str, i64))> = data::suggestions(&pet.kind).iter().enumerate().filter(|(_, s)| !tasks.iter().any(|t| t.title == s.0)).collect();
    let editing = edit.and_then(|id| tasks.iter().find(|t| t.id == id));
    html! {
        div.pt-bar {
            p.hint { "Lo que toca cada cierto tiempo. Al marcarla como hecha, vuelve a aparecer cuando le toque otra vez." }
            a.btn.primary href="#task-new" { (icon("plus")) "Nueva tarea" }
        }
        @if !ideas.is_empty() {
            div.pt-ideas {
                span.pt-label { "Ideas para " (pet.name) }
                div.pt-idea-list {
                    @for (i, (t, ic, n)) in &ideas {
                        a.pt-idea href=(v.link(&format!("{ret}&nt={i}#task-new"))) { span.pt-idea-ico { (icon(ic)) } span.pt-idea-text { (t) small { (every(*n)) } } span.pt-idea-add { (icon("plus")) } }
                    }
                }
            }
        }
        section.card.pt-card {
            @if pending.is_empty() { (empty("list-todo", "Sin tareas", "Añade el baño, cortarle las uñas, limpiar el arenero… lo que necesite.")) }
            @else { (clamp("tasks", pending.iter().map(|t| task_row(v, t, pet, ret)).collect(), false)) }
        }
        @if !done.is_empty() {
            details.card.pt-card.pt-old {
                summary { (icon("archive")) "Hechas" span.pt-count { (done.len()) } }
                div.pt-list { @for t in &done { (task_row(v, t, pet, ret)) } }
            }
        }
        @if let Some(t) = editing { (task_modal(v, pet, ret, Some(t), None)) }
    }
}

pub(super) fn task_modal(v: &V, pet: &Pet, ret: &str, t: Option<&Task>, idea: Option<&(&str, &str, i64)>) -> Markup {
    let (id, title, action) = match t {
        Some(t) => ("task-edit", "Editar tarea", format!("/plugins/pets/task/{}", t.id)),
        None => ("task-new", "Nueva tarea", "/plugins/pets/task".to_string()),
    };
    let name = t.map(|t| t.title.as_str()).or(idea.map(|i| i.0));
    let ic = t.map(|t| t.icon.as_str()).or(idea.map(|i| i.1)).unwrap_or("sparkles");
    let every = t.map(|t| t.every).or(idea.map(|i| i.2)).filter(|n| *n > 0);
    let due = t.map(|t| t.due.as_str()).filter(|d| !d.is_empty()).unwrap_or(v.today.as_str());
    modal(id, title, html! {
        form.form method="post" action=(action) {
            (v.ctx(ret))
            input type="hidden" name="pet" value=(pet.id);
            div.field {
                label for={ (id) "-title" } { "Qué hay que hacer" }
                input.input id={ (id) "-title" } name="title" required maxlength="60" autocomplete="off" placeholder="Baño, limpiar el arenero…" value=[name];
            }
            fieldset.field.pt-fieldset {
                legend { "Icono" }
                div.pick.pt-icons {
                    @for i in data::TASK_ICONS { label { input type="radio" name="icon" value=(i) checked[*i == ic] aria-label=(i); span { (icon(i)) } } }
                }
            }
            div.two {
                div.field {
                    label for={ (id) "-every" } { "Repetir cada (días)" }
                    input.input type="number" id={ (id) "-every" } name="every" min="1" max="730" inputmode="numeric" placeholder="Una vez" value=[every];
                }
                div.field {
                    label for={ (id) "-due" } { "Próxima vez" }
                    input.input type="date" id={ (id) "-due" } name="due" value=(due);
                }
            }
            p.hint.pt-hint-up { "1 cada día, 7 cada semana, 30 cada mes. Vacío para una sola vez." }
            div.field {
                label for={ (id) "-note" } { "Notas " span.pt-opt { "(opcional)" } }
                textarea.input id={ (id) "-note" } name="note" rows="2" maxlength="1000" placeholder="Con su champú, en la peluquería…" { (t.map_or("", |t| t.note.as_str())) }
            }
            button.btn.primary.wide { (icon("check")) "Guardar" }
        }
        @if let Some(t) = t { div.pt-danger { (delete_form(v, &format!("/plugins/pets/task/{}/delete", t.id), ret, "Borrar tarea")) } }
    })
}
