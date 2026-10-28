//! 7800 cartridges: the .a78 header, recognising a headerless file, and every board's wiring
//! (bank switching, the RAM some carry, POKEYs, the High Score Cartridge).
//!
//! The .a78 header is the one the 7800 homebrew community documents (128 bytes, "ATARI7800" at
//! byte 1, the board in bytes 53-54, the controllers in 55-56, the TV system in 57, the save
//! device in 58). Each board behaves as its hardware does: the plain boards put the ROM at the
//! top of memory; SuperGame boards switch 16 KB banks into $8000-$BFFF by a write there, with the
//! last bank at $C000; Absolute (F-18 Hornet) and Activision (Double Dragon, Rampage) have their
//! own schemes (all as MAME 0.289 plays them). The boards MAME doesn't have follow the emulators
//! that do: BankSet boards (two images in the file, one for the processor and one for MARIA) and
//! POKEY at $0800 as the a7800 emulator plays them, the Souper board (Rikki & Vikki) and POKEY at
//! $0440 as ProSystem does.

use crate::pokey::Pokey;
use serde::{Deserialize, Serialize};

/// How a cartridge's ROM and RAM are switched in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// No bank switching, up to 48 KB at the top of memory.
    Rom,
    /// 16 KB of RAM at $4000 whose A8 line isn't connected (Rescue on Fractalus prototype).
    Mram,
    /// SuperGame: 16 KB banks at $8000, the last at $C000, the one before it at $4000.
    Sg,
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
    /// The XM expansion and the XBoarD: played as their SuperGame part.
    Xm,
    XBoard,
    /// The Souper board (Rikki & Vikki): 32 KB of RAM in pages at $4000, 16 KB banks at $8000,
    /// bank 31 at $C000, and a map of its own for MARIA's reads.
    Souper,
    /// BankSet: the processor's image and MARIA's, each at the top of memory (down to $3000
    /// for 52 KB ones).
    BankSet,
    /// The same with 16 KB of RAM at $4000 for each side; the processor writes MARIA's at $C000.
    BankSetRam,
    /// BankSet with SuperGame banks on each side.
    BankSetSg,
    BankSetSgRam,
}

impl Kind {
    fn ram_size(self) -> usize {
        match self {
            Kind::Mram | Kind::SgRam => 0x4000,
            Kind::Versa | Kind::MegaCart | Kind::Souper | Kind::BankSetRam | Kind::BankSetSgRam => 0x8000,
            _ => 0,
        }
    }

    fn bankset(self) -> bool {
        matches!(self, Kind::BankSet | Kind::BankSetRam | Kind::BankSetSg | Kind::BankSetSgRam)
    }
}

/// Where a cartridge's POKEYs are: one at $4000 (`at_4000`), and one low in memory
/// (`low`: $0440, $0450 or $0800). Bit 0 of `bits` in the known list is $4000, bit 1 $0450.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pokeys {
    pub at_4000: bool,
    pub low: Option<u16>,
}

