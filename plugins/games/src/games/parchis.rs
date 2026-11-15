//! Parchís with two dice: each die moves a pawn (the same one or two different ones, you choose),
//! out of home with a 5, 68 squares around (12 safe ones), seven in each colour's corridor and then
//! home. Eating a pawn gives 20 to count, reaching home gives 10. Two pawns on a square make a
//! barrier nobody passes. Doubles roll again, and a third double in a row sends your last moved
//! pawn home.
//!
//! A pawn's progress: -1 at home, 0..=63 around the board from its start, 64..=70 in its corridor,
//! 71 home (goal).
use super::*;
use maud::{html, PreEscaped};
use serde::{Deserialize, Serialize};

const START: [i32; 4] = [5, 22, 39, 56];
const SAFE: [i32; 12] = [5, 12, 17, 22, 29, 34, 39, 46, 51, 56, 63, 68];
const LANE: i8 = 64;
const GOAL: i8 = 71;
pub const COLORS: [&str; 4] = ["#f6c000", "#1e88e5", "#e53935", "#43a047"];
const NAMES: [&str; 4] = ["amarilla", "azul", "roja", "verde"];

#[derive(Serialize, Deserialize)]
pub struct Parchis {
    colors: Vec<usize>,     // colour of each seat
    pawns: Vec<[i8; 4]>,    // progress of each pawn
    turn: usize,
    rolled: bool,
    #[serde(default)]
    dice: [u8; 2],
    #[serde(default)]
    rolls: u32,
    pending: Vec<(u8, bool)>, // amounts still to move this turn: (amount, it is a die)
    sixes: u8,                // doubles in a row
    last: Option<(usize, usize)>,
    #[serde(default)]
    from: Option<i8>, // where the last moved pawn started, to draw its walk
    log: Vec<(usize, String)>,
    winner: Option<usize>,
}

/// The board square (1..=68) of a pawn of `color` with progress `p`, if it is around the board.
fn square(color: usize, p: i8) -> Option<i32> {
    (0..LANE).contains(&p).then(|| (START[color] - 1 + p as i32) % 68 + 1)
}

impl Parchis {
    fn on_square(&self, sq: i32) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (s, pawns) in self.pawns.iter().enumerate() {
            for (k, &p) in pawns.iter().enumerate() {
                if square(self.colors[s], p) == Some(sq) {
                    out.push((s, k));
                }
            }
        }
        out
    }

    /// Where each pawn of `s` can go with `amount` (exiting home only with the die's 5).
    fn targets(&self, s: usize, amount: u8, die: bool) -> Vec<(usize, i8)> {
        let c = self.colors[s];
        let mut out = Vec::new();
        for k in 0..4 {
            let p = self.pawns[s][k];
            if p == -1 {
                let own = self.on_square(START[c]).iter().filter(|o| o.0 == s).count();
                if die && amount == 5 && own < 2 {
                    out.push((k, 0));
                }
                continue;
            }
            if p >= GOAL || p + amount as i8 > GOAL {
                continue;
            }
            let t = p + amount as i8;
            let blocked = (p + 1..=t).any(|step| {
                if step < LANE {
                    self.on_square(square(c, step).unwrap_or(0)).len() >= 2 // a barrier (or a full safe square)
                } else {
                    step < GOAL && self.pawns[s].iter().filter(|&&q| q == step).count() >= 2
                }
            });
            if !blocked {
                out.push((k, t));
            }
        }
        out
    }

    fn mv(&mut self, s: usize, k: usize, t: i8) {
        let c = self.colors[s];
        let from = self.pawns[s][k];
        self.pawns[s][k] = t;
        self.last = Some((s, k));
        self.from = Some(from);
        let mut said = match (from, t) {
            (-1, _) => "saca una ficha".to_string(),
            (_, GOAL) => "llega a casa (+10)".to_string(),
            (_, t) if t >= LANE => "entra en su pasillo".to_string(),
            (_, t) => format!("mueve a la {}", square(c, t).unwrap_or(0)),
        };
        if let Some(sq) = square(c, t) {
            // Safe squares protect, except your own start when you come out of home.
            if !SAFE.contains(&sq) || from == -1 {
                if let Some(&(o, ok)) = self.on_square(sq).iter().find(|o| o.0 != s) {
                    self.pawns[o][ok] = -1;
                    self.pending.push((20, false));
                    said = format!("{said} y se come una ficha {} (+20)", NAMES[self.colors[o]]);
                }
            }
        }
        if t == GOAL {
            self.pending.push((10, false));
        }
        push_log(&mut self.log, s, said);
        if self.pawns[s].iter().all(|&p| p == GOAL) {
            self.winner = Some(s);
            self.pending.clear();
        }
    }

    /// Every way to move now: (which pending amount, pawn, where it lands).
    fn options(&self, s: usize) -> Vec<(usize, usize, i8)> {
        let mut out = Vec::new();
        for (i, &(a, die)) in self.pending.iter().enumerate() {
            if self.pending[..i].contains(&(a, die)) {
                continue; // the same number twice (doubles) gives the same choices
            }
            out.extend(self.targets(s, a, die).into_iter().map(|(k, t)| (i, k, t)));
        }
        out
    }

    /// When nothing left can be moved, drops it and ends the turn (doubles roll again).
    fn settle(&mut self, s: usize) {
        if self.winner.is_some() || !self.options(s).is_empty() {
            return;
        }
        for (a, die) in std::mem::take(&mut self.pending) {
            push_log(&mut self.log, s, if die { format!("no puede mover con un {a}") } else { format!("no puede contar {a}") });
        }
        self.rolled = false;
        if self.dice[0] != self.dice[1] {
            self.sixes = 0;
            self.turn = (self.turn + 1) % self.pawns.len();
        }
    }
}

