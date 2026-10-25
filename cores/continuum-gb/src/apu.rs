//! The sound chip: two square channels (the first with a frequency sweep), the wave channel and
//! the noise channel, the frame sequencer driven by DIV, NR50-52, and a stereo mixer resampled to
//! the output rate. Register reads are masked as on hardware (write-only bits read as 1).
//!
//! The chip is run at its own 2 MHz clock, tick by tick, the way SameBoy's author measured it on
//! real consoles (SameSuite's sound tests read the channels back a sample at a time through a
//! Color's PCM12 and PCM34 registers). Each channel's countdown reloads from its period a tick
//! after it runs out; a trigger starts the sound a few ticks late; the envelope can lock at the
//! end it was heading for; writing a volume register while a note plays nudges the volume
//! ("zombie mode"); the sweep recalculates after a delay; the noise channel counts on a 14-bit
//! counter whose bits clock its shift register, and writes to its control register can step it.
//! Revision E of the Color is the one modelled; the Game Boy's own differences are kept where
//! they are known, and the Advance's sound (which mixes digitally) uses the same channels.

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

// Register offsets from $FF10.
const NR10: usize = 0x00;
const NR30: usize = 0x0A;
const NR32: usize = 0x0C;
const NR41: usize = 0x10;
const NR42: usize = 0x11;
const NR43: usize = 0x12;
const NR44: usize = 0x13;
const NR50: usize = 0x14;
const NR51: usize = 0x15;
const WAVE: usize = 0x20;

/// NRx1 to NRx4 of square channel `i`.
fn sq_reg(i: usize, n: usize) -> usize {
    i * 5 + n
}

/// The channel state saved before the chip was modelled tick by tick. Kept field for field, so
/// a state from then still loads; such a state is carried over by `Apu::adopt_legacy`.
mod legacy {
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Default, Serialize, Deserialize)]
    pub struct Envelope {
        pub volume: u8,
        pub period: u8,
        pub timer: u8,
        pub up: bool,
    }

    #[derive(Clone, Default, Serialize, Deserialize)]
    pub struct Square {
        pub on: bool,
        pub duty: u8,
        pub step: u8,
        pub length: u16,
        pub length_on: bool,
        pub freq: u16,
        pub timer: u32,
        pub env: Envelope,
        pub sweep_period: u8,
        pub sweep_down: bool,
        pub sweep_shift: u8,
        pub sweep_timer: u8,
        pub sweep_on: bool,
        pub shadow: u16,
        pub swept_down: bool,
    }

    #[derive(Clone, Default, Serialize, Deserialize)]
    pub struct Wave {
        pub on: bool,
        pub length: u16,
        pub length_on: bool,
        pub freq: u16,
        pub timer: u32,
        pub pos: u8,
        pub shift: u8,
    }

    #[derive(Clone, Default, Serialize, Deserialize)]
    pub struct Noise {
        pub on: bool,
        pub length: u16,
        pub length_on: bool,
        pub env: Envelope,
        pub lfsr: u16,
        pub timer: u32,
    }
}

/// An envelope's clock line. When it rises the envelope notes whether the volume already sits
/// at the end it is heading for; when it falls, that locks the envelope until the next trigger.
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
struct EnvClock {
    locked: bool,
    high: bool,
    lock_next: bool,
}

impl EnvClock {
    fn set(&mut self, high: bool, up: bool, volume: u8) {
        if self.high == high {
            return;
        }
        self.high = high;
        if high {
            self.lock_next = (volume == 15 && up) || (volume == 0 && !up);
        } else {
            self.locked |= self.lock_next;
        }
    }
}

/// A square channel, counted in 2 MHz ticks.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Pulse {
    length: u16,
    length_on: bool,
    volume: u8,
    /// Envelope steps left before the volume moves.
    env_count: u8,
    /// Position in the duty cycle.
    index: u8,
    /// Just started: the first step has not been taken, so nothing is put out yet.
    quiet: bool,
    /// Ticks until the next step.
    countdown: u16,
    /// The 11-bit frequency value.
    period: u16,
    clock: EnvClock,
    /// Ticks still to go of the start-up delay after a trigger.
    delay: u8,
    /// Has stepped since it was triggered.
    stepped: bool,
    /// The countdown reloaded in the last tick run.
    just_reloaded: bool,
}

/// Channel 1's frequency sweep.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Sweep {
    /// 128 Hz steps, counting up to 7.
    countdown: u8,
    /// 1 MHz cycles until a recalculation lands (and is checked for overflow).
    calc: u8,
    /// 1 MHz cycles in which the recalculation is still being set up.
    reload: u8,
    addend: u16,
    shadow: u16,
    /// The recalculation was set up with a shift of 0.
    unshifted: bool,
    /// A recalculation with no delay is waiting for its set-up to end.
    instant: bool,
    /// Ticks after a trigger in which the period is not copied to the shadow.
    hold: u8,
    /// The addend of the last recalculation that landed.
    completed: u16,
}

/// The wave channel, counted in 2 MHz ticks.
#[derive(Clone, Default, Serialize, Deserialize)]
struct WaveCh {
    /// NR30's DAC switch.
    dac: bool,
    length: u16,
    length_on: bool,
    /// Right shift of each sample: 4 is silent.
    shift: u8,
    period: u16,
    countdown: u16,
    /// Which of the 32 samples.
    index: u8,
    /// The byte of wave memory last read.
    byte: u8,
    /// Wave memory was read in the last tick run (the Game Boy only lets the CPU in then).
    just_read: bool,
    /// Triggered since the DAC was last switched on.
    pulsed: bool,
    /// Ticks until a stray read of wave memory lands (DAC on, channel stopped).
    stray: u8,
}

/// The noise channel. Its 14-bit counter is clocked every few ticks (the divisor); a chosen bit
/// of it rising clocks the shift register, kept inverted (it starts at 0, and a 1 is sound).
#[derive(Clone, Default, Serialize, Deserialize)]
struct NoiseCh {
    length: u16,
    length_on: bool,
    volume: u8,
    env_count: u8,
    lfsr: u16,
    narrow: bool,
    countdown: u8,
    counter: u16,
    /// Ticks since power on, low bits: which phase of the 512 kHz clock it is in.
    phase: u8,
    out: bool,
    stepped_counter: bool,
    reloaded: bool,
    /// On the Game Boy a trigger out of phase starts a few ticks late.
    dmg_delay: u8,
    clock: EnvClock,
    counting: bool,
    background: bool,
    stepped_narrow: bool,
    bit7_before: bool,
    started_dac_off: bool,
}

