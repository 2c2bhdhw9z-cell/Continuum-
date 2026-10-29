//! The machine: the Z80, its bus, and a frame of lines.

use bincode::Options;
use serde::{Deserialize, Serialize};

use crate::bus::Bus;
use crate::vdp::LINE_CYCLES;

/// The Z80's clock on a 60 Hz console (the picture chip's 10.738635 MHz divided by 3) and on a
/// 50 Hz one (10.640685 MHz / 3).
pub const CPU_HZ_NTSC: f64 = 3_579_545.0;
pub const CPU_HZ_PAL: f64 = 3_546_895.0;
use crate::z80::Z80;

#[derive(Clone, Serialize, Deserialize)]
pub struct Sms {
    pub cpu: Z80,
    pub bus: Bus,
    /// Cycles left over from the last line (an instruction runs past a line's end).
    over: u32,
    pub frames: u64,
    /// Sound made since it was last taken, stereo, at `rate`.
    #[serde(skip)]
    pub samples: Vec<i16>,
    #[serde(skip, default = "default_rate")]
    pub rate: u32,
    #[serde(skip)]
    pause_held: bool,
    /// The picture as the screen shows it, handed to the app.
    #[serde(skip)]
    pub shown: Vec<u32>,
    /// The CPU's speed, percent of the real one (a setting, for games that slow down; not saved).
    #[serde(skip, default = "hundred")]
    pub cpu_percent: u32,
    /// Which eye of the 3-D glasses the frame just made is for.
    #[serde(skip)]
    pub frame_left_eye: bool,
}

fn hundred() -> u32 {
    100
}

const STATE_MAGIC: &[u8; 8] = b"CGEARST1";

fn default_rate() -> u32 {
    48_000
}

impl Sms {
    pub fn new(rom: Vec<u8>) -> Sms {
        let mut cpu = Z80::new();
        // As a start-up file leaves it: the stack at the top of work RAM, interrupt mode 1.
        cpu.sp = 0xDFF0;
        cpu.im = 1;
        let mut bus = Bus::new(rom);
        bus.mapper = crate::bus::Mapper::detect(&bus.rom);
        if bus.mapper == crate::bus::Mapper::Codemasters {
            bus.banks = [0, 1, 0];
        }
        Sms { cpu, bus, over: 0, frames: 0, samples: Vec::new(), rate: default_rate(), pause_held: false, shown: Vec::new(), cpu_percent: 100, frame_left_eye: false }
    }

    /// Starts from the console's own start-up program (see `Bus::bios`), as at power-on: the
    /// start-up program on and the cartridge off (a Game Gear's cartridge stays on beneath its
    /// 1 KiB), the CPU from its reset.
    pub fn use_bios(&mut self, bios: Vec<u8>) {
        if bios.is_empty() || self.bus.sg {
            return;
        }
        self.bus.bios = bios;
        self.bus.mem_ctrl = if self.bus.gg { 0xA3 } else { 0xE3 };
        self.cpu = Z80::new();
    }

    /// One line of CPU time, with the picture chip's line events at its start.
    pub fn run_line(&mut self, line: u16) {
        self.bus.vdp.start_line(line);
        // Faster than the real CPU: more of its cycles fit in a line, and the counters the game
        // reads still see the line's own time.
        let k = self.cpu_percent.max(100);
        let budget = LINE_CYCLES * k / 100;
        let mut spent = self.over;
        while spent < budget {
            self.bus.line_cycle = spent * 100 / k;
            self.cpu.set_irq(self.bus.vdp.irq_at(self.bus.line_cycle));
            spent += self.cpu.step(&mut self.bus);
            if self.bus.ext.nmi {
                self.bus.ext.nmi = false;
                self.cpu.nmi();
            }
        }
        self.over = spent - budget;
        self.bus.clock += u64::from(LINE_CYCLES);
        let hz = if self.bus.vdp.pal { CPU_HZ_PAL } else { CPU_HZ_NTSC };
        let start = self.samples.len();
        self.bus.psg.run(LINE_CYCLES, hz, self.rate, &mut self.samples);
        self.bus.fm.mix_into(self.rate, &mut self.samples[start..]);
    }

    /// The console's Pause button: pressing it is a non-maskable interrupt.
    pub fn set_pause(&mut self, held: bool) {
        if held && !self.pause_held {
            self.cpu.nmi();
        }
        self.pause_held = held;
    }

    pub fn save_state(&self) -> Vec<u8> {
        let mut out = STATE_MAGIC.to_vec();
        out.extend(bincode::serialize(self).unwrap_or_default());
        out
    }

