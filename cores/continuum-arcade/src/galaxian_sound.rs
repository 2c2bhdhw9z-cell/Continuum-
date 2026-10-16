//! Galaxian's sound, and Moon Cresta's: not a sound chip but analogue circuitry on the main board,
//! switched by latches the game writes (`SoundLatches`). cArcade APPROXIMATES it: this is our own
//! simulation of how the documented circuit behaves, with the component values of the boards'
//! schematics (as MAME's model of the board, src/mame/galaxian/galaxian_a.cpp, lists them). It
//! is not a recording of a real board, and only the tone's counters are exact
//! (.kiro/notes/docs/ARCADE_CORE.md, "Galaxian's sound, approximated").
//!
//! The pieces, all stepped at 192 kHz (the 1.536 MHz sound clock / 8):
//! - **The tone.** The pitch latch loads two cascaded counters clocked at 1.536 MHz, so they
//!   overflow every 256 - pitch clocks (pitch FFh stops them). Each overflow steps a 74393 whose
//!   QA, QC and QD outputs are mixed: QA and QC always, QC a second time through VOL1's
//!   resistor, and QD only through VOL2's. Digital, so exact.
//! - **The background.** A four-bit resistor DAC (the LFO latches) sets the current with which a
//!   555 ramps a 1 µF capacitor: a slow sawtooth, one sweep every two to thirteen seconds. An
//!   op-amp turns the ramp into the control voltage of three 555 oscillators (FS1-FS3, each held
//!   in reset while its latch is 0), mixed through 10 kΩ each into a 0.1 µF capacitor.
//! - **The noise.** A 17-bit shift register (bits 4 and 16, one of them inverted, XORed back into
//!   bit 0) clocked at 12.288 MHz and sampled every two raster lines.
//! - **HIT.** The noise switches a capacitor that HIT charges (and that decays through 172 kΩ
//!   once HIT goes off) into an op-amp band-pass filter around 168 Hz: the explosion.
//! - **FIRE.** FIRE discharges a 47 µF capacitor which, with the noise, sets the control voltage
//!   of a fourth 555, so its pitch sweeps up; the 555 switches a capacitor that FIRE charges and
//!   that decays through 100 kΩ: the shot.
//! - **The mix.** Resistors into the amplifier, then a 0.1 µF coupling capacitor. Moon Cresta's
//!   board mixes everything in one network, with 15 kΩ for the background where Galaxian's has
//!   5.1 kΩ, and no load resistor.
//!
//! The output is in volts at the amplifier's input, handed out as volts × 32767 (MAME's scale:
//! a volt is full scale).

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::galaxian::SoundLatches;

/// Samples a second (the 1.536 MHz sound clock / 8).
pub const RATE: u32 = 192_000;
const DT: f64 = 1.0 / RATE as f64;
/// The sound clock's cycles in one sample.
const CLOCKS_PER_SAMPLE: u32 = 8;
/// A TTL output that is high, into these loads.
const TTL: f64 = 4.0;
/// The 555s' supply.
const VCC: f64 = 5.0;
/// The noise generator's clocks between two samples of it: two raster lines of 384 pixel clocks
/// at 6.144 MHz, at 12.288 MHz.
const NOISE_CLOCKS: u32 = 2 * 384 * 2;
/// 192 kHz samples between two samples of the noise (two lines of 192 CPU cycles, 16 each).
pub const NOISE_SAMPLES: u32 = 24;
const NOISE_PERIOD: u32 = (1 << 17) - 1;

const fn k(ohms: f64) -> f64 {
    ohms * 1000.0
}

/// How much of the way to its target an RC network gets in one sample.
fn rc(tau: f64) -> f64 {
    1.0 - (-DT / tau).exp()
}

/// Which board's mixing network.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mixer {
    Galaxian,
    MoonCresta,
}

/// A 555 timer running astable, with its control voltage driven from outside: the capacitor
/// charges through R1 + R2 towards the supply until it reaches the control voltage, then
/// discharges through R2 until it falls to half of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Astable {
    cap: f64,
    /// The output (and the charging phase).
    high: bool,
    /// The last sample's output, as the fraction of the sample it was high.
    out: f64,
}

