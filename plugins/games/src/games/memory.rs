//! Memory: cards face down in pairs. Turn two over: a pair stays yours and you play again; two
//! different cards stay face up until the next player turns one, so everyone can remember them.
//! The difficulty sets the number of cards and how much the robot remembers.
use super::*;
use maud::html;
use serde::{Deserialize, Serialize};

/// Card faces: an icon, its colour and how a pair of them is called.
const FACES: [(&str, &str, &str); 15] = [
    ("cat", "#f97316", "gatos"),
    ("dog", "#a16207", "perros"),
    ("bird", "#0ea5e9", "pájaros"),
    ("fish", "#0891b2", "peces"),
    ("apple", "#ef4444", "manzanas"),
    ("banana", "#ca8a04", "plátanos"),
    ("carrot", "#ea580c", "zanahorias"),
    ("coffee", "#78350f", "cafés"),
    ("cookie", "#b45309", "galletas"),
    ("flower-2", "#ec4899", "flores"),
    ("sun", "#f59e0b", "soles"),
    ("moon", "#6366f1", "lunas"),
    ("heart", "#e11d48", "corazones"),
    ("pizza", "#dc2626", "pizzas"),
    ("ice-cream-cone", "#db2777", "helados"),
];

/// (option, pairs, columns, how often the robot remembers a card it has seen, in %).
const LEVELS: [(&str, usize, usize, usize); 3] = [("easy", 6, 4, 45), ("normal", 10, 5, 70), ("hard", 15, 6, 95)];

#[derive(Serialize, Deserialize)]
pub struct Memory {
    level: usize,
    cards: Vec<u8>,
    found: Vec<Option<usize>>,
    up: Vec<usize>,
    miss: Vec<usize>,
    seen: Vec<bool>,
    last: Option<usize>,
    turn: usize,
    pairs: Vec<u32>,
    log: Vec<(usize, String)>,
}

impl Memory {
    fn face_up(&self, i: usize) -> bool {
        self.found[i].is_some() || self.up.contains(&i) || self.miss.contains(&i)
    }
    fn free(&self, i: usize) -> bool {
        self.found[i].is_none() && !self.up.contains(&i)
    }
    /// A card the robot has seen and still remembers with this face, other than `but`.
    fn recall(&self, face: u8, but: Option<usize>, sure: bool) -> Option<usize> {
        if !sure {
            return None;
        }
        (0..self.cards.len()).find(|&i| self.seen[i] && self.free(i) && self.cards[i] == face && Some(i) != but)
    }
    fn unseen_or_any(&self, rng: &mut Rng) -> usize {
        let fresh: Vec<usize> = (0..self.cards.len()).filter(|&i| self.free(i) && !self.seen[i]).collect();
        let pool = if fresh.is_empty() { (0..self.cards.len()).filter(|&i| self.free(i)).collect() } else { fresh };
        pool[rng.below(pool.len())]
    }
}

