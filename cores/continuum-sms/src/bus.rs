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

/// How the cartridge banks its ROM (SMS Power, "Mappers").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mapper {
    /// Sega's own: $FFFC-$FFFF, the first KiB fixed.
    #[default]
    Sega,
    /// Codemasters': a write to $0000, $4000 or $8000 picks that slot's bank, nothing fixed, and
    /// 8 KiB of RAM at $A000 when bit 7 of the $4000 bank is set (Ernie Els Golf).
    Codemasters,
    /// The Korean one: a write to $A000 picks the third slot's bank; the first two stay.
    Korean,
}

impl Mapper {
    /// Codemasters cartridges carry their own header at $7FE0, whose checksum at $7FE6 and its
    /// complement at $7FE8 add up to $10000 (the way Gearsystem and Genesis Plus GX tell them).
    pub fn detect(rom: &[u8]) -> Mapper {
        let word = |a: usize| rom.get(a..a + 2).map_or(0u32, |b| u32::from(b[0]) | u32::from(b[1]) << 8);
        if rom.len() >= 0x8000 && word(0x7FE6) != 0 && word(0x7FE6) + word(0x7FE8) == 0x10000 {
            Mapper::Codemasters
        } else {
            Mapper::Sega
        }
    }
}

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
    #[serde(default)]
    pub mapper: Mapper,
    /// Port $3E. Bit 6 off: the cartridge answers; bit 4 off: work RAM answers.
    pub mem_ctrl: u8,
    pub vdp: Vdp,
    pub psg: Psg,
    /// The pads, as their ports read (a bit is 0 while its button is held): $DC and $DD.
    pub pad_dc: u8,
    pub pad_dd: u8,
    /// Port $3F, the pads' output pins (for the region check and the light phaser).
    pub io_ctrl: u8,
    /// A Game Gear: its own ports $00-$06 (the Start button, the region, the link port, stereo).
    #[serde(default)]
    pub gg: bool,
    /// An SG-1000: the cartridge straight in the bottom 48 KiB with no banking, 1 KiB of work RAM
    /// seen eight times over at $C000, no memory control port.
    #[serde(default)]
    pub sg: bool,
    /// The Game Gear's Start button is held.
    #[serde(skip)]
    pub gg_start: bool,
    /// The console is an export one (not Japanese), as port $00 (Game Gear) reports.
    #[serde(default = "yes")]
    pub export: bool,
    /// What test programs have printed to the SDSC debug console.
    #[serde(skip)]
    pub console: String,
    /// The last SDSC console command (1 asks the emulator to stop).
    #[serde(skip)]
    pub console_stop: bool,
}

fn yes() -> bool {
    true
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
            mapper: Mapper::Sega,
            // As a Master System's start-up file leaves it for a cartridge: the cartridge and
            // work RAM on, the BIOS, card and expansion slots off.
            mem_ctrl: 0xAB,
            vdp: Vdp::default(),
            psg: Psg::default(),
            pad_dc: 0xFF,
            pad_dd: 0xFF,
            io_ctrl: 0xFF,
            gg: false,
            sg: false,
            gg_start: false,
            export: true,
            console: String::new(),
            console_stop: false,
        }
    }

    /// Makes this an SG-1000.
    pub fn set_sg1000(&mut self) {
        self.sg = true;
        self.vdp.tms = true;
    }

    /// Makes this a Game Gear.
    pub fn set_game_gear(&mut self) {
        self.gg = true;
        self.vdp.set_game_gear();
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
        if self.sg {
            return self.rom.get(usize::from(addr)).copied().unwrap_or(0xFF);
        }
        if self.mem_ctrl & 0x40 != 0 {
            return 0xFF;
        }
        if self.mapper == Mapper::Codemasters && (0xA000..0xC000).contains(&addr) && self.banks[1] & 0x80 != 0 {
            return self.cart_ram[usize::from(addr - 0xA000)];
        }
        match addr {
            0x0000..=0x03FF if self.mapper == Mapper::Sega => self.rom.get(usize::from(addr)).copied().unwrap_or(0xFF),
            0x0000..=0x3FFF => self.rom_at(self.banks[0], addr),
            0x4000..=0x7FFF => self.rom_at(self.banks[1], addr - 0x4000),
            _ if self.ram_ctrl & 0x08 != 0 && self.mapper == Mapper::Sega => {
                let bank = usize::from(self.ram_ctrl >> 2 & 1) * 0x4000;
                self.cart_ram[bank + usize::from(addr - 0x8000)]
            }
            _ => self.rom_at(self.banks[2], addr - 0x8000),
        }
    }
}

