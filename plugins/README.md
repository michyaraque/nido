# Extras de Nido

Un **extra** es una función opcional de Nido que vive en **su propio programa**. El núcleo de Nido no lleva nada de él: ni código, ni datos, ni estilos. Nido pesa lo mismo en la Raspberry hasta que alguien de la casa instala un extra desde **Extras** (en el menú).

```
 navegador ──► Nido (núcleo, :8080) ──► nido-<nombre> (127.0.0.1:puerto aleatorio)
                │  sesión, menú,             │  su página, sus formularios,
                │  marketplace, servicios    │  su base de datos: extras/<id>.db
```

- **Instalar** descarga el binario del extra de la release de GitHub a `/var/lib/nido/plugins/`.
- **Activar** lo arranca. Nido le da un puerto y un secreto, y a partir de ahí le pasa lo que le toca.
- **Desactivar** lo para. **Desinstalar** borra el binario; su base de datos se queda por si vuelve.

## Nombres

| | Qué es | Ejemplo |
|---|---|---|
| `<id>` | identificador: su pestaña (`/?tab=<id>`), su prefijo de rutas (`/x/<id>/`), su base de datos (`extras/<id>.db`) | `ejemplo` |
| `nido-<nombre>` | el programa y el crate; en la release, `nido-<nombre>-<arch>` (`aarch64`, `armv7`, `armv6`) | `nido-ejemplo` |
| `plugins/<carpeta>/` | su código en este repositorio | `plugins/ejemplo/` |

## Estructura de un extra

```
plugins/<carpeta>/
  Cargo.toml       crate del workspace, name = "nido-<nombre>"
  src/main.rs      el protocolo con Nido (abajo) y sus rutas
  schema.sql       sus tablas, en su propia base de datos        (opcional)
  style.css        su CSS, dentro de su página                   (opcional)
  app.js           su JavaScript, solo en su página              (opcional)
  build.rs         p. ej. para copiar iconos de assets/icons     (opcional)
```

Solo `Cargo.toml` y el programa son obligatorios. El resto depende de lo que haga.

## El protocolo

Es HTTP normal en `127.0.0.1`. Un extra es cualquier programa que cumpla esto; los de este repositorio están hechos en Rust con axum, como Nido, para compartir el workspace y las dependencias.

### Al arrancar

Nido lo ejecuta con estas variables de entorno:

| Variable | Qué es |
|---|---|
| `NIDO_PLUGIN_ADDR` | dónde escuchar, p. ej. `127.0.0.1:41873`. Solo loopback. |
| `NIDO_PLUGIN_KEY` | secreto de esta ejecución. **Rechaza (403) toda petición que no lo traiga.** |
| `NIDO_PLUGIN_DB` | ruta de su base de datos: `/var/lib/nido/extras/<id>.db`. Va en las copias de seguridad de Nido. |
| `NIDO_CORE_DB` | la base de datos de Nido, solo para leer en migraciones puntuales. **Nunca escribas en ella.** |

Además, `nido-<nombre> --version` debe imprimir su versión y salir con 0. Nido lo usa para comprobar que lo descargado funciona en esa Raspberry.

### Cada petición de Nido

Todas llevan estas cabeceras. Nido ya ha comprobado la sesión; el extra se fía de ellas solo si `X-Nido-Key` coincide.

| Cabecera | Ejemplo |
|---|---|
| `X-Nido-Key` | el secreto de `NIDO_PLUGIN_KEY` |
| `X-Nido-User` | `3`: el id de la persona en Nido |
| `X-Nido-Name` | `Ana%20Mar%C3%ADa`: su nombre, con *percent-encoding* |
| `X-Nido-People` | `1=Ana=%232f6bff=1731600000,2=Leo=%23e1166b=0`: todos los de casa, `id=nombre=color=foto` separados por comas (nombre y color con *percent-encoding*), en el orden de Nido. `foto` es la versión de su foto de perfil (0 = sin foto): la página la muestra con `<img src="/avatars/<id>?v=<foto>">`, que sirve Nido. Para invitar a alguien o pintarlo con su color y su cara. |

### Lo que el extra responde

