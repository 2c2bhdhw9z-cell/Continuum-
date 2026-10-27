//! The MOS 6532 RIOT: 128 bytes of RAM, two 8-bit ports (the joysticks on A, the console's
//! switches on B) and an interval timer, clocked at the processor's 1.79 MHz.
//!
//! Time is counted in MARIA clocks (four to a RIOT tick), so a read made at any moment gives the
//! value the chip shows then. The timer follows the 6532 data sheet as MAME 0.289 models it: a
//! write of N to a TIMxT register ends the count after 1 + N x prescale ticks, the timer then
//! raises its flag and keeps counting down once a tick; reading INTIM after that clears the flag
//! and starts the programmed prescale again from the value read.

use serde::{Deserialize, Serialize};

const TICK: u64 = 4;

#[derive(Clone, Serialize, Deserialize)]
pub struct Riot {
    pub ram: Vec<u8>,
    pa_out: u8,
    pa_ddr: u8,
    pb_out: u8,
    pb_ddr: u8,
    /// log2 of the prescale (0, 3, 6 or 10).
    shift: u32,
    /// When the current count ends, in MARIA clocks.
    target: u64,
    counting: bool,
    irq_timer: bool,
    ie_timer: bool,
    /// When the flag was last raised (MARIA clocks).
    timeout: u64,
    irq_edge: bool,
    ie_edge: bool,
}

impl Default for Riot {
    fn default() -> Self {
        Riot {
            ram: vec![0; 128],
            pa_out: 0,
            pa_ddr: 0,
            pb_out: 0,
            pb_ddr: 0,
            shift: 10,
            target: (256 << 10) * TICK,
            counting: true,
            irq_timer: false,
            ie_timer: false,
            timeout: 0,
            irq_edge: false,
            ie_edge: false,
        }
    }
}

impl Riot {
    /// Brings the count up to `now`: a finished count raises the flag and turns into the
    /// once-a-tick count that wraps every 256 ticks.
    fn update(&mut self, now: u64) {
        while now >= self.target {
            if self.counting {
                self.timeout = self.target;
                self.irq_timer = true;
            }
            self.counting = false;
            self.target += 256 * TICK;
        }
    }

    fn start(&mut self, now: u64, value: u8) {
        self.counting = true;
        let ticks = now / TICK + 1 + ((value as u64) << self.shift);
        self.target = ticks * TICK;
    }

    fn timer_value(&mut self, now: u64) -> u8 {
        self.update(now);
        let shift = if self.counting { self.shift } else { 0 };
        let remain = (self.target - now) / TICK;
        let val = (remain >> shift) as u8;
        if remain & ((1u64 << shift) - 1) != 0 {
            val
        } else {
            val.wrapping_sub(1)
        }
    }

    /// Port B as the chip drives it: output bits where the direction register says output, the
    /// rest pulled high.
    pub fn pb_drive(&self) -> u8 {
        (self.pb_out & self.pb_ddr) | !self.pb_ddr
    }

    pub fn read_io(&mut self, offset: u16, now: u64, pa_in: u8, pb_in: u8) -> u8 {
        let o = offset & 0x1F;
        match o & 0x07 {
            0 => (self.pa_out & self.pa_ddr) | (pa_in & !self.pa_ddr),
            1 => self.pa_ddr,
            2 => (self.pb_out & self.pb_ddr) | (pb_in & !self.pb_ddr),
            3 => self.pb_ddr,
            4 | 6 => {
                let v = self.timer_value(now);
                // The flag isn't cleared by a read within half a tick of being raised.
                if self.timeout + TICK / 2 < now {
                    self.irq_timer = false;
                    if !self.counting {
                        self.start(now, v);
                    }
                }
                self.ie_timer = o & 0x08 != 0;
                v
            }
            _ => {
                self.update(now);
                let v = (if self.irq_timer { 0x80 } else { 0 }) | (if self.irq_edge { 0x40 } else { 0 });
                self.irq_edge = false;
                v
            }
        }
    }

    pub fn write_io(&mut self, offset: u16, value: u8, now: u64) {
        let o = offset & 0x1F;
        if o & 0x14 == 0x14 {
            // TIM1T, TIM8T, TIM64T, T1024T (bit 3: timer interrupt on).
            self.update(now);
            self.shift = [0, 3, 6, 10][(o & 3) as usize];
            self.start(now, value);
            self.irq_timer = false;
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
        self.update(now);
        (self.ie_timer && self.irq_timer) || (self.ie_edge && self.irq_edge)
    }

    /// Keeps the stored times small: subtracts `by` MARIA clocks from every time the chip keeps.
    pub fn rebase(&mut self, by: u64) {
        self.target -= by.min(self.target);
        self.timeout = self.timeout.saturating_sub(by);
    }
}
