//! POKEY, Atari's sound and I/O chip, as some 7800 cartridges carry it (at $4000 on the early
//! POKEY boards, at $0450 on later ones): four sound channels, the random-number register, and
//! the timer interrupts. The 7800 has no POKEY keyboard, paddles or serial port on a cartridge.
//!
//! It runs on the processor's 1.79 MHz clock, one step per clock, the way Atari's POKEY data
//! sheet and the C012294 chip's measured behaviour describe it (as MAME 0.289 models it): the
//! 4, 5, 9 and 17-bit polynomial counters step every clock; each channel counts up from its
//! AUDF's complement on its clock (1.79 MHz, 64 kHz or 15 kHz) and, a few clocks after it wraps
//! (its "borrow"), steps its output by the source AUDC picks; channels 1+2 and 3+4 can join into
//! 16-bit counters, channels 3 and 4 can act as high-pass filters on 1 and 2, and SKCTL's
//! two-tone mode ties channels 1 and 2 together.
//!
//! Its interrupt line reaches the processor only on the cartridges whose header says it is
//! wired (the community's version 3 and 4 headers have the bits); otherwise, as on MAME and
//! ProSystem, a program can still poll IRQST.

use serde::{Deserialize, Serialize};

const NOTPOLY5: u8 = 0x80;
const POLY4_SEL: u8 = 0x40;
const PURE: u8 = 0x20;
const VOLUME_ONLY: u8 = 0x10;

const POLY9: u8 = 0x80;
const CH1_HICLK: u8 = 0x40;
const CH3_HICLK: u8 = 0x20;
const CH12_JOINED: u8 = 0x10;
const CH34_JOINED: u8 = 0x08;
const CH1_FILTER: u8 = 0x04;
const CH2_FILTER: u8 = 0x02;
const CLK_15KHZ: u8 = 0x01;

const DIV_64: u8 = 28;
const DIV_15: u8 = 114;

const IRQ_TIMR1: u8 = 0x01;
const IRQ_TIMR2: u8 = 0x02;
const IRQ_TIMR4: u8 = 0x04;
const IRQ_SEROC: u8 = 0x08;

const SK_RESET: u8 = 0x03;
const SK_TWOTONE: u8 = 0x08;
const SK_ASYNC: u8 = 0x10;
const SK_BREAK: u8 = 0x80;

/// The four polynomial sequences, made once.
struct Polys {
    p4: [u8; 15],
    p5: [u8; 31],
    p9: Vec<u16>,
    p17: Vec<u32>,
}

fn polys() -> &'static Polys {
    use std::sync::OnceLock;
    static P: OnceLock<Polys> = OnceLock::new();
    P.get_or_init(|| {
        // The 4 and 5-bit counters: shift left, feeding in the complement of two taps' XOR.
        fn small<const N: usize>(size: u32) -> [u8; N] {
            let mask = (1u32 << size) - 1;
            let mut lfsr = 0u32;
            let mut out = [0u8; N];
            for o in out.iter_mut() {
                lfsr = (lfsr << 1) | (!((lfsr >> 2) ^ (lfsr >> (size - 1))) & 1);
                *o = (lfsr & mask & 1) as u8;
            }
            out
        }
        let mut p9 = Vec::with_capacity(511);
        let mut l: u32 = 0x1FF;
        for _ in 0..511 {
            let inb = (l & 1) ^ ((l >> 5) & 1);
            l = (l >> 1) | (inb << 8);
            p9.push(l as u16);
        }
        let mut p17 = Vec::with_capacity(0x1FFFF);
        let mut l: u32 = 0x1FFFF;
        for _ in 0..0x1FFFF {
            let in8 = ((l >> 8) & 1) ^ ((l >> 13) & 1);
            let inb = l & 1;
            l >>= 1;
            l = (l & 0xFF7F) | (in8 << 7);
            l |= inb << 16;
            p17.push(l);
        }
        Polys { p4: small::<15>(4), p5: small::<31>(5), p9, p17 }
    })
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Channel {
    audf: u8,
    audc: u8,
    borrow: u8,
    counter: u8,
    output: u8,
    filter: u8,
    int_mask: u8,
}