impl Pokeys {
    pub const fn from_bits(bits: u8) -> Pokeys {
        Pokeys { at_4000: bits & 1 != 0, low: if bits & 2 != 0 { Some(0x0450) } else { None } }
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
    /// The game wants the XM expansion module (the header's expansion byte, or a YM2151 on it).
    pub xm: bool,
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
    /// BankSet: where MARIA's image starts in the file.
    half: usize,
    /// The POKEY at $4000, and whether it can be read (BankSet's can only be written).
    pub pokey: Option<Pokey>,
    pokey_reads: bool,
    /// The second POKEY, at `pokey2_at`.
    pub pokey2: Option<Pokey>,
    pokey2_at: u16,
    /// The Souper board's registers: character banks A and B, mode, RAM pages for $6000 and
    /// $7000.
    souper: [u8; 5],
    pub info: Info,
}

const SOUPER_MFT: u8 = 0x01;
const SOUPER_CHR: u8 = 0x02;
const SOUPER_EXS: u8 = 0x04;

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

/// The board the header's mapper word describes (bits 0-15 as the .a78 v4 header documents
/// them), with the community's usual fixes for headers that contradict themselves (as MAME and
/// the a7800 emulator apply them).
/// The same from a version 4 header's own bytes (64 the board, 65 its options, 67 the sound
/// chips), as the community's 7800header tool writes them. A version 4 file may fill the older
/// bytes 53-54 with $FF, which is what sends us here.
fn board_from_v4(mapper: u8, options: u8, audio: u8, len: usize) -> (Kind, Pokeys, bool, bool) {
    let bankset = options & 0x80 != 0;
    let kind = match (mapper, options & 7, bankset) {
        (4, _, _) => Kind::Souper,
        (2, _, _) => Kind::Activision,
        (3, _, _) => Kind::Absolute,
        (1, 1, false) => Kind::SgRam,
        (1, 2, false) => Kind::Mram,
        (1, 6, false) => {
            if len > 0x40000 {
                Kind::MegaCart
            } else {
                Kind::Versa
            }
        }
        (1, 5, false) => Kind::Sg9,
        (1, _, false) => Kind::Sg,
        (1, 1 | 6, true) => Kind::BankSetSgRam,
        (1, _, true) => Kind::BankSetSg,
        (_, 1 | 6, true) => Kind::BankSetRam,
        (_, _, true) => Kind::BankSet,
        (_, 2, false) => Kind::Mram,
        _ => Kind::Rom,
    };
    // The sound byte's low three bits say where a POKEY is: 1 $0440, 2 $0450, 3 both, 4 $0800,
    // 5 $4000.
    let (at_4000, low) = match audio & 7 {
        1 => (false, Some(0x0440)),
        2 => (false, Some(0x0450)),
        3 => (false, Some(0x0440)),
        4 => (false, Some(0x0800)),
        5 => (true, None),
        _ => (false, None),
    };
    let pokeys = Pokeys { at_4000: at_4000 && matches!(kind, Kind::Rom | Kind::Sg | Kind::BankSet), low };
    (kind, pokeys, !kind.bankset(), audio & 0x08 != 0)
}

fn board_from_mapper(mut head: u16, len: usize) -> (Kind, Pokeys, bool) {
    match head & 0x3D {
        0x05 | 0x09 | 0x11 | 0x21 => head &= !0x01,
        0x0C | 0x14 | 0x24 => head &= !0x04,
        0x18 | 0x28 => head &= !0x08,
        0x30 => head &= !0x10,
        _ => {}
    }
    if head & 0x3C != 0 && head & 0x02 == 0 && head & 0x2000 == 0 {
        head |= 0x02;
    }
    if head & 0xFF00 == 0x0100 && head & 0xFF != 0 {
        head &= 0xFF00;
    }
    if head & 0xFF00 == 0x0200 && head & 0xFF != 0 {
        head &= 0xFF00;
    }
    let mut pokeys = Pokeys { at_4000: head & 0x0001 != 0, low: None };
    // The second POKEY: $0800 before $0440 before $0450, as ProSystem picks.
    if head & 0x8000 != 0 {
        pokeys.low = Some(0x0800);
    } else if head & 0x0400 != 0 {
        pokeys.low = Some(0x0440);
    } else if head & 0x0040 != 0 {
        pokeys.low = Some(0x0450);
    }
    let mut pokey_reads = true;
    let kind = if head & 0xFF00 == 0x1000 {
        pokeys = Pokeys::default();
        Kind::Souper
    } else if head & 0x2000 != 0 {
        let sg = head & 0x0002 != 0;
        let ram = head & 0x4000 != 0;
        if sg || ram {
            pokeys.at_4000 = false;
        }
        if pokeys.low == Some(0x0450) {
            pokeys.low = None;
        }
        pokey_reads = false;
        match (sg, ram) {
            (false, false) => Kind::BankSet,
            (false, true) => Kind::BankSetRam,
            (true, false) => Kind::BankSetSg,
            (true, true) => Kind::BankSetSgRam,
        }
    } else {
        match head & 0x2E {
            0x00 => Kind::Rom,
            0x02 => Kind::Sg,
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
        }
    };
    let kind = if head & 0xFF00 == 0x0100 {
        pokeys = Pokeys::default();
        Kind::Activision
    } else if head & 0xFF00 == 0x0200 {
        pokeys = Pokeys::default();
        Kind::Absolute
    } else if head & 0x0080 != 0 && !kind.bankset() && kind != Kind::Souper {
        Kind::Mram
    } else {
        kind
    };
    if !matches!(kind, Kind::Rom | Kind::Sg | Kind::BankSet) {
        // Only these boards have a POKEY at $4000 rather than RAM or a bank there.
        pokeys.at_4000 = false;
    }
    (kind, pokeys, pokey_reads)
}

impl Cart {
    /// Reads a cartridge file: with an .a78 header, or headerless (found by its contents in the
    /// known-cartridge list, or by its size).
    pub fn load(data: &[u8]) -> Result<Cart, String> {
        let mut info = Info::default();
        let (kind, pokeys, pokey_reads, rom) = if data.len() >= 128 && &data[1..10] == b"ATARI7800" {
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
            // A version 4 header's own bytes, when the older ones are filled in with $FF.
            let (kind, pokeys, reads, ym) = if head[0] >= 4 && mapper == 0xFFFF {
                board_from_v4(head[64], head[65], head[67], len)
            } else {
                let (k, p, r) = board_from_mapper(mapper, len);
                // The older word's bit 11 is a YM2151 at $0460; a version 4 header also says so
                // in its sound byte.
                let ym = mapper & 0x0800 != 0 || (head[0] >= 4 && head[67] & 0x08 != 0);
                (k, p, r, ym)
            };
            info.xm = ym || head[63] == 1;
            (kind, pokeys, reads, rom)
        } else {
            let crc = crc32(data);
            if let Some(k) = crate::cart_list::KNOWN.iter().find(|k| k.crc == crc && k.size as usize == data.len()) {
                info.title = k.title.to_string();
                info.pal = Some(k.pal);
                info.light_gun = k.light_gun;
                info.controllers = [if k.light_gun { 2 } else { 1 }, 1];
                (k.kind, Pokeys::from_bits(k.pokeys), true, data.to_vec())
            } else {
                info.controllers = [1, 1];
                let kind = match data.len() {
                    0..=0xC000 => Kind::Rom,
                    0x24000 => Kind::Sg9,
                    _ => Kind::Sg,
                };
                (kind, Pokeys::default(), true, data.to_vec())
            }
        };
        Cart::build(kind, pokeys, pokey_reads, rom, info)
    }

