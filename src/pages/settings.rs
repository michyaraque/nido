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
    let has_photo = v.users.iter().any(|u| u.id == v.me.id && u.photo > 0);
    let main = html! {
        (sec("profile", "Tu perfil", html! { "Cómo te ven los demás y cómo entras." }, html! {
            div.card.set-card {
                (item("Foto", "Aparece en el calendario, en las cuentas y junto a lo que apuntas.", html! {
                    div.profile {
                        @if let Some(u) = v.users.iter().find(|u| u.id == v.me.id) {
                            @if u.photo > 0 { img.avatar.big src={ "/avatars/" (u.id) "?v=" (u.photo) } alt={ "Foto de " (u.name) }; }
                            @else { span class={ "avatar big p" (v.users.iter().position(|x| x.id == u.id).unwrap_or(0)) } { (initial(&u.name)) } }
                        }
                        div.profile-actions {
                            form.photo-form method="post" action="/me/avatar" enctype="multipart/form-data" {
                                label.btn.outline.sm { input type="file" name="photo" accept="image/*" required; (icon("camera")) "Cambiar foto" }
                                button.btn.primary.sm.photo-go { "Subir" }
                            }
                            @if has_photo { form method="post" action="/me/avatar/delete" { button.btn.ghost.sm { "Quitar foto" } } }
                        }
                    }
                }))
                (item("Contraseña", "La que usas para entrar en Nido.", html! {
                    a.btn.outline.sm href="#password" { (icon("key-round")) "Cambiar" }
                }))
            }
            div.modal #password role="dialog" aria-modal="true" aria-labelledby="password-t" {
                a.scrim href="#_" aria-label="Cerrar" tabindex="-1" {}
                div.dialog {
                    div.dialog-top {
                        h2 #password-t { "Cambiar contraseña" }
                        a.icon-btn href="#_" aria-label="Cerrar" { (icon("x")) }
                    }
                    form.form method="post" action="/me/password" accept-charset="utf-8" {
                        (v.ctx())
                        input type="text" name="username" value=(v.me.name) autocomplete="username" hidden;
                        div.field { label for="pc" { "Contraseña actual" } input #pc.input type="password" name="current" required autocomplete="current-password"; }
                        div.field { label for="pn" { "Nueva" } input #pn.input type="password" name="new" required minlength="4" autocomplete="new-password"; }
                        div.field {
                            label for="pr" { "Repítela" }
                            input #pr.input type="password" name="repeat" required minlength="4" autocomplete="new-password";
                            p.hint { "Se cerrará la sesión en tus otros dispositivos." }
                        }
                        button.btn.primary.wide { "Cambiar contraseña" }
                    }
                }
            }
        }))
        (sec("money", "Tus finanzas", html! { "Con esto calculamos tu saldo y tu parte de la casa." }, html! {
            div.card.set-card {
                div.set-item #salary {
                    div.set-item-txt { label for="s" { b { "Ingreso fijo mensual" } } small { "Nómina, pensión… neto. 0 si no tienes." } }
                    form.inline-field method="post" action="/me/salary" accept-charset="utf-8" {
                        (v.ctx())
                        input #s.input type="text" name="salary" inputmode="decimal" value=(eur_input(v.me.salary));
                        button.btn.outline { "Guardar" }
                    }
                }
                @if v.me.salary > 0 {
                    (item("Confirmar cada mes que lo cobro", "Si se retrasa, no contará en tu saldo hasta que lo marques.", html! {
                        form.auto-submit method="post" action="/me/track-pay" {
                            (v.ctx())
                            label.toggle { input type="checkbox" name="track" checked[v.track_pay] aria-label="Confirmar cada mes que lo cobro"; span {} }
                            button.btn.outline.sm.auto-go { "Guardar" }
                        }
                    }))
                }
            }
        }))
        @if let Some(tg) = &v.tg {
            (sec("avisos", "Tus avisos", html! { "Lo que te toca, por Telegram." }, telegram(v, tg)))
        }
        (sec("device", "Este dispositivo", html! { "Atajos y uso sin conexión. Se configura en cada móvil." }, html! {
            div.card.set-card {
                div.set-item.stacked {
                    div.set-item-txt { b { "Atajo de iPhone" } small { "Registra cada pago con Apple Pay sin abrir la app. El iPhone tiene que poder llegar a este servidor, por ejemplo desde la wifi de casa." } }
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
                div.set-item.stacked #offline {
                    div.set-item-txt { b { "Usar sin conexión" } small { "Fuera de casa, Nido abre con lo último que viste y guarda lo que apuntes; al volver a la wifi de casa lo envía solo. El iPhone necesita el certificado de casa (una vez por móvil)." } }
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
                (item("Cerrar sesión", "Solo afecta a este dispositivo.", html! {
                    form method="post" action="/logout" { button.btn.danger.sm { (icon("log-out")) "Cerrar sesión" } }
                }))
            }
        }))
    };
    html! {
        (layout(v, html! { "Tu perfil, tus avisos y este dispositivo" }, main))
    }
}

