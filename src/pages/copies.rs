use super::*;

pub async fn backups_page(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>) -> R {
    let (v, target) = {
        let c = lock(&db);
        let me = require_user!(c, h);
        let host = h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("nido.local").to_string();
        let m = month(&c, None)?;
        let target = backup::targets(&c)?.into_iter().find(|t| t.id == id);
        (View::load(&c, me, m, "household", host, None, None)?, target)
    };
    let Some(target) = target else { return Ok(StatusCode::NOT_FOUND.into_response()) };
    let (target, found) = tokio::task::spawn_blocking(move || {
        let found = backup::copies(&target);
        (target, found)
    })
    .await?;
    let body = v.shell(render(&v, &target, found), None);
    Ok(page("Copias · Nido", "", body))
}

fn render(v: &View, t: &backup::Target, found: Result<Vec<backup::Copy>, String>) -> Markup {
    let back = format!("{}#backups", v.link("household"));
    html! {
        div.page-head { div { a.back href=(back) { (icon("chevron-left")) "La casa" } } }
        section.card.sheet-page {
            div #sheet-content {
                div.dialog-top {
                    div {
                        h2 { "Copias en «" (t.name) "»" }
                        p.sheet-sub { (t.kind_label()) " · guarda las " (t.keep) " más recientes" }
                    }
                    a.icon-btn href=(back) data-close aria-label="Cerrar" { (icon("x")) }
                }
                @match found {
                    Err(e) => p.alert role="alert" { (icon("circle-alert")) "No se ha podido ver el destino: " (e) },
                    Ok(list) if list.is_empty() => div.sheet-empty {
                        span.backup-empty-ico { (icon("archive")) }
                        b { "Todavía no hay copias aquí" }
                        p { "La primera se hará a la hora programada, o ya mismo con «Copiar ahora»." }
                    },
                    Ok(list) => {
                        ul.rows.copies {
                            @for (i, c) in list.iter().enumerate() {
                                li.row {
                                    span.tile.sm { (icon("archive")) }
                                    div.row-main {
                                        span.row-title { (c.when()) }
                                        span.row-sub {
                                            @if let Some(s) = c.size { span.dest-meta { (settings::size(s)) } }
                                            @if i == 0 { span.dest-state.ok { (icon("check")) "La más reciente" } }
                                        }
                                    }
                                    a.btn.outline.sm.dl href={ "/backups/targets/" (t.id) "/files/" (c.name) } download=(c.name) aria-label={ "Descargar la copia del " (c.when()) } { (icon("download")) span.dl-txt { "Descargar" } }
                                    details.del-copy {
                                        summary.icon-btn aria-label={ "Borrar la copia del " (c.when()) } { (icon("trash-2")) (icon("x")) }
                                        form.inline method="post" action={ "/backups/targets/" (t.id) "/files/" (c.name) "/delete" } {
                                            button.btn.destructive.sm { "Borrar" }
                                        }
                                    }
                                }
                            }
                        }
                        p.hint.top { "Para recuperar una copia, descárgala y en la Pi ejecuta " code { "sudo nido-restore" } " con el archivo." }
                    }
                }
            }
        }
    }
}
