//! 7800 cartridges: the .a78 header, recognising a headerless file, and every board's wiring
//! (bank switching, the RAM some carry, POKEY at $4000 or $0450).
//!
//! The .a78 header is the one the 7800 homebrew community documents (128 bytes, "ATARI7800" at
//! byte 1, the board in bytes 53-54, the controllers in 55-56, the TV system in 57, the save
//! device in 58). Each board behaves as its hardware does, which is what MAME 0.289's 7800 cart
//! slot plays: the plain boards put the ROM at the top of memory; SuperGame boards switch 16 KB
//! banks into $8000-$BFFF by a write there, with the last bank at $C000; Absolute (F-18 Hornet)
//! and Activision (Double Dragon, Rampage) have their own schemes.

use crate::pokey::Pokey;
use serde::{Deserialize, Serialize};

/// The board a cartridge has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// No bank switching, up to 48 KB at the top of memory.
    Rom,
    /// The same with a POKEY at $4000-$7FFF.
    RomPokey,
    /// 16 KB of RAM at $4000 whose A8 line isn't connected (Rescue on Fractalus prototype).
    Mram,
    /// SuperGame: 16 KB banks at $8000, the last at $C000, the one before it at $4000.
    Sg,
    /// SuperGame with a POKEY at $4000.
    SgPokey,
    /// SuperGame with 16 KB of RAM at $4000.
    SgRam,
    /// SuperGame with nine banks: bank 0 fixed at $4000.
    Sg9,
    /// Absolute's two 16 KB banks at $4000, the top 32 KB fixed.
    Absolute,
    /// Activision's eight banks at $A000.
    Activision,
    /// VersaBoard: SuperGame banks plus two banks of 16 KB RAM at $4000.
    Versa,
    /// MegaCart+: the VersaBoard with up to 512 KB.
    MegaCart,
    /// With a POKEY at $0450 as well.
    RomP450,
    RomPokeyP450,
    SgRamP450,
    Sg9P450,
    VersaP450,
    /// The XM expansion and the XBoarD: played as their SuperGame part.
    Xm,
    XBoard,
}

impl Kind {
    fn pokey_4000(self) -> bool {
        matches!(self, Kind::RomPokey | Kind::SgPokey | Kind::RomPokeyP450)
    }

    fn pokey_0450(self) -> bool {
        matches!(self, Kind::RomP450 | Kind::RomPokeyP450 | Kind::SgRamP450 | Kind::Sg9P450 | Kind::VersaP450)
    }

    fn ram_size(self) -> usize {
        match self {
            Kind::Mram | Kind::SgRam | Kind::SgRamP450 => 0x4000,
            Kind::Versa | Kind::MegaCart | Kind::VersaP450 => 0x8000,
            _ => 0,
        }
    }
}

