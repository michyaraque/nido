//! Dominó, doble seis: 28 tiles, 7 each. A tile goes at either end of the line when one of its
//! numbers matches that end. Without a tile to lay you draw from the pool, and pass once it is
//! empty. Whoever runs out of tiles wins; if nobody can lay, the lowest hand wins.
use super::*;
use maud::{html, PreEscaped};
use serde::{Deserialize, Serialize};

type Tile = (u8, u8);

#[derive(Serialize, Deserialize)]
pub struct Domino {
    hands: Vec<Vec<Tile>>,
    pool: Vec<Tile>,
    line: Vec<Tile>, // left to right, each tile's right number meets the next one's left
    turn: usize,
    passes: usize,
    last: Option<Tile>,
    log: Vec<(usize, String)>,
}

fn pips(t: Tile) -> u32 {
    t.0 as u32 + t.1 as u32
}

impl Domino {
    fn ends(&self) -> Option<(u8, u8)> {
        Some((self.line.first()?.0, self.line.last()?.1))
    }
    /// Where tile `t` can go: left and/or right (any end while the line is empty).
    fn sides(&self, t: Tile) -> (bool, bool) {
        match self.ends() {
            None => (false, true),
            Some((l, r)) => (t.0 == l || t.1 == l, t.0 == r || t.1 == r),
        }
    }
    fn can_lay(&self, seat: usize) -> bool {
        self.hands[seat].iter().any(|&t| {
            let (l, r) = self.sides(t);
            l || r
        })
    }
    fn next(&mut self) {
        self.turn = (self.turn + 1) % self.hands.len();
    }
}

