//! The Pokémon Mini as a whole: its memory map, and its parts clocked together the way PokeMini
//! clocks them (the emulator cMini replaces, as its libretro core runs it): the processor runs
//! until at least 8 cycles have passed (or gives 32 to the PRC while it waits for it), then the
//! timers, the PRC and the sound catch up by those cycles. A frame ends when the PRC's counter
//! comes round, about 72 times a second.
//!
//! The map: the start-up program's 4 KiB at $0000, RAM at $1000 (the PRC's frame buffer, its
//! sprite table and its tile map are the first $3A0 bytes), the I/O registers at $2000, and the
//! cartridge from $2100 to $1FFFFF (a power-of-two size, repeating). Reads of nothing (above
//! $1FFFFF, or an unused register) give the last byte the processor fetched.
//!
//! RAM and the I/O registers sit together in one 8 KiB block, as on PokeMini, which is what
//! achievements read.

use crate::{
    audio::Audio,
    irq::Irq,
    lcd::Lcd,
    minx::{Bus, Minx},
    ports::{self, Ports},
    prc::{Event, Prc},
    timers::Timers,
};
use serde::{Deserialize, Serialize};

pub const CLOCK: u32 = 4_000_000;
/// The processor's cycles between catch-ups (PokeMini's "sync cycles" at its accurate setting).
pub const SYNC: u32 = 8;
/// The cycles a processor waiting for the PRC gives it at a time.
const STALL: u32 = 32;
/// The 8 KiB block: RAM, then the I/O registers.
pub const MEMORY: usize = 0x2000;
const IO: usize = 0x1000;
pub const BIOS_SIZE: usize = 0x1000;

/// The I/O registers as the console starts.
#[rustfmt::skip]
const IO_START: [u8; 256] = [
    0x7F, 0x20, 0x5C, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF,
    0x08, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF,
    0x00, 0x30, 0x02, 0x00, 0x02, 0x00, 0x00, 0x40, 0x00, 0xC0, 0x40, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0xFF, 0xFF,
    0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0xFF, 0xFF,
    0xFF, 0x00, 0xFF, 0x00, 0x01, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0x32, 0x64, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0x00, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x40, 0xFF,
];

/// The colour add-on homebrew can talk to at $20F0-$20F7 (PokeMini's "color PRC"): locked
/// until it hears $5A $CE, then a 16 KiB colour memory and four colour registers. cMini keeps
/// its registers as PokeMini does; the picture stays the console's own.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColorPrc {
    code: u16,
    unlocked: bool,
    access: u8,
    modes: u8,
    second_page: bool,
    address: u16,
    colors: [u8; 4],
    memory: Vec<u8>,
}

impl ColorPrc {
    fn consistent(&self) -> bool {
        self.memory.is_empty() && !self.unlocked || self.memory.len() == 0x4000
    }

    fn reset(&mut self) {
        *self = Self { colors: [0, 0, 0xF0, 0xF0], ..Self::default() };
    }

    fn cell(&self) -> usize {
        usize::from(self.address & 0x3FFF) ^ if self.second_page { 0x2000 } else { 0 }
    }

    fn read(&mut self, reg: usize, cpu: bool) -> u8 {
        if !self.unlocked {
            return 0;
        }
        match reg {
            0xF0 => 0xCE,
            0xF1 => self.address as u8,
            0xF2 => (self.address >> 8) as u8 & 0x3F,
            0xF3 => {
                if cpu && self.access == 3 {
                    self.address = self.address.wrapping_add(1) & 0x3FFF;
                }
                let v = self.memory.get(self.cell()).copied().unwrap_or(0);
                if cpu && self.access == 2 {
                    self.address = self.address.wrapping_sub(1) & 0x3FFF;
                }
                if cpu && self.access == 1 {
                    self.address = self.address.wrapping_add(1) & 0x3FFF;
                }
                v
            }
            0xF4..=0xF7 => self.colors[reg - 0xF4],
            _ => 0,
        }
    }

