//! The cartridge: ROM, external RAM and the memory bank controller (none, MBC1, MBC3 with its
//! real-time clock, MBC5). The ROM itself is never part of a save state.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MbcKind {
    None,
    Mbc1,
    Mbc3,
    Mbc5,
}

/// MBC3's clock. `s m h dl dh` are the live registers; `latched` is what reads return.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Rtc {
    pub regs: [u8; 5],
    pub latched: [u8; 5],
    /// T-cycles toward the next second.
    pub sub: u32,
    pub latch_prev: u8,
}

impl Rtc {
    fn halted(&self) -> bool {
        self.regs[4] & 0x40 != 0
    }

    /// One second passes.
    pub fn tick_second(&mut self) {
        if self.halted() {
            return;
        }
        let r = &mut self.regs;
        r[0] = (r[0] + 1) & 0x3F;
        if r[0] != 60 {
            return;
        }
        r[0] = 0;
        r[1] = (r[1] + 1) & 0x3F;
        if r[1] != 60 {
            return;
        }
        r[1] = 0;
        r[2] = (r[2] + 1) & 0x1F;
        if r[2] != 24 {
            return;
        }
        r[2] = 0;
        let day = (u16::from(r[4] & 1) << 8 | u16::from(r[3])) + 1;
        r[3] = day as u8;
        r[4] = (r[4] & 0xFE) | ((day >> 8) as u8 & 1);
        if day > 0x1FF {
            r[3] = 0;
            r[4] = (r[4] & 0xFE) | 0x80; // day counter carry
        }
    }

