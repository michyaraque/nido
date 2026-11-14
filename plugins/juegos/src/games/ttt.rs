//! Tres en raya. The robot plays perfectly (full minimax: nine squares are nothing).
use super::*;
use maud::html;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Ttt {
    cells: Vec<u8>, // 0 empty, 1 seat 0, 2 seat 1
    turn: usize,
    last: Option<usize>,
}

const LINES: [[usize; 3]; 8] = [[0, 1, 2], [3, 4, 5], [6, 7, 8], [0, 3, 6], [1, 4, 7], [2, 5, 8], [0, 4, 8], [2, 4, 6]];

fn line(c: &[u8]) -> Option<(u8, [usize; 3])> {
    LINES.iter().find(|l| c[l[0]] != 0 && c[l[0]] == c[l[1]] && c[l[1]] == c[l[2]]).map(|l| (c[l[0]], *l))
}

/// Score for `me` with `next` to play: +10 win, -10 loss, sooner is better.
fn minimax(c: &mut [u8], me: u8, next: u8, depth: i32) -> i32 {
    if let Some((w, _)) = line(c) {
        return if w == me { 10 - depth } else { depth - 10 };
    }
    if c.iter().all(|&v| v != 0) {
        return 0;
    }
    let free: Vec<usize> = (0..9).filter(|&i| c[i] == 0).collect();
    let scores = free.into_iter().map(|i| {
        c[i] = next;
        let s = minimax(c, me, 3 - next, depth + 1);
        c[i] = 0;
        s
    });
    if next == me { scores.max().unwrap_or(0) } else { scores.min().unwrap_or(0) }
}

/// The two marks, drawn in each player's colour.
pub fn mark(seat: usize, color: &str) -> Markup {
    html! {
        svg.gm-mark viewBox="0 0 40 40" style={ "color:" (color) } aria-hidden="true" {
            @if seat == 0 { path d="M11 11 29 29M29 11 11 29" {} } @else { circle cx="20" cy="20" r="11" {} }
        }
    }
}

impl Game for Ttt {
    fn new(_: usize, _: &mut Rng) -> Self {
        Ttt { cells: vec![0; 9], turn: 0, last: None }
    }
    fn turn(&self) -> usize {
        self.turn
    }
    fn outcome(&self) -> Option<Outcome> {
        match line(&self.cells) {
            Some((w, _)) => Some(Outcome::Win(w as usize - 1)),
            None if self.cells.iter().all(|&v| v != 0) => Some(Outcome::Draw),
            None => None,
        }
    }
    fn play(&mut self, seat: usize, mv: &str, _: &mut Rng) -> Result<(), &'static str> {
        if seat != self.turn {
            return Err("No es tu turno");
        }
        let i: usize = mv.parse().map_err(|_| "Movimiento no válido")?;
        if i > 8 || self.cells[i] != 0 {
            return Err("Esa casilla ya está ocupada");
        }
        self.cells[i] = seat as u8 + 1;
        self.last = Some(i);
        self.turn = 1 - seat;
        Ok(())
    }
    fn bot(&self, seat: usize, rng: &mut Rng) -> String {
        let me = seat as u8 + 1;
        let mut c = self.cells.clone();
        let mut best: Vec<usize> = Vec::new();
        let mut top = i32::MIN;
        for i in (0..9).filter(|&i| self.cells[i] == 0) {
            c[i] = me;
            let s = minimax(&mut c, me, 3 - me, 1);
            c[i] = 0;
            if s > top {
                top = s;
                best.clear();
            }
            if s == top {
                best.push(i);
            }
        }
        best.get(rng.below(best.len())).copied().unwrap_or(4).to_string()
    }
    fn view(&self, cx: &Ctx) -> Markup {
        let win = line(&self.cells).map(|(_, l)| l);
        let open = self.outcome().is_none() && cx.my_turn(self.turn);
        html! {
            div.gm-ttt {
                @for i in 0..9 {
                    @let v = self.cells[i];
                    @let hot = win.is_some_and(|l| l.contains(&i));
                    @if v == 0 && open {
                        button.gm-cell type="submit" name="m" value=(i) aria-label={ "Casilla " (i + 1) } {}
                    } @else {
                        div.gm-cell.win[hot].last[self.last == Some(i)] {
                            @if v > 0 { (mark(v as usize - 1, cx.color(v as usize - 1))) }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robot_takes_the_win_and_blocks() {
        let mut rng = Rng::seeded(1);
        // X X . / O O . / . . .  — seat 0 (X) to play wins at 2.
        let g = Ttt { cells: vec![1, 1, 0, 2, 2, 0, 0, 0, 0], turn: 0, last: None };
        assert_eq!(g.bot(0, &mut rng), "2");
        // Same board, seat 1 (O) to play: it wins at 5 rather than blocking.
        let g = Ttt { turn: 1, ..g };
        assert_eq!(g.bot(1, &mut rng), "5");
    }
}
