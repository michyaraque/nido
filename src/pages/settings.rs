use super::*;

fn sections(v: &View) -> Markup {
    html! {
        nav.seg.settings-seg aria-label="Ajustes" {
            a.on[v.t == "settings"] href=(v.link("settings")) aria-current=[(v.t == "settings").then_some("page")] { (icon("users")) "Mis ajustes" }
            a.on[v.t == "household"] href=(v.link("household")) aria-current=[(v.t == "household").then_some("page")] { (icon("house")) "La casa" }
        }
    }
}

pub fn render(v: &View) -> Markup {
    html! {
        (v.head("Ajustes", html! { "Tu perfil, tus avisos y este dispositivo." }))
        (sections(v))
        div.settings {
            div.stack {
                section.card #profile {
                    div.card-head { span.stat-ico { (icon("users")) } div { h2 { "Tu perfil" } p { "Tu foto aparece en el calendario, en las cuentas y junto a lo que apuntas." } } }
                    div.profile {
                        @if let Some(u) = v.users.iter().find(|u| u.id == v.me.id) {
                            @if u.photo > 0 {
                                img.avatar.big src={ "/avatars/" (u.id) "?v=" (u.photo) } alt={ "Foto de " (u.name) };
                            } @else {
                                span.avatar.big.p0 { (initial(&u.name)) }
                            }
                        }
                        div.profile-actions {
                            b { (v.me.name) }
                            form.photo-form method="post" action="/me/avatar" enctype="multipart/form-data" {
                                label.btn.outline.sm { input type="file" name="photo" accept="image/*" required; (icon("camera")) "Cambiar foto" }
                                button.btn.primary.sm.photo-go { "Subir" }
                            }
                            @if v.users.iter().any(|u| u.id == v.me.id && u.photo > 0) {
                                form method="post" action="/me/avatar/delete" { button.btn.ghost.sm { "Quitar foto" } }
                            }
                        }
                    }
                }
                section.card #salary {
                    div.card-head { span.stat-ico { (icon("banknote")) } div { h2 { "Tu sueldo" } p { "Lo que cobras neto cada mes. Con él calculamos tu parte de la casa y lo que te sobra." } } }
                    form.form method="post" action="/me/salary" accept-charset="utf-8" {
                        (v.ctx())
                        div.field { label for="s" { "Sueldo neto mensual" } input #s.input type="text" name="salary" inputmode="decimal" value=(eur_input(v.me.salary)); }
                        button.btn.primary { "Guardar sueldo" }
                    }
                }
                @if let Some(tg) = &v.tg { (telegram(v, tg)) }
            }
            div.stack {
                section.card {
                    div.card-head { span.stat-ico { (icon("smartphone")) } div { h2 { "Atajo de iPhone" } p { "Registra cada pago con Apple Pay sin abrir la app. El iPhone tiene que poder llegar a este servidor, por ejemplo desde la wifi de casa." } } }
                    div.form {
                        div.field {
                            label for="u" { "Dirección" }
                            input #u.input.mono type="text" readonly value={ "http://" (v.host) "/api/expenses" } onfocus="this.select()";
                        }
                        div.field {
                            label for="k" { "Cabecera Authorization" }
                            input #k.input.mono type="text" readonly value={ "Bearer " (v.me.token) } onfocus="this.select()";
                            p.hint { "Es tu llave personal. No la compartas: quien la tenga puede apuntar gastos a tu nombre." }
                        }
                        details.steps {
                            summary { (icon("chevron-right")) "Cómo crear el Atajo" }
                            ol {
                                li { "En Atajos, abre " b { "Automatización" } " y crea una nueva de tipo " b { "Transacción" } "." }
                                li { "Elige tus tarjetas y marca " b { "Ejecutar inmediatamente" } "." }
                                li { "Añade la acción " b { "Obtener contenido de URL" } " con la dirección de arriba y el método POST." }
                                li { "Añade la cabecera " b { "Authorization" } " con el valor de arriba." }
                                li { "En el cuerpo JSON, pon " code { "amount" } " = Importe y " code { "description" } " = Comercio." }
                            }
                        }
                    }
                }
                section.card #offline {
                    div.card-head { span.stat-ico { (icon("cloud-off")) } div { h2 { "Usar sin conexión" } p { "Fuera de casa, Nido abre con lo último que viste y guarda lo que apuntes; al volver a la wifi de casa lo envía solo. El iPhone necesita para ello el certificado de casa (una vez por móvil)." } } }
                    details.steps {
                        summary { (icon("chevron-right")) "Cómo activarlo en el iPhone" }
                        ol {
                            li {
                                "Abre la app " b { "Safari" } " (no el icono de Nido ni Chrome: no pueden instalar perfiles), pega esta dirección y pulsa " b { "Permitir" } ":"
                                input.input.mono.top type="text" readonly value="http://nido.local/nido.crt" onfocus="this.select()" aria-label="Dirección del certificado";
                            }
                            li { "Ve a Ajustes → General → " b { "VPN y gestión de dispositivos" } ", elige " b { "Nido · certificado de casa" } " e instálalo." }
                            li { "Ve a Ajustes → General → Información → " b { "Ajustes de confianza de certificados" } " y activa Nido · certificado de casa." }
                            li { "Abre " a href="https://nido.local" { "https://nido.local" } ", entra y añádela a la pantalla de inicio (Compartir → Añadir a pantalla de inicio). Puedes borrar el icono antiguo." }
                        }
                    }
                }
                section.card {
                    div.card-head { span.stat-ico { (icon("log-out")) } div { h2 { "Sesión" } p { "Has entrado como " b { (v.me.name) } ". Cerrar la sesión solo afecta a este dispositivo." } } }
                    form method="post" action="/logout" { button.btn.danger { (icon("log-out")) "Cerrar sesión" } }
                    p.hint.top { "Versión " (VERSION) " · " (merchants::LOGOS.len()) " comercios con logo" }
                }
            }
        }
    }
}

