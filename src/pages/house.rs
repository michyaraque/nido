use super::*;

pub fn render(v: &View) -> Markup {
    let shared = v.shared();
    let transfers = v.transfers();
    let month_lc = v.month_lc();
    let new_bill = "#new-bill".to_string();
    html! {
        (v.head("La casa", html! {
            "Cuentas comunes de " (month_lc) ", repartidas "
            (match v.split { "avg3" => "según lo cobrado (media de 3 meses)", "equal" => "a partes iguales", "custom" => "con porcentajes fijos", _ => "según el ingreso fijo de cada uno" })
        }))
        div.stats {
            (stat("house", "Total de la casa", money(shared), html! { (v.bills.iter().filter(|b| b.owner.is_none()).count()) " cuentas comunes" }))
            @for (i, u) in v.users.iter().enumerate() {
                @let share = v.house_share(u);
                (stat("users", &format!("Paga {}", u.name), money(share), html! {
                    span.dot class={ "p" (i) } {}
                    @if shared > 0 { (format!("{:.1}", share as f64 * 100.0 / shared as f64).replace('.', ",")) " % de la casa" }
                    @else { (format!("{:.1}", v.share_pct(u.id)).replace('.', ",")) " % del reparto" }
                }))
            }
        }
        div.split {
            section.card {
                div.card-head { div { h2 { "Cuentas comunes" } p { "Marca cada una cuando la pagues. Se apunta quién la pagó." } } }
                @if shared == 0 {
                    (empty("house", "Todavía no hay cuentas comunes", "Añade el alquiler, la luz o internet y se repartirán solas.", Some((&new_bill, "Añadir cuenta"))))
                }
                ul.rows { @for b in v.bills.iter().filter(|b| b.owner.is_none()) { (bills::row(v, b)) } }
                (bills::new_modal(v))
            }
            section.card {
                div.card-head { div { h2 { "Quién debe a quién" } p { "Según lo que ha pagado cada uno de la casa este mes" } } }
                @if v.users.len() < 2 {
                    (empty("users", "Estás tú solo en casa", "Añade a alguien en Ajustes → La casa para repartir las cuentas.", Some((&format!("{}#people", v.link("household")), "Añadir persona"))))
                } @else if transfers.is_empty() {
                    (empty("circle-check", "Estáis en paz", "Nadie debe nada por las cuentas de la casa de este mes.", None))
                } @else if let [(from, to, amount)] = transfers.as_slice() {
                    div.owe {
                        div.owe-who { (v.avatar(*from)) span.owe-arrow { (icon("arrow-right")) } (v.avatar(*to)) }
                        p.owe-txt { (owe_text(v, *from, *to)) }
                        (money(*amount))
                        (settle_form(v, *from, *to, *amount, true))
                        @if *from == v.me.id || *to == v.me.id { p.hint.center { "Quedará apuntado y el saldo volverá a cero" } }
                    }
                } @else {
                    ul.rows.owes {
                        @for (from, to, amount) in &transfers {
                            li.row {
                                div.owe-who.sm { (v.avatar(*from)) span.owe-arrow { (icon("arrow-right")) } (v.avatar(*to)) }
                                div.row-main { span.row-title { (owe_text(v, *from, *to)) } }
                                span.amt { (eur(*amount)) }
                                (settle_form(v, *from, *to, *amount, false))
                            }
                        }
                    }
                    p.hint { "Cada uno marca lo suyo cuando lo devuelva." }
                }
                @if !v.settles.is_empty() {
                    p.label-sm { "Devoluciones de " (month_lc) }
                    ul.rows.compact {
                        @for s in &v.settles {
                            li.row {
                                span.row-main { span.row-title { (v.name_of(s.from)) " devolvió a " (v.name_of(s.to)) } }
                                span.amt { (eur(s.amount)) }
                                form.inline method="post" action={ "/settlements/" (s.id) "/delete" } {
                                    (v.ctx())
                                    button.icon-btn aria-label="Borrar esta devolución" { (icon("x")) }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn owe_text(v: &View, from: i64, to: i64) -> String {
    if from == v.me.id {
        format!("Le debes a {}", v.name_of(to))
    } else if to == v.me.id {
        format!("{} te debe", v.name_of(from))
    } else {
        format!("{} le debe a {}", v.name_of(from), v.name_of(to))
    }
}

/// "Devuelto" button, only on debts that involve me (each person settles their own).
fn settle_form(v: &View, from: i64, to: i64, amount: i64, wide: bool) -> Markup {
    let (other, signed) = if to == v.me.id { (from, amount) } else { (to, -amount) };
    html! {
        @if from == v.me.id || to == v.me.id {
            form.inline method="post" action="/settlements" {
                (v.ctx())
                input type="hidden" name="other_id" value=(other);
                input type="hidden" name="amount" value=(signed);
                @if wide { button.btn.primary.wide { (icon("hand-coins")) "Marcar como devuelto" } }
                @else { button.btn.outline.sm { (icon("check")) "Devuelto" } }
            }
        }
    }
}
