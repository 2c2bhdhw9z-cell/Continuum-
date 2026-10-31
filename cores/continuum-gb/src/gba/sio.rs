//! The Advance's link port, and the cable that carries it to a second phone.
//!
//! Three of the port's modes go over the cable (GBATEK, "GBA Communication Ports"):
//! - **normal**, 8 or 32 bits: the console on its own clock drives the other, as on a Game Boy;
//! - **multi-player**, 16 bits: the parent (this phone when it hosted the session) starts every
//!   transfer, and each console ends up with both words, the parent's in SIOMULTI0 and the
//!   child's in SIOMULTI1 (SIOMULTI2 and 3, the players a two-phone cable has not got, read
//!   $FFFF).
//! UART mode and the general-purpose and JOY Bus modes are not carried; a transfer started in
//! them is left to the game, as before.
//!
//! The cable keeps the two ends in step the way the Game Boy one does (`crate::link`): the
//! driving console sends a **clock** with its word the moment its transfer starts, the other
//! console takes it and sends back an **answer** with its own word, and the driving console's
//! transfer stays open (busy) until that answer comes, however long the trip between the phones
//! takes, or gives up after [`ANSWER_FRAMES`] and reads all 1s, as with the cable pulled out. With
//! nothing plugged in at all, a transfer finishes after its own length reading all 1s, which is
//! what a game checking for a partner expects to see.

use std::collections::VecDeque;

use super::bus::Bus;

/// The Advance's serial interrupt (IF bit 7).
pub const IRQ_SERIAL: u16 = 0x80;

/// What travels: a tag, the byte's number, the mode it was sent in, the word (little-endian).
pub const PACKET_LEN: usize = 7;
const TAG_CLOCK: u8 = b'c';
const TAG_ANSWER: u8 = b'a';
pub const ANSWER_FRAMES: u32 = 6;
const QUEUE_MAX: usize = 256;

/// Port modes that go over the cable.
pub const MODE_NORMAL8: u8 = 0;
pub const MODE_NORMAL32: u8 = 1;
pub const MODE_MULTI: u8 = 2;

/// The port's side of a transfer. Not in a save state (a state loaded mid-transfer finishes that
/// transfer reading all 1s on its next line), so states from before it still load.
#[derive(Clone, Default)]
pub struct Sio {
    /// CPU cycles until the transfer in progress has all its bits out (0: none in progress).
    ///
    /// This is a cycle count rather than a scanline count because games use a timer to measure
    /// the serial port. The first bit edge is six cycles after SIOCNT's start write; later edges
    /// are 64 cycles apart at 256 KHz or 8 cycles apart at 2 MHz, as measured on the console.
    cycles: u32,
    /// One clock from the shifter finishing to IF bit 7 becoming visible.
    irq_cycles: u8,
    /// All bits are out and the transfer is held open for the partner's answer.
    waiting: bool,
    /// The answer, when it came before the bits were all out.
    early: Option<u32>,
    /// The app has a session with another phone going.
    pub connected: bool,
    /// This phone hosted that session, so it is the parent in multi-player mode.
    pub parent: bool,
    /// Words this console has put on the cable, as (mode, word), for the cable to carry over.
    pub out: VecDeque<(u8, u32)>,
}

impl Bus {
    /// The port's mode, when it is one the cable carries.
    fn sio_mode(&self) -> Option<u8> {
        if self.io16(0x134) & 0x8000 != 0 {
            return None;
        }
        match (self.io16(0x128) >> 12) & 3 {
            0 => Some(MODE_NORMAL8),
            1 => Some(MODE_NORMAL32),
            2 => Some(MODE_MULTI),
            _ => None,
        }
    }

    /// SIOCNT as a game reads it. In multi-player mode the console reports what is on the cable:
    /// SI low on the parent and high on the child, and SD high when both consoles are there.
    pub(super) fn siocnt_read(&self) -> u16 {
        let v = self.io16(0x128);
        if self.rfu.active && self.sio_mode() == Some(MODE_NORMAL32) {
            // The adapter's ready signal on SI, answering what the game puts on SO.
            let si = self.rfu.si(v & 0x08 != 0, v & 0x01 != 0);
            return (v & !0x04) | if si { 0x04 } else { 0 };
        }
        if self.sio_mode() != Some(MODE_MULTI) {
            return v;
        }
        let mut v = v & !0x004C;

        if self.sio.connected {
            v |= 0x0008;
            if !self.sio.parent {
                v |= 0x0004;
            }
        }
        v
    }