    fn write(&mut self, reg: usize, value: u8) {
        if !self.unlocked {
            if reg == 0xF0 {
                self.code = (self.code << 8) | u16::from(value);
                if self.code == 0x5ACE {
                    self.unlocked = true;
                    self.memory.resize(0x4000, 0);
                }
            }
            return;
        }
        match reg {
            0xF0 => match value {
                0xA0..=0xA3 => self.access = value & 3,
                0xD0..=0xD2 => self.modes &= !(1 << (value & 3)),
                0xD8..=0xDA => self.modes |= 1 << (value & 3),
                0xCF => {
                    self.unlocked = false;
                    self.code = 0;
                }
                0xF0 => self.second_page = !self.second_page,
                _ => {}
            },
            0xF1 => self.address = (self.address & 0x3F00) | u16::from(value),
            0xF2 => self.address = (self.address & 0x00FF) | u16::from(value & 0x3F) << 8,
            0xF3 => {
                // PokeMini lets the address run to $7FFF here, past its memory; it stays inside.
                if self.access == 3 {
                    self.address = self.address.wrapping_add(1) & 0x3FFF;
                }
                let cell = self.cell();
                self.memory[cell] = value;
                if self.access == 2 {
                    self.address = self.address.wrapping_sub(1) & 0x3FFF;
                }
                if self.access == 1 {
                    self.address = self.address.wrapping_add(1) & 0x3FFF;
                }
            }
            0xF4..=0xF7 => self.colors[reg - 0xF4] = value,
            _ => {}
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Machine {
    pub cpu: Minx,
    /// RAM ($1000-$1FFF) and then the I/O registers ($2000-$20FF), 8 KiB in all.
    pub mem: Vec<u8>,
    /// The start-up program and the cartridge are not part of a saved state.
    #[serde(skip)]
    pub bios: Vec<u8>,
    #[serde(skip)]
    pub cart: Vec<u8>,
    pub irq: Irq,
    pub timers: Timers,
    pub prc: Prc,
    pub lcd: Lcd,
    pub ports: Ports,
    pub audio: Audio,
    pub color: ColorPrc,
    /// How the panel shows its pixels (`lcd::ANALOG`, `THREE_SHADES`, `TWO_SHADES`).
    pub lcd_mode: u8,
    pub cycles: u64,
    frame_over: bool,
}

/// The memory map as the processor or the PRC sees it.
struct Hw<'a> {
    mem: &'a mut [u8],
    bios: &'a [u8],
    cart: &'a [u8],
    irq: &'a mut Irq,
    timers: &'a mut Timers,
    prc: &'a mut Prc,
    lcd: &'a mut Lcd,
    ports: &'a mut Ports,
    audio: &'a mut Audio,
    color: &'a mut ColorPrc,
    /// The last byte the processor fetched.
    ir: u8,
    /// The processor's access, as against the PRC's.
    cpu: bool,
}

impl Hw<'_> {
    fn io_read(&mut self, reg: usize) -> u8 {
        let io = &self.mem[IO..IO + 256];
        match reg {
            0x00..=0x02 => io[reg],
            0x20..=0x2A => self.irq.read(io, reg),
            0x08..=0x0B | 0x18..=0x1F | 0x30..=0x41 | 0x48..=0x4F => self.timers.read(io, reg),
            0x10 | 0x44..=0x47 | 0x50..=0x55 | 0x60..=0x62 => self.ports.read(io, reg),
            0x80..=0x8F => self.prc.read(io, reg),
            0xF0..=0xF7 => self.color.read(reg, self.cpu),
            0xFE | 0xFF => self.lcd.read(reg, self.cpu),
            0x70 | 0x71 => self.audio.read(io, reg),
            _ => self.ir,
        }
    }

    fn io_write(&mut self, reg: usize, value: u8) {
        let io = &mut self.mem[IO..IO + 256];
        match reg {
            0x00..=0x02 => io[reg] = value,
            0x20..=0x2A => self.irq.write(io, reg, value, self.cpu),
            0x08..=0x0B | 0x18..=0x1F | 0x30..=0x41 | 0x48..=0x4F => self.timers.write(io, reg, value),
            0x10 | 0x44..=0x47 | 0x50..=0x55 | 0x60..=0x62 => self.ports.write(io, reg, value),
            0x80..=0x8A => self.prc.write(io, reg, value),
            0xF0..=0xF7 => self.color.write(reg, value),
            0xFE | 0xFF => self.lcd.write(reg, value),
            0x70 | 0x71 => self.audio.write(io, reg, value),
            _ => {}
        }
    }
}

impl Bus for Hw<'_> {
    fn read(&mut self, a: u32) -> u8 {
        match a {
            0x20_0000.. => self.ir,
            0x2100.. => self.cart[a as usize & (self.cart.len() - 1)],
            0x2000.. => self.io_read(a as usize & 0xFF),
            0x1000.. => self.mem[a as usize - 0x1000],
            _ => self.bios[a as usize],
        }
    }

