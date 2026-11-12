use super::*;
use crate::chores::{COLORS, FREQUENCIES, ICONS, STARTERS};

const WEEK: [&str; 7] = ["L", "M", "X", "J", "V", "S", "D"];

fn status(c: &Chore) -> (&'static str, String) {
    match (c.days_since, c.every_days) {
        (None, _) => ("", "Sin hacer todavía".into()),
        (Some(d), Some(every)) if d > every => ("bad", format!("Atrasada {} día{}", d - every, if d - every == 1 { "" } else { "s" })),
        (Some(d), Some(every)) if d == every => ("warn", "Toca hoy".into()),
        (Some(d), Some(every)) if every - d == 1 => ("ok", "Toca mañana".into()),
        (Some(d), Some(every)) => ("ok", format!("Toca en {} días", every - d)),
        (Some(_), None) => ("", "Sin fecha fija".into()),
    }
}

fn ago(days: i64) -> String {
    match days {
        0 => "hoy".into(),
        1 => "ayer".into(),
        d => format!("hace {d} días"),
    }
}

fn chore_form(v: &View, c: Option<&Chore>) -> Markup {
    let action = c.map(|c| format!("/chores/{}", c.id)).unwrap_or_else(|| "/chores".into());
    let icon_now = c.map(|c| c.icon.as_str()).unwrap_or(ICONS[0].0);
    let color_now = c.map(|c| c.color.as_str()).unwrap_or(COLORS[0]);
    let every_now = c.and_then(|c| c.every_days).unwrap_or(0);
    html! {
        form.form method="post" action=(action) accept-charset="utf-8" {
            (v.ctx())
            div.field { label { "Nombre" } input.input type="text" name="name" value=[c.map(|c| c.name.as_str())] placeholder="Fregar, barrer, poner lavadora…" required maxlength="60"; }
            div.field {
                label { "Icono" }
                div.pick-icons role="radiogroup" aria-label="Icono" {
                    @for (ic, label) in ICONS {
                        label.pick title=(label) { input type="radio" name="icon" value=(ic) checked[*ic == icon_now] aria-label=(label); span { (icon(ic)) } }
                    }
                }
            }
            div.field {
                label { "Color" }
                div.swatches role="radiogroup" aria-label="Color" {
                    @for col in COLORS {
                        label.swatch { input type="radio" name="color" value=(col) checked[*col == color_now] aria-label={ "Color " (col) }; span style={ "--c:" (col) } {} }
                    }
                }
            }
            div.field {
                label { "¿Cada cuánto toca?" }
                div.chips role="radiogroup" aria-label="Frecuencia" {
                    @for (days, label) in FREQUENCIES {
                        label.chip-radio { input type="radio" name="every_days" value=(days) checked[*days == every_now]; span { (label) } }
                    }
                }
            }
            button.btn.primary.wide { @if c.is_some() { "Guardar cambios" } @else { (icon("plus")) "Crear tarea" } }
        }
    }
}

fn urgency(c: &Chore) -> (u8, i64) {
    match (c.days_since, c.every_days) {
        (Some(0), _) => (2, 0),
        (Some(d), Some(every)) => (0, every - d),
        (None, Some(_)) => (0, 0),
        _ => (1, 0),
    }
}

fn chore_row(v: &View, c: &Chore) -> Markup {
    let (tone, text) = status(c);
    let done = c.days_since == Some(0);
    html! {
        li.row.chore-row.done[done] style={ "--c:" (c.color) } {
            form.inline method="post" action={ "/chores/" (c.id) "/log" } {
                (v.ctx())
                button.check aria-label={ (if done { "Apuntar otra vez hoy: " } else { "Hecha hoy: " }) (c.name) }
                    title=(if done { "Hecha hoy. Tócala para apuntarla otra vez." } else { "Márcala cuando la hagas" }) { (icon("check")) }
            }
            span.tile.sm { (icon(&c.icon)) }
            div.row-main {
                span.row-title { (c.name) }
                span.row-sub {
                    @if done { (pill("ok", None, "Hecha hoy")) } @else { (pill(tone, None, &text)) }
                    @if let (Some(u), Some(d)) = (c.last_user, c.days_since) { span.muted { (v.name_of(u)) ", " (ago(d)) } }
                }
            }
            details.menu {
                summary.icon-btn aria-label={ "Opciones de " (c.name) } { (icon("ellipsis")) }
                div.pop {
                    p.pop-title { "Editar tarea" }
                    (chore_form(v, Some(c)))
                    a.btn.danger.wide href={ "#archive-" (c.id) } { (icon("trash-2")) "Quitar tarea" }
                }
            }
            div.modal id={ "archive-" (c.id) } role="dialog" aria-modal="true" aria-labelledby={ "archive-t-" (c.id) } {
                a.scrim href="#_" aria-label="Cancelar" tabindex="-1" {}
                div.dialog {
                    span.dialog-ico { (icon("trash-2")) }
                    h2 id={ "archive-t-" (c.id) } { "¿Quitar «" (c.name) "»?" }
                    p { "Dejará de aparecer en la lista. Lo que ya está apuntado en el calendario se queda." }
                    div.dialog-actions {
                        a.btn.outline href="#_" { "Cancelar" }
                        form.inline method="post" action={ "/chores/" (c.id) "/archive" } { (v.ctx()) button.btn.destructive { "Quitar tarea" } }
                    }
                }
            }
        }
    }
}

