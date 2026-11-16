//! Day-to-day care: medication, health notes, walks and weight.
use super::*;

pub(super) fn med_section(c: &Connection, v: &V, pet: &Pet, ret: &str, edit: Option<i64>) -> rusqlite::Result<Markup> {
    let meds = data::meds(c, Some(pet.id))?;
    let from = add_days(&v.today, -13);
    let doses = data::doses(c, &from)?;
    let (done, current): (Vec<&Med>, Vec<&Med>) = meds.iter().partition(|m| m.over(&v.today));
    let editing = edit.and_then(|id| meds.iter().find(|m| m.id == id));
    Ok(html! {
        div.pt-bar {
            p.hint { "Ponle el nombre que queráis, sus horas y cuántos días dura. Cada toma se marca al dársela, y se ve quién lo hizo." }
            a.btn.primary href="#med-new" { (icon("plus")) "Añadir medicina" }
        }
        @if current.is_empty() {
            section.card.pt-card { (empty("pill", "Sin tratamientos ahora", "Cuando el veterinario le mande algo, apúntalo aquí y os avisaremos de cada toma.")) }
        }
        @for m in &current {
            section.card.pt-card.pt-medcard {
                div.pt-medcard-head {
                    span.pt-task-ico { (icon("pill")) }
                    div.pt-row-main {
                        h3.pt-row-title { (m.title) }
                        span.pt-row-sub { (course(m, &v.today)) }
                    }
                    a.icon-btn href=(v.link(&format!("{ret}&e={}#med-edit", m.id))) aria-label="Editar" title="Editar" { (icon("pencil")) }
                }
                @if let (Some(n), Some(st), Some(t)) = (m.days, days(&m.start), days(&v.today)) {
                    @let pct = (((t - st + 1).clamp(0, n) as f64 / n as f64) * 100.0).round();
                    div.pt-progress role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow=(pct) aria-label="Progreso del tratamiento" { span style={ "width:" (pct) "%" } {} }
                }
                @if m.active_on(&v.today) {
                    div.pt-today-doses {
                        span.pt-label { "Hoy" }
                        div.pt-doses { @for s in m.slots() { (dose_btn(v, m, &v.today, &s, &doses, ret)) } }
                    }
                }
                (dose_grid(v, m, &doses, ret))
                @if !m.note.is_empty() { p.pt-note { (m.note) } }
            }
        }
        @if !done.is_empty() {
            details.card.pt-card.pt-old {
                summary { (icon("archive")) "Tratamientos terminados" span.pt-count { (done.len()) } }
                div.pt-list {
                    @for m in &done {
                        a.pt-old-row href=(v.link(&format!("{ret}&e={}#med-edit", m.id))) {
                            span.pt-row-title { (m.title) }
                            span.pt-row-sub { (day_label(&m.start, &v.today)) " – " (m.last_day().map(|l| day_label(&l, &v.today)).unwrap_or_default()) }
                        }
                    }
                }
            }
        }
        @if let Some(m) = editing { (med_modal(v, pet, ret, Some(m))) }
    })
}

pub(super) fn dose_grid(v: &V, m: &Med, doses: &Given, ret: &str) -> Markup {
    let days_shown: Vec<String> = (1..=6).map(|i| add_days(&v.today, -i)).filter(|d| m.active_on(d)).collect();
    if days_shown.is_empty() {
        return html! {};
    }
    html! {
        details.pt-history {
            summary { "Días anteriores" }
            div.pt-grid {
                @for d in &days_shown {
                    div.pt-grid-row {
                        span.pt-label { (day_label(d, &v.today)) }
                        div.pt-doses { @for s in m.slots() { (dose_btn(v, m, d, &s, doses, ret)) } }
                    }
                }
            }
        }
    }
}

