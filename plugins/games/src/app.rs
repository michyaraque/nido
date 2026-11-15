//! Pages and moves: the gallery, a new match, a match being played (refreshed live on every phone)
//! and the house ranking.
use crate::games::{self, Ctx, Info, Outcome, Rng, Seat, GAMES};
use crate::*;
use axum::{
    extract::Path,
    routing::post,
    Form as F,
};

pub const FLASHES: &[(&str, &str)] = &[("game_started", "¡A jugar!"), ("game_left", "Has abandonado la partida")];

const CSS: &str = include_str!(concat!(env!("OUT_DIR"), "/style.css"));
const JS: &str = include_str!("../app.js");
/// Fredoka (SIL OFL 1.1, fonts/OFL.txt), latin subset: the rounded letters of the big game messages.
const FONT: &[u8] = include_bytes!("../fonts/fredoka.woff2");
const BOT: [&str; 3] = ["#64748b", "#a16207", "#0d9488"];
const SPARE: [&str; 4] = ["#d97706", "#0891b2", "#db2777", "#17803d"];
/// Reactions during a match: (id, icon, colour, label). Icons, never emoji: they look the same on every phone.
pub const REACTIONS: [(&str, &str, &str, &str); 10] = [
    ("heart", "heart", "#e11d48", "Me encanta"),
    ("thumbs-up", "thumbs-up", "#2f6bff", "Bien jugado"),
    ("laugh", "laugh", "#d97706", "Jaja"),
    ("smile", "smile", "#16a34a", "Qué bien"),
    ("party-popper", "party-popper", "#7c3aed", "¡Fiesta!"),
    ("flame", "flame", "#ea580c", "En racha"),
    ("zap", "zap", "#ca8a04", "¡Zas!"),
    ("hand", "hand", "#0891b2", "Hola"),
    ("frown", "frown", "#64748b", "Vaya"),
    ("angry", "angry", "#dc2626", "Grrr"),
];

/// Colors is a table game with banter: there, reactions are emoji (everywhere else, icons).
pub const EMOJI: [&str; 16] = ["😭", "😎", "🤩", "🤣", "😈", "😡", "🥺", "😱", "😫", "😳", "🥴", "😴", "👋", "🎉", "❤️", "🤔"];

fn with_emoji(game: &str) -> bool {
    game == "colores"
}

pub fn reaction(id: &str) -> Option<&'static (&'static str, &'static str, &'static str, &'static str)> {
    REACTIONS.iter().find(|r| r.0 == id)
}

pub fn routes() -> Router<Db> {
    Router::new()
        .route("/new", post(create))
        .route("/m/{id}/move", post(mv))
        .route("/m/{id}/react", post(react))
        .route("/m/{id}/leave", post(leave))
        .route("/m/{id}/again", post(again))
        .route("/assets/app.js", get(script))
        .route("/assets/fredoka.woff2", get(font))
        .route("/notices", get(notices))
        .route("/notices/late", get(late))
}

async fn script() -> Response {
    ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8"), (header::CACHE_CONTROL, "private, max-age=31536000")], JS).into_response()
}

async fn font() -> Response {
    ([(header::CONTENT_TYPE, "font/woff2"), (header::CACHE_CONTROL, "public, max-age=31536000")], FONT).into_response()
}

fn js_url() -> String {
    let h = JS.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    format!("/plugins/games/assets/app.js?v={h:x}")
}

// ---------- robots, at a human pace ----------

/// Plays the robots' turns of match `id` one by one, with a pause before each, telling every phone
/// after each move. One runner per match at a time.
pub fn robots(db: Db, id: i64) {
    static BUSY: std::sync::Mutex<Vec<i64>> = std::sync::Mutex::new(Vec::new());
    {
        let mut busy = BUSY.lock().unwrap_or_else(|e| e.into_inner());
        if busy.contains(&id) {
            return;
        }
        busy.push(id);
    }
    tokio::spawn(async move {
        for _ in 0..5000 {
            // Think a moment: longer where pieces walk (parchís, la oca), so the last move is seen.
            let pause = {
                let c = lock(&db);
                match load(&c, id) {
                    Ok(Some(m)) if !m.done && m.players.get(m.turn) == Some(&0) => match m.game.as_str() {
                        "parchis" | "oca" => 1100,
                        "colores" | "fleet" => 900,
                        _ => 650,
                    },
                    _ => break,
                }
            };
            tokio::time::sleep(std::time::Duration::from_millis(pause)).await;
            let c = lock(&db);
            let Ok(Some(m)) = load(&c, id) else { break };
            match games::robot_step(&m.game, &m.state, &bots(&m.players), &mut Rng::new()) {
                Some(Some(st)) => {
                    if save(&c, &m, &st).is_err() {
                        break;
                    }
                    crate::net::changed(id);
                }
                _ => break,
            }
        }
        BUSY.lock().unwrap_or_else(|e| e.into_inner()).retain(|&x| x != id);
    });
}

// ---------- matches ----------

struct Match {
    id: i64,
    game: String,
    players: Vec<i64>, // Nido user ids by seat; 0 = robot
    state: String,
    turn: usize,
    done: bool,
    winner: Option<usize>,
    note: String,
    version: i64,
}

