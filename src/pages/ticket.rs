use super::*;

struct Ticket {
    id: i64,
    user: i64,
    status: String,
    merchant: Option<String>,
    date: Option<String>,
    total: Option<i64>,
    items: Vec<(String, i64)>,
    error: Option<String>,
}

pub async fn ticket_page(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let t: Option<Ticket> = c
        .query_row(
            "SELECT id,status,merchant,date,total,items,error,user_id FROM tickets WHERE id=?1",
            params![id],
            |r| {
                Ok(Ticket {
                    id: r.get(0)?,
                    status: r.get(1)?,
                    merchant: r.get(2)?,
                    date: r.get(3)?,
                    total: r.get(4)?,
                    items: r.get::<_, Option<String>>(5)?.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default(),
                    error: r.get(6)?,
                    user: r.get(7)?,
                })
            },
        )
        .optional()?;
    let Some(t) = t else { return Ok(StatusCode::NOT_FOUND.into_response()) };
    let m = month(&c, t.date.as_deref().map(|d| &d[..7]))?;
    let host = h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("nido.local").to_string();
    let v = View::load(&c, me, m, "expenses", host, None, None)?;
    drop(c);
    let body = v.shell(render(&v, &t), None);
    Ok(page("Ticket · Nido", "", body))
}

fn item_row(name: &str, price: Option<i64>) -> Markup {
    let (ic, color) = products::look(name);
    html! {
        li.row.item {
            span.tile.sm style={ "--c:" (color) } { (icon(ic)) }
            input.input.item-name type="text" name="item_name" value=(name) placeholder="Producto" maxlength="80" aria-label="Producto";
            input.input.item-price type="text" name="item_price" value=[price.map(eur_input)] placeholder="0,00" inputmode="decimal" aria-label="Precio";
            button.icon-btn.item-del type="button" aria-label="Quitar producto" { (icon("x")) }
        }
    }
}

