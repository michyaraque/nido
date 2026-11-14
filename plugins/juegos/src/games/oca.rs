//! La oca: 63 squares and one die, with the classic squares and their sayings.
use super::*;
use maud::html;
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
    fn new(players: usize, _: &mut Rng) -> Self {
        Oca { pos: vec![0; players], skip: vec![0; players], turn: 0, die: None, moved: None, winner: None, log: Vec::new() }
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
        // A snake of 8x8: square 0 bottom-left, rows alternate direction, 63 top-left.
        let order: Vec<u8> = (0..8u8).rev().flat_map(|row| {
            let cols: Vec<u8> = if row % 2 == 0 { (0..8).collect() } else { (0..8).rev().collect() };
            cols.into_iter().map(move |c| row * 8 + c)
        }).collect();
        let playing = self.winner.is_none();
        html! {
            div.gm-oca-wrap {
                div.gm-oca {
                    @for n in order {
                        @let sp = special(n);
                        div.gm-sq.sp[sp.is_some()].goose[OCAS.contains(&n)].end[n == END].bad[matches!(n, 19 | 31 | 42 | 52 | 58)] title=[sp.map(|s| s.1)] {
                            span.gm-sq-n { (n) }
                            @if let Some((ic, _)) = sp { span.gm-sq-ico { (crate::kit::icon(ic)) } }
                            span.gm-tokens {
                                @for (s, &p) in self.pos.iter().enumerate() {
                                    @if p == n {
                                        @if let Some(src) = cx.photo(s) {
                                            img.gm-token.face.moved[self.moved == Some(s)] src=(src) alt="" title=(cx.name(s)) style={ "border-color:" (cx.color(s)) };
                                        } @else {
                                            i.gm-token.moved[self.moved == Some(s)] style={ "background:" (cx.color(s)) } title=(cx.name(s)) {}
                                        }
                                    }
                                }
                            }
                        }
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
            let mut g = Oca::new(2, &mut Rng::seeded(1));
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
    }
}