pub(super) fn med_modal(v: &V, pet: &Pet, ret: &str, m: Option<&Med>) -> Markup {
    let (id, title, action) = match m {
        Some(m) => ("med-edit", "Editar medicina", format!("/plugins/pets/med/{}", m.id)),
        None => ("med-new", "Nueva medicina", "/plugins/pets/med".to_string()),
    };
    let times: Vec<String> = m.map(|m| m.times.split(',').filter(|t| !t.is_empty()).map(str::to_string).collect()).unwrap_or_default();
    modal(id, title, html! {
        form.form method="post" action=(action) {
            (v.ctx(ret))
            input type="hidden" name="pet" value=(pet.id);
            div.field {
                label for={ (id) "-title" } { "Qué es" }
                input.input id={ (id) "-title" } name="title" required maxlength="60" autocomplete="off" placeholder="Antibiótico, gotas para los ojos…" value=[m.map(|m| &m.title)];
            }
            div.field {
                label for={ (id) "-dose" } { "Cuánto " span.pt-opt { "(opcional)" } }
                input.input id={ (id) "-dose" } name="dose" maxlength="60" autocomplete="off" placeholder="Media pastilla, 5 gotas…" value=[m.map(|m| &m.dose).filter(|d| !d.is_empty())];
            }
            fieldset.field.pt-fieldset {
                legend { "A qué horas" }
                div.pt-times {
                    @for i in 0..4 {
                        input.input type="time" name="time" aria-label={ "Hora " (i + 1) } value=[times.get(i)];
                    }
                }
                p.hint { "Una casilla por toma. Vacías si es una vez al día sin hora fija. Os avisaremos a cada hora." }
            }
            div.two {
                div.field {
                    label for={ (id) "-start" } { "Empieza" }
                    input.input type="date" id={ (id) "-start" } name="start" value=(m.map_or(v.today.as_str(), |m| m.start.as_str()));
                }
                div.field {
                    label for={ (id) "-days" } { "Durante (días)" }
                    input.input type="number" id={ (id) "-days" } name="days" min="1" max="365" inputmode="numeric" placeholder="Sin fin" value=[m.and_then(|m| m.days)];
                }
            }
            p.hint.pt-hint-up { "Por ejemplo 10 para «durante los próximos 10 días». Vacío si es para siempre." }
            div.field {
                label for={ (id) "-note" } { "Notas " span.pt-opt { "(opcional)" } }
                textarea.input id={ (id) "-note" } name="note" rows="2" maxlength="1000" placeholder="Con comida, en el ojo izquierdo…" { (m.map_or("", |m| m.note.as_str())) }
            }
            button.btn.primary.wide { (icon("check")) "Guardar" }
        }
        @if let Some(m) = m {
            div.pt-danger {
                @if m.stopped.is_empty() && !m.over(&v.today) {
                    form method="post" action={ "/plugins/pets/med/" (m.id) "/stop" } {
                        (v.ctx(ret)) button.btn.outline.sm { (icon("circle-check")) "Terminar hoy" }
                    }
                }
                (delete_form(v, &format!("/plugins/pets/med/{}/delete", m.id), ret, "Borrar medicina"))
            }
        }
    })
}

// ---------- health ----------

pub(super) fn health_row(v: &V, h: &data::Health, ret: &str) -> Markup {
    let (_, kname, kic) = data::health_kind(&h.kind);
    let (_, mname, mic, tone) = data::mood(&h.mood);
    html! {
        a.pt-health.pt-editable href=(v.link(&format!("{ret}&e={}#health-edit", h.id))) aria-label={ "Editar: " (kname) ", " (mname) } {
            span class={ "pt-ev-ico " (tone) } title=(mname) { (icon(kic)) }
            div.pt-row-main {
                span.pt-row-title { (kname) " " span class={ "pill " (tone) } { (icon(mic)) (mname) } }
                span.pt-row-sub { (&h.at[11..16]) " · " (v.name(h.by)) }
                @if !h.note.is_empty() { span.pt-row-note { (h.note) } }
            }
            span.pt-row-go aria-hidden="true" { (icon("pencil")) }
        }
    }
}