/// Everything the chip holds, saved after the rest of the machine (see `Apu::chip`).
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Chip {
    /// The frame sequencer's step count (512 Hz events).
    divider: u8,
    /// 2: the next DIV event is skipped (the chip was switched on with DIV's bit high), 1: it was.
    skip: u8,
    /// The chip's 1 MHz phase.
    lf: u8,
    /// What each channel puts out, 0-15.
    amp: [u8; 4],
    active: [bool; 4],
    sq: [Pulse; 2],
    sweep: Sweep,
    wave: WaveCh,
    noise: NoiseCh,
    /// In double speed a Color's envelopes move a cycle after the DIV event.
    pending_env: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Apu {
    regs: Vec<u8>,
    power: bool,
    // The layout older states have (see `legacy`); not used while running.
    sq1: legacy::Square,
    sq2: legacy::Square,
    wave: legacy::Wave,
    noise: legacy::Noise,
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
    /// The chip itself: saved on its own after the rest of the machine.
    #[serde(skip)]
    pub chip: Chip,
    /// A Color's chip (set by the machine): only matters where the Game Boy's differs.
    #[serde(skip)]
    pub cgb: bool,
    /// An Advance's: it mixes digitally, so there are no per-channel DACs.
    #[serde(skip)]
    pub agb: bool,
    /// Double speed (set by the machine).
    #[serde(skip)]
    pub double_speed: bool,
    /// DIV's sequencer bit is high (set by the machine): switching the chip on then skips the
    /// first DIV event.
    #[serde(skip)]
    pub div_high: bool,
    /// The low address bits last on the bus, for the stray wave memory reads.
    #[serde(skip)]
    pub addr_bus: u16,
    /// A half tick left over (the Advance clocks it in odd amounts).
    #[serde(skip)]
    half: u32,
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
            sq1: legacy::Square::default(),
            sq2: legacy::Square::default(),
            wave: legacy::Wave::default(),
            noise: legacy::Noise { lfsr: 0x7FFF, ..legacy::Noise::default() },
            seq_step: 0,
            sample_acc: 0,
            samples: Vec::with_capacity(2048),
            chan_vol: [100; 4],
            out_hz: OUT_HZ,
            hq: false,
            acc: (0, 0, 0),
            chip: Chip::default(),
            cgb: false,
            agb: false,
            double_speed: false,
            div_high: false,
            addr_bus: 0,
            half: 0,
        };
        // The values the boot ROM leaves (channel 1 has just played the boot chime).
        let boot: [(u16, u8); 9] = [
            (0xFF10, 0x80), (0xFF11, 0xBF), (0xFF12, 0xF3), (0xFF14, 0xBF), (0xFF16, 0x3F),
            (0xFF19, 0xBF), (0xFF1A, 0x7F), (0xFF24, 0x77), (0xFF25, 0xF3),
        ];
        for (a, v) in boot {
            apu.regs[(a - 0xFF10) as usize] = v;
        }
        apu.regs[0x16] = 0x80;
        // Wave RAM's common power-up pattern.
        for i in 0..16 {
            apu.regs[WAVE + i] = if i % 2 == 0 { 0x00 } else { 0xFF };
        }
        apu.chip = Self::fresh_chip();
        apu.chip.active[0] = true;
        apu.sq1.on = true;
        apu
    }
}

impl Apu {
    /// The chip as switching it on leaves it.
    fn fresh_chip() -> Chip {
        let mut c = Chip { lf: 1, ..Chip::default() };
        c.wave.shift = 4;
        c.sq[0].countdown = 0xFFFF;
        c.sq[1].countdown = 0xFFFF;
        c
    }

    /// Carries a state saved before the chip was modelled tick by tick over to the new model:
    /// the registers are the same, the channels pick up where they were as near as they can.
    pub fn adopt_legacy(&mut self) {
        let mut c = Self::fresh_chip();
        for (i, s) in [&self.sq1, &self.sq2].into_iter().enumerate() {
            let p = &mut c.sq[i];
            c.active[i] = s.on && self.power;
            p.volume = s.env.volume & 0xF;
            p.env_count = s.env.timer & 7;
            p.length = s.length;
            p.length_on = s.length_on;
            p.period = s.freq & 0x7FF;
            p.index = s.step & 7;
            p.countdown = (p.period ^ 0x7FF) * 2 + 1;
        }
        c.sweep.shadow = self.sq1.shadow & 0x7FF;
        c.wave.dac = self.regs[NR30] & 0x80 != 0;
        c.active[2] = self.wave.on && self.power;
        c.wave.length = self.wave.length;
        c.wave.length_on = self.wave.length_on;
        c.wave.period = self.wave.freq & 0x7FF;
        c.wave.index = self.wave.pos & 31;
        c.wave.byte = self.regs[WAVE + usize::from(c.wave.index >> 1)];
        c.wave.countdown = c.wave.period ^ 0x7FF;
        c.wave.shift = [4, 0, 1, 2][usize::from((self.regs[NR32] >> 5) & 3)];
        c.active[3] = self.noise.on && self.power;
        c.noise.volume = self.noise.env.volume & 0xF;
        c.noise.env_count = self.noise.env.timer & 7;
        c.noise.length = self.noise.length;
        c.noise.length_on = self.noise.length_on;
        c.noise.lfsr = !self.noise.lfsr & 0x7FFF;
        c.noise.narrow = self.regs[NR43] & 8 != 0;
        c.noise.out = c.noise.lfsr & 1 != 0;
        c.noise.counting = c.active[3];
        c.noise.background = c.active[3];
        c.divider = self.seq_step;
        self.chip = c;
        for i in 0..2 {
            self.pulse_amp(i);
        }
        self.wave_amp();
        self.noise_amp();
    }

    // ------------------------------------------------------------ what the channels put out

    fn dac(&self, i: usize) -> bool {
        if self.agb {
            return true;
        }
        match i {
            0 | 1 => self.regs[sq_reg(i, 2)] & 0xF8 != 0,
            2 => self.chip.wave.dac,
            _ => self.regs[NR42] & 0xF8 != 0,
        }
    }

    /// A channel's output changing. With its DAC off it holds what it had.
    fn set_amp(&mut self, i: usize, v: u8) {
        if self.agb {
            self.chip.amp[i] = v;
            return;
        }
        if v == 0 && self.chip.amp[i] == 0 {
            return;
        }
        if self.dac(i) {
            self.chip.amp[i] = v;
        }
    }

    fn pulse_amp(&mut self, i: usize) {
        let p = &self.chip.sq[i];
        if p.quiet {
            return;
        }
        let duty = usize::from(self.regs[sq_reg(i, 1)] >> 6);
        let v = if DUTY[duty][usize::from(p.index & 7)] != 0 { p.volume } else { 0 };
        self.set_amp(i, v);
    }

    fn wave_amp(&mut self) {
        let w = &self.chip.wave;
        let nibble = if w.index & 1 != 0 { w.byte & 0xF } else { w.byte >> 4 };
        let v = if w.shift >= 4 { 0 } else { nibble >> w.shift };
        self.set_amp(2, v);
    }

