//! La oca: 63 squares and one die, with the classic squares and their sayings.
use super::*;
use maud::{html, PreEscaped};
use serde::{Deserialize, Serialize};

const OCAS: [u8; 13] = [5, 9, 14, 18, 23, 27, 32, 36, 41, 45, 50, 54, 59];
const END: u8 = 63;

#[derive(Serialize, Deserialize)]
pub struct Oca {
    pos: Vec<u8>,
    skip: Vec<u8>, // turns each player still has to wait
    turn: usize,
    die: Option<u8>,
    moved: Option<usize>, // who moved last, to mark them
    #[serde(default)]
    path: Vec<u8>, // squares of the last move, start to end (with the jump of a goose, bridge…), to animate it
    winner: Option<usize>,
    log: Vec<(usize, String)>,
}

/// (icon, name) of a special square.
fn special(n: u8) -> Option<(&'static str, &'static str)> {
    Some(match n {
        0 => ("house", "Salida"),
        6 | 12 => ("landmark", "Puente"),
        19 => ("bed", "Posada"),
        26 | 53 => ("dices", "Dados"),
        31 => ("droplets", "Pozo"),
        42 => ("refresh-cw", "Laberinto"),
        52 => ("lock", "Cárcel"),
        58 => ("skull", "Calavera"),
        END => ("trophy", "Jardín de la oca"),
        n if OCAS.contains(&n) => ("bird", "Oca"),
        _ => return None,
    })
}

/// The board is an 11×11 grid of cells this wide (800 / 11).
const CELL: f64 = 800.0 / 11.0;
/// A square's tile, a bit smaller than its cell.
const TILE: f64 = 62.0;

/// The classic spiral, with a gap between laps so the way is easy to follow: square 0 at the
/// bottom-left corner, round anticlockwise and inwards. Square 63, the garden, is the big one in
/// the middle. (column, row) of squares 0-62.
fn spiral() -> Vec<(i32, i32)> {
    let mut at = (0, 10);
    let mut out = vec![at];
    let dirs = [(1, 0), (0, -1), (-1, 0), (0, 1)];
    for (k, len) in [10, 10, 10, 8, 8, 6, 6, 4].into_iter().enumerate() {
        let (dx, dy) = dirs[k % 4];
        for _ in 0..len {
            at = (at.0 + dx, at.1 + dy);
            out.push(at);
        }
    }
    out
}

fn cell(c: i32, r: i32) -> (f64, f64) {
    (c as f64 * CELL + CELL / 2.0, r as f64 * CELL + CELL / 2.0)
}

/// The middle of square `n` on the 800×800 board.
fn center(n: u8) -> (f64, f64) {
    if n >= END {
        return (400.0, 400.0);
    }
    let (c, r) = spiral()[n as usize];
    cell(c, r)
}

/// Where a token sits in its square when `count` share it.
fn spot(count: usize, slot: usize) -> (f64, f64) {
    match count {
        1 => (0.0, 6.0),
        2 => ([-14.0, 14.0][slot % 2], 8.0),
        _ => ([-14.0, 14.0, -14.0, 14.0][slot % 4], [-5.0, -5.0, 17.0, 17.0][slot % 4]),
    }
}

/// What each special square does, for the legend under the board.
const RULES: [(&str, &str, &str); 9] = [
    ("bird", "Oca · 5, 9, 14…", "De oca a oca y tiro porque me toca"),
    ("landmark", "Puente · 6 y 12", "Cruzas al otro puente y vuelves a tirar"),
    ("dices", "Dados · 26 y 53", "Saltas al otro dado y vuelves a tirar"),
    ("bed", "Posada · 19", "Descansas: un turno sin tirar"),
    ("droplets", "Pozo · 31", "Te caes: dos turnos sin tirar"),
    ("refresh-cw", "Laberinto · 42", "Te pierdes y vuelves a la 30"),
    ("lock", "Cárcel · 52", "Dos turnos encerrado"),
    ("skull", "Calavera · 58", "Vuelta a empezar desde la salida"),
    ("trophy", "Jardín · 63", "Hay que llegar justo; si te pasas, rebotas"),
];

