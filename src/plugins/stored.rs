//! What plugins keep on this machine: each one's database (kept when it is uninstalled, so
//! reinstalling brings everything back) and programs left by plugins Nido no longer lists.
//! Each plugin card shows it only when it matters: when uninstalling, or when data outlived the plugin.
use super::*;

pub struct Stored {
    pub id: String,
    pub known: Option<&'static Known>,
    /// Database plus its WAL and shared-memory files.
    pub bytes: u64,
    pub program: bool,
}

impl Stored {
    /// A plugin that is installed can be switched off or uninstalled first; its data goes after.
    pub fn in_use(&self) -> bool {
        self.known.is_some_and(installed)
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Every plugin with a database or a program here, catalog order first.
pub fn stored() -> Vec<Stored> {
    fn entry(out: &mut Vec<Stored>, id: &str) -> usize {
        out.iter().position(|s| s.id == id).unwrap_or_else(|| {
            out.push(Stored { id: id.to_string(), known: find(id), bytes: 0, program: false });
            out.len() - 1
        })
    }
    let mut out: Vec<Stored> = Vec::new();
    if let Ok(files) = std::fs::read_dir(data()) {
        for f in files.filter_map(|e| e.ok()) {
            let name = f.file_name().to_string_lossy().into_owned();
            let id = name.strip_suffix(".db").or_else(|| name.strip_suffix(".db-wal")).or_else(|| name.strip_suffix(".db-shm"));
            if let Some(id) = id.filter(|id| valid_id(id)) {
                let i = entry(&mut out, id);
                out[i].bytes += f.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    if let Ok(files) = std::fs::read_dir(dir()) {
        for f in files.filter_map(|e| e.ok()).filter(|e| e.path().is_file()) {
            let name = f.file_name().to_string_lossy().into_owned();
            let stem = name.strip_suffix(std::env::consts::EXE_SUFFIX).unwrap_or(&name);
            if let Some(id) = stem.strip_prefix("nido-").filter(|id| valid_id(id)) {
                let i = entry(&mut out, id);
                out[i].program = true;
            }
        }
    }
    out.sort_by_key(|s| (s.known.map_or(usize::MAX, |k| ALL.iter().position(|x| x.id == k.id).unwrap_or(usize::MAX)), s.id.clone()));
    out
}

/// Erases a plugin's database for good and, for one Nido no longer lists, its program too.
/// Only ids found on disk are touched, so a crafted id cannot reach other files.
pub fn purge(c: &Connection, id: &str) -> Result<(), &'static str> {
    let Some(s) = stored().into_iter().find(|s| s.id == id) else { return Err("No hay datos de ese plugin") };
    if s.in_use() {
        return Err("Desinstálalo primero; después podrás borrar sus datos");
    }
    for ext in ["db", "db-wal", "db-shm"] {
        let _ = std::fs::remove_file(data().join(format!("{id}.{ext}")));
    }
    if s.known.is_none() && s.program {
        let _ = std::fs::remove_file(dir().join(format!("nido-{id}{}", std::env::consts::EXE_SUFFIX)));
    }
    let _ = c.execute("DELETE FROM settings WHERE key=?1", [format!("plugin:{id}")]);
    Ok(())
}

const BACKUPS_NOTE: &str = "Las copias de seguridad ya hechas los siguen guardando.";

/// The dialog behind «Desinstalar»: what the plugin keeps here, and whether to keep it.
/// Opening it is the first step; erasing is the second.
pub fn remove_dialog(v: &View, k: &Known, bytes: u64) -> Markup {
    let id = format!("rm-{}", k.id);
    html! {
        div.modal id=(id) role="dialog" aria-modal="true" aria-labelledby={ (id) "-t" } {
            a.scrim href="#_" aria-label="Cancelar" tabindex="-1" {}
            div.dialog {
                span.dialog-ico { (icon(k.icon)) }
                h2 id={ (id) "-t" } { "¿Desinstalar " (k.name) "?" }
                @if bytes > 0 {
                    p { "Guarda " b { (crate::pages::file_size(bytes as i64)) } " en este equipo. Si los conservas, al volver a instalarlo todo sigue como estaba." }
                } @else {
                    p { "No guarda datos todavía. Puedes volver a instalarlo cuando quieras." }
                }
                div.mk-choices {
                    form method="post" action={ "/plugins/" (k.id) } {
                        (v.ctx()) input type="hidden" name="action" value="uninstall";
                        button.btn.primary.wide { @if bytes > 0 { "Desinstalar y conservar datos" } @else { "Desinstalar" } }
                    }
                    @if bytes > 0 {
                        form method="post" action={ "/plugins/" (k.id) } {
                            (v.ctx()) input type="hidden" name="action" value="uninstall"; input type="hidden" name="purge" value="1";
                            button.btn.destructive.wide { "Desinstalar y borrar sus datos" }
                        }
                    }
                    a.btn.outline.wide href="#_" { "Cancelar" }
                }
                @if bytes > 0 { p.hint { (BACKUPS_NOTE) } }
            }
        }
    }
}

/// The dialog behind «Borrar datos», for data a plugin left when it was uninstalled or retired.
pub fn forget_dialog(v: &View, id: &str, name: &str, bytes: u64) -> Markup {
    let dlg = format!("rm-{id}");
    html! {
        div.modal id=(dlg) role="dialog" aria-modal="true" aria-labelledby={ (dlg) "-t" } {
            a.scrim href="#_" aria-label="Cancelar" tabindex="-1" {}
            div.dialog {
                span.dialog-ico { (icon("trash-2")) }
                h2 id={ (dlg) "-t" } { "¿Borrar los datos de " (name) "?" }
                p {
                    @if bytes > 0 { "Se borran para siempre " b { (crate::pages::file_size(bytes as i64)) } ". No se puede deshacer." }
                    @else { "Se borra el programa que quedó en el equipo." }
                }
                div.dialog-actions {
                    a.btn.outline href="#_" { "Cancelar" }
                    form.inline method="post" action={ "/plugins/" (id) } {
                        (v.ctx()) input type="hidden" name="action" value="purge";
                        button.btn.destructive { "Borrar para siempre" }
                    }
                }
                @if bytes > 0 { p.hint { (BACKUPS_NOTE) } }
            }
        }
    }
}

/// Programs and data of plugins Nido no longer lists: shown only when there are any.
pub fn leftovers(v: &View, kept: &[Stored]) -> Markup {
    let gone: Vec<&Stored> = kept.iter().filter(|s| s.known.is_none()).collect();
    if gone.is_empty() {
        return html! {};
    }
    html! {
        section.card.mk-leftovers {
            div.card-head { h2 { "Restos de plugins retirados" } }
            p.hint { "Plugins que ya no existen en Nido dejaron esto en el equipo. No se usan para nada." }
            ul.rows {
                @for s in &gone {
                    li.row {
                        span.tile.sm { (icon("package")) }
                        div.row-main {
                            span.row-title { (s.id) }
                            span.row-sub { @if s.bytes > 0 { (crate::pages::file_size(s.bytes as i64)) " de datos" } @else { "Solo el programa" } }
                        }
                        a.btn.ghost.sm href={ "#rm-" (s.id) } { "Borrar" }
                    }
                }
            }
        }
        @for s in &gone { (forget_dialog(v, &s.id, &s.id, s.bytes)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_stay_inside_the_plugins_folder() {
        assert!(valid_id("pets") && valid_id("notify-test_2"));
        for bad in ["", "../nido", "a/b", "Pets", "a.db", &"x".repeat(41)] {
            assert!(!valid_id(bad), "{bad}");
        }
    }
}
