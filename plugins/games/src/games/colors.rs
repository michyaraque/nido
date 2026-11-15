//! Colors: a colour-matching discard game. 100 cards: four colours with 1..9 twice each and
//! the actions Salta, Gira and +2 twice each, plus four Comodín that choose the colour. Play a
//! card of the same colour or number (or symbol); if you cannot, draw one and play it or pass.
//! The first with no cards wins. Each player only sees their own hand.
use super::*;
use crate::kit::icon;
use maud::html;
use serde::{Deserialize, Serialize};

const SKIP: u8 = 10;
const TURN: u8 = 11;
const TWO: u8 = 12;
const WILD: u8 = 13;
const ANY: u8 = 4; // colour of a wild card
pub const PALETTE: [&str; 4] = ["#ef4444", "#eab308", "#22c55e", "#3b82f6"];
const NAMES: [&str; 4] = ["rojo", "amarillo", "verde", "azul"];

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct Card {
    c: u8, // 0..3, or ANY
    v: u8, // 1..9, SKIP, TURN, TWO, WILD
}

#[derive(Serialize, Deserialize)]
pub struct Colors {
    deck: Vec<Card>,
    pile: Vec<Card>,
    hands: Vec<Vec<Card>>,
    color: u8, // colour that must be followed
    turn: usize,
    dir: i8,
    drew: bool,
    winner: Option<usize>,
    log: Vec<(usize, String)>,
    /// The last thing that happened, for the page to animate: (who, "play" | "draw" | "pass").
    #[serde(default)]
    act: Option<(usize, String)>,
    /// Who lost their turn just now (Salta, +2, or Gira between two): they get the ⊘ badge.
    #[serde(default)]
    skipped: Option<usize>,
    /// Who drew two cards because of a +2.
    #[serde(default)]
    victim: Option<usize>,
}

fn name(card: Card) -> String {
    let what = match card.v {
        SKIP => "Salta".to_string(),
        TURN => "Gira".to_string(),
        TWO => "+2".to_string(),
        WILD => return "Comodín".into(),
        n => n.to_string(),
    };
    format!("{what} {}", NAMES[card.c as usize])
}

impl Colors {
    fn top(&self) -> Card {
        *self.pile.last().unwrap_or(&Card { c: 0, v: 1 })
    }
    fn fits(&self, card: Card) -> bool {
        card.v == WILD || card.c == self.color || card.v == self.top().v
    }
    fn take(&mut self, rng: &mut Rng) -> Option<Card> {
        if self.deck.is_empty() && self.pile.len() > 1 {
            let top = self.pile.pop()?;
            self.deck = std::mem::take(&mut self.pile);
            for c in &mut self.deck {
                if c.v == WILD {
                    c.c = ANY;
                }
            }
            rng.shuffle(&mut self.deck);
            self.pile.push(top);
        }
        self.deck.pop()
    }
    fn step(&self, from: usize, n: usize) -> usize {
        let len = self.hands.len() as i64;
        ((from as i64 + self.dir as i64 * n as i64) % len + len) as usize % len as usize
    }
    fn end_turn(&mut self, extra: usize) {
        self.turn = self.step(self.turn, 1 + extra);
        self.drew = false;
    }
    /// Colour the robot (or anyone in a hurry) would pick: the one it holds most of.
    fn best_color(hand: &[Card]) -> u8 {
        (0..4u8).max_by_key(|&c| hand.iter().filter(|k| k.c == c).count()).unwrap_or(0)
    }
}

fn card_face(card: Card, shown: u8) -> Markup {
    let c = if card.v == WILD { shown } else { card.c };
    let mid = match card.v {
        SKIP => icon("ban"),
        TURN => icon("refresh-cw"),
        TWO => html! { b { "+2" } },
        WILD => html! { span.gm-wild-face { @for q in 0..4u8 { i class={ "q" (q) @if q == shown { " on" } } {} } } },
        n => html! { b { (n) } },
    };
    let corner = match card.v {
        SKIP => icon("ban"),
        TURN => icon("refresh-cw"),
        TWO => html! { "+2" },
        WILD => html! { "?" }, // the comodín can be any colour
        n => html! { (n) },
    };
    html! {
        span class={ "gm-card k" (c) @if card.v == WILD { " wild" @if shown < ANY { " chosen" } } } {
            span.gm-card-tab { (corner) }
            span.gm-card-mid { (mid) }
        }
    }
}

