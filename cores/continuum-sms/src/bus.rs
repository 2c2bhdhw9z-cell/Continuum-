//! The Master System's memory map and I/O ports, as the Z80 sees them.
//!
//! Memory: the cartridge in the bottom 48 KiB through the Sega mapper (the first KiB never moves,
//! then three 16 KiB slots whose banks are set by writing $FFFD, $FFFE and $FFFF, and $FFFC to put
//! cartridge RAM in the third slot), and 8 KiB of work RAM at $C000, seen again at $E000. Writes
//! to the four mapper registers land in work RAM too.
//!
//! Ports, decoded only by address lines 7, 6 and 0 as on a Master System: $3E memory control and
//! $3F the pads' output pins (writes); $7E and $7F the V and H counters (reads) and the sound chip
//! (writes); $BE and $BF the picture chip's data and control ports; $DC and $DD the two pads
//! (reads). Writes to $FC and $FD are the SDSC debug console, which nothing on a real console
//! answers and test programs print to.
//!
//! From SMS Power's "Sega Mapper", "Port $3E: Memory control" and "I/O port map" pages.

use serde::{Deserialize, Serialize};

use crate::psg::Psg;
use crate::vdp::Vdp;
use crate::z80;

#[derive(Clone, Serialize, Deserialize)]
pub struct Bus {
    #[serde(skip)]
    pub rom: Vec<u8>,
    pub ram: Vec<u8>,
    /// Cartridge RAM (two 16 KiB banks at most), and whether the game has used it.
    pub cart_ram: Vec<u8>,
    pub cart_ram_used: bool,
    /// The mapper's registers: $FFFC (RAM control) and the banks of the three slots.
    pub ram_ctrl: u8,
    pub banks: [u8; 3],
    /// Port $3E. Bit 6 off: the cartridge answers; bit 4 off: work RAM answers.
    pub mem_ctrl: u8,
    pub vdp: Vdp,
    pub psg: Psg,
    /// The pads, as their ports read (a bit is 0 while its button is held): $DC and $DD.
    pub pad_dc: u8,
    pub pad_dd: u8,
    /// Port $3F, the pads' output pins (for the region check and the light phaser).
    pub io_ctrl: u8,
    /// What test programs have printed to the SDSC debug console.
    #[serde(skip)]
    pub console: String,
    /// The last SDSC console command (1 asks the emulator to stop).
    #[serde(skip)]
    pub console_stop: bool,
}

impl Bus {
    pub fn new(rom: Vec<u8>) -> Bus {
        Bus {
            rom,
            ram: vec![0; 0x2000],
            cart_ram: vec![0; 0x8000],
            cart_ram_used: false,
            ram_ctrl: 0,
            banks: [0, 1, 2],
            // As a Master System's start-up file leaves it for a cartridge: the cartridge and
            // work RAM on, the BIOS, card and expansion slots off.
            mem_ctrl: 0xAB,
            vdp: Vdp::default(),
            psg: Psg::default(),
            pad_dc: 0xFF,
            pad_dd: 0xFF,
            io_ctrl: 0xFF,
            console: String::new(),
            console_stop: false,
        }
    }

    fn rom_at(&self, bank: u8, offset: u16) -> u8 {
        if self.rom.is_empty() {
            return 0xFF;
        }
        let banks = self.rom.len().div_ceil(0x4000).max(1);
        let a = (usize::from(bank) % banks) * 0x4000 + usize::from(offset);
        self.rom.get(a).copied().unwrap_or(0xFF)
    }

    fn cart_read(&self, addr: u16) -> u8 {
        if self.mem_ctrl & 0x40 != 0 {
            return 0xFF;
        }
        match addr {
            0x0000..=0x03FF => self.rom.get(usize::from(addr)).copied().unwrap_or(0xFF),
            0x0400..=0x3FFF => self.rom_at(self.banks[0], addr),
            0x4000..=0x7FFF => self.rom_at(self.banks[1], addr - 0x4000),
            _ if self.ram_ctrl & 0x08 != 0 => {
                let bank = usize::from(self.ram_ctrl >> 2 & 1) * 0x4000;
                self.cart_ram[bank + usize::from(addr - 0x8000)]
            }
            _ => self.rom_at(self.banks[2], addr - 0x8000),
        }
    }
}

