//! The games. Each one is a state (saved as JSON) with the same small contract, `Game`:
//! whose turn it is, how it ends, how a move is applied, what the robot plays, and how it looks
//! for whoever is watching (hidden cards and ships stay hidden). The server is the referee: a
//! phone only sends a move (`m`), and the state decides whether it is allowed.
use maud::Markup;
use serde::{de::DeserializeOwned, Serialize};

pub mod c4;
pub mod colores;
pub mod fleet;
pub mod oca;
pub mod parchis;
pub mod rps;
pub mod ttt;

/// Dice and shuffles: xorshift64*, seeded from the clock. Plenty for a board game at home.
pub struct Rng(u64);

impl Rng {
    pub fn new() -> Rng {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        Rng((t ^ N.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed)) | 1)
    }
    #[cfg(test)]
    pub fn seeded(s: u64) -> Rng {
        Rng(s | 1)
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    pub fn die(&mut self) -> u8 {
        1 + self.below(6) as u8
    }
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i + 1);
            v.swap(i, j);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Outcome {
    Win(usize),
    Draw,
}

/// A seat at the table, for drawing.
pub struct Seat {
    pub name: String,
    pub color: String,
    pub bot: bool,
    pub photo: Option<String>, // URL of their profile photo, if they have one
}

/// What a game needs to draw itself.
pub struct Ctx<'a> {
    pub me: Option<usize>, // my seat; None when watching
    pub seats: &'a [Seat],
}

impl Ctx<'_> {
    pub fn my_turn(&self, turn: usize) -> bool {
        self.me == Some(turn)
    }
    pub fn name(&self, seat: usize) -> &str {
        self.seats.get(seat).map_or("?", |s| s.name.as_str())
    }
    pub fn color(&self, seat: usize) -> &str {
        self.seats.get(seat).map_or("#64748b", |s| s.color.as_str())
    }
    pub fn photo(&self, seat: usize) -> Option<&str> {
        self.seats.get(seat).and_then(|s| s.photo.as_deref())
    }
    /// A seat's face: photo, robot or letter.
    pub fn face(&self, seat: usize) -> Markup {
        match self.seats.get(seat) {
            Some(s) if s.bot => maud::html! { span.avatar.bot { (crate::kit::icon("bot")) } },
            Some(s) => crate::kit::face(&s.name, &s.color, s.photo.as_deref()),
            None => maud::html! {},
        }
    }
}

pub trait Game: Serialize + DeserializeOwned {
    fn new(players: usize, rng: &mut Rng) -> Self;
    fn turn(&self) -> usize;
    fn outcome(&self) -> Option<Outcome>;
    /// A move from `seat`; `Err` explains why it is not allowed.
    fn play(&mut self, seat: usize, mv: &str, rng: &mut Rng) -> Result<(), &'static str>;
    /// What the robot plays in `seat` (it only looks at what a player could see).
    fn bot(&self, seat: usize, rng: &mut Rng) -> String;
    /// The board and its controls: buttons named `m` inside the match's form.
    fn view(&self, cx: &Ctx) -> Markup;
    /// Latest happenings, newest last: (seat, text).
    fn log(&self) -> Vec<(usize, String)> {
        Vec::new()
    }
}

pub struct Info {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub players: (usize, usize),
    pub minutes: &'static str,
    pub blurb: &'static str,
    pub accent: &'static str,
}

