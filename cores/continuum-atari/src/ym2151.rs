//! Yamaha's YM2151 (OPM), the FM sound chip of the 7800's XM expansion module and of the cartridges
//! that carry one at $0460: eight channels of four operators each, eight ways to connect them, an
//! LFO for vibrato and tremolo, a noise generator on the last operator, and two timers. It runs at
//! twice the processor's clock (3.58 MHz) and makes one stereo sample every 64 of its clocks
//! (55.9 kHz), sent to a YM3012 DAC, which keeps 10 bits and a 3-bit exponent of each.
//!
//! Written for cAtari from Yamaha's YM2151 data sheet and application manual, and from how the chip
//! is known to work inside, as read from its die by Nuke.YKT (Nuked-OPM) and measured by others;
//! it behaves as MAME 0.289 plays the chip (its ymfm, by Aaron Giles), sample for sample
//! (tests/ym2151_mame.rs). The chip's own numbers come from where the chip keeps them:
//! - its sine and exponent tables, worked out here from their formulas (they give the die's values
//!   exactly: the logarithm of a quarter sine in 256 steps, 4.8 fixed point, and 2^x - 1 in 10 bits);
//! - its note-to-pitch table, 48 notes of base and slope with the chip's interpolation for the key
//!   fraction (from the die), and its detune steps, 8 values and a shift (from the die);
//! - its envelope step patterns, eight steps for each of the 64 rates.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const EG_ATTACK: u8 = 1;
const EG_DECAY: u8 = 2;
const EG_SUSTAIN: u8 = 3;
const EG_RELEASE: u8 = 4;
/// Quieter than this an operator counts as silent.
const EG_QUIET: u16 = 0x380;
const KEYON_NORMAL: u8 = 0;
const KEYON_CSM: u8 = 2;
/// A phase step that has to be worked out each sample (the LFO is moving the pitch).
const PHASE_STEP_DYNAMIC: u32 = 1;
const STATUS_TIMERA: u8 = 0x01;
const STATUS_TIMERB: u8 = 0x02;
const STATUS_BUSY: u8 = 0x80;

/// The chip's note table, for each of the 64 codes (4 bits of note, the top 2 bits of the key
/// fraction): the base step, whether the plain interpolation applies, and the slope. The four
/// notes in each group of 16 that don't exist (codes 3, 7, 11, 15) are zero.
const FREQ: [(u16, bool, u8); 64] = [
    (1299, true, 19), (1318, true, 19), (1337, true, 19), (1356, true, 20),
    (1376, true, 20), (1396, true, 20), (1416, true, 21), (1437, true, 20),
    (1458, true, 21), (1479, true, 21), (1501, true, 22), (1523, true, 22),
    (0, false, 16), (0, false, 16), (0, false, 16), (0, false, 16),
    (1545, true, 22), (1567, true, 22), (1590, true, 23), (1613, true, 23),
    (1637, true, 23), (1660, true, 24), (1685, true, 24), (1709, true, 24),
    (1734, true, 25), (1759, true, 25), (1785, true, 26), (1811, true, 26),
    (0, false, 16), (0, false, 16), (0, false, 16), (0, false, 16),
    (1837, true, 26), (1864, true, 27), (1891, true, 27), (1918, true, 28),
    (1946, true, 28), (1975, true, 28), (2003, true, 29), (2032, true, 30),
    (2062, true, 30), (2092, true, 30), (2122, true, 31), (2153, true, 31),
    (0, false, 16), (0, false, 16), (0, false, 16), (0, false, 16),
    (2185, true, 31), (2216, false, 31), (2249, false, 31), (2281, false, 31),
    (2315, false, 31), (2348, false, 31), (2382, false, 30), (2417, false, 30),
    (2452, false, 30), (2488, false, 30), (2524, false, 30), (2561, false, 30),
    (0, false, 16), (0, false, 16), (0, false, 16), (0, false, 16),
];

/// The detune unit's eight steps.
const DETUNE: [u32; 8] = [16, 17, 19, 20, 22, 24, 27, 29];