    fn noise_amp(&mut self) {
        let v = if self.chip.noise.out { self.chip.noise.volume } else { 0 };
        self.set_amp(3, v);
    }

    fn silence(&mut self, i: usize) {
        self.chip.active[i] = false;
        self.set_amp(i, 0);
    }

    // ------------------------------------------------------------ envelopes

    fn pulse_envelope(&mut self, i: usize) {
        let nrx2 = self.regs[sq_reg(i, 2)];
        let p = &mut self.chip.sq[i];
        p.clock.set(false, false, 0);
        if p.clock.locked || nrx2 & 7 == 0 {
            return;
        }
        p.volume = if nrx2 & 8 != 0 { p.volume.wrapping_add(1) } else { p.volume.wrapping_sub(1) } & 0xF;
        if self.chip.active[i] {
            self.pulse_amp(i);
        }
    }

    fn noise_envelope(&mut self) {
        let nr42 = self.regs[NR42];
        let n = &mut self.chip.noise;
        n.clock.set(false, false, 0);
        if n.clock.locked || nr42 & 7 == 0 {
            return;
        }
        n.volume = if nr42 & 8 != 0 { n.volume.wrapping_add(1) } else { n.volume.wrapping_sub(1) } & 0xF;
        if self.chip.active[3] {
            self.noise_amp();
        }
    }

    fn envelope_ticks(&mut self) {
        for i in 0..2 {
            if self.chip.sq[i].clock.high {
                self.pulse_envelope(i);
            }
        }
        if self.chip.noise.clock.high {
            self.noise_envelope();
        }
    }

    /// Writing NRx2 while the channel plays nudges the volume the way the envelope's own
    /// circuits do when their inputs change under them. The Game Boy goes through $FF on the way.
    fn zombie(&self, volume: &mut u8, new: u8, old: u8, count: &mut u8, clock: &mut EnvClock) {
        if self.cgb || self.agb {
            Self::zombie_step(volume, new, old, count, clock);
        } else {
            Self::zombie_step(volume, 0xFF, old, count, clock);
            Self::zombie_step(volume, new, 0xFF, count, clock);
        }
    }

    fn zombie_step(volume: &mut u8, new: u8, old: u8, count: &mut u8, clock: &mut EnvClock) {
        if clock.high {
            *count = new & 7;
        }
        let mut step = new & 7 != 0 && old & 7 == 0 && !clock.locked;
        if new & 0xF == 8 && old & 0xF == 8 && !clock.locked {
            step = true;
        }
        if (new ^ old) & 8 != 0 {
            if new & 8 != 0 {
                *volume = if old & 7 == 0 && !clock.locked { *volume ^ 0xF } else { 0xEu8.wrapping_sub(*volume) & 0xF };
                step = false;
            } else {
                *volume = 0x10u8.wrapping_sub(*volume) & 0xF;
            }
        }
        if step {
            *volume = if new & 8 != 0 { volume.wrapping_add(1) } else { volume.wrapping_sub(1) } & 0xF;
        } else if new & 7 == 0 && clock.high {
            clock.set(false, false, 0);
        }
    }

    // ------------------------------------------------------------ the sweep

    fn sweep_done(&mut self) {
        let neg = self.regs[NR10] & 8 != 0;
        let s = &mut self.chip.sweep;
        if s.hold == 0 {
            s.shadow = self.chip.sq[0].period;
        }
        if neg {
            s.addend ^= 0x7FF;
        }
        let over = s.shadow + s.addend > 0x7FF && !neg;
        s.completed = s.addend;
        if over {
            self.silence(0);
        }
    }

    fn sweep_trigger(&mut self) {
        let nr10 = self.regs[NR10];
        let s = &mut self.chip.sweep;
        if nr10 & 0x70 == 0 || s.countdown != 7 {
            return;
        }
        if nr10 & 7 != 0 {
            self.chip.sq[0].period = (s.addend + s.shadow + u16::from(nr10 & 8 != 0)) & 0x7FF;
        }
        if s.hold == 0 {
            s.addend = self.chip.sq[0].period >> (nr10 & 7);
        }
        s.calc = nr10 & 7;
        s.reload = 1 + self.chip.lf;
        s.unshifted = nr10 & 7 == 0;
        s.countdown = ((nr10 >> 4) & 7) ^ 7;
        if s.calc == 0 {
            s.instant = true;
        }
    }

    fn nr10_glitch(&mut self, v: u8) {
        let nr10 = self.regs[NR10];
        let lf = self.chip.lf;
        let s = &mut self.chip.sweep;
        let mut done = false;
        if self.cgb || self.agb {
            if s.reload == 2 {
                s.calc = v & 7;
                if s.calc == 0 {
                    s.reload = 0;
                }
            }
            if v & 7 != 0 && nr10 & 7 == 0 && lf == 0 && s.calc > 1 {
                s.calc -= 1;
                done = s.calc == 0;
            }
        } else if s.reload == 0 && s.calc != 0 && nr10 & 7 == 0 && lf != 0 {
            // The Game Boy: a write with the recalculation under way and no shift steps it.
            s.calc -= 1;
            if s.calc <= 1 {
                s.calc = 0;
                done = true;
            }
        }
        if done {
            self.sweep_done();
        }
    }

    // ------------------------------------------------------------ the frame sequencer

    /// The DIV event: DIV's sequencer bit falling (bit 4 of DIV, bit 5 in double speed).
    pub fn frame_step(&mut self) {
        if !self.power {
            return;
        }
        match self.chip.skip {
            2 => {
                self.chip.skip = 1;
                return;
            }
            1 => self.chip.skip = 0,
            _ => self.chip.divider = self.chip.divider.wrapping_add(1),
        }
        let div = self.chip.divider;
        if div & 7 == 7 {
            for p in &mut self.chip.sq {
                if !p.clock.high {
                    p.env_count = p.env_count.wrapping_sub(1) & 7;
                }
            }
            let n = &mut self.chip.noise;
            if !n.clock.high {
                n.env_count = n.env_count.wrapping_sub(1) & 7;
            }
        }
        if self.double_speed && self.cgb {
            self.chip.pending_env = true;
        } else {
            self.envelope_ticks();
        }
        if div & 1 == 1 {
            for i in 0..2 {
                let p = &mut self.chip.sq[i];
                if p.length_on && p.length > 0 {
                    p.length -= 1;
                    if p.length == 0 {
                        self.silence(i);
                    }
                }
            }
            let w = &mut self.chip.wave;
            if w.length_on && w.length > 0 {
                w.length -= 1;
                if w.length == 0 {
                    self.silence(2);
                }
            }
            let n = &mut self.chip.noise;
            if n.length_on && n.length > 0 {
                n.length -= 1;
                if n.length == 0 {
                    self.silence(3);
                }
            }
        }
        if div & 3 == 3 {
            self.chip.sweep.countdown = (self.chip.sweep.countdown + 1) & 7;
            self.sweep_trigger();
        }
    }

