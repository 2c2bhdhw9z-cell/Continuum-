//! The Game Gear's link port (EXT) and the Gear-to-Gear cable, and two Game Gears joined by it on
//! one phone.
//!
//! From Charles MacDonald's notes on the Game Gear's I/O registers (SMS Power forums, "GG I/O
//! information") and SMS Power's "Gear to Gear Cable" page: port $01 the seven pins' levels,
//! $02 which are inputs (and bit 7: an NMI when TH falls), $03 a byte to send, $04 the byte
//! received, $05 the serial unit's control (speed, on, NMI on receipt) and status; and how the
//! cable crosses the pins (bit 0 arrives as 2, 1 as 3, 2 as 0, 3 as 1, 4 as 5, 5 as 4, 6 as 6).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ext {
    /// Port $01 as written (bit 7 keeps whatever is written; it is no pin).
    pub data: u8,
    /// Port $02: a 1 is an input.
    pub dir: u8,
    pub tx: u8,
    pub rx: u8,
    /// Port $05's writable bits (3-7).
    pub ctrl: u8,
    pub rx_full: bool,
    /// When the byte being sent has gone out (CPU time), and the byte, until the other end has it.
    pub tx_done: u64,
    pub sending: Option<u8>,
    /// What the other console's pins put on this one's, after the cable (1 where nothing drives).
    pub peer: u8,
    /// A cable is plugged in, with a console at the other end.
    #[serde(skip)]
    pub connected: bool,
    /// An NMI the link asked for, taken by the machine after the instruction.
    pub nmi: bool,
    /// The other end is another phone (the app's link, `remote_line`), and what is waiting to
    /// go to it: [b'P', pins] when this console's pins change, [b'S', byte] for a byte sent.
    #[serde(skip)]
    pub remote: bool,
    #[serde(skip)]
    pub outbox: Vec<[u8; 2]>,
    #[serde(skip)]
    sent_pins: Option<u8>,
}

impl Default for Ext {
    fn default() -> Ext {
        Ext { data: 0x7F, dir: 0xFF, tx: 0, rx: 0xFF, ctrl: 0, rx_full: false, tx_done: 0, sending: None, peer: 0x7F, connected: false, nmi: false, remote: false, outbox: Vec::new(), sent_pins: None }
    }
}

/// CPU cycles a byte takes at the speed in port $05's top bits (8 bits, a start and a stop bit).
fn byte_cycles(ctrl: u8) -> u64 {
    let bps = [4800u64, 2400, 1200, 300][usize::from(ctrl >> 6)];
    3_579_545 * 10 / bps
}

/// Through the cable: which pin each one arrives on.
fn cross(out: u8) -> u8 {
    const TO: [u8; 7] = [2, 3, 0, 1, 5, 4, 6];
    (0..7).fold(0, |v, i| if out & (1 << i) != 0 { v | 1 << TO[i] } else { v })
}

impl Ext {
    fn serial_on(&self) -> bool {
        self.ctrl & 0x30 == 0x30
    }

    /// The seven pins as this console drives them: its level where a pin is an output, high
    /// (not driven) where it is an input.
    fn driven(&self) -> u8 {
        (self.data & !self.dir | self.dir) & 0x7F
    }

