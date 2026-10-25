//! The machine: the Z80, its bus, and a frame of lines.

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
}

fn default_rate() -> u32 {
    48_000
}

impl Sms {
    pub fn new(rom: Vec<u8>) -> Sms {
        let mut cpu = Z80::new();
        // As a start-up file leaves it: the stack at the top of work RAM, interrupt mode 1.
        cpu.sp = 0xDFF0;
        cpu.im = 1;
        Sms { cpu, bus: Bus::new(rom), over: 0, frames: 0, samples: Vec::new(), rate: default_rate() }
    }

    /// One line of CPU time, with the picture chip's line events at its start.
    pub fn run_line(&mut self, line: u16) {
        self.bus.vdp.start_line(line);
        let mut spent = self.over;
        while spent < LINE_CYCLES {
            self.cpu.set_irq(self.bus.vdp.irq());
            spent += self.cpu.step(&mut self.bus);
        }
        self.over = spent - LINE_CYCLES;
        let hz = if self.bus.vdp.pal { CPU_HZ_PAL } else { CPU_HZ_NTSC };
        self.bus.psg.run(LINE_CYCLES, hz, self.rate, &mut self.samples);
    }

    pub fn run_frame(&mut self) {
        for line in 0..self.bus.vdp.lines() {
            self.run_line(line);
        }
        self.frames += 1;
    }
}