pub fn render_house(v: &View) -> Markup {
    let month_lc = v.month_lc();
    html! {
        (v.head("Ajustes", html! { "Lo que comparte toda la casa. Lo puede cambiar cualquiera." }))
        (sections(v))
        div.settings {
            div.stack {
                section.card #house {
                    div.card-head { span.stat-ico { (icon("house")) } div { h2 { "Control de la casa" } p { "Afecta a toda la casa. Lo puede cambiar cualquiera." } } }
                    form.form.auto-submit method="post" action="/house/settings" {
                        (v.ctx())
                        input type="hidden" name="present" value="1";
                        label.switch {
                            input type="checkbox" name="share_expenses" checked[v.share_expenses];
                            span { "Ver los gastos de los demás" small { "Cada uno podrá ver los gastos del día a día de toda la casa. Solo el autor puede editarlos o borrarlos." } }
                        }
                        button.btn.outline.sm.auto-go { "Guardar" }
                    }
                }
                @if let Some(tg) = &v.tg { (house_bot(v, tg)) }
                @if let Some(b) = &v.backups { (backups(v, b)) }
            }
            div.stack {
                section.card #new-bill {
                    div.card-head { span.stat-ico { (icon("calendar-plus")) } div { h2 { "Nueva cuenta mensual" } p { "Un pago que se repite cada mes, como el alquiler o la luz. Aparecerá desde " (month_lc) "." } } }
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
        }
    }
}

