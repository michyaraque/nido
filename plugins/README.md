# Plugins de Nido

Un plugin es una función opcional que se ejecuta como un programa independiente. El núcleo de Nido no incluye su código, sus datos ni sus estilos: solo conoce su ficha en el catálogo. Se instala desde Plugins, en el menú.

```
 navegador ──► Nido (núcleo, :8080) ──► nido-<nombre> (127.0.0.1:puerto aleatorio)
                │  sesión, menú,             │  página, formularios,
                │  catálogo, servicios       │  base de datos: plugins/<id>.db
```

- Instalar descarga el binario a `/var/lib/nido/plugins/`: de la última release si esa versión del plugin funciona con el Nido instalado, y si no, de la release de la versión de Nido.
- Cada plugin tiene su propia versión (la de su `Cargo.toml`) y declara la versión mínima de Nido que necesita. La release publica ambas en `plugins.json`.
- Actualizar Nido solo sustituye su binario. Cuando hay una versión nueva de un plugin que funciona con ese Nido, Plugins muestra «v0.1.0 → v0.2.0» y la casa decide cuándo pulsar Actualizar.
- Activar lo arranca. Nido le asigna un puerto y un secreto, y le reenvía las peticiones que le corresponden.
- Desactivar lo detiene. Desinstalar borra el binario y conserva su base de datos.

## Nombres

| | Descripción | Ejemplo |
|---|---|---|
| `<id>` | Identificador: pestaña (`/?tab=<id>`), prefijo de rutas (`/plugins/<id>/`) y base de datos (`plugins/data/<id>.db`) | `ejemplo` |
| `nido-<nombre>` | Programa y crate. En la release, `nido-<nombre>-<arch>` (`aarch64`, `armv7`, `armv6`) | `nido-ejemplo` |
| `plugins/<carpeta>/` | Código en este repositorio | `plugins/ejemplo/` |

## Estructura

```
plugins/<carpeta>/
  Cargo.toml       crate del workspace, name = "nido-<nombre>"
  src/main.rs      protocolo con Nido y rutas
  schema.sql       tablas de su base de datos                    (opcional)
  style.css        estilos de su página, minificados por build.rs (opcional)
  app.js           JavaScript de su página                        (opcional)
  build.rs         iconos de assets/icons y CSS minificado        (opcional)
```

Solo `Cargo.toml` y el programa son obligatorios.

El espacio `/plugins/<id>/` es del plugin. `POST /plugins/<id>`, sin nada detrás, lo usa Nido para instalarlo, activarlo y desinstalarlo desde la pantalla de Plugins.

Si trabajas con un asistente de IA, [AGENTS.md](AGENTS.md) resume las reglas y convenciones para crear plugins.

## Protocolo

La comunicación es HTTP sobre `127.0.0.1`. Cualquier programa que cumpla lo siguiente es un plugin válido; los de este repositorio usan Rust y axum para compartir workspace y dependencias con Nido.

### Arranque

Nido ejecuta el programa con estas variables de entorno:

| Variable | Descripción |
|---|---|
| `NIDO_PLUGIN_ADDR` | Dirección de escucha, por ejemplo `127.0.0.1:41873`. Solo loopback. |
| `NIDO_PLUGIN_KEY` | Secreto de esta ejecución. Toda petición sin él debe recibir `403`. |
| `NIDO_PLUGIN_DB` | Ruta de su base de datos: `/var/lib/nido/plugins/data/<id>.db`. Se incluye en las copias de seguridad. |
| `NIDO_CORE_DB` | Base de datos de Nido, solo para lectura en migraciones puntuales. No debe escribirse. |

`nido-<nombre> --version` debe imprimir la versión y salir con código 0. Nido lo usa para comprobar que el binario descargado funciona en esa placa.

### Cabeceras de cada petición

Nido valida la sesión antes de reenviar. El plugin solo debe confiar en estas cabeceras si `X-Nido-Key` coincide.

