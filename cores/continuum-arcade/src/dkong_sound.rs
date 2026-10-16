//! Donkey Kong's and Donkey Kong Jr.'s sound after the 8035: the DAC with its decay circuit and
//! low-pass filter, the analogue sounds the main CPU starts through latch 6H (and, on Jr.'s board,
//! 5H), the mixer and the amplifier.
//!
//! **This is an approximation.** The boards' effects are analogue circuits: 555 timers and
//! CD4049 inverter oscillators (Donkey Kong), 74LS624 oscillators, 74LS123 one-shots, counters
//! and gates (Jr.), shift-register noise, RC envelopes, transistor stages and filters. cArcade
//! steps a model of each circuit, built from the component values on the schematics as MAME's
//! discrete-sound model of the boards lists them (dkong_a.cpp), in its own code, one step per
//! 48 kHz sample, the way MAME's model is stepped. It isn't a recording of a real board; the DAC's
//! path (a current-to-voltage stage, the decay circuit, a Sallen-Key low-pass and the mixer) is
//! the part that is closest to exact.
//!
//! Donkey Kong (MAME's dkong2b): latch 6H bit 0 walks, bit 1 jumps, bit 2 stomps.
//! - Walk: a CD4049 oscillator (about 10 Hz) and the latch set a 555's control voltage, so it
//!   chirps; the latch's edges charge an RC envelope the 555 switches.
//! - Jump: the same with a slower CD4049 oscillator; the latch starts an envelope that opens a
//!   diode gate onto the 555's output.
//! - Stomp: a 24-bit shift register clocked at 4 kHz and divided by eight, through the same kind
//!   of envelope and gate.
//! - The four are mixed through 47 kΩ each into 10 kΩ and 100 nF, and a transistor amplifier with
//!   its coupling capacitors drives the speaker.
//!
//! Donkey Kong Jr. (MAME's dkongjr): latch 6H bit 0 walks, bit 1 jumps, bit 2 climbs, bit 7
//! changes the walk; latch 5H bit 1 is the fall.
//! - Walk: a 74LS624 oscillator clocks a 14-bit counter whose top bits slow it down; a 74LS123
//!   one-shot on the latch gates the counter's bits (or, with bit 7, a second oscillator mixed
//!   with them) to the output.
//! - Jump: a one-shot sweeps a third oscillator, mixed with the walk counter's bit 11, through an
//!   envelope and two filters. Climb: 16-bit shift-register noise at 710 Hz, likewise.
//! - Fall: a fourth oscillator, gated by the latch.
//! - The five are mixed through 150, 100, 20, 47 and 47 kΩ, then a coupling capacitor.
//!
//! The oscillators' frequencies follow MAME's fit of the 74LS624 to measured chips, and MAME's
//! approximations of the 555 and the CD4049 (their outputs averaged over each sample).

use serde::{Deserialize, Serialize};

/// The sound handed out.
pub const RATE: u32 = 48_000;
const DT: f64 = 1.0 / RATE as f64;

/// What reaches the sound circuits, and when (attoseconds since power-on).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Input {
    /// The 8035's port 1, into the DAC.
    Dac(u8),
    /// The 8035's port 2 bit 7 (high: the DAC's decay circuit is off).
    Decay(bool),
    /// Latch 6H, whose bits start the analogue effects.
    Latch6h(u8),
    /// Donkey Kong Jr.'s latch 5H bit 1 (the fall).
    Sound9(bool),
    /// The board's reset: every circuit starts again, the DAC at 0 (MAME resets its sound model
    /// after the 8035, so the 8035's reset write to the DAC is lost) and the decay circuit off.
    Reset,
}

/// How much of a capacitor's distance to its target is left after one sample, charging
/// through `rc` seconds.
fn keep(rc: f64) -> f64 {
    (-DT / rc).exp()
}

fn parallel(a: f64, b: f64) -> f64 {
    a * b / (a + b)
}

// ---- The circuits' parts ------------------------------------------------------------------------

/// A shift register (74LS164s) fed back through an XOR gate, clocked at a fixed rate: noise.
/// `bits` long, taps `tap0` and `tap1`; the XOR's output is what comes out, inverted on Jr.'s.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Noise {
    reg: u32,
    bits: u32,
    tap0: u32,
    tap1: u32,
    invert: bool,
    /// The clock's period, and how far into it the circuit is.
    period: f64,
    left: f64,
    out: bool,
}

impl Noise {
    fn new(bits: u32, tap0: u32, tap1: u32, invert: bool, hz: f64) -> Noise {
        Noise { reg: 0, bits, tap0, tap1, invert, period: 1.0 / hz, left: 0.0, out: invert }
    }

    /// One sample: the register shifts as many times as its clock ticks in it.
    fn step(&mut self) -> f64 {
        let ticks = (self.left + DT) / self.period;
        let n = ticks as u32;
        self.left = (ticks - f64::from(n)) * self.period;
        for _ in 0..n {
            // The XOR's last result, inverted, goes in at the bottom.
            let fb = (self.reg >> self.bits & 1) ^ 1;
            let shifted = ((self.reg << 1) | fb) & ((1 << self.bits) - 1);
            let x = (shifted >> self.tap0 ^ shifted >> self.tap1) & 1;
            self.reg = shifted | x << self.bits;
            self.out = (x != 0) ^ self.invert;
        }
        if self.out { 1.0 } else { 0.0 }
    }
}