fn house_bot(v: &View, tg: &crate::notify::Panel) -> Markup {
    html! {
        section.card #bot {
        div.card-head { span.stat-ico { (icon("send")) } div { h2 { "Bot de Telegram" } p { "Envía los avisos de casa. Se conecta una vez; después cada persona se vincula en Mis ajustes." } } }
        @match &tg.bot {
            None => {
                details.steps {
                    summary { (icon("chevron-right")) "Crear el bot de la casa (una vez, 2 minutos)" }
                    ol {
                        li { "En Telegram, abre " a href="https://t.me/BotFather" target="_blank" rel="noopener" { "@BotFather" } " y escribe " code { "/newbot" } "." }
                        li { "Ponle un nombre, por ejemplo " b { "Nido de casa" } ", y un usuario que acabe en " code { "bot" } "." }
                        li { "Copia el " b { "token" } " que te da y pégalo aquí." }
                    }
                }
                form.form.top method="post" action="/telegram/bot" accept-charset="utf-8" {
                    (v.ctx())
                    div.field {
                        label for="tgt" { "Token del bot" }
                        input #tgt.input.mono type="text" name="token" placeholder="123456789:AAH…" required autocomplete="off" spellcheck="false";
                        p.hint { "Se guarda solo en Nido. Después, cada persona se vincula en Mis ajustes → Tus avisos." }
                    }
                    button.btn.primary { (icon("send")) "Conectar bot" }
                }
            }
            Some(bot) => {
                p.hint { "Conectado: " b { "@" (bot) } ". Cada persona se vincula en Mis ajustes → Tus avisos." }
                form.form.top method="post" action="/telegram/time" {
                    (v.ctx())
                    div.field {
                        label for="tga" { "Hora del recordatorio diario" }
                        div.inline-field {
                            input #tga.input type="time" name="at" value=(tg.at) required;
                            button.btn.outline { "Guardar" }
                        }
                        p.hint { "Si queda algo pendiente. Los domingos, también el resumen de la semana." }
                    }
                }
                form.form.top method="post" action="/telegram/bot" accept-charset="utf-8" {
                    (v.ctx())
                    div.field {
                        label for="tgt" { "Cambiar el token" }
                        div.inline-field.grow {
                            input #tgt.input.mono type="text" name="token" placeholder="Nuevo token de @BotFather" required autocomplete="off" spellcheck="false";
                            button.btn.outline { "Cambiar" }
                        }
                    }
                }
                a.btn.danger.sm.top href="#remove-bot" { "Quitar el bot" }
                div.modal #remove-bot role="dialog" aria-modal="true" aria-labelledby="remove-bot-t" {
                    a.scrim href="#_" aria-label="Cancelar" tabindex="-1" {}
                    div.dialog {
                        span.dialog-ico { (icon("bell-ring")) }
                        h2 #remove-bot-t { "¿Quitar el bot de Telegram?" }
                        p { "Nadie de la casa recibirá más avisos. Para volver a tenerlos habrá que conectarlo y vincularse otra vez." }
                        div.dialog-actions {
                            a.btn.outline href="#_" { "Cancelar" }
                            form.inline method="post" action="/telegram/remove" { (v.ctx()) button.btn.destructive { "Quitar bot" } }
                        }
                    }
                }
            }
        }
        }
    }
}

fn telegram(v: &View, tg: &crate::notify::Panel) -> Markup {
    let me_linked = tg.linked.iter().any(|(u, l)| *u == v.me.id && *l);
    html! {
        section.card #telegram {
            div.card-head {
                span.stat-ico { (icon("bell-ring")) }
                div { h2 { "Tus avisos" } p { "En Telegram: cuando te planifican una tarea, lo pendiente cada día y un resumen los domingos, con un botón para marcar cada tarea como hecha." } }
            }
            @match &tg.bot {
                None => p.hint { "Primero hay que conectar el bot de la casa, en " a href=(v.link("household")) { "Ajustes → La casa" } "." },
                Some(bot) => {
                    ul.rows.compact {
                        @for (u, linked) in &tg.linked {
                            li.row {
                                (v.avatar(*u))
                                span.row-main { span.row-title { (v.name_of(*u)) @if *u == v.me.id { span.muted { " (tú)" } } } }
                                @if *linked { (pill("ok", Some("check"), "Recibe avisos")) } @else { (pill("", None, "Sin vincular")) }
                            }
                        }
                    }
                    div.actions.top {
                        @if me_linked {
                            form.inline method="post" action="/telegram/test" { (v.ctx()) button.btn.outline.sm { (icon("send")) "Enviar prueba" } }
                            form.inline method="post" action="/telegram/unlink" { (v.ctx()) button.btn.outline.sm { "Dejar de recibir" } }
                        } @else {
                            a.btn.primary href={ "https://t.me/" (bot) "?start=" (tg.code) } target="_blank" rel="noopener" { (icon("external-link")) "Vincular mi Telegram" }
                        }
                    }
                    @if !me_linked { p.hint { "Se abrirá el chat con @" (bot) ": pulsa " b { "Iniciar" } " y vuelve aquí." } }
                }
            }
        }
    }
}

