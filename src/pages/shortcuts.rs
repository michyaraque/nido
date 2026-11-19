//! Ajustes → Atajos: the house's tags, and the dialog that says what each one does.
use super::*;
use crate::shortcuts::{self as sc, store::Tag, Action, MAX_STEPS};

#[derive(Deserialize, Default)]
struct Q {
    e: Option<i64>,       // the tag whose edit dialog is open
    add: Option<String>,  // "source|action" to start a new tag with (from a plugin's "Crear atajo")
    code: Option<String>, // a tag already written, scanned before it had an atajo
}

pub async fn render(db: &Db, v: &View, h: &HeaderMap, raw: &str) -> Result<Markup, AppErr> {
    let q: Q = serde_urlencoded::from_str(raw).unwrap_or_default();
    let me = User { id: v.me.id, name: v.me.name.clone(), salary: v.me.salary, token: v.me.token.clone() };
    let catalog = sc::catalog(db, &me).await;
    let tags = sc::store::all(&lock(db))?;
    Ok(view(v, &tags, &catalog, &sc::origin(h), &q))
}

fn view(v: &View, tags: &[Tag], catalog: &[Action], origin: &str, q: &Q) -> Markup {
    let editing = q.e.and_then(|id| tags.iter().find(|t| t.id == id));
    let main = html! {
        section.set-group #tags aria-labelledby="tags-t" {
            div.sc-top {
                div.set-head { h2 #tags-t { "Atajos" } p { "Acerca el móvil a una etiqueta y se apunta solo, a nombre de quien la lee." } }
                @if !tags.is_empty() { a.btn.primary.sm href={ (v.link("shortcuts")) "#tag-new" } { (icon("plus")) "Nuevo atajo" } }
            }
            @if tags.is_empty() {
                (intro(v))
            } @else {
                ul.sc-list role="list" { @for t in tags { li { (card(v, t)) } } }
            }
        }
        // One list of every action per dialog is heavy: while editing, "Nuevo atajo" reloads without it.
        @match editing {
            Some(t) => (dialog(v, Some(t), catalog, origin, q)),
            None => (dialog(v, None, catalog, origin, q)),
        }
    };
    settings::layout(v, html! { "Etiquetas NFC y enlaces que hacen cosas en Nido" }, main)
}

fn intro(v: &View) -> Markup {
    let step = |n: u8, title: &str, hint: &str| html! { li { span.sc-n { (n) } div { b { (title) } small { (hint) } } } };
    html! {
        div.card.sc-intro {
            span.sc-intro-ico { (icon("nfc")) }
            h3 { "Apunta cosas acercando el móvil" }
            p { "Pega una etiqueta NFC junto al bol del perro, en la nevera o en la puerta. Al acercar el móvil se apunta lo que elijas, a nombre de quien la lee." }
            ol.sc-how {
                (step(1, "Crea el atajo", "Elige qué hace: una cosa o varias."))
                (step(2, "Grábalo en la etiqueta", "Desde el móvil, en un momento."))
                (step(3, "Acerca el móvil", "Se apunta solo, y se puede deshacer."))
            }
            a.btn.primary href={ (v.link("shortcuts")) "#tag-new" } { (icon("plus")) "Crear el primer atajo" }
        }
    }
}

fn last_used(v: &View, last: &Option<(String, i64)>) -> String {
    let Some((at, who)) = last else { return "Sin usar todavía".into() };
    let day = &at[..10.min(at.len())];
    let time = at.get(11..16).unwrap_or("");
    let when = if day == v.today {
        format!("hoy a las {time}")
    } else if day == v.yesterday {
        format!("ayer a las {time}")
    } else {
        let wd = weekday(day);
        day_label(day, wd, &v.today, &v.yesterday).to_lowercase()
    };
    let who = if *who == v.me.id { "ti".to_string() } else { v.name_of(*who).to_string() };
    format!("Último uso {when}, por {who}")
}

fn card(v: &View, t: &Tag) -> Markup {
    let n = t.steps.len();
    let what = if t.choose { "Pregunta qué apuntar".to_string() } else if n == 1 { "Hace 1 cosa".into() } else { format!("Hace {n} cosas") };
    let lead = if t.choose { "list-checks" } else { t.steps.first().map_or("nfc", |s| s.icon.as_str()) };
    html! {
        a.card.sc-tag href={ (v.link("shortcuts")) "&e=" (t.id) "#tag-edit" } aria-label={ "Editar " (t.name) } {
            div.sc-tag-top {
                span.sc-tag-ico { (icon(lead)) }
                span.sc-tag-name { b { (t.name) } small { (what) @if t.asks() { " · pide confirmar" } } }
                span.sc-tag-go aria-hidden="true" { (icon("pencil")) }
            }
            ul.sc-tag-steps {
                @for s in &t.steps { li { (icon(&s.icon)) span { (s.label) } } }
            }
            p.sc-tag-foot { (last_used(v, &t.last)) }
        }
    }
}