/// The RC envelope a latch bit drives through a transistor (MAME's "RC discharge, modulated"):
/// with the bit's inverted line `in1` low, the capacitor charges towards the supply through
/// R1 + R2 (and R4), and the output is the voltage across the resistors; with it high, it
/// discharges, through the clamping diode first. `in2` (above 0.6 V) shorts R4 through R3, which
/// is how an oscillator chops the envelope.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Envelope {
    v: f64,
    vp: f64,
    low: [f64; 2],
    high: [f64; 4],
    vd_gain: [f64; 4],
    gain: [f64; 2],
}

impl Envelope {
    fn new(r1: f64, r2: f64, r3: f64, r4: f64, c: f64, vp: f64) -> Envelope {
        let rc = [(r1 + r2).max(1.0), r2.max(1.0)];
        let rc2 = [r4, if r3 == 0.0 { 0.0 } else { parallel(r3, r4) }];
        let mut e = Envelope { v: 0.0, vp, low: [0.0; 2], high: [0.0; 4], vd_gain: [0.0; 4], gain: [0.0; 2] };
        for (m, &r) in rc.iter().enumerate() {
            e.low[m] = keep(c * r);
            e.gain[m] = r4 / (r + r4);
        }
        for s in 0..4 {
            let (r, r2) = (rc[s & 1], rc2[s >> 1]);
            e.high[s] = keep(c * (r + r2));
            e.vd_gain[s] = r2 / (r + r2);
        }
        e
    }

    fn step(&mut self, in1: f64, in2: f64) -> f64 {
        let m1 = usize::from(in1 > 0.5);
        let m2 = in2 > 0.6;
        let s = usize::from(m2) << 1 | m1;
        let u = if m1 == 1 { 0.0 } else { self.vp };
        let diff = u - self.v;
        if diff * self.vd_gain[s] < -0.6 {
            // The diode clamps.
            let d = u + 0.6 - self.v;
            self.v += d - d * self.low[m1];
            if m2 { 0.0 } else { -0.6 }
        } else {
            self.v += diff - diff * self.high[s];
            if m2 { 0.0 } else { (u - self.v) * self.gain[m1] }
        }
    }
}

/// A capacitor that charges towards one voltage or another through one resistor or another, as
/// a logic level picks.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct TwoWay {
    v: f64,
    target: [f64; 2],
    keep: [f64; 2],
}

impl TwoWay {
    fn new(v0: f64, r0: f64, v1: f64, r1: f64, c: f64) -> TwoWay {
        TwoWay { v: 0.0, target: [v0, v1], keep: [keep(r0 * c), keep(r1 * c)] }
    }

    fn step(&mut self, sel: bool) -> f64 {
        let s = usize::from(sel);
        let d = self.target[s] - self.v;
        self.v += d - d * self.keep[s];
        self.v
    }
}

/// A transistor behind two diodes charging and discharging a capacitor (MAME's "RC integrate"),
/// with the transistor's current from the Ebers-Moll equation: `cap` gives the capacitor's
/// voltage, otherwise the voltage at R3 (the collector) is the output.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Integrator {
    r1: f64,
    r2: f64,
    r3: f64,
    vp: f64,
    cap: bool,
    v_cap: f64,
    v_ce: f64,
    gain12: f64,
    f: f64,
    exp: [f64; 2],
    c_exp: [f64; 2],
    ic07: f64,
}

/// The Ebers-Moll collector current for a base-emitter voltage.
fn collector_current(vbe: f64) -> f64 {
    0.99 * 7e-15 * (vbe / 0.026 - 1.0).exp()
}

impl Integrator {
    fn new(r1: f64, r2: f64, r3: f64, c: f64, vp: f64, cap: bool) -> Integrator {
        let r = r1 / r2 * r3 + r1 + r3;
        let f = r2 / (r3 + r2);
        let t0 = -r * f * c;
        let t1 = -(r1 + r2) * c;
        let e0 = (DT / t0).exp();
        let e1 = (DT / t1).exp();
        Integrator {
            r1,
            r2,
            r3,
            vp,
            cap,
            v_cap: 0.0,
            v_ce: 0.0,
            gain12: r2 / (r1 + r2),
            f,
            exp: [e0, e1],
            c_exp: [c / t0 * e0, c / t1 * e1],
            ic07: collector_current(0.7),
        }
    }