    /// DIV's sequencer bit rising: an envelope that has counted down gets its clock set up.
    pub fn frame_rise(&mut self) {
        if !self.power {
            return;
        }
        for i in 0..2 {
            let nrx2 = self.regs[sq_reg(i, 2)];
            let active = self.chip.active[i];
            let p = &mut self.chip.sq[i];
            if active && p.env_count == 0 {
                p.env_count = nrx2 & 7;
                p.clock.set(p.env_count != 0, nrx2 & 8 != 0, p.volume);
            }
        }
        let nr42 = self.regs[NR42];
        let active = self.chip.active[3];
        let n = &mut self.chip.noise;
        if active && n.env_count == 0 {
            n.env_count = nr42 & 7;
            n.clock.set(n.env_count != 0, nr42 & 8 != 0, n.volume);
        }
    }

    // ------------------------------------------------------------ the noise channel

    fn lfsr_out(&mut self) {
        self.chip.noise.out = self.chip.noise.lfsr & 1 != 0;
        if self.chip.active[3] {
            self.noise_amp();
        }
    }

    fn step_lfsr(&mut self) {
        let n = &mut self.chip.noise;
        n.bit7_before = n.lfsr & 0x80 != 0;
        let high = if n.narrow { 0x4040 } else { 0x4000 };
        let bit = (n.lfsr ^ (n.lfsr >> 1) ^ 1) & 1 != 0;
        n.lfsr >>= 1;
        if bit {
            n.lfsr |= high;
        } else {
            n.lfsr &= !high;
        }
        self.lfsr_out();
        self.chip.noise.stepped_narrow = self.chip.noise.narrow;
    }

    /// A trigger sets the counter's next reload by where in the chip's 512 kHz cycle it lands.
    fn noise_start(&mut self) {
        let old_rev = !self.cgb && !self.agb;
        let active = self.chip.active[3];
        let nr42 = self.regs[NR42];
        let nr43 = self.regs[NR43];
        let n = &mut self.chip.noise;
        n.counting = nr42 & 0xF8 != 0;
        let was_dac_off = n.started_dac_off;
        n.started_dac_off = !n.counting;
        let mut divisor = nr43 & 7;
        let was_background = n.background;
        n.background = true;
        let mut instant = false;
        let mut div1_glitch = false;
        if divisor > 1 && n.countdown == 1 {
            n.counter = (n.counter + 1) & 0x3FFF;
        } else if n.countdown == 2 && n.phase & 3 == 0 && active {
            if divisor == 0 {
                divisor = 8;
            } else if divisor == 1 {
                if !n.stepped_counter {
                    div1_glitch = true;
                }
                let mask = 1u32 << (nr43 >> 4);
                let old = u32::from(n.counter) & mask != 0;
                n.counter = (n.counter + 1) & 0x3FFF;
                instant = u32::from(n.counter) & mask != 0 && !old;
            }
        }
        let mut c: i32 = if divisor == 0 { 6 } else { i32::from(divisor) * 4 + 6 };
        if n.phase & 1 != 0 {
            if divisor == 0 {
                if old_rev || !was_background {
                    c += 1;
                } else {
                    c -= 1;
                }
            } else if n.phase & 2 != 0 {
                if divisor == 1 && !active {
                    c += 1;
                } else {
                    c -= 3;
                }
            } else {
                c -= 1;
                if divisor == 1 && active {
                    c -= 4;
                }
            }
        } else if divisor != 0 {
            if n.phase & 2 != 0 {
                c -= 2;
            } else if divisor > 1 || (active && nr43 & 0xF0 == 0) {
                c -= 4;
            }
        }
        if divisor > 1 {
            if !n.counting && n.phase & 3 == 0 {
                c += 4;
            }
        } else if was_background && !active && n.phase & 3 == 0 {
            if divisor == 0 {
                if was_dac_off {
                    c += 28;
                }
            } else {
                c -= 4;
            }
        }
        if div1_glitch {
            c -= 4;
        }
        n.countdown = c.clamp(0, 255) as u8;
        n.lfsr = if divisor == 0 && active && n.phase & 3 == 3 { 0x0055 } else { 0 };
        if instant {
            self.step_lfsr();
        }
    }