impl z80::Bus for Bus {
    fn read(&mut self, addr: u16) -> u8 {
        if self.sg && addr >= 0xC000 {
            return self.ram[usize::from(addr & 0x3FF)];
        }
        if addr >= 0xC000 {
            if self.mem_ctrl & 0x10 != 0 {
                return 0xFF;
            }
            return self.ram[usize::from(addr & 0x1FFF)];
        }
        self.cart_read(addr)
    }

    fn write(&mut self, addr: u16, v: u8) {
        if self.sg {
            if addr >= 0xC000 {
                self.ram[usize::from(addr & 0x3FF)] = v;
            }
            return;
        }
        match (self.mapper, addr) {
            (Mapper::Codemasters, 0xA000..=0xBFFF) if self.banks[1] & 0x80 != 0 => {
                self.cart_ram[usize::from(addr - 0xA000)] = v;
                self.cart_ram_used = true;
                return;
            }
            (Mapper::Codemasters, 0x0000 | 0x4000 | 0x8000) => {
                self.banks[usize::from(addr >> 14)] = v;
                return;
            }
            (Mapper::Korean, 0xA000) => {
                self.banks[2] = v;
                return;
            }
            (Mapper::Codemasters | Mapper::Korean, 0xFFFC..=0xFFFF) => {
                if self.mem_ctrl & 0x10 == 0 {
                    self.ram[usize::from(addr & 0x1FFF)] = v;
                }
                return;
            }
            _ => {}
        }
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
        if self.gg && p < 7 {
            return match p {
                0 => u8::from(!self.gg_start) << 7 | u8::from(self.export) << 6,
                1 => 0x7F,
                2 => 0xFF,
                5 => 0x00,
                6 => self.psg.stereo,
                _ => 0xFF,
            };
        }
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
        if self.gg && p < 7 {
            if p == 6 {
                self.psg.stereo = v;
            }
            return;
        }
        match p & 0xC1 {
            0x00 if !self.sg => self.mem_ctrl = v,
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
    fn codemasters_cartridges_bank_all_three_slots_from_their_own_addresses() {
        let mut r = rom(8);
        r[0x7FE6..0x7FEA].copy_from_slice(&[0x34, 0x12, 0xCC, 0xED]); // $1234 + $EDCC = $10000
        assert_eq!(Mapper::detect(&r), Mapper::Codemasters);
        assert_eq!(Mapper::detect(&rom(8)), Mapper::Sega);
        let mut b = Bus::new(r);
        b.mapper = Mapper::Codemasters;
        b.write(0x0000, 3);
        b.write(0x4000, 4);
        b.write(0x8000, 5);
        assert_eq!((b.read(0x0000), b.read(0x4000), b.read(0x8000)), (3, 4, 5), "nothing fixed");
        b.write(0xFFFF, 7);
        assert_eq!(b.read(0x8000), 5, "Sega's registers do nothing here");
        b.write(0x4000, 0x84);
        b.write(0xA000, 0x99);
        assert_eq!(b.read(0xA000), 0x99, "its RAM");
    }

    #[test]
    fn the_korean_mapper_banks_only_the_third_slot() {
        let mut b = Bus::new(rom(8));
        b.mapper = Mapper::Korean;
        b.write(0xA000, 6);
        assert_eq!((b.read(0x0400), b.read(0x4000), b.read(0x8000)), (0, 1, 6));
    }

    #[test]
    fn an_sg_1000_has_no_banking_and_one_kib_of_ram() {
        let mut b = Bus::new(rom(3));
        b.set_sg1000();
        b.write(0xFFFF, 0);
        assert_eq!(b.read(0x8000), 2, "no mapper");
        b.write(0xC000, 0x55);
        assert_eq!((b.read(0xC400), b.read(0xFC00)), (0x55, 0x55));
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
