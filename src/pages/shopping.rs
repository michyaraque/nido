use super::*;

fn item_row(v: &View, it: &Item) -> Markup {
    let (ic, color) = products::look(&it.name);
    html! {
        li.row.done[it.done] {
            form.inline method="post" action={ "/shopping/" (it.id) "/toggle" } {
                (v.ctx())
                button.check aria-label={ (if it.done { "Devolver a la lista: " } else { "Marcar como cogido: " }) (it.name) } { (icon("check")) }
            }
            span.tile.sm style={ "--c:" (color) } { (icon(ic)) }
            div.row-main {
                span.row-title { (it.name) }
                @if let Some(by) = it.added_by { span.row-sub { span.muted { "Añadido por " (v.name_of(by)) } } }
            }
            form.inline method="post" action={ "/shopping/" (it.id) "/delete" } {
                (v.ctx())
                button.icon-btn aria-label={ "Quitar " (it.name) " de la lista" } { (icon("x")) }
            }
        }
    }
}

pub fn render(v: &View) -> Markup {
    let pending: Vec<&Item> = v.shopping.iter().filter(|i| !i.done).collect();
    let done: Vec<&Item> = v.shopping.iter().filter(|i| i.done).collect();
    html! {
        (v.head("Lista de la compra", html! { "Compartida: lo que añade uno, lo ven los demás al momento" }))
        div.split {
            section.card #add-item {
                div.card-head { div { h2 { "Por comprar" } p { (pending.len()) " producto" @if pending.len() != 1 { "s" } " en la lista" } } }
                form.add-row method="post" action="/shopping" accept-charset="utf-8" {
                    (v.ctx())
                    label.sr for="ni" { "Productos" }
                    input #ni.input type="text" name="names" placeholder="Leche, pan, huevos…" required maxlength="300" autocomplete="off";
                    button.btn.primary { (icon("plus")) "Añadir" }
                }
                p.hint.top { "Separa varios productos con comas" }
                @if pending.is_empty() {
                    (empty("shopping-basket", "La lista está vacía", "Apunta lo que falta en casa y aparecerá aquí para todos.", None))
                }
                ul.rows { @for it in &pending { (item_row(v, it)) } }
            }
            div.stack {
                @if !v.suggestions.is_empty() {
                    section.card {
                        div.card-head { div { h2 { "Lo de siempre" } p { "Lo que más compráis. Toca para añadirlo." } } }
                        div.chips {
                            @for s in &v.suggestions {
                                @let (ic, color) = products::look(s);
                                form.inline method="post" action="/shopping" {
                                    (v.ctx())
                                    input type="hidden" name="names" value=(s);
                                    button.chip style={ "--c:" (color) } { (icon(ic)) (s) }
                                }
                            }
                        }
                    }
                }
                section.card {
                    div.card-head {
                        div { h2 { "En el carro" } p { "Lo que ya habéis cogido" } }
                        @if !done.is_empty() {
                            form method="post" action="/shopping/clear" {
                                (v.ctx())
                                button.btn.outline.sm { (icon("list-checks")) "Vaciar" }
                            }
                        }
                    }
                    @if done.is_empty() {
                        p.hint { "Marca los productos según los vayas cogiendo" }
                    }
                    ul.rows { @for it in &done { (item_row(v, it)) } }
                }
            }
        }
    }
}