    pub fn advance_seconds(&mut self, secs: u64) {
        // Capped: a clock years behind is caught up in at most ten days' worth of steps.
        for _ in 0..secs.min(864_000) {
            self.tick_second();
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cart {
    #[serde(skip)]
    pub rom: Vec<u8>,
    pub ram: Vec<u8>,
    pub kind: MbcKind,
    pub has_battery: bool,
    pub has_rtc: bool,
    ram_enabled: bool,
    rom_bank: u16,
    ram_bank: u8,
    /// MBC1 bank2 register (2 bits) and mode.
    bank2: u8,
    mode: u8,
    pub rtc: Rtc,
    pub title: String,
    pub cgb_flag: u8,
}

pub fn ram_size(code: u8) -> usize {
    match code {
        1 => 0x800,
        2 => 0x2000,
        3 => 0x8000,
        4 => 0x20000,
        5 => 0x10000,
        _ => 0,
    }
}

impl Cart {
    pub fn new(rom: Vec<u8>) -> Result<Cart, String> {
        if rom.len() < 0x150 {
            return Err("the file is too small to be a Game Boy ROM".into());
        }
        let t = rom[0x147];
        let (kind, battery, rtc) = match t {
            0x00 | 0x08 | 0x09 => (MbcKind::None, t == 0x09, false),
            0x01..=0x03 => (MbcKind::Mbc1, t == 0x03, false),
            0x0F | 0x10 => (MbcKind::Mbc3, true, true),
            0x11..=0x13 => (MbcKind::Mbc3, t == 0x13, false),
            0x19..=0x1E => (MbcKind::Mbc5, t == 0x1B || t == 0x1E, false),
            other => return Err(format!("cartridge type ${other:02X} is not supported yet")),
        };
        let mut ram_len = ram_size(rom[0x149]);
        if kind == MbcKind::None && ram_len == 0 && (t == 0x08 || t == 0x09) {
            ram_len = 0x2000;
        }
        let title: String = rom[0x134..0x143]
            .iter()
            .take_while(|b| **b != 0)
            .map(|b| *b as char)
            .collect();
        // ROM padded to a power of two of at least 32 KiB, so bank masking is a single AND.
        let mut size = 0x8000;
        while size < rom.len() {
            size *= 2;
        }
        let mut rom = rom;
        rom.resize(size, 0xFF);
        Ok(Cart {
            cgb_flag: rom[0x143],
            rom,
            ram: vec![0xFF; ram_len],
            kind,
            has_battery: battery,
            has_rtc: rtc,
            ram_enabled: false,
            rom_bank: 1,
            ram_bank: 0,
            bank2: 0,
            mode: 0,
            rtc: Rtc::default(),
            title,
        })
    }

    fn rom_banks(&self) -> usize {
        self.rom.len() / 0x4000
    }

    fn rom_at(&self, bank: usize, addr: u16) -> u8 {
        let bank = bank % self.rom_banks();
        self.rom[bank * 0x4000 + (addr as usize & 0x3FFF)]
    }

    pub fn read_rom(&self, addr: u16) -> u8 {
        match self.kind {
            MbcKind::None => self.rom[addr as usize],
            MbcKind::Mbc1 => {
                if addr < 0x4000 {
                    let bank = if self.mode == 1 { (self.bank2 as usize) << 5 } else { 0 };
                    self.rom_at(bank, addr)
                } else {
                    let low = (self.rom_bank & 0x1F) as usize;
                    let low = if low == 0 { 1 } else { low };
                    self.rom_at((self.bank2 as usize) << 5 | low, addr)
                }
            }
            MbcKind::Mbc3 | MbcKind::Mbc5 => {
                if addr < 0x4000 {
                    self.rom[addr as usize]
                } else {
                    self.rom_at(self.rom_bank as usize, addr)
                }
            }
        }
    }

    pub fn write_rom(&mut self, addr: u16, v: u8) {
        match self.kind {
            MbcKind::None => {}
            MbcKind::Mbc1 => match addr {
                0x0000..=0x1FFF => self.ram_enabled = v & 0x0F == 0x0A,
                0x2000..=0x3FFF => self.rom_bank = u16::from(v & 0x1F),
                0x4000..=0x5FFF => self.bank2 = v & 3,
                _ => self.mode = v & 1,
            },
            MbcKind::Mbc3 => match addr {
                0x0000..=0x1FFF => self.ram_enabled = v & 0x0F == 0x0A,
                0x2000..=0x3FFF => {
                    let b = u16::from(v & 0x7F);
                    self.rom_bank = if b == 0 { 1 } else { b };
                }
                0x4000..=0x5FFF => self.ram_bank = v,
                _ => {
                    if self.rtc.latch_prev == 0 && v == 1 {
                        self.rtc.latched = self.rtc.regs;
                    }
                    self.rtc.latch_prev = v;
                }
            },
            MbcKind::Mbc5 => match addr {
                0x0000..=0x1FFF => self.ram_enabled = v & 0x0F == 0x0A,
                0x2000..=0x2FFF => self.rom_bank = (self.rom_bank & 0x100) | u16::from(v),
                0x3000..=0x3FFF => self.rom_bank = (self.rom_bank & 0xFF) | (u16::from(v & 1) << 8),
                0x4000..=0x5FFF => self.ram_bank = v & 0x0F,
                _ => {}
            },
        }
    }

    fn ram_index(&self, addr: u16) -> Option<usize> {
        if self.ram.is_empty() {
            return None;
        }
        let bank = match self.kind {
            MbcKind::None => 0,
            MbcKind::Mbc1 => if self.mode == 1 { self.bank2 as usize } else { 0 },
            MbcKind::Mbc3 => (self.ram_bank & 3) as usize,
            MbcKind::Mbc5 => self.ram_bank as usize,
        };
        Some((bank * 0x2000 + (addr as usize & 0x1FFF)) % self.ram.len())
    }

    pub fn read_ram(&self, addr: u16) -> u8 {
        if self.kind != MbcKind::None && !self.ram_enabled {
            return 0xFF;
        }
        if self.kind == MbcKind::Mbc3 && (0x08..=0x0C).contains(&self.ram_bank) {
            if !self.has_rtc {
                return 0xFF;
            }
            let i = (self.ram_bank - 8) as usize;
            let mask = [0x3F, 0x3F, 0x1F, 0xFF, 0xC1][i];
            return self.rtc.latched[i] & mask;
        }
        self.ram_index(addr).map_or(0xFF, |i| self.ram[i])
    }

    pub fn write_ram(&mut self, addr: u16, v: u8) {
        if self.kind != MbcKind::None && !self.ram_enabled {
            return;
        }
        if self.kind == MbcKind::Mbc3 && (0x08..=0x0C).contains(&self.ram_bank) {
            if self.has_rtc {
                let i = (self.ram_bank - 8) as usize;
                self.rtc.regs[i] = v;
                self.rtc.latched[i] = v;
                if i == 0 {
                    self.rtc.sub = 0;
                }
            }
            return;
        }
        if let Some(i) = self.ram_index(addr) {
            self.ram[i] = v;
        }
    }

    /// One M-cycle (4 T-cycles) for the clock.
    pub fn tick_m(&mut self) {
        if !self.has_rtc {
            return;
        }
        self.rtc.sub += 4;
        if self.rtc.sub >= 4_194_304 {
            self.rtc.sub -= 4_194_304;
            self.rtc.tick_second();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rom(kind: u8, banks: usize) -> Vec<u8> {
        let mut r = vec![0u8; banks * 0x4000];
        for b in 0..banks {
            r[b * 0x4000 + 0x10] = b as u8;
        }
        r[0x147] = kind;
        r[0x149] = 3;
        r
    }

    #[test]
    fn mbc1_banks_and_zero_maps_to_one() {
        let mut c = Cart::new(rom(0x03, 64)).unwrap();
        assert_eq!(c.read_rom(0x4010), 1);
        c.write_rom(0x2000, 0);
        assert_eq!(c.read_rom(0x4010), 1);
        c.write_rom(0x2000, 5);
        c.write_rom(0x4000, 1);
        assert_eq!(c.read_rom(0x4010), 0x25);
        c.write_rom(0x6000, 1);
        assert_eq!(c.read_rom(0x0010), 0x20);
    }

    #[test]
    fn mbc5_bank_zero_and_ninth_bit() {
        let mut c = Cart::new(rom(0x1B, 512)).unwrap();
        c.write_rom(0x2000, 0);
        assert_eq!(c.read_rom(0x4010), 0);
        c.write_rom(0x2000, 0x02);
        c.write_rom(0x3000, 1);
        assert_eq!(c.read_rom(0x4010), 0x02); // bank 258 -> low byte 2
        c.write_rom(0x0000, 0x0A);
        c.write_rom(0x4000, 2);
        c.write_ram(0xA000, 0x42);
        assert_eq!(c.read_ram(0xA000), 0x42);
        c.write_rom(0x4000, 1);
        assert_ne!(c.read_ram(0xA000), 0x42);
    }

    #[test]
    fn mbc3_rtc_latch_and_rollover() {
        let mut c = Cart::new(rom(0x10, 8)).unwrap();
        c.write_rom(0x0000, 0x0A);
        c.rtc.regs = [59, 59, 23, 0xFF, 0];
        c.rtc.tick_second();
        assert_eq!(c.rtc.regs, [0, 0, 0, 0, 1]);
        c.write_rom(0x6000, 0);
        c.write_rom(0x6000, 1);
        c.write_rom(0x4000, 0x0C);
        assert_eq!(c.read_ram(0xA000), 1);
    }
}
