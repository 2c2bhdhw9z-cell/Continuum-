//! The MOS 6532 RIOT: 128 bytes of RAM, two 8-bit ports (the joysticks on A, the console's
//! switches on B) and an interval timer.
//!
//! The chip counts the processor's clock, one tick a processor cycle. The machine hands it the
//! time in MARIA clocks, four to a tick; a tick's count is that over four, so two accesses made k
//! processor cycles apart are always k ticks apart, whatever the cycles' phase in the line.
//!
//! The timer behaves as the 6532 does on real consoles, as Stella's measurements describe it: its
//! prescaler (1, 8, 64 or 1024 ticks) runs on its own; a write to a TIMxT register loads the count
//! and sets the prescaler so the count drops one tick later, then once a prescale; when the count
//! passes zero it raises the interrupt flag, shows $FF and drops every tick; reading INTIM clears
//! the flag and lets the count drop at the prescale again, on the prescaler's own beat. Checked
//! against Stella reading the same instructions on a 2600, which has the same chip
//! (tests/riot_stella.rs). MAME 0.289 reads INTIM two or three lower, from its clock arithmetic
//! rounding down.

use serde::{Deserialize, Serialize};

const TICK: u64 = 4;

#[derive(Clone, Serialize, Deserialize)]
pub struct Riot {
    pub ram: Vec<u8>,
    pa_out: u8,
    pa_ddr: u8,
    pb_out: u8,
    pb_ddr: u8,
    timer: u8,
    divider: u32,
    /// The prescaler's count, 0 to divider - 1.
    sub: u32,
    /// Past zero: dropping every tick until INTIM is read.
    wrapped: bool,
    wrapped_this_tick: bool,
    flag_timer: bool,
    flag_edge: bool,
    ie_timer: bool,
    ie_edge: bool,
    /// The tick the timer was last brought up to.
    last: u64,
}

impl Default for Riot {
    fn default() -> Self {
        Riot {
            ram: vec![0; 128],
            pa_out: 0,
            pa_ddr: 0,
            pb_out: 0,
            pb_ddr: 0,
            timer: 0xFF,
            divider: 1024,
            sub: 1023,
            wrapped: false,
            wrapped_this_tick: false,
            flag_timer: false,
            flag_edge: false,
            ie_timer: false,
            ie_edge: false,
            last: 0,
        }
    }
}

impl Riot {
    /// Brings the timer up to tick `t`.
    fn update(&mut self, t: u64) {
        if t <= self.last {
            return;
        }
        let mut cycles = t - self.last;
        self.last = t;
        self.wrapped_this_tick = false;
        let sub = self.sub as u64;
        let div = self.divider as u64;
        self.sub = ((cycles + sub) % div) as u32;
        if !self.wrapped {
            let ticks = (cycles + sub) / div;
            if ticks > self.timer as u64 {
                cycles -= (self.timer as u64 + 1) * div - sub;
                self.wrapped_this_tick = cycles == 0;
                self.timer = 0xFF;
                self.wrapped = true;
                self.flag_timer = true;
            } else {
                self.timer -= ticks as u8;
                cycles = 0;
            }
        }
        if cycles > 0 {
            self.wrapped_this_tick = (self.timer as u64 + 1) % 0x100 <= cycles;
            self.timer = (self.timer as u64).wrapping_sub(cycles) as u8;
        }
    }

    /// Port B as the chip drives it: output bits where the direction register says output, the
    /// rest pulled high.
    pub fn pb_drive(&self) -> u8 {
        (self.pb_out & self.pb_ddr) | !self.pb_ddr
    }

    pub fn read_io(&mut self, offset: u16, now: u64, pa_in: u8, pb_in: u8) -> u8 {
        let o = offset & 0x1F;
        let t = now / TICK;
        match o & 0x07 {
            0 => (self.pa_out & self.pa_ddr) | (pa_in & !self.pa_ddr),
            1 => self.pa_ddr,
            2 => (self.pb_out & self.pb_ddr) | (pb_in & !self.pb_ddr),
            3 => self.pb_ddr,
            4 | 6 => {
                self.update(t);
                // The flag isn't cleared by a read in the very tick the count passes zero.
                if !self.wrapped_this_tick {
                    self.flag_timer = false;
                }
                self.wrapped = false;
                self.ie_timer = o & 0x08 != 0;
                self.timer
            }
            _ => {
                self.update(t);
                let v = (if self.flag_timer { 0x80 } else { 0 }) | (if self.flag_edge { 0x40 } else { 0 });
                self.flag_edge = false;
                v
            }
        }
    }

    pub fn write_io(&mut self, offset: u16, value: u8, now: u64) {
        let o = offset & 0x1F;
        let t = now / TICK;
        if o & 0x14 == 0x14 {
            // TIM1T, TIM8T, TIM64T, T1024T (bit 3: timer interrupt on).
            self.update(t);
            self.divider = 1 << [0, 3, 6, 10][(o & 3) as usize];
            self.sub = self.divider - 1;
            self.timer = value;
            self.wrapped = false;
            self.flag_timer = false;
            self.ie_timer = o & 0x08 != 0;
            return;
        }
        if o & 0x14 == 0x04 {
            // The PA7 edge-detect control; nothing is wired to PA7's edge on the 7800.
            self.ie_edge = o & 0x02 != 0;
            return;
        }
        match o & 0x03 {
            0 => self.pa_out = value,
            1 => self.pa_ddr = value,
            2 => self.pb_out = value,
            _ => self.pb_ddr = value,
        }
    }

    /// The IRQ output (not connected on the 7800, kept for completeness and tests).
    pub fn irq(&mut self, now: u64) -> bool {
        self.update(now / TICK);
        (self.ie_timer && self.flag_timer) || (self.ie_edge && self.flag_edge)
    }
}