/// How much the envelope moves on each of eight steps, at each of the 64 rates (four bits a step,
/// the first step in the low bits).
const EG_STEPS: [u32; 64] = [
    0x00000000, 0x00000000, 0x10101010, 0x10101010, 0x10101010, 0x10101010, 0x11101110, 0x11101110,
    0x10101010, 0x10111010, 0x11101110, 0x11111110, 0x10101010, 0x10111010, 0x11101110, 0x11111110,
    0x10101010, 0x10111010, 0x11101110, 0x11111110, 0x10101010, 0x10111010, 0x11101110, 0x11111110,
    0x10101010, 0x10111010, 0x11101110, 0x11111110, 0x10101010, 0x10111010, 0x11101110, 0x11111110,
    0x10101010, 0x10111010, 0x11101110, 0x11111110, 0x10101010, 0x10111010, 0x11101110, 0x11111110,
    0x10101010, 0x10111010, 0x11101110, 0x11111110, 0x10101010, 0x10111010, 0x11101110, 0x11111110,
    0x11111111, 0x21112111, 0x21212121, 0x22212221, 0x22222222, 0x42224222, 0x42424242, 0x44424442,
    0x44444444, 0x84448444, 0x84848484, 0x88848884, 0x88888888, 0x88888888, 0x88888888, 0x88888888,
];

struct Tables {
    /// The full sine wave over 1024 steps as attenuation (4.8), with its sign in bit 15.
    wave: Vec<u16>,
    /// 2^-x for the 8 fraction bits of an attenuation, as a 13-bit volume (shifted by 2).
    power: [u16; 256],
    /// The step for each of the 768 notes of an octave (12 notes x 64 fractions), at octave 7.
    phase: Vec<u32>,
    /// The detune for each of 32 key codes and 4 amounts.
    detune: [[u8; 4]; 32],
    /// The LFO's first three waveforms: AM in the low 8 bits, PM (signed) in the high 8.
    lfo: [[i16; 256]; 3],
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        let quarter: Vec<u16> = (0..256)
            .map(|i| (-((i as f64 + 0.5) * std::f64::consts::PI / 512.0).sin().log2() * 256.0).round() as u16)
            .collect();
        let wave = (0..1024u32)
            .map(|i| {
                let j = if i & 0x100 != 0 { !i } else { i } & 0xFF;
                quarter[j as usize] | (((i >> 9) & 1) << 15) as u16
            })
            .collect();
        let mut power = [0u16; 256];
        for (i, p) in power.iter_mut().enumerate() {
            let e = ((2f64.powf((255 - i) as f64 / 256.0) - 1.0) * 1024.0).round() as u16;
            *p = (e | 0x400) << 2;
        }
        let phase = (0..768u32)
            .map(|i| {
                // The chip's codes leave out every fourth note: 0, 1, 2, 4, 5, 6, ...
                let code = i >> 6;
                let note = code + code / 3;
                note_step((note << 6) | (i & 63)) * 32
            })
            .collect();
        let mut detune = [[0u8; 4]; 32];
        for (kc, row) in detune.iter_mut().enumerate() {
            for (dt, d) in row.iter_mut().enumerate().skip(1) {
                let k = kc.min(0x1C) as u32;
                // Detune 1 adds 0, 2 adds 2, 3 adds 3.
                let sum = (k >> 2) + 9 + (u32::from(dt == 3) | (dt as u32 & 2));
                *d = (DETUNE[(((sum & 1) << 2) | (k & 3)) as usize] >> (9 - (sum >> 1))) as u8;
            }
        }
        let pack = |am: u8, pm: u8| (am as u16 | (pm as u16) << 8) as i16;
        let mut lfo = [[0i16; 256]; 3];
        for (i, out) in lfo[0].iter_mut().enumerate() {
            *out = pack(i as u8 ^ 0xFF, i as u8); // sawtooth
        }
        for (i, out) in lfo[1].iter_mut().enumerate() {
            let am = if i & 0x80 != 0 { 0 } else { 0xFF };
            *out = pack(am, am ^ 0x80); // square
        }
        for (i, out) in lfo[2].iter_mut().enumerate() {
            let am = if i & 0x80 != 0 { (i << 1) as u8 } else { ((i ^ 0xFF) << 1) as u8 };
            *out = pack(am, if i & 0x40 != 0 { am } else { !am }); // triangle
        }
        Tables { wave, power, phase, detune, lfo }
    })
}

