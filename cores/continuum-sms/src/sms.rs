//! The machine: the Z80, its bus, and a frame of lines.

use serde::{Deserialize, Serialize};

use crate::bus::Bus;
use crate::vdp::LINE_CYCLES;
use crate::z80::Z80;

#[derive(Clone, Serialize, Deserialize)]
pub struct Sms {
    pub cpu: Z80,
    pub bus: Bus,
    /// Cycles left over from the last line (an instruction runs past a line's end).
    over: u32,
    pub frames: u64,
}

impl Sms {
    pub fn new(rom: Vec<u8>) -> Sms {
        let mut cpu = Z80::new();
        // As a start-up file leaves it: the stack at the top of work RAM, interrupt mode 1.
        cpu.sp = 0xDFF0;
        cpu.im = 1;
        Sms { cpu, bus: Bus::new(rom), over: 0, frames: 0 }
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
    }

    pub fn run_frame(&mut self) {
        for line in 0..self.bus.vdp.lines() {
            self.run_line(line);
        }
        self.frames += 1;
    }
}
