//! The Game Boy Advance Wireless Adapter, as the game sees it through the link port, and its
//! radio as messages to the other phone over the app's two-phone session.
//!
//! Written from afska's "Game Boy Advance Wireless Adapter" document (gba-link-connection,
//! docs/wireless_adapter.md, MIT) and GBATEK's "GBA Wireless Adapter" pages, and checked against
//! that library's own demo (tests/gba.rs). No code from either.
//!
//! The game talks to it in normal 32-bit mode, driving the clock:
//! - a start-up exchange of the word "NINTENDO", two letters at a time, each side echoing the
//!   other's last half inverted;
//! - then commands, `$9966LLCC` with LL words of parameters, answered by `$9966RR(CC+$80)` and RR
//!   words; while either side has nothing to say it sends `$80000000`;
//! - after a "wait" command the adapter drives the clock instead and clocks an event into the
//!   game (`$99660028` data arrived, `$99660027` timed out), which the game acknowledges.
//! Between words the two sides wave a pin at each other (SO and SI) to say they are ready: the
//! adapter answers SO by putting the opposite on SI while the game drives the clock, and the same
//! while the adapter does (`si`).
//!
//! The radio carries five kinds of message between the two adapters: a host's broadcast (and its
//! end), a client asking to join and the host's answer, and data either way. As on the hardware,
//! a client's data only moves when the host sends.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