pub(super) fn health_section(c: &Connection, v: &V, pet: &Pet, ret: &str, edit: Option<i64>) -> rusqlite::Result<Markup> {
    let list = data::health(c, pet.id, 150)?;
    let editing = edit.and_then(|id| list.iter().find(|h| h.id == id));
    let week: Vec<String> = (0..7).rev().map(|i| add_days(&v.today, -i)).collect();
    let worst = |day: &str| {
        let moods: Vec<&str> = list.iter().filter(|h| h.at.starts_with(day)).map(|h| h.mood.as_str()).collect();
        ["bad", "so", "good"].into_iter().find(|m| moods.contains(m))
    };
    Ok(html! {
        div.pt-bar {
            p.hint { "Caca, pis, comida, vómitos o cómo está de ánimo: lo que queráis vigilar." }
            a.btn.primary href="#health" { (icon("plus")) "Apuntar" }
        }
        section.card.pt-card {
            (sec_head("calendar", "Esta semana", None))
            div.pt-week-days {
                @for d in &week {
                    div.pt-wd.today[*d == v.today] {
                        span.pt-wd-name { (weekday(d)) }
                        @match worst(d) {
                            Some(id) => { @let (_, name, ic, tone) = data::mood(id); span class={ "pt-wd-dot " (tone) } title={ (day_label(d, &v.today)) ": " (name.to_lowercase()) } { (icon(ic)) } }
                            None => span.pt-wd-dot.none title={ (day_label(d, &v.today)) ": nada apuntado" } {},
                        }
                        span.pt-wd-num { (d[8..10].trim_start_matches('0')) }
                    }
                }
            }
            p.pt-sub.pt-week-note { "Cada día muestra lo peor que se apuntó." }
        }
        @if list.is_empty() {
            section.card.pt-card { (empty("heart-pulse", "Nada apuntado", "Con los botones «Caca bien» y «Caca mal» se apunta con un toque.")) }
        } @else {
            section.card.pt-card { (clamp("health-list", day_groups(v, &list, |h| h.at.as_str(), |h| health_row(v, h, ret)), true)) }
        }
        @if let Some(h) = editing { (health_modal(v, pet, ret, Some(h))) }
    })
}

pub(super) fn health_modal(v: &V, pet: &Pet, ret: &str, h: Option<&data::Health>) -> Markup {
    let (id, title, action) = match h {
        Some(h) => ("health-edit", "Editar nota de salud".to_string(), format!("/plugins/pets/health/{}", h.id)),
        None => ("health", format!("Salud de {}", pet.name), "/plugins/pets/health".to_string()),
    };
    let kind = h.map_or(data::HEALTH[0].0, |h| h.kind.as_str());
    let mood = h.map_or(data::MOODS[0].0, |h| h.mood.as_str());
    modal(id, &title, html! {
        form.form method="post" action=(action) {
            (v.ctx(ret))
            input type="hidden" name="pet" value=(pet.id);
            fieldset.field.pt-fieldset {
                legend { "Qué" }
                div.pt-chips {
                    @for (k, name, ic) in data::HEALTH {
                        label.pt-chip { input type="radio" name="kind" value=(k) checked[*k == kind]; span { (icon(ic)) (name) } }
                    }
                }
            }
            fieldset.field.pt-fieldset {
                legend { "Cómo" }
                div.pt-moods {
                    @for (m, name, ic, tone) in data::MOODS {
                        label class={ "pt-moodpick " (tone) } { input type="radio" name="mood" value=(m) checked[*m == mood]; span { (icon(ic)) (name) } }
                    }
                }
            }
            div.field {
                label for={ (id) "-at" } { "Cuándo" }
                input.input id={ (id) "-at" } type="datetime-local" name="at" value=(h.map_or_else(|| v.now_local(), |h| h.at.replace(' ', "T")));
            }
            div.field {
                label for={ (id) "-note" } { "Notas " span.pt-opt { "(opcional)" } }
                textarea.input id={ (id) "-note" } name="note" rows="2" maxlength="1000" placeholder="Blanda, con sangre, no ha querido comer…" { (h.map_or("", |h| h.note.as_str())) }
            }
            button.btn.primary.wide { (icon("check")) @if h.is_some() { "Guardar" } @else { "Apuntar" } }
        }
        @if let Some(h) = h { div.pt-danger { (delete_form(v, &format!("/plugins/pets/health/{}/delete", h.id), ret, "Borrar")) } }
    })
}

// ---------- walks ----------