/// Shared by both settings tabs: one centred column of blocks.
fn layout(v: &View, sub: Markup, main: Markup) -> Markup {
    html! { div.set-page { (v.head("Ajustes", sub)) (sections(v)) div.set-list { (main) } } }
}

/// One settings block: heading above, its card below.
fn sec(id: &str, title: &str, desc: Markup, body: Markup) -> Markup {
    html! {
        section.set-group id=(id) aria-labelledby={ (id) "-t" } {
            div.set-head { h2 id={ (id) "-t" } { (title) } p { (desc) } }
            (body)
        }
    }
}

/// A line inside a settings card: label + hint, control beside (or below with `.stacked`).
fn item(title: &str, hint: &str, control: Markup) -> Markup {
    html! { div.set-item { div.set-item-txt { b { (title) } small { (hint) } } (control) } }
}

pub fn render_house(v: &View) -> Markup {
    html! {
        (layout(v, html! { "Lo que comparte toda la casa. Lo puede cambiar cualquiera." }, html! {
            (sec("house", "General", html! { "Afecta a todos los de la casa." }, html! {
                div.card.set-card {
                    div.set-item.stacked #theme {
                        div.set-item-txt { b { "Tema" } small { "Los colores de Nido. En modo oscuro se adaptan solos." } }
                        form.auto-submit method="post" action="/house/theme" {
                            (v.ctx())
                            div.theme-pick role="radiogroup" aria-label="Tema" {
                                @for (id, name, desc, [side, surface, accent]) in THEMES {
                                    label.theme-opt {
                                        input type="radio" name="theme" value=(id) checked[id == v.theme];
                                        span.theme-swatch style={ "--s1:" (side) ";--s2:" (surface) ";--s3:" (accent) } { span {} span {} span {} }
                                        span.theme-txt { b { (name) } small { (desc) } }
                                    }
                                }
                            }
                            button.btn.outline.sm.auto-go { "Guardar" }
                        }
                    }
                    (item("Ver los gastos de los demás", "Cada uno ve el día a día de toda la casa. Solo el autor puede editarlos o borrarlos.", html! {
                        form.auto-submit method="post" action="/house/settings" {
                            (v.ctx())
                            input type="hidden" name="present" value="1";
                            label.toggle { input type="checkbox" name="share_expenses" checked[v.share_expenses] aria-label="Ver los gastos de los demás"; span {} }
                            button.btn.outline.sm.auto-go { "Guardar" }
                        }
                    }))
                }
            }))
            (sec("people", "Personas", html! { "Quién vive en casa y su color." }, people(v)))
            @if let Some(tg) = &v.tg {
                (sec("bot-set", "Avisos por Telegram", html! { "Un bot para toda la casa. Cada uno se vincula en Mis ajustes." }, house_bot(v, tg)))
            }
            @if let Some(b) = &v.backups {
                (sec("backups-set", "Copias de seguridad", html! { "Cifradas con vuestra contraseña antes de salir de la Pi." }, backups(v, b)))
            }
        }))
    }
}

fn swatches(current: &str) -> Markup {
    html! {
        div.swatches role="radiogroup" aria-label="Color" {
            @for c in PALETTE {
                label.swatch { input type="radio" name="color" value=(c) checked[c == current] aria-label={ "Color " (c) }; span style={ "--c:" (c) } {} }
            }
        }
    }
}

