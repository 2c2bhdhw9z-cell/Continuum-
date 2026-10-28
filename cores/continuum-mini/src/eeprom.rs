//! 24C64-compatible 8 KiB EEPROM pin protocol and the RTC bytes used by Pokémon Mini software.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Eeprom {
    pub data: Vec<u8>,
    address: u16,
    shift: u8,
    bits: u8,
    stage: u8,
    selected: bool,
    reading: bool,
    sda_out: bool,
    last_sda: bool,
    last_scl: bool,
}

impl Default for Eeprom {
    fn default() -> Self {
        Self {
            data: vec![0xff; 8192],
            address: 0,
            shift: 0,
            bits: 0,
            stage: 0,
            selected: false,
            reading: false,
            sda_out: true,
            last_sda: true,
            last_scl: false,
        }
    }
}

impl Eeprom {
    pub fn load_compatible(&mut self, bytes: &[u8]) -> bool {
        if bytes.len() != 8192 {
            return false;
        }
        self.data.copy_from_slice(bytes);
        true
    }

    /// Writes the console RTC footer in PokeMini's `.eep` layout.
    pub fn set_rtc_unix(&mut self, timestamp: u64) {
        let mut days = timestamp / 86_400;
        let mut year = 1970u16;
        loop {
            let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
            let span = if leap { 366 } else { 365 };
            if days < span {
                break;
            }
            days -= span;
            year += 1;
        }
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let months = [
            31u64,
            if leap { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ];
        let mut month = 0usize;
        while days >= months[month] {
            days -= months[month];
            month += 1;
        }
        let seconds = timestamp % 86_400;
        let fields = [
            (year % 100) as u8,
            (month + 1) as u8,
            (days + 1) as u8,
            (seconds / 3600) as u8,
            (seconds / 60 % 60) as u8,
            (seconds % 60) as u8,
        ];
        self.data[0x1ff6..0x1ff9].fill(0);
        self.data[0x1ff9..0x1fff].copy_from_slice(&fields);
        self.data[0x1fff] = fields
            .iter()
            .fold(0u8, |sum, value| sum.wrapping_add(*value));
    }

    pub fn pins(&mut self, sda: bool, scl: bool) {
        if self.last_sda && !sda && scl {
            self.selected = true;
            self.bits = 0;
            self.shift = 0;
            self.stage = 0;
            self.reading = false;
        } else if !self.last_sda && sda && scl {
            self.selected = false;
            self.sda_out = true;
        } else if self.selected && !self.last_scl && scl {
            if self.reading {
                self.sda_out =
                    self.data[self.address as usize & 0x1fff] & (0x80 >> self.bits.min(7)) != 0;
                self.bits += 1;
                if self.bits == 8 {
                    self.bits = 0;
                    self.address = self.address.wrapping_add(1) & 0x1fff;
                }
            } else {
                self.shift = (self.shift << 1) | sda as u8;
                self.bits += 1;
                if self.bits == 8 {
                    self.consume(self.shift);
                    self.bits = 0;
                    self.shift = 0;
                    self.sda_out = false;
                }
            }
        } else if self.selected && self.last_scl && !scl {
            self.sda_out = true;
        }
        self.last_sda = sda;
        self.last_scl = scl;
    }

    fn consume(&mut self, byte: u8) {
        match self.stage {
            0 => {
                if byte & 0xf0 != 0xa0 {
                    self.selected = false;
                    return;
                }
                self.reading = byte & 1 != 0;
                self.stage = if self.reading { 3 } else { 1 };
            }
            1 => {
                self.address = ((byte as u16) << 8) & 0x1fff;
                self.stage = 2;
            }
            2 => {
                self.address = (self.address & 0x1f00) | byte as u16;
                self.stage = 3;
            }
            _ => {
                self.data[self.address as usize & 0x1fff] = byte;
                self.address = self.address.wrapping_add(1) & 0x1fff;
            }
        }
    }

    pub fn sda(&self) -> bool {
        self.sda_out
    }
}