// ---------- the board: the classic cross, 19 by 19 cells ----------
//
// Four homes in the corners (yellow top-left, blue bottom-left, red bottom-right, green top-right),
// four arms of three columns: the outer two are the loop of 68 squares, the middle one is a colour's
// corridor into the centre. Squares are numbered walking anticlockwise, as on a real board.

const U: f64 = 20.0; // one cell, in SVG units
const DARK: [&str; 4] = ["#b98a00", "#1559b3", "#b71c1c", "#2e7d32"];
const CORNER: [(f64, f64); 4] = [(0.0, 0.0), (0.0, 11.0), (11.0, 11.0), (11.0, 0.0)];

/// Cell (column, row) of each square 1..=68.
fn track() -> Vec<(i32, i32)> {
    let mut v = Vec::with_capacity(68);
    v.extend((0..=7).map(|y| (8, y))); // 1..8 down the top arm
    v.extend((0..=7).rev().map(|x| (x, 8))); // 9..16 along the left arm
    v.push((0, 9)); // 17
    v.extend((0..=7).map(|x| (x, 10))); // 18..25
    v.extend((11..=18).map(|y| (8, y))); // 26..33 down the bottom arm
    v.push((9, 18)); // 34
    v.extend((11..=18).rev().map(|y| (10, y))); // 35..42
    v.extend((11..=18).map(|x| (x, 10))); // 43..50 along the right arm
    v.push((18, 9)); // 51
    v.extend((11..=18).rev().map(|x| (x, 8))); // 52..59
    v.extend((0..=7).rev().map(|y| (10, y))); // 60..67 up the top arm
    v.push((9, 0)); // 68
    v
}

/// Cell of step `k` (1..=7) of colour `c`'s corridor.
fn lane(c: usize, k: i32) -> (i32, i32) {
    match c {
        0 => (9, k),
        1 => (k, 9),
        2 => (9, 18 - k),
        _ => (18 - k, 9),
    }
}

fn center(cell: (i32, i32)) -> (f64, f64) {
    ((cell.0 as f64 + 0.5) * U, (cell.1 as f64 + 0.5) * U)
}

