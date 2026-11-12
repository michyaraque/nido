use super::*;

pub const CSS: &str = concat!(
    include_str!("../assets/css/tokens.css"),
    include_str!("../assets/css/base.css"),
    include_str!("../assets/css/layout.css"),
    include_str!("../assets/css/components.css"),
    include_str!("../assets/css/pages.css"),
);
pub const JS: &str = include_str!("../assets/js/app.js");
const SW: &str = include_str!("../assets/js/sw.js");
const ICON: &[u8] = include_bytes!("icon.png");
const TOUCH_ICON: &[u8] = include_bytes!("apple-touch-icon.png");
const INTER: &[u8] = include_bytes!("../assets/fonts/inter.woff2");
const BRICOLAGE: &[u8] = include_bytes!("../assets/fonts/bricolage.woff2");
const MANIFEST: &str = r##"{"name":"Nido","short_name":"Nido","description":"Lo de casa, en orden: cuentas, gastos, compra, tareas y tickets","lang":"es","start_url":"/","display":"standalone","background_color":"#0c0c10","theme_color":"#f7f7f8","icons":[{"src":"/icon.png","sizes":"512x512","type":"image/png","purpose":"any"},{"src":"/apple-touch-icon.png","sizes":"180x180","type":"image/png"}]}"##;

fn cached(ct: &'static str) -> [(header::HeaderName, &'static str); 2] {
    [(header::CONTENT_TYPE, ct), (header::CACHE_CONTROL, "public, max-age=31536000")]
}

pub fn routes() -> Router<Db> {
    Router::new()
        .route("/style.css", get(|| async { (cached("text/css; charset=utf-8"), CSS) }))
        .route("/app.js", get(|| async { (cached("text/javascript; charset=utf-8"), JS) }))
        .route("/icon.png", get(|| async { (cached("image/png"), ICON) }))
        .route("/apple-touch-icon.png", get(|| async { (cached("image/png"), TOUCH_ICON) }))
        .route("/fonts/inter.woff2", get(|| async { (cached("font/woff2"), INTER) }))
        .route("/fonts/bricolage.woff2", get(|| async { (cached("font/woff2"), BRICOLAGE) }))
        .route("/manifest.webmanifest", get(|| async { (cached("application/manifest+json"), MANIFEST) }))
        .route("/logos/{file}", get(logo))
        // The service worker lives at the root (its scope) and is never cached, so a new version reaches every phone.
        .route("/sw.js", get(|| async {
            static S: std::sync::OnceLock<String> = std::sync::OnceLock::new();
            let js = S.get_or_init(|| SW.replace("__VERSION__", assets_version()));
            ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], js.as_str())
        }))
        .route("/ping", get(|| async { ([(header::CACHE_CONTROL, "no-store")], StatusCode::NO_CONTENT) }))
}

async fn logo(Path(file): Path<String>) -> Response {
    match merchants::logo(file.trim_end_matches(".webp")) {
        Some(b) => ([(header::CONTENT_TYPE, merchants::mime(b)), (header::CACHE_CONTROL, "public, max-age=604800")], b).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
