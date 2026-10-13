//! The cartridge: ROM, external RAM and the memory bank controller (none, MBC1, MBC3 with its
//! real-time clock, MBC5). The ROM itself is never part of a save state.

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

/// Game Boy Camera registers. `image` is 128 by 112 shades, 0 to 3, from the phone.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Camera {
    regs: Vec<u8>,
    busy: u8,
    #[serde(skip)]
    pub image: Vec<u8>,
}

impl Default for Camera {
    fn default() -> Self {
        Self { regs: vec![0; 54], busy: 0, image: Vec::new() }
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
        0x0B..=0x0D => "MMM01, the one Momotaro Collection uses",
        0x20 => "MBC6, the one Net de Get uses",
        0xFD => "TAMA5, the Tamagotchi one",
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
            0x0F | 0x10 => (MbcKind::Mbc3, true, true),
            0x11..=0x13 => (MbcKind::Mbc3, t == 0x13, false),
            0x19..=0x1E => (MbcKind::Mbc5, t == 0x1B || t == 0x1E, false),
            0x22 => (MbcKind::Mbc7, true, false),
            0x05 | 0x06 => (MbcKind::Mbc2, t == 0x06, false),
            0xFF => (MbcKind::HuC1, true, false),
            0xFE => (MbcKind::HuC3, true, true),
            0xFC => (MbcKind::Camera, true, false),
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
        Ok(Cart {
            cgb_flag: rom[0x143],
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
            MbcKind::HuC3 => (self.ram_bank & 3) as usize,
            MbcKind::Camera => {
                if self.ram_bank == 0x10 {
                    return None;
                }
                self.ram_bank as usize & 0x0F
            }
        };
        Some((bank * 0x2000 + (addr as usize & 0x1FFF)) % self.ram.len())
    }

    pub fn read_ram(&self, addr: u16) -> u8 {
        if self.kind != MbcKind::None
            && self.kind != MbcKind::HuC3
            && !(self.kind == MbcKind::Camera && self.ram_bank == 0x10)
            && !self.ram_enabled
        {
            return 0xFF;
        }
        if self.kind == MbcKind::Mbc7 {
            return self.mbc7_read(addr);
        }
        if self.kind == MbcKind::HuC3 {
            return self.huc3_read(addr);
        }
        if self.kind == MbcKind::Camera && self.ram_bank == 0x10 {
            return self.camera_read(addr);
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
            && !(self.kind == MbcKind::Camera && self.ram_bank == 0x10)
            && !self.ram_enabled
        {
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
        if self.kind == MbcKind::Camera && self.ram_bank == 0x10 {
            self.camera_write(addr, v);
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

    fn camera_read(&self, addr: u16) -> u8 {
        let i = (addr as usize) & 0x3F;
        if i >= self.camera.regs.len() {
            return 0xFF;
        }
        self.camera.regs[i]
    }

    fn camera_write(&mut self, addr: u16, v: u8) {
        let i = (addr as usize) & 0x3F;
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

    /// Writes a 128 by 112 picture into cartridge RAM as Game Boy tiles.
    fn capture(&mut self) {
        let shade = |x: usize, y: usize| -> u8 {
            if self.camera.image.len() == 128 * 112 {
                self.camera.image[y * 128 + x] & 3
            } else {
                (x / 32) as u8
            }
        };
        for ty in 0..14 {
            for tx in 0..16 {
                let tile = ty * 16 + tx;
                for row in 0..8 {
                    let mut lo = 0u8;
                    let mut hi = 0u8;
                    for col in 0..8 {
                        let s = shade(tx * 8 + col, ty * 8 + row);
                        let bit = 7 - col;
                        if s & 1 != 0 {
                            lo |= 1 << bit;
                        }
                        if s & 2 != 0 {
                            hi |= 1 << bit;
                        }
                    }
                    let at = tile * 16 + row * 2;
                    if at + 1 < self.ram.len() {
                        self.ram[at] = lo;
                        self.ram[at + 1] = hi;
                    }
                }
            }
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

    #[test]
    fn camera_capture_writes_tiles() {
        let mut c = Cart::new(rom(0xFC, 4)).unwrap();
        c.write_rom(0x0000, 0x0A);
        c.write_rom(0x4000, 0x10);
        c.write_ram(0xA000, 0x01);
        assert_eq!(c.read_ram(0xA000) & 1, 1);
        for _ in 0..4 {
            c.tick(4);
        }
        assert_eq!(c.read_ram(0xA000) & 1, 0, "the capture finishes");
        c.write_rom(0x4000, 0);
        assert_eq!(c.read_ram(0xA000), 0, "the left of the picture is empty");
        assert_eq!(c.read_ram(0xA040), 0xFF, "the next band is shade 1");
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
        for (kind, name) in [(0x0Bu8, "MMM01"), (0x0D, "MMM01"), (0x20, "MBC6"), (0xFD, "TAMA5"), (0x15, "MBC4"), (0x77, "an unknown one")] {
            let why = Cart::new(rom(kind, 2)).unwrap_err();
            assert!(why.starts_with("This cartridge has a chip cBoy cannot play yet ("), "{why}");
            assert!(why.contains(name), "${kind:02X} should be named as {name}: {why}");
            assert!(why.contains("mGBA"), "say where the game can be played: {why}");
            assert!(!why.contains('$') && !why.contains("0x"), "no codes in a refusal: {why}");
        }
        // Every cartridge cBoy does play is still accepted.
        for kind in [0x00u8, 0x03, 0x06, 0x10, 0x13, 0x1B, 0x22, 0xFC, 0xFE, 0xFF] {
            assert!(Cart::new(rom(kind, 2)).is_ok(), "${kind:02X}");
        }
        let why = Cart::new(vec![0u8; 0x100]).unwrap_err();
        assert!(why.starts_with("This file is too small to be a Game Boy game"), "{why}");
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