    fn step(&mut self, u: f64) -> f64 {
        let vp = self.vp;
        let (iq, rg, mut ve);
        if u - 0.7 < self.v_cap * self.gain12 {
            // The transistor is off: the capacitor discharges through R1 and R2.
            let d = -self.v_cap;
            let ic = self.c_exp[1] * d;
            self.v_cap += d - d * self.exp[1];
            iq = 0.0;
            ve = self.v_cap * self.gain12;
            rg = ve / ic;
        } else {
            // On: it charges through the transistor.
            let d = (vp - self.v_ce) * self.f - self.v_cap;
            let ic = -self.c_exp[0] * d;
            self.v_cap += d - d * self.exp[0];
            iq = ic + (ic * self.r1 + self.v_cap) / self.r2;
            rg = (vp - self.v_ce) / iq;
            ve = (rg - self.r3) / rg * (vp - self.v_ce);
        }
        let iqc = if u > 0.7 + ve {
            ve = u - 0.7;
            self.ic07
        } else {
            collector_current(u - ve)
        };
        // Settle the transistor's voltage without iterating (weighted, so it doesn't swing).
        let vce = (vp - 0.1).min(vp - rg * iqc).max(0.1);
        self.v_ce = 0.1 * vce + 0.9 * (vp - ve - iq * self.r3);
        if self.cap {
            self.v_cap
        } else {
            (vp - iq * self.r3).max(0.0)
        }
    }
}

/// Three (or two) CD4049 inverters with an RC network round them: a slow oscillator. The
/// inverters' transfer curve is fitted through the datasheet's points (0.1 V out at 3.5 V in,
/// 4.9 V out at 1.5 V in, on 5 V) and looked up in a 500-point table.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct InverterOsc {
    /// Two inverters in the loop (Donkey Kong's walk) rather than three.
    two: bool,
    v_cap: f64,
    v_g2: f64,
    w: f64,
    wc: f64,
    ratio: f64,
    #[serde(skip, default = "inverter_table")]
    table: Vec<f64>,
}

const VB: f64 = 5.0;
const INV_TAB: usize = 500;

fn inverter_table() -> Vec<f64> {
    let (v_low, v_high, v_fall, v_rise) = (VB * 0.02, VB * 0.98, VB / 5.0 * 1.5, VB / 5.0 * 3.5);
    let b = ((-(v_low / VB).ln()).ln() - (-(v_high / VB).ln()).ln()) / (v_rise / v_fall).ln();
    let a = ((-(v_low / VB).ln()).ln() - b * (v_rise / VB).ln()).exp();
    (0..INV_TAB)
        .map(|i| {
            let x = i as f64 / (INV_TAB - 1) as f64;
            if x > 0.0 { VB * (-a * x.powf(b)).exp() } else { VB }
        })
        .collect()
}

impl InverterOsc {
    fn new(r: f64, rp: f64, c: f64, two: bool) -> InverterOsc {
        InverterOsc {
            two,
            v_cap: 0.0,
            v_g2: 0.0,
            w: keep(r * c),
            wc: keep(r * rp / (rp + r) * c),
            ratio: rp / (rp + r),
            table: inverter_table(),
        }
    }

    fn gate(&self, x: f64) -> f64 {
        if x < 0.0 {
            VB
        } else if x <= VB {
            self.table[((INV_TAB - 1) as f64 * x / VB) as usize]
        } else {
            self.table[INV_TAB - 1]
        }
    }

    fn step(&mut self) -> f64 {
        let mut vi = self.v_cap + self.v_g2;
        let (g2, g3) = if self.two {
            let g3 = self.gate(vi);
            (self.gate(g3), g3)
        } else {
            let g1 = self.gate(vi);
            let g2 = self.gate(g1);
            (g2, self.gate(g2))
        };
        // The input's protection diodes clamp it 0.1 V beyond the rails.
        let clamped = if vi < -0.1 {
            vi = -0.1;
            true
        } else if vi > VB + 0.1 {
            vi = VB + 0.1;
            true
        } else {
            false
        };
        let d = if clamped {
            let d = g3 * self.ratio - (self.v_cap + g2) + vi * (1.0 - self.ratio);
            d - d * self.wc
        } else {
            let d = g3 - (self.v_cap + g2);
            d - d * self.w
        };
        self.v_cap += d;
        self.v_g2 = g2;
        g3
    }
}

/// Donkey Kong's 555 control-voltage network: an open-collector inverter on the latch's line
/// through R1 + R2, the CD4049 oscillator through R3, all through R4 onto a capacitor and the
/// 555's own 5 kΩ / 10 kΩ divider.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ControlVoltage {
    v: f64,
    vp: f64,
    r3: f64,
    i_in1: [f64; 2],
    r_total: [f64; 2],
    exp: [f64; 2],
}

impl ControlVoltage {
    fn new(r1: f64, r2: f64, r3: f64, r4: f64, c: f64, vp: f64) -> ControlVoltage {
        let cv = parallel(5_000.0, 10_000.0);
        let r_in = [parallel(r1 + r2, r3), parallel(r2, r3)];
        let r_total = [parallel(r_in[0] + r4, cv), parallel(r_in[1] + r4, cv)];
        ControlVoltage {
            v: 0.0,
            vp,
            r3,
            i_in1: [vp / (r1 + r2), 0.0],
            r_total,
            exp: [1.0 - keep(r_total[0] * c), 1.0 - keep(r_total[1] * c)],
        }
    }