pub(super) fn walks_section(c: &Connection, v: &V, pet: &Pet, ret: &str, edit: Option<i64>) -> rusqlite::Result<Markup> {
    let month_ago = add_days(&v.today, -29);
    let list = data::walks_since(c, Some(pet.id), &month_ago)?;
    let editing = edit.and_then(|id| list.iter().find(|w| w.id == id));
    let mut by: Vec<(i64, usize)> = Vec::new();
    for id in list.iter().flat_map(|w| &w.people) {
        match by.iter_mut().find(|x| x.0 == *id) {
            Some(x) => x.1 += 1,
            None => by.push((*id, 1)),
        }
    }
    by.sort_by_key(|x| std::cmp::Reverse(x.1));
    let top = by.first().map_or(1, |x| x.1).max(1);
    let week: Vec<(String, usize)> = (0..7)
        .rev()
        .map(|i| add_days(&v.today, -i))
        .map(|d| {
            let n = list.iter().filter(|w| w.at.starts_with(d.as_str())).count();
            (d, n)
        })
        .collect();
    let most = week.iter().map(|w| w.1).max().unwrap_or(0).max(3);
    Ok(html! {
        div.pt-bar {
            p.hint { "Quién lo saca y cuándo, para que nadie se quede sin paseo ni salga dos veces seguidas." }
            a.btn.primary href="#walk" { (icon("plus")) "Apuntar paseo" }
        }
        div.pt-home {
            section.card.pt-card {
                (sec_head("calendar", "Esta semana", None))
                div.pt-cols role="img" aria-label={ "Paseos de los últimos 7 días: " (week.iter().map(|w| w.1.to_string()).collect::<Vec<_>>().join(", ")) } {
                    @for (d, n) in &week {
                        div.pt-col.today[*d == v.today] {
                            b { @if *n > 0 { (n) } }
                            span.pt-col-bar { span style={ "height:" (n * 100 / most) "%" } {} }
                            small { (weekday(d)) }
                        }
                    }
                }
                @if !by.is_empty() {
                    div.pt-subhead { "Quién lo pasea · 30 días" }
                    div.pt-bars {
                        @for (id, n) in &by {
                            div.pt-barrow {
                                (v.avatar(*id))
                                span.pt-barname { (v.name(*id)) }
                                span.pt-barline { span style={ "width:" (n * 100 / top) "%" } {} }
                                b { (n) }
                            }
                        }
                    }
                }
            }
            section.card.pt-card {
                (sec_head("footprints", "Paseos", None))
                @if list.is_empty() {
                    p.pt-none { "Sin paseos este mes. Pulsa «Paseado» al volver a casa." }
                } @else {
                    (clamp("walk-list", day_groups(v, &list, |w| w.at.as_str(), |w| html! {
                            a.pt-walk.pt-editable href=(v.link(&format!("{ret}&e={}#walk-edit", w.id))) aria-label={ "Editar el paseo de las " (&w.at[11..16]) } {
                                (v.faces(&w.people))
                                div.pt-row-main {
                                    span.pt-row-title { (v.names(&w.people)) }
                                    span.pt-row-sub {
                                        "A las " (&w.at[11..16])
                                        @if let Some(m) = w.minutes { " · " (m) " min" }
                                        @if !w.note.is_empty() { " · " (w.note) }
                                    }
                                }
                                span.pt-row-go aria-hidden="true" { (icon("pencil")) }
                            }
                    }), true))
                }
            }
        }
        @if let Some(w) = editing { (walk_modal(v, pet, ret, Some(w))) }
    })
}

