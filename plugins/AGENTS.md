# Guide for AI agents: building Nido plugins

Read this before creating or changing anything under `plugins/`. The full protocol is in [README.md](README.md); this file is the short, prescriptive version plus the mistakes already made once. The UI and code standards that apply to the whole project (mobile first, touch targets, dialogs, colour contrast, module size) are in the root [AGENTS.md](../AGENTS.md): read both.

## What an plugin is

- A separate program (`nido-<name>`) that Nido starts and talks to over HTTP on `127.0.0.1`. The core only holds its catalog entry in `src/plugins/mod.rs`.
- Rust 2021, `axum` 0.8, `tokio` current-thread runtime, `maud` for HTML, `rusqlite` (bundled) for its own database.
- Server-rendered pages. Every feature must work without JavaScript.
- Target hardware is any modest device: little RAM (down to 512 MB), slow storage, a few CPU cores. Idle RAM and startup time matter.

Reference implementations: `recipes/` (forms, uploads, core services), `games/` (WebSocket, binary packets), `devices/` (background tasks, LAN protocols, notices, modals, scenes), `pets/` (the UI reference: mobile-first layout, folding lists, carousels, coloured cards with AA contrast, two-step deletes, reminders persisted with `sent` keys, view split into modules, a list of sections with live summaries instead of tabs, and one-tap logs with undo: `quick=1` forms, `logged` and `take_back` in `act.rs`).

## Files to touch for a new plugin