fn render(v: &View, t: &Ticket) -> Markup {
    let back_link = v.link("expenses");
    let mine = t.user == v.me.id;
    html! {
        div.page-head {
            div {
                a.back href=(back_link) { (icon("chevron-left")) "Gastos" }
                @if mine && t.status != "done" {
                    h1 { "Revisar ticket" }
                    p { "Comprueba lo que hemos leído y corrígelo si hace falta antes de guardarlo" }
                } @else {
                    h1 { "Ticket" }
                    p { "Subido por " (if mine { "ti" } else { v.name_of(t.user) }) }
                }
            }
        }
        @match t.status.as_str() {
            "reading" => {
                section.card.reading data-refresh="3" {
                    span.spin { (icon("loader-circle")) }
                    h2 { "Leyendo el ticket…" }
                    p { "Tarda unos segundos. Esta página se actualizará sola; puedes seguir usando Nido mientras tanto." }
                    a.btn.outline.sm href={ "/tickets/" (t.id) } { (icon("refresh-cw")) "Actualizar" }
                }
            }
            _ => {
                (expenses::select_style())
                div.ticket {
                    figure.card.ticket-photo {
                        img src={ "/tickets/" (t.id) "/photo" } alt="Foto del ticket" loading="lazy";
                    }
                    @if t.status == "done" || !mine {
                        div.stack {
                            section.card {
                                div.card-head {
                                    span.stat-ico { (icon("circle-check")) }
                                    div { h2 { (t.merchant.as_deref().unwrap_or("Ticket")) } p {
                                        @if !mine && t.status != "done" { (v.name_of(t.user)) " todavía lo está revisando." }
                                        @else if mine { "Guardado como gasto. Para cambiarlo, edítalo desde la lista de gastos." }
                                        @else { "Guardado como gasto por " (v.name_of(t.user)) "." }
                                    } }
                                }
                                ul.legend {
                                    li { span.l { "Importe" } span.amt { (t.total.map(eur).unwrap_or_default()) } }
                                    @if let Some(d) = &t.date { li { span.l { "Fecha" } span.amt { (&d[8..]) "/" (&d[5..7]) "/" (&d[..4]) } } }
                                }
                                a.btn.outline.wide.top href=(back_link) { (icon("receipt")) "Ver gastos" }
                            }
                            @if !t.items.is_empty() {
                                @let sum: i64 = t.items.iter().map(|(_, p)| p).sum();
                                section.card {
                                    div.card-head {
                                        div { h2 { "Productos" } p { (t.items.len()) " producto" @if t.items.len() != 1 { "s" } " en este ticket" } }
                                        @if Some(sum) == t.total { (pill("ok", Some("check"), "Cuadra con el total")) }
                                        @else { (pill("warn", Some("circle-alert"), &format!("Suman {}", eur(sum)))) }
                                    }
                                    ul.rows.compact {
                                        @for (name, price) in &t.items {
                                            @let (ic, color) = products::look(name);
                                            li.row {
                                                span.tile.sm style={ "--c:" (color) } { (icon(ic)) }
                                                span.row-main { span.row-title { (name) } }
                                                span.amt { (eur(*price)) }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } @else {
                        form.stack method="post" action={ "/tickets/" (t.id) "/expense" } accept-charset="utf-8" {
                            @if let Some(err) = &t.error {
                                p.alert role="alert" { (icon("circle-alert")) "No hemos podido leer el ticket: " (err) " Puedes rellenar los datos a mano." }
                            }
                            section.card {
                                div.card-head { span.stat-ico { (icon("scan-line")) } div { h2 { "Datos del ticket" } p { "Se guardará como un gasto del día a día" } } }
                                div.form {
                                    div.field { label for="tm" { "Comercio" } input #tm.input type="text" name="description" value=[t.merchant.as_deref()] placeholder="Dónde fue la compra" required maxlength="100"; }
                                    div.two {
                                        div.field { label for="tt" { "Importe total" } input #tt.input type="text" name="amount" inputmode="decimal" value=[t.total.map(eur_input)] placeholder="0,00 €" required; }
                                        div.field { label for="td" { "Fecha" } input #td.input type="date" name="date" value=(t.date.clone().unwrap_or_else(|| v.today.clone())); }
                                    }
                                    div.field { label { "Categoría" } (expenses::category_select(None)) }
                                    div.field { label for="tn" { "Comentario " span.muted { "(opcional)" } } textarea #tn.input name="note" rows="2" maxlength="300" {} }
                                }
                            }
                            section.card {
                                div.card-head {
                                    div { h2 { "Productos" } p { "Corrige un nombre o un precio, quita lo que sobre o añade lo que falte" } }
                                    span.items-sum.pill data-total-of="tt" {}
                                }
                                ul.rows.items {
                                    @for (name, price) in &t.items { (item_row(name, Some(*price))) }
                                }
                                template #item-template { (item_row("", None)) }
                                button.btn.outline.sm.item-add type="button" { (icon("plus")) "Añadir producto" }
                            }
                            button.btn.primary.wide { (icon("check")) "Guardar como gasto" }
                            a.btn.danger.wide href="#discard" { (icon("trash-2")) "Descartar ticket" }
                        }
                    }
                }
                @if mine { div.modal #discard role="dialog" aria-modal="true" aria-labelledby="discard-t" {
                    a.scrim href="#_" aria-label="Cancelar" tabindex="-1" {}
                    div.dialog {
                        span.dialog-ico { (icon("trash-2")) }
                        h2 #discard-t { "¿Descartar este ticket?" }
                        p { "Se borrará la foto y lo que hemos leído. Si ya lo guardaste como gasto, el gasto se queda." }
                        div.dialog-actions {
                            a.btn.outline href="#_" { "Cancelar" }
                            form.inline method="post" action={ "/tickets/" (t.id) "/delete" } { button.btn.destructive { "Descartar" } }
                        }
                    }
                }
                }
            }
        }
    }
}
