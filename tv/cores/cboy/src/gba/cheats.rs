//! Cheat codes on the Game Boy Advance: GameShark (v1 and v2), Action Replay (v3, which some
//! lists call Pro Action Replay or GameShark SP), CodeBreaker, and raw `AAAAAAAA:VV` codes. These
//! are the kinds mGBA (the app's other Advance emulator) reads, read the same way, so a cheat list
//! made for one works on the other.
//!
//! How a code turns into what the device does:
//!
//! - **The lines.** A GameShark or Action Replay line is two eight-digit numbers, a CodeBreaker
//!   line an eight-digit and a four-digit one, a raw line an address, a colon and one, two or
//!   four bytes. A cheat list joins lines with `+` or spaces.
//! - **Encryption.** GameShark and Action Replay lines are usually encrypted with TEA (the Tiny
//!   Encryption Algorithm) under each device's own four seed words, and a `DEADFACE` line picks
//!   new seeds from the device's two tables. Which device a list is for isn't written anywhere:
//!   the first line is decrypted both ways and read both ways unencrypted too, and whichever
//!   reading looks most like a real code (an address the console has, a value that fits the
//!   code's size) decides, for the rest of the list, as on mGBA. CodeBreaker lines are plain until
//!   a `9` line switches on its own scrambling, seeded from that line.
//! - **What runs.** Every code becomes a few steps of a small program: write a byte, a halfword
//!   or a word (once, or a run of them stepping the address and value), add to one, AND or OR
//!   one, follow a pointer, or skip the next steps unless a value in memory compares as the
//!   code says (with an "else" part on Action Replay). Codes that patch the cartridge's ROM
//!   change what the console reads from it while they are on. The program runs once a frame, at
//!   the start of the blank, or, when the list has a hook code (a "master code" naming a place in
//!   the game's own program), each time the console's processor gets there, as the device does.
//! - **Not done**, as on mGBA: the "slowdown" code, Action Replay codes that switch others off,
//!   and codes that wait for the device's own button (there is no such button here, so those
//!   never fire). Unlike mGBA, a GameShark list's game ID line writes nothing (mGBA writes
//!   $001DC0DE wherever its first word points), a GameShark "write one word to many addresses"
//!   code writes only to the addresses that follow it (mGBA also writes the word at an address
//!   equal to the word), a
//!   list of none or one address doesn't swallow the next line, a rejected Action Replay line
//!   leaves nothing half made behind, an Action Replay "if" block with no end runs to the end of
//!   the list instead of guarding nothing, a new block ends the one before it rather than also
//!   guarding the new block's own test, and a ROM patch whose value line never comes patches
//!   nothing.

use super::bus::Bus;

/// Memory as the cheat device reads and writes it: the console's (`Bus`), or a test's.
pub trait Memory {
    fn read(&self, addr: u32, width: u8) -> u32;
    fn write(&mut self, addr: u32, width: u8, v: u32);
}

impl Memory for Bus {
    fn read(&self, addr: u32, width: u8) -> u32 {
        self.cheat_read(addr, width)
    }
    fn write(&mut self, addr: u32, width: u8, v: u32) {
        self.cheat_write(addr, width, v)
    }
}

/// What one step of the program does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Assign,
    /// Writes the value at the address a word in memory points to (plus the step's offset).
    AssignIndirect,
    And,
    Add,
    Sub,
    Or,
    IfEq,
    IfNe,
    /// Signed, as the devices compare.
    IfLt,
    IfGt,
    IfLe,
    IfGe,
    IfUlt,
    IfUgt,
    /// Any bit of the value set.
    IfAnd,
    /// Both the memory and the value non-zero.
    IfLand,
    /// None of the value's bits set.
    IfNand,
    /// The device's own button: never pressed here.
    IfButton,
    /// A condition the device can't make: always false.
    Never,
}

/// One step of the program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Op {
    pub kind: Kind,
    /// 1, 2 or 4 bytes.
    pub width: u8,
    pub addr: u32,
    pub value: u32,
    /// How many times it runs (a run of writes); for a condition, how many steps after it it
    /// guards.
    pub repeat: u32,
    /// For a condition, how many steps after those make its "else" part.
    pub else_steps: u32,
    /// Added to the address and to the value after each write of a run.
    pub addr_step: u32,
    pub value_step: u32,
}

impl Op {
    fn new(kind: Kind, width: u8, addr: u32, value: u32) -> Op {
        Op { kind, width, addr, value, repeat: 1, else_steps: 0, addr_step: 0, value_step: 0 }
    }
}

/// Which device the list's GameShark-shaped lines are for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Version {
    #[default]
    Unknown,
    GameShark,
    GameSharkRaw,
    ActionReplay,
    ActionReplayRaw,
}

const GS_SEEDS: [u32; 4] = [0x09F4_FBBD, 0x9681_884A, 0x3520_27E9, 0xF3DE_E5A7];
const AR_SEEDS: [u32; 4] = [0x7AA9_648F, 0x7FAE_6994, 0xC0EF_AAD5, 0x4271_2C57];
/// The line every device's lists start with, which identifies the game.
const ID_LINE: u32 = 0x001D_C0DE;
const KEYINPUT: u32 = 0x0400_0130;

/// The cheat device: the program its codes made, the ROM bytes they patch, and the hook.
#[derive(Clone, Debug)]
pub struct Device {
    pub ops: Vec<Op>,
    /// ROM bytes changed while the codes are on: (offset into the cartridge, byte), sorted.
    pub rom_patches: Vec<(u32, u8)>,
    /// Where in the game's program the codes run, when a hook code gave one.
    pub hook: Option<u32>,
    /// The list met one of the cases this module reads differently from mGBA on purpose (see
    /// the top of the file), so the two needn't agree on it: which one. For the comparison test.
    pub unlike_mgba: Option<&'static str>,
    version: Version,
    seeds: [u32; 4],
    /// A step still waiting for the line that completes it.
    incomplete: Option<usize>,
    /// A GameShark "one word to many addresses" code's word, while its addresses are coming.
    list_value: Option<u32>,
    /// A ROM patch's address, waiting for the line with its value (Action Replay).
    patch_waiting: Option<u32>,
    /// Lines still to come for a list code.
    remaining: u32,
    /// The Action Replay "if" block still open.
    block: Option<usize>,
    cb_rng: u32,
    cb_master: u32,
    cb_table: [u8; 0x30],
    cb_seeds: [u32; 4],
}

impl Default for Device {
    fn default() -> Device {
        Device {
            ops: Vec::new(),
            rom_patches: Vec::new(),
            hook: None,
            unlike_mgba: None,
            version: Version::Unknown,
            seeds: [0; 4],
            incomplete: None,
            list_value: None,
            patch_waiting: None,
            remaining: 0,
            block: None,
            cb_rng: 0,
            cb_master: 0,
            cb_table: [0; 0x30],
            cb_seeds: [0; 4],
        }
    }
}

/// One line of a code, as typed.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Line {
    /// GameShark or Action Replay: two words.
    Pair(u32, u32),
    /// CodeBreaker: a word and a halfword.
    Short(u32, u16),
    /// `AAAAAAAA:VV..`.
    Raw(String),
}

fn hex(s: &str) -> Option<u32> {
    (!s.is_empty() && s.len() <= 8 && s.chars().all(|c| c.is_ascii_hexdigit())).then(|| u32::from_str_radix(s, 16).ok())?
}

