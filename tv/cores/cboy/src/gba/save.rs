//! Cartridge saves: SRAM (32 KiB), Flash 64/128 KiB (with its command protocol and chip IDs) and
//! EEPROM (512 B or 8 KiB, serial over bit 0, size learned from the first DMA). The type is found
//! the way the official SDK marks it: an "EEPROM_V", "SRAM_V", "FLASH_V"/"FLASH512_V" or
//! "FLASH1M_V" string in the ROM.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaveKind {
    None,
    Sram,
    Flash64,
    Flash128,
    Eeprom,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Save {
    pub kind: SaveKind,
    pub data: Vec<u8>,
    // Flash
    flash_stage: u8,
    flash_id: bool,
    flash_erase: bool,
    flash_write: bool,
    flash_bank_next: bool,
    flash_bank: usize,
    // EEPROM
    pub eeprom_addr_bits: u8,
    bits_in: Vec<u8>,
    bits_out: Vec<u8>,
}

fn find(rom: &[u8], s: &[u8]) -> bool {
    rom.windows(s.len()).any(|w| w == s)
}

impl Save {
    pub fn detect(rom: &[u8]) -> Save {
        let kind = if find(rom, b"EEPROM_V") {
            SaveKind::Eeprom
        } else if find(rom, b"FLASH1M_V") {
            SaveKind::Flash128
        } else if find(rom, b"FLASH_V") || find(rom, b"FLASH512_V") {
            SaveKind::Flash64
        } else if find(rom, b"SRAM_V") || find(rom, b"SRAM_F_V") {
            SaveKind::Sram
        } else {
            SaveKind::None
        };
        Save::new(kind)
    }

    pub fn new(kind: SaveKind) -> Save {
        let size = match kind {
            SaveKind::None => 0,
            SaveKind::Sram => 0x8000,
            SaveKind::Flash64 => 0x10000,
            SaveKind::Flash128 => 0x20000,
            SaveKind::Eeprom => 0x2000,
        };
        Save {
            kind,
            data: vec![0xFF; size],
            flash_stage: 0,
            flash_id: false,
            flash_erase: false,
            flash_write: false,
            flash_bank_next: false,
            flash_bank: 0,
            eeprom_addr_bits: 0,
            bits_in: Vec::new(),
            bits_out: Vec::new(),
        }
    }

    /// $0E000000-$0FFFFFFF, 8-bit bus.
    pub fn read8(&self, addr: u32) -> u8 {
        let a = (addr & 0xFFFF) as usize;
        match self.kind {
            SaveKind::Sram => self.data[a & 0x7FFF],
            SaveKind::Flash64 | SaveKind::Flash128 => {
                if self.flash_id && a < 2 {
                    let id: [u8; 2] = if self.kind == SaveKind::Flash128 { [0x62, 0x13] } else { [0x32, 0x1B] };
                    return id[a];
                }
                self.data[self.flash_bank * 0x10000 + a]
            }
            _ => 0xFF,
        }
    }

    pub fn write8(&mut self, addr: u32, v: u8) {
        let a = (addr & 0xFFFF) as usize;
        match self.kind {
            SaveKind::Sram => self.data[a & 0x7FFF] = v,
            SaveKind::Flash64 | SaveKind::Flash128 => self.flash_write8(a, v),
            _ => {}
        }
    }

    fn flash_write8(&mut self, a: usize, v: u8) {
        if self.flash_write {
            self.flash_write = false;
            let i = self.flash_bank * 0x10000 + a;
            self.data[i] &= v;
            return;
        }
        if self.flash_bank_next && a == 0 {
            self.flash_bank_next = false;
            if self.kind == SaveKind::Flash128 {
                self.flash_bank = usize::from(v & 1);
            }
            return;
        }
        match (self.flash_stage, a, v) {
            (0, 0x5555, 0xAA) => self.flash_stage = 1,
            (1, 0x2AAA, 0x55) => self.flash_stage = 2,
            (2, 0x5555, cmd) => {
                self.flash_stage = 0;
                match cmd {
                    0x90 => self.flash_id = true,
                    0xF0 => self.flash_id = false,
                    0x80 => self.flash_erase = true,
                    0x10 if self.flash_erase => {
                        self.data.iter_mut().for_each(|b| *b = 0xFF);
                        self.flash_erase = false;
                    }
                    0xA0 => self.flash_write = true,
                    0xB0 => self.flash_bank_next = true,
                    _ => {}
                }
            }
            (2, sector, 0x30) if self.flash_erase => {
                self.flash_stage = 0;
                self.flash_erase = false;
                let base = self.flash_bank * 0x10000 + (sector & 0xF000);
                self.data[base..base + 0x1000].iter_mut().for_each(|b| *b = 0xFF);
            }
            _ => self.flash_stage = 0,
        }
    }

    /// A DMA to the EEPROM tells its address width: 9 or 73 units for 6-bit, 17 or 81 for 14-bit.
    pub fn eeprom_dma(&mut self, count: u32) {
        if self.eeprom_addr_bits == 0 {
            self.eeprom_addr_bits = match count {
                9 | 73 => 6,
                17 | 81 => 14,
                _ => 0,
            };
        }
    }

    fn addr_bits(&self) -> usize {
        if self.eeprom_addr_bits == 0 { 14 } else { self.eeprom_addr_bits as usize }
    }

    pub fn eeprom_read(&mut self) -> u16 {
        if self.bits_out.is_empty() {
            1
        } else {
            u16::from(self.bits_out.remove(0))
        }
    }

    pub fn eeprom_write(&mut self, v: u16) {
        self.bits_in.push((v & 1) as u8);
        let n = self.addr_bits();
        if self.bits_in.len() < 2 {
            return;
        }
        let read = self.bits_in[0] == 1 && self.bits_in[1] == 1;
        let need = if read { 2 + n + 1 } else { 2 + n + 64 + 1 };
        if self.bits_in.len() < need {
            return;
        }
        let mut addr = 0usize;
        for i in 0..n {
            addr = addr << 1 | usize::from(self.bits_in[2 + i]);
        }
        let base = (addr * 8) % self.data.len().max(8);
        if read {
            self.bits_out = vec![0; 4];
            for b in 0..8 {
                let byte = self.data[base + b];
                for bit in (0..8).rev() {
                    self.bits_out.push((byte >> bit) & 1);
                }
            }
        } else {
            for b in 0..8 {
                let mut byte = 0u8;
                for bit in 0..8 {
                    byte = byte << 1 | self.bits_in[2 + n + b * 8 + bit];
                }
                self.data[base + b] = byte;
            }
            self.bits_out.clear();
        }
        self.bits_in.clear();
    }
}