impl Astable {
    /// One sample: returns the fraction of it the output was high. Held in reset, the
    /// capacitor is emptied and the output is low.
    fn step(&mut self, run: bool, r1: f64, r2: f64, c: f64, cv: f64) -> f64 {
        if !run {
            self.cap = 0.0;
            self.high = true;
            self.out = 0.0;
            return 0.0;
        }
        // Below a quarter of a volt the thresholds are too close for the circuit to work: it
        // stalls where it is.
        if cv < 0.25 {
            return self.out;
        }
        let (upper, lower) = (cv.min(VCC - 1e-6), cv / 2.0);
        if self.cap >= upper {
            self.high = false;
        } else if self.cap <= lower {
            self.high = true;
        }
        let mut left = DT;
        let mut high_time = 0.0;
        // At most a few hundred changes a sample even at the fastest settings; the bound only
        // guards against a circuit that can't oscillate.
        for _ in 0..1000 {
            if self.high {
                let tau = (r1 + r2) * c;
                let next = VCC + (self.cap - VCC) * (-left / tau).exp();
                if next < upper {
                    self.cap = next;
                    high_time += left;
                    break;
                }
                let to_upper = (tau * ((VCC - self.cap) / (VCC - upper)).ln()).min(left);
                self.cap = upper;
                high_time += to_upper;
                left -= to_upper;
                self.high = false;
            } else {
                let tau = r2 * c;
                let next = self.cap * (-left / tau).exp();
                if next > lower {
                    self.cap = next;
                    break;
                }
                let to_lower = tau * (self.cap / lower).ln();
                self.cap = lower;
                left -= to_lower.min(left);
                self.high = true;
            }
            if left <= 0.0 {
                break;
            }
        }
        self.out = high_time / DT;
        self.out
    }
}

/// A capacitor charged through a diode by `input` while `on` (instantly: the diode's side has
/// next to no resistance) and discharged through R while the input is lower; while not `on`
/// the output is grounded, though a higher input still charges the capacitor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
struct DiodeCap {
    v: f64,
}

impl DiodeCap {
    fn step(&mut self, on: bool, input: f64, decay: f64) -> f64 {
        let u = (input - 0.7).max(0.0);
        let diff = u - self.v;
        if on {
            self.v += if diff < 0.0 { diff * decay } else { diff };
            self.v
        } else {
            if diff > 0.0 {
                self.v = u;
            }
            0.0
        }
    }
}