/// Ajustes de la casa → Personas: everyone with their colour, and a way to add someone.
fn people(v: &View) -> Markup {
    let next = PALETTE[v.users.len() % PALETTE.len()];
    html! {
        div.card.set-card {
            @for u in &v.users {
                div.set-item {
                    div.member {
                        (v.avatar(u.id))
                        b { (u.name) @if u.id == v.me.id { span.muted { " (tú)" } } }
                    }
                    details.menu.backup-time {
                        summary.btn.ghost.sm aria-label={ "Color de " (u.name) } { span.color-dot style={ "--c:" (u.color) } {} "Color" }
                        div.pop {
                            p.pop-title { "Color de " (u.name) }
                            form.auto-submit method="post" action={ "/users/" (u.id) "/color" } {
                                (v.ctx())
                                (swatches(&u.color))
                                button.btn.primary.wide.auto-go { "Guardar" }
                            }
                        }
                    }
                }
            }
            div.set-item {
                div.set-item-txt { b { "Añadir persona" } small { "Entrará con su nombre y contraseña." } }
                a.btn.outline.sm href="#add-member" { (icon("plus")) "Añadir" }
            }
        }
        div.modal #add-member role="dialog" aria-modal="true" aria-labelledby="add-member-t" {
            a.scrim href="#_" aria-label="Cerrar" tabindex="-1" {}
            div.dialog {
                div.dialog-top {
                    h2 #add-member-t { "Añadir persona" }
                    a.icon-btn href="#_" aria-label="Cerrar" { (icon("x")) }
                }
                form.form method="post" action="/house/members" accept-charset="utf-8" {
                    (v.ctx())
                    div.field { label for="mn" { "Nombre" } input #mn.input type="text" name="name" required maxlength="40" autocomplete="off"; }
                    div.field {
                        label for="mp" { "Contraseña" }
                        input #mp.input type="password" name="password" required minlength="4" autocomplete="new-password";
                        p.hint { "Mínimo 4 caracteres. Díselo a esa persona." }
                    }
                    div.field { label { "Color" } (swatches(next)) }
                    button.btn.primary.wide { (icon("plus")) "Añadir" }
                }
            }
        }
    }
}