impl z80::Bus for Bus {
    fn read(&mut self, addr: u16) -> u8 {
        if addr >= 0xC000 {
            if self.mem_ctrl & 0x10 != 0 {
                return 0xFF;
            }
            return self.ram[usize::from(addr & 0x1FFF)];
        }
        self.cart_read(addr)
    }

    fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0x8000..=0xBFFF if self.ram_ctrl & 0x08 != 0 => {
                let bank = usize::from(self.ram_ctrl >> 2 & 1) * 0x4000;
                self.cart_ram[bank + usize::from(addr - 0x8000)] = v;
                self.cart_ram_used = true;
            }
            0xC000..=0xFFFF => {
                if self.mem_ctrl & 0x10 == 0 {
                    self.ram[usize::from(addr & 0x1FFF)] = v;
                }
                match addr {
                    0xFFFC => self.ram_ctrl = v,
                    0xFFFD..=0xFFFF => self.banks[usize::from(addr - 0xFFFD)] = v,
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn input(&mut self, port: u16) -> u8 {
        let p = port as u8;
        match p & 0xC1 {
            0x00 | 0x01 => 0xFF,
            0x40 => self.vdp.v_counter(),
            0x41 => 0xFF,
            0x80 => self.vdp.read_data(),
            0x81 => self.vdp.read_status(),
            0xC0 => self.pad_dc,
            _ => self.pad_dd,
        }
    }

    fn output(&mut self, port: u16, v: u8) {
        let p = port as u8;
        match p {
            0xFC => {
                self.console_stop |= v == 1;
                if v == 2 {
                    self.console.clear();
                }
                return;
            }
            0xFD => {
                self.console.push(char::from(v));
                return;
            }
            _ => {}
        }
        match p & 0xC1 {
            0x00 => self.mem_ctrl = v,
            0x01 => self.io_ctrl = v,
            0x40 | 0x41 => self.psg.write(v),
            0x80 => self.vdp.write_data(v),
            0x81 => self.vdp.write_control(v),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::z80::Bus as _;

    fn rom(banks: usize) -> Vec<u8> {
        (0..banks * 0x4000).map(|a| (a / 0x4000) as u8).collect()
    }

    #[test]
    fn the_sega_mapper_pages_the_three_slots_and_the_first_kib_stays() {
        let mut b = Bus::new(rom(8));
        b.write(0xFFFD, 5);
        b.write(0xFFFE, 6);
        b.write(0xFFFF, 7);
        assert_eq!((b.read(0x0100), b.read(0x0400), b.read(0x4000), b.read(0x8000)), (0, 5, 6, 7));
        assert_eq!(b.read(0xDFFF), 7, "the register write lands in work RAM too");
        b.write(0xFFFF, 9);
        assert_eq!(b.read(0x8000), 1, "banks past the end wrap");
    }

    #[test]
    fn cartridge_ram_comes_in_over_the_third_slot() {
        let mut b = Bus::new(rom(4));
        b.write(0xFFFC, 0x08);
        b.write(0x8000, 0x42);
        assert_eq!(b.read(0x8000), 0x42);
        b.write(0xFFFC, 0x0C);
        assert_eq!(b.read(0x8000), 0, "the second bank");
        b.write(0xFFFC, 0);
        assert_eq!(b.read(0x8000), 2);
        assert!(b.cart_ram_used);
    }

    #[test]
    fn switching_the_cartridge_off_leaves_nothing_there() {
        let mut b = Bus::new(rom(4));
        b.output(0x3E, 0x6B);
        assert_eq!(b.read(0x4000), 0xFF);
        b.output(0x3E, 0xAB);
        assert_eq!(b.read(0x4000), 1);
    }
}
