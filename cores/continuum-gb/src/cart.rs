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
    /// Tilt for MBC7, -1..1 each axis (from the core's sensor input). Not saved.
    #[serde(skip)]
    pub tilt: (f32, f32),
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
            0x22 => (MbcKind::Mbc7, true, false),
            other => return Err(format!("cartridge type ${other:02X} is not supported yet")),
        };
        let mut ram_len = if kind == MbcKind::Mbc7 { 256 } else { ram_size(rom[0x149]) };
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
            ram_enabled: false,
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
            tilt: (0.0, 0.0),
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
            MbcKind::Mbc3 | MbcKind::Mbc5 | MbcKind::Mbc7 => {
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
            MbcKind::Mbc7 => match addr {
                0x0000..=0x1FFF => self.ram_enabled = v == 0x0A,
                0x2000..=0x3FFF => self.rom_bank = u16::from(v & 0x7F),
                0x4000..=0x5FFF => self.mbc7.ram_enable2 = v == 0x40,
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
            MbcKind::Mbc7 => return None,
        };
        Some((bank * 0x2000 + (addr as usize & 0x1FFF)) % self.ram.len())
    }

    pub fn read_ram(&self, addr: u16) -> u8 {
        if self.kind != MbcKind::None && !self.ram_enabled {
            return 0xFF;
        }
        if self.kind == MbcKind::Mbc7 {
            return self.mbc7_read(addr);
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
        if self.kind == MbcKind::Mbc7 {
            self.mbc7_write(addr, v);
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
        if !self.has_rtc {
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
