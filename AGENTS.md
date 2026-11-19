# Guide for AI agents: working on Nido

Nido is a household app (expenses, shopping, chores, bills…) that runs on modest hardware: single-board computers such as a Raspberry Pi, mini PCs, old laptops, down to 512 MB of RAM. Read this before changing anything. For plugins (`plugins/*`), also read [plugins/AGENTS.md](plugins/AGENTS.md): it has the protocol and the plugin rules.

## Where things are

| Path | What |
|---|---|
| `src/` | The core: Rust 2021, `axum` 0.8, `maud` HTML, `rusqlite`. Server-rendered pages; every feature works without JavaScript. |
| `src/pages/` | One module per screen. Shared markup helpers in `src/ui.rs`. |
| `src/plugins/mod.rs` | The plugin catalog (`ALL`) and how Nido starts and proxies plugins. |
| `src/shortcuts/` | Atajos: NFC tags and links (`/t/<code>`), their actions (core ones in `builtin.rs`, plugins' through `/actions`) and the page a phone sees after reading one. |
| `assets/css/` | `tokens.css` (colours, light and dark), `base.css`, `components.css` (buttons, cards, dialogs…), `layout.css`, `pages.css`. |
| `assets/js/app.js` | Progressive enhancement: in-place navigation, dialogs, toasts, notices, plugin scripts. |
| `assets/icons/` | Lucide SVGs, embedded at build time. Add missing ones from `lucide-static@0.460.0`, keeping its license header. |
| `plugins/<name>/` | Each plugin is its own binary. `pets/` is the most complete reference for UI. |
| `tools/` | Build helpers (`css.rs` minifier) and dev scripts (`plugin-shot.sh`). |

## UI standards

These apply to the core and to every plugin. When a core rule breaks a standard, fix it in the core (and write it here) instead of working around it in one screen.

- **Mobile first.** Design at 375 px, then let wider screens show more. One page scroll: no boxes with their own scroll inside the page.
- **Long lists fold** behind a button that says what it reveals ("Ver 6 más"): about 4 rows on a phone, 8 on wider screens. Secondary rows of cards slide sideways on a phone with `scroll-snap` and a peek of the next card. Done items fold away; pending ones stay in sight.
- **Show detail where it belongs, when it is needed.** Information about one item (a plugin's stored data, a technical detail) lives on that item and appears only when the person acts on it or asks for it (a dialog, a "Ver más"). Never gather per-item details into a separate table or summary card for all items.
- **Nothing people must find scrolls sideways.** Section tabs and main actions all fit on a 375 px screen, or become a visible list with a summary per row (Mascotas: "Todo de Luca"). In Baymard's tests 27–43 % of people missed tabs hidden off-screen. Sideways scrolling is only for secondary card rows.
- **Card grids with uneven content flow in columns** (`columns: 2 360px` plus `break-inside: avoid`), not rows: rows either stretch cards (empty space inside) or leave gaps between them. No blank space inside a card to match a neighbour.
- **Choices that change or erase data open a dialog.** Never expand a card in place inside a grid: it reflows or stretches the cards next to it.
- **Frequent one-tap logs save at once and offer "Apuntado · Deshacer"** instead of asking first. Undo only takes back your own recent entry (30 min), so an old link cannot erase what someone else wrote.
- **Touch targets: 44 px.** Core `.btn`, `.btn.sm` and `.icon-btn` grow to 44 px under `@media (pointer: coarse)`; a mouse keeps the compact sizes. Custom controls must do the same.
- **Dialogs** are `div.modal#id` opened by `href="#id"` (`:target`). The core already turns them into bottom sheets on phones, locks the page behind, moves focus inside, keeps Tab inside and gives focus back on close (WAI-ARIA dialog pattern). Do not restyle `.modal` or `.dialog` per screen.
- **Colour.** Only tokens (`--ink`, `--mute`, `--surface`, `--canvas`, `--line`, `--accent`, `--ok`, `--warn`, `--bad`…) so light, dark and house themes work. Text must pass WCAG AA (4.5:1, 3:1 for large text). On a coloured background (a pet's or category colour) darken it with `color-mix(in srgb, <colour> 72%, #000)` before putting white text on it, and check the contrast of every swatch.
- **Do not change colours or themes unless asked.** "Make it prettier" means layout, spacing and hierarchy.
- **The phone header holds a fixed number of things**: logo, the plugin launcher (one button showing the current plugin, opening a sheet with every active plugin) and the signed-in person's face with settings. Never add one icon per plugin or per person to it: it overflows as soon as a house has several.
- **One primary action per screen.** Secondary actions are outline or ghost buttons.
- **Everything people record can be edited**, not only deleted: tapping a row opens its edit dialog, filled in. Fix in place, never delete and re-create.
- **Destructive actions** need a second step: a `details` that reveals "Borrar", or a confirm inside the dialog. Never delete on one tap.
- **Numbers and units** are joined by a no-break space (`12,4\u{a0}kg`) so they never split across lines.
- **Copy** is Spanish, short and factual: no jargon, no slogans, no emoji as icons. Code, identifiers and comments are English.
- **Motion** stays under 300 ms and respects `prefers-reduced-motion` (the core turns animations off globally).
- **Accessibility.** Icon-only buttons have `aria-label`; form fields have visible labels; a hidden checkbox used to fold content takes its name from its `label`, not from an `aria-label`.
- **`hidden` always hides.** `base.css` makes `[hidden]` win over any class's `display`; before, a `.btn` marked `hidden` (the push buttons in Mis ajustes) still showed. Controls that only work with a browser feature (Web NFC) start `hidden` and the script shows them.
- **Saving from a dialog can reopen a dialog.** A form whose `action` ends in `#<dialog-id>` (`/shortcuts#tag-edit`) lands on that dialog after the redirect, with and without JavaScript. Use it when the next step lives there (a new tag shows how to write it).
- **Something done by a link or a tag** answers with a redirect to its own result page (`/t/<code>` → `/t/r/<id>`), so reloading shows what happened instead of doing it again. The service worker never answers `/t/` from its cache.

## Performance

Nido runs on low-power devices with little RAM and often slow storage (an SD card), and is used over Wi-Fi from phones: everything must be fast.

- **Weight per page.** Measure the HTML a tap downloads (`curl -w "%{size_download}"`). Do not repeat inline SVG: define each icon once per page and reference it with `<use>` (see `icon` and `icon_sprite` in `plugins/pets/src/kit.rs`). Caddy compresses responses in front of Nido (`deploy/install.sh`); the app does not compress again. The raw size still costs the phone parsing time: do not render hundreds of rows behind a "Ver más"; cap histories to what is useful.
- **SQLite.** Open with `journal_mode=WAL`, `synchronous=NORMAL`, `temp_store=MEMORY`. Every `WHERE` and `ORDER BY` on a table that grows has an index. Never write `?1 IS NULL OR col=?1`: it keeps SQLite from using the index; build the filter for the case at hand. Fetch only the rows shown (`LIMIT` inside each branch of a `UNION`), and no query inside a loop over rows (N+1).
- **Measure before and after** with release builds and realistic data (months of records), and say the numbers.

## Code standards

- Split modules by responsibility before they pass ~500 lines (see `plugins/pets/src/view/`: `mod.rs` shared pieces, one module per screen or section, `rows.rs` for rows several sections reuse).
- Named structs instead of tuples with more than three fields.
- **Before adding a CSS class to the core, grep `assets/css` and `src` for it.** Core classes are global: a launcher tile named `.app` once overrode the page grid (`.app` in `layout.css`) and broke the desktop layout. Give new core components a specific name (`.launcher-item`, not `.app`); plugins always prefix theirs (`pt-`, `rc-`…).
- Errors shown to people are plain Spanish and say what to do; the technical cause goes to the log.
- `cargo clippy` clean, no new warnings. The repository does not use `cargo fmt`; keep the surrounding style.
- Tests next to the code (`#[cfg(test)] mod tests`) for every non-trivial piece of logic.
- Comments say why, never what the next line does.

## Checking your work

```sh
cargo test --workspace
cargo clippy -p <crate> --all-targets
```

To see a plugin page without running Nido or signing in:

```sh
cargo build -p nido-<name>
NIDO_PLUGIN_KEY=k NIDO_PLUGIN_ADDR=127.0.0.1:8199 NIDO_PLUGIN_DB=/tmp/test.db target/debug/nido-<name> &
# fill it through its own forms (curl -H "X-Nido-Key: k" -H "X-Nido-User: 1" -d … http://127.0.0.1:8199/<route>)
tools/plugin-shot.sh <id> /tmp/shot.png "p=1&s=vet" 390 dark "#dialog-id"
```

Check every change at 375–390 px and at desktop width, in light and dark. Send the screenshots to the person when the change is visual, and keep them in a folder you do not clean afterwards.

- A plugin's `style.css` is embedded at build time: run `cargo build -p nido-<name>` again before screenshots (`cargo test` and `clippy` do not refresh the binary you run).
- Core pages need a session: insert `sha256(cookie)` into `sessions` *after* Nido starts, because it re-hashes existing tokens at startup. To screenshot a saved core page from `file://`, give Chrome `--disable-web-security` so `/icons.svg` loads, strip the scripts and turn animations off (or dialogs come out mid-slide).
- Testing with a copy of the real database: replace `tg_token` in the copy first, or the local Nido polls the same Telegram bot and steals the Pi's updates.

## Platform notes (Windows development machine)

- Files use LF. When editing with Python, write with `newline='\n'` (CRLF once broke `deploy/install.sh` on the device).
- In Git Bash, `grep -c $'\r'` counts every line, not carriage returns: use `python -c "print(open(f,'rb').read().count(b'\r'))"`.
- Headless Chrome will not make a window narrower than ~500 px: phone widths need an iframe (`tools/plugin-shot.sh` does it).
- A running plugin locks its `.exe`: stop it before `cargo build`, or the build fails with "failed to remove file".
- Git Bash paths (`/tmp/...`) mean nothing to Windows programs: use `pwd -W` or `C:/...` paths when passing them to Chrome.
- Inline heredoc scripts in Git Bash break on quotes and `\u` escapes: write the script to a file with the editor, then run it.
- A running `nido.exe` locks the binary just like a plugin: stop it before `cargo build -p nido`.

## Commits and releases

One English ASCII title line in conventional form (`feat(pets): …`, `fix(ui): …`), no body, no AI attribution. Bump versions only when asked. Deploying to the Pi is described in [plugins/AGENTS.md](plugins/AGENTS.md#build-and-deploy).