| Cabecera | Ejemplo |
|---|---|
| `X-Nido-Key` | El valor de `NIDO_PLUGIN_KEY`. |
| `X-Nido-User` | `3`: id de la persona en Nido. |
| `X-Nido-Name` | `Ana%20Mar%C3%ADa`: nombre con percent-encoding. |
| `X-Nido-People` | `1=Ana=%232f6bff=1731600000,2=Leo=%23e1166b=0`: todas las personas de la casa como `id=nombre=color=foto`, separadas por comas y en el orden de Nido. Nombre y color van con percent-encoding. `foto` es la versión de la foto de perfil (0 si no tiene); la imagen está en `/avatars/<id>?v=<foto>`. |

### Rutas que implementa el plugin

| Ruta | Descripción |
|---|---|
| `GET /manifest` | JSON con sus mensajes: `{"id":"<id>","version":"v0.1.0","flashes":{"<código>":"<texto>"}}`. |
| `GET /page?<query>` | HTML del área de contenido de `/?tab=<id>&…`. Recibe la query completa (`month`, `tab` y sus parámetros). Nido añade el menú, la cabecera, el tema y los mensajes. |
| `<método> /<ruta>` | Las peticiones a `/plugins/<id>/<ruta>` llegan como `/<ruta>`: formularios, imágenes, scripts. Nido devuelve al navegador el estado y las cabeceras `Content-Type`, `Location` y `Cache-Control`. |
| WebSocket en `/<ruta>` | El navegador abre `ws(s)://<host>/plugins/<id>/<ruta>`. Nido valida la sesión, reenvía el handshake con las cabeceras `X-Nido-*` y, tras el `101`, pasa los bytes sin interpretarlos. El formato de los mensajes lo define el plugin (Juegos usa paquetes binarios, ver `games/src/net.rs`). |
| `GET /notices` (opcional) | Avisos para la persona de `X-Nido-User`: `[{"text":"Ana te invita…","href":"/?tab=<id>&…","action":"Jugar"}]`. Nido lo consulta cada 3 s con la app en primer plano y los muestra en la parte superior. Lo devuelto se considera visto. `href` debe empezar por `/?`. |
| `GET /notices/late` (opcional) | Avisos no vistos a tiempo, con destinatario: `[{"user":2,"text":"…","href":"/?…"}]`. Nido lo consulta cada 30 s y los envía como notificación push y, si ningún móvil de esa persona confirma haberla mostrado en 3 minutos, también por Telegram. Cada aviso debe devolverse una sola vez. |

### Reglas de la página

- Enlaces a sus vistas: `/?month=<mes>&tab=<id>&…`, conservando el mes recibido.
- Formularios a `/plugins/<id>/<ruta>` con `method="post"`. Para subir archivos, `enctype="multipart/form-data"` (máximo 25 MB).
- Tras un POST, redirigir (303) a una página de Nido con `&ok=<código>`; Nido muestra el texto de ese código según `/manifest`. Una respuesta 4xx con texto se muestra como error.
- Estilos: usar las clases de Nido (`card`, `card-head`, `btn primary|outline|ghost|danger sm`, `pill ok|warn|bad`, `field`, `input`, `modal`/`dialog`, `empty`, `hint`…) y sus variables (`--ink`, `--mute`, `--line`, `--surface`, `--accent`…) para que funcione con los temas claro y oscuro. Las clases propias llevan un prefijo del plugin y van en un `<style>` dentro de su página.
- Ventanas modales: `div.modal#<id>` abierta con un enlace a `#<id>` (CSS `:target`) y cerrada con `href="#_"`, como en el núcleo.
- Actualización periódica: un elemento con `data-refresh="<segundos>"` hace que Nido recargue la vista sin perder el desplazamiento.
- JavaScript (opcional): incluir `<span hidden data-plugin="<id>" data-plugin-js="/plugins/<id>/<script>?v=<hash>"></span>`. Nido carga el script una vez y, en cada visita a la vista, llama a `window.nidoPlugins["<id>"](document)`, por lo que la función debe tolerar varias llamadas. La página debe funcionar también sin JavaScript.
- Iconos: el set Lucide de `assets/icons`, con el marcado `<svg class="i" …>`. Un `build.rs` puede incrustarlos.

### Servicios del núcleo

| Ruta | Descripción |
|---|---|
| `POST /shopping/many` | Añade a la lista de la compra cada campo `item` (repetible) y redirige a `next`, que debe empezar por `/?`. Es un formulario del navegador; el plugin no necesita llamar a Nido. |

