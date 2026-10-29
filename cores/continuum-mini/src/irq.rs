//! The interrupt controller. Its sources sit in groups, each group under two bits of a priority
//! register (zero keeps the group out), and every source has an enable bit and an active bit.
//! The registers live in the I/O block, $2020-$202A: priorities $20-$22, enables $23-$26, the
//! active bits $27-$2A (the processor clears one by writing a 1 to it).
//!
//! As PokeMini models it (the emulator cMini replaces): whenever something may have changed, the
//! controller looks through its sources in a fixed order, group by group, and accepts the first
//! one both enabled and active, as long as the processor lets interrupts in (F's level below 3
//! and no change of U pending, as last told through `check`). One accepted interrupt waits for
//! the processor; anything that comes up meanwhile waits its turn, still active.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Irq {
    /// The processor lets interrupts in, as it last said.
    pub master: bool,
    /// The vector address of the interrupt accepted and not yet taken.
    pub accepted: Option<u8>,
}

impl Default for Irq {
    fn default() -> Self {
        Self { master: true, accepted: None }
    }
}

/// Where a source's active bit is: its register in the I/O block and the bit. Its enable bit is
/// the same bit four registers lower.
fn source(n: u8) -> Option<(usize, u8)> {
    Some(match n {
        0x03..=0x0A => (0x27, 0x80 >> (n - 0x03)),
        0x0B..=0x0E => (0x28, 0x20 >> (n - 0x0B)),
        0x0F => (0x2A, 0x80),
        0x10 => (0x2A, 0x40),
        0x13 => (0x28, 0x02),
        0x14 => (0x28, 0x01),
        0x15..=0x1C => (0x29, 0x80 >> (n - 0x15)),
        0x1D..=0x1F => (0x2A, 0x04 >> (n - 0x1D)),
        _ => return None,
    })
}

/// The order the controller looks through its sources in: (priority register, its bits, first
/// source, last source). Not sorted by priority value; this is the controller's own order.
const ORDER: [(usize, u8, u8, u8); 9] = [
    (0x20, 0xC0, 0x03, 0x04),
    (0x20, 0x30, 0x05, 0x06),
    (0x20, 0x0C, 0x07, 0x08),
    (0x20, 0x03, 0x09, 0x0A),
    (0x21, 0xC0, 0x0B, 0x0E),
    (0x22, 0x03, 0x0F, 0x10),
    (0x21, 0x30, 0x13, 0x14),
    (0x21, 0x0C, 0x15, 0x1C),
    (0x21, 0x03, 0x1D, 0x1F),
];

impl Irq {
    /// The reset line: interrupts let in until the processor says otherwise, nothing active.
    pub fn reset(&mut self, io: &mut [u8]) {
        self.master = true;
        io[0x27..=0x2A].fill(0);
    }

    /// A source comes up (`n` is its number, the vector being at `n * 2`).
    pub fn raise(&mut self, io: &mut [u8], n: u8) {
        if let Some((reg, bit)) = source(n) {
            io[reg] |= bit;
        }
        self.process(io);
    }

    /// The processor's word on whether it lets interrupts in: F (its level bits) and how many
    /// instructions a change of U still has to wait.
    pub fn check(&mut self, io: &[u8], f: u8, u_hold: u8) {
        self.master = u_hold == 0 && f & 0xC0 != 0xC0;
        self.process(io);
    }

    /// Looks for an interrupt to accept.
    pub fn process(&mut self, io: &[u8]) {
        if !self.master {
            return;
        }
        for &(pri, bits, first, last) in &ORDER {
            if io[pri] & bits == 0 {
                continue;
            }
            for n in first..=last {
                let (act, bit) = source(n).expect("listed sources exist");
                if io[act - 4] & bit != 0 && io[act] & bit != 0 {
                    if self.accepted.is_none() {
                        self.accepted = Some(n << 1);
                    }
                    return;
                }
            }
        }
    }

    pub fn read(&self, io: &[u8], reg: usize) -> u8 {
        match reg {
            0x22 => io[reg] & 0x03,
            0x24 | 0x28 => io[reg] & 0x3F,
            0x26 | 0x2A => io[reg] & 0xF7,
            _ => io[reg],
        }
    }

    /// A write: `cpu` is the processor's (which clears active bits) as against the picture
    /// chip's or a debugger's (which store them).
    pub fn write(&mut self, io: &mut [u8], reg: usize, value: u8, cpu: bool) {
        match reg {
            0x20..=0x26 => {
                io[reg] = match reg {
                    0x24 => value & 0x3F,
                    0x26 => value & 0xF7,
                    _ => value,
                };
                self.process(io);
            }
            0x27..=0x2A => {
                let mask = match reg {
                    0x28 => 0x3F,
                    0x2A => 0xF7,
                    _ => 0xFF,
                };
                if cpu {
                    io[reg] &= !value;
                } else {
                    io[reg] = value & mask;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_source_in_the_controller_s_order_wins_and_waits() {
        let mut io = [0u8; 256];
        let mut irq = Irq::default();
        // Timer 1's lower half (08) and the PRC's copy (03), both enabled, priorities set.
        io[0x20] = 0xFF;
        io[0x23] = 0xFF;
        irq.raise(&mut io, 0x08);
        assert_eq!(irq.accepted, Some(0x10));
        // The copy comes up while 08 waits: 08 stays the one accepted.
        irq.raise(&mut io, 0x03);
        assert_eq!(irq.accepted, Some(0x10));
        assert_eq!(io[0x27], 0x84);
        // Taken; the next look finds the copy first.
        irq.accepted = None;
        irq.process(&io);
        assert_eq!(irq.accepted, Some(0x06));
    }

    #[test]
    fn nothing_comes_in_at_level_3_or_while_u_waits() {
        let mut io = [0u8; 256];
        let mut irq = Irq::default();
        io[0x21] = 0x0C;
        io[0x25] = 0x01;
        irq.check(&io, 0xC0, 0);
        irq.raise(&mut io, 0x1C);
        assert_eq!(irq.accepted, None);
        irq.check(&io, 0x40, 1);
        assert_eq!(irq.accepted, None);
        irq.check(&io, 0x40, 0);
        assert_eq!(irq.accepted, Some(0x38));
    }

    #[test]
    fn the_processor_clears_active_bits_the_picture_chip_stores_them() {
        let mut io = [0u8; 256];
        let mut irq = Irq::default();
        io[0x28] = 0x3F;
        irq.write(&mut io, 0x28, 0x21, true);
        assert_eq!(io[0x28], 0x1E);
        irq.write(&mut io, 0x28, 0xFF, false);
        assert_eq!(io[0x28], 0x3F);
    }
}
