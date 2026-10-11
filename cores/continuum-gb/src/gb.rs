//! The machine: CPU plus bus, frame stepping and save states.

use serde::{Deserialize, Serialize};

use crate::bus::Bus;
use crate::cart::Cart;
use crate::cpu::Cpu;

/// T-cycles in one frame (154 lines of 456 dots).
pub const FRAME_CYCLES: u64 = 70_224;


#[derive(Clone, Serialize, Deserialize)]
pub struct GameBoy {
    pub cpu: Cpu,
    pub bus: Bus,
}

const STATE_MAGIC: &[u8; 8] = b"CGBSTAT1";

impl GameBoy {
    pub fn new(rom: Vec<u8>) -> Result<GameBoy, String> {
        let cart = Cart::new(rom)?;
        // A colour cartridge (header $143 bit 7: CGB-compatible or CGB-only) runs as a CGB;
        // everything else as a DMG.
        let cgb = cart.cgb_flag & 0x80 != 0;
        Ok(Self::build(cart, cgb))
    }

    /// The machine in a chosen mode (the tests run CGB-hardware tests this way).
    pub fn new_mode(rom: Vec<u8>, cgb: bool) -> Result<GameBoy, String> {
        Ok(Self::build(Cart::new(rom)?, cgb))
    }

    fn build(cart: Cart, cgb: bool) -> GameBoy {
        let mut bus = Bus::new(cart);
        let cpu = if cgb {
            bus.cgb = true;
            bus.ppu.cgb = true;
            // The CGB boot ROM leaves every background palette white; objects too, here.
            bus.ppu.bg_pal = vec![0xFF, 0x7F].repeat(32);
            bus.ppu.obj_pal = vec![0xFF, 0x7F].repeat(32);
            Cpu::cgb()
        } else {
            Cpu::default()
        };
        GameBoy { cpu, bus }
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
        out.copy_from_slice(&self.bus.ppu.frame);
    }

    pub fn save_state(&self) -> Vec<u8> {
        let mut out = STATE_MAGIC.to_vec();
        out.extend(bincode::serialize(self).unwrap_or_default());
        out
    }

    pub fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        if data.len() < 8 || &data[..8] != STATE_MAGIC {
            return Err("not a cBoy Game Boy state".into());
        }
        let mut next: GameBoy = bincode::deserialize(&data[8..]).map_err(|e| e.to_string())?;
        next.bus.cart.rom = std::mem::take(&mut self.bus.cart.rom);
        next.bus.ppu.frame = std::mem::take(&mut self.bus.ppu.frame);
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cgb_machine() -> GameBoy {
        let mut rom = vec![0u8; 0x8000];
        rom[0x143] = 0xC0; // CGB only
        // At $100: LD A,1 / LDH ($4D),A / STOP / JR -2
        rom[0x100..0x107].copy_from_slice(&[0x3E, 0x01, 0xE0, 0x4D, 0x10, 0x00, 0x18]);
        rom[0x107] = 0xFE;
        GameBoy::new(rom).unwrap()
    }

    #[test]
    fn cgb_registers_and_mode() {
        let gb = cgb_machine();
        assert!(gb.bus.cgb && gb.bus.ppu.cgb);
        assert_eq!((gb.cpu.a, gb.cpu.f, gb.cpu.d, gb.cpu.e, gb.cpu.l), (0x11, 0x80, 0xFF, 0x56, 0x0D));
    }

    #[test]
    fn key1_and_stop_switch_speed() {
        let mut gb = cgb_machine();
        for _ in 0..4 {
            gb.cpu.step(&mut gb.bus);
        }
        assert!(gb.bus.double_speed);
        assert_eq!(gb.bus.raw_read(0xFF4D), 0xFE);
        // At double speed a frame takes twice the CPU M-cycles.
        let before = gb.bus.cycles;
        gb.run_frame();
        gb.run_frame();
        let per_frame = gb.bus.cycles - before;
        assert!(per_frame > FRAME_CYCLES * 3, "{per_frame}");
    }

    #[test]
    fn wram_and_vram_banks() {
        let mut gb = cgb_machine();
        let b = &mut gb.bus;
        for bank in 1..8u8 {
            b.raw_write(0xFF70, bank);
            b.raw_write(0xD000, bank * 11);
        }
        b.raw_write(0xFF70, 0); // 0 selects 1
        assert_eq!(b.raw_read(0xD000), 11);
        b.raw_write(0xFF70, 5);
        assert_eq!(b.raw_read(0xD000), 55);
        b.raw_write(0xFF4F, 1);
        b.raw_write(0x8000, 0xAB);
        b.raw_write(0xFF4F, 0);
        assert_ne!(b.raw_read(0x8000), 0xAB);
        assert_eq!(b.ppu.vram[0x2000], 0xAB);
    }

    #[test]
    fn palettes_auto_increment() {
        let mut gb = cgb_machine();
        let b = &mut gb.bus;
        b.raw_write(0xFF68, 0x80 | 0x3E);
        b.raw_write(0xFF69, 0x12);
        b.raw_write(0xFF69, 0x34); // wraps to index 0
        assert_eq!(b.ppu.bg_pal[0x3E], 0x12);
        assert_eq!(b.ppu.bg_pal[0x3F], 0x34);
        assert_eq!(b.raw_read(0xFF68), 0x80 | 0x40);
    }

    #[test]
    fn general_and_hblank_dma() {
        let mut gb = cgb_machine();
        let b = &mut gb.bus;
        for i in 0..0x40u16 {
            b.raw_write(0xC000 + i, i as u8 + 1);
        }
        b.raw_write(0xFF51, 0xC0);
        b.raw_write(0xFF52, 0x00);
        b.raw_write(0xFF53, 0x01);
        b.raw_write(0xFF54, 0x00);
        let t0 = b.cycles;
        b.raw_write(0xFF55, 0x01); // GDMA, 2 blocks
        assert_eq!(b.cycles - t0, 16 * 4);
        assert_eq!(b.ppu.vram[0x100], 1);
        assert_eq!(b.ppu.vram[0x11F], 0x20);
        assert_eq!(b.raw_read(0xFF55), 0xFF);
        // HDMA: one block per HBlank.
        b.raw_write(0xFF53, 0x02);
        b.raw_write(0xFF54, 0x00);
        b.raw_write(0xFF55, 0x81); // 2 blocks
        assert_eq!(b.raw_read(0xFF55), 0x01);
        for _ in 0..(456 * 3 / 4) {
            b.tick();
        }
        assert_eq!(b.raw_read(0xFF55), 0xFF);
        assert_eq!(b.ppu.vram[0x200], 0x21);
        assert_eq!(b.ppu.vram[0x21F], 0x40);
    }
}