fn when(v: &View, day: &str) -> (&'static str, String) {
    const DAYS: [&str; 7] = ["domingo", "lunes", "martes", "miércoles", "jueves", "viernes", "sábado"];
    let tomorrow = shift_day(&v.today, 1);
    if day < v.today.as_str() {
        ("bad", format!("Atrasada · {} {}", DAYS[weekday(day)], day[8..].trim_start_matches('0')))
    } else if day == v.today {
        ("warn", "Hoy".into())
    } else if day == tomorrow {
        ("accent", "Mañana".into())
    } else {
        ("", format!("{} {}", DAYS[weekday(day)], day[8..].trim_start_matches('0')))
    }
}

fn shift_day(day: &str, n: i64) -> String {
    let (y, m, d): (i64, i64, i64) = (day[..4].parse().unwrap_or(2000), day[5..7].parse().unwrap_or(1), day[8..].parse().unwrap_or(1));
    let len = match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    let (mut y, mut m, mut d) = (y, m, d + n);
    if d > len {
        d -= len;
        m += 1;
        if m > 12 {
            m = 1;
            y += 1;
        }
    }
    format!("{y:04}-{m:02}-{d:02}")
}

fn plan_row(v: &View, p: &Plan, show_when: bool) -> Markup {
    let (tone, text) = when(v, &p.day);
    let ctx = html! { (v.ctx()) input type="hidden" name="day" value=[v.day.as_deref()]; };
    html! {
        li.row.plan-row {
            span.tile.sm style={ "--c:" (p.color) } { (icon(&p.icon)) }
            div.row-main {
                span.row-title { (p.name) }
                span.row-sub {
                    @if show_when { (pill(tone, None, &text)) }
                    @match p.assignee {
                        Some(u) => span.who-chip { (v.avatar(u)) @if u == v.me.id { "Tú" } @else { (v.name_of(u)) } },
                        None => span.muted { "Libre: cualquiera puede cogerla" },
                    }
                }
            }
            div.plan-actions {
                @if p.assignee.is_none() {
                    form.inline method="post" action={ "/plans/" (p.id) "/take" } { (ctx) button.btn.outline.sm { "La cojo" } }
                }
                form.inline method="post" action={ "/plans/" (p.id) "/done" } { (ctx) button.btn.done-btn.sm style={ "--c:" (p.color) } aria-label={ "Hecha: " (p.name) } { (icon("check")) } }
                form.inline method="post" action={ "/plans/" (p.id) "/delete" } { (ctx) button.icon-btn aria-label={ "Quitar «" (p.name) "» del calendario" } { (icon("x")) } }
            }
        }
    }
}

fn calendar(v: &View) -> Markup {
    let user_idx = |id: i64| v.users.iter().position(|u| u.id == id).unwrap_or(0) % 4;
    html! {
        div.cal role="grid" aria-label="Calendario de tareas" {
            @for w in WEEK { span.cal-wd role="columnheader" { (w) } }
            @for _ in 0..v.first_weekday { span.cal-pad {} }
            @for d in 1..=v.ndays {
                @let date = format!("{}-{d:02}", v.m);
                @let logs: Vec<&Log> = v.logs.iter().filter(|l| l.day == date).collect();
                a.cal-day.today[date == v.today].sel[v.day.as_deref() == Some(date.as_str())]
                    href={ (v.link("tasks")) "&day=" (date) "#day" }
                    aria-label={ (d) " · " (logs.len()) " tareas" } {
                    span.cal-n { (d) }
                    span.cal-dots {
                        @for l in logs.iter().take(4) {
                            span class={ "cal-dot u" (user_idx(l.user_id)) } style={ "--c:" (l.color) } title={ (l.name) " · " (v.name_of(l.user_id)) } { (icon(&l.icon)) }
                        }
                        @for p in v.plans.iter().filter(|p| p.day == date).take(3) {
                            span class={ "cal-dot plan" @if let Some(u) = p.assignee { " assigned u" (user_idx(u)) } } style={ "--c:" (p.color) }
                                title={ (p.name) " · " (p.assignee.map(|u| v.name_of(u).to_string()).unwrap_or_else(|| "libre".into())) } { (icon(&p.icon)) }
                        }
                        @if logs.len() > 4 { span.cal-more { "+" (logs.len() - 4) } }
                    }
                }
            }
        }
    }
}