/// Splits a code into its lines, however the list joined them.
fn lines(code: &str) -> Vec<Line> {
    let up = code.to_ascii_uppercase();
    let tokens: Vec<&str> = up.split(|c: char| c == '+' || c.is_whitespace()).filter(|t| !t.is_empty()).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        i += 1;
        if t.contains(':') {
            out.push(Line::Raw(t.to_owned()));
            continue;
        }
        if !t.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        match t.len() {
            16 => out.push(Line::Pair(hex(&t[..8]).unwrap_or(0), hex(&t[8..]).unwrap_or(0))),
            12 => out.push(Line::Short(hex(&t[..8]).unwrap_or(0), hex(&t[8..]).unwrap_or(0) as u16)),
            8 => {
                let Some(next) = tokens.get(i).filter(|n| n.chars().all(|c| c.is_ascii_hexdigit())) else { continue };
                match next.len() {
                    8 => out.push(Line::Pair(hex(t).unwrap_or(0), hex(next).unwrap_or(0))),
                    4 => out.push(Line::Short(hex(t).unwrap_or(0), hex(next).unwrap_or(0) as u16)),
                    _ => continue,
                }
                i += 1;
            }
            _ => {}
        }
    }
    out
}

/// TEA decryption: 32 rounds back from the end of the key schedule.
pub fn tea_decrypt(mut a: u32, mut b: u32, k: &[u32; 4]) -> (u32, u32) {
    let mut sum: u32 = 0xC6EF_3720;
    for _ in 0..32 {
        b = b.wrapping_sub((a << 4).wrapping_add(k[2]) ^ a.wrapping_add(sum) ^ (a >> 5).wrapping_add(k[3]));
        a = a.wrapping_sub((b << 4).wrapping_add(k[0]) ^ b.wrapping_add(sum) ^ (b >> 5).wrapping_add(k[1]));
        sum = sum.wrapping_sub(0x9E37_79B9);
    }
    (a, b)
}

/// TEA encryption, the other way (for tests, and for anyone writing codes).
pub fn tea_encrypt(mut a: u32, mut b: u32, k: &[u32; 4]) -> (u32, u32) {
    let mut sum: u32 = 0;
    for _ in 0..32 {
        sum = sum.wrapping_add(0x9E37_79B9);
        a = a.wrapping_add((b << 4).wrapping_add(k[0]) ^ b.wrapping_add(sum) ^ (b >> 5).wrapping_add(k[1]));
        b = b.wrapping_add((a << 4).wrapping_add(k[2]) ^ a.wrapping_add(sum) ^ (a >> 5).wrapping_add(k[3]));
    }
    (a, b)
}

/// New seed words from a `DEADFACE` line's low halfword and the device's two tables.
fn reseed(params: u32, t1: &[u8; 256], t2: &[u8; 256]) -> [u32; 4] {
    let (s0, s1) = ((params >> 8) & 0xFF, params & 0xFF);
    let mut seeds = [0u32; 4];
    for (y, seed) in seeds.iter_mut().enumerate() {
        for x in 0..4 {
            let z = t1[((s0 + x) & 0xFF) as usize].wrapping_add(t2[((s1 + y as u32) & 0xFF) as usize]);
            *seed = *seed << 8 | u32::from(z);
        }
    }
    seeds
}

/// How likely an address is one a code would name: the console's RAM and registers score well,
/// the BIOS and anything off the map badly.
fn address_score(addr: u32) -> i32 {
    let off = addr & 0x00FF_FFFF;
    match addr >> 24 {
        0x00 => -0x80,
        0x02 => if off > 0x4_0000 { -0x40 } else { 0x20 },
        0x03 => if off > 0x8000 { -0x40 } else { 0x20 },
        0x04 => if off > 0x400 { -0x80 } else { 0x10 },
        0x05 | 0x07 => if off > 0x400 { -0x80 } else { -0x8 },
        0x06 => if off > 0x1_8000 { -0x80 } else { -0x8 },
        0x08..=0x0D => -0x8,
        0x0E | 0x0F => if off > 0x1_0000 { -0x80 } else { -0x8 },
        _ => -0xC0,
    }
}

/// How much a pair of words looks like a real GameShark line.
fn gameshark_score(a: u32, b: u32) -> i32 {
    if b == ID_LINE {
        return 0x100;
    }
    let addr = a & 0x0FFF_FFFF;
    let less = |bad: bool| if bad { 0x10 } else { 0 };
    match a >> 28 {
        0x0 => 0x20 - less(b & 0xFFFF_FF00 != 0) + address_score(addr),
        0x1 => 0x20 - less(b & 0xFFFF_0000 != 0) + address_score(addr),
        0x2 => 0x20 + address_score(addr),
        0x6 => 0x20 - less(b & 0xCFFF_0000 != 0),
        0x8 => 0x10,
        0xD => 0x20 - less(b & 0xFFCF_0000 != 0) + address_score(addr),
        0xE => 0x20 + address_score(b) - less(a & 0x0F00_0000 != 0),
        0xF => 0x20 - less(b & 0xFFFF_0000 != 0),
        _ => -0x40,
    }
}

/// An Action Replay address: 20 bits, and the region in the next four up.
fn ar_addr(x: u32) -> u32 {
    (x & 0xF_FFFF) | ((x << 4) & 0x0F00_0000)
}

/// How much a pair of words looks like a real Action Replay line.
fn action_replay_score(a: u32, b: u32) -> i32 {
    if b == ID_LINE || (a == 0xDEAD_FACE && b & 0xFFFF_0000 == 0) {
        return 0x100;
    }
    if a == 0 {
        let mut p = 0x40;
        match b & 0xFE00_0000 {
            0x8000_0000 | 0x8200_0000 | 0x8400_0000 => p += address_score(ar_addr(b)),
            0x1800_0000 | 0x1A00_0000 | 0x1C00_0000 | 0x1E00_0000 => {}
            0x0000_0000 | 0x0800_0000 | 0x1000_0000 | 0x1200_0000 | 0x1400_0000 | 0x4000_0000 | 0x6000_0000 => {
                if b & 0x0100_0000 != 0 {
                    p -= 0x40;
                }
            }
            _ => p -= 0x40,
        }
        return p;
    }
    // A value bigger than the code's size suggests, weighed as mGBA weighs it (one size step
    // down: any value at all counts against a byte code), so lists are read alike on both.
    let bits = (a & 0x0600_0000) >> 22;
    let over = |v: u32| v & !(1u32.wrapping_shl(bits).wrapping_sub(1)) != 0;
    let mut p = 0x20;
    if a & 0x3800_0000 != 0 {
        if bits >= 24 {
            return 0;
        }
        if over(b) {
            p -= 0x10;
        }
        return p;
    }
    match a & 0xC000_0000 {
        0x8000_0000 | 0x0000_0000 | 0x4000_0000 => {
            if a & 0xC000_0000 == 0x8000_0000 && over(b) {
                p -= 0x10;
            }
            p += address_score(ar_addr(a));
            if a & 0x0100_0000 != 0 {
                return 0;
            }
        }
        _ => {}
    }
    p
}

/// A value cut to `width` bytes.
fn fit(v: u32, width: u8) -> u32 {
    if width >= 4 { v } else { v & ((1u32 << (u32::from(width) * 8)) - 1) }
}