/// The chip's note-to-step conversion: a 10-bit code (note and key fraction) to a step.
fn note_step(code: u32) -> u32 {
    let (base, plain, slope) = FREQ[((code >> 4) & 63) as usize];
    let l = code & 15;
    let mut sum = 0u32;
    if plain {
        for i in 0..4 {
            if l & (1 << i) != 0 {
                sum += slope as u32 >> (3 - i);
            }
        }
    } else {
        let s = slope as u32 | 1;
        if l & 1 != 0 {
            sum += (s >> 3) + 2;
        }
        if l & 2 != 0 {
            sum += 8;
        }
        if l & 4 != 0 {
            sum += s >> 1;
        }
        if l & 8 != 0 {
            sum += s + 1;
        }
        if l & 12 == 12 && slope & 1 == 0 {
            sum += 4;
        }
    }
    base as u32 + (sum >> 1)
}

fn attenuation_to_volume(input: u32) -> u32 {
    let shift = input >> 8;
    if shift >= 16 {
        0
    } else {
        (tables().power[(input & 0xFF) as usize] as u32) >> shift
    }
}

/// What the YM3012 DAC keeps of a sample: 10 bits of mantissa below a 3-bit exponent.
fn dac(v: i32) -> i32 {
    if v < -32768 {
        return -32768;
    }
    if v > 32767 {
        return 32767;
    }
    let scan = (v ^ (v >> 31)) as u32;
    let exponent = (7 - (scan << 17).leading_zeros() as i32).max(1) - 1;
    v & !((1 << exponent) - 1)
}