fn day_panel(v: &View, day: &str) -> Markup {
    let logs: Vec<&Log> = v.logs.iter().filter(|l| l.day == day).collect();
    let planned: Vec<&Plan> = v.plans.iter().filter(|p| p.day == day).collect();
    let weekday = (v.first_weekday + day[8..].parse::<i64>().unwrap_or(1) - 1) % 7;
    let names = ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"];
    let title = format!("{} {} de {}", names[weekday as usize], day[8..].trim_start_matches('0'), v.month_lc());
    let (past, future) = (day <= v.today.as_str(), day >= v.today.as_str());
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    html! {
        section.card #day {
            div.card-head {
                div {
                    h2.cap { (title) }
                    p { (logs.len()) " hecha" (plural(logs.len())) @if !planned.is_empty() { " · " (planned.len()) " planificada" (plural(planned.len())) } }
                }
            }
            @if logs.is_empty() && planned.is_empty() { p.hint { "Nada apuntado este día." } }
            ul.rows.compact {
                @for l in &logs {
                    li.row {
                        span.tile.sm style={ "--c:" (l.color) } { (icon(&l.icon)) }
                        span.row-main { span.row-title { (l.name) } span.row-sub { span.who-chip { (v.avatar(l.user_id)) (v.name_of(l.user_id)) } } }
                        form.inline method="post" action={ "/chores/logs/" (l.id) "/delete" } {
                            (v.ctx())
                            input type="hidden" name="day" value=(day);
                            button.icon-btn aria-label={ "Borrar «" (l.name) "» de este día" } { (icon("x")) }
                        }
                    }
                }
                @for p in &planned { (plan_row(v, p, false)) }
            }
            @if !v.chores.is_empty() {
                p.label-sm { "Añadir a este día" }
                div.day-add {
                    @for c in &v.chores {
                        details.menu {
                            summary.chip style={ "--c:" (c.color) } { (icon(&c.icon)) (c.name) }
                            div.pop {
                                @if past {
                                    p.pop-title { "¿Quién hizo «" (c.name) "»?" }
                                    div.who {
                                        @for u in &v.users {
                                            form.inline method="post" action={ "/chores/" (c.id) "/log" } {
                                                (v.ctx())
                                                input type="hidden" name="day" value=(day);
                                                input type="hidden" name="user_id" value=(u.id);
                                                button.btn.outline { (v.avatar(u.id)) (u.name) }
                                            }
                                        }
                                    }
                                }
                                @if future {
                                    p.pop-title.top[past] { @if past { "O planifícala para hoy" } @else { "¿Quién hará «" (c.name) "»?" } }
                                    div.who {
                                        @for u in &v.users {
                                            form.inline method="post" action={ "/chores/" (c.id) "/plan" } {
                                                (v.ctx())
                                                input type="hidden" name="day" value=(day);
                                                input type="hidden" name="assignee" value=(u.id);
                                                button.btn.outline { (icon("calendar-clock")) (u.name) }
                                            }
                                        }
                                        form.inline method="post" action={ "/chores/" (c.id) "/plan" } {
                                            (v.ctx())
                                            input type="hidden" name="day" value=(day);
                                            input type="hidden" name="assignee" value="";
                                            button.btn.outline { (icon("users")) "Cualquiera" }
                                        }
                                    }
                                    p.hint { "«Cualquiera» la deja libre para que la coja quien pueda." }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn ideas(v: &View) -> Vec<(usize, &'static (&'static str, &'static str, &'static str, i64))> {
    STARTERS
        .iter()
        .enumerate()
        .filter(|(_, (name, ..))| !v.chores.iter().any(|c| c.name.eq_ignore_ascii_case(name)))
        .collect()
}

fn starter_chips(v: &View) -> Markup {
    html! {
        div.chips.center-chips {
            @for (i, (name, ic, color, _)) in ideas(v) {
                form.inline method="post" action="/chores/starter" {
                    (v.ctx())
                    input type="hidden" name="index" value=(i);
                    button.chip style={ "--c:" (color) } { (icon(ic)) (name) }
                }
            }
        }
    }
}

pub fn render(v: &View) -> Markup {
    let total = v.logs.len().max(1);
    let mut chores: Vec<&Chore> = v.chores.iter().collect();
    chores.sort_by_key(|c| urgency(c));
    let due = chores.iter().filter(|c| matches!(status(c).0, "bad" | "warn")).count();
    html! {
        (v.head("Tareas de casa", html! { "Quién hace qué y cuándo. Marca el círculo cuando hagas una." }))
        @if v.chores.is_empty() {
            section.card.starters.chore-card {
                (empty("list-todo", "Crea las tareas de casa", "Empieza con alguna de estas o crea la tuya, con el icono y el color que quieras.", None))
                (starter_chips(v))
                (new_chore(v, false))
            }
        } @else {
            section.card.chore-card {
                div.card-head {
                    span.stat-ico { (icon("list-checks")) }
                    div {
                        h2 { "Tareas" }
                        p { @match due { 0 => "Todo al día.", 1 => "1 toca hoy o va atrasada.", n => { (n) " tocan hoy o van atrasadas." } } }
                    }
                }
                ul.rows.chore-list { @for c in &chores { (chore_row(v, c)) } }
                (new_chore(v, true))
            }
        }
        @let pending: Vec<&Plan> = v.plans.iter().filter(|p| p.day.as_str() <= shift_day(&v.today, 7).as_str()).collect();
        div.split.even {
            @if !pending.is_empty() {
                section.card {
                    div.card-head { span.stat-ico { (icon("calendar-clock")) } div { h2 { "Planificadas" } p { "Próximos 7 días. Las libres las puede coger cualquiera." } } }
                    ul.rows { @for p in pending { (plan_row(v, p, true)) } }
                }
            }
                section.card {
                    div.card-head { span.stat-ico { (icon("trophy")) } div { h2 { "Este mes" } p { (v.logs.len()) " tareas hechas en " (v.month_lc()) "." } } }
                    @if !v.logs.is_empty() {
                        div.bar.shares {
                            @for (i, u) in v.users.iter().enumerate() {
                                @let n = v.logs.iter().filter(|l| l.user_id == u.id).count();
                                @if n > 0 { span class={ "p" (i % 4) } style={ "width:" (n * 100 / total) "%" } { (n) } }
                            }
                        }
                    }
                    ul.legend {
                        @for (i, u) in v.users.iter().enumerate() {
                            @let n = v.logs.iter().filter(|l| l.user_id == u.id).count();
                            li { span class={ "dot p" (i % 4) } {} span.l { (u.name) } span.amt { (n) " tarea" @if n != 1 { "s" } } }
                        }
                    }
                    @if !v.chores.is_empty() && !v.logs.is_empty() {
                        details.steps.top {
                            summary { (icon("chevron-right")) "Por tarea" }
                            ul.rows.compact {
                                @for c in &v.chores {
                                    @let mine: Vec<(usize, usize)> = v.users.iter().enumerate().map(|(i, u)| (i, v.logs.iter().filter(|l| l.chore_id == c.id && l.user_id == u.id).count())).collect();
                                    li.row {
                                        span.tile.sm style={ "--c:" (c.color) } { (icon(&c.icon)) }
                                        span.row-main { span.row-title { (c.name) } }
                                        span.tally { @for (i, n) in &mine { span class={ "tally-n u" (i % 4) } { (n) } } }
                                    }
                                }
                            }
                        }
                    }
                }
        }
        div.split.even {
            section.card {
                div.card-head { div { h2 { "Calendario" } p { "Toca un día para ver qué se hizo o planificar algo." } } }
                (calendar(v))
                div.cal-legend {
                    @for (i, u) in v.users.iter().enumerate() { span { span class={ "cal-key u" (i % 4) } {} (u.name) } }
                    span { span.cal-key.plan {} "Planificada" }
                }
            }
            @if let Some(day) = v.day.clone().or_else(|| v.today.starts_with(&v.m).then(|| v.today.clone())) { (day_panel(v, &day)) }
        }
    }
}

fn new_chore(v: &View, with_ideas: bool) -> Markup {
    html! {
        details.add-chore #new-chore {
            summary.btn.outline.sm { (icon("plus")) "Nueva tarea" }
            div.add-body {
                @if with_ideas && !ideas(v).is_empty() {
                    p.label-sm.flush { "Ideas" }
                    (starter_chips(v))
                    hr.sep;
                }
                (chore_form(v, None))
            }
        }
    }
}
