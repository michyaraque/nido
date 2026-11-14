//! Piedra, papel o tijera: both choose at the same time (nobody sees the other's pick until both
//! have chosen), and the first to three points wins. A tie is played again.
use super::*;
use maud::{html, PreEscaped};
use serde::{Deserialize, Serialize};

const TO_WIN: u8 = 3;
/// (move, drawing, name). The drawings are ours: a stone, a sheet and a pair of scissors.
const HANDS: [(&str, &str, &str); 3] = [("rock", ROCK, "piedra"), ("paper", PAPER, "papel"), ("scissors", SCISSORS, "tijeras")];

pub const ROCK: &str = r##"<svg viewBox="0 0 64 64" aria-hidden="true"><path d="M12 38c-2-9 3-18 11-22l9-4c8-3 17 1 20 9l3 8c3 9-2 18-11 21l-9 2c-11 2-20-3-23-14z" fill="#a8a29e"/><path d="M14 38c-1-8 4-15 11-19l8-3c6-2 12 0 15 5" fill="none" stroke="#d6d3d1" stroke-width="3" stroke-linecap="round"/><path d="M30 30l6 5-3 7m6-7 7-3" fill="none" stroke="#78716c" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/><ellipse cx="32" cy="58" rx="18" ry="2.5" fill="#000" opacity=".12"/></svg>"##;
pub const PAPER: &str = r##"<svg viewBox="0 0 64 64" aria-hidden="true"><path d="M16 8h24l12 12v34a3 3 0 0 1-3 3H16a3 3 0 0 1-3-3V11a3 3 0 0 1 3-3z" fill="#fafaf9" stroke="#d6d3d1" stroke-width="2"/><path d="M40 8v9a3 3 0 0 0 3 3h9" fill="#e7e5e4" stroke="#d6d3d1" stroke-width="2" stroke-linejoin="round"/><path d="M21 28h22M21 35h22M21 42h15" stroke="#93c5fd" stroke-width="2.5" stroke-linecap="round"/><ellipse cx="32" cy="60" rx="17" ry="2.5" fill="#000" opacity=".1"/></svg>"##;
pub const SCISSORS: &str = r##"<svg viewBox="0 0 64 64" aria-hidden="true"><path d="M27 34 52 9" stroke="#cbd5e1" stroke-width="5" stroke-linecap="round"/><path d="M37 34 12 9" stroke="#e2e8f0" stroke-width="5" stroke-linecap="round"/><circle cx="32" cy="32" r="2.5" fill="#64748b"/><circle cx="20" cy="47" r="9" fill="none" stroke="#f97316" stroke-width="5"/><circle cx="44" cy="47" r="9" fill="none" stroke="#ef4444" stroke-width="5"/><path d="M26 40l3-4m9 4-3-4" stroke="#f97316" stroke-width="5" stroke-linecap="round"/></svg>"##;

#[derive(Serialize, Deserialize)]
pub struct Rps {
    pick: [Option<u8>; 2],     // this round, hidden until both have chosen
    last: Option<[u8; 2]>,     // the round just played, shown to everyone
    score: [u8; 2],
    round: u8,
    log: Vec<(usize, String)>,
}

/// 0 tie, 1 seat 0 wins, 2 seat 1 wins.
fn beats(a: u8, b: u8) -> u8 {
    match (a + 3 - b) % 3 {
        0 => 0,
        1 => 1,
        _ => 2,
    }
}

