//! Hundir la flota: 8x8, ships of 4, 3, 3, 2 and 2. First each player arranges their fleet (move,
//! turn, or at random) and says "listo"; then the battle. A hit lets you shoot again. Each player
//! only ever sees their own ships.
use super::*;
use maud::{html, PreEscaped};
use serde::{Deserialize, Serialize};

const N: usize = 8;
const SHIPS: [usize; 5] = [4, 3, 3, 2, 2];

#[derive(Serialize, Deserialize)]
pub struct Fleet {
    ships: Vec<Vec<Vec<usize>>>, // per seat: ships, each its cells
    shots: Vec<Vec<u8>>,         // shots[p]: what p has fired at the other: 0 none, 1 water, 2 hit
    turn: usize,
    last: Option<usize>,
    log: Vec<(usize, String)>,
    #[serde(default = "all_ready")] // matches from before the arranging step were already at war
    ready: [bool; 2],
}

fn all_ready() -> [bool; 2] {
    [true, true]
}

/// The squares of a ship of `len` starting at `start`, if it stays on the board.
fn lay(start: usize, len: usize, across: bool) -> Option<Vec<usize>> {
    let (r, c) = (start / N, start % N);
    let fits = if across { c + len <= N } else { r + len <= N };
    fits.then(|| (0..len).map(|k| if across { start + k } else { start + k * N }).collect())
}

fn neighbours(i: usize) -> impl Iterator<Item = usize> {
    let (r, c) = ((i / N) as isize, (i % N) as isize);
    [(-1, 0), (1, 0), (0, -1), (0, 1)]
        .into_iter()
        .filter_map(move |(dr, dc)| {
            let (rr, cc) = (r + dr, c + dc);
            ((0..N as isize).contains(&rr) && (0..N as isize).contains(&cc)).then(|| (rr * N as isize + cc) as usize)
        })
}

/// Random fleet where ships do not touch, not even at the corners (easier to read).
fn place(rng: &mut Rng) -> Vec<Vec<usize>> {
    'again: loop {
        let mut taken = [false; N * N];
        let mut fleet = Vec::new();
        for &len in &SHIPS {
            let mut tries = 0;
            let ship = loop {
                tries += 1;
                if tries > 200 {
                    continue 'again;
                }
                let across = rng.below(2) == 0;
                let (r, c) = if across { (rng.below(N), rng.below(N - len + 1)) } else { (rng.below(N - len + 1), rng.below(N)) };
                let cells: Vec<usize> = (0..len).map(|k| if across { r * N + c + k } else { (r + k) * N + c }).collect();
                let clear = cells.iter().all(|&i| {
                    let (rr, cc) = ((i / N) as isize, (i % N) as isize);
                    (-1..=1).all(|dr| {
                        (-1..=1).all(|dc| {
                            let (a, b) = (rr + dr, cc + dc);
                            !((0..N as isize).contains(&a) && (0..N as isize).contains(&b)) || !taken[(a * N as isize + b) as usize]
                        })
                    })
                });
                if clear {
                    break cells;
                }
            };
            for &i in &ship {
                taken[i] = true;
            }
            fleet.push(ship);
        }
        return fleet;
    }
}