    fn step(&mut self, in1: f64, in2: f64) -> f64 {
        let s = usize::from(in1 as i32 != 0);
        let i = self.vp / 5_000.0 + self.i_in1[s] + in2 / self.r3;
        self.v += (i * self.r_total[s] - self.v) * self.exp[s];
        self.v
    }
}

/// A 555 running astable, its thresholds set by a control voltage; the output is how much of
/// each sample it spends high (4.5 V high), as its RC network times it.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Astable {
    v_cap: f64,
    high: bool,
    out: f64,
    t_charge: f64,
    t_discharge: f64,
    exp_charge: f64,
    exp_discharge: f64,
}

impl Astable {
    fn new(r1: f64, r2: f64, c: f64) -> Astable {
        let (tc, td) = ((r1 + r2) * c, r2 * c);
        Astable { v_cap: 0.0, high: true, out: 0.0, t_charge: tc, t_discharge: td, exp_charge: 1.0 - keep(tc), exp_discharge: 1.0 - keep(td) }
    }

    fn step(&mut self, cv: f64) -> f64 {
        const V_CHARGE: f64 = 5.0;
        const V_HIGH: f64 = 4.5;
        if cv < 0.25 {
            // Too low a control voltage would run it far out of range: MAME holds it as it was.
            return self.out;
        }
        let (threshold, trigger) = (cv, cv / 2.0);
        let mut v = self.v_cap;
        let mut high = self.high;
        if v >= threshold {
            high = false;
        } else if v <= trigger {
            high = true;
        }
        let mut dt = DT;
        let mut since = 0.0;
        let mut first = true;
        while dt > 0.0 {
            let full = first;
            first = false;
            if high {
                let e = if full { self.exp_charge } else { 1.0 - (-dt / self.t_charge).exp() };
                let next = v + (V_CHARGE - v) * e;
                dt = 0.0;
                if next >= threshold {
                    dt = self.t_charge * (1.0 / (1.0 - (next - threshold) / (V_CHARGE - v))).ln();
                    since = dt;
                    v = threshold;
                    high = false;
                } else {
                    v = next;
                }
            } else {
                let e = if full { self.exp_discharge } else { 1.0 - (-dt / self.t_discharge).exp() };
                let next = v - v * e;
                dt = 0.0;
                if next <= trigger {
                    if next < trigger {
                        dt = self.t_discharge * (1.0 / (1.0 - (trigger - next) / v)).ln();
                    }
                    since = dt;
                    v = trigger;
                    high = true;
                } else {
                    v = next;
                }
            }
        }
        self.v_cap = v;
        self.high = high;
        let x = if since == 0.0 { 1.0 } else { since / DT };
        self.out = V_HIGH * if high { x } else { 1.0 - x };
        self.out
    }
}

/// The DAC's decay circuit (Q7, R20, C32): while port 2 bit 7 is low the DAC's reference
/// current decays from where it was, from the moment the bit fell. As MAME's model steps it,
/// the sample it switches on in still reads 0 and the one it switches off in keeps its last
/// value.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Decay {
    on: bool,
    t: f64,
    out: f64,
}

impl Decay {
    fn step(&mut self, enabled: bool, rc: f64) -> f64 {
        if enabled {
            if self.on {
                self.out = (-self.t / rc).exp();
                self.t += DT;
            } else {
                self.on = true;
                self.t = 0.0;
                self.out = 0.0;
            }
        } else if self.on {
            self.on = false;
        } else {
            self.out = 0.0;
        }
        self.out
    }
}

/// The DAC's Sallen-Key low-pass (5.6 kΩ, 5.6 kΩ, 22 nF, 10 nF: 1,916 Hz, Q 0.74), as a
/// bilinear-transformed biquad, pre-warped at its corner.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct LowPass {
    a1: f64,
    a2: f64,
    b0: f64,
    x: [f64; 2],
    y: [f64; 2],
}

