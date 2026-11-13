use super::*;

pub fn render(v: &View) -> Markup {
    let shared = v.shared();
    let net = v.net();
    let month_lc = v.month_lc();
    let new_bill = "#new-bill".to_string();
    html! {
        (v.head("La casa", html! { "Cuentas comunes de " (month_lc) ", repartidas según el sueldo de cada uno" }))
        div.stats {
            (stat("house", "Total de la casa", money(shared), html! { (v.bills.iter().filter(|b| b.owner.is_none()).count()) " cuentas comunes" }))
            @for (i, u) in v.users.iter().enumerate() {
                @let share = v.house_share(u);
                (stat("users", &format!("Paga {}", u.name), money(share), html! {
                    span.dot class={ "p" (i % 4) } {}
                    @if shared > 0 { (format!("{:.1}", share as f64 * 100.0 / shared as f64).replace('.', ",")) " % de la casa" }
                    @else { (format!("{:.1}", v.share_pct(u.salary)).replace('.', ",")) " % según su sueldo" }
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
                    (empty("users", "Estás tú solo en casa", "Para repartir las cuentas, crea un usuario para cada persona en la Raspberry con sudo nido-user Nombre.", None))
                } @else if let (true, Some(o)) = (net != 0, v.other()) {
                    div.owe {
                        div.owe-who {
                            (v.avatar(if net > 0 { o.id } else { v.me.id }))
                            span.owe-arrow { (icon("arrow-right")) }
                            (v.avatar(if net > 0 { v.me.id } else { o.id }))
                        }
                        p.owe-txt { @if net > 0 { (o.name) " te tiene que devolver" } @else { "Le tienes que devolver a " (o.name) } }
                        (money(net.abs()))
                        form method="post" action="/settlements" {
                            (v.ctx())
                            input type="hidden" name="other_id" value=(o.id);
                            input type="hidden" name="amount" value=(net);
                            button.btn.primary.wide { (icon("hand-coins")) "Marcar como devuelto" }
                        }
                        p.hint.center { "Quedará apuntado y el saldo volverá a cero" }
                    }
                } @else {
                    (empty("circle-check", "Estáis en paz", "Nadie debe nada por las cuentas de la casa de este mes.", None))
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
