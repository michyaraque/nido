//! Real time: one WebSocket per open match page, speaking a small binary protocol.
//!
//! A WebSocket message carries one or more packets:
//!
//! ```text
//! packet := u32 length (LE, of what follows) · u8 opcode · payload
//! u8/u16/u32 little-endian · str = u16 length + UTF-8 · text = u32 length + UTF-8
//! ```
//!
//! | op   | direction | payload |
//! |------|-----------|---------|
//! | 0x01 HELLO    | → server | u32 match · u32 version the page has |
//! | 0x02 MOVE     | → server | u32 match · str move |
//! | 0x03 REACT    | → server | u32 match · str reaction (an icon id like "heart"; an emoji in Colors) |
//! | 0x04 PING     | → server | — |
//! | 0x81 VIEW     | → phone  | u32 match · u32 version · u8 sound · u8 flags · text html |
//! | 0x82 REACTION | → phone  | u32 match · u8 seat · str reaction id |
//! | 0x83 ERROR    | → phone  | str message |
//! | 0x84 PONG     | → phone  | — |
//!
//! VIEW flags: 1 my turn · 2 finished · 4 I won · 8 I lost. The html is that viewer's own view
//! (cards and ships stay hidden from the others), drawn by the server like any other page.
use crate::*;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use std::sync::OnceLock;
use tokio::sync::broadcast;

pub mod op {
    pub const HELLO: u8 = 0x01;
    pub const MOVE: u8 = 0x02;
    pub const REACT: u8 = 0x03;
    pub const PING: u8 = 0x04;
    pub const VIEW: u8 = 0x81;
    pub const REACTION: u8 = 0x82;
    pub const ERROR: u8 = 0x83;
    pub const PONG: u8 = 0x84;
}

/// A buffer with a reading position, like a game server's NetworkMessage.
#[derive(Default)]
pub struct NetworkMessage {
    buf: Vec<u8>,
    pos: usize,
}

impl NetworkMessage {
    pub fn read(bytes: &[u8]) -> Self {
        NetworkMessage { buf: bytes.to_vec(), pos: 0 }
    }
    pub fn add_u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub fn add_u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn add_u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    /// Short text: u16 length + UTF-8 (cut at 65535 bytes, on a character boundary).
    pub fn add_str(&mut self, s: &str) {
        let mut end = s.len().min(u16::MAX as usize);
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        self.add_u16(end as u16);
        self.buf.extend_from_slice(&s.as_bytes()[..end]);
    }
    /// Long text: u32 length + UTF-8.
    pub fn add_text(&mut self, s: &str) {
        self.add_u32(s.len() as u32);
        self.buf.extend_from_slice(s.as_bytes());
    }
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let end = self.pos.checked_add(n)?;
        let out = self.buf.get(self.pos..end)?;
        self.pos = end;
        Some(out)
    }
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn get_u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    pub fn get_u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
    pub fn get_u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn get_str(&mut self) -> Option<String> {
        let n = self.get_u16()? as usize;
        self.take(n).and_then(|b| std::str::from_utf8(b).ok()).map(str::to_string)
    }
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }
}

/// One packet: length, opcode, then whatever `body` writes.
pub fn packet(opcode: u8, body: impl FnOnce(&mut NetworkMessage)) -> Vec<u8> {
    let mut m = NetworkMessage::default();
    m.add_u32(0); // length, filled in below
    m.add_u8(opcode);
    body(&mut m);
    let len = (m.buf.len() - 4) as u32;
    m.buf[..4].copy_from_slice(&len.to_le_bytes());
    m.buf
}

/// The packets inside a WebSocket message. A malformed one ends the list: nothing after it is trusted.
pub fn packets(bytes: &[u8]) -> Vec<(u8, NetworkMessage)> {
    let mut out = Vec::new();
    let mut m = NetworkMessage::read(bytes);
    while let Some(len) = m.get_u32() {
        let Some(body) = (len >= 1).then(|| m.take(len as usize)).flatten() else { break };
        out.push((body[0], NetworkMessage::read(&body[1..])));
    }
    out
}

// ---------- who is watching what ----------

#[derive(Clone, Debug)]
pub enum Hub {
    Changed(i64),
    Reaction(i64, u8, String),
}

fn hub() -> &'static broadcast::Sender<Hub> {
    static HUB: OnceLock<broadcast::Sender<Hub>> = OnceLock::new();
    HUB.get_or_init(|| broadcast::channel(256).0)
}

/// Something happened in a match: every phone looking at it gets its own new view.
pub fn changed(id: i64) {
    let _ = hub().send(Hub::Changed(id));
}

pub async fn ws(State(db): State<Db>, h: HeaderMap, Query(p): Query<Params>, up: WebSocketUpgrade) -> R {
    let me = require_user!(h);
    let month = p.get("month").cloned();
    Ok(up.max_message_size(16 * 1024).on_upgrade(move |socket| session(db, me, month, socket)))
}

