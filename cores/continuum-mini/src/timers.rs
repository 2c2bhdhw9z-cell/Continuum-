//! The clocks: the seconds counter, the 256 Hz timer and its interrupts (32, 8, 2 and 1 Hz), and
//! the three programmable timers. Each programmable timer has a lower and an upper eight-bit
//! half, each with its own prescaler on one of two oscillators (the 4 MHz system clock divided by
//! 2 to 4096, or the 32768 Hz clock divided by 1 to 128), counting down from a preset and
//! reloading when they pass zero; set to sixteen bits, the halves chain into one counter. Timer 3
//! also has a pivot it compares against, which drives the sound.
//!
//! Counted as PokeMini counts them (the emulator cMini replaces): every counter in 8.24 fixed
//! point, moved on by the cycles of each step of the machine, so a counter's top byte is what a
//! program reads. Their registers: $2008-$200B, $2018-$201D, $2030-$2041, $2048-$204F.

use crate::irq::Irq;
use serde::{Deserialize, Serialize};

/// The steps per cycle of each prescaler setting on the system clock (index = the scale
/// register's four bits; the lower eight are off).
const OSC1: [u32; 16] = [
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    16_777_216 / 2,
    16_777_216 / 8,
    16_777_216 / 32,
    16_777_216 / 64,
    16_777_216 / 128,
    16_777_216 / 256,
    16_777_216 / 1024,
    16_777_216 / 4096,
];
/// The same on the 32768 Hz clock, as steps of the system clock's cycles.
const OSC2: [u32; 16] = [
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    16_777_216 / 122,
    16_777_216 / 244,
    16_777_216 / 488,
    16_777_216 / 977,
    16_777_216 / 1953,
    16_777_216 / 3906,
    16_777_216 / 7812,
    16_777_216 / 15625,
];
/// The 256 Hz timer's step per cycle.
const HZ256: u32 = 16_777_216 / 15_625;
/// One count of a top byte.
const ONE: u32 = 0x0100_0000;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Timers {
    /// The seconds counter, 24 bits as read.
    pub seconds: u32,
    /// Cycles toward the next second.
    second_cycles: i32,
    /// Per half, in order timer 1 lower and upper, timer 2, timer 3: the step per cycle, the
    /// count and the preset.
    step: [u32; 6],
    pub count: [u32; 6],
    pub preset: [u32; 6],
    hz256: u32,
    /// Timer 3's count as the pivot compares it (its two top bytes).
    t3_count: u16,
    pub t3_pivot: u16,
    /// The two oscillators running ($2019 bits 4 and 5).
    osc2: bool,
    osc1: bool,
    sixteen: [bool; 3],
    low_on: [bool; 3],
    high_on: [bool; 3],
}

/// Each timer's registers: scale, oscillator select, control low, control high.
const SCALE: [usize; 3] = [0x18, 0x1A, 0x1C];
const OSC: [usize; 3] = [0x19, 0x1B, 0x1D];
const CTRL: [usize; 3] = [0x30, 0x38, 0x48];

impl Timers {
    /// The reset line: everything cleared on power-on; the presets go to their highest either way.
    pub fn reset(&mut self, power_on: bool) {
        if power_on {
            *self = Self::default();
        }
        self.preset = [0xFFFF_FFFF; 6];
    }

    /// Moves every running clock on by `cycles`, raising the interrupts they reach.
    pub fn sync(&mut self, io: &mut [u8], irq: &mut Irq, cycles: u32) {
        if io[0x40] != 0 {
            let before = self.hz256;
            self.hz256 = before.wrapping_add(HZ256.wrapping_mul(cycles));
            let flipped = before ^ self.hz256;
            if flipped & 0x0800_0000 != 0 {
                irq.raise(io, 0x0B);
            }
            if flipped & 0x2000_0000 != 0 {
                irq.raise(io, 0x0C);
            }
            if flipped & 0x8000_0000 != 0 {
                irq.raise(io, 0x0D);
            }
            if before > self.hz256 {
                irq.raise(io, 0x0E);
            }
        }
        if io[0x08] != 0 {
            self.second_cycles += cycles as i32;
            if self.second_cycles >= 4_000_000 {
                self.seconds = self.seconds.wrapping_add(1);
                self.second_cycles -= 4_000_000;
            }
        }
        self.run_pair(io, irq, 0, cycles, 0x08, 0x07);
        self.run_pair(io, irq, 1, cycles, 0x06, 0x05);
        self.run_timer3(io, irq, cycles);
    }