pub(super) fn walk_modal(v: &V, pet: &Pet, ret: &str, w: Option<&data::Walk>) -> Markup {
    let (id, title, action) = match w {
        Some(w) => ("walk-edit", "Editar paseo".to_string(), format!("/plugins/pets/walk/{}", w.id)),
        None => ("walk", format!("Paseo de {}", pet.name), "/plugins/pets/walk".to_string()),
    };
    let went = |p: i64| w.map_or(p == v.me.id, |w| w.people.contains(&p));
    modal(id, &title, html! {
        form.form method="post" action=(action) {
            (v.ctx(ret))
            input type="hidden" name="pet" value=(pet.id);
            @if v.me.people.len() > 1 {
                fieldset.field.pt-fieldset {
                    legend { "Quién lo ha paseado" }
                    div.pt-chips {
                        @for p in &v.me.people {
                            label.pt-chip.pt-who { input type="checkbox" name="by" value=(p.id) checked[went(p.id)]; span { (v.avatar(p.id)) (if p.id == v.me.id { "Yo" } else { p.name.as_str() }) } }
                        }
                    }
                    p.hint { "Marca a todos los que fueron." }
                }
            }
            div.two {
                div.field {
                    label for={ (id) "-at" } { "Cuándo" }
                    input.input id={ (id) "-at" } type="datetime-local" name="at" value=(w.map_or_else(|| v.now_local(), |w| w.at.replace(' ', "T")));
                }
                div.field {
                    label for={ (id) "-min" } { "Minutos " span.pt-opt { "(opcional)" } }
                    input.input id={ (id) "-min" } type="number" name="minutes" min="1" max="600" inputmode="numeric" placeholder="30" value=[w.and_then(|w| w.minutes)];
                }
            }
            div.field {
                label for={ (id) "-note" } { "Notas " span.pt-opt { "(opcional)" } }
                input.input id={ (id) "-note" } name="note" maxlength="200" autocomplete="off" placeholder="Ha hecho caca, por el parque…" value=[w.map(|w| &w.note).filter(|n| !n.is_empty())];
            }
            button.btn.primary.wide { (icon("check")) @if w.is_some() { "Guardar" } @else { "Apuntar" } }
        }
        @if let Some(w) = w { div.pt-danger { (delete_form(v, &format!("/plugins/pets/walk/{}/delete", w.id), ret, "Borrar paseo")) } }
    })
}

// ---------- weight ----------

pub(super) fn weight_section(v: &V, pet: &Pet, list: &[data::Weight], ret: &str, edit: Option<i64>) -> Markup {
    let last = list.last();
    let editing = edit.and_then(|id| list.iter().find(|w| w.id == id));
    let prev = list.len().checked_sub(2).and_then(|i| list.get(i));
    html! {
        div.pt-bar {
            @match (last, prev) {
                (Some(l), Some(p)) => {
                    @let d = l.kg - p.kg;
                    p.pt-trend {
                        b { (kg(l.kg)) }
                        @if d.abs() < 0.0005 { span.pill { "Igual que " (rel(&p.day, &v.today)) } }
                        @else {
                            span class={ "pill " (if d > 0.0 { "warn" } else { "accent" }) } {
                                (icon(if d > 0.0 { "trending-up" } else { "trending-down" }))
                                (if d > 0.0 { "+" } else { "−" }) (kg(d.abs())) " desde " (day_label(&p.day, &v.today).to_lowercase())
                            }
                        }
                    }
                }
                (Some(l), None) => p.pt-trend { b { (kg(l.kg)) } span.pill { (day_label(&l.day, &v.today)) } },
                _ => p.hint { "Pésalo de vez en cuando para ver cómo va." },
            }
            a.btn.primary href="#weight" { (icon("plus")) "Apuntar peso" }
        }
        @if list.is_empty() {
            section.card.pt-card { (empty("weight", "Sin pesos todavía", "El primero que apuntes empieza su gráfica.")) }
        } @else {
            @if list.len() > 1 { section.card.pt-card style=(tint(pet)) { (chart(v, list)) } }
            section.card.pt-card {
                (clamp("weight-list", list.iter().rev().map(|w| html! {
                    a.pt-weight.pt-editable href=(v.link(&format!("{ret}&e={}#weight-edit", w.id))) aria-label={ "Editar el peso del " (day_label(&w.day, &v.today).to_lowercase()) } {
                        span.pt-row-title { (kg(w.kg)) }
                        span.pt-row-sub { (day_label(&w.day, &v.today)) " · " (v.name(w.by)) }
                        span.pt-row-go aria-hidden="true" { (icon("pencil")) }
                    }
                }).collect(), false))
            }
        }
        @if let Some(w) = editing { (weight_modal(v, pet, ret, None, Some(w))) }
    }
}