    /// NR43 written: the counter bit that clocks the shift register changes, and on its way from
    /// the old choice to the new one it can pass through a bit that clocks it (Color revision E).
    fn nr43_write(&mut self, new: u8) {
        let old_narrow = self.chip.noise.narrow;
        self.chip.noise.narrow = new & 8 != 0;
        let old = self.regs[NR43];
        self.regs[NR43] = new;
        if (old ^ new) & 0xF0 == 0 {
            return;
        }
        let old_rev = !self.cgb && !self.agb;
        let mut counter = self.chip.noise.counter;
        if old_rev && self.chip.noise.reloaded {
            counter |= counter.wrapping_sub(1) & 0x3FFF;
        }
        let bit = |shift: u8| (counter >> shift) & 1 != 0;
        let old_bit = bit(old >> 4);
        let glitch = (old & 0x7F) | (new & 0x80);
        let glitch_bit = bit(glitch >> 4);
        let new_bit = bit(new >> 4);
        if old_bit == new_bit && new_bit != glitch_bit {
            if new_bit {
                if old_rev {
                    return;
                }
                if new & 0x80 == 0 {
                    self.step_lfsr();
                    return;
                }
                let t1 = (old >> 4) & 7;
                let t2 = (new >> 4) & 7;
                if (t1 ^ 7) + t2 > 7 || (t1 ^ 7) & t2 != 0 {
                    let n = &mut self.chip.noise;
                    n.lfsr = (n.lfsr & !0x80) | ((n.lfsr >> 1) & 0x80);
                    if (t1 == 0 || t1 == 4) && t2 == 3 {
                        n.lfsr &= (n.lfsr >> 1) | 0x545;
                        self.lfsr_out();
                    } else if t1 == 2 && t2 == 3 {
                        let mut mask = 0x555;
                        if n.lfsr & 0xC == 0xC {
                            mask |= 8;
                        }
                        if n.lfsr & 0xC00 == 0xC00 {
                            mask |= 0x800;
                        }
                        n.lfsr &= (n.lfsr >> 1) | mask;
                        self.lfsr_out();
                    }
                    let n = &mut self.chip.noise;
                    if !n.narrow && old_narrow && n.stepped_narrow {
                        if n.bit7_before {
                            n.lfsr |= 0x40;
                        } else {
                            n.lfsr &= !0x40;
                        }
                    }
                    n.lfsr |= if n.narrow { 0x4040 } else { 0x4000 };
                    n.stepped_narrow = n.narrow;
                }
            } else {
                if old_rev {
                    self.step_lfsr();
                    return;
                }
                // Which glitch a change between two bit choices makes, by old (rows) and new
                // (columns) choice, when the new one is 8 or above.
                const MAP: [[u8; 8]; 8] = [
                    [0, 0, 4, 2, 2, 2, 0, 0],
                    [0, 0, 2, 4, 2, 2, 0, 0],
                    [1, 2, 0, 1, 5, 3, 0, 0],
                    [0, 0, 0, 0, 2, 2, 0, 0],
                    [0, 2, 2, 2, 0, 0, 0, 0],
                    [6, 0, 2, 2, 0, 0, 0, 0],
                    [0; 8],
                    [0; 8],
                ];
                let kind = if new & 0x80 != 0 { MAP[usize::from((old >> 4) & 7)][usize::from((new >> 4) & 7)] } else { 0 };
                match kind {
                    1 | 6 => {
                        self.step_lfsr();
                        let n = &mut self.chip.noise;
                        if kind == 6 {
                            if (n.narrow && n.lfsr & 0x71 == 0x20) || n.lfsr & 0x71 == 0x61 {
                                n.lfsr &= !0x20;
                            }
                            if n.lfsr & 0x7001 == 0x2000 || n.lfsr & 0x7001 == 0x6001 {
                                n.lfsr &= !0x2000;
                            }
                        }
                        if n.lfsr & 3 == 2 {
                            n.lfsr &= !2;
                        }
                    }
                    2 => {
                        let prev = self.chip.noise.lfsr;
                        self.step_lfsr();
                        self.chip.noise.lfsr &= prev | 1;
                    }
                    3 | 5 => {
                        let n = &mut self.chip.noise;
                        if kind == 5 {
                            if n.lfsr & 3 == 2 {
                                n.lfsr &= if n.narrow { !0x4040 } else { !0x4000 };
                            }
                            if n.lfsr & 0x19 == 8 {
                                n.lfsr &= !8;
                            }
                        }
                        n.lfsr = (n.lfsr & !1) | ((n.lfsr >> 1) & 1);
                        self.lfsr_out();
                        self.chip.noise.stepped_narrow = self.chip.noise.narrow;
                    }
                    4 => {
                        let prev = self.chip.noise.lfsr;
                        self.step_lfsr();
                        let n = &mut self.chip.noise;
                        n.lfsr &= prev | if n.narrow { !0x2022 } else { !0x2002 };
                    }
                    _ => self.step_lfsr(),
                }
            }
        } else if !old_bit && new_bit {
            if old_rev {
                let narrow = self.chip.noise.narrow;
                self.chip.noise.narrow = true;
                self.step_lfsr();
                self.chip.noise.narrow = narrow;
                if new & 0xF0 <= 0x20 && glitch_bit && counter & 8 == 0 {
                    self.step_lfsr();
                    let n = &mut self.chip.noise;
                    let (top, below) = if n.narrow { (0x4040, 0x2020) } else { (0x4000, 0x2000) };
                    n.lfsr &= !top;
                    n.lfsr |= (n.lfsr & below) << 1;
                }
            } else {
                self.step_lfsr();
            }
        } else if old_rev && new & 0xF0 <= 0x20 && !glitch_bit && !new_bit && !old_bit && counter & 8 != 0 {
            self.step_lfsr();
        }
    }

    // ------------------------------------------------------------ running

    /// `n` ticks of the chip's 2 MHz clock.
    fn run(&mut self, mut n: u16) {
        if n == 0 {
            return;
        }
        if self.chip.wave.stray != 0 {
            for _ in 0..n {
                self.chip.wave.stray -= 1;
                if self.chip.wave.stray == 0 {
                    self.chip.wave.byte = self.regs[WAVE + usize::from(self.addr_bus & 0xF)];
                    if self.chip.active[2] {
                        self.wave_amp();
                    }
                    break;
                }
            }
        }
        let mut start_noise = false;
        if self.chip.noise.dmg_delay != 0 {
            let d = u16::from(self.chip.noise.dmg_delay);
            if d < n {
                self.run(d);
                n -= d;
            } else if d == n {
                self.chip.noise.dmg_delay = 0;
                start_noise = true;
            } else {
                self.chip.noise.dmg_delay -= n as u8;
            }
        }
        self.chip.lf ^= (n & 1) as u8;
        self.chip.noise.phase = self.chip.noise.phase.wrapping_add(n as u8);

        // The sweep counts at 1 MHz.
        let mut sweep_n = n / 2 + u16::from(n & 1 == 1 && self.chip.lf == 0);
        let s = &mut self.chip.sweep;
        if u16::from(s.reload) > sweep_n {
            s.reload -= sweep_n as u8;
            sweep_n = 0;
        } else {
            let done = s.reload != 0 && s.calc == 0 && s.instant;
            s.instant = false;
            sweep_n -= u16::from(s.reload);
            s.reload = 0;
            if done {
                self.sweep_done();
            }
        }
        let s = &mut self.chip.sweep;
        if s.calc != 0 && (self.regs[NR10] & 7 != 0 || s.unshifted) {
            if u16::from(s.calc) > sweep_n {
                s.calc -= sweep_n as u8;
            } else {
                s.calc = 0;
                self.sweep_done();
            }
        }
        let s = &mut self.chip.sweep;
        s.hold = if u16::from(s.hold) > n { s.hold - n as u8 } else { 0 };

        for i in 0..2 {
            if !self.chip.active[i] {
                continue;
            }
            let mut left = n;
            let p = &mut self.chip.sq[i];
            if p.delay != 0 {
                p.delay = if u16::from(p.delay) < left { 0 } else { p.delay - left as u8 };
            }
            while left > self.chip.sq[i].countdown {
                let p = &mut self.chip.sq[i];
                left -= p.countdown + 1;
                p.countdown = (p.period ^ 0x7FF) * 2 + 1;
                p.index = (p.index + 1) & 7;
                p.quiet = false;
                p.stepped = true;
                self.pulse_amp(i);
            }
            let p = &mut self.chip.sq[i];
            p.just_reloaded = left == 0;
            if left != 0 {
                p.countdown -= left;
            }
        }

        self.chip.wave.just_read = false;
        if self.chip.active[2] {
            let mut left = n;
            while left > self.chip.wave.countdown {
                let w = &mut self.chip.wave;
                left -= w.countdown + 1;
                w.countdown = w.period ^ 0x7FF;
                w.index = (w.index + 1) & 31;
                w.byte = self.regs[WAVE + usize::from(w.index >> 1)];
                self.wave_amp();
                self.chip.wave.just_read = true;
            }
            if left != 0 {
                self.chip.wave.countdown -= left;
                self.chip.wave.just_read = false;
            }
        } else if self.chip.wave.dac && self.chip.wave.pulsed && !self.agb {
            let mut left = n;
            let w = &mut self.chip.wave;
            while left > w.countdown {
                left -= w.countdown + 1;
                w.countdown = w.period ^ 0x7FF;
                if left != 0 {
                    w.byte = self.regs[WAVE + usize::from(self.addr_bus & 0xF)];
                } else {
                    w.stray = 1;
                }
            }
            if left != 0 {
                w.countdown -= left;
            }
            if w.countdown == 0 {
                w.stray = 2;
            }
        }

        if self.chip.noise.counting || self.chip.noise.background {
            let mut left = n;
            let nr43 = self.regs[NR43];
            let mut divisor = u16::from(nr43 & 7) << 2;
            if divisor == 0 {
                divisor = 2;
            }
            if self.chip.noise.countdown == 0 {
                self.chip.noise.countdown = divisor as u8;
            }
            while left >= u16::from(self.chip.noise.countdown) {
                let nz = &mut self.chip.noise;
                left -= u16::from(nz.countdown);
                nz.countdown = divisor as u8;
                let mask = 1u32 << (nr43 >> 4);
                let old = u32::from(nz.counter) & mask != 0;
                nz.counter = (nz.counter + 1) & 0x3FFF;
                nz.stepped_counter = true;
                if u32::from(nz.counter) & mask != 0 && !old && self.chip.active[3] {
                    self.step_lfsr();
                }
            }
            let nz = &mut self.chip.noise;
            if left != 0 {
                nz.countdown -= left as u8;
                nz.reloaded = false;
            } else {
                nz.reloaded = true;
            }
        }
        if start_noise {
            let v = self.regs[NR44] | 0x80;
            self.write(0xFF23, v);
        }
    }

