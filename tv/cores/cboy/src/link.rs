//! The cable between two phones.
//!
//! The link port itself is in `bus.rs`: a shift register, its own clock, and the two clock modes
//! a Game Boy has. This is the cable that carries bytes from one of those ports to another one
//! on a different phone, and the agreement that keeps the two ends in step.
//!
//! How the two sides agree. A Game Boy cable has no master and no slave of its own: whichever
//! console is set to its internal clock drives both shift registers, and the other one is
//! dragged along. So every byte that crosses is one of exactly two things, and both phones apply
//! them in the order they arrive:
//!
//! - a **clock**: "I drove the cable and shifted this byte out", sent the moment the game starts
//!   the transfer, with a number on it;
//! - an **answer**: "here is what my shift register held at that moment", carrying the same
//!   number back, which is what finishes the first machine's transfer.
//!
//! The number is why the two cannot drift: an answer that turns up late, after its byte has been
//! given up on, is dropped instead of being handed to the next byte. And when no answer comes at
//! all, the machine driving the clock gives up after [`ANSWER_FRAMES`] and reads all 1s, the way
//! a console whose cable has been pulled out does, rather than leaving the game waiting for ever.
//!
//! The app never has to understand any of this: it carries [`PACKET_LEN`]-byte blocks from one
//! phone to the other and back, in order, and the rules stay in here. See .kiro/notes/docs/CBOY_NIGHT.md for
//! the three calls the engine owes this module.

use std::collections::VecDeque;

use crate::bus::Bus;

/// A message on the cable, as the three bytes that go over: what kind it is, which byte of the
/// conversation it belongs to, and the byte itself.
pub const PACKET_LEN: usize = 3;
const TAG_CLOCK: u8 = b'C';
const TAG_ANSWER: u8 = b'A';

/// How many frames the machine driving the clock waits for an answer before deciding the cable
/// is gone. Six frames is about a tenth of a second: long enough for a phone-to-phone round trip
/// on a bad connection, short enough that a game never looks frozen.
pub const ANSWER_FRAMES: u32 = 6;

/// How many packets are held each way before the oldest are dropped. A linking game sends a
/// handful of bytes a second, so this is only ever reached when the other phone has stopped
/// listening, and then the bytes are worthless anyway; the cap is what stops them filling the
/// phone's memory.
const QUEUE_MAX: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Packet {
    /// This end drove the cable and shifted `byte` out.
    Clock { seq: u8, byte: u8 },
    /// What the other end's shift register held when it was clocked.
    Answer { seq: u8, byte: u8 },
}

impl Packet {
    pub fn encode(&self) -> [u8; PACKET_LEN] {
        match *self {
            Packet::Clock { seq, byte } => [TAG_CLOCK, seq, byte],
            Packet::Answer { seq, byte } => [TAG_ANSWER, seq, byte],
        }
    }

    /// Anything that is not one of the two kinds, or not the right length, is not ours: a
    /// frontend may hand over whatever it likes and it must not be able to upset a game.
    pub fn decode(bytes: &[u8]) -> Option<Packet> {
        let (tag, seq, byte) = match bytes {
            [tag, seq, byte] => (*tag, *seq, *byte),
            _ => return None,
        };
        match tag {
            TAG_CLOCK => Some(Packet::Clock { seq, byte }),
            TAG_ANSWER => Some(Packet::Answer { seq, byte }),
            _ => None,
        }
    }
}

/// One end of the cable: what is waiting to go out, what has come in, and which byte this end is
/// still expecting an answer to.
pub struct Cable {
    /// The app has a session with another phone running.
    pub connected: bool,
    /// Blocks for the app to carry to the other phone, oldest first.
    out: VecDeque<[u8; PACKET_LEN]>,
    /// Blocks the app has brought in, parked until the next frame. They arrive on the app's own
    /// thread, between frames, and the machine is only ever touched inside a frame.
    inbox: VecDeque<[u8; PACKET_LEN]>,
    next_seq: u8,
    /// The byte this end is waiting for an answer to, and how many frames it has waited.
    awaiting: Option<u8>,
    waited: u32,
}