/// Options grouped as people think of them: "Compra", "Mascotas · Luca"…
fn groups(catalog: &[Action]) -> Vec<(String, Vec<&Action>)> {
    let mut out: Vec<(String, Vec<&Action>)> = Vec::new();
    for a in catalog {
        let title = match (a.source, a.group.is_empty()) {
            ("core", _) => a.group.clone(),
            (s, true) => sc::source_name(s).to_string(),
            (s, false) => format!("{} · {}", sc::source_name(s), a.group),
        };
        match out.iter_mut().find(|g| g.0 == title) {
            Some(g) => g.1.push(a),
            None => out.push((title, vec![a])),
        }
    }
    out
}

/// One row of "Qué hace": the action and, when it needs one, the short text that goes with it.
fn act_row(i: usize, groups: &[(String, Vec<&Action>)], current: Option<(String, &str, &str)>) -> Markup {
    let key = current.as_ref().map(|c| c.0.clone()).unwrap_or_default();
    let found = groups.iter().flat_map(|g| g.1.iter()).find(|a| a.key() == key);
    let ask = found.and_then(|a| a.ask.clone());
    let arg = current.as_ref().map_or("", |c| c.2);
    html! {
        li.sc-act {
            span.sc-n aria-hidden="true" { (i + 1) }
            div.sc-act-body {
                span.sc-select {
                    select.input name="act" data-asks aria-label={ "Acción " (i + 1) } required[i == 0] {
                        option value="" selected[key.is_empty()] { @if i == 0 { "Elige qué hace" } @else { "Nada más" } }
                        @for (title, list) in groups {
                            optgroup label=(title) {
                                @for a in list {
                                    option value=(a.key()) selected[a.key() == key] data-ask=[a.ask.as_deref()] { (a.name) }
                                }
                            }
                        }
                        // Saved, but its plugin is off right now: keep it choosable so saving keeps it.
                        @if found.is_none() && !key.is_empty() {
                            optgroup label="No disponible ahora" {
                                option value=(key) selected { (current.as_ref().map_or("", |c| c.1)) }
                            }
                        }
                    }
                    (icon("chevron-down"))
                }
                input.input.sc-arg type="text" name="arg" value=(arg) maxlength="60" autocomplete="off" data-ask-field
                    placeholder=(ask.as_deref().unwrap_or("Detalle")) aria-label={ "Detalle de la acción " (i + 1) } required[ask.is_some()];
            }
        }
    }
}

fn form(v: &View, t: Option<&Tag>, catalog: &[Action], q: &Q) -> Markup {
    let groups = groups(catalog);
    let action = match t {
        Some(t) => format!("/shortcuts/{}#tag-edit", t.id),
        None => "/shortcuts#tag-edit".to_string(),
    };
    let fid = if t.is_some() { "te" } else { "tn" };
    let start = q.add.clone().filter(|_| t.is_none());
    let choose = t.is_some_and(|t| t.choose);
    html! {
        form.form.sc-form method="post" action=(action) accept-charset="utf-8" {
            (v.ctx())
            @if t.is_none() {
                @if let Some(code) = q.code.as_deref().filter(|c| sc::store::valid_code(c)) { input type="hidden" name="code" value=(code); }
            }
            div.field {
                label for={ (fid) "-name" } { "Nombre" }
                input.input id={ (fid) "-name" } type="text" name="name" maxlength="40" autocomplete="off" placeholder="Junto al bol de Luca" value=[t.map(|t| t.name.as_str())];
                p.hint { "Dónde está la etiqueta o para qué es." }
            }
            fieldset.field.sc-acts {
                legend { "Qué hace" }
                ol {
                    @for i in 0..MAX_STEPS {
                        @let cur = match (t, i, &start) {
                            (Some(t), i, _) => t.steps.get(i).map(|s| (format!("{}|{}", s.source, s.action), s.label.as_str(), s.arg.as_str())),
                            (None, 0, Some(k)) => Some((k.clone(), "", "")),
                            _ => None,
                        };
                        (act_row(i, &groups, cur))
                    }
                }
                p.hint { "Hasta 5 cosas, en este orden." }
            }
            fieldset.field {
                legend { "Al acercar el móvil" }
                div.chips role="radiogroup" aria-label="Al acercar el móvil" {
                    label.chip-radio { input type="radio" name="mode" value="all" checked[!choose]; span { "Hacerlo todo" } }
                    label.chip-radio { input type="radio" name="mode" value="choose" checked[choose]; span { "Elegir una cosa" } }
                }
            }
            div.sc-confirm {
                div.set-item-txt { label for={ (fid) "-confirm" } { b { "Preguntar antes" } } small { "Para lo que enciende o cambia algo en casa." } }
                label.toggle { input id={ (fid) "-confirm" } type="checkbox" name="confirm" checked[t.is_some_and(|t| t.confirm)]; span {} }
            }
            button.btn.primary.wide { (icon("check")) @if t.is_some() { "Guardar cambios" } @else { "Crear atajo" } }
        }
    }
}