impl Game for Rps {
    fn new(_: usize, _: &mut Rng) -> Self {
        Rps { pick: [None, None], last: None, score: [0, 0], round: 1, log: Vec::new() }
    }
    /// Whoever still has to choose (seat 0 first): both can, in any order.
    fn turn(&self) -> usize {
        if self.pick[0].is_none() { 0 } else { 1 }
    }
    fn outcome(&self) -> Option<Outcome> {
        (0..2).find(|&s| self.score[s] >= TO_WIN).map(Outcome::Win)
    }
    fn play(&mut self, seat: usize, mv: &str, _: &mut Rng) -> Result<(), &'static str> {
        if seat > 1 {
            return Err("No juegas en esta partida");
        }
        let hand = HANDS.iter().position(|h| h.0 == mv).ok_or("Movimiento no válido")? as u8;
        if self.pick[seat].is_some() {
            return Err("Ya has elegido: espera al otro");
        }
        self.pick[seat] = Some(hand);
        if let [Some(a), Some(b)] = self.pick {
            let said = format!("{} contra {}", HANDS[a as usize].2, HANDS[b as usize].2);
            match beats(a, b) {
                0 => push_log(&mut self.log, 0, format!("empata la ronda {}: {said}", self.round)),
                w => {
                    let w = w as usize - 1;
                    self.score[w] += 1;
                    push_log(&mut self.log, w, format!("gana la ronda {}: {said}", self.round));
                }
            }
            self.last = Some([a, b]);
            self.pick = [None, None];
            self.round += 1;
        }
        Ok(())
    }
    fn bot(&self, _: usize, rng: &mut Rng) -> String {
        HANDS[rng.below(3)].0.to_string()
    }
    fn view(&self, cx: &Ctx) -> Markup {
        let me = cx.me.filter(|&s| s < 2);
        let mine = me.and_then(|s| self.pick[s]);
        let waiting_other = me.is_some_and(|s| self.pick[s].is_some() && self.pick[1 - s].is_none());
        let playing = self.outcome().is_none();
        html! {
            div.gm-rps {
                div.gm-rps-score {
                    @for s in 0..2 {
                        span.gm-rps-side style={ "--c:" (cx.color(s)) } {
                            b { (self.score[s]) }
                            small { (cx.name(s)) }
                            span.gm-rps-pips { @for k in 0..TO_WIN { i.on[k < self.score[s]] {} } }
                        }
                    }
                }
                @if let Some([a, b]) = self.last {
                    // The reveal: both hands shake, then open (CSS). Keyed by round so it replays only when new.
                    div.gm-rps-reveal data-round=(self.round) {
                        span.gm-hand-big.l.win[beats(a, b) == 1] { (PreEscaped(HANDS[a as usize].1)) }
                        span.gm-vs { "vs" }
                        span.gm-hand-big.r.win[beats(a, b) == 2] { (PreEscaped(HANDS[b as usize].1)) }
                    }
                } @else {
                    div.gm-rps-reveal {} // keeps its room
                }
                @if playing {
                    @if let Some(me) = me {
                        @if waiting_other {
                            p.gm-rps-wait { span.gm-hand-mini { (PreEscaped(HANDS[mine.unwrap_or(0) as usize].1)) } "Has elegido " (HANDS[mine.unwrap_or(0) as usize].2) ". Esperando a " (cx.name(1 - me)) "…" }
                        } @else {
                            div.gm-rps-pick {
                                @for (id, emoji, name) in HANDS {
                                    button.gm-rps-btn type="submit" name="m" value=(id) aria-label=(name) { span { (PreEscaped(emoji)) } small { (name) } }
                                }
                            }
                        }
                    } @else {
                        p.gm-rps-wait { "Ronda " (self.round) }
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
    fn hidden_picks_and_score() {
        let mut rng = Rng::seeded(1);
        let mut g = Rps::new(2, &mut rng);
        assert_eq!((beats(0, 2), beats(2, 0), beats(1, 1)), (1, 2, 0), "rock beats scissors");
        g.play(1, "paper", &mut rng).unwrap(); // the second player may choose first
        assert!(g.play(1, "rock", &mut rng).is_err(), "only once per round");
        assert!(g.last.is_none(), "nothing revealed yet");
        g.play(0, "rock", &mut rng).unwrap();
        assert_eq!((g.score, g.last), ([0, 1], Some([0, 1])));
        for _ in 0..2 {
            g.play(0, "scissors", &mut rng).unwrap();
            g.play(1, "paper", &mut rng).unwrap();
        }
        g.play(0, "rock", &mut rng).unwrap();
        g.play(1, "scissors", &mut rng).unwrap();
        assert_eq!(g.outcome(), Some(Outcome::Win(0)));
    }
}