impl Default for Cable {
    fn default() -> Cable {
        Cable::new()
    }
}

impl Cable {
    /// A cable with nothing on the other end. `const` so the core can hold one from the start.
    pub const fn new() -> Cable {
        Cable {
            connected: false,
            out: VecDeque::new(),
            inbox: VecDeque::new(),
            next_seq: 0,
            awaiting: None,
            waited: 0,
        }
    }

    /// The app has joined the two phones up. Everything from any earlier session is dropped.
    pub fn start(&mut self) {
        *self = Cable { connected: true, ..Cable::new() };
    }

    /// The session is over: back to a console with nothing attached. The machine itself is told
    /// on its next frame, because this comes in off the app's own thread.
    pub fn stop(&mut self) {
        *self = Cable::default();
    }

    /// Puts the machine on the cable, or takes it off, before a frame runs.
    pub fn attach(&self, bus: &mut Bus) {
        bus.link_connected = self.connected;
    }

    /// A block the app has carried over from the other phone.
    pub fn deliver(&mut self, bytes: &[u8]) {
        if let Some(p) = Packet::decode(bytes) {
            if self.inbox.len() < QUEUE_MAX {
                self.inbox.push_back(p.encode());
            }
        }
    }

    /// The next block for the app to carry to the other phone, if there is one.
    pub fn take_out(&mut self) -> Option<[u8; PACKET_LEN]> {
        self.out.pop_front()
    }

    fn send(&mut self, p: Packet) {
        if self.out.len() < QUEUE_MAX {
            self.out.push_back(p.encode());
        }
    }