impl LowPass {
    fn sallen_key(r1: f64, r2: f64, c1: f64, c2: f64) -> LowPass {
        let f = 1.0 / (2.0 * std::f64::consts::PI * (c1 * c2 * r1 * r2).sqrt());
        let q = (c1 * c2 * r1 * r2).sqrt() / (c2 * (r1 + r2));
        let d = 1.0 / q;
        let two_t = 2.0 * f64::from(RATE);
        let wc = two_t * (std::f64::consts::PI * f / f64::from(RATE)).tan();
        let den = two_t * two_t + d * wc * two_t + wc * wc;
        LowPass {
            a1: 2.0 * (wc * wc - two_t * two_t) / den,
            a2: (two_t * two_t - d * wc * two_t + wc * wc) / den,
            b0: wc * wc / den,
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }

    fn step(&mut self, x: f64) -> f64 {
        let y = -self.a1 * self.y[0] - self.a2 * self.y[1] + self.b0 * (x + 2.0 * self.x[0] + self.x[1]);
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// A resistor mixer into a node: Millman's sum (with `rf` to ground, if any), the node's
/// capacitor to ground, then a coupling capacitor into the next stage (taken as 100 kΩ).
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Mixer {
    r: Vec<f64>,
    r_total: f64,
    cf: f64,
    camp: f64,
    v_cf: f64,
    v_amp: f64,
}

impl Mixer {
    fn new(r: &[f64], rf: f64, cf: f64, camp: f64) -> Mixer {
        let mut g: f64 = r.iter().map(|x| 1.0 / x).sum();
        if rf > 0.0 {
            g += 1.0 / rf;
        }
        let r_total = 1.0 / g;
        Mixer {
            r: r.to_vec(),
            r_total,
            cf: if cf > 0.0 { 1.0 - keep(r_total * cf) } else { 0.0 },
            camp: if camp > 0.0 { 1.0 - keep(100_000.0 * camp) } else { 0.0 },
            v_cf: 0.0,
            v_amp: 0.0,
        }
    }

    fn step(&mut self, v: &[f64]) -> f64 {
        let i: f64 = v.iter().zip(&self.r).map(|(v, r)| v / r).sum();
        let mut out = i * self.r_total;
        if self.cf > 0.0 {
            self.v_cf += (out - self.v_cf) * self.cf;
            out = self.v_cf;
        }
        if self.camp > 0.0 {
            self.v_amp += (out - self.v_amp) * self.camp;
            out -= self.v_amp;
        }
        out
    }
}

/// A series capacitor into a resistor to `vref`: a high-pass.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct HighPass {
    v_cap: f64,
    e: f64,
    vref: f64,
}

impl HighPass {
    fn new(r: f64, c: f64, vref: f64) -> HighPass {
        HighPass { v_cap: 0.0, e: 1.0 - keep(r * c), vref }
    }

    fn step(&mut self, x: f64) -> f64 {
        let out = x - self.v_cap;
        self.v_cap += (out - self.vref) * self.e;
        out
    }
}

/// A series resistor into a capacitor: a low-pass.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Smooth {
    v: f64,
    e: f64,
}

impl Smooth {
    fn new(r: f64, c: f64) -> Smooth {
        Smooth { v: 0.0, e: 1.0 - keep(r * c) }
    }

    fn step(&mut self, x: f64) -> f64 {
        self.v += (x - self.v) * self.e;
        self.v
    }
}

/// A 74LS123 one-shot, retriggered by each rising edge: high (4 V) for 0.25 x R x C (with the
/// chip's 700 Ω) after the last edge.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct OneShot {
    width: f64,
    left: f64,
    on: bool,
    last: bool,
}

impl OneShot {
    fn new(r: f64, c: f64) -> OneShot {
        OneShot { width: 0.25 * r * c * (1.0 + 700.0 / r), left: 0.0, on: false, last: false }
    }

    fn step(&mut self, trigger: bool) -> bool {
        let mut count = self.on;
        if trigger != self.last {
            self.last = trigger;
            if trigger {
                if !self.on {
                    self.on = true;
                }
                self.left = self.width;
                count = false;
            }
        }
        if count {
            self.left -= DT;
            if self.left <= 0.0 {
                self.left = 0.0;
                self.on = false;
            }
        }
        self.on
    }
}

/// A 74LS624 voltage-controlled oscillator. Its frequency follows the fit MAME makes of the
/// chip's measured response (control and range voltages, for 0.1 µF, scaled by the timing
/// capacitor); the control pin's 90 kΩ divides what reaches it through the input resistor, and
/// an input capacitor slows it.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Vco {
    c: f64,
    freq_scale: f64,
    rng: f64,
    in_cap: Option<f64>,
    v_in: f64,
    /// Half periods: where in the current one it is (a fraction), and its output.
    phase: f64,
    high: bool,
}

impl Vco {
    fn new(c: f64, r_freq: f64, c_freq: f64, r_rng: f64, v_rng: f64) -> Vco {
        const R_IN: f64 = 90_000.0;
        Vco {
            c,
            freq_scale: R_IN / (r_freq + R_IN),
            rng: v_rng * R_IN / (r_rng + R_IN),
            in_cap: (c_freq > 0.0).then(|| 1.0 - keep(parallel(r_freq, R_IN) * c_freq)),
            v_in: 0.0,
            phase: 0.0,
            high: false,
        }
    }

    #[allow(clippy::excessive_precision)]
    fn freq(&mut self, v_mod: f64) -> f64 {
        let mut v = v_mod * self.freq_scale;
        if let Some(e) = self.in_cap {
            self.v_in += (v - self.v_in) * e;
            v = self.v_in;
        }
        let r = self.rng;
        let (v2, v3, v4) = (v * v, v * v * v, v * v * v * v);
        let f = 1.990_476_902_479_628_3e3 + 1.207_005_921_398_340_7e3 * v + 1.326_698_557_956_110_8e3 * v2
            - 1.550_097_982_592_269_8e2 * v3
            + 2.818_453_626_693_817_2 * v4
            - 2.350_342_158_274_455_6e2 * r
            - 3.383_678_670_452_778_8e2 * r * v
            - 1.356_913_670_325_867e2 * r * v2
            + 2.991_457_545_381_918_8 * r * v3
            + 1.685_556_908_617_317 * r * v4;
        f * 0.1e-6 / self.c
    }

