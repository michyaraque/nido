use super::*;

/// The category picker is a <details> with radios; its button shows the chosen option through :has(),
/// which needs one selector per option.
fn select_css() -> String {
    let mut css = String::from(".select:has(input[value=\"\"]:checked) .sel-auto");
    for c in CATS {
        css += &format!(",.select:has(input[value=\"{0}\"]:checked) .sel-{0}", c.id);
    }
    css + "{display:flex}"
}

pub fn select_style() -> Markup {
    html! { style { (PreEscaped(select_css())) } }
}

pub fn category_select(chosen: Option<&str>) -> Markup {
    html! {
        details.select {
            summary.input.select-btn {
                span.sel.sel-auto { (icon("wand-sparkles")) "Automática" }
                @for c in CATS { span class={ "sel sel-" (c.id) } { span.dot style={ "background:" (c.color) } {} (c.label) } }
                span.chev { (icon("chevron-right")) }
            }
            div.select-pop role="radiogroup" aria-label="Categoría" {
                label.opt {
                    input type="radio" name="category" value="" checked[chosen.is_none()];
                    span.tile.sm style="--c:#2f6bff" { (icon("wand-sparkles")) }
                    span.opt-txt { "Automática" small { "Según el comercio" } }
                    span.opt-check { (icon("check")) }
                }
                @for c in CATS {
                    label.opt {
                        input type="radio" name="category" value=(c.id) checked[chosen == Some(c.id)];
                        span.tile.sm style={ "--c:" (c.color) } { (icon(c.icon)) }
                        span.opt-txt { (c.label) }
                        span.opt-check { (icon("check")) }
                    }
                }
            }
        }
    }
}

fn expense_menu(v: &View, e: &Exp) -> Markup {
    html! {
        details.menu {
            summary.icon-btn aria-label={ "Opciones de " (e.description) } { (icon("ellipsis")) }
            div.pop {
                p.pop-title { "Editar gasto" }
                form method="post" action={ "/expenses/" (e.id) } accept-charset="utf-8" {
                    (v.ctx())
                    div.field { label { "Concepto" } input.input type="text" name="description" value=(e.description) required maxlength="100"; }
                    div.two {
                        div.field { label { "Importe" } input.input type="text" name="amount" inputmode="decimal" value=(eur_input(e.amount)) required; }
                        div.field { label { "Fecha" } input.input type="date" name="date" value=(e.date); }
                    }
                    div.field { label { "Categoría" } (category_select(e.category.as_deref())) }
                    div.field { label { "Comentario" } textarea.input name="note" rows="2" maxlength="300" { (e.note.as_deref().unwrap_or("")) } }
                    button.btn.primary.wide { "Guardar cambios" }
                }
                a.btn.danger.wide href={ "#del-exp-" (e.id) } { (icon("trash-2")) "Borrar gasto" }
            }
        }
        div.modal id={ "del-exp-" (e.id) } role="dialog" aria-modal="true" aria-labelledby={ "del-exp-t-" (e.id) } {
            a.scrim href="#_" aria-label="Cancelar" tabindex="-1" {}
            div.dialog {
                span.dialog-ico { (icon("trash-2")) }
                h2 id={ "del-exp-t-" (e.id) } { "¿Borrar «" (e.description) "»?" }
                p { "Se quitará de tus gastos de " (v.month_lc()) ". No se puede deshacer." }
                div.dialog-actions {
                    a.btn.outline href="#_" { "Cancelar" }
                    form.inline method="post" action={ "/expenses/" (e.id) "/delete" } { (v.ctx()) button.btn.destructive { "Borrar gasto" } }
                }
            }
        }
    }
}

