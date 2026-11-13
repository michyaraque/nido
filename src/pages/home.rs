use super::*;

fn pct(a: i64, b: i64) -> f64 {
    if b > 0 { (a.max(0) as f64 * 100.0 / b as f64).min(100.0) } else { 0.0 }
}

const DAYS: [&str; 7] = ["Domingo", "Lunes", "Martes", "Miércoles", "Jueves", "Viernes", "Sábado"];

/// Day of week for "YYYY-MM-DD", 0 = Sunday (Sakamoto).
fn weekday(date: &str) -> usize {
    let n = |r: std::ops::Range<usize>| date.get(r).and_then(|s| s.parse::<i64>().ok()).unwrap_or(1);
    let (mut y, m, d) = (n(0..4), n(5..7), n(8..10));
    const T: [i64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    if m < 3 { y -= 1; }
    ((y + y / 4 - y / 100 + y / 400 + T[(m as usize - 1).min(11)] + d) % 7) as usize
}

pub fn render(v: &View) -> Markup {
    let (left, spent, available, pending, total) = (v.left(), v.spent(), v.available(), v.pending(), v.bills_total());
    let (my_house, personal, days_left) = (v.my_house(), v.personal(), v.days_left());
    let unpaid: Vec<&Bill> = v.bills.iter().filter(|b| b.payer.is_none()).collect();
    let next = unpaid.iter().min_by_key(|b| b.due_day);
    let due = tasks::due(v);
    let denom = v.me.salary.max(my_house + personal + spent).max(1) as f64;
    let w = |x: i64| format!("width:{:.2}%", x.max(0) as f64 * 100.0 / denom);
    let new_bill = "#new-bill".to_string();
    let salary_link = format!("{}#salary", v.link("settings"));
    html! {
        div.page-head.greet {
            div {
                h1 { "Hola, " (v.me.name) }
                p {
                    @if v.is_now() {
                        (DAYS[weekday(&v.today)]) ", " (v.today_day()) " de " (v.month_lc()) ". "
                        @if v.me.salary > 0 { "Te quedan " b { (eur(left)) } " este mes" } @else { "Pon tu sueldo para ver cuánto te queda" }
                        @match unpaid.len() {
                            0 if !v.bills.is_empty() => " y todas las cuentas están pagadas.",
                            0 => ".",
                            1 => " y te queda 1 cuenta por pagar.",
                            n => { " y te quedan " (n) " cuentas por pagar." }
                        }
                    } @else {
                        "Así " (if v.is_past() { "fue " } else { "va " }) (v.month_lc()) " de " (v.year()) " para ti"
                    }
                }
            }
            div.head-actions {
                (v.month_nav())
                a.btn.primary href={ (v.link("expenses")) "#new" } { (icon("plus")) "Registrar gasto" }
            }
        }
        div.stats {
            (panel("wallet", html! { "Saldo previsto" }, html! {},
                html! {
                    (money(left))
                    @if v.me.salary > 0 {
                        (pill(if left >= 0 { "ok" } else { "bad" }, Some(if left >= 0 { "trending-up" } else { "trending-down" }),
                            &format!("{} % del sueldo libre", v.pct_of_salary(left))))
                    } @else { a.panel-hint href=(salary_link) { "Pon tu sueldo para calcularlo" } }
                },
                html! { "Sueldo · " (eur(v.me.salary)) }, None))
            (panel("calendar-clock", html! { "Puedes gastar al día" }, html! {},
                html! {
                    @if days_left > 0 && left > 0 { (money(left / days_left)) } @else { span.money.muted { "—" } }
                    div.meter { span style={ "width:" (format!("{:.1}", pct(spent, available))) "%" } {} }
                    span.panel-hint { "Has usado el " (format!("{:.0}", pct(spent, available))) " % de lo disponible" }
                },
                html! { @if days_left > 0 { "Quedan " (days_left) " días" } @else { "Mes cerrado" } }, None))
            (panel("receipt", html! { "Cuentas por pagar" }, html! {},
                html! {
                    (money(pending))
                    div.meter.ok { span style={ "width:" (format!("{:.1}", pct(total - pending, total))) "%" } {} }
                    span.panel-hint { (v.bills.len() - unpaid.len()) " de " (v.bills.len()) " pagadas" }
                },
                html! { @if let Some(b) = next { "Próxima · " (b.name) " día " (b.due_day) } @else { "Todo pagado" } }, None))
        }
        div.split {
            (panel("calendar-check", html! { "Cuentas del mes" },
                html! { a.btn.outline.sm href=(new_bill) { (icon("plus")) "Nueva" } },
                html! {
                    @if v.bills.is_empty() {
                        (empty("calendar-plus", "Sin cuentas este mes", "Añade el alquiler, la luz o tus suscripciones para no olvidar ningún pago.", Some((&new_bill, "Añadir cuenta"))))
                    }
                    ul.rows { @for b in &v.bills { (bills::row(v, b)) } }
                    (bills::new_modal(v))
                },
                html! { "Marca cada una cuando la pagues" }, Some((&v.link("house"), "La casa"))))
            div.stack {
            (panel("calendar-check", html! { "Tareas pendientes" }, html! {},
                html! {
                    @if due.is_empty() {
                        (empty("circle-check", "Todo hecho", "No hay tareas para hoy ni atrasadas.", None))
                    } @else {
                        ul.rows { @for c in due.iter().take(5) { (tasks::due_row(v, c)) } }
                    }
                },
                html! { @if due.len() > 5 { "Y " (due.len() - 5) " más" } @else { (v.chores.len()) " tareas en casa" } },
                Some((&v.link("tasks"), "Tareas"))))
            (panel("banknote", html! { "Reparto de tu sueldo" }, html! {},
                html! {
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
                        (empty("banknote", "Falta tu sueldo", "Con él calculamos tu parte de la casa y cuánto te sobra cada mes.", Some((&salary_link, "Poner sueldo"))))
                    }
                },
                html! { "Sueldo neto · " (eur(v.me.salary)) }, Some((&salary_link, "Cambiar sueldo"))))
            }
        }
    }
}

#[test]
fn weekday_known_dates() {
    assert_eq!(weekday("2026-11-13"), 5); // viernes
    assert_eq!(weekday("2024-02-29"), 4); // jueves
    assert_eq!(weekday("2000-01-01"), 6); // sábado
}
