//! The FM sound chip of the Japanese Master System (and the FM Sound Unit for the Mark III): Yamaha's
//! YM2413 (OPLL). Nine two-operator FM channels, or six and five drums in rhythm mode, fifteen
//! instruments built in and one the game sets.
//!
//! Written from Yamaha's YM2413 application manual (registers, the instrument parameters, the
//! rhythm mode) and the published working of the OPL family's insides: a quarter sine in a
//! logarithmic table and an exponential table to turn it back (the OPL2 die-shot analysis), the
//! envelope counting in steps of 3/8 dB with rates set by 4 bits and the key scale. The built-in
//! instruments are the chip's own ROM, as dumped from it (the values emu2413, MIT, lists; data, not
//! code). The drums' instrument values are the ones that dump gives for the rhythm channels.
//!
//! In the build since 26 October 2026: ports $F0/$F1/$F2 on a Japanese Master System (setting
//! `cgear_fm`). Its checks: the wave tables, the pitch (437 Hz for F-number 288 in block 4, as
//! Yamaha's formula gives), a built-in instrument's attack and release. NOT yet compared sample by
//! sample with a recording of the real chip; the drums are the roughest part.

use serde::{Deserialize, Serialize};

/// The chip's own instruments: 1-15, then the three rhythm ones (bass drum; hi-hat and snare;
/// tom and top cymbal). Eight bytes each, as the user instrument's registers 0-7.
const ROM: [[u8; 8]; 19] = [
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    [0x71, 0x61, 0x1E, 0x17, 0xD0, 0x78, 0x00, 0x17],
    [0x13, 0x41, 0x1A, 0x0D, 0xD8, 0xF7, 0x23, 0x13],
    [0x13, 0x01, 0x99, 0x00, 0xF2, 0xC4, 0x21, 0x23],
    [0x11, 0x61, 0x0E, 0x07, 0x8D, 0x64, 0x70, 0x27],
    [0x32, 0x21, 0x1E, 0x06, 0xE1, 0x76, 0x01, 0x28],
    [0x31, 0x22, 0x16, 0x05, 0xE0, 0x71, 0x00, 0x18],
    [0x21, 0x61, 0x1D, 0x07, 0x82, 0x81, 0x11, 0x07],
    [0x33, 0x21, 0x2D, 0x13, 0xB0, 0x70, 0x00, 0x07],
    [0x61, 0x61, 0x1B, 0x06, 0x64, 0x65, 0x10, 0x17],
    [0x41, 0x61, 0x0B, 0x18, 0x85, 0xF0, 0x81, 0x07],
    [0x33, 0x01, 0x83, 0x11, 0xEA, 0xEF, 0x10, 0x04],
    [0x17, 0xC1, 0x24, 0x07, 0xF8, 0xF8, 0x22, 0x12],
    [0x61, 0x50, 0x0C, 0x05, 0xD2, 0xF5, 0x40, 0x42],
    [0x01, 0x01, 0x55, 0x03, 0xE9, 0x90, 0x03, 0x02],
    [0x41, 0x41, 0x89, 0x03, 0xF1, 0xE4, 0xC0, 0x13],
    [0x01, 0x01, 0x18, 0x0F, 0xDF, 0xF8, 0x6A, 0x6D],
    [0x01, 0x01, 0x00, 0x00, 0xC8, 0xD8, 0xA7, 0x68],
    [0x05, 0x01, 0x00, 0x00, 0xF8, 0xAA, 0x59, 0x55],
];

/// The chip's own sample rate: its 3.579545 MHz clock divided by 72.
pub const RATE_DIV: f64 = 72.0;

/// The multiplier field's meaning, times two (so "half" is 1).
const MUL2: [u32; 16] = [1, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 20, 24, 24, 30, 30];

fn log_sin(i: usize) -> u16 {
    // -log2(sin) of a quarter wave in 256 steps, in 1/256 units (the OPL family's table).
    let x = ((i as f64) + 0.5) * std::f64::consts::PI / 512.0;
    (-(x.sin().log2()) * 256.0).round() as u16
}

