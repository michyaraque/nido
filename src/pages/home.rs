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

/// Ingresos del mes: the payslip and one-off incomes, each ticked when it arrives.
fn income_panel(v: &View) -> Markup {
    let past = v.is_past();
    // Default "cobrado el" for this month: today, or the month's last day when looking back.
    let default_day = if v.is_past() { format!("{}-{:02}", v.m, v.ndays) } else { v.today.clone() };
    let day_form = |action: String, got: &str| html! {
        form method="post" action=(action) {
            (v.ctx())
            div.field { label { "Fecha de cobro" } input.input type="date" name="day" value=(got) max=(v.today) required; }
            button.btn.outline.wide { "Guardar fecha" }
        }
    };
    let row = |action: String, got: Option<&str>, ic: &str, title: &str, amount: i64, menu: Markup| {
        let (tone, label) = match got {
            // Ticked within its own month: "día 3"; ticked later: the full date.
            Some(d) if d.starts_with(v.m.as_str()) => ("ok", format!("Cobrado · día {}", d[8..].trim_start_matches('0'))),
            Some(d) => ("ok", format!("Cobrado el {}/{}", d[8..].trim_start_matches('0'), &d[5..7])),
            // Before "Confirmar desde" an unticked month is simply unrecorded, not owed.
            None if v.m < v.pay_since => ("", "Sin marcar".to_string()),
            None if past => ("bad", "Sin cobrar".to_string()),
            None => ("warn", "Pendiente".to_string()),
        };
        html! {
            li.row.done[got.is_some()] {
                form.inline method="post" action=(action) {
                    (v.ctx())
                    button.check aria-label={ (if got.is_some() { "Marcar como no cobrado: " } else { "Marcar como cobrado: " }) (title) } { (icon("check")) }
                }
                span.tile style="--c:#17803d" { (icon(ic)) }
                div.row-main { span.row-title { (title) } span.row-sub { (pill(tone, None, &label)) } }
                div.row-amt { span.amt { (eur(amount)) } }
                (menu)
            }
        }
    };
    html! {
        @if !v.tracks_salary(&v.m) && v.month_incomes().next().is_none() {
            @if v.me.salary > 0 {
                p.panel-hint { "Ingreso fijo de " (eur(v.me.salary)) " incluido. Apunta aquí lo que cobres aparte." }
            } @else {
                (empty("banknote", "Sin ingresos este mes", "Pon tu ingreso fijo en Mis ajustes o apunta lo que cobres.", Some(("#new-income", "Apuntar ingreso"))))
            }
        }
        ul.rows {
            @if v.tracks_salary(&v.m) {
                (row(format!("/incomes/salary/{}", v.m), v.salary_day(&v.m), "banknote", "Ingreso fijo", v.me.salary, html! {
                    @if let Some(d) = v.salary_day(&v.m) {
                        details.menu {
                            summary.icon-btn aria-label="Opciones del ingreso fijo" { (icon("ellipsis")) }
                            div.pop { (day_form(format!("/incomes/salary/{}/day", v.m), d)) }
                        }
                    }
                }))
            }
            @for i in v.month_incomes() {
                (row(format!("/incomes/{}/toggle", i.id), i.received.as_deref(), "coins", &i.concept, i.amount, html! {
                    details.menu {
                        summary.icon-btn aria-label={ "Opciones de " (i.concept) } { (icon("ellipsis")) }
                        div.pop {
                            @if let Some(d) = i.received.as_deref() { (day_form(format!("/incomes/{}/day", i.id), d)) hr; }
                            form method="post" action={ "/incomes/" (i.id) "/delete" } { (v.ctx()) button.btn.danger.wide { (icon("trash-2")) "Borrar ingreso" } }
                        }
                    }
                }))
            }
        }
        div.modal #new-income role="dialog" aria-modal="true" aria-labelledby="new-income-t" {
            a.scrim href="#_" aria-label="Cerrar" tabindex="-1" {}
            div.dialog {
                div.dialog-top {
                    h2 #new-income-t { "Nuevo ingreso" }
                    a.icon-btn href="#_" aria-label="Cerrar" { (icon("x")) }
                }
                form.form method="post" action="/incomes" accept-charset="utf-8" {
                    (v.ctx())
                    div.field { label for="ic" { "Concepto" } input #ic.input type="text" name="concept" placeholder="Factura, clases, venta…" data-concepts="income-concepts" role="combobox" aria-autocomplete="list" aria-expanded="false" aria-controls="concept-pop" autocomplete="off" required maxlength="60";
                        template #income-concepts {
                            @for c in v.incomes.iter().map(|i| i.concept.as_str()).collect::<std::collections::BTreeSet<_>>() {
                                button.opt type="button" role="option" data-v=(c) {
                                    span.tile style="--c:#17803d" { (icon("coins")) }
                                    span.opt-txt { (c) small { "Ingreso" } }
                                }
                            }
                        } }
                    div.field { label for="ia" { "Importe neto" } input #ia.input type="text" name="amount" placeholder="0,00 €" inputmode="decimal" required; p.hint { "Lo que te queda a ti, sin impuestos." } }
                    label.switch { input type="checkbox" name="received" checked; span { "Ya lo he cobrado" small { "Si no, queda pendiente hasta que lo marques." } } }
                    div.field.received-on { label for="io" { "Fecha de cobro" } input #io.input type="date" name="received_on" value=(default_day) max=(v.today); }
                    button.btn.primary.wide { (icon("plus")) "Apuntar en " (v.month_lc()) }
                }
            }
        }
    }
}