impl Device {
    /// The device with every code switched on, in order (each may hold several lines).
    pub fn from_codes<'a>(codes: impl IntoIterator<Item = &'a str>) -> Device {
        let mut d = Device::default();
        for code in codes {
            for line in lines(code) {
                d.add(line);
            }
        }
        // An Action Replay "if" block the list never closed guards everything after it.
        if d.block.is_some() {
            d.unlike_mgba = Some("Action Replay block never closed");
            d.end_block();
        }
        // A ROM patch whose value line never came patches nothing, and a CodeBreaker list cut
        // short writes only the values it has (mGBA uses whatever its memory held).
        if d.patch_waiting.is_some() {
            d.unlike_mgba = Some("ROM patch with no value");
        }
        if d.incomplete.is_some() && d.remaining > 0 {
            d.unlike_mgba = Some("CodeBreaker list cut short");
        }
        d.rom_patches.sort_by_key(|p| p.0);
        d
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty() && self.rom_patches.is_empty()
    }

    fn push(&mut self, op: Op) -> usize {
        self.ops.push(op);
        self.ops.len() - 1
    }

    fn patch(&mut self, addr: u32, width: u8, value: u32) {
        for i in 0..u32::from(width) {
            let off = (addr & 0x01FF_FFFF) + i;
            let byte = (value >> (8 * i)) as u8;
            match self.rom_patches.iter_mut().find(|p| p.0 == off) {
                Some(p) => p.1 = byte,
                None => self.rom_patches.push((off, byte)),
            }
        }
    }

    fn add(&mut self, line: Line) -> bool {
        match line {
            Line::Pair(a, b) => self.add_pair(a, b),
            Line::Short(a, b) => self.add_codebreaker(a, b),
            Line::Raw(text) => self.add_raw(&text),
        }
    }

    fn set_version(&mut self, v: Version) {
        self.version = v;
        self.seeds = match v {
            Version::GameShark | Version::GameSharkRaw => GS_SEEDS,
            Version::ActionReplay | Version::ActionReplayRaw => AR_SEEDS,
            Version::Unknown => self.seeds,
        };
    }

    fn add_pair(&mut self, a: u32, b: u32) -> bool {
        if self.version == Version::Unknown {
            let mut best = (i32::MIN, Version::GameShark);
            let (ga, gb) = tea_decrypt(a, b, &GS_SEEDS);
            let (pa, pb) = tea_decrypt(a, b, &AR_SEEDS);
            for (score, v) in [
                (gameshark_score(ga, gb), Version::GameShark),
                (action_replay_score(pa, pb), Version::ActionReplay),
                (gameshark_score(a, b), Version::GameSharkRaw),
                (action_replay_score(a, b), Version::ActionReplayRaw),
            ] {
                if score > best.0 {
                    best = (score, v);
                }
            }
            self.set_version(best.1);
        }
        match self.version {
            Version::GameShark => {
                let (a, b) = tea_decrypt(a, b, &self.seeds);
                self.add_gameshark(a, b)
            }
            Version::GameSharkRaw => self.add_gameshark(a, b),
            Version::ActionReplay => {
                let (a, b) = tea_decrypt(a, b, &self.seeds);
                self.add_action_replay(a, b)
            }
            _ => self.add_action_replay(a, b),
        }
    }

    /// A decrypted GameShark line.
    fn add_gameshark(&mut self, a: u32, b: u32) -> bool {
        if self.incomplete.is_some() {
            self.unlike_mgba = Some("GameShark line after an unfinished code");
        }
        if let Some(value) = self.list_value {
            // More addresses for a "one word to many addresses" code, two to a line.
            for addr in [a, b] {
                if self.remaining > 0 {
                    self.push(Op::new(Kind::Assign, 4, addr, value));
                    self.remaining -= 1;
                }
            }
            if self.remaining == 0 {
                self.list_value = None;
            }
            return true;
        }
        if b == ID_LINE {
            // The game's ID line: the device checks it, nothing is written (mGBA reads it as a
            // write of $001DC0DE somewhere, by whatever its first word happens to say).
            self.unlike_mgba = Some("GameShark ID line");
            return true;
        }
        let addr = a & 0x0FFF_FFFF;
        match a >> 28 {
            0x0 => self.push(Op::new(Kind::Assign, 1, addr, b)),
            0x1 => self.push(Op::new(Kind::Assign, 2, addr, b)),
            0x2 => self.push(Op::new(Kind::Assign, 4, addr, b)),
            0x3 => {
                // `3000CCCC VVVVVVVV`: the word written at each of the CCCC addresses that follow.
                self.unlike_mgba = Some("GameShark word list");
                let count = a & 0xFFFF;
                if count > 0 {
                    self.list_value = Some(b);
                    self.remaining = count;
                }
                return true;
            }
            0x6 => {
                self.patch(0x0800_0000 | ((a & 0xFF_FFFF) << 1), 2, b);
                return true;
            }
            0x8 => match a & 0x00F0_0000 {
                0x0010_0000 | 0x0020_0000 => {
                    let width = if a & 0x00F0_0000 == 0x0010_0000 { 1 } else { 2 };
                    self.push(Op::new(Kind::IfButton, 0, 0, 0));
                    self.push(Op::new(Kind::Assign, width, a & 0x0F0F_FFFF, b))
                }
                _ => return false,
            },
            0xD => {
                if a == 0xDEAD_FACE {
                    self.seeds = reseed(b, &GS_T1, &GS_T2);
                    return true;
                }
                let kind = match b >> 20 {
                    0 => Kind::IfEq,
                    1 => Kind::IfNe,
                    2 => Kind::IfLe,
                    3 => Kind::IfGe,
                    _ => {
                        self.unlike_mgba = Some("GameShark test of no kind");
                        Kind::Never
                    }
                };
                self.push(Op::new(kind, 2, addr, b & 0xFFFF))
            }
            0xE => {
                let op = Op { repeat: (a >> 16) & 0xFF, ..Op::new(Kind::IfEq, 2, b & 0x0FFF_FFFF, a & 0xFFFF) };
                self.push(op)
            }
            0xF => {
                if self.hook.is_some() {
                    return false;
                }
                self.hook = Some(0x0800_0000 | (a & 0x01FF_FFFF));
                return true;
            }
            _ => return false,
        };
        true
    }

    /// A decrypted Action Replay line.
    fn add_action_replay(&mut self, a: u32, b: u32) -> bool {
        if let Some(addr) = self.patch_waiting.take() {
            self.patch(addr, 2, a);
            return true;
        }
        if let Some(i) = self.incomplete.take() {
            let width = self.ops[i].width;
            self.ops[i].value = fit(a, width);
            if i > 0 && self.ops[i - 1].kind == Kind::IfButton {
                return true;
            }
            // A run: the value's step, how many, and the address's step in units of the size.
            self.ops[i].value_step = b >> 24;
            self.ops[i].repeat = (b >> 16) & 0xFF;
            self.ops[i].addr_step = (b & 0xFFFF) * u32::from(width);
            return true;
        }
        if b == ID_LINE {
            return true;
        }
        match a {
            0 => return self.add_action_replay_special(b),
            0xDEAD_FACE => {
                self.seeds = reseed(b, &AR_T1, &AR_T2);
                return true;
            }
            _ => {}
        }
        if a >> 24 == 0xC4 {
            if self.hook.is_some() {
                return false;
            }
            self.hook = Some(0x0800_0000 | (a & 0x01FF_FFFE));
            return true;
        }
        if a & 0x3800_0000 != 0 {
            return self.add_action_replay_if(a, b);
        }
        let mut width = 1u8 << ((a & 0x0600_0000) >> 25);
        let register = a & 0xC000_0000 == 0xC000_0000;
        if a & 0x0100_0000 != 0 && a & 0xFE00_0000 != 0xC600_0000 {
            self.unlike_mgba = Some("Action Replay line refused");
            return false;
        }
        if width > 4 && !register {
            // Eight bytes: a size the device has no write for. It still takes its place in the
            // list (a condition before it guards it), and does nothing.
            self.push(Op::new(Kind::Assign, 0, ar_addr(a), 0));
            return true;
        }
        let mut op = Op::new(Kind::Assign, width, ar_addr(a), 0);
        match a & 0xC000_0000 {
            0x0000_0000 => {
                op.addr_step = u32::from(width);
                if width < 4 {
                    op.repeat = (b >> (u32::from(width) * 8)) + 1;
                }
            }
            0x4000_0000 => {
                op.kind = Kind::AssignIndirect;
                if width < 4 {
                    op.addr_step = (b >> (u32::from(width) * 8)) * u32::from(width);
                }
            }
            0x8000_0000 => op.kind = Kind::Add,
            _ => {
                // A write to a register: one byte or a halfword.
                width = ((a >> 24) as u8 & 1) + 1;
                op.addr = 0x0400_0000 | (a & 0x00FF_FFFF);
            }
        }
        op.width = width;
        op.value = fit(b, width);
        self.push(op);
        true
    }

    fn add_action_replay_if(&mut self, a: u32, b: u32) -> bool {
        if a & 0xC000_0000 == 0xC000_0000 {
            // "Switch other codes off": not done (as on mGBA).
            return false;
        }
        let width = 1u8 << ((a & 0x0600_0000) >> 25);
        let kind = match a & 0x3800_0000 {
            0x0800_0000 => Kind::IfEq,
            0x1000_0000 => Kind::IfNe,
            0x1800_0000 => Kind::IfLt,
            0x2000_0000 => Kind::IfGt,
            0x2800_0000 => Kind::IfUlt,
            0x3000_0000 => Kind::IfUgt,
            _ => Kind::IfAnd,
        };
        let mut op = if width > 4 { Op::new(Kind::Never, 0, ar_addr(a), 0) } else { Op::new(kind, width, ar_addr(a), fit(b, width)) };
        match a & 0xC000_0000 {
            0x0000_0000 => op.repeat = 1,
            0x4000_0000 => op.repeat = 2,
            _ => {
                // A block, up to the ENDIF (or ELSE).
                op.repeat = 0;
                if self.block.is_some() {
                    self.unlike_mgba = Some("Action Replay block inside a block");
                    self.end_block();
                }
                self.block = Some(self.ops.len());
            }
        }
        self.push(op);
        true
    }

    fn end_block(&mut self) {
        let Some(at) = self.block.take() else { return };
        let size = (self.ops.len() - at - 1) as u32;
        let op = &mut self.ops[at];
        if op.repeat != 0 {
            op.else_steps = size - op.repeat;
        } else {
            op.repeat = size;
        }
    }

    fn add_action_replay_special(&mut self, b: u32) -> bool {
        let width_of = |b: u32| match b & 0xFF00_0000 {
            0x1000_0000 | 0x8000_0000 => 1,
            0x1200_0000 | 0x8200_0000 => 2,
            _ => 4,
        };
        match b & 0xFF00_0000 {
            0x0800_0000 => false,
            0x1000_0000 | 0x1200_0000 | 0x1400_0000 => {
                self.push(Op::new(Kind::IfButton, 0, 0, 0));
                self.incomplete = Some(self.push(Op::new(Kind::Assign, width_of(b), ar_addr(b), 0)));
                true
            }
            0x1800_0000 | 0x1A00_0000 | 0x1C00_0000 | 0x1E00_0000 => {
                self.patch_waiting = Some(0x0800_0000 | ((b & 0xFF_FFFF) << 1));
                true
            }
            0x4000_0000 => {
                if self.block.is_none() {
                    return false;
                }
                self.end_block();
                true
            }
            0x6000_0000 => {
                let Some(at) = self.block else { return false };
                self.ops[at].repeat = (self.ops.len() - at - 1) as u32;
                true
            }
            0x8000_0000 | 0x8200_0000 | 0x8400_0000 => {
                // A fill: the value and how many come on the next line; until then it does nothing.
                let fill = Op { repeat: 0, ..Op::new(Kind::Assign, width_of(b), ar_addr(b), 0) };
                self.incomplete = Some(self.push(fill));
                true
            }
            // The end of a list, and anything else here: nothing to do.
            _ => true,
        }
    }

    fn add_codebreaker(&mut self, mut a: u32, mut b: u16) -> bool {
        if self.list_value.is_some() {
            self.unlike_mgba = Some("CodeBreaker line inside a GameShark list");
        }
        if self.cb_master != 0 {
            (a, b) = self.cb_decrypt(a, b);
        }
        if let Some(i) = self.incomplete {
            if self.remaining > 0 {
                // A list of halfwords, three to a line, byte-swapped. Each write waits for its
                // value, so a list cut short writes only what it has.
                self.ops[i].value = u32::from(((a >> 16) as u16).swap_bytes());
                self.ops[i].repeat = 1;
                self.remaining -= 1;
                let mut last = i;
                for half in [a as u16, b] {
                    if self.remaining > 0 {
                        let addr = self.ops[last].addr.wrapping_add(2);
                        last = self.push(Op::new(Kind::Assign, 2, addr, u32::from(half.swap_bytes())));
                        self.remaining -= 1;
                    }
                }
                if self.remaining == 0 {
                    self.incomplete = None;
                } else {
                    let addr = self.ops[last].addr.wrapping_add(2);
                    self.incomplete = Some(self.push(Op { repeat: 0, ..Op::new(Kind::Assign, 2, addr, 0) }));
                }
            } else {
                // A fill: how many, the address's step and the value's.
                let op = &mut self.ops[i];
                op.repeat = a & 0xFFFF;
                op.addr_step = u32::from(b);
                op.value_step = a >> 16;
                self.incomplete = None;
            }
            return true;
        }
        let addr = a & 0x0FFF_FFFF;
        let v = u32::from(b);
        let kind = match a >> 28 {
            0x0 => return true,
            0x1 => {
                if self.hook.is_some() {
                    return false;
                }
                self.hook = Some(0x0800_0000 | (a & 0x01FF_FFFF));
                return true;
            }
            0x2 => Kind::Or,
            0x3 => {
                self.push(Op::new(Kind::Assign, 1, addr, v));
                return true;
            }
            0x4 => {
                self.incomplete = Some(self.push(Op::new(Kind::Assign, 2, addr, v)));
                return true;
            }
            0x5 => {
                // A list of halfwords: how many here, the halfwords on the lines that follow. A
                // list of none writes nothing (mGBA writes the count itself, once).
                self.incomplete = Some(self.push(Op { repeat: 0, ..Op::new(Kind::Assign, 2, addr, v) }));
                self.remaining = v;
                if v == 0 {
                    self.unlike_mgba = Some("CodeBreaker list of none");
                }
                return true;
            }
            0x6 => Kind::And,
            0x7 => Kind::IfEq,
            0x8 => Kind::Assign,
            0x9 => {
                self.cb_reseed(a, b);
                return true;
            }
            0xA => Kind::IfNe,
            0xB => Kind::IfGt,
            0xC => Kind::IfLt,
            0xD => {
                if a & 0x0FFF_FFFF != 0x20 {
                    return false;
                }
                self.push(Op::new(Kind::IfNand, 2, KEYINPUT, v));
                return true;
            }
            0xE => Kind::Add,
            _ => Kind::IfAnd,
        };
        self.push(Op::new(kind, 2, addr, v));
        true
    }

    /// The CodeBreaker's random numbers: three steps of its generator, mixed.
    fn cb_rand(&mut self) -> u32 {
        let step = |s: u32| s.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        let r1 = step(self.cb_rng);
        let r2 = step(r1);
        let r3 = step(r2);
        self.cb_rng = r3;
        (r1 << 14) & 0xC000_0000 | (r2 >> 1) & 0x3FFF_8000 | (r3 >> 16) & 0x7FFF
    }

    /// A table index from the generator, as the CodeBreaker's own division arrives at it.
    fn cb_swap_index(&mut self) -> usize {
        let mut roll = self.cb_rand();
        let mut count: u32 = 0x30;
        if roll == count {
            roll = 0;
        }
        if roll < count {
            return roll as usize;
        }
        let mut bit: u32 = 1;
        while count < 0x1000_0000 && count < roll {
            count <<= 4;
            bit <<= 4;
        }
        while count < 0x8000_0000 && count < roll {
            count <<= 1;
            bit <<= 1;
        }
        let mut mask;
        loop {
            mask = 0;
            if roll >= count {
                roll -= count;
            }
            if roll >= count >> 1 {
                roll -= count >> 1;
                mask |= bit.rotate_right(1);
            }
            if roll >= count >> 2 {
                roll -= count >> 2;
                mask |= bit.rotate_right(2);
            }
            if roll >= count >> 3 {
                roll -= count >> 3;
                mask |= bit.rotate_right(3);
            }
            if roll == 0 || bit >> 4 == 0 {
                break;
            }
            bit >>= 4;
            count >>= 4;
        }
        mask &= 0xE000_0000;
        if mask != 0 && bit & 7 != 0 {
            if mask & bit.rotate_right(3) != 0 {
                roll = roll.wrapping_add(count >> 3);
            }
            if mask & bit.rotate_right(2) != 0 {
                roll = roll.wrapping_add(count >> 2);
            }
            if mask & bit.rotate_right(1) != 0 {
                roll = roll.wrapping_add(count >> 1);
            }
        }
        roll as usize % 0x30
    }

    /// A `9` line: the CodeBreaker's scrambling from here on, seeded from it.
    fn cb_reseed(&mut self, a: u32, b: u16) {
        self.cb_rng = u32::from(b & 0xFF) ^ 0x1111;
        for (i, t) in self.cb_table.iter_mut().enumerate() {
            *t = i as u8;
        }
        for _ in 0..0x50 {
            let x = self.cb_swap_index();
            let y = self.cb_swap_index();
            self.cb_table.swap(x, y);
        }
        self.cb_rng = 0x4EFA_D1C3;
        for _ in 0..((a >> 24) & 0xF) {
            self.cb_rng = self.cb_rand();
        }
        self.cb_seeds[2] = self.cb_rand();
        self.cb_seeds[3] = self.cb_rand();
        self.cb_rng = u32::from(b >> 8) ^ 0xF254;
        for _ in 0..(b >> 8) {
            self.cb_rng = self.cb_rand();
        }
        self.cb_seeds[0] = self.cb_rand();
        self.cb_seeds[1] = self.cb_rand();
        self.cb_master = a;
    }

    fn cb_decrypt(&self, a: u32, b: u16) -> (u32, u16) {
        let load = |a: u32, b: u16| -> [u8; 6] {
            let x = a.to_be_bytes();
            let y = b.to_be_bytes();
            [x[0], x[1], x[2], x[3], y[0], y[1]]
        };
        let store = |buf: &[u8; 6]| (u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]), u16::from_be_bytes([buf[4], buf[5]]));
        let mut buf = load(a, b);
        for i in (0..0x30).rev() {
            let (ox, bx) = (i >> 3, i & 7);
            let t = usize::from(self.cb_table[i]);
            let (oy, by) = (t >> 3, t & 7);
            let x = (buf[ox] >> bx) & 1;
            let y = (buf[oy] >> by) & 1;
            buf[ox] = (buf[ox] & !(1 << bx)) | (y << bx);
            buf[oy] = (buf[oy] & !(1 << by)) | (x << by);
        }
        let (mut a, mut b) = store(&buf);
        a ^= self.cb_seeds[0];
        b ^= self.cb_seeds[1] as u16;
        let mut buf = load(a, b);
        let m = self.cb_master;
        for i in 0..5 {
            buf[i] ^= (m >> 8) as u8 ^ buf[i + 1];
        }
        buf[5] ^= (m >> 8) as u8;
        for i in (1..6).rev() {
            buf[i] ^= m as u8 ^ buf[i - 1];
        }
        buf[0] ^= m as u8;
        let (mut a, mut b) = store(&buf);
        a ^= self.cb_seeds[2];
        b ^= self.cb_seeds[3] as u16;
        (a, b)
    }

    /// `AAAAAAAA:VV`, `:VVVV` or `:VVVVVVVV`: a write, or a ROM patch for a cartridge address.
    fn add_raw(&mut self, text: &str) -> bool {
        let Some((a, v)) = text.split_once(':') else { return false };
        if a.len() != 8 || v.len() % 2 != 0 {
            return false;
        }
        let (Some(addr), Some(value)) = (hex(a), hex(v)) else { return false };
        let width = (v.len() / 2) as u8;
        if !matches!(width, 1 | 2 | 4) {
            return false;
        }
        if (0x0800_0000..0x0E00_0000).contains(&addr) {
            self.patch(addr, width, value);
        } else {
            self.push(Op::new(Kind::Assign, width, addr, value));
        }
        true
    }

    /// Runs the program once against the console's memory.
    pub fn run(&self, bus: &mut impl Memory) {
        let n = self.ops.len();
        let mut else_at: Option<usize> = None;
        let mut end_at = 0usize;
        let mut i = 0usize;
        while i < n {
            let op = self.ops[i];
            let (mut addr, mut value) = (op.addr, op.value);
            let mut left = op.repeat;
            let mut condition = true;
            let (mut guarded, mut else_steps) = (0u32, 0u32);
            while left > 0 {
                let read = |bus: &dyn Memory, a: u32| if op.width == 0 { 0 } else { bus.read(a, op.width) };
                let mut write = None;
                let mut test = |c: bool| {
                    condition = c;
                    guarded = op.repeat;
                    else_steps = op.else_steps;
                    left = 1;
                };
                match op.kind {
                    Kind::Assign => write = Some(value),
                    Kind::AssignIndirect => {
                        addr = bus.read(addr, 4).wrapping_add(op.addr_step);
                        write = Some(value);
                    }
                    Kind::And => write = Some(read(bus, addr) & value),
                    Kind::Add => write = Some(read(bus, addr).wrapping_add(value)),
                    Kind::Sub => write = Some(read(bus, addr).wrapping_sub(value)),
                    Kind::Or => write = Some(read(bus, addr) | value),
                    Kind::IfEq => test(read(bus, addr) == value),
                    Kind::IfNe => test(read(bus, addr) != value),
                    Kind::IfLt => test((read(bus, addr) as i32) < value as i32),
                    Kind::IfGt => test(read(bus, addr) as i32 > value as i32),
                    Kind::IfLe => test(read(bus, addr) as i32 <= value as i32),
                    Kind::IfGe => test(read(bus, addr) as i32 >= value as i32),
                    Kind::IfUlt => test(read(bus, addr) < value),
                    Kind::IfUgt => test(read(bus, addr) > value),
                    Kind::IfAnd => test(read(bus, addr) & value != 0),
                    Kind::IfLand => test(read(bus, addr) != 0 && value != 0),
                    Kind::IfNand => test(read(bus, addr) & value == 0),
                    Kind::IfButton | Kind::Never => test(false),
                }
                if let (Some(v), true) = (write, op.width > 0) {
                    bus.write(addr, op.width, v);
                }
                addr = addr.wrapping_add(op.addr_step);
                value = value.wrapping_add(op.value_step);
                left -= 1;
            }
            if else_at == Some(i) {
                i = end_at;
                else_at = None;
            }
            if guarded > 0 && !condition {
                i += guarded as usize;
            } else if else_steps > 0 {
                else_at = Some(i + guarded as usize);
                end_at = i + (guarded + else_steps) as usize;
            }
            i += 1;
        }
    }
}