/// What the header or the known-cartridge list says about a game.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Info {
    pub title: String,
    pub pal: Option<bool>,
    /// Controller types from the header (0 none, 1 joystick, 2 light gun, 3 paddle, 4 trak-ball,
    /// 5 2600 joystick, 6 2600 driving, 7 2600 keypad, 8 ST mouse, 9 Amiga mouse, 10 AtariVox or
    /// SaveKey, 11 SNES2Atari).
    pub controllers: [u8; 2],
    /// 1: the High Score Cartridge, 2: a SaveKey/AtariVox.
    pub save_device: u8,
    pub light_gun: bool,
    pub from_header: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Cart {
    pub kind: Kind,
    #[serde(skip)]
    rom: Vec<u8>,
    pub ram: Vec<u8>,
    bank: u8,
    ram_bank: u8,
    bank_mask: u8,
    base_rom: u32,
    pub pokey: Option<Pokey>,
    pub pokey450: Option<Pokey>,
    pub info: Info,
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// The board the header's mapper word describes, with the community's usual fixes for headers
/// that contradict themselves (as the .a78 header notes and MAME apply them).
fn kind_from_mapper(mut head: u16, len: usize) -> Kind {
    match head & 0x3D {
        0x05 | 0x09 | 0x11 | 0x21 => head &= !0x01,
        0x0C | 0x14 | 0x24 => head &= !0x04,
        0x18 | 0x28 => head &= !0x08,
        0x30 => head &= !0x10,
        _ => {}
    }
    if head & 0x3C != 0 && head & 0x02 == 0 {
        head |= 0x02;
    }
    if head & 0xFF00 == 0x0100 && head & 0xFF != 0 {
        head &= 0xFF00;
    }
    if head & 0xFF00 == 0x0200 && head & 0xFF != 0 {
        head &= 0xFF00;
    }
    if head & 0xFF00 > 0x0300 {
        head &= 0x00FF;
    }
    let mut kind = match head & 0x2E {
        0x00 => {
            if head & 1 != 0 {
                Kind::RomPokey
            } else {
                Kind::Rom
            }
        }
        0x02 => {
            if head & 1 != 0 {
                Kind::SgPokey
            } else {
                Kind::Sg
            }
        }
        0x06 => Kind::SgRam,
        0x0A => Kind::Sg9,
        0x22 | 0x26 => {
            if len > 0x40000 {
                Kind::MegaCart
            } else {
                Kind::Versa
            }
        }
        _ => Kind::Rom,
    };
    if head & 0x40 != 0 {
        kind = match kind {
            Kind::Rom => Kind::RomP450,
            Kind::RomPokey => Kind::RomPokeyP450,
            Kind::SgRam => Kind::SgRamP450,
            Kind::Sg9 => Kind::Sg9P450,
            Kind::Versa | Kind::MegaCart => Kind::VersaP450,
            Kind::Sg => Kind::Sg,
            k => k,
        };
    }
    if head & 0xFF00 == 0x0100 {
        kind = Kind::Activision;
    } else if head & 0xFF00 == 0x0200 {
        kind = Kind::Absolute;
    } else if head & 0x0080 != 0 {
        kind = Kind::Mram;
    }
    kind
}

impl Cart {
    /// Reads a cartridge file: with an .a78 header, or headerless (found by its contents in the
    /// known-cartridge list, or by its size).
    pub fn load(data: &[u8]) -> Result<Cart, String> {
        let mut info = Info::default();
        let (kind, rom) = if data.len() >= 128 && &data[1..10] == b"ATARI7800" {
            let head = &data[..128];
            let mut len = u32::from_be_bytes([head[49], head[50], head[51], head[52]]) as usize;
            if len == 0 || len + 128 > data.len() {
                len = data.len() - 128;
            }
            let rom = data[128..128 + len].to_vec();
            let mapper = u16::from_be_bytes([head[53], head[54]]);
            info.title = String::from_utf8_lossy(&head[17..49]).trim_end_matches(['\0', ' ']).to_string();
            info.controllers = [head[55], head[56]];
            info.pal = Some(head[57] & 1 != 0);
            info.save_device = head[58];
            info.light_gun = head[55] == 2 || head[56] == 2;
            info.from_header = true;
            (kind_from_mapper(mapper, len), rom)
        } else {
            let crc = crc32(data);
            if let Some(k) = crate::cart_list::KNOWN.iter().find(|k| k.crc == crc && k.size as usize == data.len()) {
                info.title = k.title.to_string();
                info.pal = Some(k.pal);
                info.light_gun = k.light_gun;
                info.controllers = [if k.light_gun { 2 } else { 1 }, 1];
                (k.kind, data.to_vec())
            } else {
                info.controllers = [1, 1];
                let kind = match data.len() {
                    0..=0xC000 => Kind::Rom,
                    0x24000 => Kind::Sg9,
                    _ => Kind::Sg,
                };
                (kind, data.to_vec())
            }
        };
        if rom.is_empty() {
            return Err("empty".into());
        }
        if kind == Kind::Rom && rom.len() > 0xC000 {
            return Err("too big for a cartridge without bank switching".into());
        }
        // The SuperGame boards' bank count: nine-bank carts have an odd number of 16 KB banks.
        let banks = rom.len().div_ceil(0x4000).max(1);
        let bank_mask = if banks & 1 == 1 && banks > 1 { banks - 2 } else { banks - 1 } as u8;
        let base_rom = 0x10000u32.saturating_sub(rom.len() as u32);
        Ok(Cart {
            kind,
            ram: vec![0; kind.ram_size()],
            bank: 0,
            ram_bank: 0,
            bank_mask,
            base_rom,
            pokey: kind.pokey_4000().then(Pokey::default),
            pokey450: kind.pokey_0450().then(Pokey::default),
            rom,
            info,
        })
    }

    /// Puts the ROM back after a save state is loaded (states don't carry it).
    pub fn restore_rom(&mut self, from: &Cart) {
        self.rom = from.rom.clone();
    }

    pub fn reset(&mut self) {
        self.bank = match self.kind {
            Kind::Sg9 | Kind::Sg9P450 => 1,
            _ => 0,
        };
        self.ram_bank = 0;
    }

    fn rom_at(&self, i: usize) -> u8 {
        self.rom.get(i % self.rom.len().max(1)).copied().unwrap_or(0xFF)
    }

    fn top_rom(&self, addr: u16) -> u8 {
        let a = addr as u32;
        if a < self.base_rom {
            0xFF
        } else {
            self.rom_at((a - self.base_rom) as usize)
        }
    }

    /// A read of $4000-$FFFF.
    pub fn read(&mut self, addr: u16) -> u8 {
        if addr < 0x8000 && self.kind.pokey_4000() {
            return self.pokey.as_mut().map_or(0xFF, |p| p.read(addr as u8));
        }
        self.peek(addr)
    }

    /// A read with no side effects: everything but the POKEY at $4000 (which reads as $FF here;
    /// MARIA's DMA never reads it).
    pub fn peek(&self, addr: u16) -> u8 {
        let off = addr as usize - 0x4000;
        let bank = self.bank as usize;
        let mask = self.bank_mask as usize;
        match self.kind {
            Kind::Rom | Kind::RomP450 | Kind::RomPokey | Kind::RomPokeyP450 => {
                if off < 0x4000 && self.kind.pokey_4000() {
                    0xFF
                } else {
                    self.top_rom(addr)
                }
            }
            Kind::Mram => {
                if off < 0x4000 {
                    self.ram[off & 0xFEFF]
                } else {
                    self.top_rom(addr)
                }
            }
            Kind::Sg | Kind::Xm | Kind::XBoard => {
                if off < 0x4000 {
                    self.rom_at((off & 0x3FFF) + mask.saturating_sub(1) * 0x4000)
                } else if off < 0x8000 {
                    self.rom_at((off & 0x3FFF) + bank * 0x4000)
                } else {
                    self.rom_at((off & 0x3FFF) + mask * 0x4000)
                }
            }
            Kind::SgPokey => {
                if off < 0x4000 {
                    0xFF
                } else if off < 0x8000 {
                    self.rom_at((off & 0x3FFF) + bank * 0x4000)
                } else {
                    self.rom_at((off & 0x3FFF) + mask * 0x4000)
                }
            }
            Kind::SgRam | Kind::SgRamP450 => {
                if off < 0x4000 {
                    self.ram[off]
                } else if off < 0x8000 {
                    self.rom_at((off & 0x3FFF) + bank * 0x4000)
                } else {
                    self.rom_at((off & 0x3FFF) + mask * 0x4000)
                }
            }
            Kind::Sg9 | Kind::Sg9P450 => {
                if off < 0x4000 {
                    self.rom_at(off & 0x3FFF)
                } else if off < 0x8000 {
                    self.rom_at((off & 0x3FFF) + bank * 0x4000)
                } else {
                    self.rom_at((off & 0x3FFF) + (mask + 1) * 0x4000)
                }
            }
            Kind::Absolute => {
                if off < 0x4000 {
                    self.rom_at((off & 0x3FFF) + bank * 0x4000)
                } else {
                    self.rom_at(off - 0x4000 + 0x8000)
                }
            }
            Kind::Activision => match off & 0xE000 {
                0x0000 | 0x2000 => self.rom_at(off | 0x18000),
                0x6000 | 0x8000 => self.rom_at((off & 0x3FFF) | (bank * 0x4000)),
                _ => self.rom_at(off | 0x1C000),
            },
            Kind::Versa | Kind::MegaCart | Kind::VersaP450 => {
                if off < 0x4000 {
                    self.ram[off + self.ram_bank as usize * 0x4000]
                } else if off < 0x8000 {
                    self.rom_at((off & 0x3FFF) + bank * 0x4000)
                } else {
                    self.rom_at((off & 0x3FFF) + mask * 0x4000)
                }
            }
        }
    }

    /// A write to $4000-$FFFF.
    pub fn write(&mut self, addr: u16, v: u8) {
        let off = addr as usize - 0x4000;
        let mask = self.bank_mask;
        match self.kind {
            Kind::Rom | Kind::RomP450 => {}
            Kind::RomPokey | Kind::RomPokeyP450 => {
                if off < 0x4000 {
                    if let Some(p) = self.pokey.as_mut() {
                        p.write(addr as u8, v);
                    }
                }
            }
            Kind::Mram => {
                if off < 0x4000 {
                    self.ram[off & 0xFEFF] = v;
                }
            }
            Kind::Sg | Kind::Xm | Kind::XBoard => {
                if (0x4000..0x8000).contains(&off) {
                    self.bank = v & mask;
                }
            }
            Kind::SgPokey => {
                if off < 0x4000 {
                    if let Some(p) = self.pokey.as_mut() {
                        p.write(addr as u8, v);
                    }
                } else if off < 0x8000 {
                    self.bank = v & mask;
                }
            }
            Kind::SgRam | Kind::SgRamP450 => {
                if off < 0x4000 {
                    self.ram[off] = v;
                } else if off < 0x8000 {
                    self.bank = v & mask;
                }
            }
            Kind::Sg9 | Kind::Sg9P450 => {
                if (0x4000..0x8000).contains(&off) {
                    self.bank = (v & mask) + 1;
                }
            }
            Kind::Absolute => {
                if off == 0x4000 {
                    if v & 1 != 0 {
                        self.bank = 0;
                    } else if v & 2 != 0 {
                        self.bank = 1;
                    }
                }
            }
            Kind::Activision => {
                if off >= 0xA000 {
                    self.bank = (off & 7) as u8;
                }
            }
            Kind::Versa | Kind::VersaP450 => {
                if off < 0x4000 {
                    self.ram[off + self.ram_bank as usize * 0x4000] = v;
                } else if off < 0x8000 {
                    self.bank = (v & 0x0F) & mask;
                    self.ram_bank = (v >> 5) & 1;
                }
            }
            Kind::MegaCart => {
                if off < 0x4000 {
                    self.ram[off + self.ram_bank as usize * 0x4000] = v;
                } else if off < 0x8000 {
                    self.bank = (v & 0x1F) & mask;
                    self.ram_bank = (v >> 5) & 1;
                }
            }
        }
    }

    /// $0400-$047F: the POKEY at $0450 on the boards that have one.
    pub fn read_04xx(&mut self, addr: u16) -> Option<u8> {
        let o = (addr & 0x7F) as u8;
        match self.pokey450.as_mut() {
            Some(p) if (0x50..0x60).contains(&o) => Some(p.read(o)),
            _ => None,
        }
    }

    pub fn write_04xx(&mut self, addr: u16, v: u8) -> bool {
        let o = (addr & 0x7F) as u8;
        match self.pokey450.as_mut() {
            Some(p) if (0x50..0x60).contains(&o) => {
                p.write(o, v);
                true
            }
            _ => false,
        }
    }

    /// Clocks the cartridge's POKEYs once (1.79 MHz) and returns their output levels added.
    pub fn clock_pokeys(&mut self) -> u8 {
        let mut s = 0;
        if let Some(p) = self.pokey.as_mut() {
            s += p.clock();
        }
        if let Some(p) = self.pokey450.as_mut() {
            s += p.clock();
        }
        s
    }

    pub fn has_pokey(&self) -> bool {
        self.pokey.is_some() || self.pokey450.is_some()
    }

    pub fn irq(&self) -> bool {
        self.pokey.as_ref().is_some_and(|p| p.irq()) || self.pokey450.as_ref().is_some_and(|p| p.irq())
    }

    pub fn rom(&self) -> &[u8] {
        &self.rom
    }
}
