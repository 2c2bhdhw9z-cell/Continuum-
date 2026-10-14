//! The General Instrument AY-3-8910 sound chip, two of which play Konami's Scramble, Super Cobra,
//! Frogger and Amidar: three square-wave tone generators, the 17-bit noise generator, the
//! envelope generator with its sixteen shapes, the mixer, the fixed or envelope amplitude of each
//! channel, the 16-step logarithmic output levels, the register address latch and what each
//! register reads back, and the two 8-bit I/O ports (Konami's sound boards read the sound
//! command and a timer through them).
//!
//! The chip divides its input clock by 8 before anything counts: `tick` is one period of that,
//! and returns the three channels' output levels (0-15) for it. A tone's period register TP
//! gives a square wave of 16 × TP input clocks, noise shifts every 16 × NP clocks, and each of the
//! envelope's 16 steps lasts 16 × EP clocks, as the data sheet's equations have it.
//!
//! Written from GI's AY-3-8910/8912 data manual and from how MAME's AY-3-8910 settles what it
//! leaves open: a counter counts up and wraps when it reaches its period (so lowering a period
//! takes effect at once), a period of 0 counts as 1 for the tones and the noise, an envelope
//! period of 0 steps twice as fast as 1, and writing the shape register restarts the envelope
//! without resetting its prescaler. Proven by the tests at the end of this file against the
//! data manual (periods, the envelope shapes, the noise sequence, the mixer, the registers and
//! ports), and against MAME's output for register writes of our own (tests/ay8910_mame.rs).

use serde::{Deserialize, Serialize};

/// The data manual's D/A converter output (Fig. 9) for each 4-bit level, normalized to 1 V at
/// level 15. The figure labels 1, .707, .5, .303, .25, .1515 and .125 V for levels 15 down to 9;
/// below that it keeps halving every two steps. Level 0 is the channel off.
pub const LEVELS: [f32; 16] = [
    0.0,
    0.0078125,
    0.009_468_75,
    0.015625,
    0.018_937_5,
    0.03125,
    0.037875,
    0.0625,
    0.07575,
    0.125,
    0.1515,
    0.25,
    0.303,
    0.5,
    0.707,
    1.0,
];

/// The bits of each register the chip has: the rest read back as 0 (GI's part, as MAME has
/// measured it; Yamaha's YM2149 keeps them).
const MASK: [u8; 16] = [
    0xFF, 0x0F, 0xFF, 0x0F, 0xFF, 0x0F, 0x1F, 0xFF, 0x1F, 0x1F, 0x1F, 0xFF, 0xFF, 0x0F, 0xFF, 0xFF,
];

/// Register numbers (the data manual counts them in octal, R0-R17).
pub const TONE_A_FINE: u8 = 0;
pub const NOISE_PERIOD: u8 = 6;
pub const ENABLE: u8 = 7;
pub const AMPLITUDE_A: u8 = 8;
pub const ENVELOPE_FINE: u8 = 11;
pub const ENVELOPE_COARSE: u8 = 12;
pub const ENVELOPE_SHAPE: u8 = 13;
pub const PORT_A: u8 = 14;
pub const PORT_B: u8 = 15;

/// One of the two I/O ports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Port {
    A,
    B,
}

/// What the I/O ports are wired to on a board.
pub trait Ports {
    /// The pins of port A or B as a read of R14 or R15 sees them. An input port reads what the
    /// outside world drives (with nothing driving, the chip's pull-ups read 0xFF); an output port
    /// reads its pins back too. `None` means nothing is wired: the read gives what the register
    /// last held.
    fn read(&mut self, _port: Port) -> Option<u8> {
        None
    }
    /// Port A or B's pins changed: the register's value when the port drives them, 0xFF when it
    /// has just become an input and they float up on the pull-ups.
    fn write(&mut self, _port: Port, _val: u8) {}
}

/// Ports wired to nothing.
pub struct NoPorts;