pub const GAMES: &[Info] = &[
    Info { id: "parchis", name: "Parchís", icon: "dices", players: (2, 4), minutes: "30 min", blurb: "Saca tus cuatro fichas con un 5, come a los demás y llega a meta.", accent: "#eab308" },
    Info { id: "oca", name: "La oca", icon: "bird", players: (2, 4), minutes: "15 min", blurb: "De oca a oca y tiro porque me toca. Cuidado con el pozo y la calavera.", accent: "#16a34a" },
    Info { id: "colores", name: "Colores", icon: "layers", players: (2, 4), minutes: "10 min", blurb: "Echa una carta del mismo color o número. Gana quien se queda sin cartas.", accent: "#e11d48" },
    Info { id: "fleet", name: "Hundir la flota", icon: "anchor", players: (2, 2), minutes: "10 min", blurb: "Agua, tocado, hundido: encuentra los barcos del otro antes que él los tuyos.", accent: "#0891b2" },
    Info { id: "c4", name: "4 en raya", icon: "circle-dot", players: (2, 2), minutes: "5 min", blurb: "Deja caer fichas y junta cuatro en línea antes que tu rival.", accent: "#2f6bff" },
    Info { id: "rps", name: "Piedra, papel o tijera", icon: "scissors", players: (2, 2), minutes: "1 min", blurb: "Los dos eligen a la vez y nadie ve la mano del otro hasta el final. Gana quien llega a 3.", accent: "#d97706" },
    Info { id: "ttt", name: "Tres en raya", icon: "grid-3x3", players: (2, 2), minutes: "2 min", blurb: "El de toda la vida: tres en línea en un tablero de tres por tres.", accent: "#7c3aed" },
];