/// Where pawn `k` of colour `c` with progress `p` is drawn.
fn spot(c: usize, k: usize, p: i8, path: &[(i32, i32)]) -> (f64, f64) {
    match p {
        -1 => {
            let (ox, oy) = CORNER[c];
            let (dx, dy) = [(2.75, 2.75), (5.25, 2.75), (2.75, 5.25), (5.25, 5.25)][k];
            ((ox + dx) * U, (oy + dy) * U)
        }
        GOAL => {
            let d = k as f64 * 6.0 - 9.0;
            match c {
                0 => (190.0 + d, 170.0),
                1 => (170.0, 190.0 + d),
                2 => (190.0 + d, 210.0),
                _ => (210.0, 190.0 + d),
            }
        }
        p if p >= LANE => center(lane(c, (p - LANE + 1) as i32)),
        p => center(path[(square(c, p).unwrap_or(1) - 1) as usize]),
    }
}

/// A pawn: a little cylinder with its shadow and a shine, in its colour.
fn pawn(x: f64, y: f64, c: usize, class: &str) -> String {
    format!(
        r##"<g class="pawn {class}" transform="translate({x:.1} {y:.1})"><ellipse cx="0" cy="5.5" rx="7.5" ry="3" fill="#000" opacity=".25"/><path d="M-7 -2v5a7 3.2 0 0 0 14 0v-5z" fill="{d}"/><ellipse cx="0" cy="-2" rx="7" ry="3.2" fill="{f}"/><ellipse cx="-2" cy="-3" rx="2.6" ry="1.1" fill="#fff" opacity=".7"/></g>"##,
        d = DARK[c],
        f = COLORS[c]
    )
}