/// Grass, the river between the laps and the garden in the middle: drawn once, under the squares.
fn scenery() -> String {
    let mut s = String::new();
    // The river runs in the gap between the outer lap and the inner one.
    let (a, b) = (cell(1, 1), cell(9, 9));
    s += &format!(r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="{:.1}" fill="none" stroke="#7cc4e8" stroke-width="{:.1}"/><rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="{:.1}" fill="none" stroke="#a5d8f2" stroke-width="6" stroke-dasharray="14 26" opacity=".8"/>"##,
        a.0, a.1, b.0 - a.0, b.1 - a.1, CELL * 0.6, CELL * 0.62, a.0, a.1, b.0 - a.0, b.1 - a.1, CELL * 0.6);
    // Lily pads and ducks on the water.
    for &(c, r, duck) in &[(4, 1, false), (9, 4, true), (6, 9, false), (1, 3, true), (9, 7, false), (2, 9, false), (7, 1, true)] {
        let (x, y) = cell(c, r);
        if duck {
            s += &format!(r##"<g transform="translate({x:.1} {y:.1})"><ellipse cy="4" rx="11" ry="6" fill="#fff"/><circle cx="7" cy="-3" r="5" fill="#fff"/><path d="M11 -3l5 1-5 1z" fill="#f59e0b"/><circle cx="8" cy="-4" r=".9" fill="#1f2937"/></g>"##);
        } else {
            s += &format!(r##"<g transform="translate({x:.1} {y:.1})"><path d="M0 0L10 -4A11 11 0 1 1 10 4z" fill="#4ade80" opacity=".85"/><circle cx="-3" cy="-2" r="2.5" fill="#f9a8d4"/></g>"##);
        }
    }
    // The wooden bridge where the way crosses the river.
    let (x, y) = cell(1, 8);
    s += &format!(r##"<g transform="translate({x:.1} {y:.1})"><rect x="-26" y="-40" width="52" height="80" rx="6" fill="#b07a45"/>{}</g>"##,
        (0..6).map(|k| format!(r##"<rect x="-26" y="{}" width="52" height="3" fill="#8a5a2e" opacity=".6"/>"##, -36 + k * 13)).collect::<String>());
    // Trees and flowers round the garden.
    for c in 3..=7 {
        for r in 3..=7 {
            if (4..=6).contains(&c) && (4..=6).contains(&r) {
                continue;
            }
            let (x, y) = cell(c, r);
            s += &if (c + r) % 2 == 0 {
                format!(r##"<g transform="translate({x:.1} {y:.1})"><ellipse cy="16" rx="16" ry="5" fill="#000" opacity=".12"/><rect x="-3" y="2" width="6" height="14" rx="2" fill="#8a5a2e"/><circle cy="-6" r="17" fill="#3f9b52"/><circle cx="-6" cy="-11" r="8" fill="#5bb96c"/></g>"##)
            } else {
                format!(r##"<g transform="translate({x:.1} {y:.1})"><circle cx="-10" cy="-6" r="4" fill="#f472b6"/><circle cx="8" cy="-10" r="4" fill="#facc15"/><circle cx="2" cy="8" r="4" fill="#fb923c"/><circle cx="-6" cy="12" r="3" fill="#fff"/><circle cx="12" cy="6" r="3" fill="#c084fc"/></g>"##)
            };
        }
    }
    s
}



/// The square's look, as a CSS class of the board.
fn kind(n: u8) -> &'static str {
    match n {
        0 => "start",
        END => "end",
        6 | 12 | 26 | 53 => "jump",
        19 | 31 | 42 | 52 | 58 => "bad",
        n if OCAS.contains(&n) => "goose",
        _ => "",
    }
}

/// Text and attribute values inside the board's SVG.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn icon_at(name: &str, x: f64, y: f64, size: f64) -> String {
    icon_at_color(name, x, y, size, "currentColor")
}

fn icon_at_color(name: &str, x: f64, y: f64, size: f64, color: &str) -> String {
    let inner = crate::kit::ICONS.iter().find(|(n, _)| *n == name).map_or("", |(_, s)| *s);
    format!(r#"<svg class="ico" x="{x:.1}" y="{y:.1}" width="{size:.1}" height="{size:.1}" viewBox="0 0 24 24" fill="none" stroke="{color}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{inner}</svg>"#)
}

/// The board's own textures: a cardboard frame and a little paper grain for the squares.
const BOARD_DEFS: &str = r##"<defs><radialGradient id="oca-grass" cx="50%" cy="45%" r="75%"><stop offset="0" stop-color="#b9e4a0"/><stop offset="1" stop-color="#8fcf7a"/></radialGradient><pattern id="oca-blades" width="14" height="14" patternUnits="userSpaceOnUse"><path d="M3 10l1-4M9 13l1.5-4M12 6l-1-3" stroke="#5aa64a" stroke-width="1.2" stroke-linecap="round" opacity=".35"/></pattern><radialGradient id="oca-wood" cx="50%" cy="40%" r="75%"><stop offset="0" stop-color="#f6e2bf"/><stop offset="1" stop-color="#e2bd86"/></radialGradient><pattern id="oca-grain" width="10" height="10" patternUnits="userSpaceOnUse" patternTransform="rotate(35)"><path d="M0 5h10" stroke="#8b5e34" stroke-width=".6" opacity=".12"/></pattern><pattern id="oca-dots" width="9" height="9" patternUnits="userSpaceOnUse"><circle cx="2" cy="2" r=".9" fill="#7c5a36" opacity=".12"/></pattern><linearGradient id="oca-shine" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".55"/><stop offset=".5" stop-color="#fff" stop-opacity="0"/></linearGradient></defs><rect class="frame" x="-10" y="-10" width="820" height="820" rx="34" fill="url(#oca-grass)"/><rect x="-10" y="-10" width="820" height="820" rx="34" fill="url(#oca-blades)"/>"##;

impl Oca {
    fn next_turn(&mut self) {
        let n = self.pos.len();
        loop {
            self.turn = (self.turn + 1) % n;
            if self.skip[self.turn] == 0 {
                return;
            }
            self.skip[self.turn] -= 1;
            push_log(&mut self.log, self.turn, "pierde el turno".into());
        }
    }
}

impl Game for Oca {
    fn new(players: usize, _: &str, _: &mut Rng) -> Self {
        Oca { pos: vec![0; players], skip: vec![0; players], turn: 0, die: None, moved: None, path: Vec::new(), winner: None, log: Vec::new() }
    }
    fn turn(&self) -> usize {
        self.turn
    }
    fn outcome(&self) -> Option<Outcome> {
        self.winner.map(Outcome::Win)
    }
    fn play(&mut self, seat: usize, mv: &str, rng: &mut Rng) -> Result<(), &'static str> {
        if seat != self.turn {
            return Err("No es tu turno");
        }
        if mv != "roll" {
            return Err("Movimiento no válido");
        }
        let d = rng.die();
        self.die = Some(d);
        self.moved = Some(seat);
        let mut walk = vec![self.pos[seat]];
        let mut back = false;
        for _ in 0..d {
            let q = *walk.last().unwrap_or(&0);
            back |= q == END;
            walk.push(if back { q - 1 } else { q + 1 });
        }
        let mut p = self.pos[seat] + d;
        if p > END {
            p = END - (p - END); // bounces back from the garden
        }
        let mut again = false;
        let said = match p {
            END => {
                self.winner = Some(seat);
                "¡llega al jardín de la oca!".to_string()
            }
            n if OCAS.contains(&n) => {
                p = OCAS.iter().copied().find(|&o| o > n).unwrap_or(END);
                again = p != END;
                if p == END {
                    self.winner = Some(seat);
                }
                format!("de oca a oca ({n} → {p}) y tiro porque me toca")
            }
            6 | 12 => {
                p = if p == 6 { 12 } else { 6 };
                again = true;
                format!("de puente a puente ({p}) y tiro porque me lleva la corriente")
            }
            26 | 53 => {
                p = if p == 26 { 53 } else { 26 };
                again = true;
                format!("de dado a dado ({p}) y tiro porque me ha tocado")
            }
            19 => {
                self.skip[seat] = 1;
                "cae en la posada: un turno sin tirar".into()
            }
            31 => {
                self.skip[seat] = 2;
                "cae en el pozo: dos turnos sin tirar".into()
            }
            42 => {
                p = 30;
                "se pierde en el laberinto: vuelve a la 30".into()
            }
            52 => {
                self.skip[seat] = 2;
                "va a la cárcel: dos turnos sin tirar".into()
            }
            58 => {
                p = 0;
                "cae en la calavera: vuelta a empezar".into()
            }
            n => format!("avanza a la {n}"),
        };
        if walk.last() != Some(&p) {
            walk.push(p);
        }
        self.path = walk;
        self.pos[seat] = p;
        push_log(&mut self.log, seat, format!("saca un {d} y {said}"));
        if self.winner.is_none() && !again {
            self.next_turn();
        }
        Ok(())
    }
    fn bot(&self, _: usize, _: &mut Rng) -> String {
        "roll".into()
    }
    fn view(&self, cx: &Ctx) -> Markup {
        let playing = self.winner.is_none();
        let mut svg = String::from(BOARD_DEFS);
        svg += &scenery();
        // The way round, under the squares.
        let road: Vec<String> = (0..=END).map(|n| format!("{:.0},{:.0}", center(n).0, center(n).1)).collect();
        svg += &format!(r#"<polyline class="road" points="{}"/>"#, road.join(" "));
        for n in 0..END {
            let (x, y) = center(n);
            let kind = kind(n);
            let h = TILE / 2.0;
            svg += &format!(r#"<g class="sq {kind}"><rect x="{:.1}" y="{:.1}" width="{TILE}" height="{TILE}" rx="12"/><rect class="tex" x="{:.1}" y="{:.1}" width="{TILE}" height="{TILE}" rx="12"/><text x="{:.1}" y="{:.1}">{n}</text>"#, x - h, y - h, x - h, y - h, x - h + 7.0, y - h + 16.0);
            if let Some((ic, name)) = special(n) {
                svg += &format!(r#"<title>{name}</title>"#);
                svg += &icon_at(ic, x - 11.0, y - 13.0, 22.0);
                if n != 0 {
                    svg += &format!(r#"<text class="nm" x="{x:.1}" y="{:.1}">{name}</text>"#, y + h - 6.0);
                }
            }
            svg += "</g>";
        }
        // The garden: the goal, big in the middle.
        svg += &format!(r##"<g class="sq end garden"><rect x="291" y="291" width="218" height="218" rx="34"/><rect class="tex" x="291" y="291" width="218" height="218" rx="34"/><text x="306" y="318">63</text>{}<text class="nm big" x="400" y="490">Jardín de la oca</text><title>Jardín de la oca</title></g>"##, icon_at("bird", 352.0, 340.0, 96.0));
        // Tokens: alone in the middle of the square, or sharing it in a little group.
        let mut tokens = String::new();
        for s in 0..self.pos.len() {
            let n = self.pos[s];
            let here: Vec<usize> = (0..self.pos.len()).filter(|&o| self.pos[o] == n).collect();
            let slot = here.iter().position(|&o| o == s).unwrap_or(0);
            let (dx, dy) = spot(here.len(), slot);
            let (x, y) = (center(n).0 + dx, center(n).1 + dy);
            let moved = self.moved == Some(s) && self.path.len() > 1;
            let steps = if moved {
                let mut pts: Vec<String> = self.path[..self.path.len() - 1].iter().map(|&q| format!("{:.1},{:.1}", center(q).0, center(q).1 + 12.0)).collect();
                pts.push(format!("{x:.1},{y:.1}"));
                pts.join(" ")
            } else {
                String::new()
            };
            let r = if here.len() == 1 { 17.0 } else { 12.5 };
            let face = match (cx.seats.get(s), cx.photo(s)) {
                (Some(seat), _) if seat.bot => format!(r##"<circle r="{r}" fill="#334155"/>{}"##, icon_at_color("bot", -r * 0.62, -r * 0.62, r * 1.24, "#fde68a")),
                (_, Some(src)) => format!(r##"<clipPath id="oca-face-{s}"><circle r="{r}"/></clipPath><circle r="{r}" fill="#fff"/><image href="{}" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" clip-path="url(#oca-face-{s})" preserveAspectRatio="xMidYMid slice"/>"##, esc(src), -r, -r, r * 2.0, r * 2.0),
                _ => format!(r##"<circle r="{r}" fill="{c}"/><text class="ini" y="{:.1}" font-size="{:.0}">{}</text>"##, r * 0.36, r * 1.05, esc(&cx.name(s).chars().next().unwrap_or('?').to_uppercase().to_string()), c = cx.color(s)),
            };
            tokens += &format!(
                r##"<g class="tk{}" transform="translate({x:.1} {y:.1})"{}><title>{}</title><ellipse class="shadow" cy="{:.1}" rx="{:.1}" ry="5"/><circle r="{:.1}" fill="{c}"/>{face}<circle r="{r}" fill="none" stroke="#fff" stroke-width="2" opacity=".85"/><ellipse cx="{:.1}" cy="{:.1}" rx="{:.1}" ry="{:.1}" fill="#fff" opacity=".35"/></g>"##,
                if moved { " moved" } else { "" },
                if steps.is_empty() { String::new() } else { format!(r#" data-steps="{steps}""#) },
                esc(cx.name(s)),
                r + 3.0,
                r,
                r + 4.0,
                -r * 0.32,
                -r * 0.42,
                r * 0.42,
                r * 0.2,
                c = cx.color(s)
            );
        }
        svg += &tokens;
        html! {
            div.gm-oca-wrap {
                svg.gm-oca-board viewBox="-10 -10 820 820" role="img" aria-label="Tablero de la oca" { (PreEscaped(svg)) }
                ul.gm-oca-rules {
                    @for (ic, name, rule) in RULES {
                        li { span.gm-oca-rule-ico { (crate::kit::icon(ic)) } span { b { (name) } (rule) } }
                    }
                }
                div.gm-dice-bar {
                    @if let Some(d) = self.die { (die_face(d)) }
                    @if playing && cx.my_turn(self.turn) {
                        button.btn.primary type="submit" name="m" value="roll" { (crate::kit::icon("dices")) "Tirar el dado" }
                    }
                }
            }
        }
    }
    fn log(&self) -> Vec<(usize, String)> {
        self.log.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squares_follow_the_rules() {
        // Start the player just before a square and use a seed whose first die is `d`.
        let land = |from: u8, d: u8| {
            let mut g = Oca::new(2, "", &mut Rng::seeded(1));
            g.pos[0] = from;
            // Find a seed whose first die is `d`.
            let mut seed = 1;
            loop {
                let mut r = Rng::seeded(seed);
                if r.die() == d {
                    break;
                }
                seed += 1;
            }
            g.play(0, "roll", &mut Rng::seeded(seed)).unwrap();
            g
        };
        let g = land(3, 2); // 5 is a goose → 9, same player again
        assert_eq!((g.pos[0], g.turn), (9, 0));
        let g = land(4, 2); // 6 → bridge to 12, again
        assert_eq!((g.pos[0], g.turn), (12, 0));
        let g = land(56, 2); // 58 skull → start
        assert_eq!(g.pos[0], 0);
        let g = land(60, 6); // 66 bounces to 60
        assert_eq!(g.pos[0], 60);
        let g = land(57, 6); // exactly 63 wins
        assert_eq!(g.winner, Some(0));
        let g = land(17, 2); // 19 inn: next player plays, and the innkeeper waits
        assert_eq!((g.pos[0], g.skip[0], g.turn), (19, 1, 1));
        let g = land(3, 2); // the walk to the goose, then the jump
        assert_eq!(g.path, vec![3, 4, 5, 9]);
        let g = land(60, 6); // up to 63 and back
        assert_eq!(g.path, vec![60, 61, 62, 63, 62, 61, 60]);
    }

    #[test]
    fn the_spiral_leaves_a_gap_between_laps() {
        let s = spiral();
        let mut cells = s.clone();
        cells.sort();
        cells.dedup();
        assert_eq!((s.len(), cells.len()), (63, 63), "squares 0-62, none twice");
        assert_eq!(s[0], (0, 10), "starts at the bottom-left corner");
        assert!(s.windows(2).all(|w| (w[0].0 - w[1].0).abs() + (w[0].1 - w[1].1).abs() == 1), "each square touches the next");
        assert!(s.iter().all(|&(c, r)| !((3..=7).contains(&c) && (3..=7).contains(&r))), "the middle is left for the garden");
        assert_eq!(center(END), (400.0, 400.0));
    }
}
