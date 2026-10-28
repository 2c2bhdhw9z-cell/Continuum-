//! The XM expansion module: the box that plugs into the 7800's cartridge slot with the game on top
//! of it, adding 128 KB of RAM, a POKEY, a YM2151 FM chip and the High Score Cartridge's saving.
//! The XBoarD is the same board socketed inside the console.
//!
//! Its five control registers are the ones the 7800 community documents and the a7800 emulator
//! plays (MAME 0.289's own XM is a rougher stand-in: one 16 KB RAM window and no page registers).
//! Registers, all in the $0400-$047F window:
//!
//! - `$0470` cntrl1: bit 0-1 the ROF lines, bit 2 BIOS or the game, bit 3 the High Score part,
//!   bit 4 the POKEY at $0450, bit 5 RAM at $4000-$5FFF, bit 6 RAM at $6000-$7FFF, bit 7 the
//!   YM2151 at $0460-$0461. Everything is off at power-on, so a game sees the plain console until
//!   it switches a part on.
//! - `$0478` cntrl2 and `$047C` cntrl3: which 8 KB page shows in each RAM window, for the
//!   processor and for MARIA: bits 0-3 the $4000 window, bits 4-7 the $6000 window. The two chips
//!   see different pages, so MARIA can draw from RAM the processor is not looking at.
//! - `$0471` cntrl4: bit 0 a PIA, bits 1-6 flash banks, bit 7 locks the game slot (not emulated:
//!   no game uses them).
//! - `$0472` cntrl5: bit 0 48 KB of flat RAM over the whole cartridge space, bit 1 stops writes,
//!   bit 2 the BIOS in test mode, bits 3-5 lock the POKEY and High Score settings.
//!
//! The POKEY and the YM2151 are ours (pokey.rs, ym2151.rs). The High Score part is the console's
//! battery memory (machine.rs), as the plain High Score Cartridge is.

use crate::pokey::Pokey;
use crate::ym2151::Ym2151;
use serde::{Deserialize, Serialize};

pub const RAM: usize = 0x20000;

#[derive(Clone, Serialize, Deserialize)]
pub struct Xm {
    pub ram: Vec<u8>,
    pub pokey: Pokey,
    pub ym: Ym2151,
    cntrl: [u8; 5],
}

impl Default for Xm {
    fn default() -> Xm {
        Xm { ram: vec![0; RAM], pokey: Pokey::default(), ym: Ym2151::default(), cntrl: [0; 5] }
    }
}

/// What the XM answers at an address below $4000.
pub enum Low {
    None,
    Pokey,
    Ym,
    /// A control register (0 to 4 as above).
    Cntrl(usize),
    /// The High Score part: its RAM at $1000-$17FF, its ROM at $3000-$3FFF.
    HscRam,
    HscRom,
}

impl Xm {
    pub fn reset(&mut self) {
        self.cntrl = [0; 5];
    }

    /// True while the XM shows RAM at `addr` ($4000-$7FFF), or all of $4000-$FFFF in flat mode.
    pub fn ram_at(&self, addr: u16) -> bool {
        if self.cntrl[4] & 0x01 != 0 {
            return true;
        }
        match addr {
            0x4000..=0x5FFF => self.cntrl[0] & 0x20 != 0,
            0x6000..=0x7FFF => self.cntrl[0] & 0x40 != 0,
            _ => false,
        }
    }

    /// Where `addr` lands in the XM's RAM. `maria` picks MARIA's pages instead of the processor's.
    fn ram_index(&self, addr: u16, maria: bool) -> usize {
        if self.cntrl[4] & 0x01 != 0 {
            // Flat: 48 KB straight through, as cntrl5 bit 0 asks.
            return (addr as usize - 0x4000) & (RAM - 1);
        }
        let pages = self.cntrl[if maria { 2 } else { 1 }] as usize;
        let (page, rof) = if addr < 0x6000 {
            (pages & 15, (self.cntrl[0] as usize & 1) << 8)
        } else {
            ((pages >> 4) & 15, (self.cntrl[0] as usize & 2) << 7)
        };
        ((addr as usize & 0x1FFF) + page * 0x2000) | rof
    }

    pub fn read_ram(&self, addr: u16, maria: bool) -> u8 {
        self.ram[self.ram_index(addr, maria) & (RAM - 1)]
    }

    pub fn write_ram(&mut self, addr: u16, v: u8) {
        if self.cntrl[4] & 0x02 != 0 {
            return; // cntrl5 bit 1: writes held off
        }
        let i = self.ram_index(addr, false) & (RAM - 1);
        self.ram[i] = v;
    }