impl Channel {
    fn reset(&mut self) {
        self.counter = self.audf ^ 0xFF;
        self.borrow = 0;
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Pokey {
    ch: [Channel; 4],
    audctl: u8,
    skctl: u8,
    irqst: u8,
    irqen: u8,
    p4: u8,
    p5: u8,
    p9: u16,
    p17: u32,
    cnt28: u8,
    cnt114: u8,
    /// Each channel's volume while its output is on, packed four bits a channel.
    raw: u16,
}

impl Default for Pokey {
    fn default() -> Self {
        let mut ch: [Channel; 4] = Default::default();
        for c in ch.iter_mut() {
            c.audc = 0xB0;
        }
        ch[0].int_mask = IRQ_TIMR1;
        ch[1].int_mask = IRQ_TIMR2;
        ch[3].int_mask = IRQ_TIMR4;
        Pokey {
            ch,
            audctl: 0,
            skctl: 0,
            irqst: IRQ_SEROC,
            irqen: 0,
            p4: 0,
            p5: 0,
            p9: 0,
            p17: 0,
            cnt28: 0,
            cnt114: 0,
            raw: 0,
        }
    }
}

impl Pokey {
    fn running(&self) -> bool {
        self.skctl & SK_RESET != 0
    }

    /// A channel's counter steps; when it wraps, its borrow happens `delay` clocks later.
    fn inc(&mut self, c: usize, delay: u8) {
        let ch = &mut self.ch[c];
        ch.counter = ch.counter.wrapping_add(1);
        if ch.counter == 0 && ch.borrow == 0 {
            ch.borrow = delay;
            if self.irqen & ch.int_mask != 0 {
                self.irqst |= ch.int_mask;
            }
        }
    }

    fn borrowed(&mut self, c: usize) -> bool {
        let ch = &mut self.ch[c];
        if ch.borrow > 0 {
            ch.borrow -= 1;
            return ch.borrow == 0;
        }
        false
    }

    /// A channel's output steps by the source its AUDC picks.
    fn process(&mut self, c: usize) {
        let p = polys();
        let audc = self.ch[c].audc;
        if audc & NOTPOLY5 != 0 || p.p5[self.p5 as usize] & 1 != 0 {
            let out = &mut self.ch[c].output;
            if audc & PURE != 0 {
                *out ^= 1;
            } else if audc & POLY4_SEL != 0 {
                *out = p.p4[self.p4 as usize] & 1;
            } else if self.audctl & POLY9 != 0 {
                *out = (p.p9[self.p9 as usize] & 1) as u8;
            } else {
                *out = (p.p17[self.p17 as usize] & 1) as u8;
            }
        }
    }

    /// One 1.79 MHz clock. Returns the output level (0 to 60, the four channels added).
    pub fn clock(&mut self) -> u8 {
        if self.running() {
            self.p4 = (self.p4 + 1) % 15;
            self.p5 = (self.p5 + 1) % 31;
            self.p9 = (self.p9 + 1) % 511;
            self.p17 = (self.p17 + 1) % 0x1FFFF;
            self.cnt28 += 1;
            let t28 = self.cnt28 >= DIV_64;
            if t28 {
                self.cnt28 = 0;
            }
            self.cnt114 += 1;
            let t114 = self.cnt114 >= DIV_15;
            if t114 {
                self.cnt114 = 0;
            }
            let base = if self.audctl & CLK_15KHZ != 0 { t114 } else { t28 };
            if self.audctl & CH1_HICLK != 0 {
                self.inc(0, if self.audctl & CH12_JOINED != 0 { 7 } else { 4 });
            } else if base {
                self.inc(0, 1);
            }
            // In asynchronous serial mode channels 3 and 4 wait for a start bit, and nothing is
            // plugged into a cartridge POKEY's serial input, so they stand still.
            let wait34 = self.skctl & SK_ASYNC != 0;
            if !wait34 {
                if self.audctl & CH3_HICLK != 0 {
                    self.inc(2, if self.audctl & CH34_JOINED != 0 { 7 } else { 4 });
                } else if base {
                    self.inc(2, 1);
                }
            }
            if base {
                if self.audctl & CH12_JOINED == 0 {
                    self.inc(1, 1);
                }
                if self.audctl & CH34_JOINED == 0 && !wait34 {
                    self.inc(3, 1);
                }
            }
        }
        if self.borrowed(2) {
            if self.audctl & CH34_JOINED != 0 {
                self.inc(3, 1);
            } else {
                self.ch[2].reset();
            }
            self.process(2);
            if self.audctl & CH1_FILTER != 0 {
                self.ch[0].filter = self.ch[0].output;
            } else {
                self.ch[0].filter = 1;
            }
        }
        if self.borrowed(3) {
            if self.audctl & CH34_JOINED != 0 {
                self.ch[2].reset();
            }
            self.ch[3].reset();
            self.process(3);
            if self.audctl & CH2_FILTER != 0 {
                self.ch[1].filter = self.ch[1].output;
            } else {
                self.ch[1].filter = 1;
            }
        }
        // Two-tone mode (the cassette's tones; music players use it as a filter): channel 2's
        // borrow restarts channel 1, and with the serial output idle and no break, channel 1's
        // restarts channel 2.
        let two_tone = self.skctl & SK_TWOTONE != 0;
        if two_tone && self.ch[1].borrow == 1 {
            self.ch[0].reset();
        }
        if self.borrowed(0) {
            if self.audctl & CH12_JOINED != 0 {
                self.inc(1, 1);
            } else {
                self.ch[0].reset();
                if two_tone && self.skctl & SK_BREAK == 0 {
                    self.ch[1].reset();
                }
            }
            self.process(0);
        }
        if self.borrowed(1) {
            if self.audctl & CH12_JOINED != 0 {
                self.ch[0].reset();
            }
            self.ch[1].reset();
            self.process(1);
        }
        let mut sum = 0u8;
        let mut raw = 0u16;
        for (i, ch) in self.ch.iter().enumerate() {
            if (ch.output ^ ch.filter) != 0 || ch.audc & VOLUME_ONLY != 0 {
                sum += ch.audc & 0x0F;
                raw |= ((ch.audc & 0x0F) as u16) << (4 * i);
            }
        }
        self.raw = raw;
        sum
    }

    /// Each channel's volume while on, for the per-channel volume settings.
    pub fn channel_levels(&self) -> [u8; 4] {
        [(self.raw & 15) as u8, ((self.raw >> 4) & 15) as u8, ((self.raw >> 8) & 15) as u8, ((self.raw >> 12) & 15) as u8]
    }

    pub fn read(&mut self, offset: u8) -> u8 {
        let p = polys();
        match offset & 0x0F {
            0x00..=0x07 => 0xE4, // POT0-7: nothing connected, the count runs to its end
            0x08 => 0x00,         // ALLPOT
            0x09 => 0x09,         // KBCODE: no key
            0x0A => {
                if self.audctl & POLY9 != 0 {
                    (p.p9[self.p9 as usize] & 0xFF) as u8
                } else {
                    ((p.p17[self.p17 as usize] >> 8) & 0xFF) as u8
                }
            }
            0x0D => 0x00,               // SERIN
            0x0E => self.irqst ^ 0xFF,  // IRQST, active low
            0x0F => 0xFF,               // SKSTAT: nothing pending, active low
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, offset: u8, v: u8) {
        match offset & 0x0F {
            0x00 => self.ch[0].audf = v,
            0x01 => self.ch[0].audc = v,
            0x02 => self.ch[1].audf = v,
            0x03 => self.ch[1].audc = v,
            0x04 => self.ch[2].audf = v,
            0x05 => self.ch[2].audc = v,
            0x06 => self.ch[3].audf = v,
            0x07 => self.ch[3].audc = v,
            0x08 => self.audctl = v,
            0x09 => {
                // STIMER: every counter starts its period again, every output low.
                for (i, ch) in self.ch.iter_mut().enumerate() {
                    ch.reset();
                    ch.output = 0;
                    ch.filter = if i < 2 { 1 } else { 0 };
                }
            }
            0x0E => {
                // IRQEN: masking a source also clears its flag.
                self.irqst &= IRQ_SEROC | v;
                self.irqen = v;
            }
            0x0F => {
                if v == self.skctl {
                    return;
                }
                self.skctl = v;
                if v & SK_RESET == 0 {
                    self.irqen = 0;
                    self.irqst &= IRQ_SEROC;
                    self.p4 = 0;
                    self.p5 = 0;
                    self.p9 = 0;
                    self.p17 = 0;
                    self.cnt28 = 0;
                    self.cnt114 = 0;
                }
                if v & SK_ASYNC != 0 {
                    self.ch[2].reset();
                    self.ch[3].reset();
                }
            }
            _ => {}
        }
    }

    /// The chip's IRQ output: an enabled interrupt is pending. Only a cartridge whose header says
    /// so connects it to the processor.
    pub fn irq(&self) -> bool {
        self.irqst & self.irqen != 0
    }
}