impl Game for Domino {
    fn new(players: usize, _: &str, rng: &mut Rng) -> Self {
        let mut tiles: Vec<Tile> = (0..=6).flat_map(|a| (a..=6).map(move |b| (a, b))).collect();
        rng.shuffle(&mut tiles);
        let hands: Vec<Vec<Tile>> = (0..players).map(|_| tiles.split_off(tiles.len() - 7)).collect();
        // The highest double starts; without doubles, the highest tile.
        let rank = |t: &Tile| if t.0 == t.1 { 100 + t.0 as u32 } else { pips(*t) };
        let turn = (0..players).max_by_key(|&s| hands[s].iter().map(rank).max().unwrap_or(0)).unwrap_or(0);
        Domino { hands, pool: tiles, line: vec![], turn, passes: 0, last: None, log: vec![] }
    }
    fn hint(&self, seat: usize) -> String {
        match (self.hands.get(seat).is_some() && self.can_lay(seat), self.pool.is_empty()) {
            (true, _) => String::new(),
            (false, false) => "No tienes ficha: roba del pozo".into(),
            (false, true) => "No puedes poner: pasa".into(),
        }
    }
    fn turn(&self) -> usize {
        self.turn
    }
    fn outcome(&self) -> Option<Outcome> {
        if let Some(s) = self.hands.iter().position(Vec::is_empty) {
            return Some(Outcome::Win(s));
        }
        if self.passes < self.hands.len() {
            return None;
        }
        let low = self.hands.iter().map(|h| h.iter().map(|&t| pips(t)).sum::<u32>()).collect::<Vec<_>>();
        let min = *low.iter().min()?;
        let who: Vec<usize> = (0..low.len()).filter(|&s| low[s] == min).collect();
        Some(if who.len() == 1 { Outcome::Win(who[0]) } else { Outcome::Draw })
    }
    fn play(&mut self, seat: usize, mv: &str, _: &mut Rng) -> Result<(), &'static str> {
        if seat != self.turn {
            return Err("No es tu turno");
        }
        match mv {
            "draw" => {
                if self.can_lay(seat) {
                    return Err("Tienes una ficha que poner");
                }
                let t = self.pool.pop().ok_or("El pozo está vacío")?;
                self.hands[seat].push(t);
                push_log(&mut self.log, seat, "roba una ficha".into());
            }
            "pass" => {
                if self.can_lay(seat) || !self.pool.is_empty() {
                    return Err("Todavía puedes jugar");
                }
                self.passes += 1;
                push_log(&mut self.log, seat, "pasa".into());
                self.next();
            }
            _ => {
                let mut it = mv.strip_prefix("lay:").ok_or("Movimiento no válido")?.split(':');
                let i: usize = it.next().and_then(|n| n.parse().ok()).ok_or("Movimiento no válido")?;
                let left = it.next() == Some("l");
                let t = *self.hands[seat].get(i).ok_or("Esa ficha no es tuya")?;
                let (fits_l, fits_r) = self.sides(t);
                if left && fits_l {
                    let end = self.line[0].0;
                    self.line.insert(0, if t.1 == end { t } else { (t.1, t.0) });
                } else if !left && fits_r {
                    match self.ends() {
                        None => self.line.push(t),
                        Some((_, end)) => self.line.push(if t.0 == end { t } else { (t.1, t.0) }),
                    }
                } else {
                    return Err("Esa ficha no casa ahí");
                }
                self.hands[seat].remove(i);
                self.passes = 0;
                self.last = Some(t);
                push_log(&mut self.log, seat, format!("pone el {}·{}", t.0, t.1));
                if !self.hands[seat].is_empty() {
                    self.next();
                }
            }
        }
        Ok(())
    }
    fn bot(&self, seat: usize, _: &mut Rng) -> String {
        // Heaviest tile first (doubles a bit earlier): fewer points left if the game blocks.
        let best = self.hands[seat]
            .iter()
            .enumerate()
            .filter(|(_, &t)| {
                let (l, r) = self.sides(t);
                l || r
            })
            .max_by_key(|(_, &t)| pips(t) * 2 + u32::from(t.0 == t.1) * 3);
        match best {
            Some((i, &t)) => format!("lay:{i}:{}", if self.sides(t).1 { "r" } else { "l" }),
            None if !self.pool.is_empty() => "draw".into(),
            None => "pass".into(),
        }
    }
    fn view(&self, cx: &Ctx) -> Markup {
        let playing = self.outcome().is_none();
        let mine = playing && cx.my_turn(self.turn);
        let me = cx.me.filter(|&s| s < self.hands.len());
        html! {
            div.gm-domino {
                div.gm-dom-others {
                    @for s in (0..self.hands.len()).filter(|&s| Some(s) != me) {
                        span.gm-dom-other.on[playing && self.turn == s] style={ "--c:" (cx.color(s)) } {
                            (cx.face(s))
                            span.gm-dom-backs { @for _ in 0..self.hands[s].len().min(10) { i {} } }
                            b { (self.hands[s].len()) }
                        }
                    }
                    span.gm-dom-pool title="Fichas en el pozo" { (crate::kit::icon("package")) b { (self.pool.len()) } small { "en el pozo" } }
                }
                div.gm-dom-table {
                    @if self.line.is_empty() {
                        p.gm-dom-empty { "La mesa está vacía: empieza " (cx.name(self.turn)) }
                    }
                    div.gm-dom-line {
                        @if mine && !self.line.is_empty() { span.gm-dom-drop data-side="l" aria-hidden="true" {} }
                        @for &t in &self.line {
                            (tile(t, t.0 == t.1, self.last.is_some_and(|l| l == t || l == (t.1, t.0))))
                        }
                        @if mine { span.gm-dom-drop data-side="r" aria-hidden="true" {} }
                    }
                }
                @if let Some(me) = me {
                    div.gm-dom-hand {
                        @for (i, &t) in self.hands[me].iter().enumerate() {
                            @let (l, r) = self.sides(t);
                            @if mine && (l || r) {
                                span.gm-dom-pick data-i=(i) data-l=(if l { "1" } else { "0" }) data-r=(if r { "1" } else { "0" }) {
                                    @if l && r && self.ends().is_some_and(|(a, b)| a != b) {
                                        button.gm-dom-end type="submit" name="m" value={ "lay:" (i) ":l" } aria-label="Poner a la izquierda" { (crate::kit::icon("chevron-left")) }
                                        (tile(t, true, false))
                                        button.gm-dom-end type="submit" name="m" value={ "lay:" (i) ":r" } aria-label="Poner a la derecha" { (crate::kit::icon("chevron-right")) }
                                    } @else {
                                        button.gm-dom-play type="submit" name="m" value={ "lay:" (i) ":" (if r { "r" } else { "l" }) } aria-label={ "Poner el " (t.0) "·" (t.1) } { (tile(t, true, false)) }
                                    }
                                }
                            } @else {
                                span.gm-dom-idle.off[mine] { (tile(t, true, false)) }
                            }
                        }
                    }
                    @if mine && !self.can_lay(me) {
                        div.gm-dom-actions {
                            @if self.pool.is_empty() {
                                button.btn.outline type="submit" name="m" value="pass" { (crate::kit::icon("arrow-right")) "Pasar" }
                            } @else {
                                button.btn.primary type="submit" name="m" value="draw" { (crate::kit::icon("package")) "Robar del pozo" }
                            }
                        }
                    }
                }
            }
        }
    }
    fn log(&self) -> Vec<(usize, String)> {
        self.log.clone()
    }
}