// The devices' own tables for picking new seed words (a `DEADFACE` line): GameShark v1/v2
// and Action Replay v3. Data of the devices, as mGBA has them.

const GS_T1: [u8; 256] = [
    0x31, 0x1C, 0x23, 0xE5, 0x89, 0x8E, 0xA1, 0x37, 0x74, 0x6D, 0x67, 0xFC, 0x1F, 0xC0, 0xB1, 0x94,
    0x3B, 0x05, 0x56, 0x86, 0x00, 0x24, 0xF0, 0x17, 0x72, 0xA2, 0x3D, 0x1B, 0xE3, 0x17, 0xC5, 0x0B,
    0xB9, 0xE2, 0xBD, 0x58, 0x71, 0x1B, 0x2C, 0xFF, 0xE4, 0xC9, 0x4C, 0x5E, 0xC9, 0x55, 0x33, 0x45,
    0x7C, 0x3F, 0xB2, 0x51, 0xFE, 0x10, 0x7E, 0x75, 0x3C, 0x90, 0x8D, 0xDA, 0x94, 0x38, 0xC3, 0xE9,
    0x95, 0xEA, 0xCE, 0xA6, 0x06, 0xE0, 0x4F, 0x3F, 0x2A, 0xE3, 0x3A, 0xE4, 0x43, 0xBD, 0x7F, 0xDA,
    0x55, 0xF0, 0xEA, 0xCB, 0x2C, 0xA8, 0x47, 0x61, 0xA0, 0xEF, 0xCB, 0x13, 0x18, 0x20, 0xAF, 0x3E,
    0x4D, 0x9E, 0x1E, 0x77, 0x51, 0xC5, 0x51, 0x20, 0xCF, 0x21, 0xF9, 0x39, 0x94, 0xDE, 0xDD, 0x79,
    0x4E, 0x80, 0xC4, 0x9D, 0x94, 0xD5, 0x95, 0x01, 0x27, 0x27, 0xBD, 0x6D, 0x78, 0xB5, 0xD1, 0x31,
    0x6A, 0x65, 0x74, 0x74, 0x58, 0xB3, 0x7C, 0xC9, 0x5A, 0xED, 0x50, 0x03, 0xC4, 0xA2, 0x94, 0x4B,
    0xF0, 0x58, 0x09, 0x6F, 0x3E, 0x7D, 0xAE, 0x7D, 0x58, 0xA0, 0x2C, 0x91, 0xBB, 0xE1, 0x70, 0xEB,
    0x73, 0xA6, 0x9A, 0x44, 0x25, 0x90, 0x16, 0x62, 0x53, 0xAE, 0x08, 0xEB, 0xDC, 0xF0, 0xEE, 0x77,
    0xC2, 0xDE, 0x81, 0xE8, 0x30, 0x89, 0xDB, 0xFE, 0xBC, 0xC2, 0xDF, 0x26, 0xE9, 0x8B, 0xD6, 0x93,
    0xF0, 0xCB, 0x56, 0x90, 0xC0, 0x46, 0x68, 0x15, 0x43, 0xCB, 0xE9, 0x98, 0xE3, 0xAF, 0x31, 0x25,
    0x4D, 0x7B, 0xF3, 0xB1, 0x74, 0xE2, 0x64, 0xAC, 0xD9, 0xF6, 0xA0, 0xD5, 0x0B, 0x9B, 0x49, 0x52,
    0x69, 0x3B, 0x71, 0x00, 0x2F, 0xBB, 0xBA, 0x08, 0xB1, 0xAE, 0xBB, 0xB3, 0xE1, 0xC9, 0xA6, 0x7F,
    0x17, 0x97, 0x28, 0x72, 0x12, 0x6E, 0x91, 0xAE, 0x3A, 0xA2, 0x35, 0x46, 0x27, 0xF8, 0x12, 0x50,
];