/// Copy the link onto a tag: from Nido itself where the browser can write NFC (Chrome on Android,
/// over https), or with any NFC app on any phone. Reading it needs nothing: phones open the link.
fn write_box(t: &Tag, origin: &str) -> Markup {
    let url = format!("{origin}/t/{}", t.code);
    html! {
        div.sc-write {
            div.sc-write-head {
                span.sc-tag-ico { (icon("nfc")) }
                div.set-item-txt { b { "Grabar en la etiqueta" } small { "La etiqueta solo lleva este enlace: puedes cambiar lo que hace sin volver a grabarla." } }
            }
            button.btn.primary.wide type="button" data-nfc-write=(url) hidden { (icon("nfc")) "Grabar con este móvil" }
            p.hint.sc-nfc-status data-nfc-status aria-live="polite" {}
            p.hint data-nfc-none { "Este navegador no puede grabar etiquetas. Se puede desde Chrome en Android, con Nido abierto en " b { "https://" } ". Si no, copia el enlace y grábalo con cualquier app de NFC." }
            div.inline-field.grow {
                input.input.mono type="text" readonly value=(url) aria-label="Enlace de la etiqueta" onfocus="this.select()";
                button.btn.outline.sm type="button" data-copy=(url) { (icon("copy")) "Copiar" }
            }
            details.steps {
                summary { (icon("chevron-right")) "Grabarla con una app de NFC" }
                ol {
                    li { "Instala una app para grabar etiquetas, como " b { "NFC Tools" } " (gratis, para Android y iPhone)." }
                    li { "Copia el enlace de arriba." }
                    li { "En la app: " b { "Escribir → Añadir un registro → URL" } ", pega el enlace y pulsa " b { "Escribir" } "." }
                    li { "Acerca la etiqueta al móvil hasta que confirme que está grabada." }
                }
            }
        }
    }
}

fn dialog(v: &View, t: Option<&Tag>, catalog: &[Action], origin: &str, q: &Q) -> Markup {
    let id = if t.is_some() { "tag-edit" } else { "tag-new" };
    html! {
        div.modal id=(id) role="dialog" aria-modal="true" aria-labelledby={ (id) "-t" } {
            a.scrim href="#_" aria-label="Cerrar" tabindex="-1" {}
            div.dialog.dialog-wide {
                div.dialog-top {
                    h2 id={ (id) "-t" } { @if let Some(t) = t { (t.name) } @else { "Nuevo atajo" } }
                    a.icon-btn href="#_" aria-label="Cerrar" { (icon("x")) }
                }
                @if let Some(t) = t { (write_box(t, origin)) }
                (form(v, t, catalog, q))
                @if let Some(t) = t {
                    details.sc-del {
                        summary.btn.ghost.sm { (icon("trash-2")) "Borrar atajo" }
                        form method="post" action={ "/shortcuts/" (t.id) "/delete" } {
                            (v.ctx())
                            span { "La etiqueta dejará de hacer cosas hasta que le des otro atajo." }
                            button.btn.danger.sm { "Sí, borrar" }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_keep_order_and_name_their_source() {
        let a = |source: &'static str, group: &str, id: &str| Action { source, id: id.into(), group: group.into(), name: id.into(), icon: "zap".into(), confirm: false, ask: None };
        let cat = [a("core", "Compra", "shop"), a("pets", "Luca", "meal:1"), a("core", "Compra", "x"), a("pets", "Luca", "walk:1")];
        let g = groups(&cat);
        let got: Vec<_> = g.iter().map(|(t, l)| (t.as_str(), l.len())).collect();
        assert_eq!(got, [("Compra", 2), ("Mascotas · Luca", 2)]);
    }
}
