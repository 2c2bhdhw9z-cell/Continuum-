//! The console's small hardware: the buttons ($2052, each press raising its interrupt), the
//! shock sensor, the battery sensor ($2010), a few registers whose use is unknown, and the I/O
//! port ($2060 direction, $2061 data) with its lines: the infrared receiver and transmitter, the
//! save chip's data and clock (a 24C64-style 8 KiB EEPROM on two wires), the rumble motor and
//! the infrared enable.
//!
//! As PokeMini models them (the emulator cMini replaces), including the save chip's protocol:
//! the start and stop conditions, the device byte, the two address bytes, bytes written or read
//! with the address moving on, and the acknowledge bits. The real clock's time and date live at
//! the top of the save chip, where the console's start-up program keeps them.

use crate::irq::Irq;
use crate::link::Infrared;
use serde::{Deserialize, Serialize};

pub const EEPROM_SIZE: usize = 8192;

/// The I/O port's lines.
const IR_RX: u8 = 0x01;
const IR_TX: u8 = 0x02;
const SDA: u8 = 0x04;
const SCL: u8 = 0x08;
const RUMBLE: u8 = 0x10;
const IR_OFF: u8 = 0x20;
/// The line that reads high when nothing drives it.
const PULL_UP: u8 = 0x40;

/// The save chip's protocol, after its device byte.
const DEVICE: u8 = 0;
const ADDRESS_HIGH: u8 = 1;
const ADDRESS_LOW: u8 = 2;
const WRITING: u8 = 3;
const READING: u8 = 4;

/// The buttons, by their bit in $2052, with each one's interrupt.
pub const A: u8 = 0x01;
pub const B: u8 = 0x02;
pub const C: u8 = 0x04;
pub const UP: u8 = 0x08;
pub const DOWN: u8 = 0x10;
pub const LEFT: u8 = 0x20;
pub const RIGHT: u8 = 0x40;
pub const POWER: u8 = 0x80;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ports {
    pub eeprom: Vec<u8>,
    /// The lines as the save chip last saw them.
    last_lines: u8,
    listening: bool,
    stage: u8,
    shifted: u8,
    bit: i32,
    address: u16,
    /// Something was written to the save chip.
    pub eeprom_written: bool,
    /// The motor's line now, and whether it ran at all since last asked.
    pub rumbling: bool,
    pub rumbled: bool,
    shock: bool,
    pub infrared: Infrared,
}

impl Default for Ports {
    fn default() -> Self {
        Self {
            eeprom: vec![0xFF; EEPROM_SIZE],
            last_lines: 0,
            listening: false,
            stage: DEVICE,
            shifted: 0,
            bit: 0,
            address: 0,
            eeprom_written: false,
            rumbling: false,
            rumbled: false,
            shock: false,
            infrared: Infrared::default(),
        }
    }
}

impl Ports {
    /// The reset line: the save chip's protocol and the motor idle, no button held, and the
    /// battery reading full.
    pub fn reset(&mut self, io: &mut [u8]) {
        self.last_lines = 0;
        self.listening = false;
        self.stage = DEVICE;
        self.shifted = 0;
        self.bit = 0;
        self.address = 0;
        self.rumbling = false;
        self.rumbled = false;
        self.shock = false;
        io[0x52] = 0xFF;
        io[0x10] &= 0x1F;
    }

    /// The clock's time and date at the top of the save chip, as the start-up program keeps
    /// them (year within the century, month, day, hour, minute, second, then their sum), and the
    /// console told it has a valid time.
    pub fn set_clock(&mut self, io: &mut [u8], fields: [u8; 6]) {
        io[0x02] |= 0x02;
        self.eeprom[0x1FF6..0x1FF9].fill(0);
        self.eeprom[0x1FF9..0x1FFF].copy_from_slice(&fields);
        self.eeprom[0x1FFF] = fields.iter().fold(0u8, |sum, v| sum.wrapping_add(*v));
    }

    /// A button pressed or let go (`key` its bit in $2052). A press raises its interrupt.
    pub fn button(&mut self, io: &mut [u8], irq: &mut Irq, key: u8, pressed: bool) {
        if pressed {
            if io[0x52] & key == 0 {
                return;
            }
            irq.raise(io, 0x15 + (7 - key.trailing_zeros() as u8));
            io[0x52] &= !key;
        } else {
            io[0x52] |= key;
        }
    }

    /// The shock sensor: a shake raises its interrupt as it starts.
    pub fn shake(&mut self, io: &mut [u8], irq: &mut Irq, shaking: bool) {
        if shaking && !self.shock {
            irq.raise(io, 0x10);
        }
        self.shock = shaking;
    }

    pub fn read(&mut self, io: &[u8], reg: usize) -> u8 {
        match reg {
            0x61 => {
                let mut lines = if self.eeprom_line() { SDA } else { 0 };
                if self.infrared.receiving() {
                    lines |= IR_RX;
                }
                (io[0x61] & io[0x60]) | (lines & !io[0x60])
            }
            0x10 | 0x44..=0x47 | 0x50..=0x55 | 0x60 | 0x62 => io[reg],
            _ => 0,
        }
    }