/// A two-pole band-pass section (the bilinear transform of the analogue one), with its peak gain
/// at 1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
struct BandPass {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

#[derive(Clone, Copy, Debug)]
struct BandPassCoefficients {
    b0: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl BandPassCoefficients {
    fn new(f0: f64, q: f64) -> BandPassCoefficients {
        let w0 = 2.0 * std::f64::consts::PI * f0 / f64::from(RATE);
        let alpha = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        BandPassCoefficients {
            b0: alpha / a0,
            b2: -alpha / a0,
            a1: -2.0 * w0.cos() / a0,
            a2: (1.0 - alpha) / a0,
        }
    }
}

/// HIT's op-amp band-pass (multiple feedback): 150 kΩ in, 22 kΩ to the 2 V reference, 470 kΩ
/// feedback and two 0.01 µF capacitors. Its centre and Q follow from those; its gain at the
/// centre is -470k / 150k × C2 / (C1 + C2).
struct HitFilter {
    c: BandPassCoefficients,
    gain: f64,
}

const HIT_VREF: f64 = 5.0 * 22.0 / (33.0 + 22.0);
/// The op-amp's output can't get closer than 1.5 V to its 5 V supply.
const OPAMP_HIGH: f64 = 5.0 - 1.5;

fn hit_filter() -> HitFilter {
    let (r1, r2, rf, c1, c2) = (k(150.0), k(22.0), k(470.0), 0.01e-6, 0.01e-6);
    let rin = r1 * r2 / (r1 + r2);
    let f0 = 1.0 / (2.0 * std::f64::consts::PI * (rin * rf * c1 * c2).sqrt());
    let q = (rf / rin * c1 * c2).sqrt() / (c1 + c2);
    HitFilter {
        c: BandPassCoefficients::new(f0, q),
        gain: -(rf / r1) * c2 / (c1 + c2),
    }
}

/// The background's ramp: a 555 whose capacitor is charged by a constant current set by the
/// DAC (5 V less the DAC's voltage and a transistor's 0.7 V, over 100 kΩ), and emptied at once
/// when it reaches two thirds of the supply, back to one third.
fn dac_volts(lfo: u8) -> f64 {
    // 1 MΩ, 470 kΩ, 220 kΩ and 100 kΩ from the four latches (4 V when set), 15 kΩ from 4.4 V
    // and 330 kΩ to ground.
    let ladder = [k(1000.0), k(470.0), k(220.0), k(100.0)];
    let mut g = 1.0 / k(15.0) + 1.0 / k(330.0);
    let mut i = 4.4 / k(15.0);
    for (bit, r) in ladder.iter().enumerate() {
        g += 1.0 / r;
        if lfo >> bit & 1 != 0 {
            i += TTL / r;
        }
    }
    i / g
}

/// The noise generator's output for each of its states in turn, from power-on (all bits 0).
fn noise_table() -> &'static [u8] {
    static TABLE: OnceLock<Vec<u8>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut s: u32 = 0;
        (0..NOISE_PERIOD)
            .map(|_| {
                let f = (s >> 4 & 1) ^ (!s >> 16 & 1);
                s = ((s << 1) | f) & 0x1_FFFF;
                f as u8
            })
            .collect()
    })
}

/// The circuit's state: what a save state holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GalaxianSound {
    mixer: Mixer,
    latches: SoundLatches,
    // The tone's counters: the 8-bit pair loaded with the pitch, and the 74393.
    count1: u16,
    count2: u8,
    // The background.
    ramp: f64,
    fs: [Astable; 3],
    background: f64,
    // The noise: where the generator is, and samples until its next sample.
    noise_pos: u32,
    noise_wait: u32,
    noise: bool,
    // HIT.
    hit_cap: DiodeCap,
    hit_bp: BandPass,
    hit_out: f64,
    // FIRE.
    fire_rc: f64,
    fire_vco: Astable,
    fire_cap: DiodeCap,
    fire_coupling: f64,
    // The amplifier's coupling capacitor.
    out_coupling: f64,
}

/// The time constants and component values the steps use, worked out once.
struct Constants {
    ramp_bleed: f64,
    background: f64,
    hit_decay: f64,
    hit: HitFilter,
    fire_rc: f64,
    fire_decay: f64,
    fire_coupling: f64,
    out_coupling: f64,
}

fn constants(mixer: Mixer) -> &'static Constants {
    static GALAXIAN: OnceLock<Constants> = OnceLock::new();
    static MOONCRESTA: OnceLock<Constants> = OnceLock::new();
    let make = move || {
        // The fire input's coupling capacitor (0.01 µF) works into its 2.2 kΩ, in parallel with
        // Galaxian's 10 kΩ load.
        let fire_r = match mixer {
            Mixer::Galaxian => 1.0 / (1.0 / k(2.2) + 1.0 / k(10.0)),
            Mixer::MoonCresta => k(2.2),
        };
        Constants {
            ramp_bleed: rc(10e6 * 1e-6),
            background: rc(k(10.0) / 3.0 * 0.1e-6),
            hit_decay: rc((k(150.0) + k(22.0)) * 2.2e-6),
            hit: hit_filter(),
            fire_rc: rc(k(2.2) * 47e-6),
            fire_decay: rc(k(100.0) * 1e-6),
            fire_coupling: rc(fire_r * 0.01e-6),
            // The amplifier's input taken as 100 kΩ, behind 0.1 µF.
            out_coupling: rc(k(100.0) * 0.1e-6),
        }
    };
    match mixer {
        Mixer::Galaxian => GALAXIAN.get_or_init(make),
        Mixer::MoonCresta => MOONCRESTA.get_or_init(make),
    }
}

