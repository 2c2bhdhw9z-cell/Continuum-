//! Namco's three-voice waveform sound generator (WSG), as on the Pac-Man board: three voices, each
//! stepping through a 32-step, 4-bit waveform read from the 256 × 4-bit sound PROM (82S126 at 1M,
//! eight waveforms of 32 steps), with a 20-bit frequency (the first voice has an extra low nibble)
//! and a 4-bit volume. The CPU writes the registers as nibbles at 5040h-505Fh; the latch bit at
//! 5001h switches the sound on and off.
//!
//! Each voice adds its frequency to a phase counter every step and plays the waveform step the
//! counter's top bits select. The chip steps at 96 kHz (3.072 MHz / 32); this model steps at
//! twice that, 192 kHz, with one more fraction bit, which plays the same pitch (MAME models it
//! the same way, so its output and ours can be compared sample for sample). A voice only steps
//! while the sound is on and its volume isn't zero, as in MAME's model. The voices are summed as
//! (step - 8) × volume, the 4-bit DAC centred on zero.

use serde::{Deserialize, Serialize};

/// The model's internal sample rate: two per chip step.
pub const RATE: u32 = 192_000;

/// The loudest sum three voices can make, in either direction, is 3 × 8 × 15 = 360; this is the
/// divisor that turns the sum into a sample of -1.0 to 1.0 (128 per voice, as MAME scales it).
pub const FULL_SCALE: i32 = 128 * 3;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Voice {
    /// 20 bits: added to `counter` every 192 kHz step.
    pub frequency: u32,
    /// The phase: bits 16-20 pick one of the waveform's 32 steps.
    pub counter: u32,
    pub volume: u8,
    /// Which of the PROM's eight waveforms (0-7).
    pub waveform: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wsg {
    /// The 32 register nibbles as last written (5040h-505Fh).
    pub regs: [u8; 32],
    pub voices: [Voice; 3],
    /// The sound enable latch (5001h).
    pub enabled: bool,
}

impl Default for Wsg {
    fn default() -> Self {
        Wsg::new()
    }
}

impl Wsg {
    pub fn new() -> Wsg {
        Wsg {
            regs: [0; 32],
            voices: Default::default(),
            enabled: false,
        }
    }

    /// A write to 5040h + `reg`. Only the low nibble is wired. 5040h-5044h, 5046h-5049h and
    /// 504Bh-504Eh hold the hardware's own phase counters; a CPU write there is kept in the
    /// register file but doesn't move the voice (MAME models it so).
    pub fn write(&mut self, reg: u8, data: u8) {
        let reg = usize::from(reg & 0x1F);
        let data = data & 0x0F;
        self.regs[reg] = data;
        match reg {
            // Waveform select, three bits, at 05h, 0Ah and 0Fh.
            0x05 | 0x0A | 0x0F => self.voices[reg / 5 - 1].waveform = data & 7,
            // Volume at 15h, 1Ah and 1Fh.
            0x15 | 0x1A | 0x1F => self.voices[(reg - 0x15) / 5].volume = data,
            // Frequency: voice 0 at 10h-14h (five nibbles, low first), voices 1 and 2 at
            // 16h-19h and 1Bh-1Eh (four nibbles, the lowest bit of the 20 always zero).
            0x10..=0x14 | 0x16..=0x19 | 0x1B..=0x1E => {
                let v = if reg < 0x16 {
                    0
                } else if reg < 0x1B {
                    1
                } else {
                    2
                };
                let r = &self.regs;
                let base = 0x11 + v * 5;
                let low = if v == 0 { u32::from(r[0x10]) } else { 0 };
                self.voices[v].frequency = low
                    | u32::from(r[base]) << 4
                    | u32::from(r[base + 1]) << 8
                    | u32::from(r[base + 2]) << 12
                    | u32::from(r[base + 3]) << 16;
            }
            _ => {}
        }
    }

    /// One 192 kHz sample: the sum of the voices as (step - 8) × volume, from -360 to 315.
    #[inline]
    pub fn sample(&mut self, wave_prom: &[u8; 256]) -> i32 {
        if !self.enabled {
            return 0;
        }
        let mut sum = 0;
        for v in &mut self.voices {
            if v.volume == 0 {
                continue;
            }
            let step = (v.counter >> 16) & 0x1F;
            let w = wave_prom[(usize::from(v.waveform) << 5) | step as usize] & 0x0F;
            sum += (i32::from(w) - 8) * i32::from(v.volume);
            v.counter = v.counter.wrapping_add(v.frequency);
        }
        sum
    }
}

/// A 192 kHz sum as a 16-bit sample (full scale 32768 per `FULL_SCALE`).
#[inline]
pub fn to_i16(sum: i32) -> i16 {
    (sum * 32768 / FULL_SCALE).clamp(-32768, 32767) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square_prom() -> [u8; 256] {
        let mut p = [0u8; 256];
        for (i, b) in p.iter_mut().enumerate() {
            // Waveform 0: a square wave, 0 then 15; the others a ramp.
            *b = if i < 32 {
                if i < 16 {
                    0
                } else {
                    15
                }
            } else {
                (i & 15) as u8
            };
        }
        p
    }

    #[test]
    fn frequency_nibbles_and_voice_zero_extra_bits() {
        let mut w = Wsg::new();
        for (reg, val) in [(0x10, 1), (0x11, 2), (0x12, 3), (0x13, 4), (0x14, 5)] {
            w.write(reg, val);
        }
        assert_eq!(w.voices[0].frequency, 0x54321);
        for (reg, val) in [(0x16, 0xA), (0x17, 0xB), (0x18, 0xC), (0x19, 0xD)] {
            w.write(reg, val | 0xF0);
        }
        assert_eq!(w.voices[1].frequency, 0xDCBA0);
        w.write(0x1F, 9);
        w.write(0x0F, 0xE);
        assert_eq!((w.voices[2].volume, w.voices[2].waveform), (9, 6));
    }

    #[test]
    fn silent_until_enabled_and_steps_only_while_audible() {
        let prom = square_prom();
        let mut w = Wsg::new();
        w.write(0x15, 15);
        w.write(0x14, 1); // 0x10000: one waveform step per sample
        assert_eq!(w.sample(&prom), 0);
        assert_eq!(w.voices[0].counter, 0, "no stepping while the sound is off");
        w.enabled = true;
        let first: Vec<i32> = (0..32).map(|_| w.sample(&prom)).collect();
        assert_eq!(first[0], -8 * 15);
        assert_eq!(first[16], 7 * 15);
        w.write(0x15, 0);
        let c = w.voices[0].counter;
        assert_eq!(w.sample(&prom), 0);
        assert_eq!(w.voices[0].counter, c, "no stepping at volume 0");
    }

    #[test]
    fn full_scale() {
        assert_eq!(to_i16(-360), -30720);
        assert_eq!(to_i16(315), 26880);
        assert_eq!(to_i16(0), 0);
    }
}
