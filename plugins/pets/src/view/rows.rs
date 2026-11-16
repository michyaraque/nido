//! Rows shared by several sections: a dose to tick, a chore, a vet visit.
use super::*;

pub(super) fn dose_btn(v: &V, m: &Med, day: &str, slot: &str, doses: &Given, ret: &str) -> Markup {
    let given = doses.get(&(m.id, day.to_string(), slot.to_string()));
    let label = if slot.is_empty() { "Toma" } else { slot };
    let late = given.is_none() && day == v.today && !slot.is_empty() && slot < v.time.as_str();
    html! {
        form.pt-dose-f method="post" action="/plugins/pets/dose" {
            (v.ctx(ret))
            input type="hidden" name="med" value=(m.id);
            input type="hidden" name="day" value=(day);
            input type="hidden" name="slot" value=(slot);
            @match given {
                Some((by, at)) => button.pt-dose.on aria-pressed="true" title={ "Se la dio " (v.name(*by)) " a las " (at) ". Toca para desmarcar." } {
                    (icon("check")) span { (label) } (v.avatar(*by))
                },
                None => button.pt-dose.late[late] aria-pressed="false" title="Toca cuando se la hayáis dado" {
                    (icon(if late { "bell-ring" } else { "clock" })) span { (label) }
                },
            }
        }
    }
}

pub(super) fn task_row(v: &V, t: &Task, pet: &Pet, ret: &str) -> Markup {
    let late = !t.due.is_empty() && t.due < v.today;
    let today = t.due == v.today;
    html! {
        div.pt-task style=(tint(pet)) {
            span.pt-task-ico { (icon(&t.icon)) }
            div.pt-row-main {
                a.pt-row-title href=(v.link(&format!("p={}&s=tasks&e={}#task-edit", pet.id, t.id))) { (t.title) }
                span.pt-row-sub {
                    @if t.due.is_empty() { "Hecha" }
                    @else if late { span.pt-late { "Tocaba " (rel(&t.due, &v.today)) } }
                    @else if today { b { "Hoy" } }
                    @else { (day_label(&t.due, &v.today)) " · " (rel(&t.due, &v.today)) }
                    @if t.every > 0 { " · " (every(t.every)) }
                }
                @if let Some((d, by)) = &t.last { span.pt-row-sub.faint { "Última vez " (day_label(d, &v.today).to_lowercase()) ", " (v.name(*by).to_lowercase()) } }
            }
            @if !t.due.is_empty() {
                form method="post" action={ "/plugins/pets/task/" (t.id) "/done" } {
                    (v.ctx(ret))
                    button.pt-done.ready[late || today] aria-label={ "Hecho: " (t.title) } title="Hecho" { (icon("check")) span { "Hecho" } }
                }
            }
        }
    }
}

pub(super) fn visit_row(v: &V, x: &Visit, pet: &Pet) -> Markup {
    let d = &x.at[..10];
    let past = x.at < v.now_full();
    html! {
        a.pt-up.past[past] href=(v.link(&format!("p={}&s=vet&e={}#visit-edit", pet.id, x.id))) style=(tint(pet)) {
            (cal(d))
            div.pt-row-main {
                span.pt-row-title { @if x.title.is_empty() { "Veterinario" } @else { (x.title) } }
                span.pt-row-sub {
                    (day_label(d, &v.today)) " a las " (&x.at[11..16])
                    @if !x.place.is_empty() { " · " (x.place) }
                }
            }
            @if !past { span.pt-when { (rel(d, &v.today)) } }
        }
    }
}