    fn write(&mut self, a: u32, v: u8) {
        match a {
            0x2100.. => {}
            0x2000.. => self.io_write(a as usize & 0xFF, v),
            0x1000.. => self.mem[a as usize - 0x1000] = v,
            _ => {}
        }
    }

    fn fetched(&mut self, byte: u8) {
        self.ir = byte;
    }

    fn irq_check(&mut self, f: u8, u_hold: u8) {
        self.irq.check(&self.mem[IO..IO + 256], f, u_hold);
    }

    fn take_irq(&mut self) -> Option<u8> {
        self.irq.accepted.take()
    }
}

impl Machine {
    /// The console with a cartridge (a whole `.min` image) and a 4 KiB start-up program, as it
    /// starts: PokeMini's power-on state and then its reset (the one its libretro core gives),
    /// which runs the start-up program from its second vector.
    pub fn new(rom: &[u8], bios: Vec<u8>) -> Result<Self, String> {
        if rom.len() <= 0x2100 || rom.len() > 0x20_0000 {
            return Err("This is not a complete Pokémon Mini game file.".into());
        }
        if bios.len() != BIOS_SIZE {
            return Err("The start-up program must be 4096 bytes.".into());
        }
        let mut cart = vec![0xFF; rom.len().next_power_of_two()];
        cart[..rom.len()].copy_from_slice(rom);
        let mut mem = vec![0xFF; MEMORY];
        mem[IO..IO + 256].copy_from_slice(&IO_START);
        let mut m = Self {
            cpu: Minx::new(),
            mem,
            bios,
            cart,
            irq: Irq::default(),
            timers: Timers::default(),
            prc: Prc::default(),
            lcd: Lcd::default(),
            ports: Ports::default(),
            audio: Audio::new(),
            color: ColorPrc::default(),
            lcd_mode: crate::lcd::ANALOG,
            cycles: 0,
            frame_over: false,
        };
        // Power arrives.
        m.timers.reset(true);
        let io = &mut m.mem[IO..IO + 256];
        m.ports.reset(io);
        m.irq.reset(io);
        m.color.reset();
        m.reset();
        Ok(m)
    }