const GS_T2: [u8; 256] = [
    0xD8, 0x65, 0x04, 0xC2, 0x65, 0xD5, 0xB0, 0x0C, 0xDF, 0x9D, 0xF0, 0xC3, 0x9A, 0x17, 0xC9, 0xA6,
    0xE1, 0xAC, 0x0D, 0x14, 0x2F, 0x3C, 0x2C, 0x87, 0xA2, 0xBF, 0x4D, 0x5F, 0xAC, 0x2D, 0x9D, 0xE1,
    0x0C, 0x9C, 0xE7, 0x7F, 0xFC, 0xA8, 0x66, 0x59, 0xAC, 0x18, 0xD7, 0x05, 0xF0, 0xBF, 0xD1, 0x8B,
    0x35, 0x9F, 0x59, 0xB4, 0xBA, 0x55, 0xB2, 0x85, 0xFD, 0xB1, 0x72, 0x06, 0x73, 0xA4, 0xDB, 0x48,
    0x7B, 0x5F, 0x67, 0xA5, 0x95, 0xB9, 0xA5, 0x4A, 0xCF, 0xD1, 0x44, 0xF3, 0x81, 0xF5, 0x6D, 0xF6,
    0x3A, 0xC3, 0x57, 0x83, 0xFA, 0x8E, 0x15, 0x2A, 0xA2, 0x04, 0xB2, 0x9D, 0xA8, 0x0D, 0x7F, 0xB8,
    0x0F, 0xF6, 0xAC, 0xBE, 0x97, 0xCE, 0x16, 0xE6, 0x31, 0x10, 0x60, 0x16, 0xB5, 0x83, 0x45, 0xEE,
    0xD7, 0x5F, 0x2C, 0x08, 0x58, 0xB1, 0xFD, 0x7E, 0x79, 0x00, 0x34, 0xAD, 0xB5, 0x31, 0x34, 0x39,
    0xAF, 0xA8, 0xDD, 0x52, 0x6A, 0xB0, 0x60, 0x35, 0xB8, 0x1D, 0x52, 0xF5, 0xF5, 0x30, 0x00, 0x7B,
    0xF4, 0xBA, 0x03, 0xCB, 0x3A, 0x84, 0x14, 0x8A, 0x6A, 0xEF, 0x21, 0xBD, 0x01, 0xD8, 0xA0, 0xD4,
    0x43, 0xBE, 0x23, 0xE7, 0x76, 0x27, 0x2C, 0x3F, 0x4D, 0x3F, 0x43, 0x18, 0xA7, 0xC3, 0x47, 0xA5,
    0x7A, 0x1D, 0x02, 0x55, 0x09, 0xD1, 0xFF, 0x55, 0x5E, 0x17, 0xA0, 0x56, 0xF4, 0xC9, 0x6B, 0x90,
    0xB4, 0x80, 0xA5, 0x07, 0x22, 0xFB, 0x22, 0x0D, 0xD9, 0xC0, 0x5B, 0x08, 0x35, 0x05, 0xC1, 0x75,
    0x4F, 0xD0, 0x51, 0x2D, 0x2E, 0x5E, 0x69, 0xE7, 0x3B, 0xC2, 0xDA, 0xFF, 0xF6, 0xCE, 0x3E, 0x76,
    0xE8, 0x36, 0x8C, 0x39, 0xD8, 0xF3, 0xE9, 0xA6, 0x42, 0xE6, 0xC1, 0x4C, 0x05, 0xBE, 0x17, 0xF2,
    0x5C, 0x1B, 0x19, 0xDB, 0x0F, 0xF3, 0xF8, 0x49, 0xEB, 0x36, 0xF6, 0x40, 0x6F, 0xAD, 0xC1, 0x8C,
];