/// The background 555s' resistors: R1 100 kΩ and R2 470, 330 and 220 kΩ, each with 0.01 µF.
const FS_R2: [f64; 3] = [k(470.0), k(330.0), k(220.0)];

impl GalaxianSound {
    pub fn new(mixer: Mixer) -> GalaxianSound {
        noise_table();
        GalaxianSound {
            mixer,
            latches: SoundLatches {
                lfo: 0,
                sound: 0,
                pitch: 0,
            },
            count1: 0,
            count2: 0,
            ramp: 0.0,
            fs: [Astable::default(); 3],
            background: 0.0,
            noise_pos: 0,
            noise_wait: 0,
            noise: false,
            hit_cap: DiodeCap::default(),
            hit_bp: BandPass::default(),
            hit_out: HIT_VREF,
            fire_rc: 0.0,
            fire_vco: Astable::default(),
            fire_cap: DiodeCap::default(),
            fire_coupling: 0.0,
            out_coupling: 0.0,
        }
    }

    pub fn latches(&self) -> SoundLatches {
        self.latches
    }

    /// The latches as the game has just set them; they apply from the next sample on.
    pub fn set_latches(&mut self, latches: SoundLatches) {
        self.latches = latches;
    }

    /// The noise generator's output now.
    pub fn noise(&self) -> bool {
        self.noise
    }