/// The banner art of each game for the gallery: our own little scenes made of its pieces.
pub fn art(id: &str) -> String {
    let card = |x: i32, r: i32, c: &str, n: &str| format!(r##"<g transform="translate({x} 46) rotate({r})"><rect x="-17" y="-25" width="34" height="50" rx="6" fill="#fff"/><rect x="-14" y="-22" width="28" height="44" rx="4" fill="{c}"/><ellipse cx="0" cy="0" rx="10" ry="15" fill="#fff" transform="rotate(25)"/><text x="0" y="6" text-anchor="middle" font-size="17" font-weight="800" font-family="system-ui" fill="{c}">{n}</text></g>"##);
    let die = |x: i32, y: i32, r: i32, pips: &[(i32, i32)]| {
        let dots: String = pips.iter().map(|(a, b)| format!(r##"<circle cx="{a}" cy="{b}" r="3.2" fill="#1f2937"/>"##)).collect();
        format!(r##"<g transform="translate({x} {y}) rotate({r})"><rect x="-17" y="-17" width="34" height="34" rx="8" fill="#fff" stroke="#e5e7eb" stroke-width="2"/>{dots}</g>"##)
    };
    let inner = match id {
        "ttt" => r##"<path d="M66 18v54M94 18v54M52 36h56M52 54h56" stroke="#fff" stroke-width="4" stroke-linecap="round" opacity=".85"/><path d="M57 23l10 10M67 23 57 33M85 59l10 10M95 59l-10 10" stroke="#fde68a" stroke-width="5" stroke-linecap="round"/><circle cx="80" cy="45" r="6" fill="none" stroke="#fff" stroke-width="4.5"/><circle cx="101" cy="27" r="6" fill="none" stroke="#fff" stroke-width="4.5"/>"##.to_string(),
        "c4" => {
            let mut s = String::from(r##"<rect x="42" y="14" width="76" height="62" rx="9" fill="#2a55c4" stroke="#1e3a8a" stroke-width="2"/>"##);
            let fill = ["", "", "", "#fbbf24", "", "", "#ef4444", "#fbbf24", "", "", "#ef4444", "#ef4444", "#fbbf24", "#ef4444", "#fbbf24", "#ef4444"];
            for (k, f) in fill.iter().enumerate() {
                let (cx, cy) = (53 + (k % 4) as i32 * 18, 24 + (k / 4) as i32 * 14);
                s += &format!(r##"<circle cx="{cx}" cy="{cy}" r="6" fill="{}"/>"##, if f.is_empty() { "#1e3a8a" } else { f });
            }
            s + r##"<circle cx="71" cy="6" r="6" fill="#ef4444" stroke="#fff" stroke-width="1.5"/>"##
        }
        "fleet" => format!(r##"<path d="M10 66q10-5 20 0t20 0 20 0 20 0 20 0 20 0 20 0" fill="none" stroke="#fff" stroke-width="2.5" opacity=".45"/><path d="M0 78q10-5 20 0t20 0 20 0 20 0 20 0 20 0 20 0 20 0" fill="none" stroke="#fff" stroke-width="2.5" opacity=".3"/><g transform="translate(28 30) scale(.26)">{}</g><circle cx="24" cy="24" r="9" fill="#fde68a"/><circle cx="24" cy="24" r="15" fill="#f97316" opacity=".35"/>"##, fleet::boat(0, 4, true).replace("<svg viewBox=\"0 0 400 100\" aria-hidden=\"true\">", "<svg width=\"400\" height=\"100\" viewBox=\"0 0 400 100\">")),
        "parchis" => die(62, 46, -14, &[(-8, -8), (8, -8), (0, 0), (-8, 8), (8, 8)]) + &die(98, 42, 12, &[(-8, -8), (-8, 0), (-8, 8), (8, -8), (8, 0), (8, 8)]),
        "oca" => r##"<path d="M80 45m-4 0a4 4 0 1 1 8 0a10 10 0 1 1-20 0a17 17 0 1 1 34 0a25 25 0 1 1-50 0a33 33 0 1 1 66 0" fill="none" stroke="#fff" stroke-width="3" stroke-dasharray="1 7" stroke-linecap="round" opacity=".7"/><g transform="translate(64 26)"><path d="M10 34c0-10 6-15 14-15 2-6-1-11-6-11-4 0-6 3-6 6l-6 1 6 3c-1 4-2 8-2 16z" fill="#fff"/><path d="M4 15l-5 2 6 1z" fill="#f97316"/><circle cx="16" cy="11" r="1.6" fill="#1f2937"/><path d="M10 34c8 4 20 3 24-6-6 2-12 1-16-3" fill="#e5e7eb"/></g>"##.to_string(),
        "colores" => card(58, -16, "#ef4444", "7") + &card(80, -2, "#facc15", "+2") + &card(102, 13, "#3b82f6", "4"),
        "rps" => format!(r#"<g transform="translate(30 22) scale(.75)">{}</g><g transform="translate(64 14) scale(.95)">{}</g><g transform="translate(106 22) scale(.75)">{}</g>"#, rps::ROCK.replace("<svg ", "<svg width=\"64\" height=\"64\" "), rps::PAPER.replace("<svg ", "<svg width=\"64\" height=\"64\" "), rps::SCISSORS.replace("<svg ", "<svg width=\"64\" height=\"64\" ")),
        _ => String::new(),
    };
    format!(r#"<svg class="gm-art-svg" viewBox="0 0 160 90" aria-hidden="true">{inner}</svg>"#)
}

pub fn info(id: &str) -> Option<&'static Info> {
    GAMES.iter().find(|g| g.id == id)
}

/// Runs `$body` with `$G` as the game type named by `$id` (None for an unknown game).
macro_rules! with_game {
    ($id:expr, $G:ident, $body:expr) => {
        match $id {
            "ttt" => {
                type $G = ttt::Ttt;
                Some($body)
            }
            "c4" => {
                type $G = c4::C4;
                Some($body)
            }
            "fleet" => {
                type $G = fleet::Fleet;
                Some($body)
            }
            "oca" => {
                type $G = oca::Oca;
                Some($body)
            }
            "parchis" => {
                type $G = parchis::Parchis;
                Some($body)
            }
            "colores" => {
                type $G = colores::Colores;
                Some($body)
            }
            "rps" => {
                type $G = rps::Rps;
                Some($body)
            }
            _ => None,
        }
    };
}

/// A state after a move, ready to save.
pub struct Status {
    pub state: String,
    pub turn: usize,
    pub outcome: Option<Outcome>,
}

fn pack<G: Game>(g: &G) -> Status {
    Status { state: serde_json::to_string(g).unwrap_or_default(), turn: g.turn(), outcome: g.outcome() }
}

pub fn create(game: &str, bots: &[bool], rng: &mut Rng) -> Option<Status> {
    with_game!(game, G, pack(&G::new(bots.len(), rng)))
}

/// One robot move, if it is a robot's turn: `Some(None)` when a person has to play (or it is over).
/// The app plays these one at a time with a pause, so everyone can follow what the robots do.
pub fn robot_step(game: &str, state: &str, bots: &[bool], rng: &mut Rng) -> Option<Option<Status>> {
    with_game!(game, G, {
        serde_json::from_str::<G>(state).ok().and_then(|mut g| {
            let seat = g.turn();
            if g.outcome().is_some() || !bots.get(seat).copied().unwrap_or(false) {
                return None;
            }
            let mv = g.bot(seat, rng);
            g.play(seat, &mv, rng).ok().map(|_| pack(&g))
        })
    })
}

pub fn act(game: &str, state: &str, seat: usize, mv: &str, rng: &mut Rng) -> Option<Result<Status, &'static str>> {
    with_game!(game, G, {
        (|| {
            let mut g: G = serde_json::from_str(state).map_err(|_| "Esta partida no se puede leer")?;
            if g.outcome().is_some() {
                return Err("La partida ya ha terminado");
            }
            g.play(seat, mv, rng)?;
            Ok(pack(&g))
        })()
    })
}

pub fn render(game: &str, state: &str, cx: &Ctx) -> Option<(Markup, Vec<(usize, String)>)> {
    with_game!(game, G, {
        match serde_json::from_str::<G>(state) {
            Ok(g) => (g.view(cx), g.log()),
            Err(_) => (maud::html! { p.hint { "Esta partida no se puede leer." } }, Vec::new()),
        }
    })
}

/// Keeps the last few happenings of a game.
pub fn push_log(log: &mut Vec<(usize, String)>, seat: usize, text: String) {
    log.push((seat, text));
    if log.len() > 8 {
        log.remove(0);
    }
}

/// A die face with its pips (3x3 grid), for oca and parchís.
pub fn die_face(d: u8) -> Markup {
    let on: &[usize] = match d {
        1 => &[4],
        2 => &[0, 8],
        3 => &[0, 4, 8],
        4 => &[0, 2, 6, 8],
        5 => &[0, 2, 4, 6, 8],
        _ => &[0, 2, 3, 5, 6, 8],
    };
    maud::html! {
        span.gm-die aria-label={ "Dado: " (d) } { @for i in 0..9 { i.on[on.contains(&i)] {} } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Robots against robots finish every game, and every finished game says who won.
    #[test]
    fn robots_finish_every_game() {
        for g in GAMES {
            for players in g.players.0..=g.players.1 {
                for seed in 1..=6u64 {
                    let mut rng = Rng::seeded(seed * 7919 + players as u64);
                    let bots = vec![true; players];
                    let mut s = create(g.id, &bots, &mut rng).unwrap();
                    for _ in 0..20_000 {
                        match robot_step(g.id, &s.state, &bots, &mut rng).unwrap() {
                            Some(next) => s = next,
                            None => break,
                        }
                    }
                    assert!(s.outcome.is_some(), "{} with {players} robots (seed {seed}) never ended", g.id);
                }
            }
        }
    }

    #[test]
    fn a_person_cannot_play_out_of_turn() {
        for g in GAMES {
            let mut rng = Rng::seeded(3);
            let bots = vec![false; g.players.0];
            let s = create(g.id, &bots, &mut rng).unwrap();
            let other = (s.turn + 1) % g.players.0;
            let r = act(g.id, &s.state, other, "0", &mut rng).unwrap();
            assert!(r.is_err(), "{}: seat {other} played out of turn", g.id);
        }
    }
}