impl Ports for NoPorts {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ay8910 {
    /// R0-R15, holding only the bits each one has.
    pub regs: [u8; 16],
    /// The register the address latch holds.
    pub latch: u8,
    /// The last address written had the chip's own code (0000) in its top four bits. A
    /// different code deselects the chip: writes are ignored and reads float (0xFF).
    pub selected: bool,
    /// Each tone generator's up-counter and its square wave.
    pub tone_count: [u16; 3],
    pub tone_out: [bool; 3],
    /// The noise generator's period counter, the divide-by-2 after it, and the 17-bit shift
    /// register whose bit 0 is the noise.
    pub noise_count: u8,
    pub noise_half: bool,
    pub rng: u32,
    /// The envelope's prescaler, its 4-bit step counter (counting down from 15), the attack
    /// pattern XORed onto the step (0 to fall, 15 to rise), and the shape's hold and alternate.
    pub env_count: u32,
    pub env_step: i8,
    pub env_attack: u8,
    pub env_hold: bool,
    pub env_alternate: bool,
    /// The envelope has finished a one-shot shape and holds its last level.
    pub env_holding: bool,
}

impl Default for Ay8910 {
    fn default() -> Self {
        Ay8910::new()
    }
}

impl Ay8910 {
    pub fn new() -> Ay8910 {
        let mut ay = Ay8910 {
            regs: [0; 16],
            latch: 0,
            selected: false,
            tone_count: [0; 3],
            tone_out: [false; 3],
            noise_count: 0,
            noise_half: false,
            rng: 1,
            env_count: 0,
            env_step: 0,
            env_attack: 0,
            env_hold: false,
            env_alternate: false,
            env_holding: false,
        };
        ay.reset();
        ay
    }

    /// /RESET: every register to 0 (so all three channels' tones and noise are on at amplitude
    /// 0, and both ports are inputs), the counters cleared, the noise register seeded with 1,
    /// shape 0 started, and no register selected until an address is written. The ports' pins
    /// float up; a board that latches them sees 0xFF.
    pub fn reset(&mut self) {
        self.regs = [0; 16];
        self.latch = 0;
        self.selected = false;
        self.tone_count = [0; 3];
        self.tone_out = [false; 3];
        self.noise_count = 0;
        self.noise_half = false;
        self.rng = 1;
        self.env_count = 0;
        self.start_envelope();
    }

    /// The address latch (BDIR, BC1 high): the low four bits pick the register, and the top four
    /// must be the chip's code, 0000, or it stops responding until they are.
    pub fn write_address(&mut self, v: u8) {
        self.selected = v >> 4 == 0;
        if self.selected {
            self.latch = v & 0x0F;
        }
    }

    /// A write to the latched register (BDIR high, BC1 low).
    pub fn write_data<P: Ports>(&mut self, v: u8, ports: &mut P) {
        if !self.selected {
            return;
        }
        let r = self.latch;
        let old_enable = self.regs[ENABLE as usize];
        self.regs[r as usize] = v & MASK[r as usize];
        match r {
            ENABLE => {
                // A port turned to output drives its register; turned to input it floats up.
                for (bit, port, reg) in [(0x40, Port::A, PORT_A), (0x80, Port::B, PORT_B)] {
                    if (v ^ old_enable) & bit != 0 {
                        let pins = if v & bit != 0 {
                            self.regs[reg as usize]
                        } else {
                            0xFF
                        };
                        ports.write(port, pins);
                    }
                }
            }
            // Writing the shape starts it again from its first step, whatever the value.
            ENVELOPE_SHAPE => self.start_envelope(),
            PORT_A | PORT_B => {
                // An input port only stores what's written; an output port drives it.
                let (bit, port) = if r == PORT_A {
                    (0x40, Port::A)
                } else {
                    (0x80, Port::B)
                };
                if self.regs[ENABLE as usize] & bit != 0 {
                    ports.write(port, v);
                }
            }
            _ => {}
        }
    }