    /// One 192 kHz sample, in volts at the amplifier.
    pub fn step(&mut self) -> f64 {
        let kc = constants(self.mixer);
        let l = self.latches;
        let bit = |n: u8| l.sound >> n & 1 != 0;

        // The tone: (256 - pitch) clocks a step of the 74393; pitch FFh holds it.
        if l.pitch != 0xFF {
            for _ in 0..CLOCKS_PER_SAMPLE {
                self.count1 += 1;
                if self.count1 > 0xFF {
                    self.count1 = u16::from(l.pitch);
                    self.count2 = (self.count2 + 1) & 0x0F;
                }
            }
        }
        let q = |b: u8| if self.count2 >> b & 1 != 0 { TTL } else { 0.0 };
        let (qa, qc, qd) = (q(0), q(2), q(3));

        // The background's ramp and the control voltage the op-amp makes of it: 10 kΩ feedback
        // over 47 kΩ, 47 kΩ and 10 kΩ, and 5 V through 47 kΩ the other way.
        let vin = dac_volts(l.lfo & 0x0F);
        let limit = vin + 0.7;
        let current = ((VCC - limit) / k(100.0)).max(0.0);
        if current > 0.0 {
            let mut v = (self.ramp + current * DT / 1e-6).min(limit);
            if v >= VCC * 2.0 / 3.0 {
                v = VCC / 3.0 + (v - VCC * 2.0 / 3.0);
            }
            self.ramp = v;
        } else {
            self.ramp -= self.ramp * kc.ramp_bleed;
        }
        let parallel = 1.0 / (2.0 / k(47.0) + 1.0 / k(10.0));
        let cv = (self.ramp * k(10.0) / parallel - VCC * 10.0 / 47.0).clamp(0.0, VCC);
        let mut fs_sum = 0.0;
        for (n, osc) in self.fs.iter_mut().enumerate() {
            fs_sum += 4.5 * osc.step(bit(n as u8), k(100.0), FS_R2[n], 0.01e-6, cv);
        }
        self.background += (fs_sum / 3.0 - self.background) * kc.background;

        // The noise, sampled every two lines.
        if self.noise_wait == 0 {
            self.noise_pos = (self.noise_pos + NOISE_CLOCKS) % NOISE_PERIOD;
            self.noise = noise_table()[self.noise_pos as usize] != 0;
            self.noise_wait = NOISE_SAMPLES;
        }
        self.noise_wait -= 1;
        let noise = if self.noise { 1.0 } else { 0.0 };

        // HIT: the switched capacitor into the band-pass, around the 2 V reference.
        let hit_in = self.hit_cap.step(self.noise, if bit(3) { TTL } else { 0.0 }, kc.hit_decay);
        let c = &kc.hit.c;
        let bp = &mut self.hit_bp;
        let y = c.b0 * hit_in + c.b2 * bp.x2 - c.a1 * bp.y1 - c.a2 * bp.y2;
        bp.x2 = bp.x1;
        bp.x1 = hit_in;
        let out = (HIT_VREF + kc.hit.gain * y).clamp(0.0, OPAMP_HIGH);
        // A saturated op-amp's output is what its feedback network sees.
        bp.y2 = bp.y1;
        bp.y1 = (out - HIT_VREF) / kc.hit.gain;
        self.hit_out = out;

        // FIRE: the 47 µF capacitor and the noise set the fourth 555's control voltage (2.2 kΩ
        // and 10 kΩ), which switches the shot's capacitor.
        let fire = bit(5);
        let fire_v = if fire { TTL } else { 0.0 };
        self.fire_rc += ((TTL - fire_v) - self.fire_rc) * kc.fire_rc;
        let (r46, r48) = (k(10.0), k(2.2));
        let fire_cv = (noise * TTL / r46 + self.fire_rc / r48) * (r46 * r48 / (r46 + r48));
        let vco = self.fire_vco.step(true, k(10.0), k(22.0), 0.01e-6, fire_cv) >= 0.5;
        let shot = self.fire_cap.step(vco, fire_v, kc.fire_decay);
        self.fire_coupling += (shot - self.fire_coupling) * kc.fire_coupling;
        let shot_ac = shot - self.fire_coupling;

        // The mix.
        let (vol1, vol2) = (bit(6), bit(7));
        let (r51, r50, r49, r52) = (k(33.0), k(22.0), k(10.0), k(15.0));
        let (r40, r43) = (k(2.2) * 0.6, k(2.2));
        let mut g = 1.0 / r51 + 1.0 / r50;
        let mut i = qa / r51 + qc / r50;
        if vol1 {
            g += 1.0 / r49;
            i += qc / r49;
        }
        if vol2 {
            g += 1.0 / r52;
            i += qd / r52;
        }
        let v = match self.mixer {
            Mixer::Galaxian => {
                // The tone and the background meet first (5.1 kΩ), then join HIT and FIRE
                // through another 5.1 kΩ, with 10 kΩ to ground.
                let r34 = k(5.1);
                let pre = (i + self.background / r34) / (g + 1.0 / r34);
                (pre / r34 + self.hit_out / r40 + shot_ac / r43)
                    / (1.0 / r34 + 1.0 / r40 + 1.0 / r43 + 1.0 / k(10.0))
            }
            Mixer::MoonCresta => {
                let r34 = k(15.0);
                (i + self.background / r34 + self.hit_out / r40 + shot_ac / r43)
                    / (g + 1.0 / r34 + 1.0 / r40 + 1.0 / r43)
            }
        };
        self.out_coupling += (v - self.out_coupling) * kc.out_coupling;
        v - self.out_coupling
    }
}

