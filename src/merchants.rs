use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/logos.rs"));
include!(concat!(env!("OUT_DIR"), "/icons.rs"));

pub struct Cat {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
    pub color: &'static str,
}

const fn c(id: &'static str, label: &'static str, icon: &'static str, color: &'static str) -> Cat {
    Cat { id, label, icon, color }
}

pub const CATS: &[Cat] = &[
    c("super", "Supermercado", "shopping-cart", "#16a34a"),
    c("comida", "Restaurantes", "utensils", "#f97316"),
    c("cafe", "Cafés y bares", "coffee", "#a16207"),
    c("delivery", "Comida a domicilio", "bike", "#ef4444"),
    c("ropa", "Ropa", "shirt", "#db2777"),
    c("compras", "Compras", "shopping-bag", "#8b5cf6"),
    c("hogar", "Hogar", "house", "#0ea5e9"),
    c("tech", "Tecnología", "laptop", "#6366f1"),
    c("transporte", "Transporte", "train-front", "#0891b2"),
    c("viajes", "Viajes", "plane", "#06b6d4"),
    c("combustible", "Combustible", "fuel", "#ea580c"),
    c("coche", "Coche", "car", "#64748b"),
    c("subs", "Suscripciones", "tv", "#e11d48"),
    c("telefono", "Teléfono e internet", "smartphone", "#2563eb"),
    c("facturas", "Suministros", "lightbulb", "#eab308"),
    c("salud", "Salud", "pill", "#10b981"),
    c("deporte", "Deporte", "dumbbell", "#65a30d"),
    c("banco", "Banco y seguros", "landmark", "#475569"),
    c("ocio", "Ocio", "clapperboard", "#a855f7"),
    c("mascotas", "Mascotas", "paw-print", "#d97706"),
    c("belleza", "Belleza", "scissors", "#ec4899"),
    c("envios", "Envíos", "package", "#78716c"),
    c("educacion", "Educación", "graduation-cap", "#3b82f6"),
    c("otros", "Otros", "receipt", "#94a3b8"),
];

pub fn cat(id: &str) -> &'static Cat {
    CATS.iter().find(|c| c.id == id).unwrap_or(&CATS[CATS.len() - 1])
}

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

struct Merchant {
    slug: String,
    name: String,
    cat: &'static Cat,
    kws: Vec<String>, // folded; "=…" means the whole description must match
}

fn merchants() -> &'static [Merchant] {
    static M: OnceLock<Vec<Merchant>> = OnceLock::new();
    M.get_or_init(|| {
        include_str!("../assets/merchants.tsv")
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .filter_map(|l| {
                let f: Vec<&str> = l.split('|').collect();
                (f.len() == 5).then(|| Merchant {
                    slug: f[0].into(),
                    name: f[1].into(),
                    cat: cat(f[3]),
                    kws: f[4]
                        .split(',')
                        .map(|k| match k.trim().strip_prefix('=') {
                            Some(e) => format!("={}", fold(e)),
                            None => fold(k),
                        })
                        .filter(|k| k.trim_matches([' ', '=']) != "")
                        .collect(),
                })
            })
            .collect()
    })
}

pub struct Match {
    pub name: Option<&'static str>,
    pub logo: Option<&'static str>,
    pub cat: &'static Cat,
}

/// Brand matches beat generic keywords; among equals the longest keyword wins ("uber eats" > "uber").
pub fn classify(desc: &str) -> Match {
    let t = fold(desc);
    let mut best: Option<(&Merchant, (bool, usize))> = None;
    for m in merchants() {
        for k in &m.kws {
            let hit = match k.strip_prefix('=') {
                Some(e) => t == e,
                None => t.contains(k.as_str()),
            };
            let score = (!m.slug.is_empty(), k.len());
            if hit && best.map_or(true, |(_, s)| score > s) {
                best = Some((m, score));
            }
        }
    }
    match best {
        Some((m, _)) => Match {
            name: (!m.name.is_empty()).then_some(m.name.as_str()),
            logo: logo(&m.slug).map(|_| m.slug.as_str()),
            cat: m.cat,
        },
        None => Match { name: None, logo: None, cat: cat("otros") },
    }
}

pub fn logo(slug: &str) -> Option<&'static [u8]> {
    LOGOS.binary_search_by(|(s, _)| (*s).cmp(slug)).ok().map(|i| LOGOS[i].1)
}

pub fn mime(b: &[u8]) -> &'static str {
    match b {
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp",
        [0x89, b'P', ..] => "image/png",
        [0xFF, 0xD8, ..] => "image/jpeg",
        [0, 0, 1, 0, ..] => "image/x-icon",
        [b'G', b'I', b'F', ..] => "image/gif",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_spanish_merchants() {
        let m = classify("MERCADONA S.A. VALENCIA 1234");
        assert_eq!((m.name, m.logo, m.cat.id), (Some("Mercadona"), Some("mercadona"), "super"));
        assert_eq!(classify("Uber Eats").name, Some("Uber Eats"));
        assert_eq!(classify("UBER *TRIP").name, Some("Uber"));
        assert_eq!(classify("Tarjeta BBVA").name, Some("BBVA"));
        assert_eq!(classify("Comida del día").cat.id, "comida");
        assert_eq!(classify("DIA").name, Some("Dia"));
        assert_eq!(classify("CARREFOUR EXPRESS MADRID").logo, Some("carrefour"));
        assert_eq!(classify("TELEPIZZA S.A.U.").logo, Some("telepizza"));
        assert_eq!(classify("RUNPOD IO").name, Some("RunPod"));
        assert_eq!(classify("CLOUDFLARE, INC.").logo, Some("cloudflare"));
        assert_eq!(classify("OVH SAS").name, Some("OVHcloud"));
        assert_eq!(classify("DeepSeek API").logo, Some("deepseek"));
        assert_eq!(classify("PIXELLAB.AI").name, Some("PixelLab"));
        assert_eq!(classify("Sulake Oy Habbo").name, Some("Habbo"));
        assert_eq!(classify("TIKTOK SHOP ES").name, Some("TikTok Shop"));
        assert_eq!(classify("AUTOSERVICIOS FAMILIA BEMBIBRE").name, Some("Familia"));
        assert_eq!(classify("Familia").cat.id, "super");
        assert_ne!(classify("Ayuda para la familia").name, Some("Familia"));
        assert_eq!(classify("Café").cat.id, "cafe");
        assert_eq!(classify("Cafe\u{301} con leche").cat.id, "cafe");
        assert_eq!(classify("Internet y móvil").cat.id, "telefono");
        assert_eq!(classify("H&M Gran Vía").name, Some("H&M"));
        assert_eq!(classify("asdfgh").cat.id, "otros");
        assert_eq!(classify("REPSOL E.S. MADRID").cat.id, "combustible");
        assert_eq!(classify("Gasolinera km 12").cat.id, "combustible");
        assert_eq!(classify("Parking Plaza Mayor").cat.id, "coche");
        assert!(LOGOS.windows(2).all(|w| w[0].0 < w[1].0), "LOGOS must be sorted");
        for c in CATS {
            assert!(ICONS.iter().any(|(n, _)| *n == c.icon), "missing icon {}", c.icon);
        }
        for (slug, bytes) in LOGOS {
            assert_ne!(mime(bytes), "application/octet-stream", "{slug} is not an image");
        }
    }
}