    /// A read of the latched register (BDIR low, BC1 high). R14 and R15 read the port pins.
    pub fn read_data<P: Ports>(&mut self, ports: &mut P) -> u8 {
        if !self.selected {
            return 0xFF;
        }
        let r = self.latch;
        let port = match r {
            PORT_A => Some(Port::A),
            PORT_B => Some(Port::B),
            _ => None,
        };
        if let Some(port) = port {
            if let Some(pins) = ports.read(port) {
                self.regs[r as usize] = pins;
            }
        }
        self.regs[r as usize]
    }

    /// A tone generator's 12-bit period: the fine register and the low four bits of the coarse.
    pub fn tone_period(&self, ch: usize) -> u16 {
        u16::from(self.regs[2 * ch + 1]) << 8 | u16::from(self.regs[2 * ch])
    }

    /// The envelope's 16-bit period.
    pub fn envelope_period(&self) -> u32 {
        u32::from(self.regs[ENVELOPE_COARSE as usize]) << 8
            | u32::from(self.regs[ENVELOPE_FINE as usize])
    }

    /// The envelope's level now (E3-E0).
    pub fn envelope_level(&self) -> u8 {
        (self.env_step as u8 & 0x0F) ^ self.env_attack
    }

    /// The shape register's four bits, as the data manual names them: Continue, Attack,
    /// Alternate, Hold. With Continue clear the envelope runs once and drops to 0, which is the
    /// same as Hold set and Alternate equal to Attack.
    fn start_envelope(&mut self) {
        let shape = self.regs[ENVELOPE_SHAPE as usize];
        self.env_attack = if shape & 0x04 != 0 { 0x0F } else { 0 };
        if shape & 0x08 == 0 {
            self.env_hold = true;
            self.env_alternate = self.env_attack != 0;
        } else {
            self.env_hold = shape & 0x01 != 0;
            self.env_alternate = shape & 0x02 != 0;
        }
        self.env_step = 15;
        self.env_holding = false;
    }

    /// One period of the chip's clock divided by 8. Returns each channel's output level for it:
    /// 0 while the mixer has the channel off, otherwise its fixed amplitude or the envelope.
    pub fn tick(&mut self) -> [u8; 3] {
        // Tone: count up to the period, then flip the square wave.
        for ch in 0..3 {
            let period = self.tone_period(ch).max(1);
            self.tone_count[ch] += 1;
            while self.tone_count[ch] >= period {
                self.tone_count[ch] -= period;
                self.tone_out[ch] = !self.tone_out[ch];
            }
        }

        // Noise: the period counter, a divide by 2, then one shift of the 17-bit register, fed
        // back from bits 0 and 3.
        self.noise_count += 1;
        if self.noise_count >= self.regs[NOISE_PERIOD as usize] {
            self.noise_count = 0;
            self.noise_half = !self.noise_half;
            if !self.noise_half {
                let feedback = (self.rng ^ self.rng >> 3) & 1;
                self.rng = self.rng >> 1 | feedback << 16;
            }
        }
        let noise = self.rng & 1 != 0;

        // Envelope: a step every 2 × EP ticks (each tick when EP is 0), sixteen steps a cycle.
        if !self.env_holding {
            self.env_count += 1;
            if self.env_count >= 2 * self.envelope_period() {
                self.env_count = 0;
                self.env_step -= 1;
                if self.env_step < 0 {
                    if self.env_alternate {
                        self.env_attack ^= 0x0F;
                    }
                    if self.env_hold {
                        self.env_holding = true;
                        self.env_step = 0;
                    } else {
                        self.env_step = 15;
                    }
                }
            }
        }
        let envelope = self.envelope_level();

        // Mixer: a channel is on while its tone (or the tone being disabled) and its noise (or
        // the noise being disabled) are both high. With both disabled it stays on, which is how
        // a program plays samples through the amplitude register.
        let enable = self.regs[ENABLE as usize];
        let mut out = [0u8; 3];
        for (ch, level) in out.iter_mut().enumerate() {
            let tone = self.tone_out[ch] || enable & (1 << ch) != 0;
            let noise = noise || enable & (8 << ch) != 0;
            if tone && noise {
                let amplitude = self.regs[AMPLITUDE_A as usize + ch];
                *level = if amplitude & 0x10 != 0 {
                    envelope
                } else {
                    amplitude & 0x0F
                };
            }
        }
        out
    }
}

/// The data manual's behaviour, one piece at a time.
#[cfg(test)]
mod tests {
    use super::*;