    /// Runs `dt` seconds at `freq`: how many falling edges, and the fraction of `dt` spent high.
    fn run(&mut self, freq: f64, dt: f64) -> (u32, f64) {
        let mut left = dt * freq * 2.0;
        let (mut falls, mut high_time) = (0u32, 0.0);
        while left > 0.0 {
            let to_edge = 1.0 - self.phase;
            let step = to_edge.min(left);
            if self.high {
                high_time += step;
            }
            self.phase += step;
            left -= step;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
                self.high = !self.high;
                if !self.high {
                    falls += 1;
                }
            }
        }
        (falls, high_time / (dt * freq * 2.0).max(f64::MIN_POSITIVE))
    }
}

// ---- The boards' circuits -----------------------------------------------------------------------

/// Donkey Kong's (MAME's dkong2b): the three effects, the DAC, the mixer and the amplifier.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct DkCircuit {
    noise: Noise,
    count: u32,
    last_noise: bool,
    stomp_env: Envelope,
    stomp_gate: TwoWay,
    stomp_out: Integrator,
    jump_osc: InverterOsc,
    jump_cv: ControlVoltage,
    jump_555: Astable,
    jump_env: Envelope,
    jump_gate: TwoWay,
    jump_out: Integrator,
    walk_osc: InverterOsc,
    walk_cv: ControlVoltage,
    walk_555: Astable,
    walk_env: Envelope,
    walk_out: HighPass,
    decay: Decay,
    dac_filter: LowPass,
    mixer: Mixer,
    amp: Integrator,
    couple1: HighPass,
    couple2: HighPass,
}

const K: f64 = 1_000.0;
const U: f64 = 1e-6;
const N: f64 = 1e-9;

impl DkCircuit {
    fn new() -> DkCircuit {
        DkCircuit {
            // The stomp: 24 bits, taps at QC of the second LS164 and QH of the third, on 2VF (4 kHz).
            noise: Noise::new(24, 10, 23, false, 3_072_000.0 / 16.0 / 12.0 / 2.0 / 2.0),
            count: 0,
            last_noise: false,
            stomp_env: Envelope::new(10.0 * K, 0.0, 0.0, 10.0 * K, 1.0 * U, 5.0),
            stomp_gate: TwoWay::new(5.0, 110.0 * K, 0.0, 10.0 * K, 3.3 * U),
            stomp_out: Integrator::new(750.0, parallel(7.1 * K, 4.7 * K), 0.0, 1.0 * U, 5.0, true),
            jump_osc: InverterOsc::new(18.0 * K, 3.3e6, 3.3 * U, false),
            jump_cv: ControlVoltage::new(10.0 * K, 10.0 * K, 10.0 * K, 1.2 * K, 10.0 * U, 5.0),
            jump_555: Astable::new(47.0 * K, 27.0 * K, 47.0 * N),
            jump_env: Envelope::new(10.0 * K, 0.0, 0.0, 10.0 * K, 1.0 * U, 5.0),
            jump_gate: TwoWay::new(5.0, 110.0 * K, 0.0, 10.0 * K, 4.7 * U),
            jump_out: Integrator::new(150.0, parallel(4.7 * K, 7.1 * K), 0.0, 1.0 * U, 5.0, true),
            walk_osc: InverterOsc::new(4.3 * K, 43.0 * K, 10.0 * U, true),
            walk_cv: ControlVoltage::new(1.0 * K, 10.0 * K, 12.0 * K, 1.2 * K, 3.3 * U, 5.0),
            walk_555: Astable::new(47.0 * K, 27.0 * K, 33.0 * N),
            walk_env: Envelope::new(1.0 * K, 4.7 * K, 1.0 * K, 10.0 * K, 3.3 * U, 5.0),
            walk_out: HighPass::new(11.2 * K, 4.7 * U, 0.0),
            decay: Decay::default(),
            dac_filter: LowPass::sallen_key(5.6 * K, 5.6 * K, 22.0 * N, 10.0 * N),
            // The mixer needs a 10 kΩ to ground that the schematics don't show (MAME's note: the
            // DAC is far too loud without it).
            mixer: Mixer::new(&[47.0 * K; 4], 10.0 * K, 100.0 * N, 1.0 * U),
            amp: Integrator::new(0.0, 150.0, 1_000.0, 33.0 * U, 5.0, false),
            couple1: HighPass::new(50.0 * K, 33.0 * U, 0.0),
            couple2: HighPass::new(1.0 * K, 4.7 * U, 0.0),
        }
    }