    // ------------------------------------------------------------ the CPU's side

    pub fn read(&self, addr: u16) -> u8 {
        let mut i = (addr - 0xFF10) as usize;
        if addr == 0xFF26 {
            let mut v = 0x70 | if self.power { 0x80 } else { 0 };
            for (k, on) in self.chip.active.iter().enumerate() {
                if *on {
                    v |= 1 << k;
                }
            }
            return v;
        }
        if i >= WAVE {
            if self.chip.active[2] {
                if self.agb || (!self.cgb && !self.chip.wave.just_read) {
                    return 0xFF;
                }
                i = WAVE + usize::from(self.chip.wave.index >> 1);
            }
            return self.regs[i];
        }
        self.regs[i] | MASK[i]
    }

    pub fn write(&mut self, addr: u16, mut v: u8) {
        let mut i = (addr - 0xFF10) as usize;
        let dmg = !self.cgb && !self.agb;
        if !self.power && addr != 0xFF26 && i < WAVE && !(dmg && matches!(addr, 0xFF11 | 0xFF16 | 0xFF1B | 0xFF20)) {
            return;
        }
        if i >= WAVE {
            if self.chip.active[2] {
                if self.agb || (dmg && !self.chip.wave.just_read) {
                    return;
                }
                i = WAVE + usize::from(self.chip.wave.index >> 1);
            }
            self.regs[i] = v;
            return;
        }
        match i {
            NR50 | NR51 => {}
            0x16 => {
                let lengths = [self.chip.sq[0].length, self.chip.sq[1].length, self.chip.wave.length, self.chip.noise.length];
                if v & 0x80 != 0 && !self.power {
                    self.chip = Self::fresh_chip();
                    if self.div_high {
                        self.chip.skip = 2;
                        self.chip.divider = 1;
                    }
                    self.power = true;
                } else if v & 0x80 == 0 && self.power {
                    self.chip = Chip { lf: 0, ..Chip::default() };
                    self.chip.wave.shift = 4;
                    for r in &mut self.regs[..0x16] {
                        *r = 0;
                    }
                    self.power = false;
                }
                if dmg && v & 0x80 != 0 {
                    self.chip.sq[0].length = lengths[0];
                    self.chip.sq[1].length = lengths[1];
                    self.chip.wave.length = lengths[2];
                    self.chip.noise.length = lengths[3];
                }
                return;
            }
            NR10 => {
                if self.chip.sweep.calc != 0 || self.chip.sweep.reload != 0 {
                    self.nr10_glitch(v);
                }
                let old_neg = self.regs[NR10] & 8 != 0 || dmg;
                self.regs[NR10] = v;
                let s = &self.chip.sweep;
                if s.shadow + s.completed + u16::from(old_neg) > 0x7FF && v & 8 == 0 {
                    self.silence(0);
                }
                self.sweep_trigger();
            }
            1 | 6 => {
                let ch = i / 5;
                self.chip.sq[ch].length = 0x40 - u16::from(v & 0x3F);
                if !self.power {
                    v &= 0x3F;
                }
            }
            2 | 7 => {
                let ch = i / 5;
                if v & 0xF8 == 0 {
                    self.regs[i] = v;
                    self.silence(ch);
                } else if self.chip.active[ch] {
                    let old = self.regs[i];
                    let mut p = std::mem::take(&mut self.chip.sq[ch]);
                    self.zombie(&mut p.volume, v, old, &mut p.env_count, &mut p.clock);
                    self.chip.sq[ch] = p;
                    self.pulse_amp(ch);
                }
            }
            3 | 8 => {
                let p = &mut self.chip.sq[i / 5];
                p.period = (p.period & 0x700) | u16::from(v);
                if p.just_reloaded {
                    p.countdown = (p.period ^ 0x7FF) * 2 + 1;
                }
            }
            4 | 9 => self.pulse_nrx4(i / 5, v),
            NR30 => {
                self.chip.wave.dac = v & 0x80 != 0;
                if !self.chip.wave.dac {
                    self.chip.wave.pulsed = false;
                    if self.chip.active[2] && self.chip.wave.countdown == 0 && !self.agb {
                        self.chip.wave.byte = self.regs[WAVE + usize::from(self.addr_bus & 0xF)];
                    } else if self.chip.active[2] && self.chip.wave.just_read && dmg {
                        self.chip.wave.byte = self.regs[WAVE + NR30];
                    }
                    self.silence(2);
                }
            }
            0x0B => self.chip.wave.length = 0x100 - u16::from(v),
            NR32 => {
                self.chip.wave.shift = [4, 0, 1, 2][usize::from((v >> 5) & 3)];
                if self.chip.active[2] {
                    self.wave_amp();
                }
            }
            0x0D => {
                let w = &mut self.chip.wave;
                w.period = (w.period & 0x700) | u16::from(v);
                if w.stray == 1 {
                    w.countdown = w.period ^ 0x7FF;
                }
            }
            0x0E => self.wave_nr34(v),
            NR41 => self.chip.noise.length = 0x40 - u16::from(v & 0x3F),
            NR42 => {
                if v & 0xF8 == 0 {
                    if self.chip.active[3] && self.regs[NR43] & 7 != 0 {
                        if self.chip.noise.countdown <= 2 {
                            self.chip.noise.counter = self.chip.noise.counter.wrapping_add(1);
                        }
                        self.chip.noise.background = false;
                    }
                    self.regs[NR42] = v;
                    self.silence(3);
                    self.chip.noise.counting = false;
                } else if self.chip.active[3] {
                    let old = self.regs[NR42];
                    let mut n = std::mem::take(&mut self.chip.noise);
                    self.zombie(&mut n.volume, v, old, &mut n.env_count, &mut n.clock);
                    self.chip.noise = n;
                    self.noise_amp();
                }
            }
            NR43 => {
                if self.chip.noise.reloaded {
                    let mut divisor = u16::from(v & 7) << 2;
                    if divisor == 0 {
                        divisor = 2;
                    }
                    let extra = if divisor == 2 { 0 } else if dmg { [2, 1, 4, 3][usize::from(self.chip.noise.phase & 3)] } else { [2, 1, 0, 3][usize::from(self.chip.noise.phase & 3)] };
                    self.chip.noise.countdown = (divisor + extra) as u8;
                }
                if dmg {
                    // The Game Boy: a write as the counter reloads can clock it on the old count.
                    let n = &self.chip.noise;
                    if n.reloaded {
                        let old = self.regs[NR43];
                        let bits = |c: u16| ((c >> (old >> 4)) & 1 != 0, (c >> 7) & 1 != 0, (c >> (v >> 4)) & 1 != 0);
                        let (o, g, nb) = bits(n.counter);
                        if !o && nb && g {
                            let (o2, g2, n2) = bits(n.counter.wrapping_sub(1) & 0x3FFF);
                            if o2 && !n2 && g2 {
                                self.step_lfsr();
                            }
                        }
                    }
                    self.nr43_write(0xFF);
                }
                self.nr43_write(v);
            }
            NR44 => self.noise_nr44(v),
            _ => {}
        }
        self.regs[i] = v;
    }