    fn set(ay: &mut Ay8910, r: u8, v: u8) {
        ay.write_address(r);
        ay.write_data(v, &mut NoPorts);
    }

    fn get(ay: &mut Ay8910, r: u8) -> u8 {
        ay.write_address(r);
        ay.read_data(&mut NoPorts)
    }

    /// Channel A alone, at amplitude 15: its tone (and noise) as the mixer sets them.
    fn channel_a(enable: u8) -> Ay8910 {
        let mut ay = Ay8910::new();
        set(&mut ay, ENABLE, enable);
        set(&mut ay, AMPLITUDE_A, 15);
        ay
    }

    /// The ticks at which channel A's level changes, over `ticks`.
    fn edges(ay: &mut Ay8910, ticks: usize) -> Vec<usize> {
        let mut last = ay.tick()[0];
        let mut at = Vec::new();
        for t in 1..ticks {
            let now = ay.tick()[0];
            if now != last {
                at.push(t);
            }
            last = now;
        }
        at
    }

    #[test]
    fn a_tone_is_a_square_wave_of_16_tp_clocks() {
        // f = clock / (16 × TP): the wave flips every TP ticks of the clock divided by 8.
        for tp in [1u16, 2, 3, 100, 0x123, 4095] {
            let mut ay = channel_a(0x3E); // tone A on, everything else off
            set(&mut ay, 0, tp as u8);
            set(&mut ay, 1, (tp >> 8) as u8);
            let e = edges(&mut ay, 8 * usize::from(tp) + 2);
            let gaps: Vec<usize> = e.windows(2).map(|w| w[1] - w[0]).collect();
            assert!(
                gaps.iter().all(|&g| g == usize::from(tp)),
                "TP {tp}: {gaps:?}"
            );
            assert!(gaps.len() >= 5, "TP {tp}");
        }
    }

    #[test]
    fn tone_period_0_is_the_same_as_1() {
        let mut zero = channel_a(0x3E);
        let mut one = channel_a(0x3E);
        set(&mut one, 0, 1);
        for _ in 0..100 {
            assert_eq!(zero.tick(), one.tick());
        }
    }

    #[test]
    fn the_coarse_tune_registers_have_four_bits() {
        let mut ay = Ay8910::new();
        for (r, v) in [(1u8, 0xFFu8), (3, 0xA5), (5, 0x10)] {
            set(&mut ay, r, v);
        }
        assert_eq!(
            [ay.tone_period(0), ay.tone_period(1), ay.tone_period(2)],
            [0xF00, 0x500, 0x000]
        );
    }

    #[test]
    fn lowering_a_period_takes_effect_at_once() {
        // Counting up from 0, a counter already past the new period wraps on the next tick.
        let mut ay = channel_a(0x3E);
        set(&mut ay, 0, 0);
        set(&mut ay, 1, 1); // TP 256
        let first = ay.tick()[0];
        for _ in 0..99 {
            assert_eq!(ay.tick()[0], first);
        }
        set(&mut ay, 1, 0);
        set(&mut ay, 0, 10);
        // The count of 100 passes 10 at once; ten flips leave the wave where it was, and from
        // then on it flips every 10 ticks.
        let e = edges(&mut ay, 60);
        let gaps: Vec<usize> = e.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(gaps.iter().all(|&g| g == 10), "{e:?}");
    }

    /// An independent 17-bit shift register: taps at bits 0 and 3, fed into bit 16.
    fn lfsr_bits(n: usize) -> Vec<bool> {
        let mut r: u32 = 1;
        (0..n)
            .map(|_| {
                let fb = (r ^ (r >> 3)) & 1;
                r = (r >> 1) | (fb << 16);
                r & 1 != 0
            })
            .collect()
    }