const AR_T1: [u8; 256] = [
    0xD0, 0xFF, 0xBA, 0xE5, 0xC1, 0xC7, 0xDB, 0x5B, 0x16, 0xE3, 0x6E, 0x26, 0x62, 0x31, 0x2E, 0x2A,
    0xD1, 0xBB, 0x4A, 0xE6, 0xAE, 0x2F, 0x0A, 0x90, 0x29, 0x90, 0xB6, 0x67, 0x58, 0x2A, 0xB4, 0x45,
    0x7B, 0xCB, 0xF0, 0x73, 0x84, 0x30, 0x81, 0xC2, 0xD7, 0xBE, 0x89, 0xD7, 0x4E, 0x73, 0x5C, 0xC7,
    0x80, 0x1B, 0xE5, 0xE4, 0x43, 0xC7, 0x46, 0xD6, 0x6F, 0x7B, 0xBF, 0xED, 0xE5, 0x27, 0xD1, 0xB5,
    0xD0, 0xD8, 0xA3, 0xCB, 0x2B, 0x30, 0xA4, 0xF0, 0x84, 0x14, 0x72, 0x5C, 0xFF, 0xA4, 0xFB, 0x54,
    0x9D, 0x70, 0xE2, 0xFF, 0xBE, 0xE8, 0x24, 0x76, 0xE5, 0x15, 0xFB, 0x1A, 0xBC, 0x87, 0x02, 0x2A,
    0x58, 0x8F, 0x9A, 0x95, 0xBD, 0xAE, 0x8D, 0x0C, 0xA5, 0x4C, 0xF2, 0x5C, 0x7D, 0xAD, 0x51, 0xFB,
    0xB1, 0x22, 0x07, 0xE0, 0x29, 0x7C, 0xEB, 0x98, 0x14, 0xC6, 0x31, 0x97, 0xE4, 0x34, 0x8F, 0xCC,
    0x99, 0x56, 0x9F, 0x78, 0x43, 0x91, 0x85, 0x3F, 0xC2, 0xD0, 0xD1, 0x80, 0xD1, 0x77, 0xA7, 0xE2,
    0x43, 0x99, 0x1D, 0x2F, 0x8B, 0x6A, 0xE4, 0x66, 0x82, 0xF7, 0x2B, 0x0B, 0x65, 0x14, 0xC0, 0xC2,
    0x1D, 0x96, 0x78, 0x1C, 0xC4, 0xC3, 0xD2, 0xB1, 0x64, 0x07, 0xD7, 0x6F, 0x02, 0xE9, 0x44, 0x31,
    0xDB, 0x3C, 0xEB, 0x93, 0xED, 0x9A, 0x57, 0x05, 0xB9, 0x0E, 0xAF, 0x1F, 0x48, 0x11, 0xDC, 0x35,
    0x6C, 0xB8, 0xEE, 0x2A, 0x48, 0x2B, 0xBC, 0x89, 0x12, 0x59, 0xCB, 0xD1, 0x18, 0xEA, 0x72, 0x11,
    0x01, 0x75, 0x3B, 0xB5, 0x56, 0xF4, 0x8B, 0xA0, 0x41, 0x75, 0x86, 0x7B, 0x94, 0x12, 0x2D, 0x4C,
    0x0C, 0x22, 0xC9, 0x4A, 0xD8, 0xB1, 0x8D, 0xF0, 0x55, 0x2E, 0x77, 0x50, 0x1C, 0x64, 0x77, 0xAA,
    0x3E, 0xAC, 0xD3, 0x3D, 0xCE, 0x60, 0xCA, 0x5D, 0xA0, 0x92, 0x78, 0xC6, 0x51, 0xFE, 0xF9, 0x30,
];

