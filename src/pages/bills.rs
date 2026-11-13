use super::*;

/// The "Nueva cuenta" form, shared by the Ajustes card and the #new-bill window.
pub fn form(v: &View) -> Markup {
    html! {
        form.form method="post" action="/bills" accept-charset="utf-8" {
            (v.ctx())
            div.field { label for="bn" { "Nombre" } input #bn.input type="text" name="name" placeholder="Alquiler, Iberdrola, Netflix…" required maxlength="100"; }
            div.two {
                div.field { label for="ba" { "Importe" } input #ba.input type="text" name="amount" placeholder="0,00 €" inputmode="decimal" required; }
                div.field { label for="bd" { "Día del mes" } input #bd.input type="number" name="due_day" placeholder="1" min="1" max="31" required; }
            }
            (v.who_pays("split"))
            button.btn.primary { (icon("plus")) "Añadir cuenta" }
        }
    }
}

/// Opened by href="#new-bill" (CSS :target), like the backup windows.
pub fn new_modal(v: &View) -> Markup {
    html! {
        div.modal #new-bill role="dialog" aria-modal="true" aria-labelledby="new-bill-t" {
            a.scrim href="#_" aria-label="Cerrar" tabindex="-1" {}
            div.dialog.dialog-wide {
                div.dialog-top {
                    h2 #new-bill-t { "Nueva cuenta mensual" }
                    a.icon-btn href="#_" aria-label="Cerrar" { (icon("x")) }
                }
                p.hint { "Un pago que se repite cada mes, como el alquiler o la luz. Aparecerá desde " (v.month_lc()) "." }
                (form(v))
            }
        }
    }
}

fn due(v: &View, b: &Bill) -> (&'static str, String) {
    if b.payer.is_some() {
        return ("ok", format!("Pagada · día {}", b.due_day));
    }
    let d = b.due_day.min(v.ndays);
    let today = v.today_day();
    if v.is_past() {
        ("bad", "Sin pagar".into())
    } else if v.is_now() && d < today {
        ("bad", format!("Venció el día {d}"))
    } else if v.is_now() && d == today {
        ("warn", "Vence hoy".into())
    } else if v.is_now() && d - today <= 3 {
        ("warn", format!("Vence en {} día{}", d - today, if d - today == 1 { "" } else { "s" }))
    } else {
        ("", format!("Día {}", b.due_day))
    }
}

pub fn row(v: &View, b: &Bill) -> Markup {
    let (tone, when) = due(v, b);
    let month_lc = v.month_lc();
    html! {
        li.row.done[b.payer.is_some()] {
            form.inline method="post" action={ "/bills/" (b.id) "/payment" } {
                (v.ctx())
                button.check aria-label={ (if b.payer.is_some() { "Marcar como no pagada: " } else { "Marcar como pagada: " }) (b.name) } { (icon("check")) }
            }
            (badge(&look(&b.name, None), &b.name))
            div.row-main {
                span.row-title { (b.name) }
                span.row-sub {
                    (pill(tone, None, &when))
                    @if b.owner.is_none() { span.muted { "Casa" } } @else { span.muted { "Personal" } }
                    @if let Some(p) = b.payer { span.muted { "· pagó " (v.name_of(p)) } }
                }
            }
            div.row-amt {
                span.amt { (eur(b.amount)) }
                @if b.amount != b.base { span.muted.xs title={ "Normalmente " (eur(b.base)) } { "Solo este mes" } }
                @else if b.owner.is_none() && v.users.len() > 1 {
                    @match (b.whole, v.carrier(b)) {
                        (true, Some(u)) => span.muted.xs { "Pagada entera por " @if u == v.me.id { "ti" } @else { (v.name_of(u)) } },
                        (false, Some(u)) => span.muted.xs { "La paga " @if u == v.me.id { "tú" } @else { (v.name_of(u)) } " entera" },
                        _ => span.muted.xs { "Tu parte " (eur(v.share_of(b, &v.me_u()))) },
                    }
                }
            }
            details.menu {
                summary.icon-btn aria-label={ "Opciones de " (b.name) } { (icon("ellipsis")) }
                div.pop {
                    form method="post" action={ "/bills/" (b.id) "/amount" } accept-charset="utf-8" {
                        (v.ctx())
                        div.field {
                            label { "Importe solo de " (month_lc) }
                            input.input type="text" name="amount" inputmode="decimal" value=(eur_input(b.amount)) required;
                            p.hint { "Para cuentas que cambian, como la luz. Los demás meses no se tocan." }
                        }
                        button.btn.primary.wide { "Guardar importe de " (month_lc) }
                    }
                    hr;
                    form method="post" action={ "/bills/" (b.id) } accept-charset="utf-8" {
                        (v.ctx())
                        p.pop-title { "Cambiar desde " (month_lc) " en adelante" }
                        div.field { label { "Nombre" } input.input type="text" name="name" value=(b.name) required maxlength="100"; }
                        div.two {
                            div.field { label { "Importe" } input.input type="text" name="amount" inputmode="decimal" value=(eur_input(b.base)) required; }
                            div.field { label { "Día del mes" } input.input type="number" name="due_day" min="1" max="31" value=(b.due_day) required; }
                        }
                        (v.who_pays(&View::mode(Some(b))))
                        button.btn.outline.wide { "Guardar cambios" }
                    }
                    @if b.owner.is_none() && b.covered_by.is_none() && v.users.len() > 1 {
                        hr;
                        form method="post" action={ "/bills/" (b.id) "/whole" } {
                            (v.ctx())
                            @if b.whole && b.payer == Some(v.me.id) {
                                button.btn.outline.wide { "Volver a repartirla este mes" }
                            } @else {
                                button.btn.outline.wide { (icon("hand-coins")) "La he pagado entera este mes" }
                                p.hint { "Este mes no se reparte ni genera deuda: cuenta solo para ti" }
                            }
                        }
                    }
                    a.btn.danger.wide href={ "#end-" (b.id) } { (icon("x")) "Quitar desde " (month_lc) }
                }
            }
            // Confirmation dialog, opened by #end-<id> (CSS :target): never the browser's confirm().
            div.modal id={ "end-" (b.id) } role="dialog" aria-modal="true" aria-labelledby={ "end-t-" (b.id) } {
                a.scrim href="#_" aria-label="Cancelar" tabindex="-1" {}
                div.dialog {
                    span.dialog-ico { (icon("calendar-clock")) }
                    h2 id={ "end-t-" (b.id) } { "¿Quitar «" (b.name) "» desde " (month_lc) "?" }
                    p { "Dejará de aparecer a partir de " (month_lc) ". Los meses anteriores y sus pagos se quedan como están." }
                    div.dialog-actions {
                        a.btn.outline href="#_" { "Cancelar" }
                        form.inline method="post" action={ "/bills/" (b.id) "/end" } {
                            (v.ctx())
                            button.btn.destructive { "Quitar cuenta" }
                        }
                    }
                }
            }
        }
    }
}
