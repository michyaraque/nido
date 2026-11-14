//! The few pieces of Nido's look this extra needs, so its page reads like the rest of Nido.
//! They mirror Nido's own helpers (src/ui.rs, src/products.rs, src/merchants.rs): same markup,
//! same CSS classes, styled by Nido's stylesheet that is already on the page.
use maud::{html, Markup, PreEscaped};

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

pub fn icon(name: &str) -> Markup {
    let inner = ICONS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, s)| *s)
        .unwrap_or("");
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

/// Lower case, no accents, only letters and digits between single spaces, padded: " cafe con lena ".
pub fn fold(s: &str) -> String {
    let mut out = String::from(" ");
    for ch in s.chars().flat_map(char::to_lowercase) {
        let c = match ch {
            'á' | 'à' | 'â' | 'ä' | 'ã' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            '\u{300}'..='\u{36f}' => continue,
            c if c.is_alphanumeric() => c,
            _ => ' ',
        };
        if c == ' ' && out.ends_with(' ') {
            continue;
        }
        out.push(c);
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    out
}

pub fn clean(s: &str) -> String {
    s.trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(100)
        .collect()
}

pub fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                match std::str::from_utf8(&b[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                {
                    Some(v) => {
                        out.push(v);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Icon and colour for a product, like Nido's shopping list (milk, eggs, bread…).
pub mod products {
    use super::fold;

    const GROUPS: &[(&str, &str, &str)] = &[
        ("milk", "#3b82f6", "leche yogur yogurt queso mantequilla nata kefir batido griego cuajada"),
        ("egg", "#d97706", "huevo"),
        ("croissant", "#b45309", "pan baguette barra croissant bolleria magdalena tostada bizcocho"),
        ("cookie", "#92400e", "galleta chocolate cacao cereal muesli"),
        ("citrus", "#f59e0b", "naranja limon mandarina pomelo lima"),
        ("banana", "#eab308", "platano banana"),
        ("apple", "#16a34a", "manzana pera fruta melocoton kiwi uva fresa sandia melon piña aguacate"),
        ("carrot", "#ea580c", "zanahoria tomate patata cebolla ajo pimiento calabacin berenjena verdura brocoli pepino"),
        ("salad", "#22c55e", "lechuga ensalada espinaca cogollo rucula canonigo acelga"),
        ("beef", "#dc2626", "carne ternera cerdo filete hamburguesa secreto lomo costilla picada solomillo"),
        ("drumstick", "#ea580c", "pollo pavo jamoncito alita muslo pechuga"),
        ("ham", "#e11d48", "jamon chorizo salchichon fuet embutido bacon loncha salchicha mortadela"),
        ("fish", "#0891b2", "pescado atun salmon merluza bacalao gamba langostino mejillon sardina calamar"),
        ("wheat", "#a16207", "arroz pasta macarron espagueti harina avena legumbre lenteja garbanzo alubia quinoa"),
        ("coffee", "#78350f", "cafe te infusion capsula"),
        ("wine", "#9f1239", "vino cava sidra"),
        ("beer", "#ca8a04", "cerveza"),
        ("cup-soda", "#e11d48", "refresco cola zumo tonica"),
        ("droplets", "#0ea5e9", "agua aceite vinagre"),
    ];

    pub fn look(name: &str) -> (&'static str, &'static str) {
        let folded = fold(name);
        let same = |w: &str, kw: &str| {
            w == kw || w.strip_suffix('s') == Some(kw) || w.strip_suffix("es") == Some(kw)
        };
        folded
            .split_whitespace()
            .find_map(|w| {
                GROUPS
                    .iter()
                    .find(|(_, _, kws)| kws.split_whitespace().any(|kw| same(w, fold(kw).trim())))
            })
            .map(|(icon, color, _)| (*icon, *color))
            .unwrap_or(("shopping-basket", "#64748b"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(fold("Limón, ¡ñam!"), " limon nam ");
        assert_eq!(pct_decode("Ana%20Mar%C3%ADa"), "Ana María");
        assert_eq!(pct_decode("100%"), "100%");
        assert_eq!(products::look("Huevos L").0, "egg");
    }
}
