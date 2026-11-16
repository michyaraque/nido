//! Treats and snacks: what each pet got outside its meals, and who gave it, so nobody gives it twice.
use super::*;

pub(super) fn treats_section(c: &Connection, v: &V, pet: &Pet, ret: &str, edit: Option<i64>) -> rusqlite::Result<Markup> {
    let list = data::treats_since(c, pet.id, &add_days(&v.today, -29))?;
    let editing = edit.and_then(|id| list.iter().find(|t| t.id == id));
    Ok(html! {
        div.pt-bar {
            p.hint { "Premios y snacks fuera de sus comidas, para que nadie se los dé dos veces." }
            a.btn.primary href="#treat" { (icon("plus")) "Apuntar premio" }
        }
        section.card.pt-card {
            (sec_head("cookie", "Últimos 30 días", None))
            @if list.is_empty() {
                p.pt-none { "Sin premios este mes. Pulsa «Premio» cuando le des algo." }
            } @else {
                (clamp("treat-list", day_groups(v, &list, |t| t.at.as_str(), |t| html! {
                    a.pt-walk.pt-editable href=(v.link(&format!("{ret}&e={}#treat-edit", t.id))) aria-label={ "Editar el premio de las " (&t.at[11..16]) } {
                        (v.faces(std::slice::from_ref(&t.by)))
                        div.pt-row-main {
                            span.pt-row-title { (t.what) @if !t.amount.is_empty() { " · " (t.amount) } }
                            span.pt-row-sub { (v.name(t.by)) " · a las " (&t.at[11..16]) }
                        }
                        span.pt-row-go aria-hidden="true" { (icon("pencil")) }
                    }
                }), true))
            }
        }
        @if let Some(t) = editing { (treat_modal(v, pet, ret, Some(t), &[])) }
    })
}

/// New: the usual treats as one-tap buttons, and a form for anything else. Edit: the form alone.
pub(super) fn treat_modal(v: &V, pet: &Pet, ret: &str, t: Option<&data::Treat>, names: &[String]) -> Markup {
    let (id, title, action) = match t {
        Some(t) => ("treat-edit", "Editar premio".to_string(), format!("/plugins/pets/treat/{}", t.id)),
        None => ("treat", format!("Premio para {}", pet.name), "/plugins/pets/treat".to_string()),
    };
    let gave = |p: i64| t.map_or(p == v.me.id, |t| t.by == p);
    modal(id, &title, html! {
        @if t.is_none() && !names.is_empty() {
            form.pt-treat-quick method="post" action=(action) style=(tint(pet)) {
                (v.ctx(ret))
                input type="hidden" name="pet" value=(pet.id); input type="hidden" name="quick" value="1";
                p.pt-treat-lead { "Toca lo que le has dado:" }
                div.pt-chips {
                    @for n in names { button.pt-treat-pick name="what" value=(n) { (icon("cookie")) (n) } }
                }
            }
            p.pt-treat-or { "O apunta otra cosa" }
        }
        form.form method="post" action=(action) {
            (v.ctx(ret))
            input type="hidden" name="pet" value=(pet.id);
            @if t.is_none() { input type="hidden" name="quick" value="1"; }
            div.two {
                div.field {
                    label for={ (id) "-what" } { "Qué" }
                    input.input id={ (id) "-what" } name="what" maxlength="60" required autocomplete="off" placeholder="Hueso, zanahoria…" value=[t.map(|t| &t.what)];
                }
                div.field {
                    label for={ (id) "-amount" } { "Cuánto " span.pt-opt { "(opcional)" } }
                    input.input id={ (id) "-amount" } name="amount" maxlength="40" autocomplete="off" placeholder="1 trozo, 20 g…" value=[t.map(|t| &t.amount).filter(|a| !a.is_empty())];
                }
            }
            @if v.me.people.len() > 1 {
                fieldset.field.pt-fieldset {
                    legend { "Quién se lo ha dado" }
                    div.pt-chips {
                        @for p in &v.me.people {
                            label.pt-chip.pt-who { input type="radio" name="by" value=(p.id) checked[gave(p.id)]; span { (v.avatar(p.id)) (if p.id == v.me.id { "Yo" } else { p.name.as_str() }) } }
                        }
                    }
                }
            }
            div.field {
                label for={ (id) "-at" } { "Cuándo" }
                input.input id={ (id) "-at" } type="datetime-local" name="at" value=(t.map_or_else(|| v.now_local(), |t| t.at.replace(' ', "T")));
            }
            button.btn.primary.wide { (icon("check")) @if t.is_some() { "Guardar" } @else { "Apuntar" } }
        }
        @if let Some(t) = t { div.pt-danger { (delete_form(v, &format!("/plugins/pets/treat/{}/delete", t.id), ret, "Borrar premio")) } }
    })
}
