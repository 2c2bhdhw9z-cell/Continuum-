//! The TIA as the 7800 uses it: its two sound channels, and its input ports (the joystick
//! buttons). The 7800 never shows the TIA's own picture; MARIA draws everything.
//!
//! The sound is the TIA's own circuit, clocked at the colour clock / 114 (31.4 kHz on NTSC):
//! each channel divides that clock by AUDF+1 (and by 3 more in the AUDC modes that say so), then
//! steps the source AUDC picks: a pure tone, the 4-bit, 5-bit or 9-bit polynomial counters, the
//! divide-by-31 pattern, or a combination. Its output is AUDV while its bit is high. This follows
//! the TIA's published behaviour as Ron Fries' measurements describe it, which is what MAME 0.289
//! plays, so a sample made here is the sample MAME makes (tests/tia_mame.rs).

use serde::{Deserialize, Serialize};

const POLY4: usize = 15;
const POLY5: usize = 31;
const POLY9: usize = 511;

/// The divide-by-31 pattern: high for 13 of its 31 steps' worth of edges (two edges a cycle).
const DIV31: [u8; POLY5] = {
    let mut t = [0u8; POLY5];
    t[1] = 1;
    t[19] = 1;
    t
};

/// A polynomial counter's output sequence, from all bits set, one bit per step.
const fn poly<const N: usize>(size: u32, f0: u32, f1: u32) -> [u8; N] {
    let mut out = [0u8; N];
    let mut x: u32 = (1 << size) - 1;
    let mut i = 0;
    while i < N {
        let b0 = (x >> (size - f0)) & 1;
        let b1 = (x >> (size - f1)) & 1;
        out[i] = (x & 1) as u8;
        x = (x >> 1) | ((b0 ^ b1) << (size - 1));
        i += 1;
    }
    out
}

static BIT4: [u8; POLY4] = poly::<POLY4>(4, 4, 3);
static BIT5: [u8; POLY5] = poly::<POLY5>(5, 5, 3);
static BIT9: [u8; POLY9] = poly::<POLY9>(9, 9, 5);

#[derive(Clone, Default, Serialize, Deserialize)]
struct Channel {
    audc: u8,
    audf: u8,
    audv: u8,
    out: u8,
    p4: u8,
    p5: u8,
    p9: u16,
    div_cnt: u16,
    div_max: u16,
    div3_cnt: u8,
}

impl Channel {
    fn new() -> Self {
        Channel { div3_cnt: 3, ..Default::default() }
    }

    /// What a write to AUDC, AUDF or AUDV changes: the divider's period, and for the two
    /// "always on" modes the output itself.
    fn changed(&mut self) {
        let max = if self.audc == 0x00 || self.audc == 0x0B {
            self.out = self.audv;
            0
        } else {
            let mut n = self.audf as u16 + 1;
            if self.audc & 0x0C == 0x0C && self.audc != 0x0F {
                n *= 3;
            }
            n
        };
        if max != self.div_max {
            self.div_max = max;
            // A channel that was or becomes "always on" restarts its divider; otherwise the
            // current period runs out first.
            if self.div_cnt == 0 || max == 0 {
                self.div_cnt = max;
            }
        }
    }

    /// One TIA sound clock.
    fn clock(&mut self) {
        if self.div_cnt > 1 {
            self.div_cnt -= 1;
            return;
        }
        if self.div_cnt == 0 {
            return;
        }
        let prev5 = BIT5[self.p5 as usize];
        self.div_cnt = self.div_max;
        self.p5 = ((self.p5 as usize + 1) % POLY5) as u8;
        let b5 = BIT5[self.p5 as usize];
        let c = self.audc;
        let tick = c & 0x02 == 0
            || (c & 0x01 == 0 && DIV31[self.p5 as usize] != 0)
            || (c & 0x01 == 1 && b5 != 0)
            || (c == 0x0F && b5 != prev5);
        if !tick {
            return;
        }
        if c & 0x04 != 0 {
            if c == 0x0F {
                if b5 != prev5 {
                    self.div3_cnt -= 1;
                    if self.div3_cnt == 0 {
                        self.div3_cnt = 3;
                        self.toggle();
                    }
                }
            } else {
                self.toggle();
            }
        } else if c & 0x08 != 0 {
            if c == 0x08 {
                self.p9 = ((self.p9 as usize + 1) % POLY9) as u16;
                self.out = if BIT9[self.p9 as usize] != 0 { self.audv } else { 0 };
            } else if c & 0x02 != 0 {
                self.out = if self.out != 0 || c & 0x01 != 0 { 0 } else { self.audv };
            } else {
                self.out = if b5 != 0 { self.audv } else { 0 };
            }
        } else {
            self.p4 = ((self.p4 as usize + 1) % POLY4) as u8;
            self.out = if BIT4[self.p4 as usize] != 0 { self.audv } else { 0 };
        }
    }

    fn toggle(&mut self) {
        self.out = if self.out != 0 { 0 } else { self.audv };
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TiaSound {
    ch: [Channel; 2],
}

impl Default for TiaSound {
    fn default() -> Self {
        TiaSound { ch: [Channel::new(), Channel::new()] }
    }
}

impl TiaSound {
    /// A write to one of the TIA's registers (offset 0..$1F); only the six sound ones matter.
    pub fn write(&mut self, offset: u8, v: u8) {
        let (c, what) = match offset & 0x1F {
            0x15 => (0, 0),
            0x16 => (1, 0),
            0x17 => (0, 1),
            0x18 => (1, 1),
            0x19 => (0, 2),
            0x1A => (1, 2),
            _ => return,
        };
        let ch = &mut self.ch[c];
        match what {
            0 => ch.audc = v & 0x0F,
            1 => ch.audf = v & 0x1F,
            _ => ch.audv = v & 0x0F,
        }
        ch.changed();
    }

    /// One sound clock; returns the two channels' levels added (0 to 30).
    pub fn clock(&mut self) -> u8 {
        self.ch[0].clock();
        self.ch[1].clock();
        self.ch[0].out + self.ch[1].out
    }

    /// The level each channel shows now (for the per-channel volume settings).
    pub fn levels(&self) -> (u8, u8) {
        (self.ch[0].out, self.ch[1].out)
    }
}
