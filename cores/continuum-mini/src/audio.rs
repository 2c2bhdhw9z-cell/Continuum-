//! The sound: timer 3 drives the piezo speaker as a pulse wave, high while its count is at or
//! below its pivot, at the volume $2071 sets (off, half or full) unless $2070 mutes it.
//!
//! Sampled as PokeMini samples it (the emulator cMini replaces, in the "direct PWM" form its
//! libretro core uses): 44100 times a second from the timer's state, with the pivot's share of
//! the preset added as a level, so fast pulse widths come out as their average; then, by
//! default, through PokeMini's model of the piezo (three high-pass and three low-pass stages).
//! The samples are handed over the frame they were made in (PokeMini's own buffer holds them
//! back about 80 ms; cMini doesn't).

use crate::timers::Timers;
use serde::{Deserialize, Serialize};

pub const RATE: u32 = 44_100;
/// The sample clock's step per cycle, in 8.24 fixed point (4 MHz to 44100 Hz).
const STEP: i32 = 184_969;
const HALF: i16 = 0x2000;
const FULL: i16 = 0x4000;
/// The pulse width's level range.
const PWM_RANGE: u32 = 0x1FFF;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Audio {
    clock: i32,
    volume: i16,
    pwm_share: i16,
    /// Through the piezo's response (on by default).
    pub piezo: bool,
    high_pass: [i32; 4],
    low_pass: [i32; 4],
    /// This frame's samples, one channel.
    pub samples: Vec<i16>,
}

impl Audio {
    /// Silent, with the piezo's response on.
    pub fn new() -> Self {
        Self { piezo: true, ..Self::default() }
    }

    /// The reset line: silent until the program writes $2070 or $2071 again. (The piezo's own
    /// state carries on.)
    pub fn reset(&mut self) {
        self.clock = 0;
        self.volume = 0;
        self.pwm_share = 0;
    }

    pub fn read(&self, io: &[u8], reg: usize) -> u8 {
        io[reg] & 0x07
    }

    pub fn write(&mut self, io: &mut [u8], reg: usize, value: u8) {
        io[reg] = value & 0x07;
        if io[0x70] & 0x03 != 0 {
            // Muted: the pulse goes, its share stays (as on PokeMini).
            self.volume = 0;
        } else {
            (self.volume, self.pwm_share) = match io[0x71] & 0x03 {
                0 => (0, 0),
                3 => (FULL, 2),
                _ => (HALF, 1),
            };
        }
    }

    /// Moves the sample clock on by `cycles`, taking a sample when it comes round.
    pub fn sync(&mut self, timers: &Timers, cycles: u32) {
        self.clock += STEP * cycles as i32;
        if self.clock >= 0x0100_0000 {
            self.clock -= 0x0100_0000;
            let mut sample = self.level(timers);
            if self.piezo {
                sample = self.through_piezo(i32::from(sample));
            }
            self.samples.push(sample);
        }
    }

    fn level(&self, t: &Timers) -> i16 {
        let top = |v: u32| (v >> 24) as u16;
        let mut count = top(t.count[4]) | top(t.count[5]) << 8;
        let preset = top(t.preset[4]) | top(t.preset[5]) << 8;
        let mut share = if preset != 0 { u32::from(t.t3_pivot) * PWM_RANGE / u32::from(preset) } else { 0 };
        if share > PWM_RANGE {
            share = PWM_RANGE - 1;
        }
        if preset < 128 {
            count = 0;
        }
        let pulse = if count <= t.t3_pivot { i32::from(self.volume) } else { 0 };
        (pulse + share as i32 * i32::from(self.pwm_share)) as i16
    }

    /// The piezo speaker's response, as PokeMini models it.
    fn through_piezo(&mut self, sample: i32) -> i16 {
        const HP: i32 = 40_960;
        const LP: i32 = 4_096;
        const LP_REST: i32 = 65_535 - LP;
        let mut now = [sample, 0, 0, 0];
        for k in 1..4 {
            now[k] = HP.wrapping_mul(now[k - 1].wrapping_add(self.high_pass[k]).wrapping_sub(self.high_pass[k - 1])) >> 16;
        }
        self.high_pass = now;
        let mut s = (now[3] << 2).clamp(-32_768, 32_767);
        self.low_pass[0] = s;
        for k in 1..4 {
            self.low_pass[k] = self.low_pass[k].wrapping_mul(LP).wrapping_add(self.low_pass[k - 1].wrapping_mul(LP_REST)) >> 16;
        }
        s = (self.low_pass[3] << 1).clamp(-32_768, 32_767);
        s as i16
    }
}

/// The libretro core's softening filter (PokeMini's "low pass filter"), carried across frames.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LowPass {
    last: i32,
}

impl LowPass {
    /// `strength` is the setting in percent.
    pub fn run(&mut self, samples: &mut [i16], strength: u8) {
        let a = i32::from(strength) * 65_536 / 100;
        let b = 0x1_0000 - a;
        for s in samples {
            self.last = self.last.wrapping_mul(a).wrapping_add(i32::from(*s).wrapping_mul(b)) >> 16;
            *s = self.last as i16;
        }
    }

    pub fn reset(&mut self) {
        self.last = 0;
    }
}
