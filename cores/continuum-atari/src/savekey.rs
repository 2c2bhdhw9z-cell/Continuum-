//! The SaveKey, and the AtariVox (the same memory with a speech chip, whose speech isn't played):
//! a 32 KB EEPROM, Microchip's 24LC256, that plugs into the second joystick port. Games keep high
//! scores and settings in it. It talks I²C on two of the port's lines, as its makers document it:
//! SDA on the stick's left line (SWCHA bit 2) and SCL on its right line (bit 3). The console's
//! RIOT drives a line low by making it an output with a 0 and lets it go by making it an input;
//! the EEPROM pulls SDA low to answer. Both sides only ever pull low, so the line is low if either
//! does.
//!
//! What it does, from the 24LC256 data sheet: a START (SDA falling while SCL is high), the device
//! byte $A0 (write) or $A1 (read), each byte acknowledged by the receiver pulling SDA low for a
//! ninth clock; for a write the two address bytes (15 bits) and up to 64 bytes for one page,
//! stored when the STOP comes (SDA rising while SCL is high); for a read, bytes from the address
//! on, until the console doesn't acknowledge one. Reads run on through the whole memory; a page
//! write wraps within its 64 bytes. A write takes effect at once (the chip's 5 ms of writing
//! aren't kept).

use serde::{Deserialize, Serialize};

