//! 4 en raya: 7 columns, 6 rows. The robot looks five moves ahead (negamax with alpha-beta).
use super::*;
use maud::html;
use serde::{Deserialize, Serialize};

const W: usize = 7;
const H: usize = 6;

#[derive(Serialize, Deserialize)]
pub struct C4 {
    g: Vec<u8>, // row-major, row 0 on top; 0 empty, 1 seat 0, 2 seat 1
    turn: usize,
    last: Option<usize>,
}

fn drop_at(g: &[u8], col: usize) -> Option<usize> {
    (0..H).rev().map(|r| r * W + col).find(|&i| g[i] == 0)
}

const DIRS: [(isize, isize); 4] = [(0, 1), (1, 0), (1, 1), (1, -1)];

fn four(g: &[u8]) -> Option<(u8, [usize; 4])> {
    for r in 0..H as isize {
        for c in 0..W as isize {
            let v = g[(r * W as isize + c) as usize];
            if v == 0 {
                continue;
            }
            for (dr, dc) in DIRS {
                let cells: Vec<usize> = (0..4)
                    .map_while(|k| {
                        let (rr, cc) = (r + dr * k, c + dc * k);
                        ((0..H as isize).contains(&rr) && (0..W as isize).contains(&cc)).then(|| (rr * W as isize + cc) as usize)
                    })
                    .collect();
                if cells.len() == 4 && cells.iter().all(|&i| g[i] == v) {
                    return Some((v, [cells[0], cells[1], cells[2], cells[3]]));
                }
            }
        }
    }
    None
}

/// Every run of four cells scored for `p`: more of mine with none of theirs is better.
fn eval(g: &[u8], p: u8) -> i32 {
    let mut s = 0;
    for r in 0..H as isize {
        for c in 0..W as isize {
            for (dr, dc) in DIRS {
                let (er, ec) = (r + dr * 3, c + dc * 3);
                if !(0..H as isize).contains(&er) || !(0..W as isize).contains(&ec) {
                    continue;
                }
                let (mut mine, mut theirs) = (0, 0);
                for k in 0..4 {
                    match g[((r + dr * k) * W as isize + c + dc * k) as usize] {
                        0 => {}
                        v if v == p => mine += 1,
                        _ => theirs += 1,
                    }
                }
                s += match (mine, theirs) {
                    (3, 0) => 6,
                    (2, 0) => 2,
                    (0, 3) => -7,
                    (0, 2) => -2,
                    _ => 0,
                };
            }
        }
    }
    s + (0..H).filter(|r| g[r * W + 3] == p).count() as i32 * 3
}

const ORDER: [usize; 7] = [3, 2, 4, 1, 5, 0, 6];

fn negamax(g: &mut [u8], p: u8, depth: u32, mut a: i32, b: i32) -> i32 {
    if let Some((w, _)) = four(g) {
        return if w == p { 10_000 + depth as i32 } else { -10_000 - depth as i32 };
    }
    if depth == 0 || g.iter().all(|&v| v != 0) {
        return eval(g, p);
    }
    let mut best = -100_000;
    for col in ORDER {
        if let Some(i) = drop_at(g, col) {
            g[i] = p;
            let v = -negamax(g, 3 - p, depth - 1, -b, -a);
            g[i] = 0;
            best = best.max(v);
            a = a.max(v);
            if a >= b {
                break;
            }
        }
    }
    best
}

impl Game for C4 {
    fn new(_: usize, _: &str, _: &mut Rng) -> Self {
        C4 { g: vec![0; W * H], turn: 0, last: None }
    }
    fn turn(&self) -> usize {
        self.turn
    }
    fn outcome(&self) -> Option<Outcome> {
        match four(&self.g) {
            Some((w, _)) => Some(Outcome::Win(w as usize - 1)),
            None if self.g.iter().all(|&v| v != 0) => Some(Outcome::Draw),
            None => None,
        }
    }
    fn play(&mut self, seat: usize, mv: &str, _: &mut Rng) -> Result<(), &'static str> {
        if seat != self.turn {
            return Err("No es tu turno");
        }
        let col: usize = mv.parse().map_err(|_| "Movimiento no válido")?;
        let i = (col < W).then(|| drop_at(&self.g, col)).flatten().ok_or("Esa columna está llena")?;
        self.g[i] = seat as u8 + 1;
        self.last = Some(i);
        self.turn = 1 - seat;
        Ok(())
    }
    fn bot(&self, seat: usize, rng: &mut Rng) -> String {
        let p = seat as u8 + 1;
        let mut g = self.g.clone();
        let mut best: Vec<usize> = Vec::new();
        let mut top = i32::MIN;
        for col in ORDER {
            if let Some(i) = drop_at(&g, col) {
                g[i] = p;
                let v = -negamax(&mut g, 3 - p, 4, -100_000, 100_000);
                g[i] = 0;
                if v > top {
                    top = v;
                    best.clear();
                }
                if v == top {
                    best.push(col);
                }
            }
        }
        best.get(rng.below(best.len())).copied().unwrap_or(3).to_string()
    }
    fn view(&self, cx: &Ctx) -> Markup {
        let win = four(&self.g).map(|(_, l)| l);
        let open = self.outcome().is_none() && cx.my_turn(self.turn);
        html! {
            div.gm-c4 style={ "--me:" (cx.color(self.turn)) } {
                // Players always see the arrows (greyed while waiting), so the board never moves.
                @if cx.me.is_some() {
                    div.gm-c4-drops {
                        @for col in 0..W {
                            button.gm-drop type="submit" name="m" value=(col) disabled[!open || drop_at(&self.g, col).is_none()] aria-label={ "Columna " (col + 1) } {
                                (crate::kit::icon("chevron-down"))
                            }
                        }
                    }
                }
                div.gm-c4-grid {
                    @for i in 0..W * H {
                        @let v = self.g[i];
                        span.gm-hole.win[win.is_some_and(|l| l.contains(&i))].last[self.last == Some(i)] {
                            @if v > 0 { span.gm-disc style={ "background:" (cx.color(v as usize - 1)) } {} }
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
    fn robot_wins_or_blocks_four() {
        let mut rng = Rng::seeded(2);
        let mut g = C4::new(2, "", &mut rng);
        // Seat 0 has three in the bottom row (columns 0-2); seat 1 must block at column 3.
        for (col, seat) in [(0, 0), (6, 1), (1, 0), (6, 1), (2, 0)] {
            g.play(seat, &col.to_string(), &mut rng).unwrap();
        }
        assert_eq!(g.bot(1, &mut rng), "3");
        g.play(1, "5", &mut rng).unwrap();
        assert_eq!(g.bot(0, &mut rng), "3", "and seat 0 takes the win");
    }
}
