//! DIV and the TIMA timer, from the 16-bit system counter. TIMA counts on a FALLING edge of the
//! selected counter bit ANDed with the enable bit, so writes to DIV and TAC can tick it, as on
//! hardware. An overflow leaves TIMA at 0 for one M-cycle before TMA is loaded and the interrupt
//! is raised; a TIMA write in that cycle cancels the reload.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Timer {
    pub counter: u16,
    pub tima: u8,
    pub tma: u8,
    pub tac: u8,
    /// M-cycles until the pending reload (1 = reload at the end of the next M-cycle).
    reload_in: u8,
    /// True during the M-cycle in which TMA was just copied (writes to TIMA are ignored then).
    reloaded: bool,
}

impl Default for Timer {
    fn default() -> Self {
        Timer { counter: 0, tima: 0, tma: 0, tac: 0xF8, reload_in: 0, reloaded: false }
    }
}

impl Timer {
    fn bit(&self) -> bool {
        let sel = [9, 3, 5, 7][(self.tac & 3) as usize];
        self.tac & 4 != 0 && (self.counter >> sel) & 1 != 0
    }

    fn inc_tima(&mut self) {
        let (v, over) = self.tima.overflowing_add(1);
        self.tima = v;
        if over {
            self.reload_in = 1;
        }
    }

    /// One M-cycle. Returns true when the timer interrupt is requested.
    pub fn tick_m(&mut self) -> bool {
        self.reloaded = false;
        let mut irq = false;
        if self.reload_in > 0 {
            self.reload_in -= 1;
            if self.reload_in == 0 {
                self.tima = self.tma;
                self.reloaded = true;
                irq = true;
            }
        }
        let before = self.bit();
        self.counter = self.counter.wrapping_add(4);
        if before && !self.bit() {
            self.inc_tima();
        }
        irq
    }

    pub fn div(&self) -> u8 {
        (self.counter >> 8) as u8
    }

    pub fn write_div(&mut self) {
        let before = self.bit();
        self.counter = 0;
        if before {
            self.inc_tima();
        }
    }

    pub fn write_tima(&mut self, v: u8) {
        if self.reloaded {
            return;
        }
        self.tima = v;
        self.reload_in = 0;
    }

    pub fn write_tma(&mut self, v: u8) {
        self.tma = v;
        if self.reloaded {
            self.tima = v;
        }
    }

    pub fn write_tac(&mut self, v: u8) {
        // The write lands before this M-cycle's counter step, which `tick_m` has already taken
        // with the old TAC. Redo that step's edge with the new TAC (the timer circuit sees the
        // new enable and bit select for it): rapid start/stop (Mooneye rapid_toggle).
        let prev = self.counter.wrapping_sub(4);
        let bit_at = |tac: u8, c: u16| {
            let sel = [9, 3, 5, 7][(tac & 3) as usize];
            tac & 4 != 0 && (c >> sel) & 1 != 0
        };
        let old_edge = bit_at(self.tac, prev) && !bit_at(self.tac, self.counter);
        let new_tac = 0xF8 | (v & 7);
        // The write itself: the AND gate's output falling ticks TIMA.
        let write_edge = bit_at(self.tac, prev) && !bit_at(new_tac, prev);
        let new_edge = bit_at(new_tac, prev) && !bit_at(new_tac, self.counter);
        self.tac = new_tac;
        let ticks = i32::from(write_edge) + i32::from(new_edge) - i32::from(old_edge);
        if ticks < 0 && self.reload_in == 0 {
            self.tima = self.tima.wrapping_sub(1);
        }
        for _ in 0..ticks.max(0) {
            self.inc_tima();
        }
    }
}
