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
