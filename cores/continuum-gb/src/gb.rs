//! The machine: CPU plus bus, frame stepping and save states.

use serde::{Deserialize, Serialize};

use crate::bus::Bus;
use crate::cart::Cart;
use crate::cpu::Cpu;

/// T-cycles in one frame (154 lines of 456 dots).
pub const FRAME_CYCLES: u64 = 70_224;

/// The DMG shades as XRGB8888, lightest first.
pub const SHADES: [u32; 4] = [0x00FF_FFFF, 0x00AA_AAAA, 0x0055_5555, 0x0000_0000];

#[derive(Clone, Serialize, Deserialize)]
pub struct GameBoy {
    pub cpu: Cpu,
    pub bus: Bus,
}

const STATE_MAGIC: &[u8; 8] = b"CGBSTAT1";

impl GameBoy {
    pub fn new(rom: Vec<u8>) -> Result<GameBoy, String> {
        let cart = Cart::new(rom)?;
        Ok(GameBoy { cpu: Cpu::default(), bus: Bus::new(cart) })
    }

    /// Runs until the PPU finishes a frame (or one frame's worth of cycles with the LCD off).
    pub fn run_frame(&mut self) {
        let start = self.bus.cycles;
        self.bus.ppu.frame_ready = false;
        while !self.bus.ppu.frame_ready && self.bus.cycles - start < FRAME_CYCLES * 2 {
            self.cpu.step(&mut self.bus);
            if !self.bus.ppu.on() && self.bus.cycles - start >= FRAME_CYCLES {
                break;
            }
        }
    }

    pub fn frame_xrgb(&self, out: &mut [u32]) {
        for (o, s) in out.iter_mut().zip(self.bus.ppu.frame.iter()) {
            *o = SHADES[*s as usize & 3];
        }
    }

    pub fn save_state(&self) -> Vec<u8> {
        let mut out = STATE_MAGIC.to_vec();
        out.extend(bincode::serialize(self).unwrap_or_default());
        out
    }

    pub fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        if data.len() < 8 || &data[..8] != STATE_MAGIC {
            return Err("not a Continuum GB state".into());
        }
        let mut next: GameBoy = bincode::deserialize(&data[8..]).map_err(|e| e.to_string())?;
        next.bus.cart.rom = std::mem::take(&mut self.bus.cart.rom);
        next.bus.ppu.frame = std::mem::take(&mut self.bus.ppu.frame);
        *self = next;
        Ok(())
    }
}