const COLS: &str = "id,game,players,state,turn,done,winner,note,version";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Match> {
    Ok(Match {
        id: r.get(0)?,
        game: r.get(1)?,
        players: serde_json::from_str(&r.get::<_, String>(2)?).unwrap_or_default(),
        state: r.get(3)?,
        turn: r.get::<_, i64>(4)? as usize,
        done: r.get(5)?,
        winner: r.get::<_, Option<i64>>(6)?.map(|w| w as usize),
        note: r.get(7)?,
        version: r.get(8)?,
    })
}

fn load(c: &Connection, id: i64) -> rusqlite::Result<Option<Match>> {
    c.query_row(&format!("SELECT {COLS} FROM matches WHERE id=?1"), [id], row).optional()
}

/// Names and colours at the table: people in their Nido colour (never two alike), robots in grey.
fn seats(v: &View, players: &[i64]) -> Vec<Seat> {
    let mut out: Vec<Seat> = Vec::new();
    let mut robots = 0;
    for &p in players {
        let (name, mut color, bot, photo) = match (p, v.person(p)) {
            (0, _) => {
                robots += 1;
                (if robots == 1 { "Robot".to_string() } else { format!("Robot {robots}") }, BOT[(robots - 1) % BOT.len()].to_string(), true, None)
            }
            (_, Some(person)) => (person.name.clone(), person.color.clone(), false, person.photo_url()),
            (_, None) => ("Alguien".to_string(), BOT[0].to_string(), false, None),
        };
        if out.iter().any(|s| s.color == color) {
            color = SPARE.iter().find(|c| !out.iter().any(|s| s.color == **c)).unwrap_or(&SPARE[0]).to_string();
        }
        out.push(Seat { name, color, bot, photo });
    }
    out
}

fn bots(players: &[i64]) -> Vec<bool> {
    players.iter().map(|&p| p == 0).collect()
}

/// Saves a move; when the match ends, writes everyone's result for the ranking (once).
fn save(c: &Connection, m: &Match, st: &games::Status) -> rusqlite::Result<()> {
    let (done, winner) = match st.outcome {
        Some(Outcome::Win(w)) => (true, Some(w as i64)),
        Some(Outcome::Draw) => (true, None),
        None => (false, None),
    };
    c.execute(
        "UPDATE matches SET state=?2, turn=?3, done=?4, winner=?5, version=version+1, updated=datetime('now','localtime') WHERE id=?1",
        params![m.id, st.state, st.turn as i64, done, winner],
    )?;
    if let (false, Some(o)) = (m.done, st.outcome) {
        results(c, m, Some(o), None)?;
    }
    Ok(())
}

/// One row per person: 3 points a win, 1 a draw. Whoever abandons loses.
fn results(c: &Connection, m: &Match, outcome: Option<Outcome>, quitter: Option<usize>) -> rusqlite::Result<()> {
    for (seat, &uid) in m.players.iter().enumerate() {
        if uid == 0 {
            continue;
        }
        let (what, points) = match (outcome, quitter) {
            (Some(Outcome::Win(w)), _) if w == seat => ("win", 3),
            (Some(Outcome::Draw), _) => ("draw", 1),
            (Some(_), _) => ("loss", 0),
            (None, Some(q)) if q == seat => ("loss", 0),
            (None, _) => continue, // a match left half-way does not count for the others
        };
        c.execute(
            "INSERT OR IGNORE INTO results(match_id,user_id,game,outcome,points) VALUES(?1,?2,?3,?4,?5)",
            params![m.id, uid, m.game, what, points],
        )?;
    }
    Ok(())
}

fn to_match(c: &Connection, month_q: Option<&str>, id: i64, ok: &str) -> R {
    let m = month(c, month_q)?;
    let ok = if ok.is_empty() { String::new() } else { format!("&ok={ok}") };
    Ok(Redirect::to(&format!("/?month={m}&tab=games&m={id}{ok}")).into_response())
}

fn field<'a>(f: &'a [(String, String)], k: &str) -> Option<&'a str> {
    f.iter().find(|p| p.0 == k).map(|p| p.1.as_str())
}

type Pairs = Vec<(String, String)>;

/// After a move from the page's script (`?partial=1`): the live part, ready to swap in.
fn partial(c: &Connection, h: &HeaderMap, id: i64, month_q: Option<&str>) -> R {
    let Some(me) = crate::me(h) else { return Ok(StatusCode::FORBIDDEN.into_response()) };
    let v = View { m: month(c, month_q)?, me };
    let Some(m) = load(c, id)? else { return Ok(StatusCode::NOT_FOUND.into_response()) };
    Ok(Html(live_part(c, &v, &m)?.into_string()).into_response())
}