    /// What the XM answers at `addr` below $4000, if anything.
    pub fn low_kind(&self, addr: u16) -> Low {
        match addr {
            0x0450..=0x045F if self.cntrl[0] & 0x10 != 0 => Low::Pokey,
            0x0460..=0x0461 if self.cntrl[0] & 0x80 != 0 => Low::Ym,
            0x0470 => Low::Cntrl(0),
            0x0471 => Low::Cntrl(3),
            0x0472 => Low::Cntrl(4),
            0x0478 => Low::Cntrl(1),
            0x047C => Low::Cntrl(2),
            0x1000..=0x17FF if self.cntrl[0] & 0x08 != 0 => Low::HscRam,
            0x3000..=0x3FFF if self.cntrl[0] & 0x08 != 0 => Low::HscRom,
            _ => Low::None,
        }
    }

    /// A processor write to one of the XM's own registers (not the High Score part's RAM, which
    /// the console keeps). True if it went to a sound chip.
    pub fn write_low(&mut self, addr: u16, v: u8, now: u64) -> bool {
        match self.low_kind(addr) {
            Low::Pokey => {
                self.pokey.write(addr as u8, v);
                true
            }
            Low::Ym => {
                self.ym.write(addr as u8, v, now);
                true
            }
            Low::Cntrl(i) => {
                // Once a lock bit in cntrl5 is set, the POKEY and High Score bits stay as they are.
                if i == 0 {
                    let lock = (if self.cntrl[4] & 0x08 != 0 { 0x10 } else { 0 })
                        | (if self.cntrl[4] & 0x10 != 0 { 0x08 } else { 0 });
                    self.cntrl[0] = (v & !lock) | (self.cntrl[0] & lock);
                } else {
                    self.cntrl[i] = v;
                }
                false
            }
            _ => false,
        }
    }

    pub fn read_low(&mut self, addr: u16, now: u64) -> Option<u8> {
        match self.low_kind(addr) {
            Low::Pokey => Some(self.pokey.read(addr as u8)),
            // $0460 reads the chip's status, $0461 reads it too (the chip has one status byte).
            Low::Ym => Some(self.ym.status(now)),
            Low::Cntrl(_) => Some(0xFF),
            _ => None,
        }
    }

    /// The POKEY's level this clock (0 to 60), for the mix.
    pub fn clock_pokey(&mut self) -> u8 {
        self.pokey.clock()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_answers_until_a_game_switches_it_on() {
        let mut x = Xm::default();
        assert!(matches!(x.low_kind(0x0450), Low::None));
        assert!(matches!(x.low_kind(0x0460), Low::None));
        assert!(matches!(x.low_kind(0x1000), Low::None));
        assert!(!x.ram_at(0x4000));
        x.write_low(0x0470, 0xF8, 0);
        assert!(matches!(x.low_kind(0x0450), Low::Pokey));
        assert!(matches!(x.low_kind(0x0460), Low::Ym));
        assert!(matches!(x.low_kind(0x3000), Low::HscRom));
        assert!(x.ram_at(0x4000) && x.ram_at(0x7FFF));
    }

    #[test]
    fn the_processor_and_maria_see_their_own_pages() {
        let mut x = Xm::default();
        x.write_low(0x0470, 0x60, 0); // both RAM windows on
        x.write_low(0x0478, 0x21, 0); // the processor: page 1 at $4000, page 2 at $6000
        x.write_low(0x047C, 0x43, 0); // MARIA: page 3 and page 4
        x.ram[0x2000] = 0x11;
        x.ram[3 * 0x2000] = 0x33;
        x.ram[2 * 0x2000 + 0x100] = 0x22;
        x.ram[4 * 0x2000 + 0x100] = 0x44;
        assert_eq!((x.read_ram(0x4000, false), x.read_ram(0x4000, true)), (0x11, 0x33));
        assert_eq!((x.read_ram(0x6100, false), x.read_ram(0x6100, true)), (0x22, 0x44));
        // A write goes to the processor's page.
        x.write_ram(0x4000, 0x55);
        assert_eq!((x.read_ram(0x4000, false), x.read_ram(0x4000, true)), (0x55, 0x33));
        // cntrl5 bit 1 holds writes off.
        x.write_low(0x0472, 0x02, 0);
        x.write_ram(0x4000, 0x66);
        assert_eq!(x.read_ram(0x4000, false), 0x55);
    }

    #[test]
    fn the_locks_keep_the_pokey_and_high_score_on() {
        let mut x = Xm::default();
        x.write_low(0x0470, 0x18, 0); // POKEY and High Score on
        x.write_low(0x0472, 0x18, 0); // lock both
        x.write_low(0x0470, 0x00, 0);
        assert!(matches!(x.low_kind(0x0450), Low::Pokey));
        assert!(matches!(x.low_kind(0x3000), Low::HscRom));
    }
}