fn house_bot(v: &View, tg: &crate::notify::Panel) -> Markup {
    let step = |n: u8, title: Markup, hint: Markup| html! {
        li { span.tg-n { (n) } div { b { (title) } small { (hint) } } }
    };
    html! {
        div.card #bot {
        @match &tg.bot {
            None => {
                div.backup-empty {
                    span.backup-empty-ico { (icon("send")) }
                    h3 { "Conecta el bot" }
                    p { "Tareas, pendientes y resumen semanal." }
                }
                ol.tg-steps {
                    (step(1, html! { "Crea el bot" }, html! { "En " a href="https://t.me/BotFather" target="_blank" rel="noopener" { "@BotFather" } ", escribe " code { "/newbot" } }))
                    (step(2, html! { "Ponle nombre" }, html! { "El usuario acaba en " code { "bot" } }))
                    (step(3, html! { "Pega el token" }, html! { "Te lo da @BotFather al final" }))
                }
                form.form method="post" action="/telegram/bot" accept-charset="utf-8" {
                    (v.ctx())
                    div.inline-field.grow {
                        input #tgt.input.mono type="text" name="token" placeholder="123456789:AAH…" aria-label="Token del bot" required autocomplete="off" spellcheck="false";
                        button.btn.primary { (icon("send")) "Conectar" }
                    }
                }
            }
            Some(bot) => {
                div.backup-status.tg-status {
                    (bot_avatar(tg, false))
                    div {
                        b { (tg.title.as_deref().unwrap_or(bot.as_str())) }
                        span { "@" (bot) " · " span.ok-txt { "Conectado" } }
                    }
                    div.tg-actions {
                        a.btn.ghost.sm href={ "https://t.me/" (bot) } target="_blank" rel="noopener" { (icon("external-link")) "Abrir" }
                        details.menu.backup-time {
                            summary.btn.ghost.sm { (icon("clock")) (tg.at) }
                            div.pop {
                                form method="post" action="/telegram/time" {
                                    (v.ctx())
                                    div.field { label for="tga" { "Recordatorio diario a las" } input #tga.input type="time" name="at" value=(tg.at) required; }
                                    p.hint { "Solo si hay pendientes" }
                                    button.btn.primary.wide { "Guardar" }
                                }
                            }
                        }
                        details.menu.backup-time {
                            summary.icon-btn aria-label="Más opciones del bot" { (icon("ellipsis")) }
                            div.pop {
                                form method="post" action="/telegram/bot" accept-charset="utf-8" {
                                    (v.ctx())
                                    div.field { label for="tgt" { "Cambiar el token" } input #tgt.input.mono type="text" name="token" placeholder="Nuevo token de @BotFather" required autocomplete="off" spellcheck="false"; }
                                    button.btn.outline.wide { "Cambiar" }
                                }
                                hr;
                                a.btn.danger.wide href="#remove-bot" { (icon("x")) "Quitar el bot" }
                            }
                        }
                    }
                }
                p.tg-schedule { (icon("calendar-clock")) span { "A las " b { (tg.at) } " si hay pendientes · resumen los domingos" } }
                ul.rows.dests {
                    @for (u, linked) in &tg.linked {
                        li.row {
                            (v.avatar(*u))
                            div.row-main {
                                span.row-title { (v.name_of(*u)) @if *u == v.me.id { span.muted { " (tú)" } } }
                                span.row-sub {
                                    @if *linked { span.dest-state.ok { (icon("check")) "Recibe avisos" } }
                                    @else { span.dest-state { "Sin vincular" } }
                                }
                            }
                        }
                    }
                }
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

/// The bot's photo (or a Telegram-blue fallback), shared by both settings tabs.
fn bot_avatar(tg: &crate::notify::Panel, big: bool) -> Markup {
    html! {
        @if tg.photo > 0 { img.tg-avatar.big[big] src={ "/telegram/photo?v=" (tg.photo) } alt="" width="44" height="44"; }
        @else { span.tg-avatar.big[big] { (icon("send")) } }
    }
}

/// "Tus avisos" in Mis ajustes: only about me; who else is linked lives in La casa.
fn telegram(v: &View, tg: &crate::notify::Panel) -> Markup {
    let me_linked = tg.linked.iter().any(|(u, l)| *u == v.me.id && *l);
    html! {
        div.card #telegram {
            @match &tg.bot {
                None => {
                    div.backup-empty {
                        span.backup-empty-ico { (icon("bell-ring")) }
                        h3 { "Aún no hay bot" }
                        p { "Se conecta una vez para toda la casa." }
                        a.btn.outline href={ (v.link("household")) "#bot-set" } { (icon("send")) "Conectar bot" }
                    }
                }
                Some(bot) if !me_linked => {
                    div.backup-empty {
                        (bot_avatar(tg, true))
                        h3 { "Avisos en Telegram" }
                        p { "Tus tareas del día. Márcalas hechas desde el chat." }
                        a.btn.primary href={ "https://t.me/" (bot) "?start=" (tg.code) } target="_blank" rel="noopener" { (icon("external-link")) "Vincular" }
                        small.tg-hint { "En el chat, pulsa " b { "Iniciar" } }
                    }
                }
                Some(bot) => {
                    div.backup-status.tg-status {
                        (bot_avatar(tg, false))
                        div {
                            b { "Avisos activados" }
                            span { "@" (bot) " · " (tg.at) }
                        }
                        div.tg-actions {
                            form.inline method="post" action="/telegram/test" { (v.ctx()) button.btn.ghost.sm { (icon("send")) "Enviar prueba" } }
                            details.menu.backup-time {
                                summary.icon-btn aria-label="Más opciones de tus avisos" { (icon("ellipsis")) }
                                div.pop {
                                    a.btn.outline.wide href={ "https://t.me/" (bot) } target="_blank" rel="noopener" { (icon("external-link")) "Abrir el chat" }
                                    form method="post" action="/telegram/unlink" { (v.ctx()) button.btn.danger.wide { (icon("x")) "Dejar de recibir" } }
                                }
                            }
                        }
                    }
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
        div.card #backups {
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
                                p.hint { "Mejor de noche, cuando nadie usa Nido" }
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
                                        div.field { label { "Copias que guardar" } input.input type="number" name="keep" value=(t.keep) min="1" max="365" required; p.hint { "Al pasar de este número se borra la más antigua" } }
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
                        p.hint { "Una carpeta ya montada dentro de /media o /mnt en la que Nido pueda escribir" }
                        div.pane-foot { (keep) button.btn.primary { (icon("check")) "Añadir" } }
                    }
                }
            }
        }
    }
}