    /// A write to SIOCNT that sets the start bit.
    pub(super) fn sio_start(&mut self) {
        let Some(mode) = self.sio_mode() else { return };
        let cnt = self.io16(0x128);
        let drives = match mode {
            MODE_MULTI => self.sio.parent || !self.sio.connected,
            _ => cnt & 1 != 0,
        };
        if self.rfu.active && mode == MODE_NORMAL32 {
            // The Wireless Adapter: the game's word goes to it and its answer comes back once
            // the bits have gone; or, on the adapter's clock, the game waits for an event.
            if drives {
                let word = self.sio_word(mode);
                let parent = self.sio.parent;
                self.sio.waiting = false;
                self.sio.early = Some(self.rfu.exchange(word, parent));
                self.sio.cycles = self.sio_cycles(mode, cnt);
            } else {
                self.rfu_clock_in();
            }
            return;
        }
        if !drives {
            // On the other console's clock: armed, waiting to be clocked (normal mode). A child
            // in multi-player mode cannot start a transfer at all.
            if mode == MODE_MULTI {
                self.set_io16(0x128, cnt & !0x80);
            }
            return;
        }
        self.sio.waiting = false;
        self.sio.early = None;
        self.sio.cycles = self.sio_cycles(mode, cnt);
        if self.sio.connected && self.sio.out.len() < QUEUE_MAX {
            let word = self.sio_word(mode);
            self.sio.out.push_back((mode, word));
        }
    }

    /// How many CPU cycles a transfer takes. The serial divider's first edge comes six cycles
    /// after the write that starts it. Normal mode then shifts one bit every 64 cycles at
    /// 256 KHz or every 8 at 2 MHz. Multi-player mode uses the four divider periods measured
    /// by GBAHawk from a real console and sends 36 edges for a two-console exchange.
    fn sio_cycles(&self, mode: u8, cnt: u16) -> u32 {
        const FIRST_EDGE: u32 = 6;
        match mode {
            MODE_MULTI => FIRST_EDGE + 36 * [888, 233, 160, 87][usize::from(cnt & 3)],
            _ => {
                let bits = if mode == MODE_NORMAL32 { 32 } else { 8 };
                FIRST_EDGE + bits * if cnt & 2 != 0 { 8 } else { 64 }
            }
        }
    }

    /// The word this console sends in `mode`.
    fn sio_word(&self, mode: u8) -> u32 {
        match mode {
            MODE_NORMAL8 => u32::from(self.io[0x12A]),
            MODE_NORMAL32 => u32::from(self.io16(0x120)) | u32::from(self.io16(0x122)) << 16,
            _ => u32::from(self.io16(0x12A)),
        }
    }

    /// The adapter clocks an event into a game waiting on its clock, if it has one.
    fn rfu_clock_in(&mut self) {
        let cnt = self.io16(0x128);
        if cnt & 0x81 != 0x80 || self.sio_mode() != Some(MODE_NORMAL32) || !self.rfu.has_event() {
            return;
        }
        let Some(event) = self.rfu.clock_out() else { return };
        self.rfu.game_word = self.sio_word(MODE_NORMAL32);
        self.set_io16(0x120, event as u16);
        self.set_io16(0x122, (event >> 16) as u16);
        self.sio_done();
    }

    /// Limits one machine-clock step to the serial event so completion, the start bit and its
    /// interrupt all land on the measured cycle even when a memory access spans that cycle.
    pub(super) fn sio_slice(&self, cycles: u32) -> u32 {
        let mut slice = cycles;
        if self.sio.cycles > 0 {
            slice = slice.min(self.sio.cycles);
        }
        if self.sio.irq_cycles > 0 {
            slice = slice.min(u32::from(self.sio.irq_cycles));
        }
        slice
    }