impl Fleet {
    fn sunk(&self, owner: usize, ship: usize) -> bool {
        self.ships[owner][ship].iter().all(|&i| self.shots[1 - owner][i] == 2)
    }
    fn ship_at(&self, owner: usize, i: usize) -> Option<usize> {
        self.ships[owner].iter().position(|s| s.contains(&i))
    }
    /// Ship `k` of `seat` may sit on `cells`: no other ship is there (yours to decide if they touch).
    fn free(&self, seat: usize, k: usize, cells: &[usize]) -> bool {
        self.ships[seat].iter().enumerate().filter(|(j, _)| *j != k).flat_map(|(_, s)| s).all(|o| !cells.contains(o))
    }
    /// Moves ship `k` to start at `start` (pulled back inside the board if it sticks out).
    fn arrange(&mut self, seat: usize, k: usize, start: usize, across: bool) -> Result<(), &'static str> {
        let len = self.ships[seat][k].len();
        let (r, c) = (start / N, start % N);
        let start = if across { r * N + c.min(N - len) } else { r.min(N - len) * N + c };
        let cells = lay(start, len, across).ok_or("Ahí no cabe")?;
        if !self.free(seat, k, &cells) {
            return Err("Ahí ya hay otro barco");
        }
        self.ships[seat][k] = cells;
        Ok(())
    }
    fn across(ship: &[usize]) -> bool {
        ship.len() > 1 && ship[1] == ship[0] + 1
    }
    /// One board: `owner`'s waters with labels A–H / 1–8, their ships drawn when `reveal` (or sunk),
    /// the other player's shots on top, and buttons to fire at it when `aim`.
    fn grid(&self, owner: usize, reveal: bool, aim: bool) -> Markup {
        let shots = &self.shots[1 - owner];
        html! {
            div.gm-ocean.aim[aim] {
                span {}
                div.gm-ax.gm-ax-n { @for c in 1..=N { span { (c) } } }
                div.gm-ax.gm-ax-l { @for r in 0..N { span { (char::from(b'A' + r as u8)) } } }
                div.gm-sea {
                    div.gm-boats {
                        @for (k, cells) in self.ships[owner].iter().enumerate() {
                            @let sunk = self.sunk(owner, k);
                            @if reveal || sunk {
                                @let across = cells.len() > 1 && cells[1] == cells[0] + 1;
                                @let (r, c, len) = (cells[0] / N + 1, cells[0] % N + 1, cells.len());
                                div.gm-boat.sunk[sunk].new[sunk && self.last.is_some_and(|l| cells.contains(&l))]
                                    style={ "grid-area:" (r) "/" (c) "/span " (if across { 1 } else { len }) "/span " (if across { len } else { 1 }) ";--d:" (k) }
                                    title=(NAMES[k]) { (PreEscaped(boat(k, len, across))) }
                            }
                        }
                    }
                    div.gm-cells {
                        @for i in 0..N * N {
                            @let shot = shots[i];
                            @let sunk = self.ship_at(owner, i).is_some_and(|s| self.sunk(owner, s));
                            @if aim && shot == 0 {
                                button.gm-water type="submit" name="m" value={ "fire:" (i) } aria-label={ "Disparar a " (char::from(b'A' + (i / N) as u8)) (i % N + 1) } {}
                            } @else {
                                span.gm-water.hit[shot == 2].miss[shot == 1].sunk[sunk && shot == 2].last[self.last == Some(i) && shot != 0] {}
                            }
                        }
                    }
                }
            }
        }
    }
    /// Arranging my fleet: tap a ship to pick it, tap a square to take it there, turn it, then "listo".
    /// (app.js fills `{k}` with the picked ship.)
    fn arranging(&self, me: usize, cx: &Ctx) -> Markup {
        use crate::kit::icon;
        html! {
            div.gm-fleet.gm-arrange {
                div.gm-fleet-side {
                    div.gm-fleet-head {
                        h3 { "Coloca tu flota" }
                        span.gm-wait { @if self.ready[1 - me] { (cx.name(1 - me)) " ya está listo" } @else { "Arrastra los barcos; tócalo otra vez para girarlo" } }
                    }
                    div.gm-ocean.placing {
                        span {}
                        div.gm-ax.gm-ax-n { @for c in 1..=N { span { (c) } } }
                        div.gm-ax.gm-ax-l { @for r in 0..N { span { (char::from(b'A' + r as u8)) } } }
                        div.gm-sea {
                            div.gm-cells {
                                @for i in 0..N * N {
                                    button.gm-water type="submit" name="m" value={ "to:{k}:" (i) } aria-label={ "Llevar a " (char::from(b'A' + (i / N) as u8)) (i % N + 1) } {}
                                }
                            }
                            div.gm-boats {
                                @for (k, cells) in self.ships[me].iter().enumerate() {
                                    @let across = Self::across(cells);
                                    @let (r, c, len) = (cells[0] / N + 1, cells[0] % N + 1, cells.len());
                                    label.gm-boat style={ "grid-area:" (r) "/" (c) "/span " (if across { 1 } else { len }) "/span " (if across { len } else { 1 }) ";--d:" (k) } title=(NAMES[k]) {
                                        input type="radio" name="k" value=(k) checked[k == 0] aria-label=(NAMES[k]);
                                        (PreEscaped(boat(k, len, across)))
                                    }
                                }
                            }
                        }
                    }
                    div.gm-arrange-tools {
                        button.btn.outline type="submit" name="m" value="turn:{k}" { (icon("refresh-cw")) "Girar" }
                        button.btn.ghost type="submit" name="m" value="shuffle" { (icon("dices")) "Al azar" }
                        button.btn.primary type="submit" name="m" value="ready" { (icon("check")) "¡Listo!" }
                    }
                }
            }
        }
    }
    /// What is left of `owner`'s fleet, as little ship silhouettes.
    fn left(&self, owner: usize) -> Markup {
        html! {
            div.gm-ships {
                @for (k, s) in self.ships[owner].iter().enumerate() {
                    span.gm-mini.down[self.sunk(owner, k)] style={ "--l:" (s.len()) } title=(NAMES[k]) { (PreEscaped(boat(k, s.len(), true))) }
                }
            }
        }
    }
}