| Ruta | Para qué |
|---|---|
| `GET /manifest` | JSON con sus avisos: `{"id":"<id>","version":"v0.1.0","flashes":{"<código>":"<texto>"}}` |
| `GET /page?<query>` | El HTML de la zona de contenido de `/?tab=<id>&…`. Recibe la query entera: `month`, `tab` y sus propios parámetros. Solo el contenido: Nido pone el menú, la cabecera, el tema y los avisos. |
| `<método> /<ruta>` | Lo que el navegador pide a `/x/<id>/<ruta>` llega aquí como `/<ruta>`: formularios, imágenes, su script. Nido devuelve al navegador el estado, `Content-Type`, `Location` y `Cache-Control`. |
| WebSocket a `/<ruta>` | Para tiempo real. El navegador abre `ws(s)://<host>/x/<id>/<ruta>`; Nido comprueba la sesión, pasa el *handshake* al extra con las cabeceras `X-Nido-*` y, tras el `101`, deja pasar los bytes en los dos sentidos sin leerlos. El formato de los mensajes es cosa del extra (Juegos usa paquetes binarios: mira `games/src/net.rs`). |
| `GET /notices` *(opcional)* | Avisos para quien pregunta (`X-Nido-User`), como JSON `[{"text":"Ana te invita…","href":"/?tab=<id>&…","action":"Jugar"}]`. Nido los pide cada 3 s mientras la app está abierta y a la vista y los muestra arriba; lo que devuelvas cuenta como visto. `href` debe empezar por `/?`. |
| `GET /notices/late` *(opcional)* | Los avisos que nadie ha visto a tiempo, con su destinatario: `[{"user":2,"text":"…","href":"/?…"}]`. Nido lo pide cada 30 s y los manda como notificación del móvil o, si esa persona no las tiene activadas, por Telegram. Devuelve cada uno una sola vez. |

### Reglas de la página

- **Enlaces** a sus vistas: `/?month=<mes>&tab=<id>&…`, siempre con el mes que recibió, para que el resto de Nido lo conserve.
- **Formularios** a `/x/<id>/<ruta>` con `method="post"`, con `month` y `tab` ocultos, como los de Nido. Para subir archivos, `enctype="multipart/form-data"` (hasta 25 MB).
- **Después de un POST**, redirige (303) a una página de Nido con `&ok=<código>`. Nido muestra el texto de ese código, tomado de su `/manifest`. Un 4xx con texto se enseña como aviso de error.
- **Estilos.** Usa las clases de Nido (`card`, `card-head`, `btn primary|outline|ghost|danger sm`, `pill ok|warn|bad`, `field`, `input`, `chips`, `modal`/`dialog`, `empty`, `hint`…) y sus variables (`--ink`, `--mute`, `--line`, `--surface`, `--accent`…). Así funciona en claro, en oscuro y con los temas de la casa. Lo propio del extra va con un prefijo de clase propio, en un `<style>` dentro de su página.
- **JavaScript** (opcional). En la página pon `<span hidden data-plugin="<id>" data-plugin-js="/x/<id>/<ruta del script>?v=<hash>"></span>`. Nido carga el script una vez; las páginas cambian sin recargarse, así que cada vez que se vuelve a ella llama a `window.nidoPlugins["<id>"](document)`. El script debe registrarse ahí y aguantar que lo llamen varias veces. Sin JavaScript, la página tiene que seguir funcionando.
- **Iconos.** Lucide, los mismos de Nido (`assets/icons`), con el marcado `<svg class="i" …>`. Un `build.rs` puede incrustarlos desde esa carpeta.

### Servicios de Nido para los extras

| Ruta de Nido | Qué hace |
|---|---|
| `POST /shopping/many` | Añade a la lista de la compra cada campo `item` (puede repetirse) y vuelve a `next`, que debe empezar por `/?`. Es un formulario normal del navegador: el extra no necesita llamar a Nido. |

### Seguridad

- Escucha solo en `127.0.0.1` y exige `X-Nido-Key`: nadie más de la red puede hablar con el extra.
- Escapa todo lo que pinte (maud lo hace por defecto).
- Si descarga algo de una URL que da la persona, que sea una web pública: nunca `localhost`, IPs privadas ni nombres `.local`.

## Crear un extra paso a paso