    /// Loads a state; the cartridge itself is not in it and stays.
    pub fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        let body = data.strip_prefix(STATE_MAGIC.as_slice()).ok_or("not a cGear state")?;
        let mut next: Sms = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .allow_trailing_bytes()
            .with_limit(body.len() as u64)
            .deserialize(body)
            .map_err(|e| e.to_string())?;
        next.bus.rom = std::mem::take(&mut self.bus.rom);
        // The cheat codes the app switched on stay on (they're not in the state).
        next.bus.cheats = std::mem::take(&mut self.bus.cheats);
        next.rate = self.rate;
        next.bus.vdp.frame = std::mem::take(&mut self.bus.vdp.frame);
        // The app keeps pointers to these (cheats, achievements, the battery save), so the state
        // is copied into them rather than replacing them.
        let keep = |old: &mut Vec<u8>, new: &mut Vec<u8>| {
            if old.len() == new.len() {
                old.copy_from_slice(new);
                std::mem::swap(old, new);
            }
        };
        keep(&mut self.bus.ram, &mut next.bus.ram);
        keep(&mut self.bus.cart_ram, &mut next.bus.cart_ram);
        keep(&mut self.bus.vdp.vram, &mut next.bus.vdp.vram);
        *self = next;
        Ok(())
    }

    pub fn run_frame(&mut self) {
        // The eye this frame is for: games set the glasses in the blank before they draw it.
        self.frame_left_eye = self.bus.glasses_left;
        let clock = self.bus.clock;
        if !self.bus.ext.remote {
            crate::link::alone(&mut self.bus.ext, clock);
        }
        for line in 0..self.bus.vdp.lines() {
            self.run_line(line);
            if self.bus.ext.remote {
                let now = self.bus.clock;
                crate::link::remote_line(&mut self.bus.ext, now);
            }
        }
        self.frames += 1;
    }

    /// A frame of two Game Gears joined by the Gear-to-Gear cable, a line of each in turn, the
    /// cable brought up to date between lines (link.rs).
    pub fn run_frame_linked(&mut self, other: &mut Sms) {
        for line in 0..self.bus.vdp.lines() {
            self.run_line(line);
            other.run_line(line);
            let (ta, tb) = (self.bus.clock, other.bus.clock);
            crate::link::exchange(&mut self.bus.ext, ta, &mut other.bus.ext, tb);
        }
        self.frames += 1;
        other.frames += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LD A,$42 / LD ($C000),A / JR -2, from $0000.
    fn looping() -> Sms {
        let mut rom = vec![0u8; 0x8000];
        rom[..7].copy_from_slice(&[0x3E, 0x42, 0x32, 0x00, 0xC0, 0x18, 0xFE]);
        Sms::new(rom)
    }

    #[test]
    fn a_frame_takes_a_frame_of_cycles_and_makes_a_frame_of_sound() {
        let mut m = looping();
        m.run_frame();
        assert_eq!(m.bus.ram[0], 0x42);
        let per_frame = 48_000.0 / (CPU_HZ_NTSC / (228.0 * 262.0));
        assert!((m.samples.len() as f64 / 2.0 - per_frame).abs() < 2.0, "{}", m.samples.len());
    }

    #[test]
    fn a_state_comes_back_and_keeps_the_cartridge() {
        let mut m = looping();
        m.run_frame();
        let s = m.save_state();
        m.bus.ram[0] = 0;
        m.load_state(&s).unwrap();
        assert_eq!(m.bus.ram[0], 0x42);
        assert_eq!(m.bus.rom.len(), 0x8000);
        assert!(m.load_state(&s[..10]).is_err());
    }

    #[test]
    fn a_faster_cpu_runs_more_instructions_in_the_same_frame() {
        // INC (HL) at $C000, forever: count how often it runs in a frame.
        let mut rom = vec![0u8; 0x8000];
        rom[..6].copy_from_slice(&[0x21, 0x00, 0xC0, 0x34, 0x18, 0xFD]);
        let sound = |pct: u32| {
            let mut m = Sms::new(rom.clone());
            m.cpu_percent = pct;
            m.run_frame();
            m.samples.len()
        };
        assert_eq!(sound(100), sound(200), "the frame lasts as long and makes as much sound");
        // 23 cycles a loop, about 2600 a frame, so the byte wraps: count it line by line.
        let loops = |pct: u32| {
            let mut m = Sms::new(rom.clone());
            m.cpu_percent = pct;
            let mut n = 0u32;
            let mut last = 0u8;
            for line in 0..m.bus.vdp.lines() {
                m.run_line(line);
                n += u32::from(m.bus.ram[0].wrapping_sub(last));
                last = m.bus.ram[0];
            }
            n
        };
        let (a, b) = (loops(100), loops(200));
        assert!((b as f32 / a as f32 - 2.0).abs() < 0.01, "twice the work: {a} {b}");
    }

    /// A Light Phaser game in miniature: the picture all `colour`, then it waits for the gun's
    /// sensor (TH, port $DD bit 6) and keeps the V and H counters it sees then at $C000.
    fn phaser_game(colour: u8) -> Sms {
        let code: &[u8] = &[
            0xF3,
            0x3E, 0x04, 0xD3, 0xBF, 0x3E, 0x80, 0xD3, 0xBF, // register 0: mode 4
            0x3E, 0x00, 0xD3, 0xBF, 0x3E, 0x87, 0xD3, 0xBF, // register 7: border colour 0
            0x3E, 0x00, 0xD3, 0xBF, 0x3E, 0xC0, 0xD3, 0xBF, 0x3E, colour, 0xD3, 0xBE, // colour 0
            0x3E, 0x10, 0xD3, 0xBF, 0x3E, 0xC0, 0xD3, 0xBF, 0x3E, colour, 0xD3, 0xBE, // colour 16
            0x3E, 0x40, 0xD3, 0xBF, 0x3E, 0x81, 0xD3, 0xBF, // register 1: screen on
            0xDB, 0xDD, 0xE6, 0x40, 0x20, 0xFA, // wait: IN A,($DD) / AND $40 / JR NZ,wait
            0xDB, 0x7E, 0x32, 0x00, 0xC0, 0xDB, 0x7F, 0x32, 0x01, 0xC0, 0x18, 0xFE,
        ];
        let mut rom = vec![0u8; 0x8000];
        rom[..code.len()].copy_from_slice(code);
        let mut m = Sms::new(rom);
        m.bus.ram[0] = 0xEE;
        m.bus.ports[0].device = crate::peripherals::Device::Phaser;
        m.bus.ports[0].aim = (100.0, 50.0);
        m
    }

    #[test]
    fn the_light_phaser_sees_a_bright_picture_where_it_points_and_latches_the_h_counter() {
        let mut m = phaser_game(0x3F);
        m.run_frame();
        assert_eq!(m.bus.ram[0], 50 - crate::peripherals::PHASER_LINES as u8, "the first line it sees");
        assert_eq!(m.bus.ram[1], 50, "the H counter latched at x 100 (two pixels a count)");
        let mut dark = phaser_game(0x00);
        dark.run_frame();
        assert_eq!(dark.bus.ram[0], 0xEE, "a dark picture: nothing seen");
    }

    #[test]
    fn two_game_gears_send_a_byte_down_the_cable() {
        // Serial on at 4800 bps, NMI on receipt. The one whose cartridge has 1 at $7000 sends $A5;
        // both then wait for a byte (port $05 bit 1) and keep it at $C000. The NMI just returns.
        let code: &[u8] = &[
            0xF3, 0x31, 0xF0, 0xDF, // DI / LD SP,$DFF0
            0x3E, 0x38, 0xD3, 0x05, // serial on, NMI on receipt
            0x3A, 0x00, 0x70, 0xB7, 0x28, 0x04, // LD A,($7000) / OR A / JR Z,+4
            0x3E, 0xA5, 0xD3, 0x03, // send $A5
            0xDB, 0x05, 0x32, 0x01, 0xC0, 0xCB, 0x4F, 0x28, 0xF7, // wait for a byte
            0xDB, 0x04, 0x32, 0x00, 0xC0, 0x18, 0xFE, // keep it, stop
        ];
        let make = |sender: bool| {
            let mut rom = vec![0u8; 0x8000];
            rom[..code.len()].copy_from_slice(code);
            rom[0x66] = 0xED;
            rom[0x67] = 0x45; // RETN
            rom[0x7000] = u8::from(sender);
            let mut m = Sms::new(rom);
            m.bus.set_game_gear();
            m.bus.ram[0] = 0xEE;
            m
        };
        let (mut a, mut b) = (make(true), make(false));
        for _ in 0..3 {
            a.run_frame_linked(&mut b);
        }
        assert_eq!(b.bus.ram[0], 0xA5, "the byte arrived");
        assert_eq!(a.bus.ram[0], 0xEE, "and nothing came back");
        let mut alone = make(true);
        for _ in 0..3 {
            alone.run_frame();
        }
        assert_eq!(alone.bus.ram[1] & 0x04, 0x04, "on its own, the status says nobody is there");
    }

    #[test]
    fn a_start_up_program_runs_first_then_hands_over_to_the_cartridge() {
        // The start-up program: writes $42 to $C000, then switches itself off and the cartridge
        // on (port $3E = $AB). The next instruction comes from the cartridge, which has a jump to
        // its start there; its start writes $99 to $C001.
        let mut bios = vec![0u8; 0x2000];
        bios[..9].copy_from_slice(&[0x3E, 0x42, 0x32, 0x00, 0xC0, 0x3E, 0xAB, 0xD3, 0x3E]);
        let mut rom = vec![0u8; 0x8000];
        rom[..7].copy_from_slice(&[0x3E, 0x99, 0x32, 0x01, 0xC0, 0x18, 0xFE]);
        rom[9..12].copy_from_slice(&[0xC3, 0x00, 0x00]);
        let mut m = Sms::new(rom);
        m.use_bios(bios);
        m.run_frame();
        assert_eq!(m.bus.ram[0], 0x42, "the start-up program ran");
        assert_eq!(m.bus.ram[1], 0x99, "then the cartridge");
    }

    #[test]
    fn pause_is_a_non_maskable_interrupt_once_per_press() {
        let mut m = looping();
        m.run_frame();
        m.set_pause(true);
        assert!(m.cpu.nmi_pending);
        m.run_frame();
        m.set_pause(true);
        assert!(!m.cpu.nmi_pending, "held, not pressed again");
    }
}