    fn hw(&mut self, cpu: bool, ir: u8) -> Hw<'_> {
        Hw {
            mem: &mut self.mem,
            bios: &self.bios,
            cart: &self.cart,
            irq: &mut self.irq,
            timers: &mut self.timers,
            prc: &mut self.prc,
            lcd: &mut self.lcd,
            ports: &mut self.ports,
            audio: &mut self.audio,
            color: &mut self.color,
            ir,
            cpu,
        }
    }

    /// The reset line (waking from sleep and the reset button give it): every part reset, RAM
    /// and the registers kept, the start-up program run from its second vector.
    pub fn reset(&mut self) {
        let io = &mut self.mem[IO..IO + 256];
        self.timers.reset(false);
        self.irq.reset(io);
        self.ports.reset(io);
        self.prc.reset();
        self.color.reset();
        self.lcd.reset();
        self.audio.reset();
        self.irq.accepted = None;
        let mut cpu = std::mem::take(&mut self.cpu);
        cpu.reset(&mut self.hw(true, cpu.ir), false);
        self.cpu = cpu;
    }

    /// The real clock's time and date (year within the century, month, day, hour, minute,
    /// second), as the console is given it when it starts: into the save chip where the
    /// start-up program keeps it, and the seconds counter started from zero.
    pub fn set_clock(&mut self, fields: [u8; 6]) {
        let io = &mut self.mem[IO..IO + 256];
        self.ports.set_clock(io, fields);
        io[0x08] = 0x01;
        self.timers.seconds = 0;
    }

    /// The buttons held now (bits as $2052 reads them, 1 = held) and the shake, in the order
    /// PokeMini's libretro core hands them over: power, the pad, B, the shake, C, then A.
    pub fn set_buttons(&mut self, held: u8, shaking: bool) {
        let io = &mut self.mem[IO..IO + 256];
        for key in [ports::POWER, ports::UP, ports::DOWN, ports::LEFT, ports::RIGHT, ports::B] {
            self.ports.button(io, &mut self.irq, key, held & key != 0);
        }
        self.ports.shake(io, &mut self.irq, shaking);
        for key in [ports::C, ports::A] {
            self.ports.button(io, &mut self.irq, key, held & key != 0);
        }
    }

    /// One instruction of the processor (or its interrupt's entry, or 8 cycles asleep).
    fn step_cpu(&mut self) -> u32 {
        let mut cpu = std::mem::take(&mut self.cpu);
        let c = cpu.step(&mut self.hw(true, cpu.ir));
        self.cpu = cpu;
        self.cycles += u64::from(c);
        c
    }

    /// The timers, the PRC and (when `sound`) the sound catch up by `cycles`.
    fn catch_up(&mut self, cycles: u32, sound: bool) {
        let io = &mut self.mem[IO..IO + 256];
        self.timers.sync(io, &mut self.irq, cycles);
        match self.prc.advance(io, cycles) {
            Event::Nothing => {}
            Event::Draw => {
                let drawing = self.prc.drawing(io);
                let ir = self.cpu.ir;
                drawing.draw(&mut self.hw(false, ir));
            }
            Event::Copy => {
                let (ram, io) = self.mem.split_at_mut(IO);
                self.lcd.copy_frame(&ram[..0x300]);
                self.irq.raise(&mut io[..256], 0x03);
            }
            Event::Frame { worked } => {
                if worked {
                    self.irq.raise(io, 0x04);
                }
                self.lcd.end_frame(self.lcd_mode, worked);
                self.frame_over = true;
            }
        }
        if sound {
            self.audio.sync(&self.timers, cycles);
        }
    }

    /// One frame, to the PRC counter's turn. The sound of it is in `audio.samples`.
    pub fn run_frame(&mut self) {
        self.audio.samples.clear();
        self.frame_over = false;
        while !self.frame_over {
            let mut spent = 0;
            while spent < SYNC {
                spent += if self.prc.stall { STALL } else { self.step_cpu() };
            }
            self.catch_up(spent, true);
        }
    }

    /// One instruction with everything but the sound caught up straight after it, waiting or
    /// not (the finest step, for comparing with PokeMini instruction by instruction).
    pub fn trace_step(&mut self) -> u32 {
        let c = self.step_cpu();
        self.catch_up(c, false);
        c
    }

    /// The processor's state as PokeMini reports it: running, halted, stopped, or an interrupt
    /// accepted and about to be taken.
    pub fn cpu_status(&self) -> u8 {
        if self.irq.accepted.is_some() {
            crate::minx::ENTERING
        } else {
            self.cpu.status
        }
    }

    /// The motor runs now.
    pub fn rumbling(&self) -> bool {
        self.ports.rumbling
    }

    /// A byte as the processor would read it, without its side effects where it has any.
    pub fn peek(&mut self, a: u32) -> u8 {
        match a {
            0x1000..=0x1FFF => self.mem[a as usize - 0x1000],
            0x2000..=0x20FF => self.mem[IO + (a as usize & 0xFF)],
            _ => {
                let ir = self.cpu.ir;
                self.hw(false, ir).read(a)
            }
        }
    }

    /// Every part at its size, as a machine cMini made has them: what a saved state must show
    /// before it replaces the running one.
    pub fn consistent(&self) -> bool {
        self.mem.len() == MEMORY && self.ports.eeprom.len() == crate::ports::EEPROM_SIZE && self.lcd.consistent() && self.color.consistent()
    }

    pub fn io(&self) -> &[u8] {
        &self.mem[IO..IO + 256]
    }

    /// Another machine's state with this one's cartridge and start-up program (which states
    /// leave out).
    pub fn take_state(&mut self, mut other: Machine) {
        other.bios = std::mem::take(&mut self.bios);
        other.cart = std::mem::take(&mut self.cart);
        *self = other;
    }
}
