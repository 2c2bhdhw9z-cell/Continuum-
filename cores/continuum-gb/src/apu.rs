//! The four sound channels (two squares, the first with a sweep; the wave channel; noise), the
//! 512 Hz frame sequencer clocked by DIV, NR50-52, and a stereo mixer resampled to the output
//! rate. Register reads are masked as on hardware (write-only bits read as 1).

use serde::{Deserialize, Serialize};

pub const CPU_HZ: u32 = 4_194_304;
pub const OUT_HZ: u32 = 48_000;

const DUTY: [[u8; 8]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 0],
];

const MASK: [u8; 0x30] = [
    0x80, 0x3F, 0x00, 0xFF, 0xBF, 0xFF, 0x3F, 0x00, 0xFF, 0xBF, 0x7F, 0xFF, 0x9F, 0xFF, 0xBF, 0xFF,
    0xFF, 0x00, 0x00, 0xBF, 0x00, 0x00, 0x70, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

#[derive(Clone, Default, Serialize, Deserialize)]
struct Envelope {
    volume: u8,
    period: u8,
    timer: u8,
    up: bool,
}

impl Envelope {
    fn trigger(&mut self, reg: u8) {
        self.volume = reg >> 4;
        self.up = reg & 8 != 0;
        self.period = reg & 7;
        self.timer = if self.period == 0 { 8 } else { self.period };
    }
    fn clock(&mut self) {
        if self.period == 0 {
            return;
        }
        self.timer -= 1;
        if self.timer == 0 {
            self.timer = self.period;
            if self.up && self.volume < 15 {
                self.volume += 1;
            } else if !self.up && self.volume > 0 {
                self.volume -= 1;
            }
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Square {
    on: bool,
    duty: u8,
    step: u8,
    length: u16,
    length_on: bool,
    freq: u16,
    timer: u32,
    env: Envelope,
    // Sweep (channel 1 only).
    sweep_period: u8,
    sweep_down: bool,
    sweep_shift: u8,
    sweep_timer: u8,
    sweep_on: bool,
    shadow: u16,
    swept_down: bool,
}

impl Square {
    fn dac(reg2: u8) -> bool {
        reg2 & 0xF8 != 0
    }
    fn tick(&mut self, cycles: u32) {
        let period = (2048 - u32::from(self.freq)) * 4;
        self.timer += cycles;
        while self.timer >= period {
            self.timer -= period;
            self.step = (self.step + 1) & 7;
        }
    }
    fn out(&self) -> u8 {
        if self.on {
            DUTY[self.duty as usize][self.step as usize] * self.env.volume
        } else {
            0
        }
    }
    fn sweep_calc(&mut self) -> u16 {
        let delta = self.shadow >> self.sweep_shift;
        let f = if self.sweep_down {
            self.swept_down = true;
            self.shadow.wrapping_sub(delta)
        } else {
            self.shadow + delta
        };
        if f > 2047 {
            self.on = false;
        }
        f
    }
    fn sweep_clock(&mut self) {
        if self.sweep_timer > 0 {
            self.sweep_timer -= 1;
        }
        if self.sweep_timer == 0 {
            self.sweep_timer = if self.sweep_period == 0 { 8 } else { self.sweep_period };
            if self.sweep_on && self.sweep_period != 0 {
                let f = self.sweep_calc();
                if f <= 2047 && self.sweep_shift != 0 {
                    self.shadow = f;
                    self.freq = f;
                    self.sweep_calc();
                }
            }
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Wave {
    on: bool,
    length: u16,
    length_on: bool,
    freq: u16,
    timer: u32,
    pos: u8,
    shift: u8,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Noise {
    on: bool,
    length: u16,
    length_on: bool,
    env: Envelope,
    lfsr: u16,
    timer: u32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Apu {
    regs: Vec<u8>,
    power: bool,
    sq1: Square,
    sq2: Square,
    wave: Wave,
    noise: Noise,
    seq_step: u8,
    sample_acc: u32,
    #[serde(skip)]
    pub samples: Vec<i16>,
    /// Per-channel volume from the core options, percent (square 1, square 2, wave, noise).
    /// Output only, not saved.
    #[serde(skip, default = "full_volume")]
    pub chan_vol: [u8; 4],
    /// Output sample rate (core option). Not saved.
    #[serde(skip, default = "default_rate")]
    pub out_hz: u32,
    /// High-quality output: each sample is the average over its span instead of one point.
    #[serde(skip)]
    pub hq: bool,
    #[serde(skip)]
    acc: (i64, i64, i64),
}

fn default_rate() -> u32 {
    OUT_HZ
}

fn full_volume() -> [u8; 4] {
    [100; 4]
}

impl Default for Apu {
    fn default() -> Self {
        let mut apu = Apu {
            regs: vec![0; 0x30],
            power: true,
            sq1: Square::default(),
            sq2: Square::default(),
            wave: Wave::default(),
            noise: Noise { lfsr: 0x7FFF, ..Noise::default() },
            seq_step: 0,
            sample_acc: 0,
            samples: Vec::with_capacity(2048),
            chan_vol: [100; 4],
            out_hz: OUT_HZ,
            hq: false,
            acc: (0, 0, 0),
        };
        // The values the boot ROM leaves (channel 1 has just played the boot chime).
        let boot: [(u16, u8); 9] = [
            (0xFF10, 0x80), (0xFF11, 0xBF), (0xFF12, 0xF3), (0xFF14, 0xBF), (0xFF16, 0x3F),
            (0xFF19, 0xBF), (0xFF1A, 0x7F), (0xFF24, 0x77), (0xFF25, 0xF3),
        ];
        for (a, v) in boot {
            apu.regs[(a - 0xFF10) as usize] = v;
        }
        apu.sq1.duty = 2;
        apu.sq1.on = true;
        apu.regs[0x16] = 0x80;
        // Wave RAM's common power-up pattern.
        for i in 0..16 {
            apu.regs[0x20 + i] = if i % 2 == 0 { 0x00 } else { 0xFF };
        }
        apu
    }
}

impl Apu {
    pub fn read(&self, addr: u16) -> u8 {
        let i = (addr - 0xFF10) as usize;
        if addr == 0xFF26 {
            let mut v = 0x70 | if self.power { 0x80 } else { 0 };
            if self.sq1.on { v |= 1 }
            if self.sq2.on { v |= 2 }
            if self.wave.on { v |= 4 }
            if self.noise.on { v |= 8 }
            return v;
        }
        if (0x20..0x30).contains(&i) {
            return self.regs[i];
        }
        self.regs[i] | MASK[i]
    }

    pub fn write(&mut self, addr: u16, v: u8) {
        let i = (addr - 0xFF10) as usize;
        if (0x20..0x30).contains(&i) {
            self.regs[i] = v;
            return;
        }
        if addr == 0xFF26 {
            let on = v & 0x80 != 0;
            if self.power && !on {
                for r in 0..0x16 {
                    self.regs[r] = 0;
                }
                self.sq1 = Square::default();
                self.sq2 = Square::default();
                self.wave = Wave::default();
                self.noise = Noise { lfsr: 0x7FFF, ..Noise::default() };
            }
            if !self.power && on {
                self.seq_step = 0;
            }
            self.power = on;
            return;
        }
        if !self.power {
            // Lengths stay writable on the DMG while the APU is off.
            match addr {
                0xFF11 => self.sq1.length = 64 - u16::from(v & 0x3F),
                0xFF16 => self.sq2.length = 64 - u16::from(v & 0x3F),
                0xFF1B => self.wave.length = 256 - u16::from(v),
                0xFF20 => self.noise.length = 64 - u16::from(v & 0x3F),
                _ => {}
            }
            return;
        }
        self.regs[i] = v;
        match addr {
            0xFF10 => {
                self.sq1.sweep_period = (v >> 4) & 7;
                let down = v & 8 != 0;
                if self.sq1.swept_down && !down {
                    self.sq1.on = false;
                }
                self.sq1.sweep_down = down;
                self.sq1.sweep_shift = v & 7;
            }
            0xFF11 => {
                self.sq1.duty = v >> 6;
                self.sq1.length = 64 - u16::from(v & 0x3F);
            }
            0xFF12 => {
                if !Square::dac(v) {
                    self.sq1.on = false;
                }
            }
            0xFF13 => self.sq1.freq = (self.sq1.freq & 0x700) | u16::from(v),
            0xFF14 => {
                self.sq1.freq = (self.sq1.freq & 0xFF) | (u16::from(v & 7) << 8);
                let enable = v & 0x40 != 0;
                Self::length_enable(&mut self.sq1.length, &mut self.sq1.length_on, enable,
                                    &mut self.sq1.on, self.seq_step, 64, v & 0x80 != 0);
                if v & 0x80 != 0 {
                    let r2 = self.regs[2];
                    let s = &mut self.sq1;
                    s.on = Square::dac(r2);
                    s.env.trigger(r2);
                    s.timer = 0;
                    s.shadow = s.freq;
                    s.swept_down = false;
                    s.sweep_timer = if s.sweep_period == 0 { 8 } else { s.sweep_period };
                    s.sweep_on = s.sweep_period != 0 || s.sweep_shift != 0;
                    if s.sweep_shift != 0 {
                        s.sweep_calc();
                    }
                }
            }
            0xFF16 => {
                self.sq2.duty = v >> 6;
                self.sq2.length = 64 - u16::from(v & 0x3F);
            }
            0xFF17 => {
                if !Square::dac(v) {
                    self.sq2.on = false;
                }
            }
            0xFF18 => self.sq2.freq = (self.sq2.freq & 0x700) | u16::from(v),
            0xFF19 => {
                self.sq2.freq = (self.sq2.freq & 0xFF) | (u16::from(v & 7) << 8);
                Self::length_enable(&mut self.sq2.length, &mut self.sq2.length_on, v & 0x40 != 0,
                                    &mut self.sq2.on, self.seq_step, 64, v & 0x80 != 0);
                if v & 0x80 != 0 {
                    let r2 = self.regs[7];
                    self.sq2.on = Square::dac(r2);
                    self.sq2.env.trigger(r2);
                    self.sq2.timer = 0;
                }
            }
            0xFF1A => {
                if v & 0x80 == 0 {
                    self.wave.on = false;
                }
            }
            0xFF1B => self.wave.length = 256 - u16::from(v),
            0xFF1C => self.wave.shift = (v >> 5) & 3,
            0xFF1D => self.wave.freq = (self.wave.freq & 0x700) | u16::from(v),
            0xFF1E => {
                self.wave.freq = (self.wave.freq & 0xFF) | (u16::from(v & 7) << 8);
                Self::length_enable(&mut self.wave.length, &mut self.wave.length_on, v & 0x40 != 0,
                                    &mut self.wave.on, self.seq_step, 256, v & 0x80 != 0);
                if v & 0x80 != 0 {
                    self.wave.on = self.regs[0x0A] & 0x80 != 0;
                    self.wave.pos = 0;
                    self.wave.timer = 0;
                }
            }
            0xFF20 => self.noise.length = 64 - u16::from(v & 0x3F),
            0xFF21 => {
                if !Square::dac(v) {
                    self.noise.on = false;
                }
            }
            0xFF23 => {
                Self::length_enable(&mut self.noise.length, &mut self.noise.length_on,
                                    v & 0x40 != 0, &mut self.noise.on, self.seq_step, 64,
                                    v & 0x80 != 0);
                if v & 0x80 != 0 {
                    let r = self.regs[0x11];
                    self.noise.on = Square::dac(r);
                    self.noise.env.trigger(r);
                    self.noise.lfsr = 0x7FFF;
                    self.noise.timer = 0;
                }
            }
            _ => {}
        }
    }

    /// The length counter's enable and trigger rules, including the extra clock when it is
    /// enabled in the first half of a length period (the next sequencer step does not clock it).
    fn length_enable(length: &mut u16, on_flag: &mut bool, enable: bool, chan_on: &mut bool,
                     step: u8, full: u16, trigger: bool) {
        let was = *on_flag;
        *on_flag = enable;
        let first_half = step % 2 == 1; // the next step does not clock lengths
        if !was && enable && first_half && *length > 0 {
            *length -= 1;
            if *length == 0 && !trigger {
                *chan_on = false;
            }
        }
        if trigger && *length == 0 {
            *length = full;
            if enable && first_half {
                *length -= 1;
            }
        }
    }

    /// The frame sequencer step, on the falling edge of DIV bit 4 (bit 12 of the counter).
    pub fn frame_step(&mut self) {
        if !self.power {
            return;
        }
        let step = self.seq_step;
        self.seq_step = (step + 1) & 7;
        if step % 2 == 0 {
            for (len, on, ch_on) in [
                (&mut self.sq1.length, self.sq1.length_on, &mut self.sq1.on),
                (&mut self.sq2.length, self.sq2.length_on, &mut self.sq2.on),
                (&mut self.wave.length, self.wave.length_on, &mut self.wave.on),
                (&mut self.noise.length, self.noise.length_on, &mut self.noise.on),
            ] {
                if on && *len > 0 {
                    *len -= 1;
                    if *len == 0 {
                        *ch_on = false;
                    }
                }
            }
        }
        if step == 2 || step == 6 {
            self.sq1.sweep_clock();
        }
        if step == 7 {
            self.sq1.env.clock();
            self.sq2.env.clock();
            self.noise.env.clock();
        }
    }

    /// `t` T-cycles of sound (4 a CPU M-cycle, 2 at double speed).
    pub fn tick(&mut self, t: u32) {
        if self.power {
            self.sq1.tick(t);
            self.sq2.tick(t);
            let wp = (2048 - u32::from(self.wave.freq)) * 2;
            self.wave.timer += t;
            while self.wave.timer >= wp {
                self.wave.timer -= wp;
                self.wave.pos = (self.wave.pos + 1) & 31;
            }
            let nr43 = self.regs[0x12];
            let div = [8u32, 16, 32, 48, 64, 80, 96, 112][(nr43 & 7) as usize];
            let np = div << (nr43 >> 4);
            self.noise.timer += t;
            while self.noise.timer >= np {
                self.noise.timer -= np;
                let l = self.noise.lfsr;
                let x = (l ^ (l >> 1)) & 1;
                let mut n = (l >> 1) | (x << 14);
                if nr43 & 8 != 0 {
                    n = (n & !0x40) | (x << 6);
                }
                self.noise.lfsr = n;
            }
        }
        if self.hq {
            let (l, r) = self.mix();
            self.acc.0 += i64::from(l) * i64::from(t);
            self.acc.1 += i64::from(r) * i64::from(t);
            self.acc.2 += i64::from(t);
        }
        self.sample_acc += t * self.out_hz;
        if self.sample_acc >= CPU_HZ {
            self.sample_acc -= CPU_HZ;
            let (l, r) = if self.hq && self.acc.2 > 0 {
                let a = std::mem::take(&mut self.acc);
                ((a.0 / a.2) as i16, (a.1 / a.2) as i16)
            } else {
                self.mix()
            };
            self.samples.push(l);
            self.samples.push(r);
        }
    }

    fn mix(&self) -> (i16, i16) {
        if !self.power {
            return (0, 0);
        }
        let wave = if self.wave.on && self.wave.shift != 0 {
            let b = self.regs[0x20 + (self.wave.pos / 2) as usize];
            let s = if self.wave.pos % 2 == 0 { b >> 4 } else { b & 0x0F };
            s >> (self.wave.shift - 1)
        } else {
            0
        };
        let noise = if self.noise.on && self.noise.lfsr & 1 == 0 { self.noise.env.volume } else { 0 };
        let ch = [self.sq1.out(), self.sq2.out(), wave, noise];
        let nr51 = self.regs[0x15];
        let nr50 = self.regs[0x14];
        let (mut l, mut r) = (0i32, 0i32);
        for (i, v) in ch.iter().enumerate() {
            let v = i32::from(*v) * i32::from(self.chan_vol[i]) / 100;
            if nr51 & (0x10 << i) != 0 { l += v }
            if nr51 & (1 << i) != 0 { r += v }
        }
        let lv = i32::from((nr50 >> 4) & 7) + 1;
        let rv = i32::from(nr50 & 7) + 1;
        ((l * lv * 64) as i16, (r * rv * 64) as i16)
    }
}
