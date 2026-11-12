use super::*;

fn pct(a: i64, b: i64) -> f64 {
    if b > 0 { (a.max(0) as f64 * 100.0 / b as f64).min(100.0) } else { 0.0 }
}

pub fn render(v: &View) -> Markup {
    let (left, spent, available, pending, total) = (v.left(), v.spent(), v.available(), v.pending(), v.bills_total());
    let (my_house, personal, days_left) = (v.my_house(), v.personal(), v.days_left());
    let paid_n = v.bills.iter().filter(|b| b.payer.is_some()).count();
    let denom = v.me.salary.max(my_house + personal + spent).max(1) as f64;
    let w = |x: i64| format!("width:{:.2}%", x.max(0) as f64 * 100.0 / denom);
    let new_bill = format!("{}#new-bill", v.link("settings"));
    html! {
        (v.head(&format!("Hola, {}", v.me.name), html! { "Así va " (v.month_lc()) " de " (v.year()) "." }))
        div.stats {
            (stat("wallet", "Saldo previsto", money(left), html! {
                @if v.me.salary > 0 {
                    (pill(if left >= 0 { "ok" } else { "bad" }, Some(if left >= 0 { "trending-up" } else { "trending-down" }),
                        &format!("{} % del sueldo libre", v.pct_of_salary(left))))
                } @else { a href={ (v.link("settings")) "#salary" } { "Pon tu sueldo para calcularlo" } }
            }))
            (stat("calendar-clock", "Puedes gastar al día",
                if days_left > 0 && left > 0 { money(left / days_left) } else { html! { span.money.muted { "—" } } },
                html! {
                    div.meter { span style={ "width:" (format!("{:.1}", pct(spent, available))) "%" } {} }
                    "Has usado el " (format!("{:.0}", pct(spent, available))) " % de lo disponible"
                    @if days_left > 0 { " · quedan " (days_left) " días" }
                }))
            (stat("receipt", "Cuentas por pagar", money(pending), html! {
                div.meter.ok { span style={ "width:" (format!("{:.1}", pct(total - pending, total))) "%" } {} }
                (paid_n) " de " (v.bills.len()) " pagadas"
            }))
        }
        div.split {
            section.card {
                div.card-head {
                    div { h2 { "Cuentas del mes" } p { "Marca cada una cuando la pagues." } }
                    a.btn.outline.sm href=(new_bill) { (icon("plus")) "Nueva" }
                }
                @if v.bills.is_empty() {
                    (empty("calendar-plus", "Sin cuentas este mes", "Añade el alquiler, la luz o tus suscripciones para no olvidar ningún pago.", Some((&new_bill, "Añadir cuenta"))))
                }
                ul.rows { @for b in &v.bills { (bills::row(v, b)) } }
            }
            section.card {
                div.card-head { div { h2 { "Reparto de tu sueldo" } p { "En qué se va tu sueldo neto de " (eur(v.me.salary)) "." } } }
                @if v.me.salary > 0 {
                    div.bar role="img" aria-label="Reparto del sueldo" {
                        span.c-casa style=(w(my_house)) {}
                        span.c-pers style=(w(personal)) {}
                        span.c-gas style=(w(spent)) {}
                        span.c-sobra style=(w(left)) {}
                    }
                    ul.legend {
                        li { span.dot.c-casa {} span.l { "Tu parte de la casa" } span.amt { (eur(my_house)) } span.pc { (v.pct_of_salary(my_house)) " %" } }
                        li { span.dot.c-pers {} span.l { "Cuentas personales" } span.amt { (eur(personal)) } span.pc { (v.pct_of_salary(personal)) " %" } }
                        li { span.dot.c-gas {} span.l { "Día a día" } span.amt { (eur(spent)) } span.pc { (v.pct_of_salary(spent)) " %" } }
                        li.total { span.dot.c-sobra {} span.l { "Te sobra" } span.amt class=(if left >= 0 { "ok-txt" } else { "bad-txt" }) { (eur(left)) } span.pc { (v.pct_of_salary(left)) " %" } }
                    }
                } @else {
                    (empty("banknote", "Falta tu sueldo", "Con él calculamos tu parte de la casa y cuánto te sobra cada mes.", Some((&format!("{}#salary", v.link("settings")), "Poner sueldo"))))
                }
            }
        }
    }
}