/// Drawn on the server, so the page needs no chart library.
pub(super) fn chart(v: &V, list: &[data::Weight]) -> Markup {
    let pts: Vec<&data::Weight> = list.iter().rev().take(24).rev().collect();
    let (lo, hi) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), w| (a.min(w.kg), b.max(w.kg)));
    let pad = ((hi - lo) * 0.2).max(0.1);
    let (lo, hi) = ((lo - pad).max(0.0), hi + pad);
    let (w, h) = (600.0, 180.0);
    let (d0, d1) = (days(&pts[0].day).unwrap_or(0), days(&pts[pts.len() - 1].day).unwrap_or(0));
    let span = (d1 - d0).max(1) as f64;
    let xy: Vec<(f64, f64)> = pts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let x = if d1 > d0 { (days(&p.day).unwrap_or(d0) - d0) as f64 / span } else { i as f64 / (pts.len() - 1).max(1) as f64 };
            (16.0 + x * (w - 32.0), 12.0 + (1.0 - (p.kg - lo) / (hi - lo)) * (h - 24.0))
        })
        .collect();
    let line: String = xy.iter().map(|(x, y)| format!("{x:.1},{y:.1}")).collect::<Vec<_>>().join(" ");
    let area = format!("16,{h} {line} {:.1},{h}", xy.last().map_or(w, |p| p.0));
    let alt = format!("Peso de {} a {} entre {} y {}", kg(pts[0].kg), kg(pts[pts.len() - 1].kg), day_label(&pts[0].day, &v.today).to_lowercase(), day_label(&pts[pts.len() - 1].day, &v.today).to_lowercase());
    html! {
        figure.pt-chart {
            svg viewBox={ "0 0 " (w) " " (h) } role="img" aria-label=(alt) preserveAspectRatio="none" {
                polygon.pt-chart-area points=(area) {}
                polyline.pt-chart-line points=(line) vector-effect="non-scaling-stroke" {}
            }
            div.pt-chart-dots aria-hidden="true" {
                @for ((x, y), p) in xy.iter().zip(&pts) {
                    span style={ "left:" (format!("{:.2}", x / w * 100.0)) "%;top:" (format!("{:.2}", y / h * 100.0)) "%" } title={ (kg(p.kg)) " · " (day_label(&p.day, &v.today)) } {}
                }
            }
            figcaption {
                span { (day_label(&pts[0].day, &v.today)) }
                span { (kg(lo + pad)) " – " (kg(hi - pad)) }
                span { (day_label(&pts[pts.len() - 1].day, &v.today)) }
            }
        }
    }
}

pub(super) fn weight_modal(v: &V, pet: &Pet, ret: &str, last: Option<f64>, w: Option<&data::Weight>) -> Markup {
    let (id, title, action) = match w {
        Some(w) => ("weight-edit", "Editar peso".to_string(), format!("/plugins/pets/weight/{}", w.id)),
        None => ("weight", format!("Peso de {}", pet.name), "/plugins/pets/weight".to_string()),
    };
    let comma = |k: f64| format!("{k}").replace('.', ",");
    modal(id, &title, html! {
        form.form method="post" action=(action) {
            (v.ctx(ret))
            input type="hidden" name="pet" value=(pet.id);
            div.two {
                div.field {
                    label for={ (id) "-kg" } { "Kilos" }
                    input.input id={ (id) "-kg" } name="kg" required inputmode="decimal" autocomplete="off" placeholder=(last.map_or("4,5".into(), comma)) pattern="[0-9]+([.,][0-9]+)?" value=[w.map(|w| comma(w.kg))];
                }
                div.field {
                    label for={ (id) "-day" } { "Día" }
                    input.input id={ (id) "-day" } type="date" name="day" max=(v.today) value=(w.map_or(v.today.as_str(), |w| w.day.as_str()));
                }
            }
            p.hint { "En kilos: 0,35 para 350 gramos." }
            button.btn.primary.wide { (icon("check")) @if w.is_some() { "Guardar" } @else { "Apuntar" } }
        }
        @if let Some(w) = w { div.pt-danger { (delete_form(v, &format!("/plugins/pets/weight/{}/delete", w.id), ret, "Borrar peso")) } }
    })
}