pub fn render(v: &View) -> Markup {
    let spent: i64 = v.shown.iter().map(|e| e.amount).sum();
    let whose = match v.who.as_str() {
        "me" => String::new(),
        "all" => " de toda la casa".into(),
        id => format!(" de {}", v.name_of(id.parse().unwrap_or(0))),
    };
    let month_lc = v.month_lc();

    let mut by_cat: Vec<(&'static Cat, i64)> = Vec::new();
    for e in v.shown.iter().filter(|e| e.amount > 0) {
        let cat = look(&e.description, e.category.as_deref()).cat;
        match by_cat.iter_mut().find(|(c, _)| c.id == cat.id) {
            Some((_, x)) => *x += e.amount,
            None => by_cat.push((cat, e.amount)),
        }
    }
    by_cat.sort_by(|a, b| b.1.cmp(&a.1));
    let cat_max = by_cat.first().map(|x| x.1).unwrap_or(1).max(1);

    let mut days: Vec<(&str, i64, Vec<&Exp>)> = Vec::new();
    for e in &v.shown {
        match days.last_mut() {
            Some((d, total, items)) if *d == e.date => {
                *total += e.amount;
                items.push(e)
            }
            _ => days.push((&e.date, e.amount, vec![e])),
        }
    }
    let default_date = if v.is_now() { v.today.clone() } else { format!("{}-01", v.m) };

    html! {
        (select_style())
        (v.head("Gastos del día a día", html! { (v.shown.len()) " movimiento" @if v.shown.len() != 1 { "s" } (whose) " en " (month_lc) " · " (eur(spent)) }))
        div.split.rev {
            div.stack {
                @if !v.tickets.is_empty() {
                    section.card {
                        div.card-head { span.stat-ico { (icon("receipt")) } div { h2 { "Tickets por revisar" } p { "Ábrelos para comprobar lo leído y guardarlos como gasto." } } }
                        ul.rows.compact {
                            @for t in &v.tickets {
                                li.row {
                                    span.tile.sm style="--c:#2f6bff" { (icon("receipt")) }
                                    a.row-main href={ "/tickets/" (t.id) } {
                                        span.row-title { (t.merchant.as_deref().unwrap_or("Ticket sin leer")) }
                                        span.row-sub {
                                            @match t.status.as_str() {
                                                "reading" => (pill("accent", Some("loader-circle"), "Leyendo…")),
                                                "failed" => (pill("warn", Some("circle-alert"), "Rellenar a mano")),
                                                _ => (pill("ok", Some("check"), "Listo para revisar")),
                                            }
                                        }
                                    }
                                    @if let Some(total) = t.total { span.amt { (eur(total)) } }
                                    span.chev-r { (icon("chevron-right")) }
                                }
                            }
                        }
                    }
                }
                section.card #new {
                    div.card-head { div { h2 { "Registrar gasto" } p { "Escríbelo, o pulsa " (icon("scan-line")) " para hacer una foto al ticket y lo rellenamos por ti." } } }
                    // The photo goes in a form of its own (forms cannot nest); the button in Concepto opens it through its label.
                    form.scan method="post" action="/tickets" enctype="multipart/form-data" {
                        input #scan-photo type="file" name="photo" accept="image/*" required;
                    }
                    form.form method="post" action="/expenses" accept-charset="utf-8" {
                        (v.ctx())
                        div.field {
                            label for="d" { "Concepto" }
                            div.input-action {
                                input #d.input type="text" name="description" placeholder="Mercadona, cena, farmacia…" required maxlength="100";
                                label.scan-btn for="scan-photo" title="Escanear un ticket" aria-label="Escanear un ticket con una foto" { (icon("scan-line")) }
                            }
                            @if !ocr::installed() { p.hint { "El lector de tickets no está instalado: se guardará la foto y escribirás los datos a mano." } }
                        }
                        div.two {
                            div.field { label for="a" { "Importe" } input #a.input type="text" name="amount" placeholder="0,00 €" inputmode="decimal" required; }
                            div.field { label for="f" { "Fecha" } input #f.input type="date" name="date" value=(default_date) min={ (v.m) "-01" } max={ (v.m) "-" (format!("{:02}", v.ndays)) }; }
                        }
                        div.field { label { "Categoría" } (category_select(None)) }
                        div.field { label for="n" { "Comentario " span.muted { "(opcional)" } } textarea #n.input name="note" rows="2" maxlength="300" placeholder="Para qué fue, con quién…" {} }
                        button.btn.primary.wide { (icon("plus")) "Registrar gasto" }
                    }
                }
                @if !by_cat.is_empty() {
                    section.card {
                        div.card-head { div { h2 { "Por categoría" } p { "Dónde se ha ido tu dinero en " (month_lc) "." } } }
                        ul.cats {
                            @for (cat, x) in by_cat.iter().take(8) {
                                li {
                                    span.tile.sm style={ "--c:" (cat.color) } { (icon(cat.icon)) }
                                    span.cat-l { (cat.label) }
                                    span.track { span style={ "width:" (x * 100 / cat_max) "%;background:" (cat.color) } {} }
                                    span.amt { (eur(*x)) }
                                }
                            }
                            li.cats-total { span.cat-l { "Total" } span.amt { (eur(by_cat.iter().map(|(_, x)| x).sum::<i64>())) } }
                        }
                    }
                }
            }
            section.card {
                div.card-head { div { h2 { "Movimientos" } p { "Los últimos primero. Los del iPhone llevan su etiqueta." } } }
                @if v.share_expenses && v.users.len() > 1 {
                    nav.seg aria-label="De quién" {
                        a.on[v.who == "me"] href={ (v.link("expenses")) "&who=me" } { "Míos" }
                        @for u in v.users.iter().filter(|u| u.id != v.me.id) {
                            a.on[v.who == u.id.to_string()] href={ (v.link("expenses")) "&who=" (u.id) } { "De " (u.name) }
                        }
                        a.on[v.who == "all"] href={ (v.link("expenses")) "&who=all" } { "De todos" }
                    }
                }
                @if v.shown.is_empty() {
                    (empty("receipt", &format!("Sin gastos en {month_lc}"), "Regístralos aquí o deja que el Atajo del iPhone lo haga al pagar.", None))
                }
                @for (date, total, items) in &days {
                    div.day-head { span { (day_label(date, items[0].weekday, &v.today, &v.yesterday)) } span.amt { (eur(*total)) } }
                    ul.rows {
                        @for e in items {
                            @let l = look(&e.description, e.category.as_deref());
                            li.row {
                                (badge(&l, &e.description))
                                div.row-main {
                                    span.row-title { (l.brand.filter(|_| e.source == "shortcut").unwrap_or(&e.description)) }
                                    span.row-sub {
                                        span.muted { (l.cat.label) }
                                        @if e.source == "shortcut" { (pill("accent", Some("smartphone"), "iPhone")) }
                                        @if let Some(tid) = e.ticket { a.pill.accent href={ "/tickets/" (tid) } { (icon("receipt")) "Ticket" } }
                                    }
                                    @if let Some(note) = &e.note { span.note { (note) } }
                                }
                                span.amt { (eur(-e.amount)) }
                                @if e.user_id == v.me.id { (expense_menu(v, e)) } @else { span.owner title={ "Gasto de " (v.name_of(e.user_id)) } { (v.avatar(e.user_id)) } }
                            }
                        }
                    }
                }
            }
        }
    }
}
