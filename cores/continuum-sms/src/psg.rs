//! The sound chip: Sega's SN76489, three square-wave tones and a noise channel, inside the
//! picture chip on a Master System and Game Gear and on its own on an SG-1000.
//!
//! Each channel counts down at the CPU clock divided by 16 and flips its output when it reaches
//! zero; the noise channel shifts a 16-bit register, fed back from bits 0 and 3 for white noise
//! (Sega's taps) or from bit 0 alone for periodic noise, at one of three fixed rates or the third
//! tone's. Volumes are 4-bit attenuations in 2 dB steps, 15 being silence. The Game Gear can send
//! each channel left, right or both (port $06). From SMS Power's "SN76489" page by Maxim.

use serde::{Deserialize, Serialize};

/// Volume of each attenuation step, 2 dB apart, as a fraction of full scale.
fn level(att: u8) -> f32 {
    if att >= 15 {
        0.0
    } else {
        10f32.powf(-0.1 * f32::from(att))
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Psg {
    /// Tone periods (10 bits) for channels 0-2, and the noise control (3 bits) as entry 3.
    pub tone: [u16; 4],
    pub att: [u8; 4],
    counter: [u16; 4],
    out: [bool; 4],
    lfsr: u16,
    latched: u8,
    /// Game Gear stereo: bit n sends channel n to the right, bit n+4 to the left.
    pub stereo: u8,
    /// The chip's steps (the CPU clock / 16) not yet turned into samples, and the fraction of a
    /// step the last sample left over.
    frac: f64,
    #[serde(default)]
    carry: f64,
    /// Each channel's volume and the whole chip's, percent (settings; not saved).
    #[serde(skip, default = "full")]
    pub chan_vol: [u8; 4],
    #[serde(skip, default = "hundred")]
    pub volume: u8,
    /// Switched off by a Japanese Master System's audio control port (it still runs).
    #[serde(default)]
    pub muted: bool,
}

fn full() -> [u8; 4] {
    [100; 4]
}

fn hundred() -> u8 {
    100
}

impl Default for Psg {
    fn default() -> Psg {
        Psg { tone: [0; 4], att: [15; 4], counter: [0; 4], out: [true; 4], lfsr: 0x8000, latched: 0, stereo: 0xFF, frac: 0.0, carry: 0.0, chan_vol: [100; 4], volume: 100, muted: false }
    }
}

impl Psg {
    pub fn write(&mut self, v: u8) {
        if v & 0x80 != 0 {
            self.latched = (v >> 4) & 7;
            let ch = usize::from(self.latched >> 1);
            if self.latched & 1 != 0 {
                self.att[ch] = v & 15;
            } else if ch == 3 {
                self.tone[3] = u16::from(v & 7);
                self.lfsr = 0x8000;
            } else {
                self.tone[ch] = (self.tone[ch] & 0x3F0) | u16::from(v & 15);
            }
            return;
        }
        let ch = usize::from(self.latched >> 1);
        if self.latched & 1 != 0 {
            self.att[ch] = v & 15;
        } else if ch == 3 {
            self.tone[3] = u16::from(v & 7);
            self.lfsr = 0x8000;
        } else {
            self.tone[ch] = (self.tone[ch] & 0x00F) | u16::from(v & 0x3F) << 4;
        }
    }

    /// One step of the chip's clock (the CPU clock divided by 16).
    fn clock(&mut self) {
        for ch in 0..3 {
            if self.counter[ch] > 0 {
                self.counter[ch] -= 1;
            }
            if self.counter[ch] == 0 {
                self.counter[ch] = self.tone[ch];
                // A period of 0 or 1 holds the output high: games use it to play samples.
                self.out[ch] = if self.tone[ch] <= 1 { true } else { !self.out[ch] };
                if ch == 2 && self.tone[3] & 3 == 3 {
                    self.noise_edge();
                }
            }
        }
        if self.tone[3] & 3 != 3 {
            if self.counter[3] > 0 {
                self.counter[3] -= 1;
            }
            if self.counter[3] == 0 {
                self.counter[3] = 0x10 << (self.tone[3] & 3);
                self.noise_edge();
            }
        }
    }

    /// The noise channel's input flips; on a rising edge the shift register moves.
    fn noise_edge(&mut self) {
        self.out[3] = !self.out[3];
        if !self.out[3] {
            return;
        }
        let bit = if self.tone[3] & 4 != 0 { (self.lfsr ^ (self.lfsr >> 3)) & 1 } else { self.lfsr & 1 };
        self.lfsr = (self.lfsr >> 1) | bit << 15;
    }

    fn channel(&self, ch: usize) -> f32 {
        let high = if ch == 3 { self.lfsr & 1 != 0 } else { self.out[ch] };
        let v = level(self.att[ch]) * f32::from(self.chan_vol[ch]) / 100.0;
        if high { v } else { -v }
    }

    /// Runs `cpu_cycles` of the chip and appends the stereo samples they make at `rate`, for a
    /// CPU clock of `cpu_hz`.
    pub fn run(&mut self, cpu_cycles: u32, cpu_hz: f64, rate: u32, out: &mut Vec<i16>) {
        let per_sample = cpu_hz / 16.0 / f64::from(rate);
        self.frac += f64::from(cpu_cycles) / 16.0;
        while self.frac >= per_sample {
            self.frac -= per_sample;
            // Each sample covers four or five of the chip's steps, so that over a second they
            // add up to exactly the chip's clock; averaged, so high tones do not alias into noise.
            self.carry += per_sample;
            let n = self.carry.floor() as u32;
            self.carry -= f64::from(n);
            let (mut l, mut r) = (0.0f32, 0.0f32);
            for _ in 0..n {
                self.clock();
                for ch in 0..4 {
                    let v = self.channel(ch);
                    if self.stereo & (0x10 << ch) != 0 {
                        l += v;
                    }
                    if self.stereo & (1 << ch) != 0 {
                        r += v;
                    }
                }
            }
            let k = if self.muted { 0.0 } else { 8191.0 / n.max(1) as f32 * f32::from(self.volume) / 100.0 };
            out.push((l * k) as i16);
            out.push((r * k) as i16);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NTSC: f64 = 3_579_545.0;

    /// Rising edges a second on the left side of channel `ch` alone.
    fn crossings(p: &mut Psg) -> usize {
        let mut out = Vec::new();
        p.run(NTSC as u32, NTSC, 48_000, &mut out);
        out.chunks(2).map(|s| s[0]).collect::<Vec<_>>().windows(2).filter(|w| w[0] <= 0 && w[1] > 0).count()
    }

    #[test]
    fn a_tone_plays_at_the_documented_pitch() {
        let mut p = Psg::default();
        // Channel 0, period 254: 3579545 / (32 * 254) = 440 Hz.
        p.write(0x80 | 0x0E);
        p.write(0x0F);
        p.write(0x90);
        let hz = crossings(&mut p);
        assert!((438..=442).contains(&hz), "{hz}");
    }

    #[test]
    fn periodic_noise_repeats_every_sixteen_shifts_and_white_noise_does_not() {
        let mut p = Psg::default();
        p.write(0xE0); // periodic, fastest rate
        let mut seen = Vec::new();
        for _ in 0..64 {
            p.noise_edge();
            p.noise_edge();
            seen.push(p.lfsr & 1);
        }
        assert_eq!(seen[..16], seen[16..32]);
        p.write(0xE4); // white
        let mut w = Vec::new();
        for _ in 0..64 {
            p.noise_edge();
            p.noise_edge();
            w.push(p.lfsr & 1);
        }
        assert_ne!(w[..16], w[16..32]);
    }

    #[test]
    fn attenuation_is_two_decibels_a_step_and_fifteen_is_silent() {
        assert!((level(1) / level(0) - 0.794).abs() < 0.01);
        assert_eq!(level(15), 0.0);
        let mut p = Psg::default();
        let mut out = Vec::new();
        p.run(10_000, NTSC, 48_000, &mut out);
        assert!(out.iter().all(|s| *s == 0), "everything starts silent");
    }
}