/// Volts at the amplifier to a sample (a volt is full scale).
pub fn to_sample(v: f64) -> i16 {
    (v * 32767.0).round().clamp(-32768.0, 32767.0) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn latches(sound: u8, pitch: u8, lfo: u8) -> SoundLatches {
        SoundLatches { lfo, sound, pitch }
    }

    fn run(s: &mut GalaxianSound, n: usize) -> Vec<f64> {
        (0..n).map(|_| s.step()).collect()
    }

    /// The noise generator, written again bit by bit from the schematic's description: three
    /// shift registers chained, 17 stages used, stage 4 XOR the inverse of stage 16 fed back.
    #[test]
    fn the_noise_generator_is_the_schematics_shift_register() {
        let mut stages = [false; 17];
        let t = noise_table();
        for (n, &want) in t.iter().take(5000).enumerate() {
            let f = stages[4] ^ !stages[16];
            stages.rotate_right(1);
            stages[0] = f;
            assert_eq!(u8::from(f), want, "clock {n}");
        }
        // It runs through every state but one before repeating: 131,071 clocks.
        let mut s: u32 = 0;
        let mut n = 0u32;
        loop {
            let f = (s >> 4 & 1) ^ (!s >> 16 & 1);
            s = ((s << 1) | f) & 0x1_FFFF;
            n += 1;
            if s == 0 {
                break;
            }
            assert!(n <= NOISE_PERIOD);
        }
        assert_eq!(n, NOISE_PERIOD);
        // Half its outputs are 1: one fewer 1 than 0, since with the inverted tap the state it
        // never reaches is all ones.
        let ones = t.iter().filter(|&&b| b == 1).count();
        assert_eq!(ones, 65_535);
    }

    #[test]
    fn the_sampled_noise_is_the_generator_every_1536_clocks() {
        let mut s = GalaxianSound::new(Mixer::Galaxian);
        let t = noise_table();
        let mut pos = 0usize;
        for sample in 0..(24 * 2000) {
            s.step();
            if sample % 24 == 0 {
                pos = (pos + 1536) % NOISE_PERIOD as usize;
            }
            assert_eq!(s.noise(), t[pos] != 0, "sample {sample}");
        }
        // Sampled that way it is still balanced and changes often.
        let mut s = GalaxianSound::new(Mixer::Galaxian);
        let bits: Vec<bool> = (0..24 * 4096).filter_map(|i| {
            s.step();
            (i % 24 == 0).then(|| s.noise())
        }).collect();
        let ones = bits.iter().filter(|&&b| b).count();
        assert!((1900..2200).contains(&ones), "{ones} of 4096");
        let changes = bits.windows(2).filter(|w| w[0] != w[1]).count();
        assert!((1900..2200).contains(&changes), "{changes} changes");
    }

    /// The tone's frequency: the 74393 steps every 256 - pitch clocks of 1.536 MHz, and QC (the
    /// loudest part without VOL2) is an eighth of that.
    #[test]
    fn the_tone_counts_256_less_the_pitch() {
        for pitch in [0x00u8, 0x40, 0x80, 0xC0, 0xF0, 0xFE] {
            let mut s = GalaxianSound::new(Mixer::Galaxian);
            s.set_latches(latches(0, pitch, 0));
            // From power-on the counters first count all the way up from 0.
            run(&mut s, 100);
            let mut steps = 0u32;
            let mut last = s.count2;
            for _ in 0..19_200 {
                s.step();
                // At most 4 steps a sample (pitch FEh), so the 4-bit count never laps.
                steps += u32::from(s.count2.wrapping_sub(last) & 0x0F);
                last = s.count2;
            }
            // 0.1 s of 1.536 MHz clocks.
            let want = 153_600 / (256 - u32::from(pitch));
            assert!(steps.abs_diff(want) <= 1, "pitch {pitch:02X}: {steps} steps, want {want}");
        }
        // Pitch FFh stops the counters where they are.
        let mut s = GalaxianSound::new(Mixer::Galaxian);
        s.set_latches(latches(0, 0x80, 0));
        run(&mut s, 1000);
        s.set_latches(latches(0, 0xFF, 0));
        let held = s.count2;
        run(&mut s, 5000);
        assert_eq!(s.count2, held);
    }

    #[test]
    fn the_ramp_is_slower_the_higher_the_lfo() {
        let period = |lfo: u8| {
            let mut s = GalaxianSound::new(Mixer::Galaxian);
            s.set_latches(latches(0, 0xFF, lfo));
            let mut wraps = Vec::new();
            let mut last = 0.0;
            for n in 0..(192_000 * 30) {
                s.step();
                if s.ramp < last - 1.0 {
                    wraps.push(n);
                }
                last = s.ramp;
                if wraps.len() == 2 {
                    break;
                }
            }
            (wraps[1] - wraps[0]) as f64 / 192_000.0
        };
        let fast = period(0);
        let slow = period(15);
        // 1 µF over a third of 5 V at (5 - DAC - 0.7) / 100 kΩ.
        let want = |lfo: u8| 1e-6 * (5.0 / 3.0) / ((5.0 - dac_volts(lfo) - 0.7) / 100e3);
        assert!((fast - want(0)).abs() / want(0) < 0.01, "{fast} s");
        assert!((slow - want(15)).abs() / want(15) < 0.01, "{slow} s");
        assert!(slow > 5.0 * fast);
    }

    fn rms(v: &[f64]) -> f64 {
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        (v.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / v.len() as f64).sqrt()
    }

    /// Every latch bit changes what comes out.
    #[test]
    fn every_latch_bit_is_heard() {
        for mixer in [Mixer::Galaxian, Mixer::MoonCresta] {
            let hear = |l: SoundLatches| {
                let mut s = GalaxianSound::new(mixer);
                s.set_latches(l);
                run(&mut s, 19_200);
                run(&mut s, 38_400)
            };
            let quiet = rms(&hear(latches(0, 0xFF, 0)));
            assert!(quiet < 1e-3, "{mixer:?}: silence is {quiet}");
            // FS1-FS3, HIT and FIRE each make a sound of their own.
            for b in [0u8, 1, 2, 3, 5] {
                let r = rms(&hear(latches(1 << b, 0xFF, 0)));
                assert!(r > 0.005, "{mixer:?}: bit {b} gives {r}");
            }
            // VOL1 and VOL2 make the tone louder; the pitch changes it.
            let tone = |sound: u8, pitch: u8| hear(latches(sound, pitch, 0));
            let plain = rms(&tone(0, 0x80));
            assert!(plain > 0.01, "{mixer:?}: the tone");
            assert!(rms(&tone(0x40, 0x80)) > plain * 1.1, "{mixer:?}: VOL1");
            assert!(rms(&tone(0x80, 0x80)) > plain * 1.1, "{mixer:?}: VOL2");
            assert!(tone(0, 0x80) != tone(0, 0x81), "{mixer:?}: pitch");
            // Each LFO bit moves the background's pitch.
            let base = hear(latches(0x07, 0xFF, 0));
            for b in 0..4 {
                assert!(hear(latches(0x07, 0xFF, 1 << b)) != base, "{mixer:?}: LFO bit {b}");
            }
        }
    }

    #[test]
    fn the_shot_and_the_explosion_die_away() {
        let mut s = GalaxianSound::new(Mixer::Galaxian);
        run(&mut s, 19_200);
        s.set_latches(latches(1 << 3, 0xFF, 0));
        let on = rms(&run(&mut s, 19_200));
        s.set_latches(latches(0, 0xFF, 0));
        // The capacitor lets go over 172 kΩ × 2.2 µF, 0.38 s.
        run(&mut s, 192_000 * 3);
        let after = rms(&run(&mut s, 19_200));
        assert!(on > 0.05 && after < on / 20.0, "HIT {on} then {after}");
        let mut s = GalaxianSound::new(Mixer::Galaxian);
        run(&mut s, 19_200);
        s.set_latches(latches(1 << 5, 0xFF, 0));
        let on = rms(&run(&mut s, 9_600));
        s.set_latches(latches(0, 0xFF, 0));
        run(&mut s, 192_000);
        let after = rms(&run(&mut s, 19_200));
        assert!(on > 0.005 && after < on / 20.0, "FIRE {on} then {after}");
    }

    #[test]
    fn a_state_round_trips() {
        let mut s = GalaxianSound::new(Mixer::MoonCresta);
        s.set_latches(latches(0xEF, 0x90, 0x05));
        run(&mut s, 10_000);
        let saved = bincode::serialize(&s).unwrap();
        let a = run(&mut s, 5_000);
        let mut t: GalaxianSound = bincode::deserialize(&saved).unwrap();
        assert_eq!(run(&mut t, 5_000), a);
    }
}
