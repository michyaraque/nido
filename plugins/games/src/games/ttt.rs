//! Tres en raya sin fin: each player keeps at most three marks; the fourth takes the place of their
//! oldest one, which fades first so everyone sees it coming. It only ends when someone makes a line.
use super::*;
use maud::html;
use serde::{Deserialize, Serialize};

const KEEP: usize = 3;

#[derive(Serialize, Deserialize, Clone)]
pub struct Ttt {
    cells: Vec<u8>, // 0 empty, 1 seat 0, 2 seat 1
    turn: usize,
    last: Option<usize>,
    #[serde(default)]
    order: Vec<usize>, // marks on the board, oldest first
}

const LINES: [[usize; 3]; 8] = [[0, 1, 2], [3, 4, 5], [6, 7, 8], [0, 3, 6], [1, 4, 7], [2, 5, 8], [0, 4, 8], [2, 4, 6]];

fn line(c: &[u8]) -> Option<(u8, [usize; 3])> {
    LINES.iter().find(|l| c[l[0]] != 0 && c[l[0]] == c[l[1]] && c[l[1]] == c[l[2]]).map(|l| (c[l[0]], *l))
}

impl Ttt {
    /// The mark of `seat` that goes next, once they have three.
    fn oldest(&self, seat: usize) -> Option<usize> {
        let mine: Vec<usize> = self.order.iter().copied().filter(|&i| self.cells[i] == seat as u8 + 1).collect();
        (mine.len() >= KEEP).then(|| mine[0])
    }
    fn put(&mut self, seat: usize, i: usize) {
        if let Some(old) = self.oldest(seat) {
            self.cells[old] = 0;
            self.order.retain(|&x| x != old);
        }
        self.cells[i] = seat as u8 + 1;
        self.order.push(i);
        self.last = Some(i);
        self.turn = 1 - seat;
    }
}

/// Score for `me` looking `depth` moves ahead: a line sooner is better, nothing found is 0.
fn search(g: &Ttt, me: usize, depth: i32) -> i32 {
    if let Some((w, _)) = line(&g.cells) {
        return if w as usize == me + 1 { 100 + depth } else { -100 - depth };
    }
    if depth == 0 {
        return 0;
    }
    let scores = (0..9).filter(|&i| g.cells[i] == 0).map(|i| {
        let mut next = g.clone();
        next.put(g.turn, i);
        search(&next, me, depth - 1)
    });
    if g.turn == me { scores.max().unwrap_or(0) } else { scores.min().unwrap_or(0) }
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
    fn new(_: usize, _: &str, _: &mut Rng) -> Self {
        Ttt { cells: vec![0; 9], turn: 0, last: None, order: Vec::new() }
    }
    fn turn(&self) -> usize {
        self.turn
    }
    fn outcome(&self) -> Option<Outcome> {
        line(&self.cells).map(|(w, _)| Outcome::Win(w as usize - 1))
    }
    fn play(&mut self, seat: usize, mv: &str, _: &mut Rng) -> Result<(), &'static str> {
        if seat != self.turn {
            return Err("No es tu turno");
        }
        let i: usize = mv.parse().map_err(|_| "Movimiento no válido")?;
        if i > 8 || self.cells[i] != 0 {
            return Err("Esa casilla ya está ocupada");
        }
        self.put(seat, i);
        Ok(())
    }
    fn bot(&self, seat: usize, rng: &mut Rng) -> String {
        let free: Vec<usize> = (0..9).filter(|&i| self.cells[i] == 0).collect();
        if rng.below(6) == 0 {
            return free[rng.below(free.len())].to_string();
        }
        let mut best: Vec<usize> = Vec::new();
        let mut top = i32::MIN;
        for &i in &free {
            let mut next = self.clone();
            next.put(seat, i);
            let s = search(&next, seat, 4);
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
        let going = if self.outcome().is_none() { self.oldest(self.turn) } else { None };
        html! {
            div.gm-ttt {
                @for i in 0..9 {
                    @let v = self.cells[i];
                    @let hot = win.is_some_and(|l| l.contains(&i));
                    @if v == 0 && open {
                        button.gm-cell type="submit" name="m" value=(i) aria-label={ "Casilla " (i + 1) } {}
                    } @else {
                        div.gm-cell.win[hot].last[self.last == Some(i)].going[going == Some(i)] title=[(going == Some(i)).then_some("Desaparece en la próxima jugada")] {
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
    fn robot_takes_the_win() {
        let mut rng = Rng::seeded(2);
        // X X . / O O . / . . .  with seat 0 (X) to play: the line at 2 (unless absent-minded).
        let g = Ttt { cells: vec![1, 1, 0, 2, 2, 0, 0, 0, 0], turn: 0, last: None, order: vec![0, 3, 1, 4] };
        let wins = (0..30).filter(|_| g.bot(0, &mut rng) == "2").count();
        assert!(wins >= 20, "took the win only {wins} times out of 30");
    }

    #[test]
    fn the_fourth_mark_replaces_the_oldest() {
        let mut rng = Rng::seeded(1);
        let mut g = Ttt::new(2, "", &mut rng);
        for (seat, i) in [(0, "0"), (1, "4"), (0, "8"), (1, "2"), (0, "6"), (1, "3")] {
            g.play(seat, i, &mut rng).unwrap();
        }
        assert_eq!(g.oldest(0), Some(0));
        g.play(0, "1", &mut rng).unwrap(); // X had 0, 8, 6: the 0 goes
        assert_eq!(g.cells[0], 0);
        assert_eq!(g.cells.iter().filter(|&&v| v == 1).count(), 3);
        assert!(g.outcome().is_none(), "no draw: it goes on");
    }
}