    /// NR14 or NR24: the period's top bits, the length switch, and the trigger.
    fn pulse_nrx4(&mut self, ch: usize, v: u8) {
        let reg = sq_reg(ch, 4);
        let ce = self.cgb && !self.agb;
        let was_active = self.chip.active[ch];
        {
            let old_top = self.regs[reg] & 7;
            let p = &mut self.chip.sq[ch];
            // A period leaving $7xx right as it would reload steps back (the countdown reloads
            // from the old period; the sample index must not move).
            if v & 0x80 == 0 && was_active && old_top == 7 && v & 7 != 7 && (ce || p.countdown & 1 != 0)
                && p.stepped && p.countdown >> 1 == (p.period ^ 0x7FF) {
                p.index = p.index.wrapping_sub(1) & 7;
                p.quiet = false;
            }
        }
        let old_period = self.chip.sq[ch].period;
        let lf = self.chip.lf;
        {
            let p = &mut self.chip.sq[ch];
            p.period = (p.period & 0xFF) | (u16::from(v & 7) << 8);
            if p.just_reloaded {
                p.countdown = (p.period ^ 0x7FF) * 2 + 1;
            }
        }
        if v & 0x80 != 0 {
            let nrx2 = self.regs[sq_reg(ch, 2)];
            let mut unquiet = false;
            {
                let p = &mut self.chip.sq[ch];
                p.clock.locked = false;
                p.clock.high = false;
                p.stepped = false;
                if !was_active {
                    if ce && v & 4 == 0 && ((i32::from(p.countdown) - i32::from(p.delay)) / 2) & 0x400 == 0 {
                        p.index = (p.index + 1) & 7;
                        unquiet = true;
                    }
                    let up = !ce && self.double_speed;
                    p.delay = if up { 6 + lf } else { 6 - lf };
                    p.countdown = (p.period ^ 0x7FF) * 2 + u16::from(p.delay);
                } else {
                    let mut extra = 0;
                    if ce {
                        if !p.just_reloaded && v & 4 == 0 && ((i32::from(p.countdown) - 1 - i32::from(p.delay)) / 2) & 0x400 == 0 {
                            p.index = (p.index + 1) & 7;
                            p.quiet = false;
                        } else if p.period == 0x7FF && old_period != 0x7FF && p.quiet {
                            extra = 2;
                        }
                    }
                    // Already playing, the sound starts two ticks sooner.
                    p.delay = 4 - lf + extra;
                    p.countdown = (p.period ^ 0x7FF) * 2 + u16::from(p.delay);
                }
                p.volume = nrx2 >> 4;
            }
            if self.chip.active[ch] {
                self.pulse_amp(ch);
            }
            self.chip.sq[ch].env_count = nrx2 & 7;
            if nrx2 & 0xF8 != 0 && !self.chip.active[ch] {
                self.chip.active[ch] = true;
                self.set_amp(ch, 0);
                self.chip.sq[ch].quiet = !unquiet;
            }
            let p = &mut self.chip.sq[ch];
            if p.length == 0 {
                p.length = 0x40;
                p.length_on = false;
            }
            if ch == 0 {
                let nr10 = self.regs[NR10];
                let period = self.chip.sq[0].period;
                let double = self.double_speed;
                let cgb_hold = self.cgb || self.agb;
                let s = &mut self.chip.sweep;
                s.instant = false;
                s.shadow = 0;
                s.completed = 0;
                if nr10 & 7 != 0 {
                    s.calc = nr10 & 7;
                    s.reload = if (lf ^ u8::from(!double)) != 0 && !self.cgb && !self.agb { 3 } else { 2 };
                    s.unshifted = false;
                    if !was_active {
                        s.reload += 1;
                    }
                    s.addend = period >> (nr10 & 7);
                } else {
                    s.addend = 0;
                }
                s.hold = 2 - lf + if cgb_hold { 2 } else { 0 };
                s.countdown = ((nr10 >> 4) & 7) ^ 7;
            }
        }
        self.length_glitch_pulse(ch, v);
    }

    /// Switching length counting on while the sequencer's step is odd counts once at once.
    fn length_glitch_pulse(&mut self, ch: usize, v: u8) {
        let odd = self.chip.divider & 1 == 1;
        let p = &mut self.chip.sq[ch];
        if v & 0x40 != 0 && !p.length_on && odd && p.length != 0 {
            p.length -= 1;
            if p.length == 0 {
                if v & 0x80 != 0 {
                    p.length = 0x3F;
                } else {
                    self.chip.sq[ch].length_on = v & 0x40 != 0;
                    self.silence(ch);
                    return;
                }
            }
        }
        self.chip.sq[ch].length_on = v & 0x40 != 0;
    }

