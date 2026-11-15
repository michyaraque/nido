//! The pieces of Nido's look this plugin uses (same markup and classes as Nido's own helpers).
use maud::{html, Markup, PreEscaped};

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

pub fn icon(name: &str) -> Markup {
    let inner = ICONS.iter().find(|(n, _)| *n == name).map(|(_, s)| *s).unwrap_or("");
    html! {
        svg.i viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
            stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" { (PreEscaped(inner)) }
    }
}

pub fn pill(tone: &str, ic: Option<&str>, text: &str) -> Markup {
    html! { span class={ "pill " (tone) } { @if let Some(i) = ic { (icon(i)) } (text) } }
}

pub fn empty(ic: &str, title: &str, body: &str) -> Markup {
    html! { div.empty { span.empty-ico { (icon(ic)) } h3 { (title) } p { (body) } } }
}

/// A person's face: their photo if they have one, else a letter in their colour (Nido's `.avatar`).
pub fn face(name: &str, color: &str, photo: Option<&str>) -> Markup {
    match photo {
        Some(src) => html! { img.avatar src=(src) alt="" title=(name) width="30" height="30" loading="lazy" decoding="async"; },
        None => avatar(name, color),
    }
}

/// Letter avatar in a person's colour (Nido's `.avatar` look).
pub fn avatar(name: &str, color: &str) -> Markup {
    let letter: String = name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into());
    html! { span.avatar style={ "background:" (color) } title=(name) { (letter) } }
}

pub fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => match std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(v) => {
                    out.push(v);
                    i += 2;
                }
                None => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
