//! The cartridge: ROM, external RAM and the memory bank controller (none, MBC1, MBC3 with its
//! real-time clock, MBC5, MMM01, MBC6 with its flash, TAMA5 with its clock). The ROM itself is never part of a save state.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MbcKind {
    None,
    Mbc1,
    Mbc3,
    Mbc5,
    /// Kirby Tilt 'n' Tumble's mapper: an accelerometer and a serial EEPROM for saves.
    Mbc7,
    /// MBC2: up to 256 KiB of ROM and 512 four-bit cells of RAM built into the chip.
    Mbc2,
    /// Hudson's HuC1: MBC1-like banking plus an infrared port (no partner, so it sees no light).
    HuC1,
    /// Hudson's HuC3: ROM and RAM banks, a clock, infrared, and a tone.
    HuC3,
    /// Game Boy Camera: MBC3-style banks plus the camera registers.
    Camera,
    /// MMM01: the multicart controller (Momotaro Collection, Taito Variety Pack). Starts
    /// unmapped, showing the menu in the last 32 KiB; the menu enables mapping, after which
    /// it behaves like an MBC1 for the chosen game (Pan Docs, MMM01.md).
    Mmm01,
    /// MBC6: Net de Get's controller, with two 8 KiB ROM/flash windows, two 4 KiB RAM windows
    /// and a 1 MiB Macronix flash (Pan Docs, MBC6.md).
    Mbc6,
    /// TAMA5: Bandai's controller for the Tamagotchi games, with a real-time clock and 32
    /// bytes of RAM driven through two 4-bit ports (mGBA's tama5.c model; Pan Docs does not
    /// describe it).
    Tama5,
}

/// MBC7's accelerometer latch and its 93LC56-style serial EEPROM (128 16-bit words, kept in
/// `Cart::ram` as 256 bytes).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Mbc7 {
    ram_enable2: bool,
    latch_armed: bool,
    pub x: u16,
    pub y: u16,
    cs: bool,
    clk: bool,
    do_bit: bool,
    shift_in: u32,
    bits_in: u8,
    shift_out: u16,
    bits_out: u8,
    write_enabled: bool,
    /// A WRITE or WRAL waiting for its 16 data bits: (address, all words).
    pending: Option<(u8, bool)>,
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

    /// The clock catches up on time that passed while the game was closed. Whole days are added
    /// to the 9-bit day counter in one go (past day 511 it wraps and sets the carry bit, as the
    /// chip does); what is left, under a day, is ticked second by second. Registers a game left
    /// out of range are ticked one by one, like hardware.
    pub fn advance_seconds(&mut self, mut secs: u64) {
        if self.halted() {
            return;
        }
        let in_range = |r: &[u8; 5]| r[0] < 60 && r[1] < 60 && r[2] < 24;
        // A value a game wrote out of range wraps back on its own within hours (at most 31 → 0
        // for the hour); until then, second by second, exactly as the chip would count.
        while secs > 0 && !in_range(&self.regs) {
            self.tick_second();
            secs -= 1;
        }
        let r = &mut self.regs;
        if secs >= 86_400 {
            let day = u64::from(r[4] & 1) << 8 | u64::from(r[3]);
            let total = day + secs / 86_400;
            secs %= 86_400;
            r[3] = total as u8;
            r[4] = (r[4] & 0xFE) | ((total >> 8) & 1) as u8;
            if total > 0x1FF {
                r[4] |= 0x80; // day counter carry
            }
        }
        // Under a day now.
        for _ in 0..secs {
            self.tick_second();
        }
    }
}

/// HuC3's clock memory: 256 nybbles. Minutes of the day and the day count live at $10-$15.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HuC3 {
    select: u8,
    cmd: u8,
    result: u8,
    addr: u8,
    mem: Vec<u8>,
    pub minutes: u16,
    pub days: u16,
    pub tone: bool,
    ir: u8,
}

impl Default for HuC3 {
    fn default() -> Self {
        Self {
            select: 0,
            cmd: 0,
            result: 1,
            addr: 0,
            mem: vec![0; 256],
            minutes: 0,
            days: 0,
            tone: false,
            ir: 0,
        }
    }
}

impl HuC3 {
    fn execute(&mut self) {
        let command = self.cmd >> 4;
        let arg = self.cmd & 0x0F;
        self.result = 1;
        match command {
            0x1 => {
                self.result = self.mem[usize::from(self.addr)] & 0x0F;
                self.addr = self.addr.wrapping_add(1);
            }
            0x3 => {
                self.mem[usize::from(self.addr)] = arg;
                self.addr = self.addr.wrapping_add(1);
            }
            0x4 => self.addr = (self.addr & 0xF0) | arg,
            0x5 => self.addr = (self.addr & 0x0F) | (arg << 4),
            0x6 => match arg {
                0x0 => {
                    self.mem[0x10] = (self.minutes & 0xF) as u8;
                    self.mem[0x11] = ((self.minutes >> 4) & 0xF) as u8;
                    self.mem[0x12] = ((self.minutes >> 8) & 0xF) as u8;
                    self.mem[0x13] = (self.days & 0xF) as u8;
                    self.mem[0x14] = ((self.days >> 4) & 0xF) as u8;
                    self.mem[0x15] = ((self.days >> 8) & 0xF) as u8;
                }
                0x1 => {
                    self.minutes = u16::from(self.mem[0x10] & 0xF)
                        | u16::from(self.mem[0x11] & 0xF) << 4
                        | u16::from(self.mem[0x12] & 0xF) << 8;
                    self.days = u16::from(self.mem[0x13] & 0xF)
                        | u16::from(self.mem[0x14] & 0xF) << 4
                        | u16::from(self.mem[0x15] & 0xF) << 8;
                    if self.minutes >= 1440 {
                        self.minutes = 0;
                    }
                }
                0x2 => self.result = 1,
                0xE => self.tone = self.mem[0x27] & 0x0F == 1,
                _ => {}
            },
            _ => {}
        }
    }

    fn tick_minute(&mut self) {
        self.minutes = self.minutes.wrapping_add(1);
        if self.minutes >= 1440 {
            self.minutes = 0;
            self.days = self.days.wrapping_add(1);
        }
    }

    /// Minutes that passed while the game was closed: into the minute of the day and the day
    /// count, the way `tick_minute` would have moved them.
    pub fn advance_minutes(&mut self, mins: u64) {
        let total = u64::from(self.minutes.min(1439)) + mins;
        self.minutes = (total % 1440) as u16;
        self.days = (u64::from(self.days) + total / 1440) as u16;
    }
}

/// What the Game Boy Camera's sensor hands its controller: 128 across, 120 down. Only the
/// middle 112 rows become the picture; the four spare rows top and bottom feed the edge filter.
pub const SENSOR_W: usize = 128;
pub const SENSOR_H: usize = 120;
/// The picture the cartridge writes: 128 by 112, as 14 rows of 16 tiles.
pub const CAM_W: usize = 128;
pub const CAM_H: usize = 112;
/// Where the cartridge puts the picture: RAM bank 0, 256 bytes in. The first 256 bytes are the
/// game's own, so writing at the start of the bank trampled them.
const CAM_IMAGE_AT: usize = 0x0100;

/// MMM01's registers (Pan Docs, MMM01.md). All are 7 bits wide and 0 at power-up. The
/// extended bits (ROM mid/high, the masks, multiplex) can only be written while unmapped;
/// setting mapping enable locks them and the chip behaves like an MBC1 for the chosen game.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Mmm01 {
    /// Mapping enabled (the menu has chosen a game).
    pub mapped: bool,
    /// ROM bank low 5 bits ($2000), mid 2 bits ($2000 bits 5-6), high 2 bits ($4000 bits 4-5).
    pub rom_low: u8,
    pub rom_mid: u8,
    pub rom_high: u8,
    /// RAM bank low 2 bits ($4000 bits 0-1) and high 2 bits ($4000 bits 2-3).
    pub ram_low: u8,
    pub ram_high: u8,
    /// Write locks: ROM bank mask ($6000 bits 1-5) and RAM bank mask ($0000 bits 4-5).
    pub rom_mask: u8,
    pub ram_mask: u8,
    /// Banking mode ($6000 bit 0) and multiplex ($6000 bit 6).
    pub mode: u8,
    pub multiplex: bool,
    /// $4000 bit 6 locks the mode select.
    pub mode_locked: bool,
}

impl Mmm01 {
    /// The 16 KiB ROM bank at $0000-$3FFF in mapped mode (Pan Docs' addressing diagrams).
    fn bank0(&self) -> usize {
        let low = usize::from(self.rom_low) & usize::from(self.rom_mask);
        if self.multiplex {
            let mid = if self.mode == 1 {
                usize::from(self.ram_low)
            } else {
                usize::from(self.ram_low) & usize::from(self.ram_mask)
            };
            (usize::from(self.rom_high) << 7) | (mid << 5) | low
        } else {
            (usize::from(self.rom_high) << 7) | (usize::from(self.rom_mid) << 5) | low
        }
    }

    /// The 16 KiB ROM bank at $4000-$7FFF in mapped mode, with the 0 -> 1 correction on the
    /// unmasked low bits (Pan Docs: if `ROM Bank Low & ~ROM Bank Mask` is 0, bank 1 is used).
    fn bank1(&self) -> usize {
        let mut low = usize::from(self.rom_low);
        if low & !usize::from(self.rom_mask) == 0 {
            low |= 1;
        }
        if self.multiplex {
            (usize::from(self.rom_high) << 7) | (usize::from(self.ram_low) << 5) | low
        } else {
            (usize::from(self.rom_high) << 7) | (usize::from(self.rom_mid) << 5) | low
        }
    }

    /// The 8 KiB RAM bank at $A000-$BFFF in mapped mode.
    fn ram_bank(&self) -> usize {
        let low = if self.multiplex {
            usize::from(self.rom_mid)
        } else if self.mode == 1 {
            usize::from(self.ram_low)
        } else {
            usize::from(self.ram_low) & usize::from(self.ram_mask)
        };
        (usize::from(self.ram_high) << 2) | low
    }
}

/// MBC6's registers and its 1 MiB flash (Pan Docs, MBC6.md). The flash is a Macronix
/// MX29F008TC: 8 sectors of 128 KiB plus a 256-byte hidden region, JEDEC ID $C2/$81.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mbc6 {
    pub ram_bank_a: u8,
    pub ram_bank_b: u8,
    /// Flash chip enable ($0C00) and sector-0/hidden write enable ($1000, the /WP pin).
    pub flash_en: bool,
    pub flash_wp: bool,
    pub rom_bank_a: u8,
    pub rom_bank_b: u8,
    /// Window A/B show the flash instead of the ROM ($2800/$3800 bit 3).
    pub flash_a: bool,
    pub flash_b: bool,
    /// The flash command state machine: 0 idle, 1 after $AA to $5555, 2 after $55 to $2AAA,
    /// 3 erase set-up, 4 hidden/protect set-up.
    pub cmd_state: u8,
    /// What a completed set-up is waiting to do: 0x80 erase, 0x60 hidden/protect command.
    pub cmd_kind: u8,
    /// ID mode ($90), hidden-region read mode ($77), program mode ($A0) / hidden program ($E0).
    pub id_mode: bool,
    pub hidden_mode: bool,
    pub prog_mode: bool,
    pub prog_hidden: bool,
    /// Transient program-session state, not serialized (so CGBSTAT4 save states stay
    /// byte-compatible): the flash offset of the last byte programmed in this program
    /// session, and whether the commit repeat (a second write to that offset with a
    /// non-$F0 value) has happened. Pan Docs MBC6 programs by streaming 128 bytes,
    /// repeating the final address (any value except $F0) to commit, then $F0 to any
    /// address to exit; the NP doc adds that a $F0 repeat to the last address aborts.
    #[serde(skip)]
    pub prog_last_off: Option<usize>,
    #[serde(skip)]
    pub prog_committed: bool,
    /// Whether a flash read returns the status byte (bit 7 set, bit 1 sector-0 protection)
    /// instead of flash contents, after a program/erase/protect command, until $F0.
    pub status: bool,
    /// Sector 0 protection, set by the Protect Sector 0 flash command (non-volatile).
    pub sector0_protected: bool,
    /// The 256-byte hidden region (a `Vec` because serde's derive stops at 32-byte arrays).
    pub hidden: Vec<u8>,
}