pub const SIZE: usize = 0x8000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum State {
    /// Waiting for a START.
    Idle,
    Device,
    AddrHigh,
    AddrLow,
    Write,
    Read,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SaveKey {
    state: State,
    /// The bus as last seen.
    scl: bool,
    sda: bool,
    /// The bit within the byte (0-7), 8 for the acknowledge clock.
    bit: u8,
    shift: u8,
    /// The EEPROM's own hold on SDA: false while it pulls it low.
    out: bool,
    addr: u16,
    /// A read: the console acknowledged the last byte, so another follows.
    more: bool,
    /// A read whose first byte goes out after the device byte's acknowledge.
    first: bool,
    /// The page being written: (address, byte), stored at the STOP.
    page: Vec<(u16, u8)>,
}

impl Default for SaveKey {
    fn default() -> Self {
        SaveKey {
            state: State::Idle,
            scl: true,
            sda: true,
            bit: 0,
            shift: 0,
            out: true,
            addr: 0,
            more: false,
            first: false,
            page: Vec::new(),
        }
    }
}

impl SaveKey {
    /// The EEPROM's hold on SDA (true: let go).
    pub fn sda_out(&self) -> bool {
        self.out
    }

    /// The console's drive on the lines changed (true: let go, so the pull-up holds it high).
    pub fn lines(&mut self, scl_drive: bool, sda_drive: bool, mem: &mut [u8]) {
        let scl = scl_drive;
        let sda = sda_drive && self.out;
        let (was_scl, was_sda) = (self.scl, self.sda);
        self.scl = scl;
        self.sda = sda;
        if scl && was_scl && sda != was_sda {
            if !sda {
                // START (or a repeated one): a page not yet ended with a STOP is dropped.
                self.page.clear();
                self.state = State::Device;
                self.bit = 0;
                self.shift = 0;
                self.out = true;
            } else {
                // STOP.
                if self.state == State::Write {
                    for &(a, v) in &self.page {
                        mem[a as usize % mem.len()] = v;
                    }
                }
                self.page.clear();
                self.state = State::Idle;
                self.out = true;
            }
            return;
        }
        if scl && !was_scl {
            self.rising(sda);
        } else if !scl && was_scl {
            self.falling(mem);
            // The EEPROM's new hold shows at once.
            self.sda = sda_drive && self.out;
        }
    }

    /// SCL rising: the receiver takes the bit. `bit` counts the clocks of the byte so far (the
    /// ninth is the acknowledge).
    fn rising(&mut self, sda: bool) {
        match self.state {
            State::Idle => {}
            State::Read => {
                if self.bit == 8 {
                    self.more = !sda;
                }
                self.bit = (self.bit + 1).min(9);
            }
            _ => {
                if self.bit < 8 {
                    self.shift = (self.shift << 1) | sda as u8;
                }
                self.bit = (self.bit + 1).min(9);
            }
        }
    }

    /// SCL falling: the sender puts its next bit out.
    fn falling(&mut self, mem: &[u8]) {
        match self.state {
            State::Idle => {}
            State::Read => match self.bit {
                0..=7 => self.out = self.shift & (0x80 >> self.bit) != 0,
                // Let go for the console's acknowledge.
                8 => self.out = true,
                _ => {
                    if self.first {
                        self.first = false;
                        self.start_byte(mem);
                    } else if self.more {
                        self.addr = (self.addr + 1) & (SIZE as u16 - 1);
                        self.start_byte(mem);
                    } else {
                        self.state = State::Idle;
                        self.out = true;
                    }
                }
            },
            _ => match self.bit {
                // A whole byte in: acknowledge it if it's ours.
                8 => self.out = !self.take(self.shift),
                9 => {
                    self.bit = 0;
                    self.shift = 0;
                    self.out = true;
                }
                _ => {}
            },
        }
    }

    /// A read puts the byte at the address on the line, its top bit first.
    fn start_byte(&mut self, mem: &[u8]) {
        self.shift = mem[self.addr as usize % mem.len()];
        self.bit = 0;
        self.more = false;
        self.out = self.shift & 0x80 != 0;
    }

    /// A received byte; true if the EEPROM acknowledges it.
    fn take(&mut self, b: u8) -> bool {
        match self.state {
            State::Device => {
                if b & 0xFE != 0xA0 {
                    self.state = State::Idle;
                    return false;
                }
                if b & 1 != 0 {
                    self.state = State::Read;
                    self.first = true;
                } else {
                    self.state = State::AddrHigh;
                }
                true
            }
            State::AddrHigh => {
                self.addr = ((b as u16 & 0x7F) << 8) | (self.addr & 0xFF);
                self.state = State::AddrLow;
                true
            }
            State::AddrLow => {
                self.addr = (self.addr & 0x7F00) | b as u16;
                self.state = State::Write;
                true
            }
            State::Write => {
                if let Some(e) = self.page.iter_mut().find(|e| e.0 == self.addr) {
                    e.1 = b;
                } else {
                    self.page.push((self.addr, b));
                }
                self.addr = (self.addr & !0x3F) | ((self.addr + 1) & 0x3F);
                true
            }
            State::Read | State::Idle => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The console's side of I²C, as a 7800 program does it.
    struct Master<'a> {
        key: &'a mut SaveKey,
        mem: &'a mut Vec<u8>,
        scl: bool,
        sda: bool,
    }

    impl Master<'_> {
        fn set(&mut self, scl: bool, sda: bool) {
            self.scl = scl;
            self.sda = sda;
            self.key.lines(scl, sda, self.mem);
        }
        fn start(&mut self) {
            self.set(true, true);
            self.set(true, false);
            self.set(false, false);
        }
        fn stop(&mut self) {
            self.set(false, false);
            self.set(true, false);
            self.set(true, true);
        }
        /// Sends a byte; true if acknowledged.
        fn send(&mut self, b: u8) -> bool {
            for i in (0..8).rev() {
                let bit = b >> i & 1 != 0;
                self.set(false, bit);
                self.set(true, bit);
                self.set(false, bit);
            }
            self.set(false, true);
            self.set(true, true);
            let ack = !(self.sda && self.key.sda_out());
            self.set(false, true);
            ack
        }
        fn recv(&mut self, ack: bool) -> u8 {
            let mut b = 0;
            for _ in 0..8 {
                self.set(false, true);
                self.set(true, true);
                b = (b << 1) | self.key.sda_out() as u8;
            }
            self.set(false, true);
            self.set(false, !ack);
            self.set(true, !ack);
            self.set(false, !ack);
            self.set(false, true);
            b
        }
    }

    #[test]
    fn writes_a_page_and_reads_it_back() {
        let mut key = SaveKey::default();
        let mut mem = vec![0xFF; SIZE];
        let mut m = Master { key: &mut key, mem: &mut mem, scl: true, sda: true };
        m.start();
        assert!(m.send(0xA0) && m.send(0x30) && m.send(0x3E), "the device and the address are acknowledged");
        for b in [0x11, 0x22, 0x33, 0x44] {
            assert!(m.send(b));
        }
        m.stop();
        // A page write wraps within its 64 bytes: $303E, $303F, then $3000 and $3001.
        assert_eq!(&m.mem[0x303E..0x3040], &[0x11, 0x22]);
        assert_eq!(&m.mem[0x3000..0x3002], &[0x33, 0x44]);
        // A random read: the address written, then a repeated start for reading.
        m.start();
        assert!(m.send(0xA0) && m.send(0x30) && m.send(0x3F));
        m.start();
        assert!(m.send(0xA1));
        let got = [m.recv(true), m.recv(true), m.recv(false)];
        m.stop();
        assert_eq!(got, [0x22, 0xFF, 0xFF], "reads run on past the page");
    }

    #[test]
    fn ignores_other_devices_and_unfinished_writes() {
        let mut key = SaveKey::default();
        let mut mem = vec![0xFF; SIZE];
        let mut m = Master { key: &mut key, mem: &mut mem, scl: true, sda: true };
        m.start();
        assert!(!m.send(0xB0), "not its device code");
        m.stop();
        m.start();
        assert!(m.send(0xA0) && m.send(0x00) && m.send(0x10) && m.send(0x99));
        m.start(); // no STOP: the page is dropped
        m.stop();
        assert_eq!(m.mem[0x10], 0xFF);
    }
}
