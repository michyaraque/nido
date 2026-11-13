use super::*;

pub fn page(title: &str, body_class: &str, body: Markup) -> Response {
    Html(
        html! {
            (DOCTYPE)
            html lang="es" {
                head {
                    meta charset="utf-8";
                    meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover";
                    meta name="color-scheme" content="light dark";
                    meta name="theme-color" content="#f7f7f8" media="(prefers-color-scheme: light)";
                    meta name="theme-color" content="#0c0c10" media="(prefers-color-scheme: dark)";
                    meta name="apple-mobile-web-app-capable" content="yes";
                    meta name="mobile-web-app-capable" content="yes";
                    meta name="apple-mobile-web-app-title" content="Nido";
                    link rel="manifest" href="/manifest.webmanifest";
                    link rel="apple-touch-icon" sizes="180x180" href={ "/apple-touch-icon.png?v=" (assets_version()) };
                    link rel="icon" href="/icon.png";
                    link rel="preload" href="/fonts/inter.woff2" as="font" type="font/woff2" crossorigin;
                    title { (title) }
                    link rel="stylesheet" href={ "/style.css?v=" (assets_version()) };
                    script src={ "/app.js?v=" (assets_version()) } defer {}
                }
                body class=(body_class) { (body) }
            }
        }
        .into_string(),
    )
    .into_response()
}

/// Content hash of CSS and JS: their URLs change whenever they do, so the year-long cache never serves a stale file.
pub fn assets_version() -> &'static str {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let h = CSS.bytes().chain(JS.bytes()).fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
        format!("{h:x}")
    })
}

pub fn initial(name: &str) -> String {
    name.chars().find(|c| c.is_alphanumeric()).unwrap_or('?').to_uppercase().to_string()
}

/// House themes: (id, name, description, preview [side, surface, accent]). First is the default.
pub const THEMES: [(&str, &str, &str, [&str; 3]); 5] = [
    ("zinc", "Neutro", "Grises limpios, el de siempre", ["#f4f4f5", "#ffffff", "#09090b"]),
    ("warm", "Cálido", "Piedra y beige, como papel", ["#efece6", "#ffffff", "#1c1a17"]),
    ("ocean", "Océano", "Azul pizarra, tranquilo", ["#eef2f7", "#ffffff", "#0369a1"]),
    ("forest", "Bosque", "Verde salvia, natural", ["#edf2ee", "#ffffff", "#15803d"]),
    ("rose", "Rosa", "Suave, con un toque de color", ["#f5eef1", "#ffffff", "#be185d"]),
];

/// How shared bills are divided: (id, name, description). First is the default.
pub const SPLITS: [(&str, &str, &str); 4] = [
    ("salary", "Ingreso fijo", "Según el ingreso fijo de cada uno."),
    ("avg3", "Lo cobrado", "Media de los últimos 3 meses. Para quien cobra distinto cada mes."),
    ("equal", "A partes iguales", "Todos pagan lo mismo."),
    ("custom", "Porcentajes fijos", "Lo que hayáis pactado, como 60 / 40."),
];

pub fn split_id(s: &str) -> &'static str {
    SPLITS.iter().find(|t| t.0 == s).map_or(SPLITS[0].0, |t| t.0)
}

pub fn theme_id(s: &str) -> &'static str {
    THEMES.iter().find(|t| t.0 == s).map_or(THEMES[0].0, |t| t.0)
}

pub const PALETTE: [&str; 8] = ["#2f6bff", "#e1166b", "#7c3aed", "#d97706", "#0891b2", "#17803d", "#dc2626", "#52525b"];

pub fn hue(s: &str) -> &'static str {
    PALETTE[s.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32)) as usize % PALETTE.len()]
}

pub fn icon(name: &str) -> Markup {
    let inner = merchants::ICONS.iter().find(|(n, _)| *n == name).map(|(_, s)| *s).unwrap_or("");
    html! {
        svg.i.spinning[name == "loader-circle"] viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
            stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" { (PreEscaped(inner)) }
    }
}

pub struct Look {
    pub brand: Option<&'static str>,
    pub logo: Option<&'static str>,
    pub cat: &'static Cat,
}

pub fn look(desc: &str, cat_override: Option<&str>) -> Look {
    let m = merchants::classify(desc);
    Look { brand: m.name, logo: m.logo, cat: cat_override.map(merchants::cat).unwrap_or(m.cat) }
}

pub fn badge(l: &Look, label: &str) -> Markup {
    html! {
        @if let Some(s) = l.logo {
            span.tile.logo { img src={ "/logos/" (s) } alt="" width="40" height="40" loading="lazy" decoding="async"; }
        } @else if l.cat.id != "otros" {
            span.tile style={ "--c:" (l.cat.color) } { (icon(l.cat.icon)) }
        } @else {
            span.tile.letter style={ "--c:" (hue(label)) } aria-hidden="true" { (initial(label)) }
        }
    }
}

pub fn pill(tone: &str, ic: Option<&str>, text: &str) -> Markup {
    html! { span class={ "pill " (tone) } { @if let Some(i) = ic { (icon(i)) } (text) } }
}

pub fn empty(ic: &str, title: &str, body: &str, action: Option<(&str, &str)>) -> Markup {
    html! {
        div.empty {
            span.empty-ico { (icon(ic)) }
            h3 { (title) }
            p { (body) }
            @if let Some((href, label)) = action { a.btn.outline.sm href=(href) { (icon("plus")) (label) } }
        }
    }
}

pub fn stat(ic: &str, label: &str, value: Markup, foot: Markup) -> Markup {
    html! {
        div.card.stat {
            div.stat-head { span.stat-ico { (icon(ic)) } span { (label) } }
            (value)
            div.stat-foot { (foot) }
        }
    }
}

/// Framed card: mono label strip on top, white body, mono footnote + optional link below.
pub fn panel(ic: &str, label: Markup, action: Markup, body: Markup, note: Markup, link: Option<(&str, &str)>) -> Markup {
    html! {
        section.panel {
            div.panel-top { span.panel-label { (icon(ic)) (label) } (action) }
            div.panel-body { (body) }
            div.panel-foot {
                span.panel-note { (note) }
                @if let Some((href, text)) = link { a.panel-link href=(href) { (text) (icon("arrow-right")) } }
            }
        }
    }
}

pub fn brand() -> Markup {
    html! { span.mark { (icon("house")) } span.brand-txt { b { "Nido" } small { "Lo de casa, en orden" } } }
}

pub fn money(c: i64) -> Markup {
    let (a, b) = eur_parts(c);
    html! { span.money { (a) small { (b) " €" } } }
}