async fn create(State(db): State<Db>, h: HeaderMap, F(f): F<Pairs>) -> R {
    let me = require_user!(h);
    let Some(g) = field(&f, "game").and_then(games::info) else { return bad("Ese juego no existe") };
    let mut players = vec![me.id];
    for (_, id) in f.iter().filter(|p| p.0 == "opp") {
        match id.parse::<i64>() {
            Ok(0) => players.push(0),
            Ok(id) if id != me.id && me.people.iter().any(|p| p.id == id) && !players.contains(&id) => players.push(id),
            _ => {}
        }
    }
    let how_many: usize = field(&f, "robots").and_then(|n| n.parse().ok()).unwrap_or(0).min(3);
    players.extend(std::iter::repeat(0).take(how_many));
    if players.len() < g.players.0 {
        return bad("Faltan jugadores: invita a alguien o añade el robot");
    }
    if players.len() > g.players.1 {
        return bad("Sobran jugadores para este juego");
    }
    let opt = field(&f, "opt").filter(|o| g.options.iter().any(|x| x.0 == *o)).or(g.options.first().map(|x| x.0)).unwrap_or("");
    let c = lock(&db);
    let st = games::create(g.id, &bots(&players), opt, &mut Rng::new()).ok_or("juego")?;
    c.execute(
        "INSERT INTO matches(game,players,state,turn,created_by) VALUES(?1,?2,?3,?4,?5)",
        params![g.id, serde_json::to_string(&players)?, st.state, st.turn as i64, me.id],
    )?;
    let id = c.last_insert_rowid();
    invite(&c, &me, id, &players, &format!("{} te invita a jugar a {}", me.name, g.name))?;
    robots(db.clone(), id);
    to_match(&c, field(&f, "month"), id, "game_started")
}

#[derive(Deserialize)]
struct Partial {
    partial: Option<String>,
}

/// A move by person `who` in match `id`, from the socket or a plain form.
pub fn do_move(c: &Connection, who: i64, id: i64, mv: &str) -> Result<(), String> {
    let m = load(c, id).map_err(|e| e.to_string())?.ok_or("Esa partida no existe")?;
    let seat = m.players.iter().position(|&p| p == who).ok_or("No juegas en esta partida")?;
    match games::act(&m.game, &m.state, seat, mv, &mut Rng::new()) {
        Some(Ok(st)) => save(c, &m, &st).map_err(|e| e.to_string()),
        Some(Err(e)) => Err(e.to_string()),
        None => Err("Ese juego no existe".into()),
    }
}

/// An emoji from a player; returns their seat.
pub fn do_react(c: &Connection, who: i64, id: i64, e: &str) -> Result<usize, &'static str> {
    let m = load(c, id).map_err(|_| "Error interno")?.ok_or("Esa partida no existe")?;
    let seat = m.players.iter().position(|&p| p == who).ok_or("No juegas en esta partida")?;
    let ok = if with_emoji(&m.game) { EMOJI.contains(&e) } else { reaction(e).is_some() };
    if !ok {
        return Err("Esa reacción no existe");
    }
    c.execute("INSERT INTO reactions(match_id,seat,emoji) VALUES(?1,?2,?3)", params![id, seat as i64, e]).map_err(|_| "Error interno")?;
    Ok(seat)
}

/// A match as one viewer sees it, for the socket: version, sound, flags and the html.
pub struct Snapshot {
    pub version: i64,
    pub sound: u8,
    pub flags: u8,
    pub html: String,
}

pub const SOUNDS: [&str; 15] = ["tap", "card", "draw", "dice", "drop", "splash", "hit", "sink", "eat", "reveal", "skip", "two", "turn-around", "wild", "one"];

pub fn snapshot(c: &Connection, v: &View, id: i64) -> rusqlite::Result<Option<Snapshot>> {
    let Some(m) = load(c, id)? else { return Ok(None) };
    let me = m.players.iter().position(|&p| p == v.me.id);
    let seats = seats(v, &m.players);
    let cx = Ctx { me, seats: &seats };
    let log = games::render(&m.game, &m.state, &cx).map(|r| r.1).unwrap_or_default();
    let sound = sfx(&m.game, log.last().map(|l| l.1.as_str()));
    let mut flags = 0u8;
    if !m.done && me == Some(m.turn) {
        flags |= 1;
    }
    if m.done {
        flags |= 2;
        if me.is_some() {
            flags |= if m.winner == me { 4 } else { 8 };
        }
    }
    Ok(Some(Snapshot {
        version: m.version,
        sound: SOUNDS.iter().position(|s| *s == sound).unwrap_or(0) as u8,
        flags,
        html: live_part(c, v, &m)?.into_string(),
    }))
}

async fn mv(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, Query(q): Query<Partial>, F(f): F<Pairs>) -> R {
    let me = require_user!(h);
    let c = lock(&db);
    if let Err(e) = do_move(&c, me.id, id, field(&f, "m").unwrap_or("")) {
        return Ok((StatusCode::CONFLICT, e).into_response());
    }
    crate::net::changed(id);
    robots(db.clone(), id);
    if q.partial.is_some() {
        return partial(&c, &h, id, field(&f, "month"));
    }
    to_match(&c, field(&f, "month"), id, "")
}

async fn react(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, Query(q): Query<Partial>, F(f): F<Pairs>) -> R {
    let me = require_user!(h);
    let c = lock(&db);
    if let Err(e) = do_react(&c, me.id, id, field(&f, "e").unwrap_or("")) {
        return bad(e);
    }
    c.execute("UPDATE matches SET version=version+1 WHERE id=?1", [id])?;
    crate::net::changed(id);
    if q.partial.is_some() {
        return partial(&c, &h, id, field(&f, "month"));
    }
    to_match(&c, field(&f, "month"), id, "")
}