async fn session(db: Db, me: Me, month: Option<String>, mut socket: WebSocket) {
    let mut rx = hub().subscribe();
    let mut watching: Option<i64> = None;
    let mut sent: i64 = -1; // version of the last view this phone got
    loop {
        let out: Vec<u8> = tokio::select! {
            msg = socket.recv() => {
                let Some(Ok(msg)) = msg else { return };
                let Message::Binary(bytes) = msg else { continue };
                let mut out = Vec::new();
                for (opcode, mut m) in packets(&bytes) {
                    match opcode {
                        op::HELLO => {
                            let (Some(id), Some(v)) = (m.get_u32(), m.get_u32()) else { break };
                            watching = Some(id as i64);
                            sent = v as i64;
                            out.extend(view(&db, &me, &month, id as i64, &mut sent));
                        }
                        op::MOVE => {
                            let (Some(id), Some(mv)) = (m.get_u32(), m.get_str()) else { break };
                            let done = app::do_move(&lock(&db), me.id, id as i64, &mv);
                            match done {
                                Ok(()) => {
                                    changed(id as i64);
                                    app::robots(db.clone(), id as i64);
                                }
                                Err(e) => out.extend(packet(op::ERROR, |m| m.add_str(&e))),
                            }
                        }
                        op::REACT => {
                            let (Some(id), Some(e)) = (m.get_u32(), m.get_str()) else { break };
                            let done = app::do_react(&lock(&db), me.id, id as i64, &e);
                            match done {
                                Ok(seat) => { let _ = hub().send(Hub::Reaction(id as i64, seat as u8, e)); }
                                Err(e) => out.extend(packet(op::ERROR, |m| m.add_str(e))),
                            }
                        }
                        op::PING => out.extend(packet(op::PONG, |_| {})),
                        _ => break,
                    }
                }
                out
            }
            ev = rx.recv() => match ev {
                Ok(Hub::Changed(id)) if Some(id) == watching => view(&db, &me, &month, id, &mut sent),
                Ok(Hub::Reaction(id, seat, e)) if Some(id) == watching => packet(op::REACTION, |m| { m.add_u32(id as u32); m.add_u8(seat); m.add_str(&e) }),
                Err(broadcast::error::RecvError::Lagged(_)) => watching.map(|id| { sent = -1; view(&db, &me, &month, id, &mut sent) }).unwrap_or_default(),
                Err(broadcast::error::RecvError::Closed) => return,
                _ => continue,
            },
        };
        if !out.is_empty() && socket.send(Message::Binary(out.into())).await.is_err() {
            return;
        }
    }
}

/// This viewer's VIEW packet, unless the phone already has this version.
fn view(db: &Db, me: &Me, month: &Option<String>, id: i64, sent: &mut i64) -> Vec<u8> {
    let c = lock(db);
    let Ok(m) = crate::month(&c, month.as_deref()) else { return Vec::new() };
    let v = View { m, me: me.clone() };
    match app::snapshot(&c, &v, id) {
        Ok(Some(s)) if s.version != *sent => {
            *sent = s.version;
            packet(op::VIEW, |m| {
                m.add_u32(id as u32);
                m.add_u32(s.version as u32);
                m.add_u8(s.sound);
                m.add_u8(s.flags);
                m.add_text(&s.html);
            })
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packets_round_trip_and_reject_garbage() {
        let mut bytes = packet(op::HELLO, |m| {
            m.add_u32(42);
            m.add_u32(7);
        });
        bytes.extend(packet(op::MOVE, |m| {
            m.add_u32(42);
            m.add_str("play:3:2 ñ");
        }));
        let got = packets(&bytes);
        assert_eq!(got.len(), 2);
        let (o, mut m) = packets(&bytes).into_iter().next().unwrap();
        assert_eq!((o, m.get_u32(), m.get_u32(), m.get_u8()), (op::HELLO, Some(42), Some(7), None));
        let (o, mut m) = packets(&bytes).into_iter().nth(1).unwrap();
        assert_eq!((o, m.get_u32(), m.get_str().as_deref()), (op::MOVE, Some(42), Some("play:3:2 ñ")));
        // A length past the end, or zero, stops the reading.
        assert!(packets(&[9, 0, 0, 0, op::PING]).is_empty());
        assert!(packets(&[0, 0, 0, 0]).is_empty());
        assert!(packets(&[1, 2]).is_empty());
        // A string longer than its packet does not read past it.
        let mut m = NetworkMessage::read(&[200, 0, b'a']);
        assert_eq!(m.get_str(), None);
        // Cut on a character boundary.
        let mut m = NetworkMessage::default();
        m.add_str(&"ñ".repeat(40_000));
        assert_eq!(NetworkMessage::read(m.bytes()).get_str().map(|s| s.len() % 2), Some(0));
    }
}
