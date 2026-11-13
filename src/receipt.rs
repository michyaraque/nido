use crate::merchants::{classify, fold};
use crate::money::parse_cents;

#[derive(Debug, Default, PartialEq)]
pub struct Receipt {
    pub merchant: Option<String>,
    pub date: Option<String>,
    pub total: Option<i64>,
    pub items: Vec<(String, i64)>,
}

const NOT_ITEMS: &[&str] = &[
    "total", "subtotal", "iva", "base", "cuota", "tarjeta", "efectivo", "entregado", "cambio", "visa",
    "mastercard", "importe", "descuento", "ahorro", "pagar", "pago", "debito", "credito", "contactless",
    "nif", "cif", "tel", "factura", "ticket", "operacion", "aut", "terminal", "comercio", "fecha", "hora",
    "tpv", "bancario", "euros", "efect", "medios", "neto", "bruto", "codigo", "surt", "dto", "alb",
];

fn item_name(raw: &str) -> Option<String> {
    let mut words: Vec<&str> = raw.split_whitespace().collect();
    while let Some(w) = words.first() {
        let qty = w.chars().all(|c| !c.is_alphabetic())
            || w.eq_ignore_ascii_case("x")
            || (w.chars().count() <= 3 && w.chars().any(|c| c.is_ascii_digit()));
        if qty && !w.starts_with('+') {
            words.remove(0);
        } else {
            break;
        }
    }
    let shouting = |w: &str| w.chars().filter(|c| c.is_alphabetic()).count() >= 3 && !w.chars().any(char::is_lowercase);
    let upper = words.iter().any(|w| shouting(w));
    while let Some(w) = words.last() {
        let code = w.chars().all(|c| c.is_ascii_digit()) || matches!(*w, "E" | "€");
        let noise = upper && w.chars().any(char::is_lowercase);
        if (code || noise) && words.len() > 1 {
            words.pop();
        } else {
            break;
        }
    }
    let name = words.join(" ").trim_start_matches(['*', '#', '.', '-']).trim().to_string();
    let starts_ok = name.starts_with(|c: char| c.is_alphabetic() || c == '+');
    let letters = name.chars().filter(|c| c.is_alphabetic()).count();
    (starts_ok && letters >= 3).then_some(name)
}

fn amounts(line: &str) -> Vec<i64> {
    line.split_whitespace()
        .filter_map(|tok| {
            let t: String = tok.chars().filter(|c| c.is_ascii_digit() || matches!(c, ',' | '.' | '-')).collect();
            let t = t.trim_end_matches(['.', ',']);
            let seps: Vec<char> = t.chars().filter(|c| matches!(c, ',' | '.')).collect();
            let decimal_ok = t.len() >= 4
                && matches!(t.as_bytes()[t.len() - 3], b',' | b'.')
                && t[t.len() - 2..].bytes().all(|b| b.is_ascii_digit());
            // "12.11.26" is a date; "1.234,56" is fine (different separators).
            let repeated = seps.len() > 1 && seps.windows(2).any(|w| w[0] == w[1]);
            (decimal_ok && !repeated && seps.len() <= 2).then(|| parse_cents(t)).flatten()
        })
        .collect()
}

fn date(line: &str) -> Option<String> {
    let b = line.as_bytes();
    for i in 0..b.len() {
        let nums: Vec<(usize, usize)> = {
            let mut groups = Vec::new();
            let mut j = i;
            while groups.len() < 3 && j < b.len() {
                let start = j;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
                if j == start {
                    break;
                }
                groups.push((start, j));
                if groups.len() < 3 && j < b.len() && matches!(b[j], b'/' | b'-' | b'.') {
                    j += 1;
                } else {
                    break;
                }
            }
            groups
        };
        if nums.len() == 3 && (i == 0 || !b[i - 1].is_ascii_digit()) {
            let n = |k: usize| line[nums[k].0..nums[k].1].parse::<u32>().ok();
            let (d, m, y) = (n(0)?, n(1)?, n(2)?);
            let y = match nums[2].1 - nums[2].0 {
                2 => 2000 + y,
                4 => y,
                _ => continue,
            };
            if (1..=31).contains(&d) && (1..=12).contains(&m) && (2000..=2099).contains(&y) {
                return Some(format!("{y:04}-{m:02}-{d:02}"));
            }
        }
    }
    None
}