async fn leave(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Pairs>) -> R {
    let me = require_user!(h);
    let c = lock(&db);
    let Some(m) = load(&c, id)? else { return Ok(StatusCode::NOT_FOUND.into_response()) };
    let Some(seat) = m.players.iter().position(|&p| p == me.id) else { return bad("No juegas en esta partida") };
    if m.done {
        return to_match(&c, field(&f, "month"), id, "");
    }
    // With two, the other one wins. With more, the match just ends.
    let outcome = (m.players.len() == 2).then(|| Outcome::Win(1 - seat));
    let winner = match outcome {
        Some(Outcome::Win(w)) => Some(w as i64),
        _ => None,
    };
    c.execute(
        "UPDATE matches SET done=1, winner=?2, note=?3, version=version+1, updated=datetime('now','localtime') WHERE id=?1",
        params![id, winner, format!("{} ha abandonado", me.name)],
    )?;
    results(&c, &m, outcome, Some(seat))?;
    crate::net::changed(id);
    to_match(&c, field(&f, "month"), id, "game_left")
}

/// Same game, same people, and the next one starts.
async fn again(State(db): State<Db>, h: HeaderMap, Path(id): Path<i64>, F(f): F<Pairs>) -> R {
    let me = require_user!(h);
    let c = lock(&db);
    let Some(m) = load(&c, id)? else { return Ok(StatusCode::NOT_FOUND.into_response()) };
    if !m.players.contains(&me.id) {
        return bad("No juegas en esta partida");
    }
    let mut players = m.players.clone();
    players.rotate_left(1);
    let opt = games::option_of(&m.game, &m.state);
    let st = games::create(&m.game, &bots(&players), &opt, &mut Rng::new()).ok_or("juego")?;
    c.execute(
        "INSERT INTO matches(game,players,state,turn,created_by) VALUES(?1,?2,?3,?4,?5)",
        params![m.game, serde_json::to_string(&players)?, st.state, st.turn as i64, me.id],
    )?;
    let new = c.last_insert_rowid();
    let name = games::info(&m.game).map_or("la partida", |g| g.name);
    invite(&c, &me, new, &players, &format!("{} quiere la revancha de {name}", me.name))?;
    robots(db.clone(), new); // after a rematch a robot may start
    to_match(&c, field(&f, "month"), new, "game_started")
}

// ---------- invitations ----------

/// Tells everyone else (not the robots) in `players` about match `id`.
fn invite(c: &Connection, me: &Me, id: i64, players: &[i64], text: &str) -> rusqlite::Result<()> {
    for &p in players.iter().filter(|&&p| p != 0 && p != me.id) {
        c.execute("INSERT INTO notices(user_id, match_id, text) VALUES(?1,?2,?3)", params![p, id, text])?;
    }
    Ok(())
}

fn json(v: serde_json::Value) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], v.to_string()).into_response()
}

/// `GET /notices`: my invitations not seen yet (the open app shows them, so now they are seen).
async fn notices(State(db): State<Db>, h: HeaderMap) -> R {
    let me = require_user!(h);
    let c = lock(&db);
    let list: Vec<serde_json::Value> = c
        .prepare_cached("SELECT text, match_id FROM notices WHERE user_id=?1 AND seen=0 AND at > strftime('%s','now') - 1800 ORDER BY id DESC")?
        .query_map([me.id], |r| Ok(serde_json::json!({ "text": r.get::<_, String>(0)?, "href": format!("/?tab=games&m={}", r.get::<_, i64>(1)?), "action": "Jugar" })))?
        .collect::<Result<_, _>>()?;
    if !list.is_empty() {
        c.execute("UPDATE notices SET seen=1 WHERE user_id=?1 AND seen=0", [me.id])?;
    }
    Ok(json(list.into()))
}

/// `GET /notices/late`: invitations nobody saw in a minute, for Nido to send by Telegram (once).
async fn late(State(db): State<Db>) -> R {
    let c = lock(&db);
    let due = "seen=0 AND sent=0 AND at <= strftime('%s','now') - 60 AND at > strftime('%s','now') - 1800";
    let list: Vec<serde_json::Value> = c
        .prepare(&format!("SELECT user_id, text, match_id FROM notices WHERE {due}"))?
        .query_map([], |r| Ok(serde_json::json!({ "user": r.get::<_, i64>(0)?, "text": r.get::<_, String>(1)?, "href": format!("/?tab=games&m={}", r.get::<_, i64>(2)?) })))?
        .collect::<Result<_, _>>()?;
    c.execute(&format!("UPDATE notices SET sent=1 WHERE {due}"), [])?;
    Ok(json(list.into()))
}

// ---------- pages ----------