    #[test]
    fn noise_shifts_every_16_np_clocks_and_follows_the_17_bit_register() {
        for np in [1u8, 2, 5, 31] {
            let mut ay = channel_a(0x37); // noise A on, tones off
            set(&mut ay, NOISE_PERIOD, np);
            let ticks = 2 * usize::from(np);
            let mut seen = Vec::new();
            for _ in 0..200 {
                let mut level = 0;
                for _ in 0..ticks {
                    level = ay.tick()[0];
                }
                seen.push(level != 0);
            }
            assert_eq!(seen, lfsr_bits(200), "NP {np}");
        }
    }

    #[test]
    fn noise_period_0_is_the_same_as_1_and_r6_has_five_bits() {
        let mut zero = channel_a(0x37);
        let mut one = channel_a(0x37);
        set(&mut one, NOISE_PERIOD, 1);
        for _ in 0..1000 {
            assert_eq!(zero.tick(), one.tick());
        }
        set(&mut zero, NOISE_PERIOD, 0xE5);
        assert_eq!(get(&mut zero, NOISE_PERIOD), 0x05);
    }

    #[test]
    fn the_noise_register_repeats_after_131071_shifts() {
        let mut ay = Ay8910::new();
        set(&mut ay, NOISE_PERIOD, 1);
        let start = ay.rng;
        let mut shifts = 0u32;
        loop {
            ay.tick();
            ay.tick();
            shifts += 1;
            if ay.rng == start {
                break;
            }
        }
        assert_eq!(shifts, (1 << 17) - 1);
    }

    /// The envelope's level at each of its first `steps` steps, EP 1 (a step every two ticks).
    fn envelope(shape: u8, steps: usize) -> Vec<u8> {
        let mut ay = channel_a(0x3F); // tone and noise off: the channel follows the envelope
        set(&mut ay, AMPLITUDE_A, 0x10);
        set(&mut ay, ENVELOPE_FINE, 1);
        set(&mut ay, ENVELOPE_SHAPE, shape);
        (0..steps)
            .map(|_| {
                ay.tick();
                ay.tick()[0]
            })
            .collect()
    }

    #[test]
    fn all_sixteen_envelope_shapes_match_the_data_manual() {
        // Fig. 7: the shape after the first step, over 64 steps (four cycles).
        let down: Vec<u8> = (0..16).rev().collect();
        let up: Vec<u8> = (0..16).collect();
        let hold = |v: u8, n: usize| vec![v; n];
        let cat = |parts: &[Vec<u8>]| parts.concat();
        let expect = |shape: u8| -> Vec<u8> {
            let s = match shape {
                0..=3 | 9 => cat(&[down.clone(), hold(0, 48)]), // \___
                4..=7 | 15 => cat(&[up.clone(), hold(0, 48)]),  // /___
                8 => cat(&[down.clone(), down.clone(), down.clone(), down.clone()]), // \\\\
                10 => cat(&[down.clone(), up.clone(), down.clone(), up.clone()]), // \/\/
                11 => cat(&[down.clone(), hold(15, 48)]),       // \```
                12 => cat(&[up.clone(), up.clone(), up.clone(), up.clone()]), // ////
                13 => cat(&[up.clone(), hold(15, 48)]),         // /```
                _ => cat(&[up.clone(), down.clone(), up.clone(), down.clone()]), // /\/\
            };
            // The first level shows from the write; the first step comes 2 × EP ticks later.
            s[1..].to_vec()
        };
        for shape in 0..16u8 {
            assert_eq!(envelope(shape, 63), expect(shape), "shape {shape}");
        }
    }