1. `plugins/<folder>/Cargo.toml` with `name = "nido-<name>"`, its own `version` and `[package.metadata.nido] min` (see [Versions](#versions)).
2. Root `Cargo.toml`: add the folder to `members`.
3. `plugins/<folder>/build.rs`: copy it from `devices/build.rs`. It embeds Lucide icons from `assets/icons` and minifies `style.css` with `tools/css.rs` into `OUT_DIR`.
4. `plugins/<folder>/src/main.rs`: `--version`, env vars, key guard, `/manifest`, `/page`, routes.
5. `plugins/<folder>/schema.sql`: `CREATE TABLE IF NOT EXISTS …` only. It runs on every start.
6. `src/plugins/mod.rs`: one `Known { … }` entry in `ALL`.
7. `plugins/README.md`: one row in "Plugins incluidos". Root `README.md`: one row in "Plugins".

## Protocol checklist

- Reject with `403` any request whose `X-Nido-Key` differs from `NIDO_PLUGIN_KEY`.
- Listen only on `NIDO_PLUGIN_ADDR`. Open `NIDO_PLUGIN_DB`; never write to `NIDO_CORE_DB`.
- `nido-<name> --version` prints `v<version>` and exits 0.
- `GET /manifest` returns `{"id","version","flashes":{code:text}}`. Every `&ok=<code>` you redirect with must exist there.
- `GET /page` returns only the content area. Read the user from `X-Nido-User`, and the household from `X-Nido-People` (percent-encoded).
- Browser paths are `/plugins/<id>/<path>`; they reach the plugin as `/<path>`. `POST /plugins/<id>` with nothing after it is Nido's (install, toggle, uninstall).
- Optional `GET /notices` and `GET /notices/late`: see [Notifications and messages](#notifications-and-messages).

## Notifications and messages

An plugin has four ways to tell people something. Pick by urgency and audience.

| Need | Mechanism |
|---|---|
| Confirm an action the person just did | `303` redirect with `&ok=<code>`; Nido shows the toast text from `/manifest` `flashes`. |
| Report a failed action | Redirect with `&err=<message>` and render `div.alert.bad` on the target view, or return `4xx` with a plain-text body. |
| Tell someone something while Nido is open | `GET /notices` (top banner). |
| Reach someone who is not looking | `GET /notices/late` (push notification, or Telegram). |

### `GET /notices`

- Nido calls it about every 3 s for the person whose app is open and visible, with `X-Nido-Key`, `X-Nido-User` and `X-Nido-Name`. `X-Nido-People` is not sent here.
- Response: `200` with a JSON array of `{"text": "...", "href": "/?tab=<id>&...", "action": "Ver"}`. `action` is the button label (default "Abrir").
- `href` must start with `/?` and contain no CR, LF or `"`; other items are dropped.
- Only the first item is shown, with "(y N más)" when there are more. It disappears after 15 s or on click.
- Whatever you return counts as seen: remove that person from the notice's pending list.
- Return `[]` when there is nothing. A `404` makes Nido stop asking this plugin until it restarts, so an plugin that has notices must always answer `200`.

### `GET /notices/late`

- Nido calls it every 30 s with only `X-Nido-Key` (no user headers).
- Response: `[{"user": 2, "text": "...", "href": "/?tab=<id>&...", "action": "Ver"}]`, one entry per recipient.
- Nido sends each entry as a Web Push notification titled with the plugin's catalog name. If no phone of that person confirms showing it within 3 minutes (no subscription, refused by the push service, phone off or away from home), it also goes by Telegram with a link (`action` is the link text, default "Abrir en Nido"). Without Telegram linked, push is the only channel.
- Return each entry once, then forget it.

### Implementing them

- Keep pending notices as `{text, href, pending: Vec<user_id>, created: Instant}`. `/notices` removes the asking user from `pending`; `/notices/late` drains `pending` for notices older than ~30 s; drop notices with an empty list. See `notify`, `notices` and `notices_late` in `devices/src/main.rs`.
- To notify the whole household you need the ids: remember them from `X-Nido-People` on `/page` requests, since background tasks and `/notices/late` do not receive that header.
- In-memory is fine for short-lived notices (an invitation, "la lavadora ha terminado"). Persist them in the plugin's database only if losing them on restart matters.
- Do not notify for things the person just did themselves; use a flash for that.
- Keep `text` short, in Spanish, with no Markdown: it is shown as a banner, a phone notification or a Telegram message.

## Other core integrations

- Month: Nido passes `month` (`YYYY-MM`) in the query of `/page`. Keep it in links (`/?month=<m>&tab=<id>&…`) if your views depend on it.
- People: build the household from `X-Nido-People` (`id=name=color=photo`, percent-encoded). Show faces with `<img src="/avatars/<id>?v=<photo>">` (photo `0` means none: draw the initial with the person's colour).
- Shopping list: a plain browser form `POST /shopping/many` with repeated `item` fields and a `next` URL starting with `/?`.
- Mobile `+` button: set `fab: Some((href, label))` in the catalog entry to give the plugin's page a floating action on phones.
- Backups: the plugin's database file is included in Nido's encrypted backups automatically. Keep everything the plugin needs in that file (or regenerable).
- Stored data: uninstalling asks whether to keep the plugin's data. The Plugins page shows how much it keeps and can erase it later; erasing deletes `<id>.db`, `-wal` and `-shm` and nothing else. Anything kept outside that file would be left behind.
- Themes: never hard-code light colours; use the tokens so dark mode and house themes work.
- Real time: for live updates beyond `data-refresh`, open a WebSocket at `/plugins/<id>/<path>`; Nido proxies it after checking the session (see `games/src/net.rs`).
- Static assets: serve them from the plugin with a long `Cache-Control` and a content hash in the URL (`?v=<hash>`), as `recipes` does for its script.

## Rust rules

- One `Arc<Mutex<Connection>>`. Call `busy_timeout(5 s)` before running the schema: Nido can start two copies at once and the loser must not panic on a locked database.
- Schema changes on existing tables: `ALTER TABLE … ADD COLUMN …` at startup, ignoring the error. Also put the column in `schema.sql` for fresh installs.
- Never hold a `MutexGuard` across `.await`. Lock, copy what you need, drop, then await. axum handlers must be `Send`.
- Background loops: run inside a `LocalSet` with `spawn_local`. Blocking work (curl, std sockets, long device I/O) goes in `spawn_blocking` or `std::thread`.
- Outbound HTTPS: call `curl` with `std::process::Command`, as the core does. Do not add a TLS stack for a few requests.
- Prefer crates already in the workspace (`aes`, `aes-gcm`, `hmac`, `sha2`, `base64`, `getrandom`, `serde_json`). Add a dependency only when a few lines cannot do it.
- Errors shown to people are Spanish, plain, and say what to do. Keep the technical cause in logs or behind "Opciones avanzadas". Map driver errors through one function (see `plain_err` in `devices`).

## UI rules

- All visible text is Spanish. Code, identifiers and comments are English.
- No jargon on the main path: no IPs, keys, protocol versions or tool names (for example, never "tinytuya"). Put them under a collapsed "Opciones avanzadas".
- Page header: `div.page-head` with `a.back` (small "‹ Parent" link) above the `h1`, never a "Volver" button. The primary action goes at the right of the header (`a.btn.primary`).
- Forms: `method="post"` to `/plugins/<id>/…`, then `303` to `/?tab=<id>…&ok=<code>`. On error, redirect with `&err=<message>` and render it with `div.alert.bad` near where it happened.
- Dialogs: `div.modal#<id>` with `a.scrim href="#_"`, `div.dialog.dialog-wide`, `div.dialog-top`. Open them with `href="#<id>"` (CSS `:target`). Prefer a modal over an inline form for "add" and "edit". Nido turns every dialog into a bottom sheet on phones, locks the page behind it, moves focus into it, keeps Tab inside and gives focus back on close: do not restyle `.modal`/`.dialog` in a plugin. To prefill an edit dialog, link to `?…&e=<id>#<id>-edit` and render that dialog only on that view.
- Live data: put `span hidden data-refresh="<s>"` on views that change by themselves. Nido reloads them in place and keeps the scroll position. Do not add it to pages with forms people are typing in.
- Do not disable a control because a background health check failed once. Let the action run and show the error.
- Reuse core classes (`card`, `btn`, `field`, `input`, `pill`, `empty`, `hint`, `two`, `rows`) and tokens (`--ink`, `--mute`, `--line`, `--surface`, `--canvas`, `--accent`). Plugin-specific classes carry a prefix (`dv-`, `rc-`, `gm-`).
- `style.css` is readable CSS (one declaration per line). `build.rs` minifies it; never hand-minify.
- Classes from a plugin never appear in the core CSS or JS. If the core needs a hook, make it generic (an element type or a core class).
- Plugin colours (a category, a pet) are data: tint with `color-mix` over `--surface` for soft backgrounds, and darken to 72% with `#000` before putting white text on them (WCAG AA for every swatch).
- Everything people record or manage can be edited afterwards, not only deleted: entries in a history (a walk, a health note, a weight), schedules, settings, the item's card. A typo or "it was Leo, not me" is fixed in place (same id, `UPDATE`), never by deleting and writing it again. Tapping the row opens its edit dialog already filled in; the create and edit dialogs are the same form (`Option<&Item>`), and the create and edit handlers share one function that reads the form.
- Deleting lives inside the edit dialog, behind a second step (`delete_form` in `pets/src/view/mod.rs`: a `details` that reveals "Sí, borrar"). Rows carry no delete button of their own.
- Toggles that record who did something (a dose, a meal, a chore done) undo with a second tap on the same control.
- To prefill an edit dialog, link to `?…&e=<id>#<x>-edit` and render that dialog only on that view. To prefill a "new" dialog from a suggestion, pass the suggestion in the query (`&nt=<i>#task-new`).
- Touch targets at least 44 px, layouts that work at 375 px, `prefers-reduced-motion` respected, light and dark themes via tokens. On touch screens (`pointer: coarse`) Nido already raises `.btn`, `.btn.sm` and `.icon-btn` to 44 px; plugin controls must do the same.

### Mobile first

Design for a 375 px phone first, then let wider screens show more.

- One page scroll. Never give a box a max height with its own scroll inside the page: it traps the finger and hides content.
- Long lists show their first rows and fold the rest behind a button that says what it reveals ("Ver 6 más", "Ver 3 días más"): about 4 rows (2 days) on a phone, 8 (4 days) on wider screens. A hidden checkbox plus its `label` does it without JavaScript (see `clamp` in `pets/src/view/mod.rs`).
- Secondary rows of cards (a list of pets, what is coming up, suggestions) slide sideways on a phone with `scroll-snap`, showing a peek of the next card, instead of stacking.
- What is done folds away; what is pending stays in sight.
- Headers stay compact on a phone: tags, figures and quick actions go in one sideways row. Section tabs stick to the top while scrolling.
- Explanatory hints can hide on a phone when the screen already says it.
- Icons: Lucide only. If one is missing, add the SVG to `assets/icons/` with the same license header as the others. Brand or app logos are embedded in the plugin (see `devices/logos/`), not in the core.
- Copy is short and factual. No slogans, no exclamation-heavy marketing, no emojis as icons.

## Domain rules learned the hard way

- Local first. Control devices on the LAN; use a cloud only for what cannot be done locally (for example, fetching Tuya local keys once), and keep a manual fallback.
- Many home routers drop broadcasts between Wi-Fi clients. Discovery must also try unicast to each address of the /24.
- UDP loses packets on Wi-Fi: retry requests (3 attempts) and mark a device offline only after several failed checks in a row.
- Devices may silently ignore a repeated packet sequence number. Start counters at a random value for every connection.
- Learning modes may return the previously stored code immediately. Read the stale value first and wait for a different one.
- Re-learning or editing an item updates it in place (same id, same position). Never delete and re-insert.

## Comments

- Doc comments say why something exists or is done that way. No narration of what the next line does.
- No `ponytail:` or similar marker comments.

## Tests

- One small test per non-trivial piece of logic (parsers, codecs, protocol framing, schedulers), in a `#[cfg(test)] mod tests` in the same file. Fake devices over local sockets are fine (see `devices/src/tuya.rs`).
- `cargo test --workspace` must pass with no warnings in the plugin.

## Build and deploy

```sh
cargo test --workspace
cargo zigbuild --release --target aarch64-unknown-linux-musl -p nido-<name>
scp target/aarch64-unknown-linux-musl/release/nido-<name> <host>:nido-install/plugins/
ssh <host> 'chmod +x nido-install/plugins/* && sudo -n /usr/local/bin/nido-deploy'
```

Copy the binary with its executable bit set: `nido-deploy` runs `--version` on it.

`nido-deploy` reinstalls every binary left in `nido-install/plugins/`: delete one there to stop deploying it. There is no other sudo on the Pi, so removing an installed plugin goes through the Plugins page.

## Versions

- Each plugin has its own semver in its `Cargo.toml`, printed by `--version` as `v<version>`.
- `[package.metadata.nido] min = "x.y.z"` is the oldest Nido it works with. Raise it when the plugin starts using something new from the core (a route, a header, a service).
- The release workflow builds every `plugins/*/` and writes `plugins.json` (`bin`, `version`, `min`). Bump an plugin's `version` whenever its code changes, then release Nido.
- Installed plugins are never replaced automatically. The Plugins page compares the installed `--version` with `plugins.json` and offers an update only when the new version fits the installed Nido.

## Commits

One English ASCII title line in conventional form (`feat(devices): …`, `fix(games): …`, `refactor(plugins): …`), no body and no AI attribution lines. Bump versions only when asked.

## Before saying it is done

- [ ] `cargo test --workspace` passes, no new warnings.
- [ ] Every redirect `ok` code is in `/manifest`.
- [ ] If the plugin notifies: `/notices` answers `200` (never `404`), each notice is returned once, and the household ids come from `X-Nido-People`.
- [ ] The page works at 375 px wide and in dark mode, checked with `tools/plugin-shot.sh` (see the root [AGENTS.md](../AGENTS.md#checking-your-work)).
- [ ] No list grows without limit on a phone: long ones fold, secondary rows slide sideways.
- [ ] Touch targets are 44 px, text passes 4.5:1 on its background, and nothing is deleted on a single tap.
- [ ] Everything the plugin lets people create can also be edited.
- [ ] Pages stay light and queries use indexes (see Performance in the root [AGENTS.md](../AGENTS.md#performance)).
- [ ] No technical term on the main path; errors explain what to do.
- [ ] Works without JavaScript.
- [ ] READMEs updated if a new plugin or a user-visible feature was added.