pub fn size(bytes: i64) -> String {
    let (gb, mb) = (bytes as f64 / 1e9, bytes as f64 / 1e6);
    let n = |x: f64| format!("{x:.1}").replace('.', ",").replace(",0", "");
    if gb >= 1.0 { format!("{} GB", n(gb)) } else if mb >= 1.0 { format!("{} MB", n(mb)) } else { format!("{} KB", (bytes / 1000).max(1)) }
}

/// "Hoy, 04:00", "Ayer, 04:00" or "13/11, 04:00" from "YYYY-MM-DD HH:MM:SS".
fn when(b: &crate::backup::Panel, at: &str) -> String {
    let (day, time) = (&at[..10], &at[11..16]);
    if day == b.today {
        format!("Hoy, {time}")
    } else if day == b.yesterday {
        format!("Ayer, {time}")
    } else {
        format!("{}/{}, {time}", &day[8..], &day[5..7])
    }
}

fn backups(v: &View, b: &crate::backup::Panel) -> Markup {
    let failed: Vec<&crate::backup::Target> = b.targets.iter().filter(|t| t.last_ok == Some(false)).collect();
    let last = b.targets.iter().filter(|t| t.last_ok == Some(true)).filter_map(|t| t.last_at.as_deref()).max();
    let next = format!("{} a las {}", if b.now.as_str() < b.at.as_str() { "hoy" } else { "mañana" }, b.at);
    html! {
        section.card #backups {
            div.card-head.backup-head {
                span.stat-ico { (icon("database-backup")) }
                div { h2 { "Copias de seguridad" } p { "Cifradas con vuestra contraseña antes de salir de la Pi." } }
            }
            @if !b.has_pass {
                div.backup-start {
                    p { "Primero, una contraseña para cifrar las copias. Guardadla fuera de Nido: " b { "sin ella no se pueden recuperar." } }
                    form.form method="post" action="/backups/password" {
                        (v.ctx())
                        div.two {
                            div.field { label for="bp1" { "Contraseña" } input #bp1.input type="password" name="pass" minlength="8" required autocomplete="new-password"; }
                            div.field { label for="bp2" { "Repítela" } input #bp2.input type="password" name="pass2" minlength="8" required autocomplete="new-password"; }
                        }
                        button.btn.primary { (icon("key-round")) "Guardar contraseña" }
                    }
                }
            } @else if b.targets.is_empty() {
                div.backup-empty {
                    span.backup-empty-ico { (icon("hard-drive")) }
                    h3 { "Elige dónde guardarlas" }
                    p { "Un pendrive conectado a la Pi, la nube o vuestro NAS. Podéis usar varios a la vez." }
                    a.btn.primary href="#add-dest" { (icon("plus")) "Añadir destino" }
                }
            } @else {
                div class={ "backup-status " (if !failed.is_empty() { "bad" } else if last.is_some() { "ok" } else { "" }) } data-refresh=[b.running.then_some("2")] {
                    span.backup-status-ico { (icon(if !failed.is_empty() { "circle-alert" } else if last.is_some() { "circle-check" } else { "calendar-clock" })) }
                    div {
                        b {
                            @if let Some(t) = failed.first() { "La última copia en «" (t.name) "» falló" }
                            @else if let Some(at) = last { "Todo a salvo · última copia " (when(b, at).to_lowercase()) }
                            @else { "Aún no hay ninguna copia" }
                        }
                        span { "Siguiente copia " (next) }
                    }
                    div.backup-actions {
                    details.menu.backup-time {
                        summary.btn.ghost.sm { (icon("clock")) "Cambiar hora" }
                        div.pop {
                            form method="post" action="/backups/time" {
                                (v.ctx())
                                div.field { label for="bat" { "Cada día a las" } input #bat.input type="time" name="at" value=(b.at) required; }
                                p.hint { "Mejor de noche, cuando nadie usa Nido." }
                                button.btn.primary.wide { "Guardar" }
                            }
                        }
                    }
                    form.inline method="post" action="/backups/run" {
                        (v.ctx())
                        @if b.running { button.btn.outline.sm disabled { (icon("loader-circle")) "Copiando…" } }
                        @else { button.btn.primary.sm { (icon("play")) "Copiar ahora" } }
                    }
                    }
                }
                ul.rows.dests {
                    @for t in &b.targets {
                        li.row {
                            span.tile.sm { (icon(t.icon())) }
                            div.row-main {
                                span.row-title { (t.name) }
                                span.row-sub {
                                    span.dest-meta { (t.kind_label()) " · guarda " (t.keep) }
                                    @match (t.last_ok, t.last_at.as_deref()) {
                                        (Some(true), Some(at)) => span.dest-state.ok { (icon("check")) (when(b, at)) @if let Some(sz) = t.last_size { " · " (size(sz)) } },
                                        (Some(false), _) => span.dest-state.bad { (icon("circle-alert")) "Falló" },
                                        _ => span.dest-state { "Sin copias" },
                                    }
                                }
                                @if t.last_ok == Some(false) { span.backup-err { (t.last_error.as_deref().unwrap_or("")) } }
                            }
                            details.menu {
                                summary.icon-btn aria-label={ "Opciones de " (t.name) } { (icon("ellipsis")) }
                                div.pop {
                                    a.btn.outline.wide href={ "/backups/targets/" (t.id) } data-sheet { (icon("archive")) "Ver las copias" }
                                    form method="post" action={ "/backups/targets/" (t.id) "/test" } { (v.ctx()) button.btn.outline.wide { (icon("refresh-cw")) "Probar la conexión" } }
                                    hr;
                                    form method="post" action={ "/backups/targets/" (t.id) } accept-charset="utf-8" {
                                        (v.ctx())
                                        div.field { label { "Nombre" } input.input type="text" name="name" value=(t.name) required maxlength="60"; }
                                        div.field { label { "Copias que guardar" } input.input type="number" name="keep" value=(t.keep) min="1" max="365" required; p.hint { "Al pasar de este número se borra la más antigua." } }
                                        button.btn.primary.wide { "Guardar" }
                                    }
                                    a.btn.danger.wide href={ "#rm-backup-" (t.id) } { (icon("x")) "Quitar destino" }
                                }
                            }
                            div.modal id={ "rm-backup-" (t.id) } role="dialog" aria-modal="true" {
                                a.scrim href="#_" aria-label="Cancelar" tabindex="-1" {}
                                div.dialog {
                                    span.dialog-ico { (icon("database-backup")) }
                                    h2 { "¿Quitar «" (t.name) "»?" }
                                    p { "No se harán más copias ahí. Las que ya están guardadas no se borran." }
                                    div.dialog-actions {
                                        a.btn.outline href="#_" { "Cancelar" }
                                        form.inline method="post" action={ "/backups/targets/" (t.id) "/delete" } { (v.ctx()) button.btn.destructive { "Quitar destino" } }
                                    }
                                }
                            }
                        }
                    }
                }
                a.btn.outline.sm.top href="#add-dest" { (icon("plus")) "Añadir destino" }
            }
            @if b.has_pass { (add_destination(v, b)) }
        }
    }
}

/// The "Añadir destino" window: four kinds, only the chosen one's form shows (CSS :has, no script).
fn add_destination(v: &View, b: &crate::backup::Panel) -> Markup {
    let used: Vec<&str> = b.targets.iter().filter_map(|t| t.uuid()).collect();
    let free: Vec<&crate::backup::Disk> = b.disks.iter().filter(|d| !used.contains(&d.uuid.as_str())).collect();
    let field = |label: &str, name: &str, ph: &str, kind: &str| html! {
        div.field { label { (label) } input.input type=(kind) name=(name) placeholder=(ph) autocomplete="off" spellcheck="false" required[name != "prefix" && name != "region"]; }
    };
    let keep = html! { div.field.keep-field { label { "Copias que guardar" } input.input type="number" name="keep" value="14" min="1" max="365"; } };
    let kinds = [
        ("usb", "hard-drive", "Disco USB", "Un pendrive en la Pi"),
        ("s3", "cloud", "Nube S3", "R2, Backblaze, AWS…"),
        ("dav", "server", "NAS o Nextcloud", "Por WebDAV"),
        ("dir", "folder", "Carpeta", "Ruta montada (avanzado)"),
    ];
    html! {
        div.modal #add-dest role="dialog" aria-modal="true" aria-labelledby="add-dest-t" {
            a.scrim href="#_" aria-label="Cerrar" tabindex="-1" {}
            div.dialog.dialog-wide {
                div.dialog-top {
                    h2 #add-dest-t { "Añadir destino" }
                    a.icon-btn href="#_" aria-label="Cerrar" { (icon("x")) }
                }
                div.kind-pick role="radiogroup" aria-label="Tipo de destino" {
                    @for (i, (id, ic, title, sub)) in kinds.iter().enumerate() {
                        label.kind { input type="radio" name="dest-kind" value=(id) checked[i == 0]; span.kind-ico { (icon(ic)) } b { (title) } small { (sub) } }
                    }
                }
                div.kind-pane.pane-usb {
                    @if free.is_empty() {
                        div.no-disk {
                            p { b { "No vemos ningún disco USB libre." } " Conecta un pendrive o un disco a la Pi y vuelve a buscar." }
                            a.btn.outline.sm href={ (v.link("household")) "&scan=1#add-dest" } { (icon("refresh-cw")) "Buscar de nuevo" }
                        }
                    } @else {
                        form method="post" action="/backups/targets" {
                            (v.ctx())
                            input type="hidden" name="kind" value="local";
                            div.disk-pick {
                                @for (i, d) in free.iter().enumerate() {
                                    label.disk-opt {
                                        input type="radio" name="uuid" value=(d.uuid) checked[i == 0] required;
                                        span.kind-ico { (icon("hard-drive")) }
                                        span.disk-txt {
                                            b { (d.label) }
                                            small {
                                                @match d.avail { Some(a) => { (size(a)) " libres de " (size(d.size)) }, None => (size(d.size)) }
                                                @if !d.model.is_empty() && d.model != d.label { " · " (d.model) }
                                            }
                                        }
                                    }
                                }
                            }
                            div.pane-foot {
                                (keep)
                                button.btn.primary { (icon("check")) "Usar este disco" }
                            }
                            p.hint { "Las copias van a la carpeta «Nido copias» del disco. Nido lo monta solo cuando hace falta, también tras reiniciar la Pi." }
                        }
                    }
                }
                div.kind-pane.pane-s3 {
                    form.form method="post" action="/backups/targets" accept-charset="utf-8" {
                        (v.ctx()) input type="hidden" name="kind" value="s3";
                        (field("Nombre", "name", "Cloudflare R2", "text"))
                        (field("Dirección (endpoint)", "endpoint", "https://<cuenta>.r2.cloudflarestorage.com", "url"))
                        div.two { (field("Bucket", "bucket", "nido-copias", "text")) (field("Región", "region", "auto", "text")) }
                        div.two { (field("Access key", "key", "", "text")) (field("Secret key", "secret", "", "password")) }
                        (field("Carpeta dentro del bucket (opcional)", "prefix", "casa", "text"))
                        div.pane-foot { (keep) button.btn.primary { (icon("check")) "Añadir" } }
                    }
                }
                div.kind-pane.pane-dav {
                    form.form method="post" action="/backups/targets" accept-charset="utf-8" {
                        (v.ctx()) input type="hidden" name="kind" value="webdav";
                        (field("Nombre", "name", "Nextcloud", "text"))
                        (field("Dirección de la carpeta", "url", "https://nube.ejemplo.es/remote.php/dav/files/usuario/Nido", "url"))
                        div.two { (field("Usuario", "user", "", "text")) (field("Contraseña de aplicación", "pass", "", "password")) }
                        div.pane-foot { (keep) button.btn.primary { (icon("check")) "Añadir" } }
                    }
                }
                div.kind-pane.pane-dir {
                    form.form method="post" action="/backups/targets" accept-charset="utf-8" {
                        (v.ctx()) input type="hidden" name="kind" value="local";
                        (field("Nombre", "name", "NAS del salón", "text"))
                        (field("Carpeta", "path", "/mnt/nas/nido", "text"))
                        p.hint { "Una carpeta ya montada dentro de /media o /mnt en la que Nido pueda escribir." }
                        div.pane-foot { (keep) button.btn.primary { (icon("check")) "Añadir" } }
                    }
                }
            }
        }
    }
}