El ejemplo es un extra **Ejemplo**: `id` `ejemplo`, programa `nido-ejemplo`, carpeta `plugins/ejemplo/`.

**1. El crate.** `plugins/ejemplo/Cargo.toml`:

```toml
[package]
name = "nido-ejemplo"
version = "0.1.0"
edition = "2021"

[dependencies]
axum = "0.8"
tokio = { version = "1", features = ["rt", "net", "macros"] }
rusqlite = { version = "0.37", features = ["bundled"] }
maud = "0.27"
serde = { version = "1", features = ["derive"] }
```

Añádelo al workspace en el `Cargo.toml` de la raíz: `members = [".", …, "plugins/ejemplo"]`.

**2. El programa mínimo.** `plugins/ejemplo/src/main.rs`:

```rust
use axum::{extract::Request, http::StatusCode, middleware::{self, Next}, response::{Html, IntoResponse}, routing::get, Router};
use maud::html;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        return println!("v{}", env!("CARGO_PKG_VERSION"));
    }
    let key = std::env::var("NIDO_PLUGIN_KEY").expect("lo arranca Nido");
    let addr = std::env::var("NIDO_PLUGIN_ADDR").unwrap_or("127.0.0.1:8182".into());
    let app = Router::new()
        .route("/manifest", get(|| async { r#"{"id":"ejemplo","flashes":{"saved":"Guardado"}}"# }))
        .route("/page", get(|| async {
            Html(html! { div.page-head { div { h1 { "Ejemplo" } p { "Un extra de Nido" } } }
                         section.card { div.empty { h3 { "Aún no hay nada" } } } }.into_string())
        }))
        .layer(middleware::from_fn(move |req: Request, next: Next| {
            let ok = req.headers().get("x-nido-key").is_some_and(|v| v == key.as_str());
            async move { if ok { next.run(req).await } else { StatusCode::FORBIDDEN.into_response() } }
        }));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

**3. En el catálogo de Nido.** Añade su ficha a `ALL` en `src/plugins/mod.rs`. Es lo único que el núcleo sabe de él: lo que enseña el marketplace.

```rust
Known {
    id: "ejemplo",
    bin: "nido-ejemplo",
    name: "Ejemplo",
    icon: "sparkles",                  // un icono de assets/icons
    tagline: "Una línea bajo el nombre",
    blurb: "Qué hace, en una o dos frases.",
    features: &["Lo primero que ofrece", "Lo segundo"],
    accent: "#2f6bff",                 // su color en la tarjeta
    size: "≈ 3 MB",                    // lo que ocupa la descarga
    fab: None,                         // o Some(("/?tab=ejemplo&new=1", "Nuevo")): el + del móvil en su página
},
```

**4. La release.** No hay que tocar nada: el workflow compila el workspace entero y publica cada `plugins/*/` como `nido-<nombre>-<arch>`. Al publicar una versión, cada Raspberry puede instalarlo desde Extras.

**5. Probarlo en tu ordenador.**

```sh
cargo build --workspace
NIDO_PLUGINS=target/debug cargo run      # Nido busca los extras en esa carpeta
```

Entra, ve a **Extras → Ejemplo → Activar** y ábrelo desde el menú.

**6. En tu Raspberry sin pasar por GitHub.** Compila para la placa y copia el binario a `~/nido-install/plugins/`. `sudo nido-deploy` lo instala en `/var/lib/nido/plugins/` junto con Nido.

## Buenas prácticas

- **Ligero.** Que arranque en milisegundos y gaste pocos MB en reposo: es una Raspberry.
- **Sus datos, en su base de datos.** Nunca escribas en la de Nido.
- **Que no estorbe.** Lo pesado (fotos, catálogos grandes) se descarga aparte, y solo si la casa lo pide.
- **Tests.** `cargo test --workspace` prueba Nido y todos los extras.

## Extras en esta carpeta

| Carpeta | Extra |
|---|---|
| [`recipes/`](recipes/) | Recetas: recetario con importación desde webs, editor por bloques y paso a la lista de la compra. Es el ejemplo completo de todo lo de arriba. |
| [`games/`](games/) | Juegos: siete juegos de mesa entre los de casa o contra un robot, con ranking. Ejemplo de tiempo real: WebSocket con paquetes binarios (`juegos/src/net.rs`). |