const AR_T2: [u8; 256] = [
    0xAA, 0xAF, 0xF0, 0x72, 0x90, 0xF7, 0x71, 0x27, 0x06, 0x11, 0xEB, 0x9C, 0x37, 0x12, 0x72, 0xAA,
    0x65, 0xBC, 0x0D, 0x4A, 0x76, 0xF6, 0x5C, 0xAA, 0xB0, 0x7A, 0x7D, 0x81, 0xC1, 0xCE, 0x2F, 0x9F,
    0x02, 0x75, 0x38, 0xC8, 0xFC, 0x66, 0x05, 0xC2, 0x2C, 0xBD, 0x91, 0xAD, 0x03, 0xB1, 0x88, 0x93,
    0x31, 0xC6, 0xAB, 0x40, 0x23, 0x43, 0x76, 0x54, 0xCA, 0xE7, 0x00, 0x96, 0x9F, 0xD8, 0x24, 0x8B,
    0xE4, 0xDC, 0xDE, 0x48, 0x2C, 0xCB, 0xF7, 0x84, 0x1D, 0x45, 0xE5, 0xF1, 0x75, 0xA0, 0xED, 0xCD,
    0x4B, 0x24, 0x8A, 0xB3, 0x98, 0x7B, 0x12, 0xB8, 0xF5, 0x63, 0x97, 0xB3, 0xA6, 0xA6, 0x0B, 0xDC,
    0xD8, 0x4C, 0xA8, 0x99, 0x27, 0x0F, 0x8F, 0x94, 0x63, 0x0F, 0xB0, 0x11, 0x94, 0xC7, 0xE9, 0x7F,
    0x3B, 0x40, 0x72, 0x4C, 0xDB, 0x84, 0x78, 0xFE, 0xB8, 0x56, 0x08, 0x80, 0xDF, 0x20, 0x2F, 0xB9,
    0x66, 0x2D, 0x60, 0x63, 0xF5, 0x18, 0x15, 0x1B, 0x86, 0x85, 0xB9, 0xB4, 0x68, 0x0E, 0xC6, 0xD1,
    0x8A, 0x81, 0x2B, 0xB3, 0xF6, 0x48, 0xF0, 0x4F, 0x9C, 0x28, 0x1C, 0xA4, 0x51, 0x2F, 0xD7, 0x4B,
    0x17, 0xE7, 0xCC, 0x50, 0x9F, 0xD0, 0xD1, 0x40, 0x0C, 0x0D, 0xCA, 0x83, 0xFA, 0x5E, 0xCA, 0xEC,
    0xBF, 0x4E, 0x7C, 0x8F, 0xF0, 0xAE, 0xC2, 0xD3, 0x28, 0x41, 0x9B, 0xC8, 0x04, 0xB9, 0x4A, 0xBA,
    0x72, 0xE2, 0xB5, 0x06, 0x2C, 0x1E, 0x0B, 0x2C, 0x7F, 0x11, 0xA9, 0x26, 0x51, 0x9D, 0x3F, 0xF8,
    0x62, 0x11, 0x2E, 0x89, 0xD2, 0x9D, 0x35, 0xB1, 0xE4, 0x0A, 0x4D, 0x93, 0x01, 0xA7, 0xD1, 0x2D,
    0x00, 0x87, 0xE2, 0x2D, 0xA4, 0xE9, 0x0A, 0x06, 0x66, 0xF8, 0x1F, 0x44, 0x75, 0xB5, 0x6B, 0x1C,
    0xFC, 0x31, 0x09, 0x48, 0xA3, 0xFF, 0x92, 0x12, 0x58, 0xE9, 0xFA, 0xAE, 0x4F, 0xE2, 0xB4, 0xCC,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gba::Gba;

    #[test]
    fn tea_comes_back() {
        for (a, b) in [(0u32, 0u32), (0x1200_0004, 0x3333), (0xDEAD_FACE, 0x1234), (0xFFFF_FFFF, 1)] {
            for k in [GS_SEEDS, AR_SEEDS] {
                let (x, y) = tea_encrypt(a, b, &k);
                assert_ne!((x, y), (a, b));
                assert_eq!(tea_decrypt(x, y, &k), (a, b));
            }
        }
    }

    #[test]
    fn the_first_line_says_which_device_the_list_is_for() {
        let enc = |(a, b): (u32, u32), k| {
            let (x, y) = tea_encrypt(a, b, k);
            format!("{x:08X} {y:08X}")
        };
        let gs = Device::from_codes([enc((0, ID_LINE), &GS_SEEDS).as_str(), enc((0x1200_0004, 0x3333), &GS_SEEDS).as_str()]);
        assert_eq!(gs.version, Version::GameShark);
        assert_eq!(gs.ops, vec![Op::new(Kind::Assign, 2, 0x0200_0004, 0x3333)]);
        let ar = Device::from_codes([enc((0, ID_LINE), &AR_SEEDS).as_str(), enc((0x0220_0004, 0x5555), &AR_SEEDS).as_str()]);
        assert_eq!(ar.version, Version::ActionReplay);
        assert_eq!(ar.ops, vec![Op { addr_step: 2, ..Op::new(Kind::Assign, 2, 0x0200_0004, 0x5555) }]);
        // Plain lines, which some lists print decrypted.
        let raw = Device::from_codes(["00000000 001DC0DE", "02000010 000000AB"]);
        assert_eq!(raw.version, Version::GameSharkRaw);
        assert_eq!(raw.ops, vec![Op::new(Kind::Assign, 1, 0x0200_0010, 0xAB)]);
    }

    #[test]
    fn an_action_replay_block_guards_its_writes_and_its_else_the_others() {
        // If the byte at $02000000 is $55: $01 at $02000010, else $02 at $02000011; then ENDIF,
        // and $03 at $02000012 whatever happens.
        // (A plain list: its DEADFACE line is what marks it as an Action Replay one.)
        let codes = [
            "DEADFACE 00001234",
            "88200000 00000055",
            "00200010 00000001",
            "00000000 60000000",
            "00200011 00000002",
            "00000000 40000000",
            "00200012 00000003",
        ];
        let d = Device::from_codes(codes);
        assert_eq!(d.version, Version::ActionReplayRaw);
        let mut bus = crate::gba::bus::Bus::new(vec![0; 0x200]);
        d.run(&mut bus);
        assert_eq!(&bus.ewram[0x10..0x13], &[0, 2, 3], "not $55: the else part");
        bus.ewram[0..0x13].fill(0);
        bus.ewram[0] = 0x55;
        d.run(&mut bus);
        assert_eq!(&bus.ewram[0x10..0x13], &[1, 0, 3], "$55: the then part");
    }

    /// An Advance program that loops at $08000008 for ever, writing nothing.
    fn looping() -> Gba {
        let mut rom = vec![0u8; 0x400];
        rom[0..4].copy_from_slice(&0xE1A0_0000u32.to_le_bytes()); // MOV r0,r0
        rom[4..8].copy_from_slice(&0xE1A0_0000u32.to_le_bytes());
        rom[8..12].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes()); // B .
        Gba::new(rom)
    }

    #[test]
    fn codes_run_at_the_blank_or_at_their_hook() {
        // No hook: nothing until the frame ends, then the write.
        let mut g = looping();
        g.cheats = Device::from_codes(["02000020:77"]);
        for _ in 0..100 {
            g.step();
        }
        assert_eq!(g.bus.ewram[0x20], 0, "not before the blank");
        g.run_frame();
        assert_eq!(g.bus.ewram[0x20], 0x77);
        // A hook at the loop ($08000008): straight away, each time the processor gets there.
        let mut g = looping();
        g.cheats = Device::from_codes(["00000000 001DC0DE", "F8000008 00000000", "02000020 00000077"]);
        assert_eq!(g.cheats.hook, Some(0x0800_0008));
        for _ in 0..10 {
            g.step();
        }
        assert_eq!(g.bus.ewram[0x20], 0x77, "run at the hook, before any blank");
    }

    #[test]
    fn a_rom_patch_shows_wherever_the_cartridge_is_read() {
        let mut g = looping();
        let d = Device::from_codes(["08000100:BEEF"]);
        g.bus.rom_patches = d.rom_patches.clone();
        assert_eq!(g.bus.peek16(0x0800_0100), 0xBEEF);
        assert_eq!(g.bus.peek16(0x0A00_0100), 0xBEEF, "and in the mirrors");
        assert_eq!(g.bus.peek8(0x0800_0101), 0xBE);
        assert_eq!(g.bus.peek16(0x0800_0102), 0);
        g.bus.rom_patches.clear();
        assert_eq!(g.bus.peek16(0x0800_0100), 0);
    }

    #[test]
    fn a_codebreaker_list_writes_its_halfwords_one_after_another() {
        // Three halfwords from $02000040: $1234, $5678, $9ABC (byte-swapped on the list's line).
        let d = Device::from_codes(["52000040 0003", "34127856 BC9A"]);
        let mut bus = crate::gba::bus::Bus::new(vec![0; 0x200]);
        d.run(&mut bus);
        assert_eq!(&bus.ewram[0x40..0x46], &[0x34, 0x12, 0x78, 0x56, 0xBC, 0x9A]);
    }
}