pub fn parse(lines: &[String]) -> Receipt {
    let folded: Vec<String> = lines.iter().map(|l| fold(l)).collect();
    let has = |i: usize, w: &str| folded[i].contains(&format!(" {w} "));

    let merchant = lines
        .iter()
        .take(6)
        .find_map(|l| classify(l).name.map(str::to_string))
        .or_else(|| {
            lines.iter().take(4).find(|l| {
                let letters = l.chars().filter(|c| c.is_alphabetic()).count();
                let digits = l.chars().filter(|c| c.is_ascii_digit()).count();
                let f = fold(l);
                letters >= 3
                    && digits * 3 <= letters
                    && !l.starts_with(|c: char| c.is_ascii_digit())
                    && !l.contains([':', '@'])
                    && !["factura", "simplificada", "ticket", "nif", "cif", "tel", "telefono"].iter().any(|w| f.contains(&format!(" {w} ")))
            }).map(|l| l.trim().to_string())
        });

    let date = lines.iter().find_map(|l| date(l));

    let mut total = None;
    for i in 0..lines.len() {
        if has(i, "total") && !has(i, "subtotal") && !has(i, "iva") && !has(i, "articulos") {
            let mut found = amounts(&lines[i]);
            if found.is_empty() && i + 1 < lines.len() {
                found = amounts(&lines[i + 1]);
            }
            if let Some(&max) = found.iter().max() {
                total = Some(total.map_or(max, |t: i64| t.max(max)));
            }
        }
    }
    let total = total.or_else(|| lines.iter().flat_map(|l| amounts(l)).filter(|a| *a > 0).max());

    let items = lines
        .iter()
        .enumerate()
        .filter(|(i, _)| !NOT_ITEMS.iter().any(|w| has(*i, w)))
        .filter_map(|(_, l)| {
            let price = *amounts(l).last().filter(|p| **p > 0)?;
            let before: Vec<&str> = l.split_whitespace().take_while(|t| amounts(t).is_empty()).collect();
            Some((item_name(&before.join(" "))?, price))
        })
        .take(60)
        .collect::<Vec<(String, i64)>>();

    let shouting = |n: &str| n.split_whitespace().any(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 3 && !w.chars().any(char::is_lowercase));
    let caps = items.iter().filter(|(n, _)| shouting(n)).count();
    let items = if caps * 10 >= items.len() * 7 { items.into_iter().filter(|(n, _)| shouting(n)).collect() } else { items };

    Receipt { merchant, date, total, items }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(str::to_string).collect()
    }

    #[test]
    fn amounts_and_dates() {
        assert_eq!(amounts("LECHE ENTERA 1L 0,95"), vec![95]);
        assert_eq!(amounts("TOTAL (€) 23,45€"), vec![2345]);
        assert_eq!(amounts("2 x 1,20 2,40"), vec![120, 240]);
        assert_eq!(amounts("12.11.26 10:32"), Vec::<i64>::new());
        assert_eq!(amounts("TOTAL 1.234,56"), vec![123456]);
        assert_eq!(amounts("NIF A-46103834"), Vec::<i64>::new());
        assert_eq!(date("12/11/2026 18:04 OP: 123"), Some("2026-11-12".into()));
        assert_eq!(date("FECHA 03-01-26"), Some("2026-01-03".into()));
        assert_eq!(date("TEL 963 883 333"), None);
    }

    #[test]
    fn spanish_supermarket_receipt() {
        let r = parse(&lines(
            "MERCADONA, S.A.\nC/ COLON 25 VALENCIA\nNIF A-46103834\n12/11/2026 18:04 OP: 151\n\
             1 LECHE ENTERA 0,95\n2 x TOMATE PERA 1,20 2,40\nPAN DE MOLDE 1,35\n\
             TOTAL (€) 4,70\nTARJETA BANCARIA 4,70\nIVA 10% BASE 4,27 CUOTA 0,43",
        ));
        assert_eq!(r.merchant.as_deref(), Some("Mercadona"));
        assert_eq!(r.date.as_deref(), Some("2026-11-12"));
        assert_eq!(r.total, Some(470));
        assert_eq!(r.items, vec![("LECHE ENTERA".into(), 95), ("TOMATE PERA".into(), 240), ("PAN DE MOLDE".into(), 135)]);
    }

    #[test]
    fn total_on_next_line_and_unknown_shop() {
        let r = parse(&lines("Fruteria Lola\nKIWI 2,10\nTOTAL A PAGAR\n2,10 €"));
        assert_eq!(r.merchant.as_deref(), Some("Fruteria Lola"));
        assert_eq!(r.total, Some(210));
    }

    #[test]
    fn noisy_item_names() {
        assert_eq!(item_name("1 JAMONCITOS DE POLLO Wnlldio").as_deref(), Some("JAMONCITOS DE POLLO"));
        assert_eq!(item_name("*Gasóleo A 3 E").as_deref(), Some("Gasóleo A"));
        assert_eq!(item_name("0. LONCHAS CREMOSO ioib").as_deref(), Some("LONCHAS CREMOSO"));
        assert_eq!(item_name("+PROT. FRESA-PLATANO").as_deref(), Some("+PROT. FRESA-PLATANO"));
        assert_eq!(item_name("(SURT 3;"), None);
        assert_eq!(item_name("(Fami) x"), None);
        assert_eq!(item_name("1E BOLSA PLASTICO").as_deref(), Some("BOLSA PLASTICO"));
        assert_eq!(parse(&lines("28001 MADRID Madrid)\nTelf:910000000\nFACTURA SIMPLIFICADA")).merchant, None);
    }
}