    fn wave_nr34(&mut self, v: u8) {
        let dmg = !self.cgb && !self.agb;
        let w = &mut self.chip.wave;
        w.period = (w.period & 0xFF) | (u16::from(v & 7) << 8);
        if v & 0x80 != 0 {
            w.pulsed = true;
            if dmg && self.chip.active[2] && w.countdown == 0 {
                // Retriggered the tick before it reads wave memory: the Game Boy corrupts it.
                let offset = usize::from(((w.index + 1) >> 1) & 0xF);
                if offset < 4 {
                    self.regs[WAVE] = self.regs[WAVE + offset];
                } else {
                    let base = WAVE + (offset & !3);
                    let copy: [u8; 4] = self.regs[base..base + 4].try_into().unwrap_or([0; 4]);
                    self.regs[WAVE..WAVE + 4].copy_from_slice(&copy);
                }
            }
            let w = &mut self.chip.wave;
            w.index = 0;
            if self.chip.active[2] && w.countdown == 0 {
                w.byte = self.regs[WAVE];
            }
            if w.dac {
                self.chip.active[2] = true;
                let w = &self.chip.wave;
                let v = if w.shift >= 4 { 0 } else { (w.byte >> 4) >> w.shift };
                self.set_amp(2, v);
            }
            let w = &mut self.chip.wave;
            // It starts reading three ticks late; the sample stays as it was until then.
            w.countdown = (w.period ^ 0x7FF) + 3;
            if w.length == 0 {
                w.length = 0x100;
                w.length_on = false;
            }
        }
        let odd = self.chip.divider & 1 == 1;
        let w = &mut self.chip.wave;
        if v & 0x40 != 0 && !w.length_on && odd && w.length != 0 {
            w.length -= 1;
            if w.length == 0 {
                if v & 0x80 != 0 {
                    w.length = 0xFF;
                } else {
                    w.length_on = true;
                    self.silence(2);
                    return;
                }
            }
        }
        self.chip.wave.length_on = v & 0x40 != 0;
    }

    fn noise_nr44(&mut self, v: u8) {
        let dmg = !self.cgb && !self.agb;
        if v & 0x80 != 0 {
            let n = &mut self.chip.noise;
            n.clock.locked = false;
            n.clock.high = false;
            if dmg && n.phase & 3 != 0 {
                n.dmg_delay = 6;
            } else {
                n.lfsr = 0;
                self.noise_start();
                let nr42 = self.regs[NR42];
                let n = &mut self.chip.noise;
                n.volume = nr42 >> 4;
                n.out = false;
                n.env_count = nr42 & 7;
                n.stepped_counter = n.phase & 3 == 2;
                if nr42 & 0xF8 != 0 {
                    self.chip.active[3] = true;
                    self.set_amp(3, 0);
                }
                let n = &mut self.chip.noise;
                if n.length == 0 {
                    n.length = 0x40;
                    n.length_on = false;
                }
            }
        }
        let odd = self.chip.divider & 1 == 1;
        let n = &mut self.chip.noise;
        if v & 0x40 != 0 && !n.length_on && odd && n.length != 0 {
            n.length -= 1;
            if n.length == 0 {
                if v & 0x80 != 0 {
                    n.length = 0x3F;
                } else {
                    n.length_on = true;
                    self.silence(3);
                    return;
                }
            }
        }
        self.chip.noise.length_on = v & 0x40 != 0;
    }

    // ------------------------------------------------------------ the clock and the mixer

    /// `t` T-cycles of sound (4 a CPU M-cycle, 2 at double speed): the chip runs a tick every 2.
    pub fn tick(&mut self, t: u32) {
        if self.power {
            let all = t + self.half;
            self.half = all & 1;
            let mut ticks = all / 2;
            while ticks > 0 {
                let n = ticks.min(0x4000);
                self.run(n as u16);
                ticks -= n;
            }
            if self.chip.pending_env {
                self.chip.pending_env = false;
                self.envelope_ticks();
            }
        }
        if self.hq {
            let (l, r) = self.mix();
            self.acc.0 += i64::from(l) * i64::from(t);
            self.acc.1 += i64::from(r) * i64::from(t);
            self.acc.2 += i64::from(t);
        }
        self.sample_acc += t * self.out_hz;
        while self.sample_acc >= CPU_HZ {
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

    /// What the four channels are putting out right now, 0-15 each: what the mixer adds up, and
    /// what a Color reads back from $FF76 and $FF77.
    fn channels(&self) -> [u8; 4] {
        if !self.power {
            return [0; 4];
        }
        let c = &self.chip;
        [0, 1, 2, 3].map(|i| if c.active[i] { c.amp[i] & 0xF } else { 0 })
    }

    /// $FF76 on a Color: channel 1 in the low four bits, channel 2 in the high four.
    pub fn pcm12(&self) -> u8 {
        let c = self.channels();
        c[0] | c[1] << 4
    }

    /// $FF77 on a Color: channels 3 and 4 the same way.
    pub fn pcm34(&self) -> u8 {
        let c = self.channels();
        c[2] | c[3] << 4
    }

    fn mix(&self) -> (i16, i16) {
        if !self.power {
            return (0, 0);
        }
        let ch = self.channels();
        let nr51 = self.regs[NR51];
        let nr50 = self.regs[NR50];
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A state saved before the chip was modelled tick by tick carries its channels over: the
    /// ones that were playing still play, at their volume, length and pitch.
    #[test]
    fn channels_saved_the_old_way_carry_over() {
        let mut a = Apu::default();
        a.regs[sq_reg(1, 2)] = 0xF0;
        a.regs[NR30] = 0x80;
        a.regs[NR42] = 0x80;
        a.sq2 = legacy::Square { on: true, duty: 2, step: 3, length: 20, length_on: true, freq: 0x6D6, env: legacy::Envelope { volume: 12, period: 0, timer: 0, up: false }, ..Default::default() };
        a.wave = legacy::Wave { on: true, length: 100, freq: 0x500, pos: 5, shift: 1, ..Default::default() };
        a.noise = legacy::Noise { on: true, length: 9, env: legacy::Envelope { volume: 7, ..Default::default() }, lfsr: 0x7FFE, ..Default::default() };
        a.sq1.on = false;
        a.adopt_legacy();
        assert_eq!(a.read(0xFF26) & 0x0F, 0b1110, "channels 2, 3 and 4 still play, 1 does not");
        let c = &a.chip;
        assert_eq!((c.sq[1].volume, c.sq[1].length, c.sq[1].period, c.sq[1].index), (12, 20, 0x6D6, 3));
        assert_eq!((c.wave.length, c.wave.period, c.wave.index), (100, 0x500, 5));
        assert_eq!((c.noise.volume, c.noise.length, c.noise.lfsr), (7, 9, 1), "the shift register is kept inverted now");
        // And it plays on.
        a.tick(4000);
        assert!(a.read(0xFF26) & 0x0E != 0);
    }
}
