//! The Intel 8255 PPI (programmable peripheral interface), as Konami's Scramble-family boards use
//! two of them: one reads the controls and DIP switches, the other sends the sound board its
//! command and its interrupt and reads (or, on Scramble, talks to the protection chip through)
//! port C.
//!
//! Mode 0, the only one these boards use: ports A and B and the two halves of port C are each an
//! input or an output, as the control word says. Writing a control word with bit 7 set chooses
//! the directions and clears every output latch (an input port's pins float high, so whatever
//! listens to it sees FFh); with bit 7 clear it sets or clears one bit of port C. Reading an
//! output port gives its latch. Reset is the control word 9Bh: everything an input. Modes 1 and 2
//! (the handshaking modes) are taken as mode 0. Written from Intel's data sheet; the behaviour at
//! a mode change follows MAME's i8255 (an output that's switched to input reports its pins
//! floating high), which the boards' tests compare against.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct I8255 {
    control: u8,
    latch: [u8; 3],
}

/// What a write did to the pins: the new value on each port that changed (or was driven again),
/// as the outside world sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outputs {
    pub a: Option<u8>,
    pub b: Option<u8>,
    pub c: Option<u8>,
}

impl Default for I8255 {
    fn default() -> Self {
        let mut p = I8255 { control: 0x9B, latch: [0; 3] };
        p.reset();
        p
    }
}

impl I8255 {
    pub fn new() -> I8255 {
        I8255::default()
    }

    fn a_input(&self) -> bool {
        self.control & 0x10 != 0
    }
    fn b_input(&self) -> bool {
        self.control & 0x02 != 0
    }
    fn c_upper_input(&self) -> bool {
        self.control & 0x08 != 0
    }
    fn c_lower_input(&self) -> bool {
        self.control & 0x01 != 0
    }

    /// Port C's pins as the outside sees them: the latch on the output halves, high on the
    /// floating input halves.
    fn c_pins(&self) -> u8 {
        let mut v = 0;
        v |= if self.c_upper_input() { 0xF0 } else { self.latch[2] & 0xF0 };
        v |= if self.c_lower_input() { 0x0F } else { self.latch[2] & 0x0F };
        v
    }

    /// /RESET: control word 9Bh.
    pub fn reset(&mut self) -> Outputs {
        self.set_mode(0x9B)
    }

    fn set_mode(&mut self, control: u8) -> Outputs {
        self.control = control;
        self.latch = [0; 3];
        Outputs {
            a: Some(if self.a_input() { 0xFF } else { 0 }),
            b: Some(if self.b_input() { 0xFF } else { 0 }),
            c: Some(self.c_pins()),
        }
    }

    /// A read of register `reg` (0-3: A, B, C, the control word), with the input pins as given.
    pub fn read(&self, reg: u8, pins: [u8; 3]) -> u8 {
        match reg & 3 {
            0 => if self.a_input() { pins[0] } else { self.latch[0] },
            1 => if self.b_input() { pins[1] } else { self.latch[1] },
            2 => {
                let upper = if self.c_upper_input() { pins[2] } else { self.latch[2] } & 0xF0;
                let lower = if self.c_lower_input() { pins[2] } else { self.latch[2] } & 0x0F;
                upper | lower
            }
            _ => self.control,
        }
    }

    /// A write to register `reg`.
    pub fn write(&mut self, reg: u8, data: u8) -> Outputs {
        match reg & 3 {
            0 => {
                if self.a_input() {
                    return Outputs::default();
                }
                self.latch[0] = data;
                Outputs { a: Some(data), ..Outputs::default() }
            }
            1 => {
                if self.b_input() {
                    return Outputs::default();
                }
                self.latch[1] = data;
                Outputs { b: Some(data), ..Outputs::default() }
            }
            2 => {
                self.latch[2] = data;
                Outputs { c: Some(self.c_pins()), ..Outputs::default() }
            }
            _ => {
                if data & 0x80 != 0 {
                    return self.set_mode(data);
                }
                let bit = (data >> 1) & 7;
                self.latch[2] = (self.latch[2] & !(1 << bit)) | (data & 1) << bit;
                Outputs { c: Some(self.c_pins()), ..Outputs::default() }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_makes_everything_an_input() {
        let mut p = I8255::new();
        assert_eq!(p.read(3, [0; 3]), 0x9B);
        assert_eq!(p.read(0, [0x12, 0x34, 0x56]), 0x12);
        assert_eq!(p.read(2, [0x12, 0x34, 0x56]), 0x56);
        // Writing an input port does nothing.
        assert_eq!(p.write(0, 0x55), Outputs::default());
        assert_eq!(p.reset(), Outputs { a: Some(0xFF), b: Some(0xFF), c: Some(0xFF) });
    }

    #[test]
    fn outputs_latch_and_read_back() {
        let mut p = I8255::new();
        // A and B out, C upper in, C lower out (Scramble's second PPI).
        assert_eq!(p.write(3, 0x88), Outputs { a: Some(0), b: Some(0), c: Some(0xF0) });
        assert_eq!(p.write(0, 0x5A), Outputs { a: Some(0x5A), ..Outputs::default() });
        assert_eq!(p.read(0, [0xFF; 3]), 0x5A);
        assert_eq!(p.write(2, 0x37).c, Some(0xF7));
        assert_eq!(p.read(2, [0xA0; 3]), 0xA7);
        // Bit set/reset on port C.
        assert_eq!(p.write(3, 0x01).c, Some(0xF7));
        assert_eq!(p.write(3, 0x04).c, Some(0xF3));
        // A new mode clears the latches.
        p.write(3, 0x80);
        assert_eq!(p.read(0, [0xFF; 3]), 0);
        assert_eq!(p.read(2, [0xFF; 3]), 0);
    }
}
