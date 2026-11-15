//! Recetas: the house's recipe book. Ingredients that scale with the servings, steps to tick off
//! while cooking, and one tap to send what is missing to Compra. A catalog of Spanish recipes is
//! built in (`catalog/*.txt`); TheMealDB (https://www.themealdb.com) is only reached when someone
//! imports from it. Recipes are written in a block editor (`editor.js`, loaded only here).
use super::*;
use axum::{
    extract::{DefaultBodyLimit, Multipart, Path},
    routing::post,
    Form as F,
};
use image::{imageops::FilterType, DynamicImage, ImageDecoder, ImageReader};
use serde_json::Value;

/// Toasts after a redirect (`ok=<code>`); Nido reads them from `/manifest`.
pub const FLASHES: &[(&str, &str)] = &[
    ("recipe_added", "Receta guardada"),
    ("recipe_saved", "Receta actualizada"),
    ("recipe_deleted", "Receta borrada"),
    ("recipe_imported", "Receta importada"),
    ("recipe_exists", "Ya la teníais: aquí está"),
    ("recipe_restored", "Recetas del recetario recuperadas"),
    ("recipe_cooked", "¡Que aproveche! Apuntado"),
    ("recipe_shopped", "Añadido a la lista de la compra"),
    ("recipe_nothing_new", "Todo eso ya estaba en la lista"),
    ("recipe_photos", "Descargando las fotos del recetario"),
    ("recipe_photos_removed", "Fotos del recetario quitadas"),
];

const CSS: &str = include_str!(concat!(env!("OUT_DIR"), "/style.css"));
const JS: &str = include_str!("../editor.js");

/// (name, icon, colour). The last one is the fallback.
const CATS: &[(&str, &str, &str)] = &[
    ("Desayunos", "croissant", "#b45309"),
    ("Entrantes", "salad", "#16a34a"),
    ("Sopas y cremas", "soup", "#0d9488"),
    ("Legumbres", "wheat", "#a16207"),
    ("Pasta y arroz", "utensils", "#ca8a04"),
    ("Verduras", "carrot", "#65a30d"),
    ("Huevos", "egg", "#d97706"),
    ("Carnes", "beef", "#dc2626"),
    ("Aves", "drumstick", "#ea580c"),
    ("Pescados", "fish", "#0891b2"),
    ("Postres", "cookie", "#db2777"),
    ("Bebidas", "coffee", "#78350f"),
    ("Salsas", "droplets", "#b91c1c"),
    ("Otros", "chef-hat", "#64748b"),
];

fn cat(name: &str) -> &'static (&'static str, &'static str, &'static str) {
    CATS.iter()
        .find(|c| c.0 == name)
        .unwrap_or(&CATS[CATS.len() - 1])
}

pub fn routes() -> Router<Db> {
    let big = || DefaultBodyLimit::max(25 * 1024 * 1024);
    Router::new()
        .route("/new", post(create).layer(big()))
        .route("/restore", post(restore))
        .route("/import", post(import))
        .route("/photos", post(photos))
        .route("/assets/editor.js", get(script))
        .route("/{id}", post(update).layer(big()))
        .route("/{id}/photo", get(photo))
        .route("/{id}/favorite", post(favorite))
        .route("/{id}/shop", post(shop))
        .route("/{id}/cooked", post(cooked))
        .route("/{id}/delete", post(delete))
}

/// Long cache: the URL carries a hash of the script, so a new version is a new URL.
async fn script() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "private, max-age=31536000"),
        ],
        JS,
    )
        .into_response()
}

fn js_url() -> String {
    let h = JS.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    });
    format!("/plugins/recipes/assets/editor.js?v={h:x}")
}

// ---------- data ----------

struct Recipe {
    id: i64,
    name: String,
    category: String,
    area: String,
    serves: i64,
    minutes: Option<i64>,
    ingredients: String,
    steps: String,
    note: String,
    source: String,
    photo: i64,
    favorite: bool,
    cooked: i64,
}

/// Columns of a recipe without its lines: the list never needs ingredients or steps.
const COLS: &str = "id,name,category,area,serves,minutes,note,source,photo,favorite,cooked";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Recipe> {
    Ok(Recipe {
        id: r.get(0)?,
        name: r.get(1)?,
        category: r.get(2)?,
        area: r.get(3)?,
        serves: r.get(4)?,
        minutes: r.get(5)?,
        ingredients: String::new(),
        steps: String::new(),
        note: r.get(6)?,
        source: r.get(7)?,
        photo: r.get(8)?,
        favorite: r.get(9)?,
        cooked: r.get(10)?,
    })
}

fn all(c: &Connection) -> rusqlite::Result<Vec<Recipe>> {
    c.prepare(&format!(
        "SELECT {COLS} FROM recipes ORDER BY favorite DESC, cooked DESC, name COLLATE NOCASE"
    ))?
    .query_map([], row)?
    .collect()
}