    /// Counts a half down; true when it passed zero.
    fn down(&mut self, half: usize, by: u32) -> bool {
        let before = self.count[half];
        self.count[half] = before.wrapping_sub(by);
        before < self.count[half]
    }

    fn run_pair(&mut self, io: &mut [u8], irq: &mut Irq, t: usize, cycles: u32, low_irq: u8, high_irq: u8) {
        let (a, b) = (2 * t, 2 * t + 1);
        if self.sixteen[t] {
            if self.low_on[t] && self.down(a, self.step[a].wrapping_mul(cycles)) && self.down(b, ONE) {
                self.count[a] = self.preset[a];
                self.count[b] = self.preset[b];
                irq.raise(io, high_irq);
            }
        } else {
            if self.low_on[t] && self.down(a, self.step[a].wrapping_mul(cycles)) {
                self.count[a] = self.preset[a];
                irq.raise(io, low_irq);
            }
            if self.high_on[t] && self.down(b, self.step[b].wrapping_mul(cycles)) {
                self.count[b] = self.preset[b];
                irq.raise(io, high_irq);
            }
        }
    }

    /// Timer 3: as the others, with no interrupt of its lower half, and the pivot interrupt when
    /// the count comes down through the pivot.
    fn run_timer3(&mut self, io: &mut [u8], irq: &mut Irq, cycles: u32) {
        let pivot_passed = |before: u16, now: u16, pivot: u16| before > pivot && now <= pivot;
        if self.sixteen[2] {
            if self.low_on[2] {
                if self.down(4, self.step[4].wrapping_mul(cycles)) {
                    if self.down(5, ONE) {
                        self.count[4] = self.preset[4];
                        self.count[5] = self.preset[5];
                        irq.raise(io, 0x09);
                    }
                    self.t3_count = (self.t3_count & 0x00FF) | (((self.count[5] >> 24) as u16) << 8);
                }
                let before = self.t3_count;
                self.t3_count = (self.t3_count & 0xFF00) | (self.count[4] >> 24) as u16;
                if pivot_passed(before, self.t3_count, self.t3_pivot) {
                    irq.raise(io, 0x0A);
                }
            }
        } else {
            if self.low_on[2] {
                if self.down(4, self.step[4].wrapping_mul(cycles)) {
                    self.count[4] = self.preset[4];
                }
                self.t3_count = (self.t3_count & 0xFF00) | (self.count[4] >> 24) as u16;
            }
            if self.high_on[2] {
                if self.down(5, self.step[5].wrapping_mul(cycles)) {
                    self.count[5] = self.preset[5];
                    irq.raise(io, 0x09);
                }
                let before = self.t3_count;
                self.t3_count = (self.t3_count & 0x00FF) | (((self.count[5] >> 24) as u16) << 8);
                if pivot_passed(before, self.t3_count, self.t3_pivot) {
                    irq.raise(io, 0x0A);
                }
            }
        }
    }

    /// A timer's prescalers, from its scale and oscillator registers and the oscillators on.
    fn rescale(&mut self, io: &[u8], t: usize) {
        let (scale, osc) = (io[SCALE[t]], io[OSC[t]]);
        let pick = |second: bool, k: u8, osc2: bool, osc1: bool| {
            if second {
                if osc2 {
                    OSC2[k as usize]
                } else {
                    0
                }
            } else if osc1 {
                OSC1[k as usize]
            } else {
                0
            }
        };
        self.step[2 * t] = pick(osc & 1 != 0, scale & 0x0F, self.osc2, self.osc1);
        self.step[2 * t + 1] = pick(osc & 2 != 0, scale >> 4, self.osc2, self.osc1);
    }