    /// Advances the serial clock by CPU cycles. A connected transfer waits after its last bit
    /// for the other phone's answer; an unconnected one reads all 1s at that exact point.
    pub(super) fn sio_advance(&mut self, cycles: u32) {
        // The shifter lowers Start first; its interrupt flag crosses into the I/O block one
        // clock later. Process a delay that existed on entry only, so completing below cannot
        // also consume its newly-created clock in this call.
        if self.sio.irq_cycles > 0 {
            let left = u32::from(self.sio.irq_cycles).saturating_sub(cycles);
            self.sio.irq_cycles = left as u8;
            if self.sio.irq_cycles == 0 {
                self.request_irq(IRQ_SERIAL);
            }
        }
        if self.rfu.active {
            self.rfu_clock_in();
        }
        if self.sio.cycles == 0 {
            return;
        }
        if cycles < self.sio.cycles {
            self.sio.cycles -= cycles;
            return;
        }
        self.sio.cycles = 0;
        if let Some(word) = self.sio.early.take() {
            self.sio_finish(word);
        } else if self.sio.connected {
            self.sio.waiting = true;
        } else {
            self.sio_finish(0xFFFF_FFFF);
        }
    }

    /// The driving console's transfer is over: what came back goes where the mode puts it, the
    /// busy bit falls and the interrupt is asked for.
    fn sio_finish(&mut self, theirs: u32) {
        self.sio.cycles = 0;
        self.sio.waiting = false;
        self.sio.early = None;
        let Some(mode) = self.sio_mode() else { return };
        match mode {
            MODE_NORMAL8 => self.io[0x12A] = theirs as u8,
            MODE_NORMAL32 => {
                self.set_io16(0x120, theirs as u16);
                self.set_io16(0x122, (theirs >> 16) as u16);
            }
            _ => {
                let mine = self.io16(0x12A);
                self.sio_multi(mine, theirs as u16, 0);
            }
        }
        self.sio_done();
    }

    /// SIOMULTI0-3 after a multi-player word: the parent's, the child's, and two nobody sent.
    fn sio_multi(&mut self, parent: u16, child: u16, id: u16) {
        self.set_io16(0x120, parent);
        self.set_io16(0x122, child);
        self.set_io16(0x124, 0xFFFF);
        self.set_io16(0x126, 0xFFFF);
        let cnt = self.io16(0x128);
        self.set_io16(0x128, (cnt & !0x0030) | id << 4);
    }

    fn sio_done(&mut self) {
        let cnt = self.io16(0x128);
        self.set_io16(0x128, cnt & !0x80);
        if cnt & 0x4000 != 0 {
            self.sio.irq_cycles = 1;
        }
    }

    /// The partner drove the cable with `word` in `mode`. Returns this console's word when it was
    /// set to take one (armed on the other console's clock in normal mode, or any child in
    /// multi-player mode), and nothing otherwise, leaving the game's own registers alone.
    pub fn sio_clock(&mut self, mode: u8, word: u32) -> Option<u32> {
        if self.sio_mode() != Some(mode) {
            return None;
        }
        let cnt = self.io16(0x128);
        match mode {
            MODE_MULTI => {
                if self.sio.parent {
                    return None;
                }
                let mine = self.io16(0x12A);
                self.sio_multi(word as u16, mine, 1);
                self.sio_done();
                Some(u32::from(mine))
            }
            _ => {
                if cnt & 0x81 != 0x80 {
                    return None;
                }
                let mine = self.sio_word(mode);
                match mode {
                    MODE_NORMAL8 => self.io[0x12A] = word as u8,
                    _ => {
                        self.set_io16(0x120, word as u16);
                        self.set_io16(0x122, (word >> 16) as u16);
                    }
                }
                self.sio_done();
                Some(mine)
            }
        }
    }

    /// The partner's answer to a word this console clocked out. Ignored when no transfer of
    /// this console's is open, so a late answer cannot land in the next one.
    pub fn sio_answer(&mut self, word: u32) {
        if self.sio.waiting {
            self.sio_finish(word);
        } else if self.sio.cycles > 0 {
            self.sio.early = Some(word);
        }
    }