impl Game for Memory {
    fn new(players: usize, opt: &str, rng: &mut Rng) -> Self {
        let level = LEVELS.iter().position(|l| l.0 == opt).unwrap_or(0);
        let mut faces: Vec<u8> = (0..FACES.len() as u8).collect();
        rng.shuffle(&mut faces);
        let mut cards: Vec<u8> = faces[..LEVELS[level].1].iter().flat_map(|&f| [f, f]).collect();
        rng.shuffle(&mut cards);
        let n = cards.len();
        Memory { level, cards, found: vec![None; n], up: vec![], miss: vec![], seen: vec![false; n], last: None, turn: 0, pairs: vec![0; players], log: vec![] }
    }
    fn opt(&self) -> String {
        LEVELS[self.level].0.to_string()
    }
    fn hint(&self, _seat: usize) -> String {
        if self.up.len() == 1 { "Levanta otra carta".into() } else { String::new() }
    }
    fn turn(&self) -> usize {
        self.turn
    }
    fn outcome(&self) -> Option<Outcome> {
        if self.found.iter().any(Option::is_none) {
            return None;
        }
        let best = *self.pairs.iter().max()?;
        let leaders: Vec<usize> = (0..self.pairs.len()).filter(|&s| self.pairs[s] == best).collect();
        Some(if leaders.len() == 1 { Outcome::Win(leaders[0]) } else { Outcome::Draw })
    }
    fn play(&mut self, seat: usize, mv: &str, _: &mut Rng) -> Result<(), &'static str> {
        if seat != self.turn {
            return Err("No es tu turno");
        }
        let i: usize = mv.strip_prefix("flip:").and_then(|n| n.parse().ok()).ok_or("Movimiento no válido")?;
        if i >= self.cards.len() || !self.free(i) {
            return Err("Esa carta ya está boca arriba");
        }
        if self.up.is_empty() {
            self.miss.clear();
        }
        self.up.push(i);
        self.seen[i] = true;
        self.last = Some(i);
        if self.up.len() == 2 {
            let (a, b) = (self.up[0], self.up[1]);
            if self.cards[a] == self.cards[b] {
                self.found[a] = Some(seat);
                self.found[b] = Some(seat);
                self.pairs[seat] += 1;
                push_log(&mut self.log, seat, format!("encuentra la pareja de {}", FACES[self.cards[a] as usize].2));
            } else {
                self.miss = vec![a, b];
                self.turn = (seat + 1) % self.pairs.len();
                push_log(&mut self.log, seat, "no encuentra pareja".into());
            }
            self.up.clear();
        }
        Ok(())
    }
    fn bot(&self, _seat: usize, rng: &mut Rng) -> String {
        let memory = LEVELS[self.level].3;
        let sure = rng.below(100) < memory;
        let pick = match self.up.first() {
            Some(&first) => self.recall(self.cards[first], Some(first), sure).unwrap_or_else(|| self.unseen_or_any(rng)),
            None => (0..self.cards.len())
                .filter(|&i| self.seen[i] && self.free(i))
                .find(|&i| self.recall(self.cards[i], Some(i), sure).is_some())
                .unwrap_or_else(|| self.unseen_or_any(rng)),
        };
        format!("flip:{pick}")
    }
    fn view(&self, cx: &Ctx) -> Markup {
        let mine = self.outcome().is_none() && cx.my_turn(self.turn);
        let cols = LEVELS[self.level].2;
        html! {
            div.gm-memory {
                div.gm-mem-score {
                    @for s in 0..self.pairs.len() {
                        span.gm-mem-chip.on[self.outcome().is_none() && self.turn == s] style={ "--c:" (cx.color(s)) } {
                            (cx.face(s)) b { (self.pairs[s]) } small { @if self.pairs[s] == 1 { "pareja" } @else { "parejas" } }
                        }
                    }
                }
                div.gm-mem-grid style={ "--cols:" (cols) } {
                    @for i in 0..self.cards.len() {
                        @let up = self.face_up(i);
                        @let (ic, color, _) = FACES[self.cards[i] as usize];
                        @let owner = self.found[i];
                        @let class = format!("gm-mem-card{}{}{}{}", if up { " up" } else { "" }, if owner.is_some() { " found" } else { "" }, if self.miss.contains(&i) { " miss" } else { "" }, if self.last == Some(i) { " just" } else { "" });
                        @let style = format!("--f:{color};--o:{}", owner.map_or("transparent".to_string(), |o| cx.color(o).to_string()));
                        @if mine && !up {
                            button class=(class) style=(style) type="submit" name="m" value={ "flip:" (i) } aria-label={ "Carta " (i + 1) } {
                                span.gm-mem-back {} span.gm-mem-face {}
                            }
                        } @else {
                            span class=(class) style=(style) aria-label=[up.then_some(FACES[self.cards[i] as usize].2)] {
                                span.gm-mem-back {}
                                span.gm-mem-face { @if up { (crate::kit::icon(ic)) } }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_keep_the_turn_and_misses_pass_it() {
        let mut rng = Rng::seeded(5);
        let mut g = Memory::new(2, "easy", &mut rng);
        assert_eq!((g.cards.len(), g.opt().as_str()), (12, "easy"));
        let a = 0;
        let b = (1..12).find(|&i| g.cards[i] == g.cards[a]).unwrap();
        let other = (1..12).find(|&i| g.cards[i] != g.cards[a]).unwrap();
        g.play(0, &format!("flip:{a}"), &mut rng).unwrap();
        assert!(g.play(0, &format!("flip:{a}"), &mut rng).is_err(), "the same card twice");
        g.play(0, &format!("flip:{b}"), &mut rng).unwrap();
        assert_eq!((g.turn, g.pairs[0]), (0, 1), "a pair keeps the turn");
        let c = (0..12).find(|&i| g.found[i].is_none() && g.cards[i] != g.cards[other] && i != other).unwrap();
        g.play(0, &format!("flip:{other}"), &mut rng).unwrap();
        g.play(0, &format!("flip:{c}"), &mut rng).unwrap();
        assert_eq!((g.turn, g.miss.len()), (1, 2), "a miss passes the turn and stays visible");
        let mv = g.bot(1, &mut rng);
        g.play(1, &mv, &mut rng).unwrap();
        assert!(g.miss.is_empty(), "the next flip hides the miss");
        assert_eq!(Memory::new(3, "hard", &mut rng).cards.len(), 30);
    }
}