/// Pip positions on a 3x3 grid for 0-6.
fn spots(n: u8) -> &'static [usize] {
    match n {
        1 => &[4],
        2 => &[0, 8],
        3 => &[0, 4, 8],
        4 => &[0, 2, 6, 8],
        5 => &[0, 2, 4, 6, 8],
        6 => &[0, 2, 3, 5, 6, 8],
        _ => &[],
    }
}

const PIP: [&str; 7] = ["", "#e11d48", "#ea580c", "#16a34a", "#0284c7", "#7c3aed", "#334155"];

/// A tile drawn upright (`up`) or lying, ivory with coloured pips.
pub fn tile(t: Tile, up: bool, last: bool) -> Markup {
    let half = |n: u8, ox: f32, oy: f32| -> String {
        spots(n)
            .iter()
            .map(|&k| format!(r##"<circle cx="{:.1}" cy="{:.1}" r="3.4" fill="{}"/>"##, ox + 8.0 + (k % 3) as f32 * 9.0, oy + 8.0 + (k / 3) as f32 * 9.0, PIP[n as usize]))
            .collect()
    };
    let (w, h, a, b, line) = if up {
        (34, 66, half(t.0, 0.0, 0.0), half(t.1, 0.0, 32.0), r##"<path d="M5 33h24" stroke="#d6cfc2" stroke-width="1.5" stroke-linecap="round"/>"##)
    } else {
        (66, 34, half(t.0, 0.0, 0.0), half(t.1, 32.0, 0.0), r##"<path d="M33 5v24" stroke="#d6cfc2" stroke-width="1.5" stroke-linecap="round"/>"##)
    };
    html! {
        svg class={ "gm-tile" @if up { " up" } @if last { " last" } } viewBox={ "0 0 " (w) " " (h) } aria-label={ (t.0) "·" (t.1) } {
            (PreEscaped(format!(r##"<rect x="1" y="1" width="{}" height="{}" rx="7" fill="#fffaf0" stroke="#e7dfd0" stroke-width="1.5"/>{line}{a}{b}"##, w - 2, h - 2)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn laying_drawing_and_passing() {
        let mut rng = Rng::seeded(9);
        let mut g = Domino::new(2, "", &mut rng);
        assert_eq!((g.hands[0].len(), g.hands[1].len(), g.pool.len()), (7, 7, 14));
        g.hands = vec![vec![(6, 6), (6, 2), (1, 1)], vec![(2, 3), (4, 5)]];
        g.turn = 0;
        g.play(0, "lay:0:r", &mut rng).unwrap();
        g.play(1, "draw", &mut rng).unwrap_or_default();
        assert!(g.play(1, "pass", &mut rng).is_err(), "cannot pass while the pool has tiles");
        g.pool.clear();
        g.hands[1] = vec![(2, 3), (4, 5)];
        g.play(1, "pass", &mut rng).unwrap();
        g.play(0, "lay:0:l", &mut rng).unwrap();
        assert_eq!(g.line, vec![(2, 6), (6, 6)], "the 6·2 turns round to meet the 6");
        g.play(1, "lay:0:l", &mut rng).unwrap();
        assert_eq!(g.line[0], (3, 2));
        assert_eq!(g.turn, 0);
        assert!(g.play(0, "lay:0:l", &mut rng).is_err(), "the 1·1 fits nowhere");
    }
}