/// A five-pointed star for the safe squares.
fn star(x: f64, y: f64) -> String {
    let pts: String = (0..10)
        .map(|i| {
            let r = if i % 2 == 0 { 6.0 } else { 2.6 };
            let a = -std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::PI / 5.0;
            format!("{:.1},{:.1} ", x + r * a.cos(), y + r * a.sin())
        })
        .collect();
    format!(r#"<polygon points="{pts}" class="star"/>"#)
}

impl Game for Parchis {
    fn new(players: usize, _: &str, _: &mut Rng) -> Self {
        let colors = match players {
            2 => vec![0, 2],
            3 => vec![0, 1, 2],
            _ => vec![0, 1, 2, 3],
        };
        Parchis { pawns: vec![[-1; 4]; colors.len()], colors, turn: 0, rolled: false, dice: [0, 0], rolls: 0, pending: Vec::new(), sixes: 0, last: None, from: None, log: Vec::new(), winner: None }
    }
    fn rolls(&self) -> u32 {
        self.rolls
    }
    fn turn(&self) -> usize {
        self.turn
    }
    fn outcome(&self) -> Option<Outcome> {
        self.winner.map(Outcome::Win)
    }
    fn play(&mut self, s: usize, mv: &str, rng: &mut Rng) -> Result<(), &'static str> {
        if s != self.turn {
            return Err("No es tu turno");
        }
        if mv == "roll" {
            if self.rolled {
                return Err("Ya has tirado: mueve una ficha");
            }
            let (a, b) = (rng.die(), rng.die());
            self.rolls += 1;
            self.dice = [a, b];
            self.from = None; // a roll moves nothing: no walk to replay
            if a == b {
                self.sixes += 1;
                if self.sixes == 3 {
                    if let Some((ls, lk)) = self.last.filter(|l| l.0 == s) {
                        if (0..LANE).contains(&self.pawns[ls][lk]) {
                            self.pawns[ls][lk] = -1;
                        }
                    }
                    push_log(&mut self.log, s, "saca tres dobles: su última ficha vuelve a casa".into());
                    self.sixes = 0;
                    self.dice = [0, 0];
                    self.turn = (s + 1) % self.pawns.len();
                    return Ok(());
                }
                push_log(&mut self.log, s, format!("saca dobles de {a}: vuelve a tirar"));
            } else {
                self.sixes = 0;
                push_log(&mut self.log, s, format!("saca {a} y {b}"));
            }
            self.pending = vec![(a, true), (b, true)];
            self.rolled = true;
            self.settle(s);
            return Ok(());
        }
        // "pawn:which" (which pending number), or just "pawn" for the first one that can move it.
        let mut parts = mv.split(':');
        let k: usize = parts.next().and_then(|n| n.parse().ok()).ok_or("Movimiento no válido")?;
        let which: Option<usize> = parts.next().and_then(|n| n.parse().ok());
        if self.pending.is_empty() {
            return Err("Primero tira los dados");
        }
        let (i, _, t) = self.options(s).into_iter().find(|o| o.1 == k && which.map_or(true, |w| self.pending.get(w) == self.pending.get(o.0))).ok_or("Esa ficha no puede moverse con ese número")?;
        self.pending.remove(i); // used up before moving: the move may add prizes, or win and clear the rest
        self.mv(s, k, t);
        self.settle(s);
        Ok(())
    }
    fn bot(&self, s: usize, _: &mut Rng) -> String {
        if !self.rolled {
            return "roll".into();
        }
        let c = self.colors[s];
        self.options(s)
            .into_iter()
            .max_by_key(|&(_, k, t)| {
                let from = self.pawns[s][k];
                let mut score = t as i32;
                if from == -1 {
                    score += 60;
                }
                if t == GOAL {
                    score += 80;
                } else if t >= LANE {
                    score += 30;
                }
                if let Some(sq) = square(c, t) {
                    if SAFE.contains(&sq) {
                        score += 15;
                    }
                    if (!SAFE.contains(&sq) || from == -1) && self.on_square(sq).iter().any(|o| o.0 != s) {
                        score += 100;
                    }
                }
                score
            })
            .map_or("roll".into(), |(i, k, _)| format!("{k}:{i}"))
    }
    fn view(&self, cx: &Ctx) -> Markup {
        let mine = self.winner.is_none() && cx.my_turn(self.turn);
        let options = if mine && self.rolled { self.options(self.turn) } else { Vec::new() };
        let path = track();
        let seat_of = |c: usize| self.colors.iter().position(|&x| x == c);
        let mut svg = String::with_capacity(24_000);
        // Frame and board.
        svg += r##"<defs><linearGradient id="gold" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ffd76a"/><stop offset=".5" stop-color="#f2b33d"/><stop offset="1" stop-color="#d9962a"/></linearGradient></defs>"##;
        svg += r##"<rect x="-9" y="-9" width="398" height="398" rx="22" fill="url(#gold)"/><rect x="-2" y="-2" width="384" height="384" rx="14" fill="#fff"/>"##;
        // The four homes, with the player's name.
        for c in 0..4 {
            let (ox, oy) = CORNER[c];
            let used = seat_of(c).is_some();
            svg += &format!(
                r##"<g class="home{}"><rect x="{:.0}" y="{:.0}" width="{:.0}" height="{:.0}" rx="12" fill="{}"/><rect x="{:.0}" y="{:.0}" width="{:.0}" height="{:.0}" rx="12" fill="#fff" opacity=".92"/>"##,
                if used { "" } else { " idle" },
                ox * U + 3.0,
                oy * U + 3.0,
                8.0 * U - 6.0,
                8.0 * U - 6.0,
                COLORS[c],
                (ox + 1.5) * U,
                (oy + 1.5) * U,
                5.0 * U,
                5.0 * U
            );
            for (dx, dy) in [(2.75, 2.75), (5.25, 2.75), (2.75, 5.25), (5.25, 5.25)] {
                svg += &format!(r##"<circle cx="{:.1}" cy="{:.1}" r="15" fill="#e5e7eb"/>"##, (ox + dx) * U, (oy + dy) * U);
            }
            if let Some(s) = seat_of(c) {
                let name: String = cx.name(s).chars().take(12).collect();
                let name = name.replace('&', "&amp;").replace('<', "&lt;");
                svg += &format!(r#"<text class="who" x="{:.0}" y="{:.0}">{name}</text>"#, (ox + 4.0) * U, (oy + 7.55) * U);
                // Their face in the middle of the home box, under the pawns' slots.
                if let Some(src) = cx.photo(s) {
                    let (fx, fy) = ((ox + 4.0) * U, (oy + 4.0) * U);
                    svg += &format!(
                        r##"<clipPath id="face{c}"><circle cx="{fx:.0}" cy="{fy:.0}" r="11"/></clipPath><circle cx="{fx:.0}" cy="{fy:.0}" r="12.5" fill="{}"/><image href="{}" x="{:.0}" y="{:.0}" width="22" height="22" clip-path="url(#face{c})" preserveAspectRatio="xMidYMid slice"/>"##,
                        COLORS[c],
                        src.replace('&', "&amp;").replace('"', "&quot;"),
                        fx - 11.0,
                        fy - 11.0
                    );
                }
            }
            svg += "</g>";
        }
        // The loop: white squares, starts in their colour, safe squares with a star.
        for (i, &(x, y)) in path.iter().enumerate() {
            let n = i as i32 + 1;
            let start = START.iter().position(|&s| s == n);
            let fill = start.map_or(if SAFE.contains(&n) { "#eef0f3" } else { "#fff" }, |c| COLORS[c]);
            svg += &format!(r#"<rect class="sq" x="{}" y="{}" width="{U}" height="{U}" fill="{fill}"/>"#, x as f64 * U, y as f64 * U);
            if SAFE.contains(&n) {
                let (cx_, cy_) = center((x, y));
                svg += &star(cx_, cy_);
            }
        }
        // Corridors, and the centre as four triangles.
        for c in 0..4 {
            for k in 1..=7 {
                let (x, y) = lane(c, k);
                svg += &format!(r#"<rect class="sq" x="{}" y="{}" width="{U}" height="{U}" fill="{}"/>"#, x as f64 * U, y as f64 * U, COLORS[c]);
            }
        }
        for (c, pts) in [(0, "160,160 220,160 190,190"), (1, "160,160 160,220 190,190"), (2, "160,220 220,220 190,190"), (3, "220,160 220,220 190,190")] {
            svg += &format!(r##"<polygon points="{pts}" fill="{}" stroke="#fff" stroke-width="1.5"/>"##, COLORS[c]);
        }
        // Pawns, nudged apart when they share a square; the ones that can move get a ring.
        let mut seen: Vec<((i32, i32), u8)> = Vec::new();
        let mut hits = Vec::new();
        for (s, pawns) in self.pawns.iter().enumerate() {
            let c = self.colors[s];
            for (k, &p) in pawns.iter().enumerate() {
                let (mut x, mut y) = spot(c, k, p, &path);
                if (0..GOAL).contains(&p) {
                    let key = (x as i32, y as i32);
                    let n = seen.iter().find(|e| e.0 == key).map_or(0, |e| e.1);
                    x += [0.0, 4.0, -4.0, 0.0][n as usize % 4];
                    y += [0.0, -3.0, -3.0, -6.0][n as usize % 4];
                    match seen.iter_mut().find(|e| e.0 == key) {
                        Some(e) => e.1 += 1,
                        None => seen.push((key, 1)),
                    }
                }
                let can = s == self.turn && options.iter().any(|o| o.1 == k);
                let last = self.last == Some((s, k));
                // Its walk, square by square, for the page to animate (from home: home → start).
                let steps = match (last, self.from) {
                    (true, Some(-1)) => format!("{:.1},{:.1} {x:.1},{y:.1}", spot(c, k, -1, &path).0, spot(c, k, -1, &path).1),
                    (true, Some(f)) if f >= 0 && p > f => {
                        let mut pts: Vec<String> = (f..p).map(|q| { let (a, b) = spot(c, k, q, &path); format!("{a:.1},{b:.1}") }).collect();
                        pts.push(format!("{x:.1},{y:.1}"));
                        pts.join(" ")
                    }
                    _ => String::new(),
                };
                let class = format!("{}{}", if can { "can" } else { "" }, if last { " last" } else { "" });
                if can {
                    svg += &format!(r#"<circle class="ring" cx="{x:.1}" cy="{y:.1}" r="11"/>"#);
                    // One bubble per place: pawns at home (or together) would offer the same thing.
                    if !hits.iter().any(|h: &(usize, f64, f64, i8)| self.pawns[s][h.0] == p) {
                        hits.push((k, (x + 9.0) / 398.0 * 100.0, (y + 9.0) / 398.0 * 100.0, p));
                    }
                }
                let pawn_svg = pawn(x, y, c, &class);
                svg += &if steps.is_empty() { pawn_svg } else { pawn_svg.replacen("<g ", &format!(r#"<g data-steps="{steps}" "#), 1) };
            }
        }
        let describe = |k: usize, t: i8| -> String {
            let c = self.colors[self.turn];
            match (self.pawns[self.turn][k], t) {
                (-1, _) => "sale de casa".into(),
                (_, GOAL) => "llega a casa".into(),
                (_, t) if t >= LANE => format!("al pasillo ({} de 7)", t - LANE + 1),
                (_, t) => {
                    let sq = square(c, t).unwrap_or(0);
                    let eats = (!SAFE.contains(&sq)) && self.on_square(sq).iter().any(|o| o.0 != self.turn);
                    format!("a la {sq}{}", if eats { ", y come" } else { "" })
                }
            }
        };
        // The dice: each one dimmed once used (a double shows both until both are used).
        let mut left: Vec<u8> = self.pending.iter().filter(|p| p.1).map(|p| p.0).collect();
        let dice: Vec<(u8, bool)> = self
            .dice
            .iter()
            .map(|&d| match left.iter().position(|&x| x == d) {
                Some(i) => {
                    left.remove(i);
                    (d, true)
                }
                None => (d, false),
            })
            .collect();
        let prizes: Vec<u8> = self.pending.iter().filter(|p| !p.1).map(|p| p.0).collect();
        html! {
            div.gm-parchis-wrap {
                div.gm-parchis-board {
                    svg.gm-parchis viewBox="-9 -9 398 398" role="img" aria-label="Tablero de parchís" { (PreEscaped(svg)) }
                    // Over each pawn that can move, a bubble with the numbers it can use: tap one.
                    @for (k, x, y, _) in &hits {
                        div.gm-nums style={ "left:" (format!("{x:.2}")) "%;top:" (format!("{y:.2}")) "%;--c:" (COLORS[self.colors[self.turn]]) } {
                            @for (i, _, t) in options.iter().filter(|o| o.1 == *k) {
                                button.prize[!self.pending[*i].1] type="submit" name="m" value={ (k) ":" (i) } title=(describe(*k, *t)) aria-label={ "Ficha " (k + 1) ", mover " (self.pending[*i].0) ": " (describe(*k, *t)) } {
                                    (self.pending[*i].0)
                                }
                            }
                        }
                    }
                }
                div.gm-dice-bar {
                    @if self.dice[0] > 0 {
                        div.gm-dice { @for (d, live) in &dice { span.gm-die-slot.used[!live] { (die_face(*d)) } } }
                    }
                    @if mine && !self.rolled {
                        button.btn.primary type="submit" name="m" value="roll" { (crate::kit::icon("dices")) "Tirar los dados" }
                    } @else if mine && !prizes.is_empty() {
                        span.gm-choices-t { "Premio: cuenta " @for (n, p) in prizes.iter().enumerate() { @if n > 0 { " y " } (p) } " con una ficha" }
                    } @else if mine && self.rolled {
                        span.gm-choices-t { "Elige un número encima de una ficha" }
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

    /// The loop is 68 distinct squares, one step apart (or a corner diagonal), and every start
    /// and corridor entry sits where the rules say.
    #[test]
    fn board_geometry() {
        let t = track();
        assert_eq!(t.len(), 68);
        let mut cells = t.clone();
        cells.sort();
        cells.dedup();
        assert_eq!(cells.len(), 68);
        for i in 0..68 {
            let (a, b) = (t[i], t[(i + 1) % 68]);
            assert!((a.0 - b.0).abs() <= 1 && (a.1 - b.1).abs() <= 1, "squares {} and {} are not neighbours", i + 1, (i + 1) % 68 + 1);
        }
        assert_eq!([t[4], t[21], t[38], t[55]], [(8, 4), (4, 10), (10, 14), (14, 8)], "starts next to their homes");
        assert_eq!([t[67], t[16], t[33], t[50]], [(9, 0), (0, 9), (9, 18), (18, 9)], "entries at the end of each arm");
        for c in 0..4 {
            assert!(!t.contains(&lane(c, 1)), "corridors are not part of the loop");
        }
    }

    #[test]
    fn rules() {
        let mut g = Parchis::new(2, "", &mut Rng::seeded(1)); // yellow (start 5) vs red (start 39)
        // Out only with a 5.
        assert!(g.targets(0, 4, true).is_empty());
        assert_eq!(g.targets(0, 5, true), vec![(0, 0), (1, 0), (2, 0), (3, 0)]);
        assert!(g.targets(0, 5, false).is_empty(), "a bonus never takes a pawn out");
        // Eating: yellow at progress 0 (square 5) moves 3 to square 8 where red sits.
        g.pawns[0][0] = 0;
        g.pawns[1][0] = 37; // red progress 37 = square (39-1+37)%68+1 = 8
        assert_eq!(square(2, 37), Some(8));
        g.mv(0, 0, 3);
        assert_eq!(g.pawns[1][0], -1, "red goes home");
        assert_eq!(g.pending, vec![(20, false)]);
        // A barrier blocks: two red pawns on square 10 stop yellow coming from 8.
        g.pending.clear();
        g.pawns[1][1] = 39;
        g.pawns[1][2] = 39;
        assert_eq!(square(2, 39), Some(10));
        assert!(g.targets(0, 4, true).iter().all(|o| o.0 != 0), "cannot pass the barrier");
        // Exact count into home.
        g.pawns[0][3] = 69;
        assert!(g.targets(0, 2, true).iter().any(|&o| o == (3, GOAL)));
        assert!(g.targets(0, 3, true).iter().all(|o| o.0 != 3));
        // Safe squares protect: red on 12 (safe) is not eaten.
        assert_eq!(square(2, 41), Some(12));
        g.pawns[1][3] = 41;
        g.pawns[0][1] = 5; // yellow progress 5 = square 10? no: (5-1+5)%68+1 = 10
        g.pawns[1][1] = -1;
        g.pawns[1][2] = -1;
        g.mv(0, 1, 7); // to square 12
        assert_eq!(g.pawns[1][3], 41, "safe");
    }

    #[test]
    fn two_dice_pick_per_pawn() {
        let mut rng = Rng::seeded(3);
        let mut g = Parchis::new(2, "", &mut rng);
        g.pawns[0] = [0, 10, -1, -1];
        g.rolled = true;
        g.dice = [2, 5];
        g.pending = vec![(2, true), (5, true)];
        assert!(g.play(0, "2:0", &mut rng).is_err(), "a 2 does not take a pawn out");
        g.play(0, "1:0", &mut rng).unwrap(); // pawn 1 uses the 2
        assert_eq!(g.pawns[0][1], 12);
        assert_eq!(g.pending, vec![(5, true)]);
        g.play(0, "2:0", &mut rng).unwrap(); // and the 5 takes one out
        assert_eq!(g.pawns[0][2], 0);
        assert!(g.pending.is_empty() && g.turn == 1, "not a double: the turn passes");
    }
}