    /// No answer is coming: the transfer finishes reading all 1s.
    pub fn sio_give_up(&mut self) {
        if self.sio.waiting {
            self.sio_finish(0xFFFF_FFFF);
        }
    }

    /// Cycles until a transfer raises its interrupt, for HALT to stop on the event instead of
    /// jumping a whole display-time slice past it.
    pub(super) fn cycles_to_sio_irq(&self) -> Option<u32> {
        if self.io16(0x128) & 0x4000 == 0 {
            return None;
        }
        let clocks = self.sio.cycles + u32::from(self.sio.irq_cycles);
        (clocks > 0).then_some(clocks)
    }

    /// A transfer is open: shifting, or waiting for the partner's answer.
    pub fn sio_busy(&self) -> bool {
        self.sio.cycles > 0 || self.sio.waiting
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Packet {
    Clock { seq: u8, mode: u8, word: u32 },
    Answer { seq: u8, word: u32 },
}

impl Packet {
    pub fn encode(&self) -> [u8; PACKET_LEN] {
        let (tag, seq, mode, word) = match *self {
            Packet::Clock { seq, mode, word } => (TAG_CLOCK, seq, mode, word),
            Packet::Answer { seq, word } => (TAG_ANSWER, seq, 0, word),
        };
        let w = word.to_le_bytes();
        [tag, seq, mode, w[0], w[1], w[2], w[3]]
    }

    /// Anything that is not exactly one of the two kinds is not ours (a Game Boy cable's
    /// packets included).
    pub fn decode(bytes: &[u8]) -> Option<Packet> {
        let [tag, seq, mode, a, b, c, d] = *bytes else { return None };
        let word = u32::from_le_bytes([a, b, c, d]);
        match tag {
            TAG_CLOCK if mode <= MODE_MULTI => Some(Packet::Clock { seq, mode, word }),
            TAG_ANSWER => Some(Packet::Answer { seq, word }),
            _ => None,
        }
    }
}

/// This phone's end of the Advance cable.
pub struct Cable {
    pub connected: bool,
    /// This phone hosted the session (the parent in multi-player mode).
    pub parent: bool,
    out: VecDeque<Vec<u8>>,
    inbox: VecDeque<[u8; PACKET_LEN]>,
    /// The Wireless Adapter's radio messages each way (`rfu.rs`), which share the cable.
    radio_in: VecDeque<Vec<u8>>,
    next_seq: u8,
    awaiting: Option<u8>,
    waited: u32,
}

impl Default for Cable {
    fn default() -> Cable {
        Cable::new()
    }
}

impl Cable {
    pub const fn new() -> Cable {
        Cable { connected: false, parent: false, out: VecDeque::new(), inbox: VecDeque::new(), radio_in: VecDeque::new(), next_seq: 0, awaiting: None, waited: 0 }
    }

    pub fn start(&mut self, parent: bool) {
        *self = Cable { connected: true, parent, ..Cable::new() };
    }

    pub fn stop(&mut self) {
        *self = Cable::new();
    }

    pub fn attach(&self, bus: &mut Bus) {
        bus.sio.connected = self.connected;
        bus.sio.parent = self.parent;
    }

    pub fn deliver(&mut self, bytes: &[u8]) {
        if bytes.first() == Some(&super::rfu::RADIO_TAG) {
            if self.radio_in.len() < QUEUE_MAX {
                self.radio_in.push_back(bytes.to_vec());
            }
            return;
        }
        if let Some(p) = Packet::decode(bytes) {
            if self.inbox.len() < QUEUE_MAX {
                self.inbox.push_back(p.encode());
            }
        }
    }

    pub fn take_out(&mut self) -> Option<Vec<u8>> {
        self.out.pop_front()
    }

    fn send(&mut self, p: Packet) {
        if self.out.len() < QUEUE_MAX {
            self.out.push_back(p.encode().to_vec());
        }
    }

    /// One frame's worth of cable, as `crate::link::Cable::pump`.
    pub fn pump(&mut self, bus: &mut Bus) {
        self.attach(bus);
        if bus.rfu.active {
            bus.rfu.frame();
            while let Some(m) = self.radio_in.pop_front() {
                bus.rfu.hear(&m);
            }
            // With no second phone the radio has nobody to talk to, and its messages go nowhere.
            while let Some(m) = bus.rfu.radio_out.pop_front() {
                if self.connected && self.out.len() < QUEUE_MAX {
                    self.out.push_back(m);
                }
            }
        }
        if !self.connected {
            bus.sio_give_up();
            bus.sio.out.clear();
            return;
        }
        while let Some(raw) = self.inbox.pop_front() {
            match Packet::decode(&raw) {
                Some(Packet::Clock { seq, mode, word }) => {
                    let ours = bus.sio_clock(mode, word).unwrap_or(match mode {
                        MODE_NORMAL8 => 0xFF,
                        MODE_MULTI => 0xFFFF,
                        _ => 0xFFFF_FFFF,
                    });
                    self.send(Packet::Answer { seq, word: ours });
                }
                Some(Packet::Answer { seq, word }) => {
                    if self.awaiting == Some(seq) {
                        self.awaiting = None;
                        self.waited = 0;
                        bus.sio_answer(word);
                    }
                }
                None => {}
            }
        }
        while let Some((mode, word)) = bus.sio.out.pop_front() {
            let seq = self.next_seq;
            self.next_seq = self.next_seq.wrapping_add(1);
            self.send(Packet::Clock { seq, mode, word });
            self.awaiting = Some(seq);
            self.waited = 0;
        }
        if self.awaiting.is_some() {
            self.waited += 1;
            if self.waited > ANSWER_FRAMES {
                self.awaiting = None;
                self.waited = 0;
                bus.sio_give_up();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bus() -> Bus {
        Bus::new(vec![0; 0x200])
    }

    fn cycles(b: &mut Bus, n: u32) {
        b.sio_advance(n);
    }

    /// Carries everything both ways, as the app's session does, and gives both a frame of cable.
    fn carry(a: (&mut Cable, &mut Bus), b: (&mut Cable, &mut Bus)) {
        let (ca, ba) = a;
        let (cb, bb) = b;
        while let Some(p) = ca.take_out() {
            cb.deliver(&p);
        }
        while let Some(p) = cb.take_out() {
            ca.deliver(&p);
        }
        ca.pump(ba);
        cb.pump(bb);
    }

    fn pair() -> ((Cable, Bus), (Cable, Bus)) {
        let (mut ca, mut cb) = (Cable::new(), Cable::new());
        ca.start(true);
        cb.start(false);
        let (mut a, mut b) = (bus(), bus());
        ca.pump(&mut a);
        cb.pump(&mut b);
        ((ca, a), (cb, b))
    }

    #[test]
    fn normal_serial_edges_and_interrupt_land_on_the_measured_clocks() {
        for (control, transfer_cycles) in [(0x4081, 518), (0x4083, 70)] {
            let mut b = bus();
            b.io_write16(0x128, control);
            b.sio_advance(transfer_cycles - 1);
            assert_ne!(b.io16(0x128) & 0x80, 0, "still shifting one clock before the end");
            b.sio_advance(1);
            assert_eq!(b.io16(0x128) & 0x80, 0, "Start falls with the last bit");
            assert_eq!(b.io16(0x202) & IRQ_SERIAL, 0, "IF has its own crossing clock");
            b.sio_advance(1);
            assert_ne!(b.io16(0x202) & IRQ_SERIAL, 0, "IF rises one clock later");
        }
    }

    #[test]
    fn a_packet_survives_the_trip_and_rubbish_is_refused() {
        for p in [Packet::Clock { seq: 3, mode: MODE_MULTI, word: 0x1234 }, Packet::Answer { seq: 255, word: 0xDEAD_BEEF }] {
            assert_eq!(Packet::decode(&p.encode()), Some(p));
        }
        assert_eq!(Packet::decode(&[b'C', 0, 0x55]), None, "a Game Boy cable's packet");
        assert_eq!(Packet::decode(&[b'c', 0, 3, 0, 0, 0, 0]), None, "a mode the cable does not carry");
        assert_eq!(Packet::decode(&[]), None);
    }

    /// Multi-player: the parent starts, both end up with both words, and both get the interrupt.
    #[test]
    fn two_advances_swap_words_in_multi_player_mode() {
        let ((mut ca, mut a), (mut cb, mut b)) = pair();
        for x in [&mut a, &mut b] {
            x.io_write16(0x134, 0);
            x.io_write16(0x128, 0x6003); // multi-player, 115200 baud, interrupt on
            x.set_io16(0x202, 0);
        }
        assert_eq!(a.siocnt_read() & 0x0C, 0x08, "parent: SI low, SD high");
        assert_eq!(b.siocnt_read() & 0x0C, 0x0C, "child: SI high");
        a.io_write16(0x12A, 0xAAAA);
        b.io_write16(0x12A, 0xBBBB);
        a.io_write16(0x128, 0x6083);
        assert!(a.sio_busy());
        cycles(&mut a, 40_000);
        assert!(a.sio_busy(), "waiting for the other phone");
        for _ in 0..3 {
            carry((&mut ca, &mut a), (&mut cb, &mut b));
        }
        // Each receiver's serial interrupt flag crosses into the I/O block one clock after
        // the last bit, just as it does for a locally-clocked transfer.
        cycles(&mut a, 1);
        cycles(&mut b, 1);
        for (name, x, id) in [("parent", &a, 0), ("child", &b, 1)] {
            assert_eq!((x.io16(0x120), x.io16(0x122), x.io16(0x124), x.io16(0x126)), (0xAAAA, 0xBBBB, 0xFFFF, 0xFFFF), "{name}");
            assert_eq!(x.io16(0x128) & 0x80, 0, "{name}: no longer busy");
            assert_eq!((x.io16(0x128) >> 4) & 3, id, "{name}: its player number");
            assert_ne!(x.io16(0x202) & IRQ_SERIAL, 0, "{name}: interrupt");
        }
        assert!(!a.sio_busy());
    }

    /// Normal 32-bit mode: the console on its own clock drives the other, as on a Game Boy.
    #[test]
    fn normal_mode_drives_the_other_console() {
        let ((mut ca, mut a), (mut cb, mut b)) = pair();
        for x in [&mut a, &mut b] {
            x.io_write16(0x134, 0);
        }
        b.io_write16(0x120, 0x2222);
        b.io_write16(0x122, 0x2222);
        b.io_write16(0x128, 0x1080); // 32-bit, the other console's clock, start: armed
        a.io_write16(0x120, 0x1111);
        a.io_write16(0x122, 0x1111);
        a.io_write16(0x128, 0x1081); // 32-bit, own clock, start
        cycles(&mut a, 3_000);
        for _ in 0..3 {
            carry((&mut ca, &mut a), (&mut cb, &mut b));
        }
        assert_eq!((a.io16(0x120), a.io16(0x122)), (0x2222, 0x2222));
        assert_eq!((b.io16(0x120), b.io16(0x122)), (0x1111, 0x1111));
        assert_eq!((a.io16(0x128) & 0x80, b.io16(0x128) & 0x80), (0, 0));
    }

    /// With nothing plugged in, a transfer finishes on its own reading all 1s, instead of
    /// staying busy for ever.
    #[test]
    fn a_lone_console_reads_all_ones() {
        let mut a = bus();
        a.io_write16(0x134, 0);
        a.io_write16(0x12A, 0x1234);
        a.io_write16(0x128, 0x2083);
        assert_eq!(a.siocnt_read() & 0x0C, 0, "no partner: SD low");
        cycles(&mut a, 100_000);
        assert_eq!(a.io16(0x128) & 0x80, 0);
        assert_eq!((a.io16(0x120), a.io16(0x122)), (0x1234, 0xFFFF));
    }

    #[test]
    fn a_partner_that_stops_answering_is_given_up_on() {
        let ((mut ca, mut a), _) = pair();
        a.io_write16(0x134, 0);
        a.io_write16(0x128, 0x2083);
        cycles(&mut a, 100_000);
        for _ in 0..ANSWER_FRAMES {
            ca.pump(&mut a);
            assert!(a.sio_busy());
        }
        ca.pump(&mut a);
        assert!(!a.sio_busy());
        assert_eq!(a.io16(0x122), 0xFFFF);
    }
}