    /// A ProSystem cartridge description (.cdf): "ProSystem", the board ("EMPTY", "SUPER",
    /// "ABSOLUTE", "ACTIVISION" or "SOUPER"), the title, the ROM file (read with `read`, a path
    /// beside the .cdf), then the files of the Souper board's music player, which isn't played.
    pub fn from_cdf(text: &str, read: impl Fn(&str) -> Option<Vec<u8>>) -> Result<Cart, String> {
        let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
        if lines.next() != Some("ProSystem") {
            return Err("not a ProSystem description".into());
        }
        let kind = match lines.next() {
            Some("SUPER") => Kind::Sg,
            Some("ABSOLUTE") => Kind::Absolute,
            Some("ACTIVISION") => Kind::Activision,
            Some("SOUPER") => Kind::Souper,
            _ => Kind::Rom,
        };
        let info = Info { title: lines.next().unwrap_or_default().to_string(), controllers: [1, 1], ..Info::default() };
        let file = lines.next().ok_or("no ROM file named")?.replace('\\', "/");
        let rom = read(&file).ok_or_else(|| format!("{file} is missing"))?;
        Cart::build(kind, Pokeys::default(), true, rom, info)
    }

    fn build(kind: Kind, pokeys: Pokeys, pokey_reads: bool, rom: Vec<u8>, info: Info) -> Result<Cart, String> {
        if rom.is_empty() {
            return Err("empty".into());
        }
        if kind == Kind::Rom && rom.len() > 0xC000 {
            return Err("too big for a cartridge without bank switching".into());
        }
        if kind.bankset() && (!rom.len().is_multiple_of(2) || (matches!(kind, Kind::BankSet | Kind::BankSetRam) && rom.len() > 0x1A000)) {
            return Err("not a BankSet image".into());
        }
        // BankSet: each side is half the file.
        let half = if kind.bankset() { rom.len() / 2 } else { rom.len() };
        // The SuperGame boards' bank count: nine-bank carts have an odd number of 16 KB banks.
        let banks = half.div_ceil(0x4000).max(1);
        let bank_mask = if banks & 1 == 1 && banks > 1 { banks - 2 } else { banks - 1 } as u8;
        let base_rom = 0x10000u32.saturating_sub(half as u32);
        Ok(Cart {
            kind,
            ram: vec![0; kind.ram_size()],
            bank: 0,
            ram_bank: 0,
            bank_mask,
            base_rom,
            half: if kind.bankset() { half } else { 0 },
            pokey: pokeys.at_4000.then(Pokey::default),
            pokey_reads,
            pokey2: pokeys.low.map(|_| Pokey::default()),
            pokey2_at: pokeys.low.unwrap_or(0),
            souper: [0; 5],
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
            Kind::Sg9 => 1,
            _ => 0,
        };
        self.ram_bank = 0;
        self.souper = [0; 5];
    }

    fn rom_at(&self, i: usize) -> u8 {
        self.rom.get(i % self.rom.len().max(1)).copied().unwrap_or(0xFF)
    }

    /// BankSet: a byte of the processor's image (`side` 0) or MARIA's (`side` 1).
    fn side_at(&self, side: usize, i: usize) -> u8 {
        let h = self.half.max(1);
        self.rom[side * h + i % h]
    }

    fn top_rom(&self, addr: u16) -> u8 {
        let a = addr as u32;
        if a < self.base_rom {
            0xFF
        } else {
            self.rom_at((a - self.base_rom) as usize)
        }
    }

    /// A read of $4000-$FFFF by the processor.
    pub fn read(&mut self, addr: u16) -> u8 {
        if addr < 0x8000 && self.pokey_reads {
            if let Some(p) = self.pokey.as_mut() {
                return p.read(addr as u8);
            }
        }
        self.peek(addr)
    }

    /// What the processor sees at $4000-$FFFF, with no side effects (a POKEY at $4000 reads as
    /// $FF here).
    pub fn peek(&self, addr: u16) -> u8 {
        self.view(addr, 0)
    }

    /// What MARIA's DMA sees at $4000-$FFFF.
    pub fn maria_peek(&self, addr: u16) -> u8 {
        match self.kind {
            Kind::Souper => self.souper_maria(addr),
            k if k.bankset() => self.view(addr, 1),
            _ => self.view(addr, 0),
        }
    }

    fn view(&self, addr: u16, side: usize) -> u8 {
        let off = addr as usize - 0x4000;
        let bank = self.bank as usize;
        let mask = self.bank_mask as usize;
        if off < 0x4000 && self.pokey.is_some() && self.pokey_reads {
            return 0xFF;
        }
        match self.kind {
            Kind::Rom => self.top_rom(addr),
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
            Kind::SgRam => {
                if off < 0x4000 {
                    self.ram[off]
                } else if off < 0x8000 {
                    self.rom_at((off & 0x3FFF) + bank * 0x4000)
                } else {
                    self.rom_at((off & 0x3FFF) + mask * 0x4000)
                }
            }
            Kind::Sg9 => {
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
            Kind::Versa | Kind::MegaCart => {
                if off < 0x4000 {
                    self.ram[off + self.ram_bank as usize * 0x4000]
                } else if off < 0x8000 {
                    self.rom_at((off & 0x3FFF) + bank * 0x4000)
                } else {
                    self.rom_at((off & 0x3FFF) + mask * 0x4000)
                }
            }
            Kind::Souper => {
                if off < 0x4000 {
                    self.ram[self.souper_ram(addr)]
                } else if off < 0x8000 {
                    self.rom_at((off & 0x3FFF) + bank * 0x4000)
                } else {
                    self.rom_at((off & 0x3FFF) + 31 * 0x4000)
                }
            }
            Kind::BankSet => {
                let a = addr as u32;
                if a < self.base_rom {
                    0xFF
                } else {
                    self.side_at(side, (a - self.base_rom) as usize)
                }
            }
            Kind::BankSetRam => {
                if off < 0x4000 {
                    self.ram[side * 0x4000 + off]
                } else {
                    let a = addr as u32;
                    if a < self.base_rom {
                        0xFF
                    } else {
                        self.side_at(side, (a - self.base_rom) as usize)
                    }
                }
            }
            Kind::BankSetSg | Kind::BankSetSgRam => {
                // Each side is a SuperGame image; the bank count is one side's.
                if off < 0x4000 {
                    if self.kind == Kind::BankSetSgRam {
                        self.ram[side * 0x4000 + off]
                    } else if self.pokey2_at == 0x0800 {
                        0xFF
                    } else {
                        self.side_at(side, off + mask.saturating_sub(1) * 0x4000)
                    }
                } else if off < 0x8000 {
                    self.side_at(side, (off & 0x3FFF) + bank * 0x4000)
                } else {
                    self.side_at(side, (off & 0x3FFF) + mask * 0x4000)
                }
            }
        }
    }

    /// The Souper board's RAM: four 4 KB pages at $4000-$7FFF, of eight; in its extended mode the
    /// pages at $6000 and $7000 are picked by its registers.
    fn souper_ram(&self, addr: u16) -> usize {
        let mut page = ((addr as usize - 0x4000) >> 12) & 3;
        if self.souper[2] & SOUPER_EXS != 0 {
            match addr {
                0x6000..=0x6FFF => page = self.souper[3] as usize & 7,
                0x7000..=0x7FFF => page = self.souper[4] as usize & 7,
                _ => {}
            }
        }
        (addr as usize & 0x0FFF) | (page << 12)
    }

    /// The Souper board's map for MARIA: with its "MARIA fetch" mode on, $C000 and up shows the
    /// RAM, and with its character mode too, $8000-$9FFF shows the fixed bank and $A000-$BFFF
    /// the two character banks (A for the first 128 bytes of each 256, B for the rest).
    fn souper_maria(&self, addr: u16) -> u8 {
        let mode = self.souper[2];
        if mode & SOUPER_MFT == 0 || addr < 0x8000 || (mode & SOUPER_CHR == 0 && addr < 0xC000) {
            return self.view(addr, 0);
        }
        if addr >= 0xC000 {
            return self.ram[self.souper_ram(addr - 0x8000)];
        }
        if addr < 0xA000 {
            return self.view(addr + 0x4000, 0);
        }
        let page = self.souper[if addr & 0x80 != 0 { 1 } else { 0 }] as usize;
        let chr = (((page & 0xFE) << 4) | (page & 1)) << 7;
        self.rom.get((addr as usize & 0x0F7F) | chr).copied().unwrap_or(0)
    }

    /// A write to $4000-$FFFF. True if it went to the POKEY at $4000.
    pub fn write(&mut self, addr: u16, v: u8) -> bool {
        let off = addr as usize - 0x4000;
        let mask = self.bank_mask;
        if off < 0x4000 {
            if let Some(p) = self.pokey.as_mut() {
                // BankSet's write-only POKEY answers at $4000-$400F, or all of $4000-$7FFF when
                // its 52 KB of ROM leave no room for anything else.
                if self.pokey_reads || off < 0x10 || self.half >= 0xD000 {
                    p.write(addr as u8, v);
                    return true;
                }
            }
        }
        match self.kind {
            Kind::Rom | Kind::BankSet => {}
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
            Kind::SgRam => {
                if off < 0x4000 {
                    self.ram[off] = v;
                } else if off < 0x8000 {
                    self.bank = v & mask;
                }
            }
            Kind::Sg9 => {
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
            Kind::Versa => {
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
            Kind::Souper => {
                if off < 0x4000 {
                    let i = self.souper_ram(addr);
                    self.ram[i] = v;
                } else {
                    match addr {
                        0x8000 => self.bank = v & 31,
                        0x8001 => self.souper[0] = v,
                        0x8002 => self.souper[1] = v,
                        0x8003 => self.souper[2] = v,
                        0x8004 => self.souper[3] = v & 7,
                        0x8005 => self.souper[4] = v & 7,
                        // $8007 commands the board's own sound player (the "BupChip"), which
                        // plays music from files beside the game: not played yet.
                        _ => {}
                    }
                }
            }
            Kind::BankSetRam => {
                if off < 0x4000 {
                    self.ram[off] = v;
                } else if off >= 0x8000 {
                    // MARIA's RAM: the processor writes it through $C000-$FFFF.
                    self.ram[0x4000 + (off - 0x8000)] = v;
                }
            }
            Kind::BankSetSg | Kind::BankSetSgRam => {
                if off < 0x4000 {
                    if self.kind == Kind::BankSetSgRam {
                        self.ram[off] = v;
                    }
                } else if off < 0x8000 {
                    self.bank = (v & 0x0F) & mask;
                } else if self.kind == Kind::BankSetSgRam {
                    self.ram[0x4000 + (off - 0x8000)] = v;
                }
            }
        }
        false
    }

    /// Whether the cartridge answers at `addr` below $4000: the second POKEY ($0440-$044F,
    /// $0450-$045F, or all of $0800-$0FFF), or a 52 KB BankSet image ($3000-$3FFF).
    fn low_kind(&self, addr: u16) -> Low {
        if self.pokey2.is_some() {
            let hit = match self.pokey2_at {
                0x0800 => (0x0800..0x1000).contains(&addr),
                at => (at..at + 0x10).contains(&addr),
            };
            if hit {
                return Low::Pokey;
            }
        }
        if (0x3000..0x4000).contains(&addr) && self.kind == Kind::BankSet && self.base_rom <= addr as u32 {
            return Low::BankSet;
        }
        Low::None
    }

    /// A processor read below $4000 the cartridge answers, if it does.
    pub fn read_low(&mut self, addr: u16) -> Option<u8> {
        match self.low_kind(addr) {
            Low::Pokey => self.pokey2.as_mut().map(|p| p.read(addr as u8)),
            _ => self.peek_low(addr, false),
        }
    }

    /// The same with no side effects, for the processor's view or MARIA's.
    pub fn peek_low(&self, addr: u16, maria: bool) -> Option<u8> {
        match self.low_kind(addr) {
            Low::None => None,
            Low::Pokey => Some(0xFF),
            Low::BankSet => Some(self.side_at(maria as usize, (addr as u32 - self.base_rom) as usize)),
        }
    }

    /// A processor write below $4000: `Some(true)` if it went to a POKEY, `Some(false)` if the
    /// cartridge took it otherwise, `None` if it isn't the cartridge's.
    pub fn write_low(&mut self, addr: u16, v: u8) -> Option<bool> {
        match self.low_kind(addr) {
            Low::None => None,
            Low::Pokey => {
                if let Some(p) = self.pokey2.as_mut() {
                    p.write(addr as u8, v);
                }
                Some(true)
            }
            Low::BankSet => Some(false),
        }
    }

    /// Clocks the cartridge's POKEYs once (1.79 MHz) and returns their output levels added.
    pub fn clock_pokeys(&mut self) -> u8 {
        let mut s = 0;
        if let Some(p) = self.pokey.as_mut() {
            s += p.clock();
        }
        if let Some(p) = self.pokey2.as_mut() {
            s += p.clock();
        }
        s
    }

    pub fn has_pokey(&self) -> bool {
        self.pokey.is_some() || self.pokey2.is_some()
    }

    /// Where the POKEYs are, as the header or the list put them.
    pub fn pokeys(&self) -> Pokeys {
        Pokeys { at_4000: self.pokey.is_some(), low: self.pokey2.is_some().then_some(self.pokey2_at) }
    }

    pub fn rom(&self) -> &[u8] {
        &self.rom
    }
}

enum Low {
    None,
    Pokey,
    BankSet,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(mapper: u16, rom: &[u8]) -> Vec<u8> {
        let mut h = vec![0u8; 128];
        h[1..10].copy_from_slice(b"ATARI7800");
        h[49..53].copy_from_slice(&(rom.len() as u32).to_be_bytes());
        h[53..55].copy_from_slice(&mapper.to_be_bytes());
        h.extend_from_slice(rom);
        h
    }

    /// A ROM whose every byte tells where it is: 16 KB bank k holds k + `side` * 0x40.
    fn banks(n: usize, side: u8) -> Vec<u8> {
        (0..n).flat_map(|k| vec![k as u8 + side * 0x40; 0x4000]).collect()
    }

    #[test]
    fn the_header_bits_pick_the_board() {
        let cases: &[(u16, usize, Kind, Pokeys)] = &[
            (0x0000, 0x8000, Kind::Rom, Pokeys { at_4000: false, low: None }),
            (0x0001, 0x8000, Kind::Rom, Pokeys { at_4000: true, low: None }),
            (0x0003, 0x20000, Kind::Sg, Pokeys { at_4000: true, low: None }),
            (0x0006, 0x20000, Kind::SgRam, Pokeys::default()),
            (0x0040, 0x8000, Kind::Rom, Pokeys { at_4000: false, low: Some(0x0450) }),
            (0x8000, 0x8000, Kind::Rom, Pokeys { at_4000: false, low: Some(0x0800) }),
            (0x8002, 0x20000, Kind::Sg, Pokeys { at_4000: false, low: Some(0x0800) }),
            (0x0400, 0x8000, Kind::Rom, Pokeys { at_4000: false, low: Some(0x0440) }),
            (0x0441, 0x8000, Kind::Rom, Pokeys { at_4000: true, low: Some(0x0440) }),
            (0x1000, 0x80000, Kind::Souper, Pokeys::default()),
            (0x2000, 0x10000, Kind::BankSet, Pokeys::default()),
            (0x2001, 0x10000, Kind::BankSet, Pokeys { at_4000: true, low: None }),
            (0x6000, 0x10000, Kind::BankSetRam, Pokeys::default()),
            (0xE000, 0x10000, Kind::BankSetRam, Pokeys { at_4000: false, low: Some(0x0800) }),
            (0x2002, 0x40000, Kind::BankSetSg, Pokeys::default()),
            (0x6002, 0x40000, Kind::BankSetSgRam, Pokeys::default()),
            (0xA002, 0x40000, Kind::BankSetSg, Pokeys { at_4000: false, low: Some(0x0800) }),
            (0x0100, 0x20000, Kind::Activision, Pokeys::default()),
            (0x0200, 0x10000, Kind::Absolute, Pokeys::default()),
            (0x0080, 0x8000, Kind::Mram, Pokeys::default()),
        ];
        for &(mapper, len, kind, pokeys) in cases {
            let c = Cart::load(&header(mapper, &vec![0; len])).unwrap();
            assert_eq!((c.kind, c.pokeys()), (kind, pokeys), "mapper {mapper:04x}");
        }
    }

    #[test]
    fn bankset_gives_maria_the_second_image() {
        // 2 x 48 KB, flat: the processor's image is all $11, MARIA's all $22.
        let mut rom = vec![0x11u8; 0xC000];
        rom.extend(vec![0x22u8; 0xC000]);
        let c = Cart::load(&header(0x2000, &rom)).unwrap();
        assert_eq!((c.peek(0x4000), c.maria_peek(0x4000)), (0x11, 0x22));
        assert_eq!((c.peek(0xFFFF), c.maria_peek(0xFFFF)), (0x11, 0x22));
        // 2 x 52 KB reaches down to $3000.
        let mut rom = vec![0x33u8; 0xD000];
        rom.extend(vec![0x44u8; 0xD000]);
        let c = Cart::load(&header(0x2000, &rom)).unwrap();
        assert_eq!((c.peek_low(0x3000, false), c.peek_low(0x3000, true)), (Some(0x33), Some(0x44)));
        assert_eq!(c.peek_low(0x2FFF, false), None);
    }

    #[test]
    fn bankset_supergame_switches_both_sides_together() {
        let mut rom = banks(8, 0);
        rom.extend(banks(8, 1));
        let mut c = Cart::load(&header(0x2002, &rom)).unwrap();
        assert_eq!((c.peek(0x8000), c.maria_peek(0x8000)), (0x00, 0x40));
        assert_eq!((c.peek(0xC000), c.maria_peek(0xC000)), (0x07, 0x47));
        assert_eq!((c.peek(0x4000), c.maria_peek(0x4000)), (0x06, 0x46));
        c.write(0x8000, 3);
        assert_eq!((c.peek(0x8000), c.maria_peek(0x8000)), (0x03, 0x43));
        // With RAM: the processor's at $4000, and it writes MARIA's through $C000.
        let mut c = Cart::load(&header(0x6002, &rom)).unwrap();
        c.write(0x4000, 0x5A);
        c.write(0xC000, 0xA5);
        assert_eq!((c.peek(0x4000), c.maria_peek(0x4000)), (0x5A, 0xA5));
        assert_eq!(c.peek(0xC000), 0x07, "$C000 still reads the last bank");
    }

    #[test]
    fn souper_maps_ram_banks_and_maria_s_character_banks() {
        let rom = banks(32, 0);
        let mut c = Cart::load(&header(0x1000, &rom)).unwrap();
        assert_eq!((c.peek(0x8000), c.peek(0xC000)), (0, 31));
        c.write(0x8000, 5);
        assert_eq!(c.peek(0x8000), 5);
        // RAM pages: $6000 and $7000 pick from eight in the extended mode.
        c.write(0x6000, 0x61);
        c.write(0x8003, SOUPER_EXS);
        c.write(0x8004, 6);
        c.write(0x6000, 0x66);
        assert_eq!(c.peek(0x6000), 0x66);
        c.write(0x8004, 2);
        assert_eq!(c.peek(0x6000), 0x61, "page 2 is the normal $6000");
        // MARIA: RAM at $C000, the fixed bank at $8000, character banks at $A000.
        c.write(0x8003, SOUPER_MFT | SOUPER_CHR);
        c.write(0x4123, 0x77);
        assert_eq!(c.maria_peek(0xC123), 0x77);
        assert_eq!(c.maria_peek(0x8000), 31);
        c.write(0x8001, 16); // character bank A: ROM offset (16 << 4) << 7 = $8000, bank 2
        assert_eq!(c.maria_peek(0xA000), 2);
        c.write(0x8002, 17); // B: the same, 128 bytes on, for the second half of each 256
        assert_eq!(c.maria_peek(0xA080), 2);
        assert_eq!(c.peek(0xA000), 5, "the processor still sees its bank");
    }

    #[test]
    fn a_prosystem_description_names_the_board_and_the_rom() {
        let cdf = "ProSystem\r\nSOUPER\r\nA game\r\n\r\nroms\\game.bin\r\nCORETONE\r\nsamples.bin\r\n";
        let c = Cart::from_cdf(cdf, |p| (p == "roms/game.bin").then(|| banks(32, 0))).unwrap();
        assert_eq!((c.kind, c.info.title.as_str(), c.peek(0xC000)), (Kind::Souper, "A game", 31));
        assert!(Cart::from_cdf("ProSystem\nSUPER\nx\nmissing.bin\n", |_| None).is_err());
    }

    #[test]
    fn a_second_pokey_answers_where_the_header_says() {
        let mut c = Cart::load(&header(0x8000, &vec![0; 0x8000])).unwrap();
        assert_eq!(c.write_low(0x0F01, 0xAF), Some(true), "$0800-$0FFF, mirrored");
        assert_eq!(c.write_low(0x0450, 0xAF), None);
        let mut c = Cart::load(&header(0x0400, &vec![0; 0x8000])).unwrap();
        assert_eq!(c.write_low(0x0441, 0xAF), Some(true));
        assert_eq!(c.write_low(0x0451, 0xAF), None);
    }
}