    pub fn read(&mut self, port: u8, now: u64) -> u8 {
        match port {
            1 => {
                let outside = if self.connected { self.peer } else { 0x7F };
                (self.data & 0x80) | (self.data & !self.dir & 0x7F) | (outside & self.dir & 0x7F)
            }
            2 => self.dir,
            3 => self.tx,
            4 => {
                if !self.connected {
                    return 0xFF;
                }
                self.rx_full = false;
                self.rx
            }
            5 => {
                let busy = self.sending.is_some() && now < self.tx_done;
                let gone = self.serial_on() && !self.connected;
                (self.ctrl & 0xF8) | u8::from(gone) << 2 | u8::from(self.rx_full) << 1 | u8::from(busy)
            }
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, port: u8, v: u8, now: u64) {
        match port {
            1 => self.data = v,
            2 => self.dir = v,
            3 => {
                self.tx = v;
                if self.serial_on() {
                    self.sending = Some(v);
                    self.tx_done = now + byte_cycles(self.ctrl);
                }
            }
            5 => self.ctrl = v & 0xF8,
            _ => {}
        }
    }
}

/// The cable between two consoles: each one's pins onto the other's, a byte that has finished
/// going out into the other's receiver, and the NMIs those raise. `now_a` and `now_b` are their
/// CPU times; called between their lines, so it is at most a line late.
pub fn exchange(a: &mut Ext, now_a: u64, b: &mut Ext, now_b: u64) {
    a.connected = true;
    b.connected = true;
    let (to_b, to_a) = (cross(a.driven()), cross(b.driven()));
    pins(b, to_b);
    pins(a, to_a);
    if let Some(byte) = a.sending.filter(|_| now_a >= a.tx_done) {
        a.sending = None;
        receive(b, byte);
    }
    if let Some(byte) = b.sending.filter(|_| now_b >= b.tx_done) {
        b.sending = None;
        receive(a, byte);
    }
}

fn pins(to: &mut Ext, level: u8) {
    let fell = to.peer & 0x40 != 0 && level & 0x40 == 0;
    // An NMI on TH falling, when bit 7 of port $02 is 0 and TH is an input.
    if fell && to.dir & 0x80 == 0 && to.dir & 0x40 != 0 {
        to.nmi = true;
    }
    to.peer = level;
}

fn receive(to: &mut Ext, byte: u8) {
    if !to.serial_on() {
        return;
    }
    to.rx = byte;
    to.rx_full = true;
    if to.ctrl & 0x08 != 0 {
        to.nmi = true;
    }
}

/// Between lines, with another phone on the cable: this console's pins when they changed, and a
/// byte that has finished going out, are put in the outbox for the app to carry over.
pub fn remote_line(e: &mut Ext, now: u64) {
    e.connected = true;
    let d = e.driven();
    if e.sent_pins != Some(d) {
        e.sent_pins = Some(d);
        e.outbox.push([b'P', d]);
    }
    if let Some(byte) = e.sending.filter(|_| now >= e.tx_done) {
        e.sending = None;
        e.outbox.push([b'S', byte]);
    }
}

/// What came from the other phone: its pins (crossed by the cable) or a byte it sent.
pub fn remote_in(e: &mut Ext, packet: &[u8]) {
    e.connected = true;
    match packet {
        [b'P', pins_out] => pins(e, cross(*pins_out)),
        [b'S', byte] => receive(e, *byte),
        _ => {}
    }
}

/// The other phone has gone: nothing on the cable now.
pub fn unplug(e: &mut Ext) {
    e.remote = false;
    e.connected = false;
    e.peer = 0x7F;
    e.sent_pins = None;
    e.outbox.clear();
}

/// With no cable, a byte sent just goes.
pub fn alone(e: &mut Ext, now: u64) {
    e.connected = false;
    if e.sending.is_some() && now >= e.tx_done {
        e.sending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pins_cross_on_the_way_and_a_falling_th_is_an_nmi() {
        let (mut a, mut b) = (Ext::default(), Ext::default());
        a.dir = 0x70; // D0-D3 outputs
        a.data = 0x05; // D0 and D2 high, D1 and D3 low
        b.dir = 0x7F & !0x80; // all inputs, NMI on TH
        exchange(&mut a, 0, &mut b, 0);
        // D0 high arrives as D2, D2 high as D0; D1 and D3 low arrive as D3 and D1.
        assert_eq!(b.read(1, 0) & 0x0F, 0x05);
        assert!(!b.nmi);
        a.dir = 0x30; // TH an output too
        a.data = 0x00;
        exchange(&mut a, 0, &mut b, 0);
        assert!(b.nmi, "TH fell");
    }

    #[test]
    fn a_byte_goes_across_after_its_time_with_an_nmi_and_the_flags() {
        let (mut a, mut b) = (Ext::default(), Ext::default());
        a.write(5, 0x30, 0); // on, 4800 bps
        b.write(5, 0x38, 0); // on, NMI on receipt
        a.write(3, 0x5A, 100);
        assert_eq!(a.read(5, 101) & 1, 1, "sending");
        exchange(&mut a, 101, &mut b, 101);
        assert!(!b.rx_full, "not yet");
        exchange(&mut a, 100 + byte_cycles(0x30), &mut b, 0);
        assert_eq!(a.read(5, 100 + byte_cycles(0x30)) & 1, 0);
        assert!(b.rx_full && b.nmi);
        assert_eq!(b.read(5, 0) & 2, 2);
        assert_eq!(b.read(4, 0), 0x5A);
        assert_eq!(b.read(5, 0) & 2, 0, "read: empty again");
    }

    #[test]
    fn over_two_phones_the_pins_and_a_byte_go_as_packets() {
        let (mut a, mut b) = (Ext::default(), Ext::default());
        a.remote = true;
        b.remote = true;
        a.write(5, 0x30, 0);
        b.write(5, 0x38, 0);
        a.dir = 0x7E; // D0 an output
        a.data = 0x00; // driven low
        a.write(3, 0x77, 0);
        remote_line(&mut a, byte_cycles(0x30));
        assert_eq!(a.outbox.len(), 2, "its pins, then the byte");
        for p in std::mem::take(&mut a.outbox) {
            remote_in(&mut b, &p);
        }
        assert_eq!(b.read(1, 0) & 0x04, 0, "D0 low arrives on D2");
        assert!(b.rx_full && b.nmi);
        assert_eq!(b.read(4, 0), 0x77);
        remote_line(&mut a, byte_cycles(0x30) + 1);
        assert!(a.outbox.is_empty(), "nothing changed: nothing sent");
    }

    #[test]
    fn with_nothing_plugged_in_the_pins_read_high_and_the_status_says_so() {
        let mut a = Ext::default();
        a.write(5, 0x30, 0);
        alone(&mut a, 0);
        assert_eq!(a.read(1, 0) & 0x7F, 0x7F);
        assert_eq!(a.read(4, 0), 0xFF);
        assert_eq!(a.read(5, 0) & 4, 4);
    }
}