pub fn page(c: &Connection, v: &View, p: &Params) -> rusqlite::Result<Markup> {
    let body = if let Some(g) = p.get("new").and_then(|g| games::info(g)) {
        new_match(v, g)
    } else if let Some(m) = p.get("m").and_then(|id| id.parse().ok()).map(|id| load(c, id)).transpose()?.flatten() {
        match_page(c, v, &m)?
    } else {
        gallery(c, v, p.get("rank").map(String::as_str))?
    };
    Ok(html! {
        style { (PreEscaped(CSS)) }
        span hidden data-plugin="games" data-plugin-js=(js_url()) {}
        (body)
    })
}

fn game_art(g: &Info) -> Markup {
    html! { span.gm-art style={ "--c:" (g.accent) } { (icon(g.icon)) } }
}

fn players_label(g: &Info) -> String {
    if g.players.0 == g.players.1 { format!("{} jugadores", g.players.0) } else { format!("{}–{} jugadores", g.players.0, g.players.1) }
}

/// "con Ana y Robot" for a list.
fn with_whom(v: &View, m: &Match) -> String {
    let seats = seats(v, &m.players);
    let others: Vec<&str> = m.players.iter().zip(&seats).filter(|(p, _)| **p != v.me.id).map(|(_, s)| s.name.as_str()).collect();
    match others.as_slice() {
        [] => String::new(),
        [one] => format!("con {one}"),
        [rest @ .., last] => format!("con {} y {last}", rest.join(", ")),
    }
}

/// A list of matches: the first few, and the rest behind "Ver más" (native `details`, no script).
fn rows(v: &View, ms: &[&Match]) -> Markup {
    const SHOWN: usize = 4;
    html! {
        div.gm-rows { @for m in ms.iter().take(SHOWN) { (match_row(v, m)) } }
        @if ms.len() > SHOWN {
            details.gm-more {
                summary { span.gm-more-on { "Ver " (ms.len() - SHOWN) " más" } span.gm-more-off { "Ver menos" } (icon("chevron-down")) }
                div.gm-rows { @for m in ms.iter().skip(SHOWN) { (match_row(v, m)) } }
            }
        }
    }
}

fn match_row(v: &View, m: &Match) -> Markup {
    let g = games::info(&m.game);
    let seats = seats(v, &m.players);
    let me = m.players.iter().position(|&p| p == v.me.id);
    let status = if m.done {
        match m.winner {
            Some(w) if Some(w) == me => html! { (pill("ok", Some("crown"), "Has ganado")) },
            Some(w) => html! { (pill("", None, &format!("Ganó {}", seats[w].name))) },
            None => html! { (pill("", None, "Empate")) },
        }
    } else if Some(m.turn) == me {
        html! { (pill("accent", None, "Te toca")) }
    } else {
        html! { span.muted { "Le toca a " (seats.get(m.turn).map_or("?", |s| s.name.as_str())) } }
    };
    html! {
        a.gm-row href=(v.link(&format!("m={}", m.id))) {
            @if let Some(g) = g { (game_art(g)) }
            span.gm-row-main {
                b { (g.map_or("Juego", |g| g.name)) }
                small { (with_whom(v, m)) }
            }
            (status)
        }
    }
}

fn gallery(c: &Connection, v: &View, rank: Option<&str>) -> rusqlite::Result<Markup> {
    let all: Vec<Match> = c
        .prepare(&format!("SELECT {COLS} FROM matches ORDER BY done, updated DESC LIMIT 200"))?
        .query_map([], row)?
        .collect::<Result<_, _>>()?;
    let mine: Vec<&Match> = all.iter().filter(|m| m.players.contains(&v.me.id)).collect();
    let my_turn: Vec<&&Match> = mine.iter().filter(|m| !m.done && m.players.get(m.turn) == Some(&v.me.id)).collect();
    let waiting: Vec<&&Match> = mine.iter().filter(|m| !m.done && m.players.get(m.turn) != Some(&v.me.id)).collect();
    let recent: Vec<&&Match> = mine.iter().filter(|m| m.done).take(6).collect();
    Ok(html! {
        div.page-head {
            div {
                h1 { "Juegos" }
                p { "Para jugar entre los de casa, cada uno desde su móvil, o contra el robot." }
            }
        }
        (ranking(c, v, rank)?)
        @if !my_turn.is_empty() {
            section.card.gm-turns {
                div.card-head { div { h2 { "Te toca" } p { (my_turn.len()) " partida" @if my_turn.len() != 1 { "s" } " esperándote" } } }
                (rows(v, &my_turn.iter().map(|m| **m).collect::<Vec<_>>()))
            }
        }
        div.gm-gallery {
            @for g in GAMES {
                a.gm-game href=(v.link(&format!("new={}", g.id))) style={ "--c:" (g.accent) } {
                    span.gm-banner {
                        (PreEscaped(games::art(g.id)))
                        span.gm-chip { (icon("users")) (players_label(g)) }
                    }
                    span.gm-game-body {
                        span.gm-game-title { b { (g.name) } span.gm-time { (icon("clock")) (g.minutes) } }
                        small { (g.blurb) }
                        span.gm-play { (icon("play")) "Jugar" }
                    }
                }
            }
        }
        div {
            section.card {
                div.card-head { div { h2 { "Tus partidas" } p { "Las de ahora y las últimas" } } }
                @if waiting.is_empty() && recent.is_empty() {
                    (empty("gamepad-2", "Aún no has jugado", "Elige un juego de arriba e invita a alguien de casa, o juega contra el robot."))
                } @else {
                    (rows(v, &waiting.iter().chain(recent.iter()).map(|m| **m).collect::<Vec<_>>()))
                }
            }
        }
    })
}