/// What an operator works out from its registers when they change.
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
struct Cache {
    block_freq: u32,
    detune: i32,
    multiple: u32,
    phase_step: u32,
    total_level: u32,
    eg_sustain: u32,
    eg_rate: [u32; 5],
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Op {
    /// 10.10 fixed point.
    phase: u32,
    /// 4.6 fixed point, 0 loudest, $3FF silent.
    att: u16,
    state: u8,
    key_state: u8,
    keyon_live: u8,
    cache: Cache,
}

impl Default for Op {
    fn default() -> Op {
        Op { phase: 0, att: 0x3FF, state: EG_RELEASE, key_state: 0, keyon_live: 0, cache: Cache::default() }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Ym2151 {
    regs: Vec<u8>,
    address: u8,
    ops: [Op; 32],
    feedback: [[i16; 2]; 8],
    feedback_in: [i16; 8],
    env_counter: u32,
    lfo_counter: u32,
    noise_lfsr: u32,
    noise_counter: u8,
    noise_state: u8,
    lfo_am: u8,
    lfo_noise: Vec<u8>,
    modified: bool,
    prepare_count: u32,
    active: u8,
    total_clocks: u8,
    status: u8,
    /// The timers: when each next fires (in the machine's clocks), if it is running.
    timer_due: [Option<u64>; 2],
    /// The machine clocks in one of the chip's samples, for the timers and the busy flag.
    pub sample_clocks: u64,
    busy_until: u64,
}

impl Default for Ym2151 {
    fn default() -> Ym2151 {
        let mut regs = vec![0u8; 0x100];
        regs[0x20..0x28].fill(0xC0);
        Ym2151 {
            regs,
            address: 0,
            ops: [Op::default(); 32],
            feedback: [[0; 2]; 8],
            feedback_in: [0; 8],
            env_counter: 0,
            lfo_counter: 0,
            noise_lfsr: 1,
            noise_counter: 0,
            noise_state: 0,
            lfo_am: 0,
            lfo_noise: vec![0; 256],
            modified: true,
            prepare_count: 0,
            active: 0xFF,
            total_clocks: 0,
            status: 0,
            timer_due: [None; 2],
            sample_clocks: 128,
            busy_until: 0,
        }
    }
}

/// A channel's four operators in the order the connections use them: M1, C1, M2, C2.
fn ops_of(ch: usize) -> [usize; 4] {
    [ch, ch + 16, ch + 8, ch + 24]
}

impl Ym2151 {
    fn reg(&self, r: usize) -> u32 {
        self.regs[r] as u32
    }

    // ---- registers -----------------------------------------------------------------------

    /// A processor write: offset 0 is the address, 1 the data. `now` is the machine's time.
    pub fn write(&mut self, offset: u8, v: u8, now: u64) {
        if offset & 1 == 0 {
            self.address = v;
            return;
        }
        let r = self.address as usize;
        if r == 0x14 {
            self.mode_write(v, now);
        } else {
            self.modified = true;
            if r == 0x19 {
                self.regs[0x19 + (v >> 7) as usize] = v;
            } else if r != 0x1A {
                self.regs[r] = v;
            }
            if r == 0x08 {
                let ch = (v & 7) as usize;
                for (i, &o) in ops_of(ch).iter().enumerate() {
                    let on = (v >> (3 + i)) & 1;
                    let op = &mut self.ops[o];
                    op.keyon_live = (op.keyon_live & !(1 << KEYON_NORMAL)) | (on << KEYON_NORMAL);
                }
            }
        }
        // The chip takes 64 of its clocks over a write; the status says so meanwhile.
        self.busy_until = now + self.sample_clocks;
    }

    /// The status: the timers' flags, and busy while a write is being taken.
    pub fn status(&mut self, now: u64) -> u8 {
        self.run_timers(now);
        self.status | if now < self.busy_until { STATUS_BUSY } else { 0 }
    }

    /// Whether a timer's flag is up (the chip's IRQ output, when a cartridge connects it).
    pub fn irq(&self) -> bool {
        self.status & (STATUS_TIMERA | STATUS_TIMERB) != 0
    }

    fn mode_write(&mut self, v: u8, now: u64) {
        self.run_timers(now);
        self.modified = true;
        self.regs[0x14] = v;
        if v & 0x20 != 0 {
            self.status &= !STATUS_TIMERB;
        }
        if v & 0x10 != 0 {
            self.status &= !STATUS_TIMERA;
        }
        let tb = -((self.total_clocks & 15) as i64);
        self.load_timer(1, v & 0x02 != 0, tb, now);
        self.load_timer(0, v & 0x01 != 0, 0, now);
    }

    fn timer_period(&self, t: usize) -> i64 {
        if t == 0 {
            1024 - ((self.reg(0x10) << 2) | (self.reg(0x11) & 3)) as i64
        } else {
            16 * (256 - self.reg(0x12) as i64)
        }
    }

    fn load_timer(&mut self, t: usize, on: bool, delta: i64, now: u64) {
        if on && self.timer_due[t].is_none() {
            let period = (self.timer_period(t) + delta).max(1) as u64;
            self.timer_due[t] = Some(now + period * self.sample_clocks);
        } else if !on {
            self.timer_due[t] = None;
        }
    }

    /// Fires the timers due by `now`.
    fn run_timers(&mut self, now: u64) {
        for t in 0..2 {
            while let Some(due) = self.timer_due[t] {
                if due > now {
                    break;
                }
                let mode = self.reg(0x14);
                if t == 0 && mode & 0x04 != 0 {
                    self.status |= STATUS_TIMERA;
                }
                if t == 1 && mode & 0x08 != 0 {
                    self.status |= STATUS_TIMERB;
                }
                if t == 0 && mode & 0x80 != 0 {
                    // CSM: timer A keys every operator on, for one look at the keys.
                    for op in self.ops.iter_mut() {
                        op.keyon_live |= 1 << KEYON_CSM;
                    }
                    self.modified = true;
                }
                let period = self.timer_period(t).max(1) as u64;
                self.timer_due[t] = Some(due + period * self.sample_clocks);
            }
        }
    }

    // ---- one sample ------------------------------------------------------------------------

    /// Brings the timers to `now` and makes the next sample: (left, right) as the DAC puts them out.
    pub fn sample(&mut self, now: u64) -> (i32, i32) {
        self.run_timers(now);
        self.clock();
        let mut out = [0i32; 2];
        for ch in 0..8 {
            if self.active & (1 << ch) != 0 {
                self.output_channel(ch, &mut out);
            }
        }
        (dac(out[0]), dac(out[1]))
    }

    fn clock(&mut self) {
        self.total_clocks = self.total_clocks.wrapping_add(1);
        let prepare = if self.modified {
            true
        } else {
            let p = self.prepare_count >= 4096;
            self.prepare_count += 1;
            p
        };
        if prepare {
            self.active = 0;
            for ch in 0..8 {
                if self.prepare_channel(ch) {
                    self.active |= 1 << ch;
                }
            }
            self.modified = false;
            self.prepare_count = 0;
        }
        self.env_counter = self.env_counter.wrapping_add(1);
        if self.env_counter & 3 == 3 {
            self.env_counter = self.env_counter.wrapping_add(1);
        }
        let pm = self.clock_noise_and_lfo();
        for ch in 0..8 {
            self.feedback[ch][0] = self.feedback[ch][1];
            self.feedback[ch][1] = self.feedback_in[ch];
            for o in ops_of(ch) {
                if self.env_counter & 3 == 0 {
                    self.clock_envelope(o, self.env_counter >> 2);
                }
                let mut step = self.ops[o].cache.phase_step;
                if step == PHASE_STEP_DYNAMIC {
                    step = self.phase_step(ch, o, pm);
                }
                self.ops[o].phase = self.ops[o].phase.wrapping_add(step);
            }
        }
    }

    fn prepare_channel(&mut self, ch: usize) -> bool {
        let mut active = false;
        for o in ops_of(ch) {
            self.cache_operator(ch, o);
            let keystate = u8::from(self.ops[o].keyon_live != 0);
            let op = &mut self.ops[o];
            if keystate != op.key_state {
                op.key_state = keystate;
                if keystate != 0 {
                    if op.state != EG_ATTACK {
                        op.state = EG_ATTACK;
                        op.phase = 0;
                        if op.cache.eg_rate[EG_ATTACK as usize] >= 62 {
                            op.att = 0;
                        }
                    }
                } else if op.state < EG_RELEASE {
                    op.state = EG_RELEASE;
                }
            }
            op.keyon_live &= !(1 << KEYON_CSM);
            active |= op.state != EG_RELEASE || op.att < EG_QUIET;
        }
        active
    }

    fn cache_operator(&mut self, ch: usize, o: usize) {
        let block_freq = (self.reg(0x28 + ch) & 0x7F) << 6 | (self.reg(0x30 + ch) >> 2);
        let keycode = (block_freq >> 8) & 31;
        let dt = (self.reg(0x40 + o) >> 4) & 7;
        let d = tables().detune[keycode as usize][(dt & 3) as usize] as i32;
        let mut multiple = (self.reg(0x40 + o) & 15) * 2;
        if multiple == 0 {
            multiple = 1;
        }
        let mut c = Cache {
            block_freq,
            detune: if dt & 4 != 0 { -d } else { d },
            multiple,
            phase_step: 0,
            total_level: (self.reg(0x60 + o) & 0x7F) << 3,
            eg_sustain: 0,
            eg_rate: [0; 5],
        };
        let mut sl = self.reg(0xE0 + o) >> 4;
        sl |= (sl + 1) & 0x10;
        c.eg_sustain = sl << 5;
        let ksr = keycode >> ((self.reg(0x80 + o) >> 6) ^ 3);
        let rate = |raw: u32| if raw == 0 { 0 } else { (raw + ksr).min(63) };
        c.eg_rate[EG_ATTACK as usize] = rate((self.reg(0x80 + o) & 31) * 2);
        c.eg_rate[EG_DECAY as usize] = rate((self.reg(0xA0 + o) & 31) * 2);
        c.eg_rate[EG_SUSTAIN as usize] = rate((self.reg(0xC0 + o) & 31) * 2);
        c.eg_rate[EG_RELEASE as usize] = rate((self.reg(0xE0 + o) & 15) * 4 + 2);
        self.ops[o].cache = c;
        let pms = (self.reg(0x38 + ch) >> 4) & 7;
        self.ops[o].cache.phase_step = if self.reg(0x1A) & 0x7F == 0 || pms == 0 {
            self.phase_step(ch, o, 0)
        } else {
            PHASE_STEP_DYNAMIC
        };
    }

    fn phase_step(&self, ch: usize, o: usize, lfo_pm: i32) -> u32 {
        const DT2: [i32; 4] = [0, (600 * 64 + 50) / 100, (781 * 64 + 50) / 100, (950 * 64 + 50) / 100];
        let c = &self.ops[o].cache;
        let mut delta = DT2[(self.reg(0xC0 + o) >> 6) as usize];
        let pms = (self.reg(0x38 + ch) >> 4) & 7;
        if pms != 0 {
            if pms < 6 {
                delta += lfo_pm >> (6 - pms);
            } else {
                delta = delta.wrapping_add(((lfo_pm as u32) << (pms - 5)) as i32);
            }
        }
        let step = Self::key_code_step(c.block_freq, delta);
        let step = step.wrapping_add(c.detune as u32);
        step.wrapping_mul(c.multiple) >> 1
    }

    /// A 13-bit block, key code and fraction, moved by `delta` (in 1/64ths of a note), to a step.
    fn key_code_step(block_freq: u32, delta: i32) -> u32 {
        let t = tables();
        let mut block = (block_freq >> 10) & 7;
        let code = ((block_freq >> 6) & 15) - ((block_freq >> 8) & 3);
        let mut eff = ((code << 6) | (block_freq & 63)) as i32 + delta;
        if !(0..768).contains(&eff) {
            if eff < 0 {
                eff += 768;
                if block == 0 {
                    return t.phase[0] >> 7;
                }
                block -= 1;
            } else {
                eff -= 768;
                if eff >= 768 {
                    block += 1;
                    eff -= 768;
                }
                if block >= 7 {
                    return t.phase[767];
                }
                block += 1;
            }
        }
        t.phase[eff as usize] >> (block ^ 7)
    }

    fn clock_noise_and_lfo(&mut self) -> i32 {
        let freq = (self.reg(0x0F) & 31) ^ 31;
        for _ in 0..2 {
            // The 17-bit shift register, with the last 8 bits it put out below it.
            let s = self.noise_lfsr << 1;
            self.noise_lfsr = s | (((s >> 17) ^ (s >> 14) ^ 1) & 1);
            let c = self.noise_counter;
            self.noise_counter = c.wrapping_add(1);
            if c as u32 >= freq {
                self.noise_counter = 0;
                self.noise_state = ((self.noise_lfsr >> 17) & 1) as u8;
            }
        }
        let rate = self.reg(0x18);
        self.lfo_counter = self.lfo_counter.wrapping_add((0x10 | (rate & 15)) << (rate >> 4));
        if self.reg(0x01) & 0x02 != 0 {
            self.lfo_counter = 0;
        }
        let lfo = ((self.lfo_counter >> 22) & 0xFF) as usize;
        self.lfo_noise[(lfo + 1) & 0xFF] = ((self.noise_lfsr >> 17) & 0xFF) as u8;
        let wave = self.reg(0x1B) & 3;
        let ampm: i32 = if wave == 3 {
            let n = self.lfo_noise[lfo] as u16;
            (n | (n << 8)) as i16 as i32
        } else {
            tables().lfo[wave as usize][lfo] as i32
        };
        self.lfo_am = (((ampm & 0xFF) as u32 * (self.reg(0x19) & 0x7F)) >> 7) as u8;
        ((ampm >> 8) * (self.reg(0x1A) & 0x7F) as i32) >> 7
    }

    fn clock_envelope(&mut self, o: usize, env_counter: u32) {
        let op = &mut self.ops[o];
        if op.state == EG_ATTACK && op.att == 0 {
            op.state = EG_DECAY;
        }
        if op.state == EG_DECAY && op.att as u32 >= op.cache.eg_sustain {
            op.state = EG_SUSTAIN;
        }
        let rate = op.cache.eg_rate[op.state as usize];
        let shift = rate >> 2;
        let counter = env_counter << shift;
        if counter & 0x7FF != 0 {
            return;
        }
        let bits = (counter >> if shift <= 11 { 11 } else { shift }) & 7;
        let inc = (EG_STEPS[rate as usize] >> (4 * bits)) & 15;
        if op.state == EG_ATTACK {
            if rate < 62 {
                let a = op.att as i32;
                op.att = (a + ((!a * inc as i32) >> 4)) as u16;
            }
        } else {
            op.att = op.att.wrapping_add(inc as u16);
            if op.att >= 0x400 {
                op.att = 0x3FF;
            }
        }
    }

    // ---- the output ------------------------------------------------------------------------

    fn envelope_attenuation(&self, o: usize, am_offset: u32) -> u32 {
        let op = &self.ops[o];
        let mut r = op.att as u32;
        if self.reg(0xA0 + o) & 0x80 != 0 {
            r += am_offset;
        }
        r += op.cache.total_level;
        r.min(0x3FF)
    }

    fn volume(&self, o: usize, phase: u32, am_offset: u32) -> i32 {
        if self.ops[o].att > EG_QUIET {
            return 0;
        }
        let s = tables().wave[(phase & 0x3FF) as usize] as u32;
        let env = self.envelope_attenuation(o, am_offset) << 2;
        let v = attenuation_to_volume((s & 0x7FFF) + env) as i32;
        if s & 0x8000 != 0 {
            -v
        } else {
            v
        }
    }

    fn noise_volume(&self, o: usize, am_offset: u32) -> i32 {
        let v = ((self.envelope_attenuation(o, am_offset) ^ 0x3FF) << 1) as i32;
        if self.noise_state & 1 != 0 {
            -v
        } else {
            v
        }
    }

    fn output_channel(&mut self, ch: usize, out: &mut [i32; 2]) {
        // Where each operator's input comes from, per connection: (O2 in, O3 in, O4 in, which of
        // O1-O3 are heard as well as O4). Inputs index [0, O1, O2, O3, -, O1+O2, O1+O3, O2+O3].
        const ALG: [(usize, usize, usize, [bool; 3]); 8] = [
            (1, 2, 3, [false, false, false]),
            (0, 5, 3, [false, false, false]),
            (0, 2, 6, [false, false, false]),
            (1, 0, 7, [false, false, false]),
            (1, 0, 3, [false, true, false]),
            (1, 1, 1, [false, true, true]),
            (1, 0, 0, [false, true, true]),
            (0, 0, 0, [true, true, true]),
        ];
        let ops = ops_of(ch);
        let ams = self.reg(0x38 + ch) & 3;
        let am_offset = if ams == 0 { 0 } else { (self.lfo_am as u32) << (ams - 1) };
        let fb = (self.reg(0x20 + ch) >> 3) & 7;
        let opmod = if fb != 0 {
            (self.feedback[ch][0] as i32 + self.feedback[ch][1] as i32) >> (10 - fb)
        } else {
            0
        };
        let ph = |o: usize, m: i32| ((self.ops[o].phase >> 10) as i32).wrapping_add(m) as u32;
        let op1 = self.volume(ops[0], ph(ops[0], opmod), am_offset);
        self.feedback_in[ch] = op1 as i16;
        let pan = self.reg(0x20 + ch) >> 6;
        if pan == 0 {
            return;
        }
        let (in2, in3, in4, also) = ALG[(self.reg(0x20 + ch) & 7) as usize];
        let mut v = [0i16; 8];
        v[1] = op1 as i16;
        v[2] = self.volume(ops[1], ph(ops[1], v[in2] as i32 >> 1), am_offset) as i16;
        v[5] = v[1].wrapping_add(v[2]);
        v[3] = self.volume(ops[2], ph(ops[2], v[in3] as i32 >> 1), am_offset) as i16;
        v[6] = v[1].wrapping_add(v[3]);
        v[7] = v[2].wrapping_add(v[3]);
        let mut result = if self.reg(0x0F) & 0x80 != 0 && ch == 7 {
            self.noise_volume(ops[3], am_offset)
        } else {
            self.volume(ops[3], ph(ops[3], v[in4] as i32 >> 1), am_offset)
        };
        for (k, &heard) in also.iter().enumerate() {
            if heard {
                result = (result + v[1 + k] as i32).clamp(-32768, 32767);
            }
        }
        if pan & 1 != 0 {
            out[0] += result;
        }
        if pan & 2 != 0 {
            out[1] += result;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fnv(data: impl IntoIterator<Item = u32>) -> u64 {
        let mut h = 0xCBF2_9CE4_8422_2325u64;
        for v in data {
            for b in v.to_le_bytes() {
                h = (h ^ b as u64).wrapping_mul(0x0100_0000_01B3);
            }
        }
        h
    }

    #[test]
    fn the_chips_tables_come_out_as_on_the_die() {
        let t = tables();
        // A few values read from the die, then the whole of each as a check.
        assert_eq!(&t.wave[..4], &[0x859, 0x6C3, 0x607, 0x58B]);
        assert_eq!(t.power[0], (0x3FA | 0x400) << 2);
        assert_eq!(t.power[255], 0x400 << 2);
        assert_eq!(&t.phase[..9], &[41568, 41600, 41632, 41664, 41696, 41728, 41760, 41792, 41856]);
        assert_eq!(t.phase[767], 82976);
        assert_eq!(t.detune[0], [0, 0, 1, 2]);
        assert_eq!(t.detune[31], [0, 8, 16, 22]);
        // Checksums of the die's own tables (as published from the chip), so a platform whose
        // floating point rounds a value differently can't slip through.
        assert_eq!(fnv(t.wave.iter().map(|&v| v as u32)), 0xDA32_481A_C3B3_BC5D, "the sine table");
        assert_eq!(fnv(t.power.iter().map(|&v| v as u32)), 0xED23_22EE_D297_D038, "the power table");
        assert_eq!(fnv(t.phase.iter().copied()), 0x3B0F_96A4_7792_BDB3, "the note table");
    }

    #[test]
    fn the_dac_keeps_ten_bits() {
        assert_eq!(dac(511), 511);
        assert_eq!(dac(1023), 1022);
        assert_eq!(dac(-1), -1);
        assert_eq!(dac(32767), 32704);
        assert_eq!(dac(40000), 32767);
        assert_eq!(dac(-40000), -32768);
    }

    #[test]
    fn a_keyed_note_sounds_and_releases() {
        let mut y = Ym2151::default();
        let w = |r: u8, v: u8, y: &mut Ym2151| {
            y.write(0, r, 0);
            y.write(1, v, 0);
        };
        w(0x20, 0xC7, &mut y); // channel 0: both sides, algorithm 7 (all four heard)
        w(0x28, 0x4A, &mut y); // A4
        for (r, v) in [(0x40, 0x01), (0x60, 0x10), (0x80, 0x1F), (0xA0, 0x00), (0xC0, 0x00), (0xE0, 0x0F)] {
            w(r, v, &mut y);
        }
        for o in [0x48, 0x50, 0x58] {
            w(0x60 + (o - 0x40), 0x7F, &mut y); // the other three silent
        }
        w(0x08, 0x08, &mut y); // key on M1
        let loud = (0..2000).map(|_| y.sample(0).0.abs()).max().unwrap();
        assert!(loud > 1000, "{loud}");
        w(0x08, 0x00, &mut y);
        for _ in 0..20000 {
            y.sample(0);
        }
        assert_eq!(y.sample(0), (0, 0), "released to silence");
    }
}