    pub fn write(&mut self, io: &mut [u8], reg: usize, value: u8) {
        match reg {
            0x10 => io[reg] = (io[reg] & 0x20) | (value & 0x1F),
            0x44 => io[reg] = value & 0xF7,
            0x45 | 0x47 => io[reg] = value & 0x0F,
            0x46 | 0x50 => io[reg] = value,
            0x51 => io[reg] = value & 0x03,
            // PokeMini lets these three run on into the direction register's write (a missing
            // `return`); each is its own register here.
            0x53 => io[reg] = 0,
            0x54 => io[reg] = value & 0x77,
            0x55 => io[reg] = value & 0x07,
            0x60 | 0x61 => {
                io[reg] = value;
                self.drive(io);
            }
            0x62 => io[reg] = value & 0xF0,
            _ => {}
        }
    }

    /// The port's lines as driven: outputs from the data register, inputs pulled up or low.
    fn drive(&mut self, io: &[u8]) {
        let lines = (PULL_UP & !io[0x60]) | (io[0x61] & io[0x60]);
        self.rumbling = lines & RUMBLE != 0;
        self.rumbled |= self.rumbling;
        let ir_on = io[0x60] & IR_OFF != 0 && lines & IR_OFF == 0;
        self.infrared.transmit(ir_on, lines & IR_TX != 0);
        self.eeprom_lines(lines);
    }

    /// The save chip watches its two lines.
    fn eeprom_lines(&mut self, lines: u8) {
        let rose = lines & !self.last_lines;
        let fell = !lines & self.last_lines;
        self.last_lines = lines;
        if fell & SDA != 0 && lines & SCL != 0 {
            self.listening = true;
            self.stage = DEVICE;
            self.bit = 8;
            self.shifted = 0;
            return;
        }
        if rose & SDA != 0 && lines & SCL != 0 {
            self.listening = false;
            return;
        }
        if !self.listening {
            return;
        }
        if rose & SCL != 0 {
            if self.bit < 0 {
                self.eeprom_byte(self.shifted);
                self.bit = 8;
                self.shifted = 0;
            } else {
                self.shifted = (self.shifted << 1) | u8::from(lines & SDA != 0);
            }
        } else if fell & SCL != 0 {
            self.bit -= 1;
        }
    }

    /// What the save chip puts on the data line: a bit of the byte read, an acknowledge (low),
    /// or nothing (high).
    fn eeprom_line(&self) -> bool {
        let in_byte = (0..8).contains(&self.bit);
        if !self.listening {
            return true;
        }
        if self.stage == READING {
            return in_byte && (self.eeprom[usize::from(self.address) & 0x1FFF] >> self.bit) & 1 != 0;
        }
        in_byte
    }

    fn eeprom_byte(&mut self, byte: u8) {
        match self.stage {
            DEVICE => {
                if byte & 0xF0 == 0xA0 {
                    self.stage = if byte & 0x01 != 0 { READING } else { ADDRESS_HIGH };
                }
            }
            ADDRESS_HIGH => {
                self.address = (self.address & 0x00FF) | u16::from(byte) << 8;
                self.stage = ADDRESS_LOW;
            }
            ADDRESS_LOW => {
                self.address = (self.address & 0xFF00) | u16::from(byte);
                self.stage = WRITING;
            }
            WRITING => {
                self.eeprom_written = true;
                self.eeprom[usize::from(self.address) & 0x1FFF] = byte;
                self.address = self.address.wrapping_add(1);
            }
            _ => self.address = self.address.wrapping_add(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives the port's data register as a program would (both lines outputs).
    fn lines(p: &mut Ports, io: &mut [u8], sda: bool, scl: bool) {
        io[0x60] = SDA | SCL;
        p.write(io, 0x61, if sda { SDA } else { 0 } | if scl { SCL } else { 0 });
    }

    fn send(p: &mut Ports, io: &mut [u8], byte: u8) {
        for k in (0..8).rev() {
            let b = byte >> k & 1 != 0;
            lines(p, io, b, false);
            lines(p, io, b, true);
            lines(p, io, b, false);
        }
        // The acknowledge clock.
        lines(p, io, false, true);
        lines(p, io, false, false);
    }

    #[test]
    fn a_byte_written_through_the_two_wires_lands_at_its_address() {
        let mut io = [0u8; 256];
        let mut p = Ports::default();
        lines(&mut p, &mut io, true, true);
        lines(&mut p, &mut io, false, true); // start
        lines(&mut p, &mut io, false, false);
        for b in [0xA0, 0x12, 0x34, 0x5A] {
            send(&mut p, &mut io, b);
        }
        lines(&mut p, &mut io, false, true);
        lines(&mut p, &mut io, true, true); // stop
        assert_eq!(p.eeprom[0x1234], 0x5A);
        assert!(p.eeprom_written);
    }

    #[test]
    fn a_press_raises_its_interrupt_once() {
        let mut io = [0u8; 256];
        let mut irq = Irq::default();
        let mut p = Ports::default();
        p.reset(&mut io);
        io[0x21] = 0x0C;
        io[0x25] = 0xFF;
        p.button(&mut io, &mut irq, A, true);
        assert_eq!((io[0x52], irq.accepted), (0xFE, Some(0x1C * 2)));
        // Taken and cleared; held down, the button raises nothing more.
        irq.accepted = None;
        io[0x29] = 0;
        p.button(&mut io, &mut irq, A, true);
        assert_eq!((io[0x29], irq.accepted), (0, None));
        // Let go and pressed again: a new interrupt.
        p.button(&mut io, &mut irq, A, false);
        p.button(&mut io, &mut irq, A, true);
        assert_eq!(irq.accepted, Some(0x1C * 2));
    }
}