    /// One sample, in volts at the speaker's input; `s` holds latch 6H's bits inverted.
    fn step(&mut self, s: [f64; 3], dac: u8, decay_on: bool) -> f64 {
        // Stomp: the noise, divided by eight by a 74LS161, through the envelope's diode gate.
        let n = self.noise.step();
        let nb = n != 0.0;
        if nb != self.last_noise {
            self.last_noise = nb;
            if nb {
                self.count = (self.count + 1) & 7;
            }
        }
        let divided = if self.count > 3 { 5.0 } else { 0.0 };
        let e = self.stomp_env.step(s[2], 0.0);
        let g = self.stomp_gate.step(e > 0.6);
        let mix = (g - 0.4).max(divided - 0.8).max(0.0);
        let stomp = self.stomp_out.step(mix) * 5.1 / 7.1;
        // Jump: the 555, swept by its control voltage, through the envelope's diode gate.
        let o = self.jump_osc.step();
        let cv = self.jump_cv.step(s[1], o);
        let tone = self.jump_555.step(cv);
        let e = self.jump_env.step(s[1], 0.0);
        let g = self.jump_gate.step(e > 0.6);
        let mix = (g - 0.4).max(tone - 0.8).max(0.0);
        let jump = self.jump_out.step(mix) * 5.1 / 7.1;
        // Walk: the 555 chops the envelope.
        let o = self.walk_osc.step();
        let cv = self.walk_cv.step(s[0], o);
        let tone = self.walk_555.step(cv);
        let e = self.walk_env.step(s[0], tone);
        let walk = self.walk_out.step(e) * 0.5;
        // The DAC: 5 V full scale, its decay, the low-pass, VR2 at 90%.
        let d = self.decay.step(decay_on, 10.0 * K * 10.0 * U);
        let v = f64::from(dac) * 5.0 / 256.0 * (f64::from(u8::from(!decay_on)) + d);
        let dac = self.dac_filter.step(v) * 0.9;
        // The mixer and the amplifier.
        let m = self.mixer.step(&[stomp, jump, dac, walk]);
        let a = self.amp.step(m + 5.0 * 43.0 / 143.0);
        self.couple2.step(self.couple1.step(a))
    }
}

/// Donkey Kong Jr.'s (MAME's dkongjr).
#[derive(Clone, Debug, Serialize, Deserialize)]
struct JrCircuit {
    // Walk.
    walk_shot: OneShot,
    walk_vco1: Vco,
    walk_vco2: Vco,
    counter: u32,
    /// The counter's bits 11 and 12 as the VCO's input sees them (energy, last sample).
    walk_mod: f64,
    // Jump.
    jump_shot: OneShot,
    jump_cv: Mixer,
    jump_vco: Vco,
    jump_env: Envelope,
    jump_hp: HighPass,
    jump_lp: Smooth,
    // Climb.
    noise: Noise,
    climb_shot: OneShot,
    climb_env: Envelope,
    climb_hp: HighPass,
    climb_lp: Smooth,
    // Fall.
    fall_vco: Vco,
    decay: Decay,
    dac_filter: LowPass,
    mixer: Mixer,
    couple: HighPass,
}

impl JrCircuit {
    fn new() -> JrCircuit {
        JrCircuit {
            walk_shot: OneShot::new(47.0 * K, 4.7 * U),
            walk_vco1: Vco::new(22.0 * N, 10.0 * K, 3.3 * U, 1.0 * K, 5.0),
            walk_vco2: Vco::new(4.7 * N, 20.0 * K, 3.3 * U, 1.0 * K, 5.0),
            counter: 0,
            walk_mod: 4.15,
            jump_shot: OneShot::new(47.0 * K, 22.0 * U),
            jump_cv: Mixer::new(&[47.0 * K, 10.0 * K], 0.0, 47.0 * U, 0.0),
            jump_vco: Vco::new(220.0 * N, parallel(47.0 * K, 10.0 * K), 0.0, 1.0 * K, 5.0),
            jump_env: Envelope::new(120.0, 10.0 * K, 1.0, 100.0 * K, 10.0 * U, 5.0),
            jump_hp: HighPass::new(47.0 * K, 0.47 * U, 2.5),
            jump_lp: Smooth::new(100.0, 56.0 * N),
            noise: Noise::new(16, 2, 15, true, 710.0),
            climb_shot: OneShot::new(47.0 * K, 22.0 * U),
            climb_env: Envelope::new(120.0, 4.7 * K, 1.0, 100.0 * K, 10.0 * U, 5.0),
            climb_hp: HighPass::new(20.0 * K, 0.47 * U, 2.5),
            climb_lp: Smooth::new(120.0, 1.0 * U),
            fall_vco: Vco::new(0.12 * U, 30.0 * K, 47.0 * U, 1.0 * K, 5.0),
            decay: Decay::default(),
            dac_filter: LowPass::sallen_key(5.6 * K, 5.6 * K, 22.0 * N, 10.0 * N),
            mixer: Mixer::new(&[150.0 * K, 100.0 * K, 20.0 * K, 47.0 * K, 47.0 * K], 0.0, 10.0 * N, 1.0 * U),
            couple: HighPass::new(1.0 * K, 4.7 * U, 0.0),
        }
    }