fn ranking(c: &Connection, v: &View, game: Option<&str>) -> rusqlite::Result<Markup> {
    let game = game.filter(|g| games::info(g).is_some());
    let rows: Vec<(i64, i64, i64, i64)> = c
        .prepare("SELECT user_id, sum(points), sum(outcome='win'), count(*) FROM results WHERE ?1 IS NULL OR game=?1 GROUP BY user_id")?
        .query_map([game], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    let mut table: Vec<(&Person, i64, i64, i64)> = v
        .me
        .people
        .iter()
        .map(|p| rows.iter().find(|r| r.0 == p.id).map_or((p, 0, 0, 0), |r| (p, r.1, r.2, r.3)))
        .collect();
    table.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)).then(a.3.cmp(&b.3)));
    let link = |g: &str| if g.is_empty() { v.link("") } else { v.link(&format!("rank={g}")) };
    Ok(html! {
        section.gm-hof #ranking {
            div.gm-hof-head {
                span.gm-hof-badge { (icon("trophy")) }
                div { h2 { "Ranking de la casa" } p { "3 puntos por victoria, 1 por empate" } }
            }
            nav.gm-rank-tabs aria-label="Ranking por juego" {
                a.on[game.is_none()] href={ (link("")) "#ranking" } { "Todos" }
                @for g in GAMES { a.on[game == Some(g.id)] href={ (link(g.id)) "#ranking" } title=(g.name) aria-label=(g.name) { (icon(g.icon)) } }
            }
            // Second, first, third: the winner stands in the middle, highest.
            div.gm-podium {
                @for i in [1usize, 0, 2] {
                    @if let Some((p, points, wins, _)) = table.get(i) {
                        div class={ "gm-step gm-p" (i + 1) @if p.id == v.me.id { " me" } } style={ "--d:" (i) } {
                            @if i == 0 && *points > 0 { span.gm-crown { (icon("crown")) } }
                            span.gm-step-face { (p.face()) }
                            b.gm-step-name { (p.name) }
                            span.gm-step-pts { (points) " pts" small { " · " (wins) " vict." } }
                            span.gm-block { (i + 1) }
                        }
                    }
                }
            }
            @if table.len() > 3 {
                ol.gm-rank-list start="4" {
                    @for (i, (p, points, wins, played)) in table.iter().enumerate().skip(3) {
                        li.me[p.id == v.me.id] {
                            span.gm-pos { (i + 1) }
                            (p.face())
                            span.gm-rank-name { b { (p.name) } small { (wins) " victoria" @if *wins != 1 { "s" } " · " (played) " partida" @if *played != 1 { "s" } } }
                            span.gm-points { b { (points) } small { " pts" } }
                        }
                    }
                }
            }
        }
    })
}

fn new_match(v: &View, g: &Info) -> Markup {
    let others: Vec<&Person> = v.me.people.iter().filter(|p| p.id != v.me.id).collect();
    let duel = g.players.1 == 2;
    html! {
        a.back href=(v.link("")) { (icon("chevron-left")) "Juegos" }
        section.card.gm-new style={ "--c:" (g.accent) } {
            div.gm-new-head {
                span.gm-game-art.big { (icon(g.icon)) }
                div { h1 { (g.name) } p { (g.blurb) } p.gm-game-meta { span { (icon("users")) (players_label(g)) } span { (icon("clock")) (g.minutes) } } }
            }
            form.form method="post" action="/plugins/games/new" {
                (v.ctx())
                input type="hidden" name="game" value=(g.id);
                div.field {
                    label { "¿Con quién juegas?" }
                    div.gm-pick {
                        @if duel {
                            @for (i, p) in others.iter().enumerate() {
                                label.gm-opt { input type="radio" name="opp" value=(p.id) checked[i == 0]; (p.face()) span { (p.name) } }
                            }
                            label.gm-opt { input type="radio" name="opp" value="0" checked[others.is_empty()]; span.avatar.bot { (icon("bot")) } span { "El robot" } }
                        } @else {
                            @for p in &others {
                                label.gm-opt { input type="checkbox" name="opp" value=(p.id); (p.face()) span { (p.name) } }
                            }
                        }
                    }
                    @if !duel {
                        label.gm-robots {
                            span.avatar.bot { (icon("bot")) }
                            span { "Robots" }
                            select.input name="robots" {
                                @for n in 0..g.players.1 { option value=(n) selected[n == usize::from(others.is_empty())] { (n) } }
                            }
                        }
                    }
                    @if !g.options.is_empty() {
                        div.field.gm-level {
                            label { "Dificultad" }
                            div.gm-levels {
                                @for (i, (val, label)) in g.options.iter().enumerate() {
                                    label.gm-level-opt { input type="radio" name="opt" value=(val) checked[i == 0]; span { (label) } }
                                }
                            }
                        }
                    }
                    p.hint {
                        "Empiezas tú. Los demás ven la partida en Juegos y juegan desde su móvil cuando les toca."
                    }
                }
                button.btn.primary.wide { (icon("play")) "Empezar partida" }
            }
        }
    }
}