impl Default for Mbc6 {
    fn default() -> Self {
        Self {
            ram_bank_a: 0,
            ram_bank_b: 0,
            flash_en: false,
            flash_wp: false,
            rom_bank_a: 0,
            rom_bank_b: 0,
            flash_a: false,
            flash_b: false,
            cmd_state: 0,
            cmd_kind: 0,
            id_mode: false,
            hidden_mode: false,
            prog_mode: false,
            prog_hidden: false,
            prog_last_off: None,
            prog_committed: false,
            status: false,
            sector0_protected: false,
            hidden: vec![0xFF; 256],
        }
    }
}

/// TAMA5's runtime state (mGBA's tama5.c model). The 32 bytes of RAM and the four 16-nybble
/// clock pages live in `Cart::ram` in mGBA's save layout (RAM, then the pages packed two
/// nybbles to a byte, then 8 bytes of latched time), so a battery save carries them.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Tama5 {
    /// The eight 4-bit registers (0/1 ROM bank, 4/5 data, 6 command, 7 address/trigger).
    pub regs: [u8; 8],
    /// The register number the odd port ($A001) last selected.
    pub selected: u8,
    /// What the $C/$D read ports return (low/high nybble of the last command's result).
    pub result_lo: u8,
    pub result_hi: u8,
    /// T-cycles toward the next clock second.
    pub sub: u32,
}

impl Tama5 {
    /// The mask table for writes to the clock pages (mGBA's `_tama6RTCMask`).
    const MASK: [[u8; 16]; 2] = [
        [0xF, 0x7, 0xF, 0x7, 0xF, 0x3, 0x7, 0xF, 0x3, 0xF, 0x1, 0xF, 0xF, 0, 0, 0],
        [0, 0, 0xF, 0x7, 0xF, 0x3, 0x7, 0xF, 0x3, 0, 0x1, 0x3, 0, 0, 0, 0],
    ];
}

/// Game Boy Camera registers (A000-A035) and the newest picture from the phone's camera.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Camera {
    regs: Vec<u8>,
    busy: u8,
    /// The phone's camera as the sensor sees it: `SENSOR_W` by `SENSOR_H` shades of grey, 0 to
    /// 255, newest frame first. Empty until a frame arrives, and then the viewfinder is a flat
    /// grey rather than a picture nobody took. Filled by `libretro.rs` from the frontend's
    /// camera, not part of a save state.
    #[serde(skip)]
    pub image: Vec<u8>,
}

impl Default for Camera {
    fn default() -> Self {
        Self { regs: vec![0; 54], busy: 0, image: Vec::new() }
    }
}