    /// One sample; `s` holds latch 6H's bits 0, 1, 2 and 7 inverted, `fall` latch 5H bit 1.
    fn step(&mut self, s: [bool; 4], fall: bool, dac: u8, decay_on: bool) -> f64 {
        const SUB: usize = 8;
        let dt = DT / SUB as f64;
        let s7 = s[3];
        // Walk: the one-shot on the latch's line, the two oscillators, the counter and the gates,
        // worked out eight times a sample.
        let shot = self.walk_shot.step(s[0]);
        let f1 = self.walk_vco1.freq(if shot { 0.151 } else { 4.14 });
        let f2 = self.walk_vco2.freq(self.walk_mod);
        let (mut walk_high, mut bit11_high, mut mod_high) = (0.0, 0.0, 0.0);
        for _ in 0..SUB {
            let (falls, _) = self.walk_vco2.run(f2, dt);
            self.counter = (self.counter + falls) & 0x3FFF;
            let (_, h1) = self.walk_vco1.run(f1, dt);
            let c = self.counter;
            let (lo, hi) = if s7 { (c >> 3 & 1, c >> 11 & 1) } else { (c >> 6 & 1, c >> 12 & 1) };
            // With bit 7 set, the second oscillator is XORed in.
            let osc = if s7 { h1 } else { 0.0 };
            let x = if lo == 1 { 1.0 - osc } else { osc };
            // NAND with the one-shot.
            walk_high += if shot { 1.0 - x } else { 1.0 };
            mod_high += f64::from(hi as u8);
            bit11_high += f64::from((c >> 11 & 1) as u8);
        }
        let sub = SUB as f64;
        let walk = 0.2 + 4.7 * walk_high / sub;
        // The counter's top bit, inverted, steers the second oscillator from the next sample.
        self.walk_mod = 4.15 - (4.15 - 0.135) * mod_high / sub;
        let bit11 = bit11_high / sub;
        // Jump: the one-shot and the counter's bit 11 (inverted) sweep the oscillator.
        let jshot = self.jump_shot.step(s[1]);
        let cv = self.jump_cv.step(&[if jshot { 4.14 } else { 0.151 }, 4.14 - (4.14 - 0.151) * bit11]);
        let f = self.jump_vco.freq(cv);
        let (_, h) = self.jump_vco.run(f, DT);
        let e = self.jump_env.step(if jshot { 0.0 } else { 1.0 }, 4.5 * h);
        let jump = self.jump_lp.step(self.jump_hp.step(e));
        // Climb: the noise through the one-shot's envelope.
        let n = self.noise.step();
        let cshot = self.climb_shot.step(s[2]);
        let e = self.climb_env.step(if cshot { 0.0 } else { 4.0 }, n);
        let climb = self.climb_lp.step(self.climb_hp.step(e));
        // Fall: the fourth oscillator NANDed with the latch.
        let f = self.fall_vco.freq(if fall { 0.134 } else { 4.16 });
        let (_, h) = self.fall_vco.run(f, DT);
        let fall_v = if fall { 0.2 + 4.7 * (1.0 - h) } else { 4.9 };
        // The DAC.
        let d = self.decay.step(decay_on, 10.0 * K * 10.0 * U);
        let v = f64::from(dac) * 5.0 / 256.0 * (f64::from(u8::from(!decay_on)) + d);
        let dac = self.dac_filter.step(v);
        let m = self.mixer.step(&[fall_v, walk, climb, jump, dac]);
        self.couple.step(m)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
enum Circuit {
    Dk(Box<DkCircuit>),
    Jr(Box<JrCircuit>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sound {
    jr: bool,
    dac: u8,
    /// Port 2 bit 7 is low: the decay circuit works.
    decay_on: bool,
    latch: u8,
    sound9: bool,
    circuit: Circuit,
}

impl Sound {
    pub fn new(jr: bool) -> Sound {
        Sound {
            jr,
            dac: 0,
            decay_on: false,
            latch: 0,
            sound9: false,
            circuit: if jr { Circuit::Jr(Box::new(JrCircuit::new())) } else { Circuit::Dk(Box::new(DkCircuit::new())) },
        }
    }

    pub fn reset(&mut self) {
        *self = Sound::new(self.jr);
    }

    pub fn set(&mut self, input: Input) {
        match input {
            Input::Dac(v) => self.dac = v,
            Input::Decay(high) => self.decay_on = !high,
            Input::Latch6h(v) => self.latch = v,
            Input::Sound9(on) => self.sound9 = on,
            Input::Reset => self.reset(),
        }
    }

    /// One sample, as MAME's output stage scales it (full scale 1.0).
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f64 {
        let inv = |b: u8| self.latch >> b & 1 == 0;
        match &mut self.circuit {
            Circuit::Dk(c) => {
                let s = [f64::from(u8::from(inv(0))), f64::from(u8::from(inv(1))), f64::from(u8::from(inv(2)))];
                // MAME's output gain (32767 / 5 x 3.41) and its route to the speaker (0.9).
                c.step(s, self.dac, self.decay_on) * 3.41 / 5.0 * 0.9
            }
            Circuit::Jr(c) => {
                let s = [inv(0), inv(1), inv(2), inv(7)];
                // Full scale at 1.18 V, and the route at 0.5.
                c.step(s, self.sound9, self.dac, self.decay_on) / 1.18 * 0.5
            }
        }
    }
}