/// One recipe with its lines, written back as text ("## " sections, "> " tips) for the page and the editor.
fn one(c: &Connection, id: i64) -> rusqlite::Result<Option<Recipe>> {
    let Some(mut r) = c
        .query_row(
            &format!("SELECT {COLS} FROM recipes WHERE id=?1"),
            [id],
            row,
        )
        .optional()?
    else {
        return Ok(None);
    };
    let lines = |sql: &str| -> rusqlite::Result<String> {
        Ok(c.prepare_cached(sql)?
            .query_map([id], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
            .join("\n"))
    };
    r.ingredients = lines("SELECT CASE WHEN section THEN '## ' ELSE '' END || text FROM recipe_ingredients WHERE recipe_id=?1 ORDER BY pos")?;
    r.steps = lines("SELECT CASE kind WHEN 'h' THEN '## ' WHEN 'tip' THEN '> ' ELSE '' END || text FROM recipe_steps WHERE recipe_id=?1 ORDER BY pos")?;
    Ok(Some(r))
}

/// What search looks at for each recipe: name, origin, category and every ingredient, folded.
/// The page carries it on each card (`data-k`), so the browser filters as you type.
// One GROUP BY over all lines; thousands of recipes still take a millisecond or two.
fn search_keys(c: &Connection) -> rusqlite::Result<std::collections::HashMap<i64, String>> {
    c.prepare_cached(
        "SELECT r.id, trim(r.search || ' ' || COALESCE(group_concat(i.item, ' '), ''))
         FROM recipes r LEFT JOIN recipe_ingredients i ON i.recipe_id=r.id AND i.section=0 GROUP BY r.id",
    )?
    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
    .collect()
}

/// Every word of `q` appears somewhere in `keys` ("huevo patata" finds the tortilla). Same rule as editor.js.
fn matches(keys: &str, q: &str) -> bool {
    fold(q).split_whitespace().all(|w| keys.contains(w))
}

/// What search looks at besides the ingredients.
fn search_key(name: &str, area: &str, category: &str) -> String {
    fold(&format!("{name} {area} {category}"))
}

/// Replaces a recipe's lines. `item` is the product, folded, for search and for Compra.
fn save_lines(c: &Connection, id: i64, ingredients: &str, steps: &str) -> rusqlite::Result<()> {
    c.execute("DELETE FROM recipe_ingredients WHERE recipe_id=?1", [id])?;
    c.execute("DELETE FROM recipe_steps WHERE recipe_id=?1", [id])?;
    let mut add = c.prepare_cached(
        "INSERT INTO recipe_ingredients(recipe_id,pos,section,text,item) VALUES(?1,?2,?3,?4,?5)",
    )?;
    for (pos, l) in lines_of(ingredients).enumerate() {
        let (section, text) = match l {
            Line::Section(t) => (true, t),
            Line::Plain(t) | Line::Tip(t) => (false, t),
        };
        let item = if section {
            String::new()
        } else {
            fold(&shop_name(&parse_line(text).name)).trim().to_string()
        };
        add.execute(params![id, pos as i64, section, text, item])?;
    }
    let mut add =
        c.prepare_cached("INSERT INTO recipe_steps(recipe_id,pos,kind,text) VALUES(?1,?2,?3,?4)")?;
    for (pos, l) in lines_of(steps).enumerate() {
        let (kind, text) = match l {
            Line::Section(t) => ("h", t),
            Line::Tip(t) => ("tip", t),
            Line::Plain(t) => ("", t),
        };
        add.execute(params![id, pos as i64, kind, text])?;
    }
    Ok(())
}

/// Opens the recetario: its tables, the recipes an older Nido kept in its own database (once),
/// and the catalog recipes it does not have yet.
pub fn prepare(c: &Connection, core_db: Option<&str>) -> rusqlite::Result<()> {
    c.execute_batch(include_str!("../schema.sql"))?;
    if let Some(core) = core_db {
        import_from_core(c, core)?;
    }
    seed_catalog(c)
}

/// Before plugins were programs of their own, Recetas lived inside nido.db. Its recipes move here
/// the first time, if this database is still empty.
fn import_from_core(c: &Connection, core: &str) -> rusqlite::Result<()> {
    let empty: bool = c.query_row("SELECT NOT EXISTS (SELECT 1 FROM recipes)", [], |r| {
        r.get(0)
    })?;
    if !empty || !std::path::Path::new(core).exists() {
        return Ok(());
    }
    c.execute("ATTACH DATABASE ?1 AS core", [core])?;
    let has: bool = c.query_row(
        "SELECT count(*)=2 FROM core.sqlite_master WHERE name IN ('recipes','recipe_ingredients')",
        [],
        |r| r.get(0),
    )?;
    let moved = if has {
        c.execute_batch(
            "BEGIN;
             INSERT INTO recipes(id,name,category,area,serves,minutes,note,source,photo,favorite,cooked,added_by,created,search)
               SELECT id,name,category,area,serves,minutes,note,source,photo,favorite,cooked,added_by,created,search FROM core.recipes;
             INSERT INTO recipe_ingredients SELECT recipe_id,pos,section,text,item FROM core.recipe_ingredients;
             INSERT INTO recipe_steps SELECT recipe_id,pos,kind,text FROM core.recipe_steps;
             INSERT INTO recipe_photos SELECT recipe_id,webp FROM core.recipe_photos;
             INSERT OR IGNORE INTO recipe_hidden SELECT source FROM core.recipe_hidden;
             UPDATE recipes SET photo=0 WHERE photo>0 AND id NOT IN (SELECT recipe_id FROM recipe_photos);
             COMMIT;",
        )
    } else {
        Ok(())
    };
    c.execute("DETACH DATABASE core", [])?;
    moved
}

// ---------- built-in photos ----------

/// `photo` value of a recipe showing its catalog photo (uploaded ones use a timestamp).
const BUILTIN_PHOTO: i64 = 1;
const CREDITS: &str = include_str!("../photos/CREDITS.tsv");

/// (author, licence, file page) of a built-in photo.
fn credit(source: &str) -> Option<(&'static str, &'static str, &'static str)> {
    let slug = source.strip_prefix("nido:")?;
    CREDITS
        .lines()
        .find_map(|l| match l.split('\t').collect::<Vec<_>>()[..] {
            [s, author, license, page] if s == slug => Some((author, license, page)),
            _ => None,
        })
}

/// One ingredient line: "200 g de harina" → amount 200, unit "g", name "harina".
#[derive(Debug, PartialEq)]
struct Ing {
    amount: Option<f64>, // None when there is no number to scale ("Sal", "2-3")
    qty: String,         // the quantity as written, used when it cannot be scaled
    unit: String,
    name: String,
}

const UNITS: &str = "g gr grs gramo gramos kg kilo kilos mg ml cl dl l litro litros cda cdas cucharada cucharadas \
    cdta cdtas cucharadita cucharaditas taza tazas vaso vasos pizca pizcas diente dientes lata latas sobre sobres \
    puñado puñados rebanada rebanadas loncha lonchas rodaja rodajas hoja hojas rama ramas ramita ramitas cabeza \
    cabezas lomo lomos manojo manojos chorrito chorro bote botes tarrina paquete paquetes \
    cup cups tbsp tbs tbls tblsp tsp oz lb lbs pinch clove cloves can cans slice slices handful";

fn is_unit(w: &str) -> bool {
    let w = w.trim_end_matches('.').to_lowercase();
    UNITS.split_whitespace().any(|u| u == w)
}

fn number(s: &str) -> Option<f64> {
    let frac = |c| match c {
        '½' => Some(0.5),
        '¼' => Some(0.25),
        '¾' => Some(0.75),
        '⅓' => Some(1.0 / 3.0),
        '⅔' => Some(2.0 / 3.0),
        _ => None,
    };
    let last = s.chars().last()?;
    if let Some(f) = frac(last) {
        let head = &s[..s.len() - last.len_utf8()];
        return Some(if head.is_empty() {
            f
        } else {
            head.parse::<f64>().ok()? + f
        });
    }
    if let Some((a, b)) = s.split_once('/') {
        let (a, b) = (a.parse::<f64>().ok()?, b.parse::<f64>().ok()?);
        return (b > 0.0).then(|| a / b);
    }
    s.replace(',', ".")
        .parse()
        .ok()
        .filter(|n: &f64| n.is_finite() && *n > 0.0)
}

fn parse_line(line: &str) -> Ing {
    let line = line.trim();
    let mut ing = Ing {
        amount: None,
        qty: String::new(),
        unit: String::new(),
        name: line.to_string(),
    };
    let split = |s: &str| -> (String, String) {
        let s = s.trim_start();
        let (a, b) = s.split_once(char::is_whitespace).unwrap_or((s, ""));
        (a.to_string(), b.to_string())
    };
    let (tok, mut rest) = split(line);
    let end = tok
        .find(|c: char| !(c.is_ascii_digit() || ".,/½¼¾⅓⅔-".contains(c)))
        .unwrap_or(tok.len());
    if !tok[..end]
        .chars()
        .any(|c| c.is_ascii_digit() || "½¼¾⅓⅔".contains(c))
    {
        return ing;
    }
    ing.qty = tok[..end].to_string();
    ing.amount = number(&ing.qty);
    let glued = &tok[end..]; // "200g"
    if !glued.is_empty() {
        if is_unit(glued) {
            ing.unit = glued.to_string();
        } else {
            rest = format!("{glued} {rest}");
        }
    } else {
        let (w, after) = split(&rest);
        if is_unit(&w) {
            ing.unit = w;
            rest = after;
        }
    }
    let (w, after) = split(&rest);
    if w == "de" || w == "of" {
        rest = after;
    }
    ing.name = rest.trim().to_string();
    if ing.name.is_empty() {
        ing.name = line.to_string();
    }
    ing
}

/// What goes on the shopping list: the product, without how it is cut or "al gusto".
fn shop_name(name: &str) -> String {
    let n = name.split([',', '(']).next().unwrap_or(name).trim();
    let n = n.strip_suffix(" al gusto").unwrap_or(n);
    n.trim().to_string()
}

/// Always at home: unticked by default when sending to Compra.
fn staple(name: &str) -> bool {
    let f = fold(name);
    let first = f.split_whitespace().next().unwrap_or("");
    matches!(
        first,
        "sal" | "pimienta" | "agua" | "salt" | "pepper" | "water"
    )
}

/// 1.5 → "1½", 0.25 → "¼", 133.3 → "133".
fn fmt_amount(x: f64) -> String {
    if x >= 10.0 {
        return format!("{}", x.round() as i64);
    }
    let whole = x.trunc();
    let frac = x - whole;
    let parts: [(f64, &str); 7] = [
        (0.0, ""),
        (0.25, "¼"),
        (1.0 / 3.0, "⅓"),
        (0.5, "½"),
        (2.0 / 3.0, "⅔"),
        (0.75, "¾"),
        (1.0, ""),
    ];
    let (f, s) = parts
        .iter()
        .min_by(|a, b| (a.0 - frac).abs().total_cmp(&(b.0 - frac).abs()))
        .copied()
        .unwrap_or((0.0, ""));
    let whole = whole as i64 + (f == 1.0) as i64;
    match (whole, s) {
        (0, "") => format!("{:.1}", x).replace('.', ","),
        (w, "") => w.to_string(),
        (0, s) => s.to_string(),
        (w, s) => format!("{w}{s}"),
    }
}

impl Ing {
    fn qty_for(&self, factor: f64) -> String {
        let q = match self.amount {
            Some(a) => fmt_amount(a * factor),
            None => self.qty.clone(),
        };
        format!("{q} {}", self.unit).trim().to_string()
    }
}

fn mins(m: i64) -> String {
    match (m / 60, m % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// A line of ingredients or steps: a section title ("## …"), a tip ("> …", steps only) or plain.
enum Line<'a> {
    Section(&'a str),
    Tip(&'a str),
    Plain(&'a str),
}

fn lines_of(text: &str) -> impl Iterator<Item = Line<'_>> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            if let Some(t) = l.strip_prefix("## ") {
                Line::Section(t.trim())
            } else if let Some(t) = l.strip_prefix("> ") {
                Line::Tip(t.trim())
            } else {
                Line::Plain(l)
            }
        })
}

/// Ingredient lines that are things to buy (not section titles).
fn ingredients_of(r: &Recipe) -> impl Iterator<Item = &str> {
    lines_of(&r.ingredients).filter_map(|l| match l {
        Line::Plain(t) | Line::Tip(t) => Some(t),
        Line::Section(_) => None,
    })
}

// ---------- built-in catalog ----------

/// Spanish home cooking, written for Nido. Format: "# Name", "Category | serves | minutes | area",
/// ingredient lines, "--", step lines; a blank line ends a recipe.
const CATALOG: &str = concat!(
    include_str!("../catalog/1-entrantes.txt"),
    "\n\n",
    include_str!("../catalog/2-cuchara.txt"),
    "\n\n",
    include_str!("../catalog/3-principales.txt"),
    "\n\n",
    include_str!("../catalog/4-postres.txt"),
);

struct Entry {
    name: &'static str,
    category: &'static str,
    serves: i64,
    minutes: i64,
    area: &'static str,
    ingredients: String,
    steps: String,
}

impl Entry {
    fn source(&self) -> String {
        format!("nido:{}", fold(self.name).trim().replace(' ', "-"))
    }
}

fn catalog() -> Vec<Entry> {
    let mut out = Vec::new();
    let mut chunk: Vec<&'static str> = Vec::new();
    for line in CATALOG.lines().map(str::trim).chain([""]) {
        if !line.is_empty() {
            chunk.push(line);
            continue;
        }
        if let [head, meta, rest @ ..] = chunk.as_slice() {
            let m: Vec<&'static str> = meta.split('|').map(str::trim).collect();
            if let (Some(name), Some(cut), [cat, serves, minutes, area]) = (
                head.strip_prefix("# "),
                rest.iter().position(|l| *l == "--"),
                m.as_slice(),
            ) {
                out.push(Entry {
                    name,
                    category: cat,
                    serves: serves.parse().unwrap_or(4),
                    minutes: minutes.parse().unwrap_or(0),
                    area,
                    ingredients: rest[..cut].join("\n"),
                    steps: rest[cut + 1..].join("\n"),
                });
            }
        }
        chunk.clear();
    }
    out
}

/// Adds the catalog recipes the house does not have yet, except the ones it deleted.
fn seed_catalog(c: &Connection) -> rusqlite::Result<()> {
    let tx = c.unchecked_transaction()?;
    for e in catalog() {
        let source = e.source();
        let added = tx.execute(
            "INSERT INTO recipes(name,category,area,serves,minutes,source,search)
             SELECT ?1,?2,?3,?4,?5,?6,?7
             WHERE NOT EXISTS (SELECT 1 FROM recipes WHERE source=?6) AND NOT EXISTS (SELECT 1 FROM recipe_hidden WHERE source=?6)",
            params![e.name, e.category, e.area, e.serves, (e.minutes > 0).then_some(e.minutes), source, search_key(e.name, e.area, e.category)],
        )?;
        if added > 0 {
            save_lines(&tx, tx.last_insert_rowid(), &e.ingredients, &e.steps)?;
        }
    }
    tx.commit()
}

// ---------- pages ----------

fn url(v: &View, extra: &str) -> String {
    if extra.is_empty() {
        v.link("recipes")
    } else {
        format!("{}&{extra}", v.link("recipes"))
    }
}

fn qs(pairs: &[(&str, &str)]) -> String {
    serde_urlencoded::to_string(pairs.iter().filter(|p| !p.1.is_empty()).collect::<Vec<_>>())
        .unwrap_or_default()
}

/// Icons by reference. The list repeats a handful of icons over 100+ cards, so each card points at
/// one copy in the page's sprite instead of carrying its own SVG; the stroke style lives in CSS (`.i.u`).
fn ico(name: &str) -> Markup {
    PreEscaped(format!(
        r##"<svg class="i u" aria-hidden="true"><use href="#rc-{name}"/></svg>"##
    ))
}

fn sprite(names: &[&str]) -> Markup {
    let mut s =
        String::from(r#"<svg width="0" height="0" style="position:absolute" aria-hidden="true">"#);
    for n in names {
        let inner = ICONS
            .iter()
            .find(|(k, _)| k == n)
            .map_or("", |(_, svg)| *svg);
        s += &format!(r#"<symbol id="rc-{n}" viewBox="0 0 24 24">{inner}</symbol>"#);
    }
    PreEscaped(s + "</svg>")
}

/// The content area for Nido: our CSS (only travels with this page), the editor script, and the view.
pub fn page(c: &Connection, v: &View, p: &Params) -> rusqlite::Result<Markup> {
    Ok(html! {
        style { (PreEscaped(CSS)) }
        span hidden data-plugin="recipes" data-plugin-js=(js_url()) {}
        (view(c, v, p)?)
    })
}

fn view(c: &Connection, v: &View, p: &Params) -> rusqlite::Result<Markup> {
    if p.contains_key("new") {
        return Ok(editor(v, None));
    }
    if let Some(id) = p.get("r").and_then(|s| s.parse().ok()) {
        if let Some(r) = one(c, id)? {
            if p.contains_key("edit") {
                return Ok(editor(v, Some(&r)));
            }
            let serves = p
                .get("serves")
                .and_then(|s| s.parse().ok())
                .filter(|n| (1..=50).contains(n))
                .unwrap_or(r.serves);
            let have: Option<String> = c.query_row("SELECT items FROM recipe_have WHERE recipe_id=?1", [id], |r| r.get(0)).optional()?;
            return Ok(detail(v, &r, serves, have.as_deref()));
        }
    }
    let q = p.get("q").map(|s| clean(s)).unwrap_or_default();
    let filter = p.get("cat").map(String::as_str).unwrap_or("");
    let hidden: i64 = c.query_row("SELECT count(*) FROM recipe_hidden", [], |r| r.get(0))?;
    let photos = photo_state(c)?;
    Ok(listing(v, &all(c)?, &q, filter, hidden, &search_keys(c)?, &photos))
}

fn cover(r: &Recipe, art: Markup) -> Markup {
    html! {
        @if r.photo > 0 {
            img.rc-cover src={ "/plugins/recipes/" (r.id) "/photo?v=" (r.photo) } alt="" width="800" height="600" loading="lazy" decoding="async";
        } @else {
            div.rc-cover.rc-art aria-hidden="true" { (art) }
        }
    }
}

fn card(v: &View, r: &Recipe, keys: &str, hidden: bool) -> Markup {
    let (_, ic, color) = cat(&r.category);
    html! {
        a.rc-card href=(url(v, &format!("r={}", r.id))) style={ "--c:" (color) } data-k=(keys) hidden[hidden] {
            (cover(r, ico(ic)))
            @if r.favorite { span.rc-fav title="Favorita" { (ico("heart")) } }
            div.rc-card-body {
                span.rc-card-cat { (ico(ic)) (r.category) }
                h3 { (r.name) }
                div.rc-meta {
                    @if let Some(m) = r.minutes { span { (ico("clock")) (mins(m)) } }
                    span { (ico("users")) (r.serves) }
                    @if r.cooked > 0 { span title="Veces que la habéis hecho" { (ico("chef-hat")) "×" (r.cooked) } }
                }
            }
        }
    }
}

fn listing(
    v: &View,
    book: &[Recipe],
    q: &str,
    filter: &str,
    hidden: i64,
    keys: &std::collections::HashMap<i64, String>,
    photos: &PhotoState,
) -> Markup {
    let shown: Vec<&Recipe> = book
        .iter()
        .filter(|r| match filter {
            "" => true,
            "fav" => r.favorite,
            c => r.category == c,
        })
        .collect();
    // Cards that do not match stay on the page, hidden: typing a different word shows them again at once.
    let key = |r: &Recipe| keys.get(&r.id).map_or("", String::as_str);
    let visible = shown.iter().filter(|r| matches(key(r), q)).count();
    let used: Vec<_> = CATS
        .iter()
        .filter(|c| book.iter().any(|r| r.category == c.0))
        .collect();
    let count = |c: &str| book.iter().filter(|r| r.category == c).count();
    let mut icons: Vec<&str> = CATS
        .iter()
        .map(|c| c.1)
        .chain(["clock", "users", "chef-hat", "heart", "book-open"])
        .collect();
    icons.sort_unstable();
    icons.dedup();
    html! {
        (sprite(&icons))
        div.page-head {
            div {
                h1 { "Recetas" }
                p { @if book.is_empty() { "El recetario de casa" } @else { b { (book.len()) } " receta" @if book.len() != 1 { "s" } " en el recetario de casa" } }
            }
            div.head-actions {
                a.btn.outline href="#import-recipe" { (icon("download")) "Importar" }
                a.btn.primary href=(url(v, "new=1")) { (icon("plus")) "Nueva receta" }
            }
        }
        @if book.is_empty() {
            section.card.rc-welcome {
                span.rc-welcome-art { (icon("chef-hat")) }
                h2 { "El recetario está vacío" }
                p { "Escribe vuestras recetas con sus ingredientes y sus pasos. Desde cada una, lo que falte va directo a la lista de la compra." }
                div.rc-welcome-actions {
                    a.btn.primary href=(url(v, "new=1")) { (icon("pencil")) "Escribir una receta" }
                    @if hidden > 0 {
                        form method="post" action="/plugins/recipes/restore" { (v.ctx()) button.btn.outline { (icon("refresh-cw")) "Recuperar las del recetario" } }
                    }
                }
            }
        } @else {
            (photo_banner(v, photos))
            form.rc-search method="get" action="/" role="search" {
                input type="hidden" name="month" value=(v.m);
                input type="hidden" name="tab" value="recipes";
                @if !filter.is_empty() { input type="hidden" name="cat" value=(filter); }
                span.rc-search-ico { (icon("search")) }
                input.input type="search" name="q" value=(q) aria-label="Buscar recetas" autocomplete="off" enterkeyhint="search"
                    placeholder="Busca por nombre o ingrediente: garbanzos, tortilla…";
            }
            nav.rc-cats aria-label="Categorías" {
                a.rc-cat.on[filter.is_empty()] href=(url(v, &qs(&[("q", q)]))) { (ico("book-open")) "Todas" small { (book.len()) } }
                @if book.iter().any(|r| r.favorite) {
                    a.rc-cat.on[filter == "fav"] style="--c:#e11d48" href=(url(v, &qs(&[("q", q), ("cat", "fav")]))) { (ico("heart")) "Favoritas" }
                }
                @for (name, ic, color) in used {
                    a.rc-cat.on[filter == *name] style={ "--c:" (color) } href=(url(v, &qs(&[("q", q), ("cat", name)]))) {
                        (ico(ic)) (name) small { (count(name)) }
                    }
                }
            }
            section.card.rc-none hidden[visible > 0] { (empty("search", "Nada por aquí", "Prueba con otra palabra o quita el filtro.")) }
            div.rc-grid { @for r in &shown { (card(v, r, key(r), !matches(key(r), q))) } }
            div.rc-restore {
                @if hidden > 0 {
                    form method="post" action="/plugins/recipes/restore" {
                        (v.ctx())
                        button.btn.ghost.sm { (icon("refresh-cw")) "Recuperar " (hidden) " receta" @if hidden != 1 { "s" } " del recetario que borrasteis" }
                    }
                }
                @if photos.have > 0 && photos.running.is_none() {
                    form method="post" action="/plugins/recipes/photos" { (v.ctx()) input type="hidden" name="do" value="remove"; button.btn.ghost.sm { (icon("trash-2")) "Quitar las fotos del recetario" } }
                } @else if photos.skipped && photos.missing > 0 {
                    form method="post" action="/plugins/recipes/photos" { (v.ctx()) input type="hidden" name="do" value="get"; button.btn.ghost.sm { (icon("image")) "Descargar las fotos del recetario" } }
                }
            }
        }
        (import_modal(v))
    }
}

fn detail(v: &View, r: &Recipe, serves: i64, have: Option<&str>) -> Markup {
    let owned = |name: &str| have.map_or_else(|| staple(name), |h| h.lines().any(|l| l == name));
    let (_, ic, color) = cat(&r.category);
    let factor = serves as f64 / r.serves as f64;
    let has_ings = ingredients_of(r).next().is_some();
    let nsteps = lines_of(&r.steps)
        .filter(|l| matches!(l, Line::Plain(_)))
        .count();
    let base = url(v, &format!("r={}", r.id));
    let ctx = html! { (v.ctx()) input type="hidden" name="serves" value=(serves); };
    let mut n = 0;
    html! {
        a.back href=(v.link("recipes")) { (icon("chevron-left")) "Recetas" }
        article.rc-page style={ "--c:" (color) } {
            header.card.rc-hero {
                div.rc-hero-cover { (cover(r, icon(ic))) }
                div.rc-hero-txt {
                    div.rc-tags {
                        span.pill.rc-pill { (icon(ic)) (r.category) }
                        @if !r.area.is_empty() { (pill("", None, &r.area)) }
                    }
                    h1 { (r.name) }
                    div.rc-facts {
                        @if let Some(m) = r.minutes {
                            div.rc-fact { (icon("clock")) span { b { (mins(m)) } small { "en total" } } }
                        }
                        div.rc-fact {
                            (icon("users"))
                            span { b { (serves) @if serves == 1 { " ración" } @else { " raciones" } } small { "ajusta las cantidades" } }
                            div.rc-stepper {
                                a.icon-btn href={ (base) "&serves=" (serves - 1) } aria-label="Una ración menos" aria-disabled=[(serves <= 1).then_some("true")] { (icon("minus")) }
                                a.icon-btn href={ (base) "&serves=" (serves + 1) } aria-label="Una ración más" aria-disabled=[(serves >= 50).then_some("true")] { (icon("plus")) }
                            }
                        }
                        @if r.cooked > 0 {
                            div.rc-fact { (icon("chef-hat")) span { b { "Hecha " (r.cooked) @if r.cooked == 1 { " vez" } @else { " veces" } } small { "en casa" } } }
                        }
                    }
                    div.rc-actions {
                        form.inline method="post" action={ "/plugins/recipes/" (r.id) "/favorite" } {
                            (ctx)
                            button.btn.outline.sm.rc-heart.on[r.favorite] aria-pressed=(r.favorite) { (icon("heart")) @if r.favorite { "En favoritas" } @else { "Favorita" } }
                        }
                        form.inline method="post" action={ "/plugins/recipes/" (r.id) "/cooked" } {
                            (ctx)
                            button.btn.outline.sm { (icon("utensils")) "La hemos hecho hoy" }
                        }
                        a.btn.ghost.sm href=(url(v, &format!("r={}&edit=1", r.id))) { (icon("pencil")) "Editar" }
                        a.btn.danger.sm href="#delete-recipe" { (icon("trash-2")) "Borrar" }
                    }
                }
            }
            div.rc-body {
                section.card.rc-ing {
                    div.card-head {
                        div {
                            h2 { "Ingredientes" }
                            p { "Para " (serves) @if serves == 1 { " ración" } @else { " raciones" } @if serves != r.serves { " · la receta original es para " (r.serves) } }
                        }
                    }
                    @if !has_ings {
                        p.hint { "Todavía no tiene ingredientes. Añádelos desde «Editar»." }
                    } @else {
                        form method="post" action={ "/plugins/recipes/" (r.id) "/shop" } {
                            input type="hidden" name="next" value={ (base) "&serves=" (serves) };
                            ul.rc-ings {
                                @for l in lines_of(&r.ingredients) {
                                    @match l {
                                        Line::Section(t) => li.rc-ing-h { (t) },
                                        Line::Plain(t) | Line::Tip(t) => {
                                            @let g = parse_line(t);
                                            @let (gi, gc) = products::look(&g.name);
                                            @let qty = g.qty_for(factor);
                                            li {
                                                input type="hidden" name="item" value=(shop_name(&g.name));
                                                label.rc-ing-row {
                                                    input type="checkbox" name="skip" value=(shop_name(&g.name)) checked[owned(&shop_name(&g.name))] aria-label={ "Lo tengo: " (g.name) };
                                                    span.tile.sm style={ "--c:" (gc) } { (icon(gi)) }
                                                    span.rc-ing-txt { @if !qty.is_empty() { b { (qty) } " " } (g.name) }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            button.btn.primary.wide { (icon("shopping-basket")) "Añadir lo que falta a la compra" }
                            p.hint.top { "Marca lo que ya tengáis. Lo que quede sin marcar va a la lista, sin repetir lo que ya esté." }
                        }
                    }
                }
                div.stack {
                    section.card.rc-steps {
                        div.card-head {
                            div { h2 { "Preparación" } p { (nsteps) " paso" @if nsteps != 1 { "s" } " · toca cada uno al terminarlo" } }
                        }
                        @if nsteps == 0 {
                            p.hint { "Todavía no tiene pasos. Añádelos desde «Editar»." }
                        } @else {
                            ol {
                                @for l in lines_of(&r.steps) {
                                    @match l {
                                        Line::Section(t) => li.rc-step-h { h3 { (t) } },
                                        Line::Tip(t) => li.rc-tip { span.rc-tip-ico { (icon("lightbulb")) } p { (t) } },
                                        Line::Plain(t) => {
                                            @let _ = { n += 1; };
                                            li { label.rc-step { input type="checkbox"; span.rc-n { span { (n) } (icon("check")) } p { (t) } } }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    @if !r.note.is_empty() {
                        section.card.rc-note { div.card-head { div { h2 { "Notas de casa" } } } p.rc-note-txt { (r.note) } }
                    }
                    @if r.photo == BUILTIN_PHOTO {
                        @if let Some((author, license, page)) = credit(&r.source) {
                            a.rc-source href=(page) target="_blank" rel="noopener noreferrer" { (icon("camera")) "Foto: " (author) " · " (license) }
                        }
                    }
                    @if let Some(id) = r.source.strip_prefix("themealdb:") {
                        a.rc-source href={ "https://www.themealdb.com/meal/" (id) } target="_blank" rel="noopener noreferrer" { (icon("external-link")) "Vista en TheMealDB" }
                    } @else if r.source.starts_with("http") {
                        a.rc-source href=(r.source) target="_blank" rel="noopener noreferrer nofollow" { (icon("external-link")) "Receta original en " (r.source.split('/').nth(2).unwrap_or("la web").trim_start_matches("www.")) }
                    }
                }
            }
        }
        div.modal #delete-recipe role="dialog" aria-modal="true" aria-labelledby="delete-recipe-t" {
            a.scrim href="#_" aria-label="Cerrar" tabindex="-1" {}
            div.dialog {
                span.dialog-ico { (icon("trash-2")) }
                h2 #delete-recipe-t { "¿Borrar «" (r.name) "»?" }
                p {
                    "Desaparece del recetario de toda la casa, con su foto."
                    @if r.source.starts_with("nido:") { " Si es del recetario de Nido, se puede recuperar desde la lista." } @else { " No se puede deshacer." }
                }
                form.dialog-actions method="post" action={ "/plugins/recipes/" (r.id) "/delete" } {
                    (v.ctx())
                    a.btn.ghost href="#_" { "Cancelar" }
                    button.btn.destructive { "Borrar receta" }
                }
            }
        }
    }
}

/// The recipe as a page, Notion-style: cover, a big title, properties, then blocks. Without JS the
/// block fields are plain textareas (one line per ingredient or step), so the form always works.
fn editor(v: &View, r: Option<&Recipe>) -> Markup {
    let (action, back, back_label) = match r {
        Some(r) => (
            format!("/plugins/recipes/{}", r.id),
            url(v, &format!("r={}", r.id)),
            "Volver a la receta",
        ),
        None => ("/plugins/recipes/new".to_string(), v.link("recipes"), "Recetas"),
    };
    let current = r.map_or("Otros", |r| r.category.as_str());
    let (_, ic, color) = cat(current);
    let photo = r.is_some_and(|r| r.photo > 0);
    // Catalog and TheMealDB recipes keep their origin; only own recipes have an editable link.
    let own_link = !r.is_some_and(|r| !r.source.is_empty() && !r.source.starts_with("http"));
    html! {
        a.back href=(back) { (icon("chevron-left")) (back_label) }
        form.rc-editor method="post" action=(action) enctype="multipart/form-data" accept-charset="utf-8" style={ "--c:" (color) } {
            (v.ctx())
            div.rc-ed-cover {
                @match r {
                    Some(r) if photo => img.rc-cover src={ "/plugins/recipes/" (r.id) "/photo?v=" (r.photo) } alt="";
                    _ => div.rc-cover.rc-art aria-hidden="true" { (icon(ic)) }
                }
                div.rc-ed-cover-actions {
                    label.btn.outline.sm.rc-ed-file {
                        input type="file" name="photo" accept="image/*";
                        (icon("camera")) @if photo { "Cambiar portada" } @else { "Añadir portada" }
                    }
                    @if photo {
                        label.btn.outline.sm.rc-ed-file.rc-ed-remove { input type="checkbox" name="remove_photo" value="1"; (icon("trash-2")) "Quitar" }
                    }
                }
            }
            label.sr for="rc-title" { "Nombre de la receta" }
            input #rc-title.rc-ed-title name="name" required maxlength="100" placeholder="Sin título" autocomplete="off" value=[r.map(|r| &r.name)] autofocus[r.is_none()];
            div.rc-props {
                div.rc-prop {
                    label for="rc-cat" { (icon("book-open")) "Categoría" }
                    select #rc-cat.rc-prop-in name="category" {
                        @for (name, ..) in CATS { option value=(name) selected[*name == current] { (name) } }
                    }
                }
                div.rc-prop {
                    label for="rc-serves" { (icon("users")) "Raciones" }
                    input #rc-serves.rc-prop-in type="number" name="serves" min="1" max="50" inputmode="numeric" value=(r.map_or(4, |r| r.serves));
                }
                div.rc-prop {
                    label for="rc-min" { (icon("clock")) "Minutos" }
                    input #rc-min.rc-prop-in type="number" name="minutes" min="1" max="1440" inputmode="numeric" placeholder="Vacío" value=[r.and_then(|r| r.minutes)];
                }
                div.rc-prop {
                    label for="rc-area" { (icon("house")) "Origen" }
                    input #rc-area.rc-prop-in name="area" maxlength="40" placeholder="Vacío" value=[r.map(|r| &r.area).filter(|a| !a.is_empty())];
                }
                @if own_link {
                    div.rc-prop {
                        label for="rc-src" { (icon("external-link")) "Enlace" }
                        input #rc-src.rc-prop-in type="url" name="source" maxlength="500" placeholder="Vacío" value=[r.map(|r| &r.source).filter(|s| !s.is_empty())];
                    }
                }
            }
            div.rc-ed-sec { (icon("shopping-basket")) "Ingredientes" }
            textarea.input name="ingredients" data-blocks="ing" rows="8" aria-label="Ingredientes"
                placeholder="300 g de lentejas\n1 zanahoria\n## Para el sofrito\n1 cebolla" { (r.map_or("", |r| r.ingredients.as_str())) }
            div.rc-ed-sec { (icon("list-checks")) "Preparación" }
            textarea.input name="steps" data-blocks="steps" rows="8" aria-label="Preparación"
                placeholder="Pica la cebolla y sofríela 10 minutos.\n> Truco: a fuego suave." { (r.map_or("", |r| r.steps.as_str())) }
            div.rc-ed-sec { (icon("pencil")) "Notas de casa" }
            textarea.input.rc-ed-note name="note" rows="3" maxlength="2000" aria-label="Notas de casa"
                placeholder="Trucos, con qué acompañarla, quién la hace mejor…" { (r.map_or("", |r| r.note.as_str())) }
            div.rc-ed-bar {
                span.rc-ed-keys { kbd { "/" } " bloques" span.rc-ed-dot { "·" } kbd { "Alt ↑↓" } " mover" span.rc-ed-dot { "·" } kbd { "Ctrl S" } " guardar" }
                a.btn.ghost href=(back) { "Cancelar" }
                button.btn.primary { (icon("check")) "Guardar" }
            }
        }
    }
}

fn import_modal(v: &View) -> Markup {
    html! {
        div.modal #import-recipe role="dialog" aria-modal="true" aria-labelledby="import-recipe-t" {
            a.scrim href="#_" aria-label="Cerrar" tabindex="-1" {}
            div.dialog {
                div.dialog-top {
                    h2 #import-recipe-t { "Importar una receta" }
                    a.icon-btn href="#_" aria-label="Cerrar" { (icon("x")) }
                }
                p { "Pega el enlace de una receta de cualquier web: se guarda con su foto, sus ingredientes y sus pasos, y después es vuestra para editarla." }
                form.form method="post" action="/plugins/recipes/import" accept-charset="utf-8" {
                    (v.ctx())
                    div.field {
                        label for="imp-q" { "Enlace de la receta" }
                        input #imp-q.input name="q" required maxlength="1000" inputmode="url" autocomplete="off" placeholder="https://…";
                        p.hint { "Funciona con la mayoría de webs de recetas. Si escribes un nombre de plato en inglés, lo busca en TheMealDB. Solo aquí el recetario sale a internet." }
                    }
                    button.btn.primary.wide { (icon("download")) "Importar" }
                }
            }
        }
    }
}

// ---------- actions ----------

fn to(c: &Connection, month_q: Option<&str>, id: Option<i64>, serves: Option<&str>, ok: &str) -> R {
    let m = month(c, month_q)?;
    let mut u = format!("/?month={m}&tab=recipes");
    if let Some(id) = id {
        u += &format!("&r={id}");
    }
    if let Some(s) = serves
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|n| (1..=50).contains(n))
    {
        u += &format!("&serves={s}");
    }
    Ok(Redirect::to(&format!("{u}&ok={ok}")).into_response())
}

fn field<'a>(f: &'a [(String, String)], k: &str) -> Option<&'a str> {
    f.iter().find(|p| p.0 == k).map(|p| p.1.as_str())
}

/// Lines trimmed, without control characters or list markers ("1.", "-", "•"), capped.
fn lines(s: &str, max: usize, len: usize, unmark: bool) -> String {
    s.lines()
        .map(|l| {
            let l = l.trim();
            let l = if unmark {
                let l = l.trim_start_matches(['-', '•', '*']).trim_start();
                let digits = l.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
                if digits > 0 && l[digits..].starts_with(['.', ')']) {
                    l[digits + 1..].trim_start()
                } else {
                    l
                }
            } else {
                l
            };
            l.chars()
                .filter(|c| !c.is_control())
                .take(len)
                .collect::<String>()
        })
        .filter(|l| !l.is_empty())
        .take(max)
        .collect::<Vec<_>>()
        .join("\n")
}

struct Data {
    name: String,
    category: &'static str,
    area: String,
    serves: i64,
    minutes: Option<i64>,
    ingredients: String,
    steps: String,
    note: String,
    source: Option<String>, // None = keep what it had
}

fn data(f: &[(String, String)]) -> Result<Data, &'static str> {
    let get = |k| field(f, k).unwrap_or("");
    let name = clean(get("name"));
    if name.is_empty() {
        return Err("Ponle un nombre a la receta");
    }
    let src = get("source").trim();
    Ok(Data {
        name,
        category: cat(get("category")).0,
        area: clean(get("area")).chars().take(40).collect(),
        serves: get("serves")
            .trim()
            .parse::<i64>()
            .unwrap_or(4)
            .clamp(1, 50),
        minutes: get("minutes")
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|m| (1..=1440).contains(m)),
        ingredients: lines(get("ingredients"), 60, 150, true),
        steps: lines(get("steps"), 40, 1000, true),
        note: get("note")
            .trim()
            .chars()
            .filter(|c| *c == '\n' || !c.is_control())
            .take(2000)
            .collect(),
        source: field(f, "source").map(|_| {
            if (src.starts_with("https://") || src.starts_with("http://")) && src.len() <= 500 {
                src.to_string()
            } else {
                String::new()
            }
        }),
    })
}

/// A photo in any format and orientation → 640×480 WebP (~25 KB): sharp on a phone, light on the Pi.
fn to_webp(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoder = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .into_decoder()
        .map_err(|_| "El archivo no es una imagen que podamos leer.".to_string())?;
    let orientation = decoder.orientation().map_err(|e| e.to_string())?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);
    let rgb = img.resize_to_fill(640, 480, FilterType::Triangle).to_rgb8();
    Ok(webp::Encoder::from_rgb(&rgb, rgb.width(), rgb.height())
        .encode(62.0)
        .to_vec())
}

fn set_photo(c: &Connection, id: i64, webp: Option<&[u8]>) -> rusqlite::Result<()> {
    match webp {
        Some(b) => {
            c.execute("INSERT INTO recipe_photos(recipe_id,webp) VALUES(?1,?2) ON CONFLICT(recipe_id) DO UPDATE SET webp=excluded.webp", params![id, b])?;
            // The version is a timestamp: a new photo never reuses the URL of an old cached one.
            c.execute(
                "UPDATE recipes SET photo=CAST(strftime('%s','now') AS INTEGER) WHERE id=?1",
                [id],
            )?;
        }
        None => {
            c.execute("DELETE FROM recipe_photos WHERE recipe_id=?1", [id])?;
            c.execute("UPDATE recipes SET photo=0 WHERE id=?1", [id])?;
        }
    }
    Ok(())
}

/// Reads the recipe form: text fields, plus the photo already turned into WebP (off the main thread).
async fn read_form(mut m: Multipart) -> Result<(Vec<(String, String)>, Option<Vec<u8>>), Response> {
    let fail = |e: String| (StatusCode::BAD_REQUEST, e).into_response();
    let mut fields = Vec::new();
    let mut photo = Vec::new();
    while let Some(f) = m.next_field().await.map_err(|e| fail(e.to_string()))? {
        let name = f.name().unwrap_or("").to_string();
        if name == "photo" {
            photo = f.bytes().await.map_err(|e| fail(e.to_string()))?.to_vec();
        } else {
            fields.push((name, f.text().await.map_err(|e| fail(e.to_string()))?));
        }
    }
    if photo.is_empty() {
        return Ok((fields, None));
    }
    match tokio::task::spawn_blocking(move || to_webp(&photo)).await {
        Ok(Ok(w)) => Ok((fields, Some(w))),
        Ok(Err(e)) => Err(fail(e)),
        Err(e) => Err(fail(e.to_string())),
    }
}

async fn create(State(db): State<Db>, h: HeaderMap, m: Multipart) -> R {
    let me = require_user!((), h); // who it is comes in Nido's headers: no need to touch the database yet
    let (f, photo) = match read_form(m).await {
        Ok(x) => x,
        Err(r) => return Ok(r),
    };
    let d = match data(&f) {
        Ok(d) => d,
        Err(e) => return bad(e),
    };
    let c = lock(&db);
    let tx = c.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO recipes(name,category,area,serves,minutes,note,source,added_by,search) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![d.name, d.category, d.area, d.serves, d.minutes, d.note, d.source.unwrap_or_default(), me.id, search_key(&d.name, &d.area, d.category)],
    )?;
    let id = tx.last_insert_rowid();
    save_lines(&tx, id, &d.ingredients, &d.steps)?;
    if let Some(p) = photo {
        set_photo(&tx, id, Some(&p))?;
    }
    tx.commit()?;
    to(&c, field(&f, "month"), Some(id), None, "recipe_added")
}

async fn update(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, m: Multipart) -> R {
    require_user!((), h);
    let (f, photo) = match read_form(m).await {
        Ok(x) => x,
        Err(r) => return Ok(r),
    };
    let d = match data(&f) {
        Ok(d) => d,
        Err(e) => return bad(e),
    };
    let c = lock(&db);
    let tx = c.unchecked_transaction()?;
    let n = tx.execute(
        "UPDATE recipes SET name=?1,category=?2,area=?3,serves=?4,minutes=?5,note=?6,source=COALESCE(?7,source),search=?8 WHERE id=?9",
        params![d.name, d.category, d.area, d.serves, d.minutes, d.note, d.source, search_key(&d.name, &d.area, d.category), id],
    )?;
    if n == 0 {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    save_lines(&tx, id, &d.ingredients, &d.steps)?;
    if let Some(p) = photo {
        set_photo(&tx, id, Some(&p))?;
    } else if field(&f, "remove_photo").is_some() {
        set_photo(&tx, id, None)?;
    }
    tx.commit()?;
    to(&c, field(&f, "month"), Some(id), None, "recipe_saved")
}

async fn photo(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    let webp: Option<Vec<u8>> = c
        .query_row(
            "SELECT webp FROM recipe_photos WHERE recipe_id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match webp {
        Some(b) => (
            [
                (header::CONTENT_TYPE, "image/webp"),
                (
                    header::CACHE_CONTROL,
                    "private, max-age=31536000, immutable",
                ),
            ],
            b,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    })
}

// ---------- catalog photos: downloaded apart, only if the house wants them ----------

/// Where the catalog photos are published (the repository, next to this code).
fn photos_base() -> String {
    std::env::var("NIDO_RECETAS_FOTOS").unwrap_or_else(|_| {
        format!(
            "https://raw.githubusercontent.com/{}/master/plugins/recipes/photos/",
            option_env!("NIDO_REPO").unwrap_or("michyaraque/nido")
        )
    })
}

/// Download in progress: (done, total).
static DOWNLOAD: std::sync::Mutex<Option<(usize, usize)>> = std::sync::Mutex::new(None);
/// The last download brought nothing: the banner says so instead of quietly going back to the start.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct PhotoState {
    missing: usize, // catalog recipes that could have a photo and do not
    have: usize,    // catalog photos stored
    skipped: bool,  // the house said "not now"
    running: Option<(usize, usize)>,
    failed: bool,
}

fn photo_slugs() -> impl Iterator<Item = &'static str> {
    CREDITS
        .lines()
        .filter_map(|l| l.split('\t').next())
        .filter(|s| !s.is_empty())
}

fn missing_photos(c: &Connection) -> rusqlite::Result<Vec<String>> {
    let wanted: std::collections::HashSet<&str> = photo_slugs().collect();
    let rows: Vec<String> = c
        .prepare("SELECT substr(source,6) FROM recipes WHERE photo=0 AND source LIKE 'nido:%'")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(rows
        .into_iter()
        .filter(|s| wanted.contains(s.as_str()))
        .collect())
}

fn photo_state(c: &Connection) -> rusqlite::Result<PhotoState> {
    Ok(PhotoState {
        missing: missing_photos(c)?.len(),
        have: c.query_row(
            "SELECT count(*) FROM recipes WHERE photo=?1",
            [BUILTIN_PHOTO],
            |r| r.get(0),
        )?,
        skipped: c.query_row(
            "SELECT EXISTS (SELECT 1 FROM recipe_settings WHERE key='photos_skip' AND value='1')",
            [],
            |r| r.get(0),
        )?,
        running: *DOWNLOAD.lock().unwrap_or_else(|e| e.into_inner()),
        failed: FAILED.load(std::sync::atomic::Ordering::Relaxed),
    })
}

fn photo_banner(v: &View, p: &PhotoState) -> Markup {
    html! {
        @if let Some((done, total)) = p.running {
            // app.js reloads the page every 2 s while this is on screen.
            div.card.rc-banner data-refresh="2" role="status" {
                span.rc-banner-ico.busy { (icon("loader-circle")) }
                div.rc-banner-txt { b { "Descargando las fotos del recetario" } small { (done) " de " (total) " · puedes seguir usando Nido" } }
                div.meter { span style={ "width:" ((done * 100).checked_div(total).unwrap_or(0)) "%" } {} }
            }
        } @else if p.failed && p.missing > 0 {
            div.card.rc-banner.bad role="alert" {
                span.rc-banner-ico { (icon("cloud-off")) }
                div.rc-banner-txt { b { "No se han podido descargar las fotos" } small { "Revisa que Nido tenga internet y vuelve a intentarlo." } }
                div.rc-banner-actions {
                    form method="post" action="/plugins/recipes/photos" { (v.ctx()) input type="hidden" name="do" value="skip"; button.btn.ghost.sm { "Ahora no" } }
                    form method="post" action="/plugins/recipes/photos" { (v.ctx()) input type="hidden" name="do" value="get"; button.btn.primary.sm { (icon("refresh-cw")) "Reintentar" } }
                }
            }
        } @else if p.missing > 0 && !p.skipped {
            div.card.rc-banner {
                span.rc-stack aria-hidden="true" { span { (icon("egg")) } span { (icon("fish")) } span { (icon("cookie")) } }
                div.rc-banner-txt {
                    b { "Fotos para el recetario" }
                    @let tenths = (p.missing * 14 + 50) / 100; // ~14 KB each, in tenths of a MB
                    small { (p.missing) " fotos, unos " (tenths / 10) "," (tenths % 10) " MB. Se descargan una vez y se guardan en Nido." }
                }
                div.rc-banner-actions {
                    form method="post" action="/plugins/recipes/photos" { (v.ctx()) input type="hidden" name="do" value="skip"; button.btn.ghost.sm { "Ahora no" } }
                    form method="post" action="/plugins/recipes/photos" { (v.ctx()) input type="hidden" name="do" value="get"; button.btn.primary.sm { (icon("download")) "Descargar fotos" } }
                }
            }
        }
    }
}

/// Saves one downloaded catalog photo, if it really is a small WebP.
fn store_catalog_photo(c: &Connection, slug: &str, bytes: &[u8]) -> rusqlite::Result<bool> {
    if bytes.len() < 12
        || bytes.len() > 400_000
        || &bytes[..4] != b"RIFF"
        || &bytes[8..12] != b"WEBP"
    {
        return Ok(false);
    }
    let id: Option<i64> = c
        .query_row(
            "SELECT id FROM recipes WHERE source=?1 AND photo=0",
            [format!("nido:{slug}")],
            |r| r.get(0),
        )
        .optional()?;
    let Some(id) = id else { return Ok(false) };
    c.execute(
        "INSERT OR REPLACE INTO recipe_photos(recipe_id,webp) VALUES(?1,?2)",
        params![id, bytes],
    )?;
    c.execute(
        "UPDATE recipes SET photo=?2 WHERE id=?1",
        params![id, BUILTIN_PHOTO],
    )?;
    Ok(true)
}

/// Downloads in the background, a dozen at a time (curl in parallel); the page shows the progress.
fn download_photos(db: Db, slugs: Vec<String>) {
    *DOWNLOAD.lock().unwrap_or_else(|e| e.into_inner()) = Some((0, slugs.len()));
    std::thread::spawn(move || {
        let dir = std::env::temp_dir().join(format!("nido-recipes-photos-{}", std::process::id()));
        let base = photos_base();
        let total = slugs.len();
        // A second pass for whatever a flaky connection dropped the first time.
        for pass in 0..2 {
            let todo = if pass == 0 {
                slugs.clone()
            } else {
                missing_photos(&lock(&db)).unwrap_or_default()
            };
            for (n, chunk) in todo.chunks(12).enumerate() {
                let _ = std::fs::create_dir_all(&dir);
                let cfg: String = chunk
                    .iter()
                    .map(|s| {
                        format!(
                            "url = \"{base}{s}.webp\"\noutput = \"{}\"\n",
                            dir.join(format!("{s}.webp"))
                                .display()
                                .to_string()
                                .replace('\\', "/")
                        )
                    })
                    .collect();
                let child = std::process::Command::new("curl")
                    .args([
                        "-K",
                        "-",
                        "--parallel",
                        "-sf",
                        "--proto",
                        "=https,http",
                        "--max-time",
                        "60",
                    ])
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                if let Ok(mut child) = child {
                    use std::io::Write;
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = stdin.write_all(cfg.as_bytes());
                    }
                    let _ = child.wait();
                }
                {
                    let c = lock(&db);
                    for s in chunk {
                        let file = dir.join(format!("{s}.webp"));
                        if let Ok(bytes) = std::fs::read(&file) {
                            let _ = store_catalog_photo(&c, s, &bytes);
                        }
                        let _ = std::fs::remove_file(file);
                    }
                }
                if pass == 0 {
                    *DOWNLOAD.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some((((n + 1) * 12).min(total), total));
                }
            }
        }
        let _ = std::fs::remove_dir(&dir);
        let left = missing_photos(&lock(&db)).map(|m| m.len()).unwrap_or(total);
        FAILED.store(left == total, std::sync::atomic::Ordering::Relaxed);
        if left == total {
            eprintln!("fotos del recetario: no se ha descargado ninguna desde {base}");
        }
        *DOWNLOAD.lock().unwrap_or_else(|e| e.into_inner()) = None;
    });
}

#[derive(Deserialize)]
struct PhotosF {
    #[serde(rename = "do")]
    action: String,
    month: String,
}

async fn photos(State(db): State<Db>, h: HeaderMap, F(f): F<PhotosF>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    let skip = |v: &str| {
        c.execute("INSERT INTO recipe_settings(key,value) VALUES('photos_skip',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [v])
    };
    let ok = match f.action.as_str() {
        "get" => {
            skip("0")?;
            FAILED.store(false, std::sync::atomic::Ordering::Relaxed);
            let busy = DOWNLOAD.lock().unwrap_or_else(|e| e.into_inner()).is_some();
            let missing = missing_photos(&c)?;
            if !busy && !missing.is_empty() {
                download_photos(db.clone(), missing);
            }
            "recipe_photos"
        }
        "remove" => {
            c.execute("DELETE FROM recipe_photos WHERE recipe_id IN (SELECT id FROM recipes WHERE photo=?1)", [BUILTIN_PHOTO])?;
            c.execute("UPDATE recipes SET photo=0 WHERE photo=?1", [BUILTIN_PHOTO])?;
            skip("1")?;
            "recipe_photos_removed"
        }
        _ => {
            skip("1")?;
            ""
        }
    };
    to(&c, Some(&f.month), None, None, ok)
}

type Pairs = Vec<(String, String)>;

/// "Añadir lo que falta": keeps what was ticked as "lo tengo", so the recipe shows it next time,
/// then hands the same form to Nido's shopping list (a 307 keeps the method and the body).
async fn shop(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Pairs>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    let have: Vec<&str> = f.iter().filter(|p| p.0 == "skip").map(|p| p.1.as_str()).collect();
    c.execute(
        "INSERT INTO recipe_have(recipe_id,items) SELECT id,?2 FROM recipes WHERE id=?1
         ON CONFLICT(recipe_id) DO UPDATE SET items=excluded.items",
        params![id, have.join("\n")],
    )?;
    Ok((StatusCode::TEMPORARY_REDIRECT, [(header::LOCATION, "/shopping/many")]).into_response())
}

async fn favorite(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Pairs>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    c.execute("UPDATE recipes SET favorite=1-favorite WHERE id=?1", [id])?;
    to(&c, field(&f, "month"), Some(id), field(&f, "serves"), "")
}

async fn cooked(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Pairs>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    c.execute("UPDATE recipes SET cooked=cooked+1 WHERE id=?1", [id])?;
    to(
        &c,
        field(&f, "month"),
        Some(id),
        field(&f, "serves"),
        "recipe_cooked",
    )
}

async fn delete(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Pairs>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    // A deleted catalog recipe is remembered, so the next release does not bring it back.
    c.execute("INSERT OR IGNORE INTO recipe_hidden(source) SELECT source FROM recipes WHERE id=?1 AND source LIKE 'nido:%'", [id])?;
    c.execute("DELETE FROM recipes WHERE id=?1", [id])?; // its photo goes with it (ON DELETE CASCADE)
    to(&c, field(&f, "month"), None, None, "recipe_deleted")
}

async fn restore(State(db): State<Db>, h: HeaderMap, F(f): F<Pairs>) -> R {
    let c = lock(&db);
    require_user!(c, h);
    c.execute("DELETE FROM recipe_hidden", [])?;
    seed_catalog(&c)?;
    to(&c, field(&f, "month"), None, None, "recipe_restored")
}

// ---------- importing: any recipe page, or TheMealDB ----------

/// A recipe read from outside, ready to be saved.
struct Imported {
    name: String,
    category: &'static str,
    area: String,
    serves: i64,
    minutes: Option<i64>,
    ingredients: String,
    steps: String,
    note: String,
    source: String,
    image: String, // absolute http(s) URL, or empty
}

/// Fetches a URL with curl: http(s) only (also after redirects), size and time capped.
fn curl(url: &str) -> Result<Vec<u8>, String> {
    let out = std::process::Command::new("curl")
        .args(["-sfL", "--compressed", "--max-time", "20", "--max-filesize", "8000000", "--proto", "=http,https", "--proto-redir", "=http,https"])
        .args(["-A", "Mozilla/5.0 (X11; Linux aarch64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36 Nido"])
        .args(["-H", "Accept-Language: es-ES,es;q=0.9,en;q=0.5", "--", url])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(
            "No se ha podido abrir esa página. Revisa el enlace y que Nido tenga internet."
                .into(),
        )
    }
}

/// Only public web pages: not the Pi itself, the router or anything else on the home network.
// Checks the written host only (no DNS lookup); a public name pointing inside the house
// would pass. Everyone who can use this is already a person of the house.
fn public_url(url: &str) -> Option<&str> {
    let url = url.trim();
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    if url.len() > 1000 || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next()?.rsplit('@').next()?;
    let host = host
        .rsplit_once(':')
        .filter(|(_, p)| p.bytes().all(|b| b.is_ascii_digit()))
        .map_or(host, |(h, _)| h)
        .to_lowercase();
    let local = host == "localhost"
        || host.ends_with(".local")
        || host.ends_with(".lan")
        || host.ends_with(".home")
        || host.ends_with(".internal")
        || host.starts_with('[')
        || !host.contains('.');
    let private = match host
        .split('.')
        .map(|p| p.parse::<u8>())
        .collect::<Result<Vec<_>, _>>()
        .as_deref()
    {
        Ok([a, b, _, _]) => {
            matches!(
                (a, b),
                (0 | 10 | 127, _) | (169, 254) | (192, 168) | (100, 64..=127)
            ) || (*a == 172 && (16..=31).contains(b))
        }
        _ => false,
    };
    (!local && !private).then_some(url)
}

/// HTML text → plain text: tags out, entities decoded, spaces collapsed.
fn plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut tag = false;
    for c in s.chars() {
        match c {
            '<' => tag = true,
            '>' if tag => {
                tag = false;
                out.push(' ');
            }
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    let mut text = String::with_capacity(out.len());
    let mut rest = out.as_str();
    while let Some(i) = rest.find('&') {
        text.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.find(';').filter(|e| *e <= 10);
        let decoded = end.and_then(|e| match &rest[1..e] {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "aacute" => Some('á'),
            "eacute" => Some('é'),
            "iacute" => Some('í'),
            "oacute" => Some('ó'),
            "uacute" => Some('ú'),
            "ntilde" => Some('ñ'),
            "uuml" => Some('ü'),
            "Aacute" => Some('Á'),
            "Eacute" => Some('É'),
            "Iacute" => Some('Í'),
            "Oacute" => Some('Ó'),
            "Uacute" => Some('Ú'),
            "Ntilde" => Some('Ñ'),
            "iexcl" => Some('¡'),
            "iquest" => Some('¿'),
            "ordm" => Some('º'),
            "deg" => Some('°'),
            "frac12" => Some('½'),
            "frac14" => Some('¼'),
            "frac34" => Some('¾'),
            n if n.starts_with("#x") || n.starts_with("#X") => u32::from_str_radix(&n[2..], 16)
                .ok()
                .and_then(char::from_u32),
            n if n.starts_with('#') => n[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        });
        match (decoded, end) {
            (Some(c), Some(e)) => {
                text.push(c);
                rest = &rest[e + 1..];
            }
            _ => {
                text.push('&');
                rest = &rest[1..];
            }
        }
    }
    text.push_str(rest);
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A JSON-LD value as text: a string, or the first of a list, or an object's `url`/`name`/`text`.
fn ld_text(v: &Value) -> String {
    match v {
        Value::String(s) => plain(s),
        Value::Number(n) => n.to_string(),
        Value::Array(a) => a.first().map(ld_text).unwrap_or_default(),
        Value::Object(o) => o
            .get("url")
            .or_else(|| o.get("text"))
            .or_else(|| o.get("name"))
            .map(ld_text)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// The schema.org Recipe object of a page, wherever its JSON-LD puts it (alone, in a list, in @graph).
fn ld_recipe(html: &str) -> Option<Value> {
    fn find(v: &Value) -> Option<&Value> {
        let is_recipe = |t: &Value| {
            t.as_str() == Some("Recipe")
                || t.as_array()
                    .is_some_and(|a| a.iter().any(|x| x.as_str() == Some("Recipe")))
        };
        match v {
            Value::Object(o) if o.get("@type").is_some_and(is_recipe) => Some(v),
            Value::Object(o) => o
                .get("@graph")
                .or_else(|| o.get("mainEntity"))
                .and_then(find),
            Value::Array(a) => a.iter().find_map(find),
            _ => None,
        }
    }
    let mut rest = html;
    while let Some(i) = rest.find("application/ld+json") {
        rest = &rest[i..];
        let start = rest.find('>')? + 1;
        let end = rest[start..].find("</script")? + start;
        let body = rest[start..end]
            .trim()
            .trim_start_matches("<!--")
            .trim_end_matches("-->")
            .trim();
        if let Some(r) = serde_json::from_str::<Value>(body)
            .ok()
            .as_ref()
            .and_then(find)
        {
            return Some(r.clone());
        }
        rest = &rest[end..];
    }
    None
}

/// "PT1H30M" → 90.
fn iso_minutes(s: &str) -> Option<i64> {
    let t = s.split_once('T')?.1;
    let (mut total, mut n) = (0i64, String::new());
    for c in t.chars() {
        match c {
            '0'..='9' | '.' => n.push(c),
            'H' => total += n.parse::<f64>().ok()? as i64 * 60,
            'M' => total += n.parse::<f64>().ok()? as i64,
            _ => {}
        }
        if c.is_ascii_alphabetic() {
            n.clear();
        }
    }
    (total > 0).then_some(total.min(1440))
}

/// Steps from `recipeInstructions`: text, a list of HowToStep, or HowToSections (each a "## " title).
fn ld_steps(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => {
            // Some sites put every step in one string, one per line or as <p>/<li>.
            let s = s
                .replace("</p>", "\n")
                .replace("</li>", "\n")
                .replace("<br>", "\n")
                .replace("<br />", "\n");
            out.extend(s.lines().map(plain).filter(|l| !l.is_empty()));
        }
        Value::Array(a) => a.iter().for_each(|x| ld_steps(x, out)),
        Value::Object(o) if o.contains_key("itemListElement") => {
            if let Some(name) = o.get("name").map(ld_text).filter(|n| !n.is_empty()) {
                out.push(format!("## {name}"));
            }
            ld_steps(&o["itemListElement"], out);
        }
        Value::Object(o) => {
            if let Some(t) = o.get("text").or_else(|| o.get("name")) {
                ld_steps(t, out);
            }
        }
        _ => {}
    }
}

/// Our category for an outside recipe, from its own category, its name and its keywords.
fn guess_cat(hints: &str) -> &'static str {
    const WORDS: &[(&str, &str)] = &[
        ("Postres", "postre postres dulce tarta bizcocho galleta galletas flan helado natillas mousse dessert cake cookie cookies"),
        ("Desayunos", "desayuno desayunos breakfast tortitas pancakes porridge"),
        ("Bebidas", "bebida bebidas coctel batido smoothie drink cocktail"),
        ("Salsas", "salsa salsas sauce aliño mayonesa"),
        ("Sopas y cremas", "sopa sopas crema cremas caldo gazpacho soup"),
        ("Legumbres", "lenteja lentejas garbanzo garbanzos alubia alubias judiones fabada legumbre legumbres lentils chickpeas beans"),
        ("Pasta y arroz", "pasta arroz paella risotto espaguetis macarrones lasana fideua fideos rice spaghetti noodles"),
        ("Pescados", "pescado pescados marisco merluza bacalao atun salmon gambas mejillones pulpo fish seafood"),
        ("Aves", "pollo pavo pato chicken turkey"),
        ("Carnes", "carne carnes ternera cerdo cordero albondigas solomillo beef pork lamb"),
        ("Huevos", "huevo huevos tortilla revuelto egg eggs omelette"),
        ("Verduras", "verdura verduras ensalada vegetal vegano vegetariano vegetables salad vegan vegetarian"),
        ("Entrantes", "entrante entrantes aperitivo tapa tapas starter appetizer"),
    ];
    let words = fold(hints);
    WORDS
        .iter()
        .find(|(_, ks)| {
            ks.split(' ')
                .any(|k| words.contains(&format!(" {} ", fold(k).trim())))
        })
        .map_or("Otros", |w| w.0)
}

/// Absolute URL of an image found on `page` ("//cdn…", "/img…" or already absolute).
fn absolute(page: &str, src: &str) -> String {
    let origin = page
        .find("://")
        .and_then(|i| page[i + 3..].find('/').map(|j| &page[..i + 3 + j]))
        .unwrap_or(page);
    match src {
        s if s.starts_with("https://") || s.starts_with("http://") => s.to_string(),
        s if s.starts_with("//") => format!("https:{s}"),
        s if s.starts_with('/') => format!("{origin}{s}"),
        _ => String::new(),
    }
}

/// Reads a recipe from its web page: the schema.org data most recipe sites publish for search engines.
fn from_page(url: &str, html: &str) -> Result<Imported, String> {
    let r = ld_recipe(html).ok_or("Esa página no publica la receta en un formato que podamos leer. Prueba con otra web o escríbela a mano.")?;
    let name = ld_text(&r["name"]);
    // "Huevos rotos con langostinos: receta para un plato informal…" → the dish, not the headline.
    let name = match name.split_once(": ") {
        Some((dish, _)) if name.chars().count() > 45 && dish.chars().count() >= 4 => {
            dish.to_string()
        }
        _ => name,
    };
    if name.is_empty() {
        return Err("La página no dice cómo se llama la receta".into());
    }
    let list = |v: &Value| -> Vec<String> {
        match v {
            Value::Array(a) => a.iter().map(ld_text).filter(|s| !s.is_empty()).collect(),
            Value::String(s) => s.lines().map(plain).filter(|l| !l.is_empty()).collect(),
            _ => Vec::new(),
        }
    };
    let ingredients = list(
        r.get("recipeIngredient")
            .or_else(|| r.get("ingredients"))
            .unwrap_or(&Value::Null),
    );
    let mut steps = Vec::new();
    ld_steps(&r["recipeInstructions"], &mut steps);
    let serves = ld_text(&r["recipeYield"])
        .split(|c: char| !c.is_ascii_digit())
        .find_map(|n| n.parse::<i64>().ok())
        .filter(|n| (1..=50).contains(n))
        .unwrap_or(4);
    let minutes = iso_minutes(&ld_text(&r["totalTime"])).or_else(|| {
        let (p, c) = (
            iso_minutes(&ld_text(&r["prepTime"])),
            iso_minutes(&ld_text(&r["cookTime"])),
        );
        (p.is_some() || c.is_some()).then(|| p.unwrap_or(0) + c.unwrap_or(0))
    });
    let hints = format!(
        "{} {} {}",
        ld_text(&r["recipeCategory"]),
        name,
        ld_text(&r["keywords"])
    );
    Ok(Imported {
        category: guess_cat(&hints),
        area: ld_text(&r["recipeCuisine"]).chars().take(40).collect(),
        serves,
        minutes,
        ingredients: lines(&ingredients.join("\n"), 60, 150, true),
        steps: lines(&steps.join("\n"), 40, 1000, true),
        note: ld_text(&r["description"]).chars().take(600).collect(),
        source: url.to_string(),
        image: absolute(url, &ld_text(&r["image"])),
        name,
    })
}

fn meal_cat(c: &str) -> &'static str {
    match c {
        "Beef" | "Lamb" | "Pork" | "Goat" => "Carnes",
        "Chicken" => "Aves",
        "Seafood" => "Pescados",
        "Pasta" => "Pasta y arroz",
        "Dessert" => "Postres",
        "Breakfast" => "Desayunos",
        "Starter" | "Side" => "Entrantes",
        "Vegan" | "Vegetarian" => "Verduras",
        _ => "Otros",
    }
}

fn meal(v: &Value) -> Option<Imported> {
    let s = |k: &str| v[k].as_str().unwrap_or("").trim().to_string();
    let id = s("idMeal");
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let ingredients = (1..=20)
        .filter_map(|i| {
            let ing = s(&format!("strIngredient{i}"));
            (!ing.is_empty()).then(|| {
                format!("{} {ing}", s(&format!("strMeasure{i}")))
                    .trim()
                    .to_string()
            })
        })
        .collect::<Vec<_>>()
        .join("\n");
    // Instructions come as paragraphs, sometimes with "STEP 1" headings of their own.
    let steps = s("strInstructions")
        .lines()
        .map(str::trim)
        .filter(|l| {
            let w = l.to_lowercase();
            let w = w
                .trim_start_matches("step")
                .trim()
                .trim_end_matches(['.', ':']);
            !l.is_empty() && !w.chars().all(|c| c.is_ascii_digit())
        })
        .collect::<Vec<_>>()
        .join("\n");
    let thumb = s("strMealThumb");
    Some(Imported {
        name: s("strMeal"),
        category: meal_cat(&s("strCategory")),
        area: s("strArea"),
        serves: 4,
        minutes: None,
        ingredients: lines(&ingredients, 60, 150, false),
        steps: lines(&steps, 40, 1000, true),
        note: String::new(),
        source: format!("themealdb:{id}"),
        image: if thumb.starts_with("https://www.themealdb.com/images/") {
            format!("{thumb}/medium")
        } else {
            String::new()
        },
    })
}

/// First TheMealDB meal named like `q` that the house does not have yet.
fn find_meal(q: &str, known: &[String]) -> Result<Imported, String> {
    let url = format!(
        "https://www.themealdb.com/api/json/v1/1/search.php?{}",
        qs(&[("s", q)])
    );
    let v: Value = serde_json::from_slice(&curl(&url)?)
        .map_err(|_| "TheMealDB no responde bien. Prueba en un rato.".to_string())?;
    let meals: Vec<Imported> = v["meals"]
        .as_array()
        .map(|a| a.iter().filter_map(meal).collect())
        .unwrap_or_default();
    if meals.is_empty() {
        return Err("No hay recetas con ese nombre en TheMealDB. Prueba en inglés (chicken, soup, cake…) o pega el enlace de una receta.".into());
    }
    meals
        .into_iter()
        .find(|m| !known.contains(&m.source))
        .ok_or_else(|| "Esas recetas ya están en vuestro recetario".into())
}

/// The recipe behind what was pasted (a link or a dish name), with its photo as WebP.
fn fetch_recipe(input: &str, known: &[String]) -> Result<(Imported, Option<Vec<u8>>), String> {
    let r = if input.starts_with("http://") || input.starts_with("https://") {
        let url = public_url(input).ok_or("Ese enlace no es una página web pública")?;
        let html = curl(url)?;
        from_page(url, &String::from_utf8_lossy(&html))?
    } else {
        find_meal(input, known)?
    };
    // Without a photo the recipe still comes in; the image is a bonus, never a reason to fail.
    let photo = public_url(&r.image)
        .and_then(|u| curl(u).ok())
        .and_then(|b| to_webp(&b).ok());
    Ok((r, photo))
}

#[derive(Deserialize)]
struct ImportF {
    q: String,
    month: String,
}

async fn import(State(db): State<Db>, h: HeaderMap, F(f): F<ImportF>) -> R {
    let input = f.q.trim().chars().take(1000).collect::<String>();
    let (me, known) = {
        let c = lock(&db);
        let me = require_user!(c, h);
        // Already imported from this page: open it instead of making a copy.
        let existing: Option<i64> = c
            .query_row("SELECT id FROM recipes WHERE source=?1", [&input], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(id) = existing.filter(|_| input.starts_with("http")) {
            return to(&c, Some(&f.month), Some(id), None, "recipe_exists");
        }
        let known: Vec<String> = c
            .prepare("SELECT source FROM recipes WHERE source LIKE 'themealdb:%'")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        (me, known)
    };
    if input.is_empty() {
        return bad("Pega el enlace de una receta o escribe un plato");
    }
    // The database is not locked while the other site answers.
    let (r, photo) = match tokio::task::spawn_blocking(move || fetch_recipe(&input, &known)).await?
    {
        Ok(x) => x,
        Err(e) => return Ok((StatusCode::BAD_REQUEST, e).into_response()),
    };
    let c = lock(&db);
    let tx = c.unchecked_transaction()?;
    let (name, area) = (clean(&r.name), clean(&r.area));
    tx.execute(
        "INSERT INTO recipes(name,category,area,serves,minutes,note,source,added_by,search) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![name, r.category, area, r.serves, r.minutes, r.note, r.source, me.id, search_key(&name, &area, r.category)],
    )?;
    let id = tx.last_insert_rowid();
    save_lines(&tx, id, &r.ingredients, &r.steps)?;
    if let Some(p) = photo {
        set_photo(&tx, id, Some(&p))?;
    }
    tx.commit()?;
    to(&c, Some(&f.month), Some(id), None, "recipe_imported")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ingredient_lines() {
        let p = parse_line("200 g de harina");
        assert_eq!(
            (p.amount, p.unit.as_str(), p.name.as_str()),
            (Some(200.0), "g", "harina")
        );
        let p = parse_line("2 dientes de ajo");
        assert_eq!(
            (p.qty_for(1.5).as_str(), p.name.as_str()),
            ("3 dientes", "ajo")
        );
        let p = parse_line("500ml leche");
        assert_eq!((p.unit.as_str(), p.name.as_str()), ("ml", "leche"));
        let p = parse_line("½ limón");
        assert_eq!(
            (p.amount, p.qty_for(2.0).as_str(), p.name.as_str()),
            (Some(0.5), "1", "limón")
        );
        let p = parse_line("1/2 cup Flour");
        assert_eq!(
            (p.qty_for(1.0).as_str(), p.name.as_str()),
            ("½ cup", "Flour")
        );
        let p = parse_line("Sal");
        assert_eq!(
            (p.amount, p.qty_for(2.0).as_str(), p.name.as_str()),
            (None, "", "Sal")
        );
        let p = parse_line("2-3 tomates");
        assert_eq!(
            (p.qty_for(2.0).as_str(), p.name.as_str()),
            ("2-3", "tomates")
        );
        assert_eq!(parse_line("4 lomos de merluza").name, "merluza");
        assert_eq!(parse_line("1,5 kg de patatas").qty_for(1.0), "1½ kg");
    }

    #[test]
    fn amounts_and_names() {
        assert_eq!(fmt_amount(1.5), "1½");
        assert_eq!(fmt_amount(0.25), "¼");
        assert_eq!(fmt_amount(2.0), "2");
        assert_eq!(fmt_amount(133.33), "133");
        assert_eq!(fmt_amount(0.98), "1");
        assert_eq!(fmt_amount(0.1), "0,1");
        assert_eq!(shop_name("cebolla, picada"), "cebolla");
        assert_eq!(shop_name("perejil al gusto"), "perejil");
        assert!(staple("Sal gorda") && staple("Pimienta") && !staple("Salmón"));
        assert_eq!(mins(75), "1 h 15 min");
        assert_eq!(
            lines(" 1. Pica\n\n- Sofríe\n2) Sirve", 10, 100, true),
            "Pica\nSofríe\nSirve"
        );
    }

    #[test]
    fn reads_themealdb() {
        let v: Value = serde_json::from_str(
            r#"{"idMeal":"52772","strMeal":"Teriyaki Chicken","strCategory":"Chicken","strArea":"Japanese",
            "strInstructions":"STEP 1\r\nPreheat oven.\r\n\r\nstep 2\r\nCook the chicken.","strMealThumb":"https://www.themealdb.com/images/media/meals/x.jpg",
            "strIngredient1":"soy sauce","strMeasure1":"3/4 cup","strIngredient2":"","strMeasure2":" "}"#,
        )
        .unwrap();
        let m = meal(&v).unwrap();
        assert_eq!(
            (m.category, m.ingredients.as_str(), m.steps.as_str()),
            (
                "Aves",
                "3/4 cup soy sauce",
                "Preheat oven.\nCook the chicken."
            )
        );
        assert_eq!(
            (m.source.as_str(), m.image.as_str()),
            (
                "themealdb:52772",
                "https://www.themealdb.com/images/media/meals/x.jpg/medium"
            )
        );
        assert!(meal(&serde_json::json!({"idMeal": "../x"})).is_none());
    }

    /// The whole catalog parses into valid recipes, lands once in its tables, and a deleted one stays deleted.
    #[test]
    fn catalog_seeds_once() {
        let book = catalog();
        assert!(book.len() >= 130, "catalog has {} recipes", book.len());
        let mut names: Vec<_> = book.iter().map(|e| e.source()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), book.len(), "names are unique");
        for e in &book {
            assert!(
                CATS.iter().any(|c| c.0 == e.category),
                "{}: category {}",
                e.name,
                e.category
            );
            assert!(
                e.serves > 0 && e.minutes > 0 && !e.area.is_empty(),
                "{}: header",
                e.name
            );
            assert!(
                e.ingredients.lines().count() >= 1 && e.steps.lines().count() >= 2,
                "{}: body",
                e.name
            );
            assert!(
                !e.ingredients.contains("--") && !e.steps.lines().any(|l| l.starts_with("# ")),
                "{}: split",
                e.name
            );
        }

        let c = Connection::open_in_memory().unwrap();
        prepare(&c, None).unwrap();
        seed_catalog(&c).unwrap();
        assert_eq!(all(&c).unwrap().len(), book.len());
        let id_of = |name: &str| {
            all(&c)
                .unwrap()
                .into_iter()
                .find(|r| r.name == name)
                .unwrap()
                .id
        };
        let tortilla = one(&c, id_of("Tortilla de patatas")).unwrap().unwrap();
        let names: Vec<String> = ingredients_of(&tortilla)
            .map(|l| shop_name(&parse_line(l).name))
            .collect();
        assert_eq!(
            names,
            [
                "huevos",
                "patatas medianas",
                "cebolla",
                "aceite de oliva",
                "Sal"
            ]
        );
        assert!(
            tortilla
                .steps
                .ends_with("> Mejor jugosa por dentro: que tiemble un poco al moverla."),
            "tips survive the tables"
        );

        // Search looks in names and in ingredients, without accents.
        let keys = search_keys(&c).unwrap();
        let hits = |q: &str| keys.iter().filter(|(_, k)| matches(k, q)).map(|(id, _)| *id).collect::<Vec<_>>();
        assert!(hits("limon").contains(&id_of("Arroz con leche")) && hits("limon").contains(&id_of("Limonada casera")));
        assert!(hits("Fabada").contains(&id_of("Fabada asturiana")));
        assert!(hits("huevo patata").contains(&id_of("Tortilla de patatas")), "every word, anywhere");
        assert!(hits("zzz").is_empty());
        assert!(!keys[&id_of("Croquetas de jamón")].contains("bechamel"), "section titles are not ingredients");

        // Deleting a catalog recipe hides it from later seeds; restoring brings it back. Lines go with it.
        c.execute(
            "INSERT INTO recipe_hidden(source) SELECT source FROM recipes WHERE id=?1",
            [tortilla.id],
        )
        .unwrap();
        c.execute("DELETE FROM recipes WHERE id=?1", [tortilla.id])
            .unwrap();
        let orphans: i64 = c
            .query_row(
                "SELECT count(*) FROM recipe_ingredients WHERE recipe_id=?1",
                [tortilla.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(orphans, 0);
        seed_catalog(&c).unwrap();
        assert_eq!(all(&c).unwrap().len(), book.len() - 1);
        c.execute("DELETE FROM recipe_hidden", []).unwrap();
        seed_catalog(&c).unwrap();
        assert_eq!(all(&c).unwrap().len(), book.len());

        // Sections are not things to buy.
        let croquetas = one(&c, id_of("Croquetas de jamón")).unwrap().unwrap();
        assert!(croquetas.ingredients.starts_with("## Para la bechamel"));
        assert!(ingredients_of(&croquetas).all(|l| !l.starts_with("##")));
    }

    /// Recipes an older Nido kept in nido.db move to recipes.db the first time; never twice.
    #[test]
    fn moves_recipes_from_the_core() {
        let dir = std::env::temp_dir().join(format!("nido-recipes-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let core = dir.join("nido.db");
        let _ = std::fs::remove_file(&core);
        {
            let old = Connection::open(&core).unwrap();
            old.execute_batch(include_str!("../schema.sql")).unwrap();
            old.execute("INSERT INTO recipes(id,name,source,photo,search) VALUES(500,'Gachas de mi madre','',1,' gachas ')", []).unwrap();
            save_lines(&old, 500, "80 g de avena", "Calienta.").unwrap();
        }
        let c = Connection::open_in_memory().unwrap();
        prepare(&c, Some(core.to_str().unwrap())).unwrap();
        let r = one(&c, 500).unwrap().unwrap();
        assert_eq!(
            (r.name.as_str(), r.ingredients.as_str(), r.photo),
            ("Gachas de mi madre", "80 g de avena", 0)
        ); // its photo lived in the old binary
        assert_eq!(
            all(&c).unwrap().len(),
            catalog().len() + 1,
            "the catalog fills the rest"
        );
        prepare(&c, Some(core.to_str().unwrap())).unwrap(); // a second start moves nothing again
        assert_eq!(all(&c).unwrap().len(), catalog().len() + 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every photo that comes in (upload or import) ends as a 640x480 WebP, like the tickets.
    #[test]
    fn photos_become_webp() {
        let mut png = Vec::new();
        image::RgbImage::from_pixel(1200, 300, image::Rgb([200, 120, 40]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let webp = to_webp(&png).unwrap();
        assert_eq!((&webp[..4], &webp[8..12]), (&b"RIFF"[..], &b"WEBP"[..]));
        let img = image::load_from_memory(&webp).unwrap();
        assert_eq!((img.width(), img.height()), (640, 480));
        assert!(webp.len() < png.len());
        assert!(to_webp(b"not an image").is_err());
    }

    #[test]
    fn keeps_only_real_photos() {
        let c = Connection::open_in_memory().unwrap();
        prepare(&c, None).unwrap();
        let slug = photo_slugs().next().expect("credits list photos");
        assert!(!store_catalog_photo(&c, slug, b"<html>404</html>").unwrap());
        let mut webp = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
        webp.extend([0u8; 64]);
        assert!(store_catalog_photo(&c, slug, &webp).unwrap());
        assert!(!store_catalog_photo(&c, slug, &webp).unwrap(), "only once");
        assert_eq!(photo_state(&c).unwrap().have, 1);
    }

    /// A recipe page as most sites publish it (JSON-LD inside @graph, HowToSteps, ISO times, entities).
    #[test]
    fn reads_recipe_pages() {
        let html = r#"<html><head><script type="application/ld+json">{"@context":"https://schema.org","@graph":[
            {"@type":"WebPage","name":"x"},
            {"@type":["Recipe"],"name":"Bizcocho de lim&oacute;n &amp; yogur","recipeCategory":"Postres","recipeCuisine":"Española",
             "recipeYield":["8","8 raciones"],"prepTime":"PT15M","cookTime":"PT40M","image":[{"@type":"ImageObject","url":"/img/bizcocho.jpg"}],
             "description":"Esponjoso.","recipeIngredient":["3 huevos","200 g de harina","1 yogur <b>natural</b>"],
             "recipeInstructions":[{"@type":"HowToSection","name":"La masa","itemListElement":[{"@type":"HowToStep","text":"Bate los huevos."},{"@type":"HowToStep","text":"A&ntilde;ade la harina."}]},
               {"@type":"HowToStep","text":"Hornea 40 minutos."}]}]}</script></head></html>"#;
        let r = from_page("https://www.recetas.example/bizcocho", html).unwrap();
        assert_eq!(r.name, "Bizcocho de limón & yogur");
        assert_eq!(
            (r.category, r.area.as_str(), r.serves, r.minutes),
            ("Postres", "Española", 8, Some(55))
        );
        assert_eq!(r.ingredients, "3 huevos\n200 g de harina\n1 yogur natural");
        assert_eq!(
            r.steps,
            "## La masa\nBate los huevos.\nAñade la harina.\nHornea 40 minutos."
        );
        assert_eq!(r.image, "https://www.recetas.example/img/bizcocho.jpg");
        assert!(from_page("https://x.example/", "<html>sin receta</html>").is_err());
        assert_eq!(iso_minutes("PT1H30M"), Some(90));
        assert_eq!(iso_minutes("P0DT2H"), Some(120));
        assert_eq!(
            plain("Caf&eacute; &#241; &#xF1; &lt;b&gt; &rarr;"),
            "Café ñ ñ <b> &rarr;"
        ); // unknown entities stay as they are
        assert_eq!(guess_cat("Plato principal pollo al horno"), "Aves");
        assert_eq!(guess_cat("algo raro"), "Otros");
    }

    #[test]
    fn only_public_pages() {
        assert!(public_url("https://www.directoalpaladar.com/recetas/tortilla").is_some());
        assert!(public_url("http://cookpad.com:8080/es/recetas/1").is_some());
        for bad in [
            "http://localhost:8080/",
            "http://192.168.1.1/",
            "http://10.0.0.5/x",
            "http://172.20.1.1/",
            "http://127.0.0.1/",
            "http://nido.local/",
            "http://router/",
            "http://[::1]/",
            "file:///etc/passwd",
            "ftp://x.com/",
            "https://x.com/a b",
        ] {
            assert!(public_url(bad).is_none(), "{bad}");
        }
    }
}