/// "Pendiente de cobrar": incomes from past months that have not arrived yet.
fn owed_banner(v: &View, owed: &[Owed]) -> Markup {
    let total: i64 = owed.iter().map(|o| o.amount).sum();
    html! {
        section.owed role="status" {
            div.owed-head {
                span.owed-ico { (icon("circle-alert")) }
                div { b { "Pendiente de cobrar · " (eur(total)) } span { "De meses anteriores. No cuenta en tu saldo hasta que lo marques." } }
            }
            ul.owed-list {
                @for o in owed {
                    li {
                        span.owed-l { (o.label) }
                        span.owed-v { (eur(o.amount)) }
                        form.inline method="post" action=(o.action) { (v.ctx()) button.btn.outline.sm { (icon("check")) "Cobrado" } }
                    }
                }
            }
        }
    }
}

pub fn render(v: &View) -> Markup {
    let (left, spent, available, pending, total) = (v.left(), v.spent(), v.available(), v.pending(), v.bills_total());
    // What I really have to spend: this month plus what was carried (if I carry it).
    let (balance, can_spend) = (v.balance(), v.carried + available - spent);
    // Money available this month before day-to-day spending: carried + received - bills.
    let pool = v.carried + available;
    let (my_house, personal, days_left) = (v.my_house(), v.personal(), v.days_left());
    let outflow = my_house + personal + spent;
    let (income, expected, owed) = (v.income(), v.expected(), v.owed());
    let unpaid: Vec<&Bill> = v.bills.iter().filter(|b| b.payer.is_none()).collect();
    let next = unpaid.iter().min_by_key(|b| b.due_day);
    let due = tasks::due(v);
    let denom = income.max(my_house + personal + spent).max(1) as f64;
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
                        @if income > 0 || v.carried != 0 { "Te quedan " b { (eur(balance)) } " este mes" }
                        @else if v.tracks_salary(&v.m) { "Aún no has cobrado este mes" }
                        @else { "Apunta tus ingresos para ver cuánto te queda" }
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
        @if !owed.is_empty() { (owed_banner(v, &owed)) }
        div.stats {
            (panel("wallet", html! { "Saldo previsto" }, html! {},
                html! {
                    (money(balance))
                    @if v.carried != 0 {
                        span.panel-hint { "Incluye " (if v.carried > 0 { "+" } else { "" }) (eur(v.carried)) " que traías" }
                    }
                    @if income > 0 {
                        @if left >= 0 { (pill("ok", Some("trending-up"), &format!("{} % de lo cobrado libre", v.pct_of_income(left)))) }
                        @else { (pill("bad", Some("trending-down"), "Más gasto que lo cobrado")) }
                    } @else { a.panel-hint href="#income" { "Aún no has cobrado este mes" } }
                },
                html! { "Cobrado · " (eur(income)) @if expected > income { " de " (eur(expected)) } }, None))
            (panel("calendar-clock", html! { "Puedes gastar al día" }, html! {},
                html! {
                    @if days_left > 0 && can_spend > 0 { (money(can_spend / days_left)) } @else { span.money.muted { "—" } }
                    div.meter { span style={ "width:" (format!("{:.1}", if pool > 0 { pct(spent, pool) } else if spent > 0 { 100.0 } else { 0.0 })) "%" } {} }
                    @if pool > 0 { span.panel-hint { "Has usado el " (format!("{:.0}", pct(spent, pool))) " % de lo disponible" } }
                    @else if spent > 0 { span.panel-hint { "Gastos del mes: " (eur(spent)) " sin ingresos" } }
                    @else { span.panel-hint { "Nada disponible este mes" } }
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
            div.stack {
            div #income {
            (panel("coins", html! { "Ingresos del mes" },
                html! { a.btn.outline.sm href="#new-income" { (icon("plus")) "Ingreso" } },
                income_panel(v),
                html! { "Marca cada uno cuando lo cobres" }, Some((&salary_link, "Ingreso fijo"))))
            }
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
            }
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
            (panel("banknote", html! { @if income > 0 { "Reparto de lo cobrado" } @else { "En qué se ha ido" } }, html! {},
                html! {
                    @if income == 0 && outflow > 0 { p.panel-hint { "Sin ingresos marcados este mes." } }
                    @if income > 0 || outflow > 0 {
                        div.bar role="img" aria-label="Reparto de lo cobrado" {
                            span.c-casa style=(w(my_house)) {}
                            span.c-pers style=(w(personal)) {}
                            span.c-gas style=(w(spent)) {}
                            span.c-sobra style=(w(left)) {}
                        }
                        ul.legend {
                            li { span.dot.c-casa {} span.l { "Tu parte de la casa" } span.amt { (eur(my_house)) } span.pc { @if left >= 0 { (v.pct_of_income(my_house)) " %" } } }
                            li { span.dot.c-pers {} span.l { "Cuentas personales" } span.amt { (eur(personal)) } span.pc { @if left >= 0 { (v.pct_of_income(personal)) " %" } } }
                            li { span.dot.c-gas {} span.l { "Día a día" } span.amt { (eur(spent)) } span.pc { @if left >= 0 { (v.pct_of_income(spent)) " %" } } }
                            li.total { span.dot.c-sobra {} span.l { "Te sobra" } span.amt class=(if left >= 0 { "ok-txt" } else { "bad-txt" }) { (eur(left)) } span.pc { @if left >= 0 { (v.pct_of_income(left)) " %" } } }
                        }
                    } @else {
                        (empty("banknote", "Nada este mes", "Ni ingresos ni gastos apuntados en este mes.", None))
                    }
                },
                html! { "Cobrado en " (v.month_lc()) " · " (eur(income)) }, Some((&salary_link, "Ingreso fijo"))))
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