const NAMES: [&str; 5] = ["Acorazado", "Crucero", "Submarino", "Destructor", "Lancha"];

/// A ship seen from above, our own drawing: `k` picks the kind (see `NAMES`), `len` squares long,
/// bow to the right (or down when not `across`).
pub fn boat(k: usize, len: usize, across: bool) -> String {
    let w = len as i32 * 100;
    let mut g = String::new();
    if k == 2 {
        // Submarine: a dark cigar with its tower.
        g += &format!(r##"<rect x="8" y="30" width="{}" height="40" rx="20" fill="#334155" stroke="#1e293b" stroke-width="4"/>"##, w - 16);
        g += &format!(r##"<rect x="{}" y="40" width="56" height="20" rx="10" fill="#475569"/><circle cx="{}" cy="50" r="5" fill="#fbbf24"/>"##, w / 2 - 28, w / 2 + 16);
        g += &format!(r##"<path d="M14 50h-8" stroke="#94a3b8" stroke-width="4" stroke-linecap="round"/><path d="M40 50h{}" stroke="#1e293b" stroke-width="2" opacity=".5"/>"##, w - 90);
    } else {
        let hull = format!("M6 50C6 26 18 16 38 16H{a}C{b} 16 {c} 34 {d} 50C{c} 66 {b} 84 {a} 84H38C18 84 6 74 6 50Z", a = w - 70, b = w - 30, c = w - 10, d = w - 3);
        let deck = format!("M18 50C18 33 26 27 40 27H{a}C{b} 27 {c} 38 {d} 50C{c} 62 {b} 73 {a} 73H40C26 73 18 67 18 50Z", a = w - 74, b = w - 42, c = w - 24, d = w - 16);
        g += &format!(r##"<path d="{hull}" fill="#4b5d73" stroke="#2f3b4b" stroke-width="4"/><path d="{deck}" fill="#e9dcc3"/>"##);
        g += &format!(r##"<path d="M28 40H{e}M24 50H{f}M28 60H{e}" stroke="#b9a684" stroke-width="2" opacity=".6"/>"##, e = w - 40, f = w - 22);
        let turret = |x: i32| format!(r##"<rect x="{x}" y="47" width="30" height="6" rx="3" fill="#475569"/><circle cx="{x}" cy="50" r="14" fill="#64748b" stroke="#334155" stroke-width="3"/>"##);
        let bridge = |x: i32, wd: i32| format!(r##"<rect x="{x}" y="34" width="{wd}" height="32" rx="8" fill="#f8fafc" stroke="#cbd5e1" stroke-width="3"/><rect x="{}" y="39" width="7" height="22" rx="3" fill="#7dd3fc"/>"##, x + wd - 12);
        let funnel = |x: i32| format!(r##"<circle cx="{x}" cy="50" r="10" fill="#e76f51" stroke="#9a3412" stroke-width="3"/><circle cx="{x}" cy="50" r="4" fill="#431407"/>"##);
        match k {
            0 => g += &(turret(70) + &turret(140) + &bridge(190, 70) + &funnel(290) + &turret(w - 90)),
            1 => g += &(turret(60) + &bridge(110, 60) + &funnel(200) + &turret(w - 80)),
            3 => g += &(bridge(40, 50) + &turret(w - 70)),
            _ => g += r##"<rect x="46" y="36" width="64" height="28" rx="10" fill="#f8fafc" stroke="#cbd5e1" stroke-width="3"/><path d="M110 38q18 12 0 24" fill="none" stroke="#7dd3fc" stroke-width="6" stroke-linecap="round"/>"##,
        }
    }
    let (vw, vh, inner) = if across { (w, 100, g) } else { (100, w, format!(r#"<g transform="translate(100 0) rotate(90)">{g}</g>"#)) };
    format!(r#"<svg viewBox="0 0 {vw} {vh}" aria-hidden="true">{inner}</svg>"#)
}

impl Game for Fleet {
    fn new(_: usize, _: &str, rng: &mut Rng) -> Self {
        Fleet { ships: vec![place(rng), place(rng)], shots: vec![vec![0; N * N]; 2], turn: 0, last: None, log: Vec::new(), ready: [false; 2] }
    }
    fn turn(&self) -> usize {
        // While arranging, it is the turn of whoever is not ready yet (so a robot gets to say so).
        self.ready.iter().position(|r| !r).unwrap_or(self.turn)
    }
    fn outcome(&self) -> Option<Outcome> {
        (0..2).find(|&p| (0..self.ships[1 - p].len()).all(|s| self.sunk(1 - p, s))).map(Outcome::Win)
    }
    fn play(&mut self, seat: usize, mv: &str, rng: &mut Rng) -> Result<(), &'static str> {
        if seat > 1 {
            return Err("No juegas en esta partida");
        }
        // Arranging: any time before saying "listo", whoever's turn it is.
        let arranging = !self.ready[seat];
        let mut parts = mv.split(':');
        match (parts.next(), parts.next().and_then(|k| k.parse::<usize>().ok()).filter(|&k| k < SHIPS.len())) {
            (Some("shuffle" | "ready" | "to" | "turn"), _) if !arranging => return Err("Tu flota ya está lista"),
            (Some("shuffle"), _) => {
                self.ships[seat] = place(rng);
                return Ok(());
            }
            (Some("ready"), _) => {
                self.ready[seat] = true;
                push_log(&mut self.log, seat, "tiene su flota lista".into());
                return Ok(());
            }
            (Some("to"), Some(k)) => {
                let at = parts.next().and_then(|n| n.parse().ok()).filter(|&i: &usize| i < N * N).ok_or("Movimiento no válido")?;
                let across = Self::across(&self.ships[seat][k]);
                return self.arrange(seat, k, at, across);
            }
            (Some("turn"), Some(k)) => {
                let (at, across) = (self.ships[seat][k][0], !Self::across(&self.ships[seat][k]));
                // Turn on its first square; if it does not fit there, slide it back along the new line.
                let len = self.ships[seat][k].len();
                return (0..len)
                    .find_map(|back| {
                        let (r, c) = (at / N, at % N);
                        let start = if across { r * N + c.saturating_sub(back) } else { r.saturating_sub(back) * N + c };
                        self.arrange(seat, k, start, across).ok()
                    })
                    .ok_or("No se puede girar ahí: no hay sitio");
            }
            _ => {}
        }
        if self.ready != [true, true] {
            return Err("Aún se están colocando las flotas");
        }
        if seat != self.turn {
            return Err("No es tu turno");
        }
        let i: usize = mv.strip_prefix("fire:").and_then(|n| n.parse().ok()).filter(|&i| i < N * N).ok_or("Movimiento no válido")?;
        if self.shots[seat][i] != 0 {
            return Err("Ahí ya has disparado");
        }
        let target = 1 - seat;
        self.last = Some(i);
        let cell = format!("{}{}", char::from(b'A' + (i / N) as u8), i % N + 1);
        match self.ship_at(target, i) {
            Some(s) => {
                self.shots[seat][i] = 2;
                let text = if self.sunk(target, s) { format!("dispara a {cell}: ¡hundido!") } else { format!("dispara a {cell}: tocado") };
                push_log(&mut self.log, seat, text); // and shoots again
            }
            None => {
                self.shots[seat][i] = 1;
                push_log(&mut self.log, seat, format!("dispara a {cell}: agua"));
                self.turn = target;
            }
        }
        Ok(())
    }
    fn bot(&self, seat: usize, rng: &mut Rng) -> String {
        if !self.ready[seat] {
            return "ready".into(); // its random fleet is fine
        }
        let shots = &self.shots[seat];
        let target = 1 - seat;
        // Hits on ships still afloat (sinking is public): fire around them, along the line if there is one.
        let open: Vec<usize> = (0..N * N).filter(|&i| shots[i] == 2 && self.ship_at(target, i).is_some_and(|s| !self.sunk(target, s))).collect();
        let mut aims: Vec<usize> = Vec::new();
        if open.len() >= 2 {
            let across = open[0] / N == open[1] / N;
            for &h in &open {
                aims.extend(neighbours(h).filter(|&n| shots[n] == 0 && if across { n / N == h / N } else { n % N == h % N }));
            }
        }
        if aims.is_empty() {
            for &h in &open {
                aims.extend(neighbours(h).filter(|&n| shots[n] == 0));
            }
        }
        if aims.is_empty() {
            // Hunting: every other square is enough to find ships of two or more.
            aims = (0..N * N).filter(|&i| shots[i] == 0 && (i / N + i % N) % 2 == 0).collect();
        }
        if aims.is_empty() {
            aims = (0..N * N).filter(|&i| shots[i] == 0).collect();
        }
        format!("fire:{}", aims[rng.below(aims.len())])
    }
    fn view(&self, cx: &Ctx) -> Markup {
        let playing = self.outcome().is_none();
        match cx.me.filter(|&s| s < 2) {
            Some(me) if !self.ready[me] => self.arranging(me, cx),
            Some(me) => html! {
                div.gm-fleet {
                    div.gm-fleet-side.target {
                        div.gm-fleet-head {
                            h3 { "Aguas de " (cx.name(1 - me)) }
                            @if self.ready[1 - me] { (self.left(1 - me)) } @else { span.gm-wait { "Colocando su flota…" } }
                        }
                        (self.grid(1 - me, !playing, playing && cx.my_turn(self.turn)))
                    }
                    div.gm-fleet-side.mine {
                        div.gm-fleet-head {
                            h3 { "Tu flota" }
                            (self.left(me))
                        }
                        (self.grid(me, true, false))
                    }
                }
            },
            None => html! {
                div.gm-fleet {
                    @for p in 0..2 {
                        div.gm-fleet-side { div.gm-fleet-head { h3 { "Aguas de " (cx.name(p)) } (self.left(p)) } (self.grid(p, !playing, false)) }
                    }
                }
            },
        }
    }
    fn log(&self) -> Vec<(usize, String)> {
        self.log.clone()
    }
    fn hint(&self, seat: usize) -> String {
        if self.ready.get(seat) == Some(&false) { "Coloca tus barcos y dale a Listo".into() } else { String::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arranging_moves_and_turns() {
        let mut rng = Rng::new();
        let mut g = Fleet::new(2, "", &mut rng);
        g.ships[0] = vec![vec![0, 1, 2, 3], vec![16, 17, 18], vec![32, 33, 34], vec![48, 49], vec![60, 61]];
        assert!(g.play(0, "fire:0", &mut rng).is_err(), "no shots while arranging");
        g.play(0, "to:3:7", &mut rng).unwrap(); // sticks out on the right: pulled back to A7-A8
        assert_eq!(g.ships[0][3], vec![6, 7]);
        assert!(g.play(0, "to:3:2", &mut rng).is_err(), "would sit on the big one");
        g.play(0, "turn:3", &mut rng).unwrap();
        assert_eq!(g.ships[0][3], vec![6, 14]);
        assert_eq!(g.turn(), 0);
        g.play(0, "ready", &mut rng).unwrap();
        assert!(g.play(0, "shuffle", &mut rng).is_err());
        assert_eq!(g.turn(), 1, "the other one is still arranging");
        let mv = g.bot(1, &mut rng);
        g.play(1, &mv, &mut rng).unwrap();
        assert_eq!(g.turn(), 0);
        g.play(0, "fire:0", &mut rng).unwrap();
    }
}
