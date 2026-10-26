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
        Sms { cpu, bus, over: 0, frames: 0, samples: Vec::new(), rate: default_rate(), pause_held: false, shown: Vec::new(), cpu_percent: 100 }
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
            self.cpu.set_irq(self.bus.vdp.irq());
            spent += self.cpu.step(&mut self.bus);
        }
        self.over = spent - budget;
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
        for line in 0..self.bus.vdp.lines() {
            self.run_line(line);
        }
        self.frames += 1;
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