    #[test]
    fn an_envelope_step_lasts_16_ep_clocks_and_ep_0_is_half_of_1() {
        for (ep, ticks_per_step) in [(0u16, 1usize), (1, 2), (2, 4), (300, 600)] {
            let mut ay = channel_a(0x3F);
            set(&mut ay, AMPLITUDE_A, 0x10);
            set(&mut ay, ENVELOPE_FINE, ep as u8);
            set(&mut ay, ENVELOPE_COARSE, (ep >> 8) as u8);
            set(&mut ay, ENVELOPE_SHAPE, 0x08); // \\\\
            let e = edges(&mut ay, 20 * ticks_per_step);
            let gaps: Vec<usize> = e.windows(2).map(|w| w[1] - w[0]).collect();
            assert!(
                gaps.iter().all(|&g| g == ticks_per_step),
                "EP {ep}: {gaps:?}"
            );
        }
    }

    #[test]
    fn writing_the_shape_restarts_the_envelope() {
        let mut ay = channel_a(0x3F);
        set(&mut ay, AMPLITUDE_A, 0x10);
        set(&mut ay, ENVELOPE_SHAPE, 0x0D); // /```, EP 0: up in 16 ticks, then holds 15
        for _ in 0..40 {
            ay.tick();
        }
        assert_eq!(ay.tick()[0], 15);
        set(&mut ay, ENVELOPE_SHAPE, 0x0D);
        assert_eq!(ay.tick()[0], 1, "it starts again from the bottom");
    }

    #[test]
    fn the_mixer_ands_tone_and_noise_and_level_0_is_off() {
        // Tone and noise both on: the channel is on only while both are high.
        let mut both = channel_a(0x36);
        set(&mut both, 0, 3);
        set(&mut both, NOISE_PERIOD, 2);
        let mut tone = channel_a(0x3E);
        set(&mut tone, 0, 3);
        let mut noise = channel_a(0x37);
        set(&mut noise, NOISE_PERIOD, 2);
        for _ in 0..2000 {
            let (b, t, n) = (both.tick()[0], tone.tick()[0], noise.tick()[0]);
            assert_eq!(b != 0, t != 0 && n != 0);
        }
        // Both disabled: a steady level, the amplitude itself.
        let mut dc = channel_a(0x3F);
        for v in 0..16 {
            set(&mut dc, AMPLITUDE_A, v);
            assert_eq!(dc.tick(), [v, 0, 0]);
        }
    }

    #[test]
    fn each_channel_has_its_own_tone_and_amplitude() {
        let mut ay = Ay8910::new();
        set(&mut ay, ENABLE, 0x38); // three tones, no noise
        for (ch, tp, amp) in [(0u8, 2u8, 5u8), (1, 3, 9), (2, 5, 14)] {
            set(&mut ay, 2 * ch, tp);
            set(&mut ay, AMPLITUDE_A + ch, amp);
        }
        let mut out = [[0u8; 3]; 60];
        for o in out.iter_mut() {
            *o = ay.tick();
        }
        for (ch, tp, amp) in [(0usize, 2usize, 5u8), (1, 3, 9), (2, 5, 14)] {
            let wave: Vec<u8> = out.iter().map(|o| o[ch]).collect();
            for (t, &v) in wave.iter().enumerate() {
                // The wave starts low and flips every TP ticks, the first flip on tick TP.
                let high = ((t + 1) / tp) % 2 == 1;
                assert_eq!(v, if high { amp } else { 0 }, "channel {ch} tick {t}");
            }
        }
    }

    #[test]
    fn registers_read_back_only_the_bits_they_have() {
        let mut ay = Ay8910::new();
        for r in 0..14u8 {
            set(&mut ay, r, 0xFF);
        }
        let back: Vec<u8> = (0..14u8).map(|r| get(&mut ay, r)).collect();
        assert_eq!(back, MASK[..14]);
    }