    pub fn read(&self, io: &[u8], reg: usize) -> u8 {
        let top = |v: u32| (v >> 24) as u8;
        match reg {
            0x08 => io[0x08] & 0x01,
            0x09 => self.seconds as u8,
            0x0A => (self.seconds >> 8) as u8,
            0x0B => (self.seconds >> 16) as u8,
            0x18 | 0x1A | 0x1C => io[reg],
            0x19 => io[reg] & 0x33,
            0x1B | 0x1D => io[reg] & 0x03,
            0x30 | 0x38 | 0x48 => {
                if io[reg] & 0x80 != 0 {
                    io[reg] & 0x85
                } else {
                    io[reg] & 0x0D
                }
            }
            0x31 | 0x39 | 0x49 => {
                if io[reg - 1] & 0x80 != 0 {
                    io[reg] & 0x08
                } else {
                    io[reg] & 0x0D
                }
            }
            0x32 => top(self.preset[0]),
            0x33 => top(self.preset[1]),
            0x3A => top(self.preset[2]),
            0x3B => top(self.preset[3]),
            0x4A => top(self.preset[4]),
            0x4B => top(self.preset[5]),
            0x34 | 0x35 | 0x3C | 0x3D => io[reg],
            0x4C => self.t3_pivot as u8,
            0x4D => (self.t3_pivot >> 8) as u8,
            0x36 => top(self.count[0]),
            0x37 => top(self.count[1]),
            0x3E => top(self.count[2]),
            0x3F => top(self.count[3]),
            0x4E => top(self.count[4]),
            0x4F => top(self.count[5]),
            0x40 => io[0x40],
            0x41 => top(self.hz256),
            // $201E and $201F answer with their own address byte.
            _ => reg as u8,
        }
    }

    pub fn write(&mut self, io: &mut [u8], reg: usize, value: u8) {
        match reg {
            0x08 => {
                if value & 0x02 != 0 {
                    self.seconds = 0;
                }
                io[0x08] = value & 0x01;
            }
            0x18 | 0x1A | 0x1C => {
                io[reg] = value;
                self.rescale(io, (reg - 0x18) / 2);
            }
            0x19 => {
                io[reg] = value & 0x33;
                self.osc2 = value & 0x10 != 0;
                self.osc1 = value & 0x20 != 0;
                for t in 0..3 {
                    self.rescale(io, t);
                }
            }
            0x1B | 0x1D => {
                io[reg] = value & 0x03;
                self.rescale(io, (reg - 0x18) / 2);
            }
            0x30 | 0x38 | 0x48 => {
                let t = CTRL.iter().position(|&r| r == reg).expect("a control register");
                io[reg] = value & 0x8D;
                self.low_on[t] = value & 0x04 != 0;
                self.sixteen[t] = io[reg] & 0x80 != 0;
                if value & 0x02 != 0 {
                    self.count[2 * t] = self.preset[2 * t];
                    if self.sixteen[t] {
                        self.count[2 * t + 1] = self.preset[2 * t + 1];
                    }
                }
            }
            0x31 | 0x39 | 0x49 => {
                let t = CTRL.iter().position(|&r| r == reg - 1).expect("a control register");
                io[reg] = value & 0x0D;
                self.high_on[t] = value & 0x04 != 0;
                if io[reg - 1] & 0x80 == 0 && value & 0x02 != 0 {
                    self.count[2 * t + 1] = self.preset[2 * t + 1];
                }
            }
            0x32 => self.preset[0] = u32::from(value) << 24,
            0x33 => self.preset[1] = u32::from(value) << 24,
            0x3A => self.preset[2] = u32::from(value) << 24,
            0x3B => self.preset[3] = u32::from(value) << 24,
            0x4A => self.preset[4] = u32::from(value) << 24,
            0x4B => self.preset[5] = u32::from(value) << 24,
            0x34 | 0x35 | 0x3C | 0x3D => io[reg] = value,
            0x4C => self.t3_pivot = (self.t3_pivot & 0xFF00) | u16::from(value),
            0x4D => self.t3_pivot = (self.t3_pivot & 0x00FF) | u16::from(value) << 8,
            0x40 => {
                io[0x40] = value & 0x01;
                if value & 0x02 != 0 {
                    self.hz256 = 0;
                }
            }
            // The counts, the seconds and $201E/$201F take no writes.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_half_counts_down_from_its_preset_and_interrupts_when_it_passes_zero() {
        let mut io = [0u8; 256];
        let mut irq = Irq::default();
        let mut t = Timers::default();
        t.reset(true);
        // Timer 1 lower half: system clock / 2, preset 3, enabled and reloaded.
        t.write(&mut io, 0x19, 0x20);
        t.write(&mut io, 0x18, 0x08);
        t.write(&mut io, 0x32, 3);
        t.write(&mut io, 0x30, 0x06);
        assert_eq!(t.read(&io, 0x36), 3);
        io[0x20] = 0x0C;
        io[0x23] = 0x04;
        // Two cycles a count: 6 cycles reach 0, 8 pass it.
        t.sync(&mut io, &mut irq, 6);
        assert_eq!((t.read(&io, 0x36), irq.accepted), (0, None));
        t.sync(&mut io, &mut irq, 2);
        assert_eq!(t.read(&io, 0x36), 3, "reloaded");
        assert_eq!(irq.accepted, Some(0x10));
    }
}