impl Camera {
    /// Contrast and dithering: the 4x4 matrix at A006-A035, three thresholds per cell, which
    /// turns one shade of grey into one of the Game Boy's four.
    fn matrix(&self, value: i32, x: usize, y: usize) -> u8 {
        let base = 6 + ((y & 3) * 4 + (x & 3)) * 3;
        let at = |i: usize| i32::from(self.regs[base + i]);
        if value < at(0) {
            0x00
        } else if value < at(1) {
            0x40
        } else if value < at(2) {
            0x80
        } else {
            0xC0
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
    pub rom_bank: u16,
    ram_bank: u8,
    /// MBC1 bank2 register (2 bits) and mode.
    bank2: u8,
    mode: u8,
    pub rtc: Rtc,
    pub title: String,
    pub cgb_flag: u8,
    /// MBC1M multicart wiring: 4-bit low bank, bank2 shifted by 4.
    #[serde(default)]
    multicart: bool,
    /// MBC5 rumble cartridges (types $1C-$1E): RAM bank bit 3 drives the motor.
    #[serde(default)]
    pub rumble_cart: bool,
    #[serde(default)]
    pub motor: bool,
    #[serde(default)]
    pub mbc7: Mbc7,
    /// Bytes in the file the user imported, before the power-of-two pad. Not saved.
    #[serde(skip)]
    pub file_len: usize,
    /// Tilt for MBC7, -1..1 each axis (from the core's sensor input). Not saved.
    #[serde(skip)]
    pub tilt: (f32, f32),
    /// HuC3 clock, infrared and tone. Unused for every other cartridge.
    #[serde(default)]
    pub huc3: HuC3,
    /// Game Boy Camera registers and the last picture from the phone. Unused otherwise.
    #[serde(default)]
    pub camera: Camera,
    /// MMM01's registers. Unused for every other cartridge.
    #[serde(default)]
    pub mmm01: Mmm01,
    /// MBC6's registers and hidden flash region. Unused for every other cartridge.
    #[serde(default)]
    pub mbc6: Mbc6,
    /// TAMA5's registers and clock state. Unused for every other cartridge.
    #[serde(default)]
    pub tama5: Tama5,
}

impl Cart {
    /// MBC6's 1 MiB flash lives at the end of `ram`, after its 32 KiB of SRAM: the frontend's
    /// battery save is just `ram`, so the flash has to be in it for Net de Get's progress to
    /// survive a close. `ram.len()` for an MBC6 cartridge is therefore 0x108000.
    const MBC6_SRAM_LEN: usize = 0x8000;
    const MBC6_FLASH_LEN: usize = 0x10_0000;

    fn flash_at(&self, off: usize) -> u8 {
        self.ram[Self::MBC6_SRAM_LEN + (off % Self::MBC6_FLASH_LEN)]
    }

    /// Programming a flash byte only clears bits; `v` is ANDed in.
    fn flash_and(&mut self, off: usize, v: u8) {
        let i = Self::MBC6_SRAM_LEN + (off % Self::MBC6_FLASH_LEN);
        self.ram[i] &= v;
    }

    fn flash_fill(&mut self, range: std::ops::Range<usize>, v: u8) {
        let base = Self::MBC6_SRAM_LEN;
        self.ram[base + range.start..base + range.end].fill(v);
    }
}

pub fn declared_rom_bytes(code: u8) -> Option<usize> {
    match code {
        0x00..=0x08 => Some(0x8000usize << code),
        0x52 => Some(72 * 0x4000),
        0x53 => Some(80 * 0x4000),
        0x54 => Some(96 * 0x4000),
        _ => None,
    }
}

/// The Nintendo logo at $104-$133. The console's own start-up code compares it byte for byte and
/// refuses the cartridge if it differs, so every cartridge that ever ran on a Game Boy has it.
pub const LOGO: [u8; 48] = [
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

/// The header checksum at $14D: every byte of $134-$14C subtracted from zero, one at a time.
fn header_checksum_ok(rom: &[u8]) -> bool {
    let sum = rom[0x134..0x14D].iter().fold(0u8, |a, b| a.wrapping_sub(*b).wrapping_sub(1));
    sum == rom[0x14D]
}

/// The file carries a Game Boy cartridge header: the Nintendo logo, or a header checksum that
/// adds up (a little homebrew leaves the logo out but gets the checksum right). Used to tell a
/// Game Boy game from a Game Boy Advance one, whose header sits somewhere else entirely.
pub fn looks_like_gb_rom(rom: &[u8]) -> bool {
    rom.len() >= 0x150 && (rom[0x104..0x134] == LOGO || header_checksum_ok(rom))
}

/// What a cartridge's chip is called, for a refusal a player can read. The ones cBoy plays are
/// not in here: this only names what it has to turn away.
fn chip_name(kind: u8) -> &'static str {
    match kind {
        0x15..=0x17 => "MBC4",
        _ => "an unknown one",
    }
}

/// How the Library sizes a file, so a refusal names the same number the list already shows.
fn size_label(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        let tenths = (bytes as u64 * 10 + (1024 * 1024 / 2)) / (1024 * 1024) as u64;
        format!("{}.{} MB", tenths / 10, tenths % 10)
    } else if bytes >= 1024 {
        format!("{} KB", (bytes as u64 + 512) / 1024)
    } else {
        format!("{bytes} bytes")
    }
}

/// A file shorter than the cartridge header, past the 32 KB minimum every Game Boy ROM is padded
/// to. Small homebrew that only claims 32 KB still runs. A Yellow that claims 1 MB and stops at
/// 997 KB does not: the opening after the Game Freak logo is in the missing part, and padding
/// that hole with $FF makes the picture run wild.
pub fn cutoff_reason(rom: &[u8]) -> Option<String> {
    if rom.len() < 0x149 {
        return None;
    }
    let declared = declared_rom_bytes(rom[0x148])?;
    if declared <= 0x8000 || rom.len() >= declared {
        return None;
    }
    Some(format!(
        "This file is cut off. It is {} and the cartridge says {}. The missing part is not in the file. Use a complete copy.",
        size_label(rom.len()),
        size_label(declared),
    ))
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
            return Err("This file is too small to be a Game Boy game. It may have been cut short, or it may not be a game at all.".into());
        }
        let t = rom[0x147];
        let (kind, battery, rtc) = match t {
            0x00 | 0x08 | 0x09 => (MbcKind::None, t == 0x09, false),
            0x01..=0x03 => (MbcKind::Mbc1, t == 0x03, false),
            0x0B..=0x0D => (MbcKind::Mmm01, t == 0x0C || t == 0x0D, false),
            0x0F | 0x10 => (MbcKind::Mbc3, true, true),
            0x11..=0x13 => (MbcKind::Mbc3, t == 0x13, false),
            0x19..=0x1E => (MbcKind::Mbc5, t == 0x1B || t == 0x1E, false),
            0x20 => (MbcKind::Mbc6, true, false),
            0x22 => (MbcKind::Mbc7, true, false),
            0x05 | 0x06 => (MbcKind::Mbc2, t == 0x06, false),
            0xFF => (MbcKind::HuC1, true, false),
            0xFE => (MbcKind::HuC3, true, true),
            0xFC => (MbcKind::Camera, true, false),
            0xFD => (MbcKind::Tama5, true, false),
            other => {
                return Err(format!(
                    "This cartridge has a chip cBoy cannot play yet ({}). Try this game on mGBA instead: open its settings and pick mGBA as the emulator.",
                    chip_name(other)
                ))
            }
        };
        let mut ram_len = match kind {
            MbcKind::Mbc7 => 256,
            MbcKind::Mbc2 => 512,
            // MBC6's 1 MiB flash rides at the end of `ram` (see `MBC6_SRAM_LEN`), so the
            // frontend's battery save carries it.
            MbcKind::Mbc6 => 0x8000 + 0x10_0000,
            // TAMA5's save is mGBA's 72-byte layout: 32 bytes of RAM, the four clock pages
            // packed two nybbles to a byte, then 8 bytes of latched time.
            MbcKind::Tama5 => 72,
            MbcKind::HuC3 | MbcKind::Camera if ram_size(rom[0x149]) == 0 => 0x2000,
            _ => ram_size(rom[0x149]),
        };
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
        let file_len = rom.len();
        let mut rom = rom;
        rom.resize(size, 0xFF);
        let multicart = kind == MbcKind::Mbc1
            && rom.len() == 0x10_0000
            && rom[0x40104] == 0xCE
            && rom[0x40104..0x40134] == rom[0x104..0x134];
        // MMM01: the header the console boots from is the menu's, in the last 32 KiB, not the
        // first game's at $0100 (Pan Docs, MMM01.md). The title and colour flag come from it
        // when it carries the Nintendo logo.
        let (title, cgb_flag) = if kind == MbcKind::Mmm01
            && rom.len() >= 0x8000
            && rom[rom.len() - 0x8000 + 0x104..rom.len() - 0x8000 + 0x134] == LOGO
        {
            let base = rom.len() - 0x8000;
            (
                rom[base + 0x134..base + 0x143]
                    .iter()
                    .take_while(|b| **b != 0)
                    .map(|b| *b as char)
                    .collect(),
                rom[base + 0x143],
            )
        } else {
            (title, rom[0x143])
        };
        let mut cart = Cart {
            cgb_flag,
            rom,
            ram: vec![0xFF; ram_len],
            kind,
            has_battery: battery,
            has_rtc: rtc,
            ram_enabled: kind == MbcKind::HuC1,
            rom_bank: 1,
            ram_bank: 0,
            bank2: 0,
            mode: 0,
            rtc: Rtc::default(),
            multicart,
            title,
            rumble_cart: (0x1C..=0x1E).contains(&t),
            motor: false,
            mbc7: Mbc7::default(),
            file_len,
            tilt: (0.0, 0.0),
            huc3: HuC3::default(),
            camera: Camera::default(),
            mmm01: Mmm01::default(),
            mbc6: Mbc6::default(),
            tama5: Tama5 { regs: [1, 0, 0, 0, 0, 0, 0, 0], ..Tama5::default() },
        };
        if kind == MbcKind::Tama5 {
            // The clock pages start empty, with each page's register (index $D) holding its
            // own page number (mGBA's set-up). The 32 bytes of RAM stay $FF, as at reset.
            for b in &mut cart.ram[32..64] {
                *b = 0;
            }
            for (page, n) in [1u8, 2, 3].into_iter().enumerate() {
                // Index $D is odd, so it is the high nybble of the page's byte 6.
                cart.ram[32 + (page + 1) * 8 + 6] = n << 4;
            }
        }
        Ok(cart)
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
                    let shift = if self.multicart { 4 } else { 5 };
                    let bank = if self.mode == 1 { (self.bank2 as usize) << shift } else { 0 };
                    self.rom_at(bank, addr)
                } else {
                    let raw = (self.rom_bank & 0x1F) as usize;
                    let low = if raw == 0 { 1 } else { raw };
                    if self.multicart {
                        self.rom_at((self.bank2 as usize) << 4 | (low & 0x0F), addr)
                    } else {
                        self.rom_at((self.bank2 as usize) << 5 | low, addr)
                    }
                }
            }
            MbcKind::Mmm01 => {
                if !self.mmm01.mapped {
                    // Unmapped: the last 32 KiB of the ROM (the menu) at $0000-$7FFF.
                    let banks = self.rom_banks();
                    self.rom_at(if addr < 0x4000 { banks - 2 } else { banks - 1 }, addr)
                } else if addr < 0x4000 {
                    self.rom_at(self.mmm01.bank0(), addr)
                } else {
                    self.rom_at(self.mmm01.bank1(), addr)
                }
            }
            MbcKind::Tama5 => {
                if addr < 0x4000 {
                    self.rom[addr as usize]
                } else {
                    let bank = (usize::from(self.tama5.regs[1]) << 4) | usize::from(self.tama5.regs[0]);
                    self.rom_at(bank, addr)
                }
            }
            MbcKind::Mbc6 => self.mbc6_read_rom(addr),
            MbcKind::Mbc3 | MbcKind::Mbc5 | MbcKind::Mbc7 | MbcKind::Mbc2 | MbcKind::HuC1
            | MbcKind::HuC3 | MbcKind::Camera => {
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
            MbcKind::Mmm01 => self.mmm01_write(addr, v),
            MbcKind::Mbc6 => self.mbc6_write_rom(addr, v),
            MbcKind::Tama5 => {} // Writes to the ROM area are ignored (mGBA's model).
            MbcKind::Mbc3 => match addr {
                0x0000..=0x1FFF => self.ram_enabled = v & 0x0F == 0x0A,
                0x2000..=0x3FFF => {
                    // MBC30 (over 2 MiB of ROM) decodes all eight bits.
                    let b = u16::from(if self.rom.len() > 0x20_0000 { v } else { v & 0x7F });
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
                0x4000..=0x5FFF => {
                    if self.rumble_cart {
                        self.ram_bank = v & 0x07;
                        self.motor = v & 0x08 != 0;
                    } else {
                        self.ram_bank = v & 0x0F;
                    }
                }
                _ => {}
            },
            MbcKind::Mbc2 => {
                if addr < 0x4000 {
                    if addr & 0x100 == 0 {
                        self.ram_enabled = v & 0x0F == 0x0A;
                    } else {
                        let b = u16::from(v & 0x0F);
                        self.rom_bank = if b == 0 { 1 } else { b };
                    }
                }
            }
            MbcKind::HuC1 => match addr {
                // 0x0E switches A000-BFFF to the infrared port; anything else back to RAM.
                0x0000..=0x1FFF => {
                    self.mode = u8::from(v & 0x0F == 0x0E);
                    self.ram_enabled = true;
                }
                0x2000..=0x3FFF => self.rom_bank = u16::from(v & 0x3F),
                0x4000..=0x5FFF => self.ram_bank = v & 3,
                _ => {}
            },
            MbcKind::Mbc7 => match addr {
                0x0000..=0x1FFF => self.ram_enabled = v == 0x0A,
                0x2000..=0x3FFF => self.rom_bank = u16::from(v & 0x7F),
                0x4000..=0x5FFF => self.mbc7.ram_enable2 = v == 0x40,
                _ => {}
            },
            MbcKind::HuC3 => match addr {
                0x0000..=0x1FFF => self.huc3.select = v & 0x0F,
                0x2000..=0x3FFF => self.rom_bank = u16::from(v & 0x7F),
                0x4000..=0x5FFF => self.ram_bank = v & 0x0F,
                _ => {}
            },
            MbcKind::Camera => match addr {
                0x0000..=0x1FFF => self.ram_enabled = v & 0x0F == 0x0A,
                0x2000..=0x3FFF => {
                    let b = u16::from(v & 0x7F);
                    self.rom_bank = if b == 0 { 1 } else { b };
                }
                0x4000..=0x5FFF => self.ram_bank = v,
                _ => {}
            },
        }
    }

    /// MMM01's register writes (Pan Docs, MMM01.md). The masks act as write locks on the
    /// matching bits of the low bank registers, even in unmapped mode; the extended bits can
    /// only be written while unmapped, and mapping enable locks them.
    fn mmm01_write(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x1FFF => {
                self.ram_enabled = v & 0x0F == 0x0A;
                if !self.mmm01.mapped {
                    self.mmm01.ram_mask = (v >> 4) & 3;
                    if v & 0x40 != 0 {
                        self.mmm01.mapped = true;
                    }
                }
            }
            0x2000..=0x3FFF => {
                let m = &mut self.mmm01;
                if m.mapped {
                    m.rom_low = (m.rom_low & m.rom_mask) | (v & !m.rom_mask & 0x1F);
                } else {
                    m.rom_low = v & 0x1F;
                    m.rom_mid = (v >> 5) & 3;
                }
            }
            0x4000..=0x5FFF => {
                let m = &mut self.mmm01;
                if m.mapped {
                    m.ram_low = (m.ram_low & m.ram_mask) | (v & !m.ram_mask & 3);
                    m.ram_high = (v >> 2) & 3;
                } else {
                    m.ram_low = v & 3;
                    m.ram_high = (v >> 2) & 3;
                    m.rom_high = (v >> 4) & 3;
                }
                if v & 0x40 != 0 {
                    m.mode_locked = true;
                }
            }
            _ => {
                if !self.mmm01.mode_locked {
                    self.mmm01.mode = v & 1;
                }
                if !self.mmm01.mapped {
                    self.mmm01.rom_mask = (v >> 1) & 0x1F;
                    self.mmm01.multiplex = v & 0x40 != 0;
                }
            }
        }
    }

    /// MBC6's ROM/flash windows: $0000-$3FFF is the first 16 KiB of ROM, $4000-$5FFF window A
    /// and $6000-$7FFF window B, each an 8 KiB bank of ROM or flash.
    fn mbc6_read_rom(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.rom[addr as usize],
            0x4000..=0x5FFF => {
                let off = (usize::from(self.mbc6.rom_bank_a & 0x7F) * 0x2000) + usize::from(addr & 0x1FFF);
                if self.mbc6.flash_a {
                    self.flash_read(off)
                } else {
                    self.rom[off % self.rom.len()]
                }
            }
            _ => {
                let off = (usize::from(self.mbc6.rom_bank_b & 0x7F) * 0x2000) + usize::from(addr & 0x1FFF);
                if self.mbc6.flash_b {
                    self.flash_read(off)
                } else {
                    self.rom[off % self.rom.len()]
                }
            }
        }
    }

    /// One byte out of the MBC6 flash, through the chip's current mode.
    fn flash_read(&self, off: usize) -> u8 {
        if !self.mbc6.flash_en {
            return 0xFF;
        }
        let off = off % Self::MBC6_FLASH_LEN;
        if self.mbc6.id_mode {
            return if off & 1 == 0 { 0xC2 } else { 0x81 };
        }
        if self.mbc6.hidden_mode {
            return self.mbc6.hidden[off & 0xFF];
        }
        if self.mbc6.status {
            return 0x80 | u8::from(self.mbc6.sector0_protected) << 1;
        }
        self.flash_at(off)
    }

    /// MBC6's register writes ($0000-$3FFF, decoded by address bits 10-15 as mGBA does) and
    /// flash command writes through the ROM/flash windows ($4000-$7FFF).
    fn mbc6_write_rom(&mut self, addr: u16, v: u8) {
        match addr >> 10 {
            0 => self.ram_enabled = v & 0x0F == 0x0A,
            1 => self.mbc6.ram_bank_a = v & 7,
            2 => self.mbc6.ram_bank_b = v & 7,
            3 => self.mbc6.flash_en = v & 1 != 0,
            4 => self.mbc6.flash_wp = v & 1 != 0,
            8 | 9 => self.mbc6.rom_bank_a = v,
            10 | 11 => self.mbc6.flash_a = v & 8 != 0,
            12 | 13 => self.mbc6.rom_bank_b = v,
            14 | 15 => self.mbc6.flash_b = v & 8 != 0,
            16..=31 => self.mbc6_flash_write(addr, v),
            _ => {} // $1400-$1FFF: unused registers.
        }
    }

    /// Whether the flash address is write-protected: sector 0 (the first 128 KiB) is protected
    /// when the flash write-enable bit is clear or the Protect Sector 0 command set it; the
    /// hidden region is protected when the write-enable bit is clear.
    fn flash_protected(&self, off: usize, hidden: bool) -> bool {
        if hidden {
            !self.mbc6.flash_wp
        } else if off < 0x20000 {
            !self.mbc6.flash_wp || self.mbc6.sector0_protected
        } else {
            false
        }
    }

    /// A write to the MBC6 flash, through the chip's command state machine (Pan Docs, MBC6.md).
    /// Flash addresses are the banked offset: bank * 8 KiB + the window offset.
    fn mbc6_flash_write(&mut self, addr: u16, v: u8) {
        let (bank, is_flash) = if addr < 0x6000 {
            (self.mbc6.rom_bank_a, self.mbc6.flash_a)
        } else {
            (self.mbc6.rom_bank_b, self.mbc6.flash_b)
        };
        if !is_flash || !self.mbc6.flash_en {
            return;
        }
        let off = (usize::from(bank & 0x7F) * 0x2000) + usize::from(addr & 0x1FFF);
        // Program mode: data writes come before the $F0 reset check, so $F0 itself can
        // be programmed. Pan Docs MBC6 streams 128 bytes, repeats the final address
        // with any value except $F0 to commit, then $F0 to any address exits; the NP
        // doc adds that a $F0 repeat to the last address aborts the programming
        // instead. So $F0 is data at a new offset, but exits after the commit repeat
        // (any address) or as a repeat to the last programmed offset (abort).
        if self.mbc6.prog_mode {
            if v == 0xF0 {
                if self.mbc6.prog_committed || self.mbc6.prog_last_off == Some(off) {
                    self.mbc6.cmd_state = 0;
                    self.mbc6.id_mode = false;
                    self.mbc6.hidden_mode = false;
                    self.mbc6.prog_mode = false;
                    self.mbc6.prog_hidden = false;
                    self.mbc6.status = false;
                    self.mbc6.prog_last_off = None;
                    self.mbc6.prog_committed = false;
                    return;
                }
            } else if self.mbc6.prog_last_off == Some(off) {
                // A repeat write to the last programmed offset (non-$F0) is the commit:
                // it commits the byte already programmed and must not AND anything onto
                // it, so it returns at once instead of falling through to flash_and
                // (Pan Docs MBC6: committing with $00 must leave the byte intact).
                self.mbc6.prog_committed = true;
                return;
            }
            if self.mbc6.prog_hidden {
                if !self.flash_protected(off, true) {
                    self.mbc6.hidden[off & 0xFF] &= v;
                }
            } else if !self.flash_protected(off, false) {
                self.flash_and(off, v);
            }
            self.mbc6.prog_last_off = Some(off);
            return;
        }
        // $F0 exits any command, from any address.
        if v == 0xF0 {
            self.mbc6.cmd_state = 0;
            self.mbc6.id_mode = false;
            self.mbc6.hidden_mode = false;
            self.mbc6.prog_mode = false;
            self.mbc6.prog_hidden = false;
            self.mbc6.status = false;
            self.mbc6.prog_last_off = None;
            self.mbc6.prog_committed = false;
            return;
        }
        match self.mbc6.cmd_state {
            0 => {
                if off == 0x5555 && v == 0xAA {
                    self.mbc6.cmd_state = 1;
                }
            }
            1 => {
                self.mbc6.cmd_state = if off == 0x2AAA && v == 0x55 { 2 } else { 0 };
            }
            2 => {
                self.mbc6.cmd_state = 0;
                if off != 0x5555 {
                    return;
                }
                match v {
                    0x80 | 0x60 | 0x77 => {
                        self.mbc6.cmd_kind = v;
                        self.mbc6.cmd_state = 3;
                    }
                    0x90 => self.mbc6.id_mode = true,
                    0xA0 => {
                        self.mbc6.prog_mode = true;
                        self.mbc6.status = true;
                        self.mbc6.prog_last_off = None;
                        self.mbc6.prog_committed = false;
                    }
                    _ => {}
                }
            }
            3 => {
                self.mbc6.cmd_state = if off == 0x5555 && v == 0xAA { 4 } else { 0 };
            }
            4 => {
                self.mbc6.cmd_state = if off == 0x2AAA && v == 0x55 { 5 } else { 0 };
            }
            _ => {
                self.mbc6.cmd_state = 0;
                match self.mbc6.cmd_kind {
                    0x80 => {
                        if v == 0x10 && off == 0x5555 {
                            // Erase the whole chip except a protected sector 0 (never the hidden region).
                            let start = if self.flash_protected(0, false) { 0x20000 } else { 0 };
                            self.flash_fill(start..Self::MBC6_FLASH_LEN, 0xFF);
                            self.mbc6.status = true;
                        } else if v == 0x30 {
                            let sector = off >> 17;
                            let base = sector << 17;
                            if !self.flash_protected(base, false) {
                                let end = (base + 0x20000).min(Self::MBC6_FLASH_LEN);
                                self.flash_fill(base..end, 0xFF);
                            }
                            self.mbc6.status = true;
                        }
                    }
                    0x60 => {
                        if off != 0x5555 {
                            return;
                        }
                        match v {
                            0x04 => {
                                if self.mbc6.flash_wp {
                                    self.mbc6.hidden.fill(0xFF);
                                }
                                self.mbc6.status = true;
                            }
                            0xE0 => {
                                self.mbc6.prog_mode = true;
                                self.mbc6.prog_hidden = true;
                                self.mbc6.status = true;
                                self.mbc6.prog_last_off = None;
                                self.mbc6.prog_committed = false;
                            }
                            0x40 => {
                                if self.mbc6.flash_wp {
                                    self.mbc6.sector0_protected = false;
                                }
                                self.mbc6.status = true;
                            }
                            0x20 => {
                                if self.mbc6.flash_wp {
                                    self.mbc6.sector0_protected = true;
                                }
                                self.mbc6.status = true;
                            }
                            _ => {}
                        }
                    }
                    0x77 => {
                        if v == 0x77 && off == 0x5555 {
                            self.mbc6.hidden_mode = true;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// MBC6's RAM windows: $A000-$AFFF is RAM bank A, $B000-$BFFF RAM bank B, 4 KiB each.
    fn mbc6_read(&self, addr: u16) -> u8 {
        if !self.ram_enabled || self.ram.is_empty() {
            return 0xFF;
        }
        let bank = if addr < 0xB000 { self.mbc6.ram_bank_a } else { self.mbc6.ram_bank_b };
        self.ram[(usize::from(bank & 7) * 0x1000 + usize::from(addr & 0x0FFF)) % Self::MBC6_SRAM_LEN]
    }

    fn mbc6_write(&mut self, addr: u16, v: u8) {
        if !self.ram_enabled || self.ram.is_empty() {
            return;
        }
        let bank = if addr < 0xB000 { self.mbc6.ram_bank_a } else { self.mbc6.ram_bank_b };
        let i = (usize::from(bank & 7) * 0x1000 + usize::from(addr & 0x0FFF)) % Self::MBC6_SRAM_LEN;
        self.ram[i] = v;
    }

    /// One nybble of a TAMA5 clock page (pages are packed two nybbles to a byte in `ram`).
    fn tama_page(&self, page: usize, index: usize) -> u8 {
        let b = self.ram[32 + page * 8 + index / 2];
        if index & 1 == 0 { b & 0xF } else { b >> 4 }
    }

    fn tama_set_page(&mut self, page: usize, index: usize, v: u8) {
        let i = 32 + page * 8 + index / 2;
        self.ram[i] = if index & 1 == 0 {
            (self.ram[i] & 0xF0) | (v & 0xF)
        } else {
            (self.ram[i] & 0x0F) | ((v & 0xF) << 4)
        };
    }

    /// Whether the TAMA5 clock is running (bit 3 of the timer page's register, index $D).
    fn tama_timer_enabled(&self) -> bool {
        self.tama_page(0, 0xD) & 8 != 0
    }

    /// TAMA5's data port ($A000): the result of the last command, or the ready status.
    fn tama_read(&self, addr: u16) -> u8 {
        if addr & 1 != 0 {
            return 0xFF;
        }
        match self.tama5.selected {
            0x0A => 0xF1,
            0x0C => 0xF0 | self.tama5.result_lo,
            0x0D => 0xF0 | self.tama5.result_hi,
            _ => 0xF1,
        }
    }

    /// TAMA5's ports: the odd address selects a register, the even one writes its low nybble;
    /// writing register 7 executes the command register 6 chose (mGBA's tama5.c model).
    fn tama_write(&mut self, addr: u16, v: u8) {
        if addr & 1 != 0 {
            self.tama5.selected = v;
            return;
        }
        let sel = self.tama5.selected;
        if sel >= 8 {
            return;
        }
        self.tama5.regs[usize::from(sel)] = v & 0xF;
        if sel == 7 {
            self.tama_execute();
        }
    }

    fn tama_execute(&mut self) {
        let regs = self.tama5.regs;
        let class = regs[6] >> 1;
        let address = ((regs[6] & 1) << 4) | regs[7];
        let data = (regs[5] << 4) | regs[4];
        match class {
            0 => self.ram[usize::from(address) & 0x1F] = data,
            1 => {
                let b = self.ram[usize::from(address) & 0x1F];
                self.tama5.result_lo = b & 0xF;
                self.tama5.result_hi = b >> 4;
            }
            2 => self.tama_clock_cmd(address, data),
            4 => self.tama_page_cmd(regs[7]),
            _ => {}
        }
    }

    /// TAMA5's clock commands (class 2), by address.
    fn tama_clock_cmd(&mut self, address: u8, data: u8) {
        match address {
            0x00 => {
                for page in 0..4 {
                    let r = self.tama_page(page, 0xD);
                    self.tama_set_page(page, 0xD, r & !8);
                }
            }
            0x01 => {
                self.tama_set_page(0, 0, 0);
                self.tama_set_page(0, 1, 0);
                for page in 0..4 {
                    let r = self.tama_page(page, 0xD);
                    self.tama_set_page(page, 0xD, r | 8);
                }
                self.tama5.sub = 0;
            }
            0x04 => {
                self.tama_set_page(0, 2, data & 0xF);
                self.tama_set_page(0, 3, (data >> 4) & 7);
            }
            0x05 => {
                self.tama_set_page(0, 4, data & 0xF);
                self.tama_set_page(0, 5, (data >> 4) & 3);
            }
            0x06 => {
                self.tama5.result_lo = self.tama_page(0, 2);
                self.tama5.result_hi = self.tama_page(0, 3);
            }
            0x07 => {
                self.tama5.result_lo = self.tama_page(0, 4);
                self.tama5.result_hi = self.tama_page(0, 5);
            }
            0x10 => {
                for page in 0..4 {
                    let r = self.tama_page(page, 0xD);
                    self.tama_set_page(page, 0xD, r & !4);
                }
            }
            0x11 => {
                for page in 0..4 {
                    let r = self.tama_page(page, 0xD);
                    self.tama_set_page(page, 0xD, r | 4);
                }
            }
            _ => {
                self.tama5.result_lo = address & 0xF;
                self.tama5.result_hi = address >> 4;
            }
        }
    }

    /// TAMA5's clock-page access (class 4): register 7 even writes a page, odd reads the timer
    /// page back through the $C port ($D reads $FF there, as in mGBA).
    fn tama_page_cmd(&mut self, reg7: u8) {
        let index = usize::from(self.tama5.regs[4]);
        match reg7 {
            0 | 2 | 4 | 6 => {
                if index >= 0xD {
                    return;
                }
                let page = usize::from(reg7 / 2);
                let mask = if page < 2 { Tama5::MASK[page][index] } else { 0xF };
                let v = self.tama5.regs[5] & mask;
                self.tama_set_page(page, index, v);
            }
            1 | 3 | 5 | 7 => {
                let v = if index > 0xD { 0 } else { self.tama_page(0, index) };
                self.tama5.result_lo = v;
                self.tama5.result_hi = 0xF;
            }
            _ => {}
        }
    }

    /// One second of the TAMA5 clock, in BCD, when the timer is enabled.
    fn tama_tick_second(&mut self) {
        if !self.tama_timer_enabled() {
            return;
        }
        let bcd = |c: &Cart, o: usize, t: usize| u32::from(c.tama_page(0, o)) + 10 * u32::from(c.tama_page(0, t));
        let mut total = bcd(self, 4, 5) * 3600 + bcd(self, 2, 3) * 60 + bcd(self, 0, 1) + 1;
        let mut day = bcd(self, 7, 8);
        let mut month = bcd(self, 9, 0xA);
        let mut year = bcd(self, 0xB, 0xC);
        if total >= 86_400 {
            total -= 86_400;
            let dim = [31u32, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][(month.max(1).min(12) - 1) as usize]
                + u32::from(month == 2 && year % 4 == 0);
            day += 1;
            if day > dim {
                day = 1;
                month += 1;
                if month > 12 {
                    month = 1;
                    year = (year + 1) % 100;
                }
            }
            let wd = self.tama_page(0, 6);
            self.tama_set_page(0, 6, (wd + 1) % 7);
        }
        let set = |c: &mut Cart, o: usize, t: usize, v: u32| {
            c.tama_set_page(0, o, (v % 10) as u8);
            c.tama_set_page(0, t, ((v / 10) % 10) as u8);
        };
        set(self, 0, 1, total % 60);
        set(self, 2, 3, (total / 60) % 60);
        set(self, 4, 5, total / 3600);
        set(self, 7, 8, day);
        set(self, 9, 0xA, month);
        set(self, 0xB, 0xC, year);
    }

    fn ram_index(&self, addr: u16) -> Option<usize> {
        if self.ram.is_empty() {
            return None;
        }
        let bank = match self.kind {
            MbcKind::None => 0,
            MbcKind::Mbc1 => if self.mode == 1 { self.bank2 as usize } else { 0 },
            // MBC30 has eight RAM banks; plain MBC3 four (the modulo below wraps smaller RAM).
            MbcKind::Mbc3 => (self.ram_bank & 7) as usize,
            MbcKind::HuC1 => self.ram_bank as usize,
            MbcKind::Mbc2 => return Some(addr as usize & 0x1FF),
            MbcKind::Mbc5 => self.ram_bank as usize,
            MbcKind::Mbc7 => return None,
            MbcKind::Mbc6 => return None, // Two 4 KiB windows, handled by `mbc6_read`/`mbc6_write`.
            MbcKind::Mmm01 => self.mmm01.ram_bank(),
            MbcKind::Tama5 => return None, // Ports, handled by `tama_read`/`tama_write`.
            MbcKind::HuC3 => (self.ram_bank & 3) as usize,
            MbcKind::Camera => {
                if self.cam_regs() {
                    return None;
                }
                self.ram_bank as usize & 0x0F
            }
        };
        Some((bank * 0x2000 + (addr as usize & 0x1FFF)) % self.ram.len())
    }

    /// A byte written into cartridge RAM bank `bank`, the game's own bank put back afterwards:
    /// how a GameShark writes a code meant for one bank (cheats.rs). The cartridge's own rules
    /// still apply, so nothing lands while the game has its RAM switched off.
    pub fn write_ram_in_bank(&mut self, bank: u8, addr: u16, v: u8) {
        let keep = (self.ram_bank, self.bank2, self.mode);
        if self.kind == MbcKind::Mbc1 {
            self.bank2 = bank & 3;
            self.mode = 1;
        } else {
            self.ram_bank = bank;
        }
        self.write_ram(addr, v);
        (self.ram_bank, self.bank2, self.mode) = keep;
    }

    /// The Game Boy Camera's registers are in the A000-BFFF window: any RAM bank number with
    /// bit 4 set picks them, not only bank $10.
    fn cam_regs(&self) -> bool {
        self.kind == MbcKind::Camera && self.ram_bank & 0x10 != 0
    }

    pub fn read_ram(&self, addr: u16) -> u8 {
        if self.kind != MbcKind::None
            && self.kind != MbcKind::HuC3
            && self.kind != MbcKind::Tama5
            && !self.cam_regs()
            && !self.ram_enabled
        {
            return 0xFF;
        }
        if self.kind == MbcKind::Mbc6 {
            return self.mbc6_read(addr);
        }
        if self.kind == MbcKind::Tama5 {
            return self.tama_read(addr);
        }
        if self.kind == MbcKind::Mbc7 {
            return self.mbc7_read(addr);
        }
        if self.kind == MbcKind::HuC3 {
            return self.huc3_read(addr);
        }
        if self.cam_regs() {
            return self.camera_read(addr);
        }
        // While the picture is being taken the cartridge's RAM is unreadable; only A000 above
        // answers, which is how the game knows the camera is still working.
        if self.kind == MbcKind::Camera && self.camera.busy > 0 {
            return 0x00;
        }
        if self.kind == MbcKind::HuC1 && self.mode == 1 {
            return 0xC0; // infrared: no light received
        }
        if self.kind == MbcKind::Mbc2 {
            return self.ram_index(addr).map_or(0xFF, |i| self.ram[i] | 0xF0);
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
        if self.kind != MbcKind::None
            && self.kind != MbcKind::HuC3
            && self.kind != MbcKind::Tama5
            && !self.cam_regs()
            && !self.ram_enabled
        {
            return;
        }
        if self.kind == MbcKind::Mbc6 {
            self.mbc6_write(addr, v);
            return;
        }
        if self.kind == MbcKind::Tama5 {
            self.tama_write(addr, v);
            return;
        }
        if self.kind == MbcKind::Mbc7 {
            self.mbc7_write(addr, v);
            return;
        }
        if self.kind == MbcKind::HuC3 {
            self.huc3_write(addr, v);
            return;
        }
        if self.cam_regs() {
            self.camera_write(addr, v);
            return;
        }
        // The capture has the RAM; writes to it are dropped until the picture is finished.
        if self.kind == MbcKind::Camera && self.camera.busy > 0 {
            return;
        }
        if self.kind == MbcKind::HuC1 && self.mode == 1 {
            return; // the infrared LED
        }
        let v = if self.kind == MbcKind::Mbc2 { v & 0x0F } else { v };
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

    fn huc3_read(&self, addr: u16) -> u8 {
        match self.huc3.select {
            0x0 | 0xA => self.ram_index(addr).map_or(0xFF, |i| self.ram[i]),
            0xC => 0x80 | ((self.huc3.cmd & 0x70) | (self.huc3.result & 0x0F)),
            0xD => 0x81,
            0xE => 0x80 | (self.huc3.ir & 1),
            _ => 0xFF,
        }
    }

    fn huc3_write(&mut self, addr: u16, v: u8) {
        match self.huc3.select {
            0xA => {
                if let Some(i) = self.ram_index(addr) {
                    self.ram[i] = v;
                }
            }
            0xB => self.huc3.cmd = v & 0x7F,
            0xD => {
                if v & 1 == 0 {
                    self.huc3.execute();
                }
            }
            0xE => self.huc3.ir = v & 1,
            _ => {}
        }
    }

    /// Only A000 reads back, and only its lower three bits; every other register reads as 0.
    /// They are mirrored every $80 bytes through A000-BFFF.
    fn camera_read(&self, addr: u16) -> u8 {
        match (addr as usize) & 0x7F {
            0 => self.camera.regs[0] & 7,
            _ => 0x00,
        }
    }

    fn camera_write(&mut self, addr: u16, v: u8) {
        let i = (addr as usize) & 0x7F;
        if i >= self.camera.regs.len() {
            return;
        }
        self.camera.regs[i] = v;
        if i == 0 && v & 1 != 0 {
            self.capture();
            self.camera.regs[0] |= 1;
            self.camera.busy = 2;
        }
    }

    /// Takes a picture: the phone's camera frame through the sensor's own processing and the
    /// controller's dither matrix, into cartridge RAM as 14 rows of 16 Game Boy tiles.
    ///
    /// The order of work is the hardware's (Pan Docs, "Game Boy Camera"): exposure time and the
    /// sensor's narrow output range, the invert bit, the edge filter the registers chose, then
    /// the 4x4 contrast and dithering matrix. With no frame from the phone the sensor sees a
    /// flat mid-grey, so the game shows an even picture instead of one nobody took.
    fn capture(&mut self) {
        let regs = self.camera.regs.clone();
        // A000 bits 1-2 pick which of the sensor's own 1-D filter taps are added and subtracted.
        let (p_bits, m_bits) = match (regs[0] >> 1) & 3 {
            0 => (0u8, 1u8),
            1 => (1, 0),
            _ => (1, 2),
        };
        let n_bit = u32::from(regs[1] & 0x80 != 0);
        let vh = u32::from((regs[1] >> 5) & 3);
        let exposure = i32::from(regs[3]) | i32::from(regs[2]) << 8;
        let alpha = [0.5f32, 0.75, 1.0, 1.25, 2.0, 3.0, 4.0, 5.0][usize::from((regs[4] & 0x70) >> 4)];
        let e3_bit = u32::from(regs[4] & 0x80 != 0);
        let invert = regs[4] & 0x08 != 0;
        let at = |x: usize, y: usize| y * SENSOR_W + x;

        // The sensor: the phone's grey picture (or a flat grey), through the exposure time and
        // the small voltage swing the chip actually puts out, then made signed.
        let flat = self.camera.image.len() != SENSOR_W * SENSOR_H;
        let mut buf = vec![0i32; SENSOR_W * SENSOR_H];
        for (i, px) in buf.iter_mut().enumerate() {
            let grey = if flat { 128 } else { i32::from(self.camera.image[i]) };
            let v = grey * exposure / 0x300;
            let v = (128 + (v - 128) / 8).clamp(0, 255);
            *px = (if invert { 255 - v } else { v }) - 128;
        }

        let mut temp = buf.clone();
        match (n_bit << 3) | (vh << 1) | e3_bit {
            // Plain 1-D filtering, and the same with horizontal edge enhancement first.
            mode @ (0x0 | 0x2) => {
                if mode == 0x2 {
                    for y in 0..SENSOR_H {
                        for x in 0..SENSOR_W {
                            let west = buf[at(x.saturating_sub(1), y)];
                            let east = buf[at((x + 1).min(SENSOR_W - 1), y)];
                            let px = buf[at(x, y)];
                            let edge = (2 * px - west - east) as f32 * alpha;
                            temp[at(x, y)] = (px + edge as i32).clamp(0, 255);
                        }
                    }
                }
                for y in 0..SENSOR_H {
                    for x in 0..SENSOR_W {
                        let south = temp[at(x, (y + 1).min(SENSOR_H - 1))];
                        let px = temp[at(x, y)];
                        let mut v = 0;
                        if p_bits & 1 != 0 {
                            v += px;
                        }
                        if p_bits & 2 != 0 {
                            v += south;
                        }
                        if m_bits & 1 != 0 {
                            v -= px;
                        }
                        if m_bits & 2 != 0 {
                            v -= south;
                        }
                        buf[at(x, y)] = v.clamp(-128, 127);
                    }
                }
            }
            // Two-dimensional edge enhancement, all four neighbours.
            0xE => {
                for y in 0..SENSOR_H {
                    for x in 0..SENSOR_W {
                        let north = buf[at(x, y.saturating_sub(1))];
                        let south = buf[at(x, (y + 1).min(SENSOR_H - 1))];
                        let west = buf[at(x.saturating_sub(1), y)];
                        let east = buf[at((x + 1).min(SENSOR_W - 1), y)];
                        let px = buf[at(x, y)];
                        let edge = (4 * px - north - south - west - east) as f32 * alpha;
                        temp[at(x, y)] = (px + edge as i32).clamp(-128, 127);
                    }
                }
                buf.copy_from_slice(&temp);
            }
            // The real cartridge gives one flat colour here; the sensor's datasheet does not
            // document the combination at all.
            0x1 => buf.fill(-128),
            // Anything else: the sensor passes the picture through unfiltered.
            _ => {}
        }

        // The controller: its matrix picks one of four shades, and the picture goes out as
        // tiles. The darkest shade of grey is the Game Boy's colour 3, so the count is flipped.
        const EXTRA: usize = (SENSOR_H - CAM_H) / 2;
        let mut tiles = [0u8; CAM_H / 8 * 16 * 16];
        for y in 0..CAM_H {
            for x in 0..CAM_W {
                let shade = 3 - (self.camera.matrix(buf[at(x, y + EXTRA)] + 128, x, y) >> 6);
                let base = ((y / 8) * 16 + x / 8) * 16 + (y & 7) * 2;
                let bit = 1 << (7 - (x & 7));
                if shade & 1 != 0 {
                    tiles[base] |= bit;
                }
                if shade & 2 != 0 {
                    tiles[base + 1] |= bit;
                }
            }
        }
        let end = CAM_IMAGE_AT + tiles.len();
        if self.ram.len() >= end {
            self.ram[CAM_IMAGE_AT..end].copy_from_slice(&tiles);
        }
    }

    fn mbc7_read(&self, addr: u16) -> u8 {
        let m = &self.mbc7;
        if !m.ram_enable2 || addr >= 0xB000 {
            return 0xFF;
        }
        match (addr >> 4) & 0xF {
            2 => m.x as u8,
            3 => (m.x >> 8) as u8,
            4 => m.y as u8,
            5 => (m.y >> 8) as u8,
            6 => 0x00,
            8 => u8::from(m.cs) << 7 | u8::from(m.clk) << 6 | u8::from(m.do_bit),
            _ => 0xFF,
        }
    }

    fn eeprom_word(&self, a: u8) -> u16 {
        let i = usize::from(a & 0x7F) * 2;
        u16::from(self.ram[i]) | u16::from(self.ram[i + 1]) << 8
    }

    fn set_eeprom_word(&mut self, a: u8, w: u16) {
        let i = usize::from(a & 0x7F) * 2;
        self.ram[i] = w as u8;
        self.ram[i + 1] = (w >> 8) as u8;
    }

    fn mbc7_write(&mut self, addr: u16, v: u8) {
        if !self.mbc7.ram_enable2 || addr >= 0xB000 {
            return;
        }
        match (addr >> 4) & 0xF {
            0 => self.mbc7.latch_armed = v == 0x55,
            1 => {
                if self.mbc7.latch_armed && v == 0xAA {
                    // Centre $81D0, about $70 to either side for a full tilt.
                    let axis = |t: f32| (0x81D0 as f32 + t.clamp(-1.0, 1.0) * 0x70 as f32) as u16;
                    self.mbc7.x = axis(self.tilt.0);
                    self.mbc7.y = axis(self.tilt.1);
                    self.mbc7.latch_armed = false;
                }
            }
            8 => self.eeprom_pins(v),
            _ => {}
        }
    }

    /// The EEPROM's CS (bit 7), CLK (bit 6) and DI (bit 1) pins; DO comes back on bit 0.
    fn eeprom_pins(&mut self, v: u8) {
        let (cs, clk, di) = (v & 0x80 != 0, v & 0x40 != 0, v & 0x02 != 0);
        if !cs {
            let m = &mut self.mbc7;
            m.cs = false;
            m.clk = clk;
            m.bits_in = 0;
            m.shift_in = 0;
            m.bits_out = 0;
            m.pending = None;
            m.do_bit = true; // ready
            return;
        }
        let rising = clk && !self.mbc7.clk;
        self.mbc7.cs = true;
        self.mbc7.clk = clk;
        if !rising {
            return;
        }
        if self.mbc7.bits_out > 0 {
            let m = &mut self.mbc7;
            m.do_bit = m.shift_out & 0x8000 != 0;
            m.shift_out <<= 1;
            m.bits_out -= 1;
            return;
        }
        let m = &mut self.mbc7;
        if m.bits_in == 0 && !di {
            return; // waiting for the start bit
        }
        m.shift_in = m.shift_in << 1 | u32::from(di);
        m.bits_in += 1;
        if let Some((a, all)) = m.pending {
            if m.bits_in == 16 {
                let w = m.shift_in as u16;
                m.pending = None;
                m.bits_in = 0;
                m.shift_in = 0;
                if self.mbc7.write_enabled {
                    if all {
                        (0..128).for_each(|i| self.set_eeprom_word(i, w));
                    } else {
                        self.set_eeprom_word(a, w);
                    }
                }
                self.mbc7.do_bit = true;
            }
            return;
        }
        // Start bit, 2-bit opcode, 8 address bits.
        if m.bits_in == 11 {
            let op = (m.shift_in >> 8) & 3;
            let a = (m.shift_in & 0xFF) as u8;
            m.bits_in = 0;
            m.shift_in = 0;
            match op {
                0b10 => {
                    let w = self.eeprom_word(a);
                    let m = &mut self.mbc7;
                    m.shift_out = w;
                    m.bits_out = 16;
                    m.do_bit = false; // the dummy 0 before the data
                }
                0b01 => self.mbc7.pending = Some((a, false)),
                0b11 => {
                    if self.mbc7.write_enabled {
                        self.set_eeprom_word(a, 0xFFFF);
                    }
                    self.mbc7.do_bit = true;
                }
                _ => match a >> 6 {
                    0b11 => self.mbc7.write_enabled = true,
                    0b00 => self.mbc7.write_enabled = false,
                    0b10 => {
                        if self.mbc7.write_enabled {
                            (0..128).for_each(|i| self.set_eeprom_word(i, 0xFFFF));
                        }
                    }
                    _ => self.mbc7.pending = Some((0, true)),
                },
            }
        }
    }

    /// `t` T-cycles of real time for the clock.
    pub fn tick(&mut self, t: u32) {
        if self.kind == MbcKind::HuC3 {
            self.rtc.sub += t;
            if self.rtc.sub >= 4_194_304 * 60 {
                self.rtc.sub -= 4_194_304 * 60;
                self.huc3.tick_minute();
            }
        }
        if self.kind == MbcKind::Camera && self.camera.busy > 0 {
            self.camera.busy = self.camera.busy.saturating_sub(1);
            if self.camera.busy == 0 {
                self.camera.regs[0] &= !1;
            }
        }
        // TAMA5's clock is not an MBC3 clock: it ticks while the game is running, from its own
        // accumulator, and its state lives in `ram` (mGBA's 72-byte layout), not the MBC3 block.
        if self.kind == MbcKind::Tama5 {
            self.tama5.sub += t;
            while self.tama5.sub >= 4_194_304 {
                self.tama5.sub -= 4_194_304;
                self.tama_tick_second();
            }
            return;
        }
        if !self.has_rtc || self.kind == MbcKind::HuC3 {
            return;
        }
        self.rtc.sub += t;
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
    fn mbc2_banks_and_four_bit_ram() {
        let mut c = Cart::new(rom(0x06, 16)).unwrap();
        assert!(c.has_battery);
        assert_eq!(c.ram.len(), 512);
        // Address bit 8 set: ROM bank; 0 means 1; only four bits.
        c.write_rom(0x2100, 0x00);
        assert_eq!(c.read_rom(0x4010), 1);
        c.write_rom(0x2100, 0x1F);
        assert_eq!(c.read_rom(0x4010), 15);
        // Bit 8 clear: RAM enable, never a bank write.
        assert_eq!(c.read_ram(0xA000), 0xFF);
        c.write_rom(0x0000, 0x0A);
        assert_eq!(c.read_rom(0x4010), 15);
        c.write_ram(0xA001, 0xAB);
        assert_eq!(c.read_ram(0xA001), 0xFB, "upper nibble reads as 1s");
        assert_eq!(c.read_ram(0xA201), 0xFB, "512 cells mirrored through A000-BFFF");
        assert_eq!(c.read_ram(0xBE01), 0xFB);
        c.write_rom(0x0000, 0x00);
        assert_eq!(c.read_ram(0xA001), 0xFF);
    }

    #[test]
    fn huc1_banks_and_infrared() {
        let mut c = Cart::new(rom(0xFF, 64)).unwrap();
        c.write_rom(0x2000, 0x2A);
        assert_eq!(c.read_rom(0x4010), 0x2A);
        c.write_rom(0x4000, 2);
        c.write_ram(0xA000, 0x77);
        assert_eq!(c.read_ram(0xA000), 0x77);
        c.write_rom(0x0000, 0x0E);
        assert_eq!(c.read_ram(0xA000), 0xC0, "infrared mode sees no light");
        c.write_ram(0xA000, 0x01);
        c.write_rom(0x0000, 0x00);
        assert_eq!(c.read_ram(0xA000), 0x77, "the IR write did not touch RAM");
        c.write_rom(0x4000, 0);
        assert_ne!(c.read_ram(0xA000), 0x77);
    }

    fn huc3_cmd(c: &mut Cart, cmd: u8) {
        c.write_rom(0x0000, 0x0B);
        c.write_ram(0xA000, cmd);
        c.write_rom(0x0000, 0x0D);
        c.write_ram(0xA000, 0x00);
    }

    #[test]
    fn huc3_clock_and_ram() {
        let mut c = Cart::new(rom(0xFE, 8)).unwrap();
        c.write_rom(0x2000, 3);
        assert_eq!(c.read_rom(0x4010), 3);
        c.write_rom(0x0000, 0x0A);
        c.write_rom(0x4000, 1);
        c.write_ram(0xA010, 0x5A);
        assert_eq!(c.read_ram(0xA010), 0x5A);
        huc3_cmd(&mut c, 0x62);
        c.write_rom(0x0000, 0x0C);
        assert_eq!(c.read_ram(0xA000) & 0x0F, 1, "the clock reports ready");
        huc3_cmd(&mut c, 0x42);
        huc3_cmd(&mut c, 0x37);
        huc3_cmd(&mut c, 0x42);
        huc3_cmd(&mut c, 0x10);
        c.write_rom(0x0000, 0x0C);
        assert_eq!(c.read_ram(0xA000) & 0x0F, 7);
        c.huc3.minutes = 61;
        huc3_cmd(&mut c, 0x60);
        huc3_cmd(&mut c, 0x51);
        huc3_cmd(&mut c, 0x40);
        huc3_cmd(&mut c, 0x10);
        c.write_rom(0x0000, 0x0C);
        assert_eq!(c.read_ram(0xA000) & 0x0F, 0xD, "61 minutes, low nybble");
    }

    /// The four shades the dither matrix below picks out, and the grey that produces each.
    const BANDS: [u8; 4] = [0, 64, 152, 255];

    /// A picture the camera cartridge can be given: four vertical bands of 32 pixels, with the
    /// four rows top and bottom that the controller throws away filled with the opposite shade.
    fn test_picture() -> Vec<u8> {
        let mut image = vec![0u8; SENSOR_W * SENSOR_H];
        for y in 0..SENSOR_H {
            for x in 0..SENSOR_W {
                let band = BANDS[x / 32];
                image[y * SENSOR_W + x] =
                    if (4..SENSOR_H - 4).contains(&y) { band } else { 255 - band };
            }
        }
        image
    }

    /// Programs the camera for a straight-through capture: the exposure that leaves the picture
    /// as it came, and a matrix that splits the sensor's output range into the four shades.
    fn set_up_camera(c: &mut Cart) {
        c.write_ram(0xA002, 0x03); // exposure $0300
        c.write_ram(0xA003, 0x00);
        for cell in 0..16u16 {
            for (i, threshold) in [116u8, 124, 132].iter().enumerate() {
                c.write_ram(0xA006 + cell * 3 + i as u16, *threshold);
            }
        }
    }

    #[test]
    fn camera_capture_turns_a_picture_into_tiles() {
        let mut c = Cart::new(rom(0xFC, 4)).unwrap();
        c.write_rom(0x0000, 0x0A);
        // A byte of the game's own, in the 256 bytes the picture must not reach.
        c.write_ram(0xA0FF, 0x5A);
        c.write_rom(0x4000, 0x10);
        c.camera.image = test_picture();
        set_up_camera(&mut c);
        // Bit 1: the sensor hands the pixel straight through. Bit 0 takes the picture.
        c.write_ram(0xA000, 0x03);
        assert_eq!(c.read_ram(0xA000) & 1, 1, "the camera says it is working");
        assert_eq!(c.read_ram(0xA002), 0x00, "only A000 reads back");
        c.write_rom(0x4000, 0x11);
        assert_eq!(c.read_ram(0xA080), c.read_ram(0xA000), "the registers repeat every $80");
        c.write_rom(0x4000, 0);
        assert_eq!(c.read_ram(0xA100), 0x00, "the RAM is unreadable while the picture is taken");
        for _ in 0..4 {
            c.tick(4);
        }
        c.write_rom(0x4000, 0x10);
        assert_eq!(c.read_ram(0xA000) & 1, 0, "the capture finishes");

        c.write_rom(0x4000, 0);
        assert_eq!(c.read_ram(0xA0FF), 0x5A, "the game's own 256 bytes were trampled");
        // The four bands, darkest to lightest, four tiles each, in the two bitplanes.
        for (tile, shade) in [(0u16, 3u8), (4, 2), (8, 1), (12, 0)] {
            let at = 0xA100 + tile * 16;
            let planes = (
                if shade & 1 != 0 { 0xFF } else { 0x00 },
                if shade & 2 != 0 { 0xFF } else { 0x00 },
            );
            assert_eq!((c.read_ram(at), c.read_ram(at + 1)), planes, "band at tile {tile}");
        }
        // Every one of the 14 tile rows is the darkest band, so the four spare rows the sensor
        // sends top and bottom really are dropped instead of showing up in the picture.
        for row in 0..14u16 {
            let at = 0xA100 + row * 16 * 16;
            assert_eq!((c.read_ram(at), c.read_ram(at + 1)), (0xFF, 0xFF), "tile row {row}");
        }
    }

    #[test]
    fn camera_with_no_picture_is_an_even_grey() {
        let mut c = Cart::new(rom(0xFC, 4)).unwrap();
        c.write_rom(0x0000, 0x0A);
        c.write_rom(0x4000, 0x10);
        set_up_camera(&mut c);
        c.write_ram(0xA000, 0x03);
        for _ in 0..4 {
            c.tick(4);
        }
        c.write_rom(0x4000, 0);
        // Mid-grey everywhere: one shade, the same in every tile, and no made-up picture.
        for byte in 0..14 * 16 * 16u16 {
            let want = if byte % 2 == 0 { 0xFF } else { 0x00 };
            assert_eq!(c.read_ram(0xA100 + byte), want, "byte {byte} of the picture");
        }
    }

    #[test]
    fn camera_invert_bit_swaps_dark_for_light() {
        let picture = test_picture();
        let bands = |invert: bool| -> Vec<u8> {
            let mut c = Cart::new(rom(0xFC, 4)).unwrap();
            c.write_rom(0x0000, 0x0A);
            c.write_rom(0x4000, 0x10);
            c.camera.image = picture.clone();
            set_up_camera(&mut c);
            c.write_ram(0xA004, if invert { 0x08 } else { 0x00 });
            c.write_ram(0xA000, 0x03);
            for _ in 0..4 {
                c.tick(4);
            }
            c.write_rom(0x4000, 0);
            // One tile of each band, read back as the Game Boy colour its two bitplanes hold.
            [0u16, 4, 8, 12]
                .iter()
                .map(|t| {
                    u8::from(c.read_ram(0xA100 + t * 16) != 0)
                        | u8::from(c.read_ram(0xA100 + t * 16 + 1) != 0) << 1
                })
                .collect()
        };
        assert_eq!(bands(false), vec![3, 2, 1, 0], "the four bands, darkest to lightest");
        let inverted = bands(true);
        assert_eq!(inverted[0], 0, "the invert bit makes the darkest band the lightest");
        assert_eq!(inverted[3], 3, "and the lightest band the darkest");
    }

    #[test]
    fn camera_matrix_dithers_within_a_tile() {
        let mut c = Cart::new(rom(0xFC, 4)).unwrap();
        c.write_rom(0x0000, 0x0A);
        c.write_rom(0x4000, 0x10);
        // No picture, so an even grey, and the dithering is the only thing that can vary it.
        // Every other column of the 4x4 matrix is given a threshold above that grey.
        for cell in 0..16u16 {
            let high = cell % 2 == 1;
            c.write_ram(0xA006 + cell * 3 + 2, if high { 0xFF } else { 0x00 });
        }
        c.write_ram(0xA000, 0x03);
        for _ in 0..4 {
            c.tick(4);
        }
        c.write_rom(0x4000, 0);
        // The matrix repeats every four pixels, so one flat grey comes out striped.
        assert_eq!(c.read_ram(0xA100), 0b0101_0101, "the matrix did not dither the grey");
        assert_eq!(c.read_ram(0xA101), 0x00);
    }

    #[test]
    fn mbc30_eight_bit_rom_bank_and_eight_ram_banks() {
        let mut r = rom(0x13, 256); // 4 MiB
        r[0x149] = 5; // 64 KiB
        let mut c = Cart::new(r).unwrap();
        c.write_rom(0x2000, 0xC1);
        assert_eq!(c.read_rom(0x4010), 0xC1);
        c.write_rom(0x0000, 0x0A);
        for b in 0..8u8 {
            c.write_rom(0x4000, b);
            c.write_ram(0xA000, 0x40 + b);
        }
        for b in 0..8u8 {
            c.write_rom(0x4000, b);
            assert_eq!(c.read_ram(0xA000), 0x40 + b);
        }
        // Plain MBC3 (2 MiB or less) still masks the bank to seven bits.
        let mut c = Cart::new(rom(0x13, 128)).unwrap();
        c.write_rom(0x2000, 0x81);
        assert_eq!(c.read_rom(0x4010), 1);
    }

    #[test]
    fn mbc7_tilt_latch_and_eeprom_write_read() {
        let mut c = Cart::new(rom(0x22, 8)).unwrap();
        c.write_rom(0x0000, 0x0A);
        c.write_rom(0x4000, 0x40);
        c.tilt = (1.0, -1.0);
        c.write_ram(0xA000, 0x55);
        c.write_ram(0xA010, 0xAA);
        let x = u16::from(c.read_ram(0xA020)) | u16::from(c.read_ram(0xA030)) << 8;
        let y = u16::from(c.read_ram(0xA040)) | u16::from(c.read_ram(0xA050)) << 8;
        assert_eq!((x, y), (0x81D0 + 0x70, 0x81D0 - 0x70));
        let clock = |c: &mut Cart, bits: u32, n: u32| {
            for i in (0..n).rev() {
                let di = if bits >> i & 1 != 0 { 2 } else { 0 };
                c.write_ram(0xA080, 0x80 | di);
                c.write_ram(0xA080, 0xC0 | di);
            }
        };
        let cmd = |c: &mut Cart, op: u32, a: u32| {
            c.write_ram(0xA080, 0x00);
            clock(c, 1 << 10 | op << 8 | a, 11);
        };
        cmd(&mut c, 0b00, 0xC0); // EWEN
        cmd(&mut c, 0b01, 5);
        clock(&mut c, 0xBEEF, 16);
        cmd(&mut c, 0b10, 5);
        let mut w = 0u16;
        for _ in 0..16 {
            c.write_ram(0xA080, 0x80);
            c.write_ram(0xA080, 0xC0);
            w = w << 1 | u16::from(c.read_ram(0xA080) & 1);
        }
        assert_eq!(w, 0xBEEF);
    }

    #[test]
    fn mbc5_rumble_bit_drives_the_motor_not_the_ram_bank() {
        let mut c = Cart::new(rom(0x1E, 8)).unwrap();
        c.write_rom(0x4000, 0x09);
        assert!(c.motor);
        assert_eq!(c.ram_bank, 1);
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

    /// A cartridge cBoy cannot play says so in words a player can read, and never in code. The
    /// app shows the core's own last line when a game is refused, so this is what a tester sees.
    #[test]
    fn an_unsupported_cartridge_is_refused_in_plain_words() {
        for (kind, name) in [(0x15u8, "MBC4"), (0x77, "an unknown one")] {
            let why = Cart::new(rom(kind, 2)).unwrap_err();
            assert!(why.starts_with("This cartridge has a chip cBoy cannot play yet ("), "{why}");
            assert!(why.contains(name), "${kind:02X} should be named as {name}: {why}");
            assert!(why.contains("mGBA"), "say where the game can be played: {why}");
            assert!(!why.contains('$') && !why.contains("0x"), "no codes in a refusal: {why}");
        }
        // MMM01, MBC6 and TAMA5 are now played, not refused.
        for kind in [0x00u8, 0x03, 0x06, 0x0B, 0x0C, 0x0D, 0x10, 0x13, 0x1B, 0x20, 0x22, 0xFC, 0xFD, 0xFE, 0xFF] {
            assert!(Cart::new(rom(kind, 2)).is_ok(), "${kind:02X}");
        }
        let why = Cart::new(vec![0u8; 0x100]).unwrap_err();
        assert!(why.starts_with("This file is too small to be a Game Boy game"), "{why}");
    }

    #[test]
    fn mmm01_menu_then_mapped_banks() {
        // 16 banks (256 KiB): unmapped, the menu is the last 32 KiB.
        let mut c = Cart::new(rom(0x0C, 16)).unwrap();
        assert_eq!(c.read_rom(0x0010), 14, "unmapped $0000 is the second-last bank");
        assert_eq!(c.read_rom(0x4010), 15, "unmapped $4000 is the last bank");
        assert!(!c.mmm01.mapped);
        // The menu selects a game (ROM bank $21, RAM bank 1, mask $10) and enables mapping.
        c.write_rom(0x2000, 0x01);
        c.write_rom(0x4000, 0x21);
        c.write_rom(0x6000, 0x20);
        c.write_rom(0x0000, 0x4A); // RAM enable $A, RAM mask 0, mapping enable bit 6
        assert!(c.mmm01.mapped);
        // $0000 is the game's base bank (fixed bits only): high 2, mid 0, low & mask = 0, so
        // bank 256 of a 16-bank ROM, which wraps to bank 0.
        assert_eq!(c.read_rom(0x0010), 0);
        // $4000 is the game's bank 1 (low 1; the 0 -> 1 correction fires when the unmasked
        // bits are 0, so writing 0 still gives bank 1).
        c.write_rom(0x2000, 0x00);
        assert_eq!(c.read_rom(0x4010), 1);
        // The mask is a write lock: the game cannot change the locked bit 4 of the low bank.
        c.write_rom(0x2000, 0x1F);
        assert_eq!(c.read_rom(0x4010), 0x0F, "locked bit 4 stays 0");
        // RAM banking needs mode 1 (in mode 0 the RAM bank is the fixed, masked base bank).
        c.write_rom(0x6000, 0x01);
        c.write_rom(0x0000, 0x0A);
        c.write_rom(0x4000, 0x01);
        c.write_ram(0xA123, 0x5A);
        assert_eq!(c.read_ram(0xA123), 0x5A);
        c.write_rom(0x4000, 0x00);
        assert_eq!(c.read_ram(0xA123), 0xFF, "RAM bank 0 is a different bank");
    }

    #[test]
    fn mbc6_flash_id_program_and_erase() {
        let mut c = Cart::new(rom(0x20, 2)).unwrap();
        // Flash is off until the chip is enabled.
        assert_eq!(c.read_rom(0x4010), 0, "window A is ROM bank 0 while flash is off");
        c.write_rom(0x0C00, 0x01); // flash enable
        c.write_rom(0x1000, 0x01); // flash write enable (/WP)
        c.write_rom(0x2800, 0x08); // window A shows the flash
        c.write_rom(0x3800, 0x08); // window B shows the flash
        // The unlock addresses are flash addresses: bank A 2 puts flash $5555 at CPU $5555,
        // bank B 1 puts flash $2AAA at CPU $6AAA.
        c.write_rom(0x2000, 0x02);
        c.write_rom(0x3000, 0x01);
        assert_eq!(c.read_rom(0x4010), 0xFF, "erased flash reads $FF");
        // JEDEC ID: $C2/$81.
        c.write_rom(0x5555, 0xAA);
        c.write_rom(0x6AAA, 0x55);
        c.write_rom(0x5555, 0x90);
        assert_eq!(c.read_rom(0x4000), 0xC2);
        assert_eq!(c.read_rom(0x4001), 0x81);
        c.write_rom(0x4000, 0xF0); // exit ID mode
        // Program a byte at flash $10: $AA/$55/$A0, then the byte (bits only clear).
        c.write_rom(0x5555, 0xAA);
        c.write_rom(0x6AAA, 0x55);
        c.write_rom(0x5555, 0xA0);
        c.write_rom(0x2000, 0x00); // bank A 0, so CPU $4010 is flash $10
        c.write_rom(0x4010, 0x5A);
        c.write_rom(0x4010, 0xF0); // exit program mode
        assert_eq!(c.read_rom(0x4010), 0x5A);
        c.write_rom(0x2000, 0x02);
        c.write_rom(0x5555, 0xAA);
        c.write_rom(0x6AAA, 0x55);
        c.write_rom(0x5555, 0xA0);
        c.write_rom(0x2000, 0x00);
        c.write_rom(0x4010, 0xFF); // programming $FF clears nothing
        c.write_rom(0x4010, 0xF0);
        assert_eq!(c.read_rom(0x4010), 0x5A, "flash programming only clears bits");
        // Sector erase: $AA/$55/$80, $AA/$55, then $30 to an address in the sector.
        c.write_rom(0x2000, 0x02);
        c.write_rom(0x5555, 0xAA);
        c.write_rom(0x6AAA, 0x55);
        c.write_rom(0x5555, 0x80);
        c.write_rom(0x5555, 0xAA);
        c.write_rom(0x6AAA, 0x55);
        c.write_rom(0x2000, 0x00);
        c.write_rom(0x4000, 0x30);
        c.write_rom(0x4000, 0xF0);
        assert_eq!(c.read_rom(0x4010), 0xFF, "the sector is erased back to $FF");
        // RAM windows: bank A at $A000, bank B at $B000; both at bank 0 show the same cell.
        c.write_rom(0x0000, 0x0A);
        c.write_ram(0xA000, 0x11);
        c.write_rom(0x0800, 0x01); // RAM bank B 1
        c.write_ram(0xB000, 0x22);
        assert_eq!(c.read_ram(0xA000), 0x11);
        assert_eq!(c.read_ram(0xB000), 0x22);
        c.write_rom(0x0400, 0x01); // RAM bank A 1: the same cell bank B 1 wrote
        assert_eq!(c.read_ram(0xA000), 0x22);
        c.write_rom(0x0400, 0x02);
        assert_eq!(c.read_ram(0xA000), 0xFF, "RAM bank 2 is a different bank");
        // The flash is in the battery save, after the 32 KiB of SRAM.
        assert_eq!(c.ram.len(), 0x8000 + 0x10_0000);
    }

    #[test]
    fn mbc6_flash_program_f0() {
        // Programming $F0 itself works: $F0 is only special as the commit-repeat
        // abort and the post-commit exit, not as streamed program data (Pan Docs
        // MBC6; the NP doc's trigger rule). Bank A 0 puts flash $10/$20/$30/$40 at
        // CPU $4010/$4020/$4030/$4040.
        let mut c = Cart::new(rom(0x20, 2)).unwrap();
        c.write_rom(0x0C00, 0x01); // flash enable
        c.write_rom(0x1000, 0x01); // flash write enable (/WP)
        c.write_rom(0x2800, 0x08); // window A shows the flash
        c.write_rom(0x3800, 0x08); // window B shows the flash
        c.write_rom(0x2000, 0x02);
        c.write_rom(0x3000, 0x01);
        // Program $5A at flash $10, then $F0 at flash $20 (a new offset, so it is
        // data), then exit with a $F0 repeat to $20 (abort/exit) and read both back.
        c.write_rom(0x5555, 0xAA);
        c.write_rom(0x6AAA, 0x55);
        c.write_rom(0x5555, 0xA0);
        c.write_rom(0x2000, 0x00);
        c.write_rom(0x4010, 0x5A);
        c.write_rom(0x4020, 0xF0);
        c.write_rom(0x4020, 0xF0); // repeat to the last offset: abort/exit
        assert_eq!(c.read_rom(0x4010), 0x5A, "earlier byte is untouched");
        assert_eq!(c.read_rom(0x4020), 0xF0, "programming $F0 to a new offset stores $F0");
        // Commit, then exit: program $A5 at flash $40, repeat it (commit), then $F0
        // to a different offset ($30) exits instead of programming (post-commit,
        // $F0 to any address exits).
        c.write_rom(0x2000, 0x02);
        c.write_rom(0x5555, 0xAA);
        c.write_rom(0x6AAA, 0x55);
        c.write_rom(0x5555, 0xA0);
        c.write_rom(0x2000, 0x00);
        c.write_rom(0x4040, 0xA5);
        c.write_rom(0x4040, 0xA5); // commit repeat (non-$F0)
        c.write_rom(0x4030, 0xF0); // post-commit $F0 to a new offset exits
        assert_eq!(c.read_rom(0x4040), 0xA5, "committed byte reads back");
        assert_eq!(c.read_rom(0x4030), 0xFF, "post-commit $F0 exits instead of programming");
        // Committing with $00 must not AND $00 onto the byte: program $3C at flash
        // $50, commit it with a $00 repeat, exit, and read the byte back intact.
        c.write_rom(0x2000, 0x02);
        c.write_rom(0x5555, 0xAA);
        c.write_rom(0x6AAA, 0x55);
        c.write_rom(0x5555, 0xA0);
        c.write_rom(0x2000, 0x00);
        c.write_rom(0x4050, 0x3C);
        c.write_rom(0x4050, 0x00); // commit repeat with $00
        c.write_rom(0x4030, 0xF0); // exit
        assert_eq!(c.read_rom(0x4050), 0x3C, "committing with $00 leaves the byte intact");
    }

    #[test]
    fn tama5_ports_bank_and_clock() {
        let mut c = Cart::new(rom(0xFD, 32)).unwrap();
        assert_eq!(c.ram.len(), 72);
        assert_eq!(c.read_rom(0x4010), 1, "TAMA5 starts on ROM bank 1");
        // Select register 0, write bank low $5; register 1, write bank high $1: bank $15.
        c.write_ram(0xA001, 0x00);
        c.write_ram(0xA000, 0x05);
        c.write_ram(0xA001, 0x01);
        c.write_ram(0xA000, 0x01);
        assert_eq!(c.read_rom(0x4010), 21, "bank $15 of a 32-bank ROM reads its bank number");
        // Write a byte to the 32-byte RAM (class 0, address 3) and read it back (class 1).
        c.write_ram(0xA001, 0x04);
        c.write_ram(0xA000, 0x0A);
        c.write_ram(0xA001, 0x05);
        c.write_ram(0xA000, 0x00);
        c.write_ram(0xA001, 0x06);
        c.write_ram(0xA000, 0x00);
        c.write_ram(0xA001, 0x07);
        c.write_ram(0xA000, 0x03); // executes: write data $0A to RAM[3]
        c.write_ram(0xA001, 0x06);
        c.write_ram(0xA000, 0x02); // class 1
        c.write_ram(0xA001, 0x07);
        c.write_ram(0xA000, 0x03); // executes: select RAM[3] for reading
        c.write_ram(0xA001, 0x0C);
        assert_eq!(c.read_ram(0xA000), 0xFA, "low nybble of $0A");
        c.write_ram(0xA001, 0x0D);
        assert_eq!(c.read_ram(0xA000), 0xF0, "high nybble of $0A");
        // Enable the clock (class 2, address $01), then run two seconds.
        c.write_ram(0xA001, 0x06);
        c.write_ram(0xA000, 0x04);
        c.write_ram(0xA001, 0x07);
        c.write_ram(0xA000, 0x01);
        assert!(c.tama_timer_enabled());
        c.tick(4_194_304 * 2);
        // Read the minutes back (class 2, address $06): still 0, seconds are 2.
        assert_eq!(c.tama_page(0, 0), 2, "two seconds have passed");
        assert_eq!(c.tama_page(0, 1), 0);
    }

    /// Telling a Game Boy cartridge from an Advance one: the logo the console checks, or a header
    /// checksum that adds up.
    #[test]
    fn a_game_boy_header_is_recognised_by_its_logo_or_its_checksum() {
        let mut r = rom(0x1B, 2);
        assert!(!looks_like_gb_rom(&r), "nothing but a cartridge type is not a header");
        r[0x104..0x134].copy_from_slice(&LOGO);
        assert!(looks_like_gb_rom(&r), "the Nintendo logo is enough");
        let mut c = rom(0x1B, 2);
        c[0x14D] = c[0x134..0x14D].iter().fold(0u8, |a, b| a.wrapping_sub(*b).wrapping_sub(1));
        assert!(looks_like_gb_rom(&c), "a header checksum that adds up is enough");
        assert!(!looks_like_gb_rom(&r[..0x14F]), "a file that stops inside the header is not one");
    }

    #[test]
    fn a_short_yellow_is_named_as_cut_off_and_a_full_one_is_not() {
        let mut short = vec![0u8; 997 * 1024];
        short[0x147] = 0x1B; // MBC5, like Yellow
        short[0x148] = 0x05; // 1 MB
        let why = cutoff_reason(&short).unwrap();
        assert_eq!(
            why,
            "This file is cut off. It is 997 KB and the cartridge says 1.0 MB. The missing part is not in the file. Use a complete copy."
        );
        let mut full = vec![0u8; 1024 * 1024];
        full[0x147] = 0x1B;
        full[0x148] = 0x05;
        assert!(cutoff_reason(&full).is_none());
        let mut homebrew = vec![0u8; 0x400];
        homebrew.resize(0x150, 0);
        homebrew[0x148] = 0x00; // claims 32 KB, the usual small-file case
        assert!(cutoff_reason(&homebrew).is_none());
        let cart = Cart::new(short).unwrap();
        assert_eq!(cart.file_len, 997 * 1024);
        assert_eq!(cart.rom.len(), 1024 * 1024, "banking still pads; the phone refuses before this runs");
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

    /// The catch-up for a game closed a long time gives what ticking every second would.
    #[test]
    fn mbc3_clock_catches_up_whole_days_and_carries_past_day_511() {
        let mut r = Rtc { regs: [10, 20, 13, 5, 0], ..Rtc::default() };
        r.advance_seconds(30 * 86_400 + 3_600 + 5);
        assert_eq!(r.regs, [15, 20, 14, 35, 0], "30 days, an hour and 5 s");
        // Day 510 plus 3 days: wraps to day 1 with the carry bit, the 9th day bit clear.
        let mut r = Rtc { regs: [0, 0, 0, 0xFE, 1], ..Rtc::default() };
        r.advance_seconds(3 * 86_400);
        assert_eq!(r.regs, [0, 0, 0, 1, 0x80]);
        // Same answer as ticking one second at a time, across a day boundary.
        let (mut fast, mut slow) = (Rtc { regs: [50, 59, 23, 7, 0], ..Rtc::default() }, Rtc::default());
        slow.regs = fast.regs;
        fast.advance_seconds(2 * 86_400 + 70);
        for _ in 0..2 * 86_400 + 70 {
            slow.tick_second();
        }
        assert_eq!(fast.regs, slow.regs);
        // A halted clock does not move; an out-of-range hour a game wrote wraps like the chip.
        let mut halted = Rtc { regs: [1, 2, 3, 4, 0x40], ..Rtc::default() };
        halted.advance_seconds(1_000_000);
        assert_eq!(halted.regs, [1, 2, 3, 4, 0x40]);
        let (mut odd, mut odd_slow) = (Rtc { regs: [0, 0, 30, 0, 0], ..Rtc::default() }, Rtc::default());
        odd_slow.regs = odd.regs;
        odd.advance_seconds(3 * 86_400);
        for _ in 0..3 * 86_400 {
            odd_slow.tick_second();
        }
        assert_eq!(odd.regs, odd_slow.regs);
    }

    #[test]
    fn huc3_clock_catches_up_in_minutes_and_days() {
        let mut h = HuC3 { minutes: 600, days: 5, ..HuC3::default() };
        h.advance_minutes(60);
        assert_eq!((h.minutes, h.days), (660, 5));
        h.advance_minutes(3 * 1440 + 800);
        assert_eq!((h.minutes, h.days), (20, 9));
    }
}