const HEADER: u32 = 0x9966;
pub const DATA_REQUEST: u32 = 0x8000_0000;
/// The "NINTENDO" exchange: the adapter's own half for each step.
const LOGIN: [u16; 5] = [0x494E, 0x544E, 0x4E45, 0x4F44, 0x8001];
/// What `VersionStatus` reports (afska's adapter).
const VERSION: u32 = 8_585_495;
/// The message tag on the cable for the radio (the cable's own packets use 'c' and 'a').
pub const RADIO_TAG: u8 = b'W';
const MSG_BROADCAST: u8 = b'B';
const MSG_BROADCAST_END: u8 = b'b';
const MSG_JOIN: u8 = b'J';
const MSG_ACCEPT: u8 = b'A';
const MSG_HOST_DATA: u8 = b'D';
const MSG_CLIENT_DATA: u8 = b'd';
const MSG_LEAVE: u8 = b'X';
/// A host heard of no more for this long disappears from a search (the document: 3 seconds).
const FORGET_FRAMES: u32 = 180;
/// How often a host repeats its broadcast, so a search started later still hears it.
const REPEAT_FRAMES: u32 = 30;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum Phase {
    /// The "NINTENDO" exchange, at step `n`.
    #[default]
    Login,
    /// Waiting for a command word.
    Command,
    /// Taking a command's parameters.
    Params,
    /// The adapter drives the clock: waiting for something to tell the game.
    Waiting,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Rfu {
    /// An adapter is plugged in (the game has looked for one and the setting allows it).
    pub active: bool,
    phase: Phase,
    login_step: usize,
    login_started: bool,
    last_gba: u32,
    last_reply: u32,
    cmd: u8,
    params_left: u8,
    params: Vec<u32>,
    /// Words for the game, one an exchange: the acknowledgement and the reply's data.
    replies: VecDeque<u32>,
    /// While the adapter drives the clock: words to clock into the game.
    events: VecDeque<u32>,
    /// The game's word from the last transfer the adapter clocked.
    pub game_word: u32,
    /// Frames left before a wait times out (0: no timeout set).
    wait_left: u32,
    timeout: u32,
    id: u16,
    next_id: u16,
    broadcast: [u32; 6],
    hosting: bool,
    open: bool,
    /// As host: the client on the other phone (its id), number 0.
    client: Option<u16>,
    /// As client: the host joined (its id) and whether it has answered.
    joined: Option<u16>,
    accepted: bool,
    searching: bool,
    /// Hosts heard: id, broadcast, frames since.
    heard: Vec<(u16, [u32; 6], u32)>,
    /// Data received and not yet read: bytes from the host (as client) or from client 0 (host).
    rx: Vec<u8>,
    /// As host: the client's data that has come but only counts once the host sends.
    client_scheduled: Option<Vec<u8>>,
    host_sent: bool,
    last_payload: Vec<u8>,
    frames: u32,
    /// Messages for the other phone.
    #[serde(skip)]
    pub radio_out: VecDeque<Vec<u8>>,
}

fn le(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn words_of(bytes: &[u8]) -> Vec<u32> {
    bytes.chunks(4).map(|c| {
        let mut b = [0u8; 4];
        b[..c.len()].copy_from_slice(c);
        u32::from_le_bytes(b)
    }).collect()
}

impl Rfu {
    /// Plugged in, or the game reset it (by its SD pin): back to the "NINTENDO" exchange.
    pub fn reset(&mut self) {
        let keep = (self.next_id, self.radio_out.clone(), self.frames);
        if self.hosting || self.joined.is_some() {
            self.radio(MSG_LEAVE, &[]);
        }
        *self = Rfu { active: true, next_id: keep.0, radio_out: keep.1, frames: keep.2, ..Rfu::default() };
    }

    /// The SI pin as the game reads it, given what it put on SO and who drives the clock.
    pub fn si(&self, so_high: bool, game_drives: bool) -> bool {
        if game_drives { !so_high } else { so_high }
    }

    fn new_id(&mut self, parent: bool) -> u16 {
        self.next_id = self.next_id.wrapping_add(1);
        // Different on the two phones, never 0.
        (if parent { 0x2A00 } else { 0x5300 }) | (self.next_id & 0xFF).max(1)
    }

    fn radio(&mut self, kind: u8, body: &[u8]) {
        let mut m = vec![RADIO_TAG, kind];
        m.extend_from_slice(body);
        self.radio_out.push_back(m);
    }

    /// One 32-bit exchange the game drives: its word in, the adapter's word out.
    pub fn exchange(&mut self, w: u32, parent: bool) -> u32 {
        let reply = match self.phase {
            Phase::Login if w >> 16 == HEADER => {
                self.phase = Phase::Command;
                return self.exchange(w, parent);
            }
            Phase::Login => {
                // Each answer is worked out from the exchange before it: the game's last half
                // inverted, after the adapter's own two letters, which move on once the game has
                // echoed them back inverted (the document's table, step by step).
                let out = if !self.login_started {
                    0
                } else {
                    let echoed = self.last_reply != 0 && (self.last_gba >> 16) as u16 == !((self.last_reply >> 16) as u16);
                    if echoed {
                        self.login_step = (self.login_step + 1).min(LOGIN.len() - 1);
                    }
                    u32::from(LOGIN[self.login_step]) << 16 | u32::from(!(self.last_gba as u16))
                };
                self.login_started = true;
                self.last_gba = w;
                self.last_reply = out;
                return out;
            }
            Phase::Command => {
                if w >> 16 == HEADER {
                    self.cmd = w as u8;
                    self.params_left = (w >> 8) as u8;
                    self.params.clear();
                    if self.params_left == 0 {
                        self.run(parent);
                    } else {
                        self.phase = Phase::Params;
                    }
                    DATA_REQUEST
                } else {
                    self.replies.pop_front().unwrap_or(DATA_REQUEST)
                }
            }
            Phase::Params => {
                self.params.push(w);
                self.params_left -= 1;
                if self.params_left == 0 {
                    self.phase = Phase::Command;
                    self.run(parent);
                }
                DATA_REQUEST
            }
            // The acknowledgement of the wait command is still read the usual way.
            Phase::Waiting => self.replies.pop_front().unwrap_or(DATA_REQUEST),
        };
        reply
    }

    /// While the adapter drives the clock: the word it clocks into the game, if it has one.
    pub fn clock_out(&mut self) -> Option<u32> {
        if self.phase != Phase::Waiting {
            return None;
        }
        let w = self.events.pop_front()?;
        if self.events.is_empty() {
            // The game's acknowledgement is the next word; then the game has the clock again.
            self.phase = Phase::Command;
        }
        Some(w)
    }

    /// Whether the adapter has a word to clock into the game right now.
    pub fn has_event(&self) -> bool {
        self.phase == Phase::Waiting && !self.events.is_empty()
    }

    fn ack(&mut self, words: &[u32]) {
        self.replies.clear();
        self.replies.push_back(HEADER << 16 | (words.len() as u32) << 8 | u32::from(self.cmd.wrapping_add(0x80)));
        self.replies.extend(words.iter().copied());
    }

    fn error(&mut self, code: u32) {
        self.replies.clear();
        self.replies.push_back(0x9966_01EE);
        self.replies.push_back(code);
    }

    fn start_wait(&mut self) {
        self.phase = Phase::Waiting;
        self.events.clear();
        self.wait_left = self.timeout;
        self.check_wait();
    }

    /// While waiting: data that has come wakes the game.
    fn check_wait(&mut self) {
        if self.phase == Phase::Waiting && self.events.is_empty() && !self.rx.is_empty() {
            self.events.push_back(0x9966_0028);
            self.events.push_back(DATA_REQUEST);
        }
    }

    fn run(&mut self, parent: bool) {
        let p = std::mem::take(&mut self.params);
        match self.cmd {
            0x10 => self.ack(&[]),
            0x11 => {
                let level = if self.client.is_some() || self.accepted { 0xFF } else { 0 };
                self.ack(&[level]);
            }
            0x12 => self.ack(&[VERSION]),
            0x13 => {
                let state = if self.hosting { if self.open { 2 } else { 1 } } else if self.accepted { 5 } else if self.joined.is_some() { 4 } else if self.searching { 3 } else { 0 };
                let slots = if self.accepted { 1 << 16 } else { 0 };
                self.ack(&[state << 24 | slots | u32::from(self.id)]);
            }
            0x14 => {
                let next = if self.hosting && self.open && self.client.is_none() { 0 } else { 0xFF };
                let mut w = vec![next];
                if let Some(c) = self.client {
                    w.push(u32::from(c));
                }
                self.ack(&w);
            }
            0x15 => {
                let mut w = if self.hosting { self.broadcast.to_vec() } else { vec![0; 6] };
                if self.hosting {
                    w.push(0x003C_0420);
                }
                w.push(257);
                self.ack(&w);
            }
            0x16 => {
                if p.len() == 6 {
                    self.broadcast.copy_from_slice(&p);
                    if self.hosting && self.open {
                        self.send_broadcast();
                    }
                }
                self.ack(&[]);
            }
            0x17 => {
                self.timeout = p.first().map_or(0, |v| v & 0xFF);
                self.ack(&[]);
            }
            0x19 => {
                if !self.hosting {
                    self.id = self.new_id(parent);
                }
                self.hosting = true;
                self.open = true;
                self.send_broadcast();
                self.ack(&[]);
            }
            0x1A => {
                if !(self.hosting && self.open) {
                    return self.error(1);
                }
                let w: Vec<u32> = self.client.iter().map(|c| u32::from(*c)).collect();
                self.ack(&w);
            }
            0x1B => {
                if !self.hosting {
                    return self.error(1);
                }
                self.open = false;
                self.radio(MSG_BROADCAST_END, &self.id.to_le_bytes().to_vec());
                let w: Vec<u32> = self.client.iter().map(|c| u32::from(*c)).collect();
                self.ack(&w);
            }
            0x1C => {
                self.searching = true;
                self.ack(&[]);
            }
            0x1D | 0x1E => {
                if !self.searching {
                    return self.error(1);
                }
                let mut w = Vec::new();
                for (id, data, _) in self.heard.iter().take(4) {
                    w.push(u32::from(*id));
                    w.extend_from_slice(data);
                }
                if self.cmd == 0x1E {
                    self.searching = false;
                }
                self.ack(&w);
            }
            0x1F => {
                let host = p.first().map_or(0, |v| *v as u16);
                self.id = self.new_id(parent);
                self.joined = Some(host);
                self.accepted = false;
                let mut body = self.id.to_le_bytes().to_vec();
                body.extend_from_slice(&host.to_le_bytes());
                self.radio(MSG_JOIN, &body);
                self.ack(&[]);
            }
            0x20 | 0x21 => {
                if self.joined.is_none() {
                    return self.error(1);
                }
                let w = if self.accepted { u32::from(self.id) } else if self.cmd == 0x20 { 0x0100_0000 } else { 0x0004_0000 | u32::from(self.id) };
                self.ack(&[w]);
            }
            0x24 | 0x25 => {
                self.send_data(&p);
                self.ack(&[]);
                if self.cmd == 0x25 {
                    self.start_wait();
                }
            }
            0x26 => {
                let rx = std::mem::take(&mut self.rx);
                if rx.is_empty() {
                    return self.ack(&[]);
                }
                let header = if self.hosting { (rx.len() as u32 & 0x1F) << 8 } else { rx.len() as u32 & 0x7F };
                let mut w = vec![header];
                w.extend(words_of(&rx));
                self.ack(&w);
            }
            0x27 | 0x37 => {
                if self.cmd == 0x37 && self.hosting {
                    let last = self.last_payload.clone();
                    self.radio(MSG_HOST_DATA, &last);
                }
                self.ack(&[]);
                self.start_wait();
            }
            0x30 => {
                if self.hosting {
                    if self.client.take().is_some() {
                        self.radio(MSG_LEAVE, &[]);
                    }
                } else if self.joined.take().is_some() {
                    self.accepted = false;
                    self.radio(MSG_LEAVE, &[]);
                }
                self.ack(&[]);
            }
            0x3D => {
                if self.hosting || self.joined.is_some() {
                    self.radio(MSG_LEAVE, &[]);
                }
                if self.hosting && self.open {
                    self.radio(MSG_BROADCAST_END, &self.id.to_le_bytes().to_vec());
                }
                self.hosting = false;
                self.open = false;
                self.client = None;
                self.joined = None;
                self.accepted = false;
                self.ack(&[]);
            }
            _ => self.error(2),
        }
    }

    fn send_broadcast(&mut self) {
        let mut body = self.id.to_le_bytes().to_vec();
        body.extend(le(&self.broadcast));
        self.radio(MSG_BROADCAST, &body);
    }

    /// SendData: the first word is the header, the number of bytes (shifted for a client).
    fn send_data(&mut self, p: &[u32]) {
        let Some((&header, data)) = p.split_first() else { return };
        let bytes = if self.hosting { (header & 0x7F) as usize } else { ((header >> 8) & 0x1F) as usize };
        let mut payload = le(data);
        if data.is_empty() {
            // A "ghost send": the last bytes again.
            payload = self.last_payload.clone();
        }
        payload.truncate(bytes);
        self.last_payload = payload.clone();
        if self.hosting {
            self.radio(MSG_HOST_DATA, &payload);
            // The client's data that was waiting for this send now counts.
            if let Some(d) = self.client_scheduled.take() {
                self.rx = d;
            } else {
                self.host_sent = true;
            }
        } else if self.accepted {
            self.radio(MSG_CLIENT_DATA, &payload);
        }
    }

    /// A message from the other phone's adapter.
    pub fn hear(&mut self, m: &[u8]) {
        let [RADIO_TAG, kind, body @ ..] = m else { return };
        let id = |b: &[u8]| b.get(..2).map_or(0, |x| u16::from_le_bytes([x[0], x[1]]));
        match *kind {
            MSG_BROADCAST if body.len() >= 26 => {
                let mut data = [0u32; 6];
                data.copy_from_slice(&words_of(&body[2..26])[..6]);
                let who = id(body);
                match self.heard.iter_mut().find(|h| h.0 == who) {
                    Some(h) => *h = (who, data, 0),
                    None => self.heard.push((who, data, 0)),
                }
            }
            MSG_BROADCAST_END => {
                let who = id(body);
                self.heard.retain(|h| h.0 != who);
            }
            MSG_JOIN if body.len() >= 4 => {
                let (who, host) = (id(body), id(&body[2..]));
                if self.hosting && self.open && host == self.id && self.client.is_none() {
                    self.client = Some(who);
                    let mut b = who.to_le_bytes().to_vec();
                    b.push(0);
                    self.radio(MSG_ACCEPT, &b);
                }
            }
            MSG_ACCEPT => {
                if self.joined.is_some() && id(body) == self.id {
                    self.accepted = true;
                }
            }
            MSG_HOST_DATA => {
                if self.accepted {
                    self.rx = body.to_vec();
                    self.check_wait();
                }
            }
            MSG_CLIENT_DATA => {
                if self.client.is_some() {
                    if self.host_sent {
                        self.host_sent = false;
                        self.rx = body.to_vec();
                        self.check_wait();
                    } else {
                        self.client_scheduled = Some(body.to_vec());
                    }
                }
            }
            MSG_LEAVE => {
                self.client = None;
                if self.joined.is_some() {
                    self.joined = None;
                    self.accepted = false;
                }
            }
            _ => {}
        }
    }

    /// Once a frame: a wait that has run out tells the game so; hosts repeat their broadcast;
    /// hosts not heard for a while are forgotten.
    pub fn frame(&mut self) {
        self.frames = self.frames.wrapping_add(1);
        for h in self.heard.iter_mut() {
            h.2 += 1;
        }
        self.heard.retain(|h| h.2 < FORGET_FRAMES);
        if self.hosting && self.open && self.frames % REPEAT_FRAMES == 0 {
            self.send_broadcast();
        }
        if self.phase == Phase::Waiting && self.events.is_empty() && self.wait_left > 0 {
            self.wait_left -= 1;
            if self.wait_left == 0 {
                self.events.push_back(0x9966_0027);
                self.events.push_back(DATA_REQUEST);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The game's side of the "NINTENDO" exchange, as the document lays it out.
    fn login(r: &mut Rfu) {
        let mut prev_adapter: u16 = 0;
        let parts: [u16; 10] = [0x494E, 0x494E, 0x494E, 0x544E, 0x544E, 0x4E45, 0x4E45, 0x4F44, 0x4F44, 0x8001];
        for (i, part) in parts.iter().enumerate() {
            let w = u32::from(!prev_adapter) << 16 | u32::from(*part);
            let got = r.exchange(w, true);
            if i >= 2 {
                assert_eq!(got >> 16, u32::from(parts[i]), "step {i}: {got:08X}");
            }
            prev_adapter = (got >> 16) as u16;
        }
    }

    fn command(r: &mut Rfu, cmd: u8, params: &[u32], parent: bool) -> Vec<u32> {
        assert_eq!(r.exchange(HEADER << 16 | (params.len() as u32) << 8 | u32::from(cmd), parent), DATA_REQUEST);
        for p in params {
            assert_eq!(r.exchange(*p, parent), DATA_REQUEST);
        }
        let ack = r.exchange(DATA_REQUEST, parent);
        assert_eq!(ack >> 16, HEADER, "{ack:08X}");
        assert_eq!(ack as u8, cmd.wrapping_add(0x80), "{ack:08X}");
        (0..(ack >> 8) & 0xFF).map(|_| r.exchange(DATA_REQUEST, parent)).collect()
    }

    #[test]
    fn the_nintendo_exchange_matches_the_document() {
        let mut r = Rfu::default();
        r.reset();
        let table = [
            (0x7FFF_494E, 0x0000_0000), (0xFFFF_494E, 0x494E_B6B1), (0xB6B1_494E, 0x494E_B6B1),
            (0xB6B1_544E, 0x544E_B6B1), (0xABB1_544E, 0x544E_ABB1), (0xABB1_4E45, 0x4E45_ABB1),
            (0xB1BA_4E45, 0x4E45_B1BA), (0xB1BA_4F44, 0x4F44_B1BA), (0xB0BB_4F44, 0x4F44_B0BB),
            (0xB0BB_8001, 0x8001_B0BB),
        ];
        for (gba, adapter) in table {
            assert_eq!(r.exchange(gba, true), adapter, "the game sent {gba:08X}");
        }
        let mut again = Rfu::default();
        again.reset();
        login(&mut again);
    }

    #[test]
    fn two_adapters_find_each_other_join_and_swap_data() {
        let (mut host, mut client) = (Rfu::default(), Rfu::default());
        host.reset();
        client.reset();
        login(&mut host);
        login(&mut client);
        let carry = |a: &mut Rfu, b: &mut Rfu| {
            while let Some(m) = a.radio_out.pop_front() {
                b.hear(&m);
            }
            while let Some(m) = b.radio_out.pop_front() {
                a.hear(&m);
            }
        };
        command(&mut host, 0x10, &[], true);
        command(&mut host, 0x17, &[0x003C_0420], true);
        command(&mut host, 0x16, &[1, 2, 3, 4, 5, 6], true);
        command(&mut host, 0x19, &[], true);
        command(&mut client, 0x1C, &[], false);
        carry(&mut host, &mut client);
        let found = command(&mut client, 0x1D, &[], false);
        assert_eq!(found.len(), 7, "one host, its id and six words");
        assert_eq!(&found[1..], &[1, 2, 3, 4, 5, 6]);
        command(&mut client, 0x1E, &[], false);
        assert_eq!(command(&mut client, 0x1F, &[found[0]], false), Vec::<u32>::new());
        assert_eq!(command(&mut client, 0x20, &[], false), vec![0x0100_0000], "still connecting");
        carry(&mut host, &mut client);
        carry(&mut host, &mut client);
        let done = command(&mut client, 0x20, &[], false);
        assert_eq!(done[0] >> 16, 0, "client number 0");
        let polled = command(&mut host, 0x1A, &[], true);
        assert_eq!(polled, vec![done[0] & 0xFFFF]);
        // The host sends 4 bytes; the client waits for them.
        command(&mut client, 0x25, &[4 << 8, 0xAABB_CCDD], false);
        assert!(!client.has_event(), "nothing yet");
        command(&mut host, 0x24, &[4, 0x1234_5678], true);
        carry(&mut host, &mut client);
        assert_eq!(client.clock_out(), Some(0x9966_0028), "data arrived");
        assert_eq!(client.clock_out(), Some(DATA_REQUEST));
        assert_eq!(command(&mut client, 0x26, &[], false), vec![4, 0x1234_5678]);
        // The client's data came with its SendDataWait and counts after the host's send.
        let got = command(&mut host, 0x26, &[], true);
        assert_eq!(got, vec![4 << 8, 0xAABB_CCDD]);
    }

    #[test]
    fn an_unknown_command_and_one_out_of_turn_are_errors() {
        let mut r = Rfu::default();
        r.reset();
        login(&mut r);
        assert_eq!(r.exchange(0x9966_0018, true), DATA_REQUEST);
        assert_eq!((r.exchange(DATA_REQUEST, true), r.exchange(DATA_REQUEST, true)), (0x9966_01EE, 2));
        assert_eq!(r.exchange(0x9966_001D, true), DATA_REQUEST);
        assert_eq!((r.exchange(DATA_REQUEST, true), r.exchange(DATA_REQUEST, true)), (0x9966_01EE, 1));
    }

    #[test]
    fn a_wait_with_nothing_coming_times_out() {
        let mut r = Rfu::default();
        r.reset();
        login(&mut r);
        command(&mut r, 0x17, &[0x003C_0420], true);
        command(&mut r, 0x27, &[], true);
        for _ in 0..0x1F {
            r.frame();
            assert!(!r.has_event());
        }
        r.frame();
        assert_eq!(r.clock_out(), Some(0x9966_0027));
    }
}