fn exp_table(i: usize) -> u16 {
    ((2f64.powf(i as f64 / 256.0) - 1.0) * 1024.0).round() as u16
}

thread_local! {
    static TABLES: ([u16; 256], [u16; 256]) = {
        let mut ls = [0u16; 256];
        let mut ex = [0u16; 256];
        for i in 0..256 {
            ls[i] = log_sin(i);
            ex[i] = exp_table(i);
        }
        (ls, ex)
    };
}

/// One output of an operator: the phase (10 bits) through the wave, attenuated by `att` (in
/// 1/256 of a factor of two, after the log), as a signed 12-bit level (up to 4090). `half`: the half-sine
/// wave, which is silent for the second half.
fn op_out(phase: u32, att: u32, half: bool) -> i32 {
    TABLES.with(|(ls, ex)| {
        let p = (phase & 0x3FF) as usize;
        let neg = p & 0x200 != 0;
        if neg && half {
            return 0;
        }
        let q = if p & 0x100 != 0 { 0xFF - (p & 0xFF) } else { p & 0xFF };
        let total = u32::from(ls[q]) + att;
        let shift = total >> 8;
        if shift > 13 {
            return 0;
        }
        let v = (((u32::from(ex[0xFF - (total & 0xFF) as usize]) + 1024) << 1) >> shift) as i32;
        if neg { -v } else { v }
    })
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
enum Eg {
    Attack,
    Decay,
    Sustain,
    Release,
    #[default]
    Off,
    /// A key-on first drives the old sound down before a fresh attack starts.
    Damp,
}

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
struct Op {
    phase: u32,
    /// The envelope's attenuation, 0 (loudest) to 128 in 3/8 dB steps.
    env: f32,
    eg: Eg,
    out: [i32; 2],
}

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
struct Channel {
    fnum: u16,
    block: u8,
    key: bool,
    sustain: bool,
    inst: u8,
    vol: u8,
    ops: [Op; 2],
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Fm {
    addr: u8,
    user: [u8; 8],
    ch: [Channel; 9],
    rhythm: u8,
    noise: u32,
    am_phase: u32,
    pm_phase: u32,
    /// Chip samples not yet turned into output samples.
    #[serde(default)]
    carry: f64,
    /// The FM unit is switched on (port $F2 bit 0 on a Japanese Master System).
    pub enabled: bool,
    /// Its volume, percent (a setting; not saved).
    #[serde(skip, default = "hundred")]
    pub volume: u8,
    /// Each channel's volume, percent (settings; not saved).
    #[serde(skip, default = "full")]
    pub chan_vol: [u8; 9],
}

fn full() -> [u8; 9] {
    [100; 9]
}

fn hundred() -> u8 {
    100
}

impl Default for Fm {
    fn default() -> Fm {
        Fm { addr: 0, user: [0; 8], ch: [Channel::default(); 9], rhythm: 0, noise: 1, am_phase: 0, pm_phase: 0, carry: 0.0, enabled: false, volume: 100, chan_vol: [100; 9] }
    }
}

impl Fm {
    pub fn write_addr(&mut self, v: u8) {
        self.addr = v;
    }

    pub fn write_data(&mut self, v: u8) {
        let a = self.addr;
        match a {
            0x00..=0x07 => self.user[usize::from(a)] = v,
            0x0E => {
                let before = self.rhythm;
                self.rhythm = v & 0x3F;
                // Each drum bit is a key: a rising bit starts that drum.
                for bit in 0..5 {
                    let (on_before, on) = (before & (1 << bit) != 0, v & (1 << bit) != 0);
                    if on && !on_before {
                        self.drum_key(bit, true);
                    } else if !on && on_before {
                        self.drum_key(bit, false);
                    }
                }
            }
            0x10..=0x18 => {
                let c = &mut self.ch[usize::from(a - 0x10)];
                c.fnum = (c.fnum & 0x100) | u16::from(v);
            }
            0x20..=0x28 => {
                let n = usize::from(a - 0x20);
                let c = &mut self.ch[n];
                c.fnum = (c.fnum & 0xFF) | u16::from(v & 1) << 8;
                c.block = (v >> 1) & 7;
                c.sustain = v & 0x20 != 0;
                let key = v & 0x10 != 0;
                if key != c.key {
                    c.key = key;
                    for op in c.ops.iter_mut() {
                        if key {
                            op.eg = Eg::Damp;
                        } else if op.eg != Eg::Off {
                            op.eg = Eg::Release;
                        }
                    }
                }
            }
            0x30..=0x38 => {
                let c = &mut self.ch[usize::from(a - 0x30)];
                c.inst = v >> 4;
                c.vol = v & 15;
            }
            _ => {}
        }
    }

    /// Drum keys: bit 4 bass drum, 3 snare, 2 tom, 1 top cymbal, 0 hi-hat.
    fn drum_key(&mut self, bit: u8, on: bool) {
        let (ch, op) = match bit {
            4 => (6, 2),
            3 => (7, 1),
            2 => (8, 0),
            1 => (8, 1),
            _ => (7, 0),
        };
        let targets: &[usize] = if op == 2 { &[0, 1] } else { std::slice::from_ref(&[0usize, 1][op]) };
        for &o in targets {
            let x = &mut self.ch[ch].ops[o];
            if on {
                x.eg = Eg::Damp;
            } else if x.eg != Eg::Off {
                x.eg = Eg::Release;
            }
        }
    }

    fn patch(&self, ch: usize) -> [u8; 8] {
        if self.rhythm & 0x20 != 0 && ch >= 6 {
            return ROM[16 + ch - 6];
        }
        match self.ch[ch].inst {
            0 => self.user,
            n => ROM[usize::from(n)],
        }
    }

    /// Advances one operator's measured 7-bit envelope on the chip's shared sample counter.
    ///
    /// The four low rate patterns and the high-rate exceptions come from the real-chip transition
    /// captures. Keeping the integer envelope in the existing `f32` field preserves old save-state
    /// layout while replacing the former continuous approximation.
    fn step_env(op: &mut Op, ar: u8, dr: u8, ksr_val: u8, sustain_level: u8, percussive: bool, release_rate: u8, counter: u32) {
        let mut env = op.env.round().clamp(0.0, 127.0) as u8;
        match op.eg {
            Eg::Damp if env >= 124 => {
                op.phase = 0;
                if ar == 15 {
                    env = 0;
                    op.eg = Eg::Decay;
                } else {
                    op.eg = Eg::Attack;
                }
            }
            Eg::Attack if env == 0 => op.eg = Eg::Decay,
            Eg::Decay if env >> 3 == sustain_level => op.eg = Eg::Sustain,
            _ => {}
        }

        let (attack, base_rate) = match op.eg {
            Eg::Damp => (false, 12),
            Eg::Attack => (true, ar),
            Eg::Decay => (false, dr),
            Eg::Sustain => (false, if percussive { release_rate } else { 0 }),
            Eg::Release => (false, release_rate),
            Eg::Off => {
                op.env = f32::from(env);
                return;
            }
        };
        let rate = if base_rate == 0 { 0 } else { (base_rate.saturating_mul(4).saturating_add(ksr_val)).min(63) };
        let pattern = [
            [0u8, 1, 0, 1, 0, 1, 0, 1],
            [0u8, 1, 0, 1, 1, 1, 0, 1],
            [0u8, 1, 1, 1, 0, 1, 1, 1],
            [0u8, 1, 1, 1, 1, 1, 1, 1],
        ];
        let row = usize::from(rate & 3);
        if attack {
            match rate / 4 {
                0 | 15 => {}
                1..=11 => {
                    let shift = 13 - u32::from(rate / 4);
                    let mask = ((1u32 << shift) - 1) & !3;
                    if (counter.wrapping_add(3) & mask) == 0 {
                        let column = ((counter.wrapping_add(3) >> shift) & 7) as usize;
                        if pattern[row][column] != 0 {
                            env = env.saturating_sub((env >> 4).saturating_add(1));
                        }
                    }
                }
                12..=14 => {
                    let column = ((counter & 0x0c) >> 1) as usize;
                    let shift = 16 - u32::from(rate / 4) - u32::from(pattern[row][column]);
                    env = env.saturating_sub((env >> shift).saturating_add(1));
                }
                _ => {}
            }
        } else {
            match rate / 4 {
                0 => {}
                1..=12 => {
                    let shift = 13 - u32::from(rate / 4);
                    if counter & ((1u32 << shift) - 1) == 0 {
                        let column = ((counter >> shift) & 7) as usize;
                        env = env.saturating_add(pattern[row][column]).min(127);
                    }
                }
                13 => {
                    let column = (((counter & 0x0c) >> 1) | (counter & 1)) as usize;
                    env = env.saturating_add(pattern[row][column]).min(127);
                }
                14 => {
                    let column = ((counter & 0x0c) >> 1) as usize;
                    env = env.saturating_add(pattern[row][column] + 1).min(127);
                }
                _ => env = env.saturating_add(2).min(127),
            }
        }
        if op.eg == Eg::Release && env == 127 {
            op.eg = Eg::Off;
        }
        op.env = f32::from(env);
    }

    /// One of the chip's own samples (49.7 kHz), mixed.
    fn sample(&mut self) -> i32 {
        self.am_phase = (self.am_phase + 1) % 13_440;
        self.pm_phase = (self.pm_phase + 1) & 0x1fff;
        // The measured 23-bit LFSR, polynomial x^23 + x^9 + x^8 + x + 1.
        if self.noise & 1 != 0 {
            self.noise ^= 0x800302;
        }
        self.noise >>= 1;
        let am_step = self.am_phase / 64;
        let am_raw = if am_step < 7 {
            0
        } else if am_step < 107 {
            1 + (am_step - 7) / 4
        } else if am_step < 110 {
            26
        } else {
            25 - (am_step - 110) / 4
        };
        let am = am_raw / 2;
        const PM: [[i32; 8]; 8] = [
            [0, 0, 0, 0, 0, 0, 0, 0],
            [0, 0, 1, 0, 0, 0, -1, 0],
            [0, 1, 2, 1, 0, -1, -2, -1],
            [0, 1, 3, 1, 0, -1, -3, -1],
            [0, 2, 4, 2, 0, -2, -4, -2],
            [0, 2, 5, 2, 0, -2, -5, -2],
            [0, 3, 6, 3, 0, -3, -6, -3],
            [0, 3, 7, 3, 0, -3, -7, -3],
        ];
        let pm_step = ((self.pm_phase >> 10) & 7) as usize;
        let mut mix = 0i32;
        let rhythm = self.rhythm & 0x20 != 0;
        for n in 0..9 {
            let p = self.patch(n);
            let c = self.ch[n];
            let block = u32::from(c.block);
            let fnum = u32::from(c.fnum);
            let ksr_base = (c.block << 1) | ((c.fnum >> 8) as u8 & 1);
            let mut outs = [0i32; 2];
            for o in 0..2 {
                let (mult, am_on, pm_on, eg_type, ksr) = (p[o] & 15, p[o] & 0x80 != 0, p[o] & 0x40 != 0, p[o] & 0x20 != 0, p[o] & 0x10 != 0);
                let ksr_val = if ksr { ksr_base } else { ksr_base >> 2 };
                let ar = p[4 + o] >> 4;
                let dr = p[4 + o] & 15;
                let sl = p[6 + o] >> 4;
                let rr_patch = p[6 + o] & 15;
                let release_rate = if c.ops[o].eg == Eg::Release {
                    if c.sustain { 5 } else if eg_type { rr_patch } else { 7 }
                } else {
                    rr_patch
                };
                let percussive = !eg_type;
                let mut op = c.ops[o];
                Self::step_env(&mut op, ar, dr, ksr_val, sl, percussive, release_rate, self.pm_phase);
                // The phase step and its eight-step, 6.4 Hz vibrato pattern are measured from the
                // real chip. `MUL2` keeps the half multiplier exact without floating point.
                let pm = if pm_on { PM[(fnum >> 6) as usize][pm_step] } else { 0 };
                let f = (2 * fnum as i32 + pm).max(0) as u32;
                op.phase = op.phase.wrapping_add(((f * MUL2[usize::from(mult)]) << block) >> 2);
                self.ch[n].ops[o] = op;
                // Attenuation, in 3/8 dB: envelope, total level (modulator, 3/4 dB steps) or volume
                // (carrier, 3 dB steps), key scale, AM.
                let ksl_bits = if o == 0 { p[2] >> 6 } else { p[3] >> 6 };
                let ksl_att = {
                    const BASE: [i32; 16] = [112, 64, 48, 38, 32, 26, 22, 18, 16, 12, 10, 8, 6, 4, 2, 0];
                    let k = 16 * block as i32 - BASE[usize::from(c.fnum >> 5) & 15];
                    if ksl_bits == 0 || k <= 0 { 0 } else { (k as u32) >> (3 - ksl_bits) }
                };
                let level = if o == 0 { u32::from(p[2] & 0x3F) * 2 } else { u32::from(c.vol) * 8 };
                let env = op.env as u32 + level + ksl_att + if am_on { am } else { 0 };
                // 3/8 dB is 0.0623 of a factor of two: 16 of the tables' 1/256 steps.
                let att = env.min(255) * 16;
                let half = if o == 0 { p[3] & 0x08 != 0 } else { p[3] & 0x10 != 0 };
                let phase10 = op.phase >> 9;
                let modulated = if o == 0 {
                    let fb = p[3] & 7;
                    let fbm = if fb == 0 { 0 } else { (op.out[0] + op.out[1]) >> (9 - fb) };
                    phase10.wrapping_add(fbm as u32)
                } else {
                    phase10.wrapping_add(outs[0] as u32)
                };
                let v = if op.eg == Eg::Off { 0 } else { op_out(modulated, att, half) };
                let x = &mut self.ch[n].ops[o];
                x.out = [x.out[1], v];
                outs[o] = v;
            }
            let v = if rhythm && n >= 6 {
                // The drums use the operators as their own voices.
                let noise = if self.noise & 1 != 0 { 1 } else { -1 };
                match n {
                    6 => outs[1] * 2,
                    7 => (outs[0].abs() * noise) + outs[1] * 2,
                    _ => outs[0] * 2 + (outs[1].abs() * noise),
                }
            } else {
                outs[1]
            };
            mix += v * i32::from(self.chan_vol[n]) / 100;
        }
        mix
    }

    /// Adds what the chip makes into `out` (stereo, the same on both sides), one output sample
    /// per pair, as the Master System mixes the FM unit with the other sound chip. Driven by the
    /// number of samples the other chip made, so the two can never drift apart; each output
    /// sample averages the chip's own samples it covers (about one, at 48 kHz).
    pub fn mix_into(&mut self, rate: u32, out: &mut [i16]) {
        if !self.enabled {
            return;
        }
        let per_out = 3_579_545.0 / RATE_DIV / f64::from(rate);
        for pair in out.chunks_mut(2) {
            self.carry += per_out;
            let n = self.carry.floor() as u32;
            self.carry -= f64::from(n);
            let mut sum = 0i32;
            for _ in 0..n {
                sum += self.sample();
            }
            let v = (sum * 3 / 2 / n.max(1) as i32 * i32::from(self.volume) / 100).clamp(-32768, 32767) as i16;
            for x in pair {
                *x = x.saturating_add(v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(fm: &mut Fm, frames: usize) -> Vec<i16> {
        let mut out = vec![0i16; 1600 * frames];
        for chunk in out.chunks_mut(1600) {
            fm.mix_into(48_000, chunk);
        }
        out
    }

    #[test]
    fn the_quarter_sine_tables_turn_back_into_a_sine() {
        assert_eq!(op_out(0x100, 0, false), op_out(0xFF, 0, false));
        assert!(op_out(0x100, 0, false) > 4000, "full level near the top");
        assert_eq!(op_out(0x300, 0, false), -op_out(0x100, 0, false), "the second half is the first turned over");
        assert_eq!(op_out(0x300, 0, true), 0, "the half-sine wave is silent there");
        assert!(op_out(0x100, 256, false) * 2 - op_out(0x100, 0, false) <= 2, "256 more is half");
    }

    #[test]
    fn a_note_plays_at_its_pitch_and_stops_when_released() {
        let mut fm = Fm { enabled: true, ..Fm::default() };
        // The user instrument as a pure sine: the modulator silenced (total level 63), both
        // multipliers 1, sustained, instant attack. Channel 0 at full volume; F-number 288 in
        // block 4 is 288 * 8 * 49716 / 2^18 = 437 Hz.
        for (r, v) in [0x21u8, 0x21, 0x3F, 0x00, 0xF0, 0xF0, 0x00, 0x0F].iter().enumerate() {
            fm.write_addr(r as u8);
            fm.write_data(*v);
        }
        fm.write_addr(0x30);
        fm.write_data(0x00);
        fm.write_addr(0x10);
        fm.write_data(0x20);
        fm.write_addr(0x20);
        fm.write_data(0x10 | 4 << 1 | 1);
        let out = play(&mut fm, 30);
        let left: Vec<i16> = out.chunks(2).map(|s| s[0]).collect();
        let tail = &left[left.len() - 24_000..];
        let rises = tail.windows(2).filter(|w| w[0] <= 0 && w[1] > 0).count();
        assert!((430..=444).contains(&(rises * 2)), "about 437 Hz: {}", rises * 2);
        assert!(tail.iter().map(|v| i32::from(*v).abs()).max().unwrap() > 1000, "and loud enough to hear");
        fm.write_addr(0x20);
        fm.write_data(4 << 1 | 1);
        let out = play(&mut fm, 60);
        assert!(out[out.len() - 2000..].iter().all(|v| v.abs() < 50), "silent once released");
    }

    #[test]
    fn a_built_in_instrument_sounds_and_its_release_dies_away() {
        let mut fm = Fm { enabled: true, ..Fm::default() };
        // Flute (instrument 4) on channel 3; it is a sustained instrument whose release rate is 7.
        fm.write_addr(0x33);
        fm.write_data(0x40);
        fm.write_addr(0x13);
        fm.write_data(0x20);
        fm.write_addr(0x23);
        fm.write_data(0x10 | 4 << 1 | 1);
        let out = play(&mut fm, 30);
        let loud = out[out.len() - 4800..].iter().map(|v| i32::from(*v).abs()).max().unwrap();
        assert!(loud > 1000, "it plays: {loud}");
        fm.write_addr(0x23);
        fm.write_data(4 << 1 | 1);
        // Release rate 7: 48 dB in 19.64 s / 2^6 = 0.31 s, so after a tenth it is quieter but on.
        let soon = play(&mut fm, 6);
        let still = soon[soon.len() - 1600..].iter().map(|v| i32::from(*v).abs()).max().unwrap();
        assert!(still > 0 && still < loud, "dying away: {still} of {loud}");
        let out = play(&mut fm, 60);
        assert!(out[out.len() - 2000..].iter().all(|v| v.abs() < 50), "silent once released");
    }

    #[test]
    fn switched_off_it_adds_nothing() {
        let mut fm = Fm::default();
        let mut out = vec![7i16; 1600];
        fm.mix_into(48_000, &mut out);
        assert!(out.iter().all(|v| *v == 7));
    }
}