fn match_page(c: &Connection, v: &View, m: &Match) -> rusqlite::Result<Markup> {
    c.execute("UPDATE notices SET seen=1 WHERE user_id=?1 AND match_id=?2", params![v.me.id, m.id])?;
    let g = games::info(&m.game);
    Ok(html! {
        a.back href=(v.link("")) { (icon("chevron-left")) "Juegos" }
        div.gm-match-head {
            @if let Some(g) = g { (game_art(g)) }
            div { h1 { (g.map_or("Partida", |g| g.name)) } p { (with_whom(v, m)) } }
        }
        // A stable box around the live part: fullscreen stays on while the board is swapped.
        div.gm-stage data-game=(m.game) { (live_part(c, v, m)?) }
    })
}

/// Everything that changes during a match. The socket sends it again, drawn for each viewer,
/// whenever `version` grows.
fn live_part(c: &Connection, v: &View, m: &Match) -> rusqlite::Result<Markup> {
    let seats = seats(v, &m.players);
    let me = m.players.iter().position(|&p| p == v.me.id);
    let cx = Ctx { me, seats: &seats };
    let (board, log) = games::render(&m.game, &m.state, &cx).unwrap_or_else(|| (html! {}, Vec::new()));
    let said: Vec<(usize, String)> = c
        .prepare_cached("SELECT seat, emoji FROM reactions WHERE match_id=?1 AND at > strftime('%s','now') - 8 ORDER BY at")?
        .query_map([m.id], |r| Ok((r.get::<_, i64>(0)? as usize, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let action = |what: &str| format!("/plugins/games/m/{}/{what}", m.id);
    let my_turn = !m.done && me == Some(m.turn);
    let hint = games::turn_hint(&m.game, &m.state, m.turn);
    html_ok(html! {
        div #gm-live data-id=(m.id) data-v=(m.version) data-game=(m.game) data-sfx=(sfx(&m.game, log.last().map(|l| l.1.as_str())))
            data-live=[(!m.done).then_some("1")] data-turn=[my_turn.then_some("1")]
            data-end=[m.done.then(|| if me.is_some() && m.winner == me { "win" } else if me.is_some() { "lose" } else { "end" })] {
            div.gm-players {
                @for (i, s) in seats.iter().enumerate() {
                    div.gm-player.on[!m.done && m.turn == i].me[me == Some(i)].won[m.winner == Some(i)] style={ "--c:" (s.color) } {
                        (cx.face(i))
                        span.gm-player-name { (s.name) }
                        @if m.winner == Some(i) { (icon("crown")) }
                        @if let Some((_, e)) = said.iter().rev().find(|r| r.0 == i) {
                            @if with_emoji(&m.game) {
                                span.gm-bubble.emoji { (e) }
                            } @else if let Some(r) = reaction(e) {
                                span.gm-bubble style={ "--r:" (r.2) } title=(r.3) { (icon(r.1)) }
                            }
                        }
                    }
                }
            }
            // One bar of fixed height: whose turn (or the result) and the latest happening, so the
            // page never jumps while playing.
            div.gm-bar.mine[my_turn].done[m.done] {
                @if m.done {
                    span.gm-turn.gm-result-big {
                        @match m.winner {
                            Some(w) if Some(w) == me => { (icon("crown")) "¡Has ganado!" }
                            Some(w) => { (icon("crown")) "Gana " (seats[w].name) }
                            None => { @if m.note.is_empty() { "Empate" } @else { "Partida terminada" } }
                        }
                    }
                    @if !m.note.is_empty() { span.gm-last { (m.note) } }
                    @if me.is_some() {
                        form.gm-again method="post" action=(action("again")) { (v.ctx()) button.btn.primary.sm { (icon("refresh-cw")) "Revancha" } }
                    }
                } @else if my_turn {
                    span.gm-turn.you {
                        span.gm-turn-txt { b { "¡Te toca!" } small { (hint) } }
                    }
                    @if let Some((s, text)) = log.last() {
                        span.gm-last style={ "--c:" (cx.color(*s)) } { i {} b { (cx.name(*s)) } " " (text) }
                    }
                } @else {
                    span.gm-turn.them style={ "--c:" (cx.color(m.turn)) } {
                        (cx.face(m.turn))
                        span.gm-turn-txt {
                            b { "Turno de " (cx.name(m.turn)) }
                            small.gm-think { @if seats.get(m.turn).is_some_and(|s| s.bot) { "pensando" } @else { "esperando su jugada" } i {} i {} i {} }
                        }
                    }
                    @if let Some((s, text)) = log.last() {
                        span.gm-last style={ "--c:" (cx.color(*s)) } { i {} b { (cx.name(*s)) } " " (text) }
                    }
                }
            }
            @if my_turn {
                div.gm-pop aria-hidden="true" {
                    b.gm-pop-title { @for (i, ch) in "¡Te toca!".chars().enumerate() { span style={ "--i:" (i) } { (ch) } } }
                    span.gm-pop-hint { (hint) }
                }
            }
            form.gm-move.gm-live-form method="post" action=(action("move")) { (v.ctx()) (board) }
            div.gm-foot {
                @if me.is_some() {
                    div.gm-tools {
                        button.btn.ghost.sm.gm-mute type="button" aria-label="Sonido" title="Sonido" { (icon("volume-2")) (icon("volume-x")) }
                        button.btn.ghost.sm.gm-full type="button" aria-label="Pantalla completa" title="Pantalla completa" { (icon("maximize-2")) (icon("minimize-2")) }
                        details.gm-react {
                            summary.btn.outline.sm aria-label="Reaccionar" { (icon("smile")) }
                            form.gm-live-form.gm-emojis.emoji[with_emoji(&m.game)] method="post" action=(action("react")) {
                                (v.ctx())
                                @if with_emoji(&m.game) {
                                    @for e in EMOJI { button type="submit" name="e" value=(e) { (e) } }
                                } @else {
                                    @for (id, ic, color, label) in REACTIONS {
                                        button type="submit" name="e" value=(id) style={ "--r:" (color) } aria-label=(label) title=(label) { (icon(ic)) }
                                    }
                                }
                            }
                        }
                        @if !m.done {
                            details.gm-leave {
                                summary.btn.ghost.sm { (icon("flag")) "Abandonar" }
                                form method="post" action=(action("leave")) {
                                    (v.ctx())
                                    span.hint { "¿Seguro? Cuenta como derrota." }
                                    button.btn.destructive.sm { "Sí, abandonar" }
                                }
                            }
                        }
                    }
                }
            }
        }
    })
}

/// Which sound the latest happening deserves (app.js plays it when the board changes).
fn sfx(game: &str, last: Option<&str>) -> &'static str {
    let t = last.unwrap_or("");
    match () {
        _ if t.contains("encuentra la pareja") => "reveal",
        _ if t.contains("no encuentra") => "skip",
        _ if t.contains("pone el") => "card",
        _ if t.contains("pasa") && game == "domino" => "skip",
        _ if t.contains("ronda") => "reveal",
        _ if t.contains("le queda una") => "one",
        _ if t.contains("roba dos") => "two",
        _ if t.contains("comodín") => "wild",
        _ if t.contains("echa Salta") => "skip",
        _ if t.contains("echa Gira") => "turn-around",
        _ if t.contains("hundido") => "sink",
        _ if t.contains("tocado") => "hit",
        _ if t.contains("agua") => "splash",
        _ if t.contains("come") || t.contains("calavera") || t.contains("pozo") || t.contains("cárcel") => "eat",
        _ if t.contains("roba") => "draw",
        _ if t.contains("echa") => "card",
        _ if t.contains("saca") || t.contains("mueve") || t.contains("avanza") || t.contains("oca") => "dice",
        _ if game == "c4" => "drop",
        _ => "tap",
    }
}

fn html_ok(m: Markup) -> rusqlite::Result<Markup> {
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(id: i64) -> View {
        let people = vec![
            Person { id: 1, name: "Ana".into(), color: "#2f6bff".into(), photo: 7 },
            Person { id: 2, name: "Leo".into(), color: "#2f6bff".into(), photo: 0 },
        ];
        View { m: "2026-11".into(), me: Me { id, name: "Ana".into(), people } }
    }

    #[test]
    fn seats_never_share_a_colour() {
        let s = seats(&view(1), &[1, 2, 0, 0]);
        let mut colors: Vec<&str> = s.iter().map(|s| s.color.as_str()).collect();
        colors.sort();
        colors.dedup();
        assert_eq!(colors.len(), 4);
        assert_eq!((s[2].name.as_str(), s[3].name.as_str()), ("Robot", "Robot 2"));
        assert_eq!((s[0].photo.as_deref(), s[1].photo.as_deref()), (Some("/avatars/1?v=7"), None), "faces come from Nido");
    }

    /// A finished match writes one result per person; the ranking sums them.
    #[test]
    fn results_feed_the_ranking() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../schema.sql")).unwrap();
        let mut rng = Rng::seeded(4);
        let players = vec![1i64, 2];
        let st = games::create("ttt", &bots(&players), "", &mut rng).unwrap();
        c.execute("INSERT INTO matches(id,game,players,state,turn) VALUES(1,'ttt','[1,2]',?1,0)", [&st.state]).unwrap();
        // Ana: 0,1,2 on the top row; Leo: 3,4.
        for (seat, cell) in [(0, "0"), (1, "3"), (0, "1"), (1, "4"), (0, "2")] {
            let m = load(&c, 1).unwrap().unwrap();
            let st = games::act("ttt", &m.state, seat, cell, &mut rng).unwrap().unwrap();
            save(&c, &m, &st).unwrap();
        }
        let m = load(&c, 1).unwrap().unwrap();
        assert!(m.done && m.winner == Some(0));
        let pts: Vec<(i64, i64)> = c.prepare("SELECT user_id, points FROM results ORDER BY user_id").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect();
        assert_eq!(pts, vec![(1, 3), (2, 0)]);
        let html = gallery(&c, &view(1), None).unwrap().into_string();
        assert!(html.contains("Has ganado") && html.contains(">3 pts"));
    }
}