    #[test]
    fn an_address_with_another_chip_code_deselects_it() {
        let mut ay = Ay8910::new();
        set(&mut ay, AMPLITUDE_A, 7);
        ay.write_address(0x18); // code 0001: not this chip
        ay.write_data(3, &mut NoPorts);
        assert_eq!(ay.read_data(&mut NoPorts), 0xFF, "the bus floats");
        assert_eq!(ay.regs[AMPLITUDE_A as usize], 7);
        assert_eq!(ay.latch, AMPLITUDE_A, "the latch keeps its register");
        // Before any address after reset, nothing is selected either.
        let mut fresh = Ay8910::new();
        fresh.write_data(5, &mut NoPorts);
        assert_eq!(fresh.regs, [0; 16]);
    }

    /// Konami's wiring: port A reads the sound command, port B a timer; a log of pin changes.
    struct Board {
        command: u8,
        timer: u8,
        pins: Vec<(Port, u8)>,
    }

    impl Ports for Board {
        fn read(&mut self, port: Port) -> Option<u8> {
            Some(match port {
                Port::A => self.command,
                Port::B => self.timer,
            })
        }
        fn write(&mut self, port: Port, val: u8) {
            self.pins.push((port, val));
        }
    }

    #[test]
    fn the_ports_read_the_board_and_drive_it_when_outputs() {
        let mut ay = Ay8910::new();
        let mut board = Board {
            command: 0x42,
            timer: 0x8E,
            pins: Vec::new(),
        };
        ay.write_address(PORT_A);
        assert_eq!(ay.read_data(&mut board), 0x42);
        ay.write_address(PORT_B);
        assert_eq!(ay.read_data(&mut board), 0x8E);
        // As inputs, writes only store.
        ay.write_address(PORT_A);
        ay.write_data(0x11, &mut board);
        assert!(board.pins.is_empty());
        // Port A to output: it drives what was stored; then a write drives at once.
        ay.write_address(ENABLE);
        ay.write_data(0x40, &mut board);
        ay.write_address(PORT_A);
        ay.write_data(0x22, &mut board);
        // Back to input: the pins float up.
        ay.write_address(ENABLE);
        ay.write_data(0x00, &mut board);
        assert_eq!(
            board.pins,
            [(Port::A, 0x11), (Port::A, 0x22), (Port::A, 0xFF)]
        );
        // With nothing wired, a port reads what its register holds.
        ay.write_address(PORT_A);
        assert_eq!(ay.read_data(&mut NoPorts), 0x22);
    }

    #[test]
    fn reset_state() {
        let mut ay = Ay8910::new();
        // Every register 0: all tones and noise on, amplitudes 0, so silence.
        assert_eq!(ay.regs, [0; 16]);
        assert_eq!(ay.tick(), [0, 0, 0]);
        assert_eq!(ay.rng, 1);
        assert!(!ay.selected);
    }

    #[test]
    fn the_datasheet_levels_double_every_two_steps() {
        // 16 levels over 15 steps of about 3 dB each, 0 being off; from level 1 up, each level
        // is twice the one two below it, except .707 V over .303 V at the top.
        assert_eq!((LEVELS[0], LEVELS[15]), (0.0, 1.0));
        for n in 1..15 {
            let ratio = LEVELS[n + 1] / LEVELS[n];
            assert!((1.2..=1.7).contains(&ratio), "{n}: {ratio}");
        }
        for n in 3..16 {
            let want = if n == 14 { 0.707 / 0.303 } else { 2.0 };
            assert!((LEVELS[n] / LEVELS[n - 2] - want).abs() < 1e-5, "{n}");
        }
    }

    #[test]
    fn a_save_state_round_trips() {
        let mut ay = Ay8910::new();
        for (r, v) in [
            (0u8, 0x55u8),
            (1, 1),
            (6, 7),
            (7, 0x30),
            (8, 0x10),
            (11, 3),
            (13, 0x0E),
        ] {
            set(&mut ay, r, v);
        }
        for _ in 0..777 {
            ay.tick();
        }
        let bytes = bincode::serialize(&ay).unwrap();
        let mut back: Ay8910 = bincode::deserialize(&bytes).unwrap();
        assert_eq!(back, ay);
        for _ in 0..5000 {
            assert_eq!(back.tick(), ay.tick());
        }
    }
}