    /// One frame's worth of cable: hands the machine everything that has arrived, picks up
    /// whatever it has put on the cable, and keeps count of how long an answer is taking.
    pub fn pump(&mut self, bus: &mut Bus) {
        self.attach(bus);
        if !self.connected {
            // Nothing attached, so a byte that was half way across when the other phone went
            // finishes instead of leaving the game waiting for ever.
            bus.link_give_up();
            bus.link_out.clear();
            return;
        }
        while let Some(raw) = self.inbox.pop_front() {
            match Packet::decode(&raw) {
                Some(Packet::Clock { seq, byte }) => {
                    // The other end drove the cable. This machine answers with whatever its own
                    // shift register held, even when it was not expecting anything, so the other
                    // end's transfer always finishes.
                    let ours = bus.link_clock(byte).unwrap_or(0xFF);
                    self.send(Packet::Answer { seq, byte: ours });
                }
                Some(Packet::Answer { seq, byte }) => {
                    if self.awaiting == Some(seq) {
                        self.awaiting = None;
                        self.waited = 0;
                        bus.link_answer(byte);
                    }
                }
                None => {}
            }
        }
        while let Some(byte) = bus.link_out.pop_front() {
            let seq = self.next_seq;
            self.next_seq = self.next_seq.wrapping_add(1);
            self.send(Packet::Clock { seq, byte });
            self.awaiting = Some(seq);
            self.waited = 0;
        }
        if self.awaiting.is_some() {
            self.waited += 1;
            if self.waited > ANSWER_FRAMES {
                self.awaiting = None;
                self.waited = 0;
                bus.link_give_up();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gb::GameBoy;

    fn machine() -> GameBoy {
        GameBoy::new(vec![0u8; 0x8000]).unwrap()
    }

    /// Carries everything waiting on each end over to the other, the way the app's own session
    /// does, and gives both machines their frame of cable.
    fn carry(a: (&mut Cable, &mut GameBoy), b: (&mut Cable, &mut GameBoy)) {
        let (ca, ga) = a;
        let (cb, gb) = b;
        while let Some(p) = ca.take_out() {
            cb.deliver(&p);
        }
        while let Some(p) = cb.take_out() {
            ca.deliver(&p);
        }
        ca.pump(&mut ga.bus);
        cb.pump(&mut gb.bus);
    }

    #[test]
    fn a_packet_survives_the_trip_and_rubbish_is_refused() {
        for p in [Packet::Clock { seq: 7, byte: 0x5A }, Packet::Answer { seq: 255, byte: 0 }] {
            assert_eq!(Packet::decode(&p.encode()), Some(p));
        }
        assert_eq!(Packet::decode(&[b'C', 0, 0, 0]), None, "too long");
        assert_eq!(Packet::decode(&[b'C', 0]), None, "too short");
        assert_eq!(Packet::decode(&[]), None);
        assert_eq!(Packet::decode(&[b'Z', 1, 2]), None, "not one of ours");
    }

    /// The whole point of the job: two machines, each on its own cable end, swapping a byte.
    /// One is set to its internal clock and so drives the cable; the other waits to be clocked.
    #[test]
    fn two_machines_swap_a_byte_down_the_cable() {
        let (mut ga, mut gb) = (machine(), machine());
        let (mut ca, mut cb) = (Cable::default(), Cable::default());
        ca.start();
        cb.start();
        ca.pump(&mut ga.bus);
        cb.pump(&mut gb.bus);
        // The one dragged along arms itself on the external clock: nothing happens until the
        // other end clocks it.
        gb.bus.write(0xFF01, 0x22);
        gb.bus.write(0xFF02, 0x80);
        for _ in 0..200 {
            gb.bus.tick();
        }
        assert_eq!(gb.bus.sb, 0x22, "no clock, so nothing has moved");
        assert!(gb.bus.link_busy());
        // The driving end starts its transfer.
        ga.bus.iflag = 0;
        gb.bus.iflag = 0;
        ga.bus.write(0xFF01, 0x11);
        ga.bus.write(0xFF02, 0x81);
        // Its eight bits are clocked out at the cable's own speed, and then it holds the
        // transfer open: the byte coming back has a phone-to-phone trip to make.
        for _ in 0..crate::bus::SERIAL_BYTE_CYCLES / 4 + 1 {
            ga.bus.tick();
        }
        assert!(ga.bus.link_busy(), "waiting for the other phone");
        assert_eq!(ga.bus.iflag & crate::bus::INT_SERIAL, 0, "and no interrupt yet");
        // Three frames: the byte goes out, the other end answers, the answer comes back.
        for _ in 0..3 {
            carry((&mut ca, &mut ga), (&mut cb, &mut gb));
        }
        assert_eq!(ga.bus.sb, 0x22, "the other machine's byte");
        assert_eq!(gb.bus.sb, 0x11, "and it got ours");
        assert!(!ga.bus.link_busy() && !gb.bus.link_busy(), "both transfers are over");
        for (name, gx) in [("driving", &ga), ("dragged", &gb)] {
            assert_eq!(gx.bus.iflag & crate::bus::INT_SERIAL, crate::bus::INT_SERIAL, "{name}");
            assert_eq!(gx.bus.raw_read(0xFF02) & 0x80, 0, "{name} end's start bit falls");
        }
    }

    /// The other phone drops off mid-game: the transfer finishes reading 1s instead of leaving
    /// the game waiting for ever, and only after a wait long enough to survive a bad connection.
    #[test]
    fn a_partner_that_stops_answering_is_given_up_on() {
        let (mut g, mut c) = (machine(), Cable::default());
        c.start();
        c.pump(&mut g.bus);
        g.bus.write(0xFF01, 0x3C);
        g.bus.write(0xFF02, 0x81);
        for _ in 0..crate::bus::SERIAL_BYTE_CYCLES / 4 + 1 {
            g.bus.tick();
        }
        for _ in 0..ANSWER_FRAMES {
            c.pump(&mut g.bus);
            assert!(g.bus.link_busy(), "still hoping");
        }
        c.pump(&mut g.bus);
        assert!(!g.bus.link_busy());
        assert_eq!(g.bus.sb, 0xFF, "what a pulled-out cable reads as");
        // And the byte it sent is still the one it sent, not the one it gave up on.
        assert_eq!(g.bus.link_sent(), 0x3C);
    }

    /// A clock arriving for a machine that is not linking at all: it answers so the other end
    /// does not hang, but its own byte is left alone rather than being overwritten.
    #[test]
    fn a_clock_nobody_asked_for_leaves_the_game_alone() {
        let (mut g, mut c) = (machine(), Cable::default());
        c.start();
        c.pump(&mut g.bus);
        g.bus.write(0xFF01, 0x77);
        g.bus.iflag = 0;
        c.deliver(&Packet::Clock { seq: 3, byte: 0x99 }.encode());
        c.pump(&mut g.bus);
        assert_eq!(g.bus.sb, 0x77, "the game's own byte");
        assert_eq!(g.bus.iflag & crate::bus::INT_SERIAL, 0, "and no interrupt it did not ask for");
        assert_eq!(c.take_out(), Some(Packet::Answer { seq: 3, byte: 0xFF }.encode()));
    }

    /// An answer that turns up after its byte has been given up on is dropped, so it can never
    /// be handed to the next byte and set the two phones telling each other different stories.
    #[test]
    fn a_late_answer_cannot_land_in_the_wrong_byte() {
        let (mut g, mut c) = (machine(), Cable::default());
        c.start();
        c.pump(&mut g.bus);
        g.bus.write(0xFF01, 0x01);
        g.bus.write(0xFF02, 0x81);
        for _ in 0..crate::bus::SERIAL_BYTE_CYCLES / 4 + 1 {
            g.bus.tick();
        }
        for _ in 0..=ANSWER_FRAMES {
            c.pump(&mut g.bus);
        }
        assert_eq!(c.take_out(), Some(Packet::Clock { seq: 0, byte: 0x01 }.encode()));
        assert_eq!(g.bus.sb, 0xFF, "given up on");
        // The second byte goes out with its own number on it.
        g.bus.write(0xFF01, 0x02);
        g.bus.write(0xFF02, 0x81);
        for _ in 0..crate::bus::SERIAL_BYTE_CYCLES / 4 + 1 {
            g.bus.tick();
        }
        c.pump(&mut g.bus);
        assert_eq!(c.take_out(), Some(Packet::Clock { seq: 1, byte: 0x02 }.encode()));
        // The first byte's answer finally arrives. It is not this byte's.
        c.deliver(&Packet::Answer { seq: 0, byte: 0xAB }.encode());
        c.pump(&mut g.bus);
        assert!(g.bus.link_busy(), "still waiting for the right answer");
        assert_ne!(g.bus.sb, 0xAB);
        c.deliver(&Packet::Answer { seq: 1, byte: 0xCD }.encode());
        c.pump(&mut g.bus);
        assert_eq!(g.bus.sb, 0xCD);
    }

    /// The session ending puts the machine back to a console with nothing attached, and does not
    /// leave a transfer hanging.
    #[test]
    fn the_session_ending_unplugs_the_cable() {
        let (mut g, mut c) = (machine(), Cable::default());
        c.start();
        c.pump(&mut g.bus);
        g.bus.write(0xFF02, 0x81);
        for _ in 0..crate::bus::SERIAL_BYTE_CYCLES / 4 + 1 {
            g.bus.tick();
        }
        assert!(g.bus.link_busy());
        c.stop();
        c.pump(&mut g.bus);
        assert!(!g.bus.link_busy() && !g.bus.link_connected);
        assert_eq!(c.take_out(), None);
        // And from here on a transfer finishes on its own, as an unplugged console's does.
        g.bus.write(0xFF02, 0x81);
        for _ in 0..crate::bus::SERIAL_BYTE_CYCLES / 4 + 1 {
            g.bus.tick();
        }
        assert!(!g.bus.link_busy());
    }
}
