//! What a phone sees when it reads a tag: `/t/<code>` does the actions and sends it to the scan's
//! own page (`/t/r/<id>`), so reloading shows what happened instead of doing it again.
use super::*;
use store::{Run, Tag};

struct Face {
    name: String,
    photo: i64,
    color: String,
}

fn face_of(c: &Connection, id: i64) -> rusqlite::Result<Face> {
    let rows: Vec<(i64, String, i64, Option<String>)> = c
        .prepare_cached("SELECT id,name,photo,color FROM users ORDER BY id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    let i = rows.iter().position(|u| u.0 == id).unwrap_or(0);
    Ok(rows.into_iter().nth(i).map_or(Face { name: "?".into(), photo: 0, color: PALETTE[0].into() }, |(_, name, photo, color)| Face {
        name,
        photo,
        color: color.filter(|c| PALETTE.contains(&c.as_str())).unwrap_or_else(|| PALETTE[i % PALETTE.len()].into()),
    }))
}

fn shell(c: &Connection, me: &User, title: &str, card: Markup) -> R {
    let theme: Option<String> = c.query_row("SELECT value FROM settings WHERE key='theme'", [], |r| r.get(0)).optional()?;
    let f = face_of(c, me.id)?;
    let body = html! {
        main.tap-page {
            header.tap-top {
                a.tap-home href="/" { img src="/icon.png" alt="" width="28" height="28"; span { "Nido" } }
                span.tap-me {
                    @if f.photo > 0 { img.avatar src={ "/avatars/" (me.id) "?v=" (f.photo) } alt="" width="28" height="28"; }
                    @else { span.avatar style={ "background:" (f.color) } aria-hidden="true" { (initial(&f.name)) } }
                    (f.name)
                }
            }
            (card)
        }
    };
    Ok(page(&format!("{title} · Nido"), &format!("tap-body theme-{}", theme_id(theme.as_deref().unwrap_or(""))), body))
}

fn head(tone: &str, ic: &str, place: &str, title: &str) -> Markup {
    html! {
        div.tap-head {
            span class={ "tap-mark " (tone) } { (icon(ic)) }
            div {
                p.tap-place { (icon("nfc")) (place) }
                h1 #tap-t { (title) }
            }
        }
    }
}

fn hidden(name: &str, value: impl std::fmt::Display) -> Markup {
    html! { input type="hidden" name=(name) value=(value.to_string()); }
}

fn edit_link(tag: &Tag) -> Markup {
    html! { a href={ "/?tab=shortcuts&e=" (tag.id) "#tag-edit" } { "Cambiar este atajo" } }
}

/// The tag's actions before doing them (confirm) or while choosing.
fn plain_lines(tag: &Tag) -> Markup {
    html! {
        ul.tap-lines {
            @for s in &tag.steps {
                li.tap-line { span.tap-ico { (icon(&s.icon)) } div.tap-txt { b { (s.label) } } }
            }
        }
    }
}

/// `main`: the page's own button already goes there, so the line does not repeat it.
fn result_lines(tag: &Tag, run: &Run, main: Option<&str>) -> Markup {
    html! {
        ul.tap-lines {
            @for l in &run.lines {
                @let st = if run.undone { "undone" } else { l.status.as_str() };
                li class={ "tap-line " (st) } {
                    span.tap-ico { (icon(&l.icon)) }
                    div.tap-txt {
                        b { (l.label) }
                        @if run.undone { small { "Deshecho" } } @else if !l.text.is_empty() { small { (l.text) } }
                    }
                    @if !run.undone {
                        @match Status::from_id(&l.status) {
                            Status::Already => form method="post" action={ "/t/" (tag.code) } { (hidden("force", l.pos)) button.btn.outline.sm { "Apuntar igualmente" } },
                            Status::Failed => form method="post" action={ "/t/" (tag.code) } { (hidden("pick", l.pos)) button.btn.ghost.sm { "Reintentar" } },
                            _ => @if let Some(h) = l.href.as_deref().filter(|h| Some(*h) != main) { a.btn.ghost.sm href=(h) { "Ver" } },
                        }
                    }
                }
            }
        }
    }
}

/// Title of a scan's page from what happened to each action.
fn verdict(run: &Run) -> (&'static str, &'static str, &'static str) {
    let count = |s: &str| run.lines.iter().filter(|l| l.status == s).count();
    let (failed, already, n) = (count("failed"), count("already"), run.lines.len());
    match () {
        _ if run.undone => ("mute", "undo-2", "Deshecho"),
        _ if failed == n => ("bad", "circle-alert", "No se ha podido"),
        _ if failed > 0 => ("bad", "circle-alert", "Una parte no se ha podido"),
        _ if already == n => ("warn", "info", "Ya estaba apuntado"),
        _ if already > 0 => ("warn", "info", "Apuntado, con un aviso"),
        _ => ("ok", "circle-check", "Apuntado"),
    }
}

fn result_card(c: &Connection, me: &User, tag: &Tag, run: &Run) -> rusqlite::Result<Markup> {
    let (tone, ic, title) = verdict(run);
    let wrote = run.lines.iter().any(|l| l.status == "done");
    let who = match () {
        _ if !wrote || run.undone => "Hoy".to_string(),
        _ if run.user_id == me.id => "A tu nombre".to_string(),
        _ => format!("A nombre de {}", face_of(c, run.user_id)?.name),
    };
    let go = run.lines.iter().find(|l| l.status == "done" && !run.undone).and_then(|l| l.href.clone());
    let go_name = go.as_deref().and_then(|h| h.split("tab=").nth(1)).map(|t| t.split('&').next().unwrap_or("")).map(|t| match plugins::find(t) {
        Some(k) => k.name,
        None => match t {
            "shopping" => "Compra",
            "tasks" => "Tareas",
            "expenses" => "Gastos",
            _ => "Nido",
        },
    });
    Ok(html! {
        section.card.tap-card aria-labelledby="tap-t" {
            (head(tone, ic, &tag.name, title))
            (result_lines(tag, run, go.as_deref()))
            div.tap-actions {
                @if let (Some(h), Some(n)) = (&go, go_name) { a.btn.primary.wide href=(h) { "Ver en " (n) } }
                @else { a.btn.primary.wide href="/" { "Abrir Nido" } }
                @if run.can_undo(me.id) {
                    form method="post" action={ "/t/r/" (run.id) "/undo" } { button.btn.outline.wide { (icon("undo-2")) "Deshacer" } }
                }
            }
            p.tap-foot { (who) " · " (run.at.get(11..16).unwrap_or("")) " · " (edit_link(tag)) }
        }
    })
}

fn confirm_card(tag: &Tag) -> Markup {
    html! {
        section.card.tap-card aria-labelledby="tap-t" {
            (head("accent", "nfc", &tag.name, "¿Lo hacemos?"))
            (plain_lines(tag))
            div.tap-actions {
                form method="post" action={ "/t/" (tag.code) } { (hidden("go", 1)) button.btn.primary.wide { (icon("check")) "Sí, hacerlo" } }
                a.btn.ghost.wide href="/" { "Ahora no" }
            }
            p.tap-foot { "Se hará a tu nombre · " (edit_link(tag)) }
        }
    }
}

fn choose_card(tag: &Tag) -> Markup {
    html! {
        section.card.tap-card aria-labelledby="tap-t" {
            (head("accent", "nfc", &tag.name, "¿Qué apuntamos?"))
            div.tap-picks {
                @for s in &tag.steps {
                    form method="post" action={ "/t/" (tag.code) } {
                        (hidden("pick", s.pos))
                        button.tap-pick { span.tap-ico { (icon(&s.icon)) } span { (s.label) } (icon("chevron-right")) }
                    }
                }
            }
            p.tap-foot { "Se apunta a tu nombre · " (edit_link(tag)) }
        }
    }
}

fn repeat_card(me: &User, tag: &Tag, run: &Run) -> Markup {
    let ago = match run.minutes { 0 => "hace nada".to_string(), 1 => "hace 1\u{a0}min".to_string(), m => format!("hace {m}\u{a0}min") };
    html! {
        section.card.tap-card aria-labelledby="tap-t" {
            (head("warn", "clock", &tag.name, &format!("Ya lo apuntaste {ago}")))
            p.tap-note { "El móvil a veces lee la etiqueta dos veces. No se ha apuntado nada nuevo." }
            (result_lines(tag, run, None))
            div.tap-actions {
                a.btn.primary.wide href={ "/t/r/" (run.id) } { "Ver lo apuntado" }
                form method="post" action={ "/t/" (tag.code) } { (hidden("again", 1)) button.btn.outline.wide { (icon("refresh-cw")) "Apuntar otra vez" } }
                @if run.can_undo(me.id) {
                    form method="post" action={ "/t/r/" (run.id) "/undo" } { button.btn.ghost.wide { (icon("undo-2")) "Deshacer lo de antes" } }
                }
            }
        }
    }
}

fn unknown_card(code: &str) -> Markup {
    html! {
        section.card.tap-card aria-labelledby="tap-t" {
            (head("mute", "nfc", "Etiqueta sin atajo", "Esta etiqueta no hace nada todavía"))
            p.tap-note { "Puede que su atajo se borrara, o que la acabes de grabar. Dile qué tiene que hacer y funcionará sin volver a grabarla." }
            div.tap-actions {
                a.btn.primary.wide href={ "/?tab=shortcuts&code=" (code) "#tag-new" } { (icon("plus")) "Darle un atajo" }
                a.btn.ghost.wide href="/" { "Abrir Nido" }
            }
        }
    }
}

fn to_login(code: &str) -> R {
    Ok(Redirect::to(&format!("/login?next=/t/{code}")).into_response())
}

/// `GET /t/<code>`: what the phone opens when it reads the tag.
pub async fn scan_get(State(db): State<Db>, h: HeaderMap, Path(code): Path<String>) -> R {
    if !store::valid_code(&code) {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let (me, tag) = {
        let c = lock(&db);
        let Some(me) = current_user(&c, &h) else { return to_login(&code) };
        let tag = store::by_code(&c, &code)?;
        let Some(tag) = tag else { return shell(&c, &me, "Etiqueta", unknown_card(&code)) };
        if tag.choose {
            return shell(&c, &me, &tag.name, choose_card(&tag));
        }
        if tag.asks() {
            return shell(&c, &me, &tag.name, confirm_card(&tag));
        }
        if let Some(run) = store::recent(&c, tag.id, me.id)?.map(|id| store::run(&c, id)).transpose()?.flatten() {
            return shell(&c, &me, &tag.name, repeat_card(&me, &tag, &run));
        }
        (me, tag)
    };
    go(&db, &me, &tag, &tag.steps.iter().collect::<Vec<_>>(), false).await
}

/// Does the steps and sends the phone to what happened (or straight to the place a tag opens).
async fn go(db: &Db, me: &User, tag: &Tag, steps: &[&store::Step], force: bool) -> R {
    let id = perform(db, me, tag.id, steps, force).await?;
    if tag.just_opens() {
        let run = store::run(&lock(db), id)?;
        if let Some(h) = run.and_then(|r| r.lines.first().and_then(|l| l.href.clone())) {
            return Ok(Redirect::to(&h).into_response());
        }
    }
    Ok(Redirect::to(&format!("/t/r/{id}")).into_response())
}

/// `POST /t/<code>`: confirmed (`go`), picked from the menu (`pick`), done anyway (`force`) or
/// done again after a repeated read (`again`).
pub async fn scan_post(State(db): State<Db>, h: HeaderMap, Path(code): Path<String>, F(f): F<Vec<(String, String)>>) -> R {
    let (me, tag) = {
        let c = lock(&db);
        let Some(me) = current_user(&c, &h) else { return to_login(&code) };
        let Some(tag) = store::by_code(&c, &code)? else { return bad("Esta etiqueta ya no tiene atajo") };
        (me, tag)
    };
    let pos = |k: &str| f.iter().find(|p| p.0 == k).and_then(|p| p.1.parse::<i64>().ok());
    let (one, force) = match (pos("pick"), pos("force")) {
        (Some(p), _) => (Some(p), false),
        (None, Some(p)) => (Some(p), true),
        (None, None) => (None, false),
    };
    let steps: Vec<&store::Step> = match one {
        Some(p) => tag.steps.iter().filter(|s| s.pos == p).collect(),
        None if tag.choose => return Ok(Redirect::to(&format!("/t/{code}")).into_response()),
        None => tag.steps.iter().collect(),
    };
    if steps.is_empty() {
        return bad("Esa acción ya no está en el atajo");
    }
    go(&db, &me, &tag, &steps, force).await
}

/// `GET /t/r/<id>`: a scan's result, only for whoever scanned.
pub async fn run_page(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let Some(run) = store::run(&c, id)?.filter(|r| r.user_id == me.id) else { return Ok(Redirect::to("/").into_response()) };
    let Some(tag) = store::by_id(&c, run.tag_id)? else { return Ok(Redirect::to("/").into_response()) };
    let card = result_card(&c, &me, &tag, &run)?;
    shell(&c, &me, &tag.name, card)
}

/// `POST /t/r/<id>/undo`: takes back everything that scan wrote.
pub async fn undo_run(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>) -> R {
    let (me, run) = {
        let c = lock(&db);
        let me = require_user!(c, h);
        let run = store::run(&c, id)?;
        (me, run)
    };
    let Some(run) = run else { return bad("Eso ya no está apuntado.") };
    if !run.can_undo(me.id) {
        return bad(match () {
            _ if run.user_id != me.id => "Solo puede deshacerlo quien lo apuntó.",
            _ if run.undone => "Ya está deshecho.",
            _ if run.undo.is_empty() => "No hay nada que deshacer: no se apuntó nada nuevo.",
            _ => "Ya no se puede deshacer: han pasado más de 30 minutos. Cámbialo desde su sección.",
        });
    }
    let mut any = false;
    for (source, token) in &run.undo {
        any |= undo(&db, &me, source, token).await;
    }
    if !any {
        return bad("No se ha podido deshacer. Ábrelo en su sección para cambiarlo o borrarlo.");
    }
    store::mark_undone(&lock(&db), run.id)?;
    Ok(Redirect::to(&format!("/t/r/{}", run.id)).into_response())
}

fn plain(status: StatusCode, text: String) -> R {
    Ok((status, [(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response())
}

/// `POST /api/t/<code>` with `Authorization: Bearer <token>`: for a phone automation ("when
/// this NFC tag is scanned"), which runs without opening anything. Answers one line per action,
/// ready for a notification.
pub async fn api_run(State(db): State<Db>, h: HeaderMap, Path(code): Path<String>) -> R {
    let (me, tag) = {
        let c = lock(&db);
        let tok = h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).unwrap_or("");
        let me = c
            .query_row("SELECT id,name,salary,token FROM users WHERE token=?1 AND token<>''", [tok], |r| Ok(User { id: r.get(0)?, name: r.get(1)?, salary: r.get(2)?, token: r.get(3)? }))
            .optional()?;
        let Some(me) = me else { return plain(StatusCode::UNAUTHORIZED, "La llave no es válida. Cópiala otra vez de Ajustes → Atajos.".into()) };
        let Some(tag) = store::by_code(&c, &code)? else { return plain(StatusCode::NOT_FOUND, "Esa etiqueta ya no tiene atajo.".into()) };
        if tag.choose {
            return plain(StatusCode::CONFLICT, "Esta etiqueta pide elegir: ábrela con el navegador.".into());
        }
        if store::recent(&c, tag.id, me.id)?.is_some() {
            return plain(StatusCode::OK, "Ya lo apuntaste hace un momento.".into());
        }
        (me, tag)
    };
    let id = perform(&db, &me, tag.id, &tag.steps.iter().collect::<Vec<_>>(), false).await?;
    let run = store::run(&lock(&db), id)?;
    let text = run.map(|r| r.lines.iter().map(|l| if l.text.is_empty() { l.label.clone() } else { l.text.clone() }).collect::<Vec<_>>().join("\n")).unwrap_or_default();
    plain(StatusCode::OK, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(statuses: &[&str], undone: bool) -> Run {
        let lines = statuses.iter().enumerate().map(|(i, s)| store::Line { pos: i as i64, label: String::new(), icon: "zap".into(), status: s.to_string(), text: String::new(), href: None }).collect();
        Run { id: 1, tag_id: 1, user_id: 1, at: String::new(), lines, undo: Vec::new(), undone, minutes: 0 }
    }

    #[test]
    fn verdicts() {
        assert_eq!(verdict(&run(&["done", "open"], false)).2, "Apuntado");
        assert_eq!(verdict(&run(&["done", "already"], false)).2, "Apuntado, con un aviso");
        assert_eq!(verdict(&run(&["already"], false)).2, "Ya estaba apuntado");
        assert_eq!(verdict(&run(&["done", "failed"], false)).2, "Una parte no se ha podido");
        assert_eq!(verdict(&run(&["failed"], false)).2, "No se ha podido");
        assert_eq!(verdict(&run(&["done"], true)).2, "Deshecho");
    }
}