impl Game for Colors {
    fn new(players: usize, _: &str, rng: &mut Rng) -> Self {
        let mut deck = Vec::with_capacity(100);
        for c in 0..4 {
            for v in 1..=TWO {
                deck.push(Card { c, v });
                deck.push(Card { c, v });
            }
        }
        for _ in 0..4 {
            deck.push(Card { c: ANY, v: WILD });
        }
        rng.shuffle(&mut deck);
        let hands: Vec<Vec<Card>> = (0..players).map(|_| deck.split_off(deck.len() - 7)).collect();
        // The first card face up is a plain number.
        let i = deck.iter().position(|c| c.v <= 9).unwrap_or(0);
        let first = deck.remove(i);
        Colors { deck, pile: vec![first], hands, color: first.c, turn: 0, dir: 1, drew: false, winner: None, log: Vec::new(), act: None, skipped: None, victim: None }
    }
    fn turn(&self) -> usize {
        self.turn
    }
    fn outcome(&self) -> Option<Outcome> {
        self.winner.map(Outcome::Win)
    }
    fn again(&self, seat: usize) -> bool {
        self.turn == seat && self.skipped.is_some() && matches!(&self.act, Some((s, k)) if *s == seat && k == "play")
    }
    fn hint(&self, seat: usize) -> String {
        if self.again(seat) { "El otro pierde el turno: juegas otra vez".into() } else { String::new() }
    }
    fn call(&self) -> Option<Call> {
        let seat = match &self.act {
            Some((s, k)) if k == "play" && self.winner.is_none() => *s,
            _ => return None,
        };
        let card = *self.pile.last()?;
        let hit = |text: &str| self.skipped.map(|s| (s, text.to_string()));
        let (text, to) = match card.v {
            TWO => ("¡+2! Roba dos y pierde el turno".to_string(), hit("¡+2 para ti! Robas dos y pierdes el turno")),
            SKIP => ("¡Salta! Pierde el turno".to_string(), hit("¡Te saltan! Pierdes el turno")),
            TURN if self.hands.len() == 2 => ("¡Gira! Vuelve a jugar".to_string(), hit("¡Gira! Pierdes el turno")),
            TURN => ("¡Cambio de sentido!".to_string(), None),
            WILD => (format!("¡Comodín! Ahora va {}", NAMES[card.c as usize]), None),
            _ if self.hands[seat].len() == 1 => ("¡Le queda una!".to_string(), None),
            _ => return None,
        };
        Some(Call { seat, art: "", text, own: None, to, good: true })
    }
    fn play(&mut self, s: usize, mv: &str, rng: &mut Rng) -> Result<(), &'static str> {
        if s != self.turn {
            return Err("No es tu turno");
        }
        self.skipped = None;
        self.victim = None;
        match mv {
            "draw" => {
                if self.drew {
                    return Err("Ya has robado: juega o pasa");
                }
                let fits = match self.take(rng) {
                    Some(card) => {
                        self.hands[s].push(card);
                        self.fits(card)
                    }
                    None => false,
                };
                self.drew = true;
                self.act = Some((s, "draw".into()));
                if fits {
                    push_log(&mut self.log, s, "roba una carta".into());
                } else {
                    // Nothing to play with it: the turn passes on by itself.
                    push_log(&mut self.log, s, "roba y pasa".into());
                    self.end_turn(0);
                }
                Ok(())
            }
            "pass" => {
                if !self.drew {
                    return Err("Antes de pasar, roba una carta");
                }
                push_log(&mut self.log, s, "pasa".into());
                self.act = Some((s, "pass".into()));
                self.end_turn(0);
                Ok(())
            }
            _ => {
                let mut parts = mv.strip_prefix("play:").ok_or("Movimiento no válido")?.split(':');
                let i: usize = parts.next().and_then(|n| n.parse().ok()).ok_or("Movimiento no válido")?;
                let card = *self.hands[s].get(i).ok_or("Esa carta no está en tu mano")?;
                if !self.fits(card) {
                    return Err("Esa carta no sirve: mismo color o mismo número");
                }
                let chosen: Option<u8> = parts.next().and_then(|c| c.parse().ok()).filter(|&c| c < 4);
                if card.v == WILD && chosen.is_none() {
                    return Err("Elige un color para el comodín");
                }
                self.hands[s].remove(i);
                let mut played = card;
                if card.v == WILD {
                    played.c = chosen.unwrap_or(0);
                }
                self.color = played.c;
                self.pile.push(played);
                self.act = Some((s, "play".into()));
                let said = if card.v == WILD { format!("echa un comodín: {}", NAMES[played.c as usize]) } else { format!("echa {}", name(card)) };
                push_log(&mut self.log, s, said);
                if self.hands[s].is_empty() {
                    self.winner = Some(s);
                    return Ok(());
                }
                if self.hands[s].len() == 1 {
                    push_log(&mut self.log, s, "¡le queda una!".into());
                }
                if matches!(card.v, SKIP | TWO) || (card.v == TURN && self.hands.len() == 2) {
                    self.skipped = Some(self.step(s, 1));
                }
                match card.v {
                    SKIP => self.end_turn(1),
                    TURN => {
                        self.dir = -self.dir;
                        self.end_turn(if self.hands.len() == 2 { 1 } else { 0 });
                    }
                    TWO => {
                        let next = self.step(s, 1);
                        for _ in 0..2 {
                            if let Some(c) = self.take(rng) {
                                self.hands[next].push(c);
                            }
                        }
                        push_log(&mut self.log, next, "roba dos y pierde el turno".into());
                        self.victim = Some(next);
                        self.end_turn(1);
                    }
                    _ => self.end_turn(0),
                }
                Ok(())
            }
        }
    }
    fn bot(&self, s: usize, _: &mut Rng) -> String {
        let hand = &self.hands[s];
        // Actions first, then the highest number, the comodín last.
        let pick = (0..hand.len())
            .filter(|&i| self.fits(hand[i]))
            .max_by_key(|&i| match hand[i].v {
                WILD => 0,
                v if v >= SKIP => 20,
                v => v as i32,
            });
        match pick {
            Some(i) if hand[i].v == WILD => format!("play:{i}:{}", Self::best_color(hand)),
            Some(i) => format!("play:{i}"),
            None if !self.drew => "draw".into(),
            None => "pass".into(),
        }
    }
    fn view(&self, cx: &Ctx) -> Markup {
        let playing = self.winner.is_none();
        let mine = playing && cx.me.is_some_and(|m| m == self.turn);
        let count = |n: usize| html! {
            span.gm-count.warn[n == 2].bad[n == 1] { (n) (icon("layers")) }
        };
        // The last cards of the pile peek out under the top one, each a little turned.
        let under: Vec<Card> = self.pile.iter().rev().take(4).rev().copied().collect();
        const TILT: [(f64, f64, f64); 4] = [(-9.0, -6.0, 3.0), (7.0, 4.0, -2.0), (-4.0, 2.0, 1.0), (-4.0, 0.0, 0.0)];
        let (act_seat, act_kind) = self.act.as_ref().map_or((None, ""), |(s, k)| (Some(*s), k.as_str()));
        html! {
            div.gm-colores data-act=(act_kind) data-seat=[act_seat] data-me=[cx.me] data-victim=[self.victim] data-dir=(self.dir) {
                div.gm-others {
                    @for (s, hand) in self.hands.iter().enumerate() {
                        @if Some(s) != cx.me {
                            div.gm-other.on[s == self.turn && playing].skipped[self.skipped == Some(s)] data-seat=(s) {
                                @if playing {
                                    span.gm-backs { @for _ in 0..hand.len().min(9) { i {} } }
                                } @else {
                                    // The end: everyone's hand face up.
                                    span.gm-reveal { @for &c in hand { (card_face(c, c.c)) } }
                                }
                                span.gm-other-who {
                                    span.gm-face { (cx.face(s)) @if self.skipped == Some(s) { span.gm-ban { (icon("ban")) } } }
                                    b { (cx.name(s)) }
                                    (count(hand.len()))
                                }
                            }
                        }
                    }
                }
                div.gm-table {
                    // Two curved arrows around the pile: the way the turn goes.
                    svg.gm-arrows.back[self.dir < 0] viewBox="0 0 220 220" aria-hidden="true" {
                        path d="M58 52 A78 78 0 0 1 162 52" {}
                        path d="M168 46 l2 14 -14 -2" {}
                        path d="M162 168 A78 78 0 0 1 58 168" {}
                        path d="M52 174 l-2 -14 14 2" {}
                    }
                    div.gm-pile {
                        @for (i, &c) in under.iter().enumerate() {
                            @let (r, x, y) = TILT[TILT.len() - under.len() + i];
                            span.gm-pile-card.top[i + 1 == under.len()] style={ "--r:" (r) "deg;--x:" (x) "px;--y:" (y) "px" } {
                                (card_face(c, if c.v == WILD { c.c } else { c.c }))
                            }
                        }
                    }
                    @if mine && !self.drew {
                        button.gm-deck.glow type="submit" name="m" value="draw" aria-label="Robar una carta" title="Robar" { span.gm-card.back {} span.gm-card.back.b2 {} }
                    } @else {
                        span.gm-deck title={ (self.deck.len()) " en el mazo" } { span.gm-card.back {} span.gm-card.back.b2 {} }
                    }
                }
                div.gm-now {
                    span.gm-color-pill style={ "background:" (PALETTE[self.color as usize]) } { "Toca " (NAMES[self.color as usize]) }
                    @if mine && self.drew { button.btn.outline.sm type="submit" name="m" value="pass" { "Pasar" } }
                    @else if mine { span.hint { "Echa una carta o roba del mazo" } }
                }
                @if let Some(me) = cx.me.filter(|&m| m < self.hands.len()) {
                    div.gm-mine.on[mine].skipped[self.skipped == Some(me)] {
                        div.gm-hand style={ "--n:" (self.hands[me].len()) } {
                            @for (i, &card) in self.hands[me].iter().enumerate() {
                                @let ok = mine && self.fits(card);
                                @if card.v == WILD && ok {
                                    // The comodín opens a big diamond of colours (no script needed: it is a <details>).
                                    details.gm-wild {
                                        summary.gm-card-btn.can aria-label="Echar el comodín" { (card_face(card, ANY)) }
                                        div.gm-picker role="dialog" aria-label="Elige un color" {
                                            div.gm-diamond {
                                                @for c in 0..4u8 {
                                                    button class={ "q q" (c) } type="submit" name="m" value={ "play:" (i) ":" (c) } aria-label={ "Comodín: " (NAMES[c as usize]) } {}
                                                }
                                            }
                                        }
                                    }
                                } @else if ok {
                                    button.gm-card-btn.can type="submit" name="m" value={ "play:" (i) } aria-label={ "Echar " (name(card)) } { (card_face(card, card.c)) }
                                } @else {
                                    span.gm-card-btn.off { (card_face(card, card.c)) }
                                }
                            }
                        }
                        div.gm-me-who {
                            span.gm-face { (cx.face(me)) @if self.skipped == Some(me) { span.gm-ban { (icon("ban")) } } }
                            b { "Tú" }
                            (count(self.hands[me].len()))
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
    fn deck_and_actions() {
        let mut rng = Rng::seeded(5);
        let g = Colors::new(3, "", &mut rng);
        assert_eq!(g.deck.len() + g.pile.len() + g.hands.iter().map(Vec::len).sum::<usize>(), 100);
        assert!(g.top().v <= 9, "starts on a number");

        let mut g = Colors::new(3, "", &mut rng);
        g.color = 0;
        g.pile.push(Card { c: 0, v: 3 });
        g.hands[0] = vec![Card { c: 0, v: TWO }, Card { c: 2, v: 5 }];
        let before = g.hands[1].len();
        g.play(0, "play:0", &mut rng).unwrap(); // +2 on red: seat 1 draws two and is skipped
        assert_eq!((g.hands[1].len(), g.turn), (before + 2, 2));
        assert!(g.play(2, "pass", &mut rng).is_err(), "must draw before passing");
        g.hands[2] = vec![Card { c: ANY, v: WILD }, Card { c: 1, v: 1 }];
        assert!(g.play(2, "play:0", &mut rng).is_err(), "a comodín needs a colour");
        g.play(2, "play:0:3", &mut rng).unwrap();
        assert_eq!((g.color, g.turn), (3, 0));
        assert!(g.play(0, "play:0", &mut rng).is_err(), "green 5 does not follow blue");
    }

    #[test]
    fn plays_again_after_blocking_the_other() {
        let mut rng = Rng::seeded(7);
        let mut g = Colors::new(2, "", &mut rng);
        g.color = 0;
        g.pile.push(Card { c: 0, v: 3 });
        g.hands[0] = vec![Card { c: 0, v: SKIP }, Card { c: 0, v: 4 }, Card { c: 2, v: 5 }];
        g.play(0, "play:0", &mut rng).unwrap();
        assert_eq!(g.turn, 0);
        assert!(g.again(0) && !g.again(1));
        g.play(0, "play:0", &mut rng).unwrap(); // a plain number: the turn moves on
        assert!(!g.again(0) && !g.again(1));

        let mut g = Colors::new(2, "", &mut rng);
        g.color = 0;
        g.pile.push(Card { c: 0, v: 3 });
        g.hands[0] = vec![Card { c: 0, v: TURN }, Card { c: 2, v: 5 }];
        let dir = g.dir;
        g.play(0, "play:0", &mut rng).unwrap();
        assert_eq!((g.dir, g.turn), (-dir, 0), "Gira between two: the arrows turn and I play again");
        let c = g.call().unwrap();
        assert_eq!(c.for_seat(Some(0)), None, "nothing to tell the one who played it");
        assert_eq!(c.for_seat(Some(1)), Some("¡Gira! Pierdes el turno"));
        assert_eq!(c.for_seat(None), Some("¡Gira! Vuelve a jugar"));
    }
}