### Seguridad

- Escuchar solo en `127.0.0.1` y exigir `X-Nido-Key` en todas las peticiones.
- Escapar todo el contenido generado (maud lo hace por defecto).
- Al descargar desde una URL proporcionada por el usuario, rechazar `localhost`, IPs privadas y nombres `.local`.

## Crear un plugin

Ejemplo con `id` `ejemplo`, programa `nido-ejemplo` y carpeta `plugins/ejemplo/`.

### 1. Crate

`plugins/ejemplo/Cargo.toml`:

```toml
[package]
name = "nido-ejemplo"
version = "0.1.0"
edition = "2021"

[package.metadata.nido]
min = "0.18.0"          # versión mínima de Nido

[dependencies]
axum = "0.8"
tokio = { version = "1", features = ["rt", "net", "macros"] }
rusqlite = { version = "0.37", features = ["bundled"] }
maud = "0.27"
serde = { version = "1", features = ["derive"] }
```

Añádelo a `members` en el `Cargo.toml` raíz.

### 2. Programa mínimo

`plugins/ejemplo/src/main.rs`:

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
            Html(html! { div.page-head { div { h1 { "Ejemplo" } p { "Un plugin de Nido" } } }
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

### 3. Ficha en el catálogo

Añade la entrada a `ALL` en `src/plugins/mod.rs`. Es lo único del plugin que conoce el núcleo:

```rust
Known {
    id: "ejemplo",
    bin: "nido-ejemplo",
    name: "Ejemplo",
    icon: "sparkles",                  // icono de assets/icons
    tagline: "Una línea bajo el nombre",
    blurb: "Qué hace, en una o dos frases.",
    features: &["Primera función", "Segunda función"],
    accent: "#2f6bff",                 // color de la tarjeta
    size: "≈ 3 MB",                    // tamaño aproximado de la descarga
    fab: None,                         // o Some(("/?tab=ejemplo&new=1", "Nuevo")): botón + en el móvil
},
```

### 4. Release

El workflow compila todo el workspace, publica cada `plugins/*/` como `nido-<nombre>-<arch>` y añade su versión y su mínima de Nido a `plugins.json`. Para publicar una versión nueva de un plugin, sube `version` en su `Cargo.toml` (y `min` si necesita algo nuevo del núcleo) y publica una release de Nido.

### 5. Pruebas en local

```sh
cargo build --workspace
NIDO_PLUGINS=target/debug cargo run
```

Activa el plugin en Plugins → Ejemplo y ábrelo desde el menú.

### 6. Despliegue manual en el equipo

Compila para la placa, copia el binario a `~/nido-install/plugins/` y ejecuta `sudo nido-deploy`, que lo instala en `/var/lib/nido/plugins/` junto con Nido.

## Recomendaciones

- Arranque rápido y poco consumo en reposo.
- Todos los datos en su propia base de datos; nunca en la de Nido.
- Contenido pesado (fotos, catálogos grandes) como descarga opcional.
- `cargo test --workspace` ejecuta los tests de Nido y de todos los plugins.

## Plugins incluidos

| Carpeta | Plugin |
|---|---|
| [`recipes/`](recipes/) | Recetas: recetario con importación desde webs, editor por bloques y envío de ingredientes a la lista de la compra. Ejemplo completo de formularios, subida de archivos y servicios del núcleo. |
| [`games/`](games/) | Juegos: nueve juegos de mesa entre personas de la casa o contra el ordenador, con ranking. Ejemplo de tiempo real con WebSocket y paquetes binarios (`games/src/net.rs`). |
| [`devices/`](devices/) | Domótica: control local de aparatos Tuya y mandos infrarrojos Broadlink, consumo eléctrico, escenas y avisos. Ejemplo de tareas en segundo plano, protocolos de red propios y `/notices`. |
| [`pets/`](pets/) | Mascotas: agenda de hoy (comidas, tomas, tareas), peso, salud, paseos, cartilla, citas del veterinario, diario de la casa y tareas que se repiten. Ejemplo de recordatorios en segundo plano con `/notices` y `/notices/late`, y de fotos subidas por la casa. |
