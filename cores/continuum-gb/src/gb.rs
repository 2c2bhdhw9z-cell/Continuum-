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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Model {
    Auto,
    Dmg,
    /// The first DMG revision (different boot ROM: registers and DIV differ).
    Dmg0,
    Mgb,
    Sgb,
    Sgb2,
    Cgb,
    Agb,
}

const STATE_MAGIC: &[u8; 8] = b"CGBSTAT2";

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

    /// The machine for a model chosen in the core options, optionally starting from a boot ROM.
    /// A Color or Advance model given a Game Boy-only cartridge runs it the way those consoles
    /// do (in Game Boy mode, telling the game which console it is through the registers); a
    /// Game Boy model given a Color cartridge runs it in Game Boy mode.
    pub fn with_model(rom: Vec<u8>, model: Model, boot: Option<Vec<u8>>) -> Result<GameBoy, String> {
        let cart = Cart::new(rom)?;
        let colour_cart = cart.cgb_flag & 0x80 != 0;
        let model = if model == Model::Auto {
            if colour_cart { Model::Cgb } else { Model::Dmg }
        } else {
            model
        };
        let cgb = colour_cart && matches!(model, Model::Cgb | Model::Agb);
        let mut gb = Self::build(cart, cgb);
        let c = &mut gb.cpu;
        match (model, cgb) {
            (Model::Mgb, _) => c.a = 0xFF,
            (Model::Dmg0, _) => {
                (c.a, c.f, c.b, c.c, c.d, c.e, c.h, c.l) = (0x01, 0x00, 0xFF, 0x13, 0x00, 0xC1, 0x84, 0x03);
                gb.bus.timer.counter = 0x182C; // Mooneye boot_div-dmg0
            }
            (Model::Sgb | Model::Sgb2, _) => {
                (c.a, c.f, c.b, c.c, c.d, c.e, c.h, c.l) = (0x01, 0x00, 0x00, 0x14, 0x00, 0x00, 0xC0, 0x60);
                if model == Model::Sgb2 {
                    c.a = 0xFF;
                }
                gb.bus.timer.counter = 0xD85C; // Mooneye boot_div-S / boot_div2-S
                gb.bus.sgb = Some(Box::default());
            }
            (Model::Agb, true) => {
                c.f = 0x00;
                c.b = 0x01;
            }
            (Model::Cgb | Model::Agb, false) => {
                (c.a, c.f, c.b, c.c, c.d, c.e, c.h, c.l) = (0x11, 0x80, 0x00, 0x00, 0x00, 0x08, 0x00, 0x7C);
                // The CGB boot ROM spends longer on a DMG cartridge (palette pick); DIV after it
                // (Mooneye boot_div-cgbABCDE, boot_div-A).
                gb.bus.timer.counter = 0x2674;
                if model == Model::Agb {
                    c.b = 0x01;
                    c.f = 0x00;
                    gb.bus.timer.counter = 0x2678;
                }
            }
            _ => {}
        }
        if let Some(boot) = boot {
            let want = if cgb { 0x900 } else { 0x100 };
            if boot.len() != want {
                return Err(format!("that boot ROM is {} bytes; this model needs {want}", boot.len()));
            }
            gb.bus.boot = boot;
            gb.bus.boot_on = true;
            let sgb = gb.bus.sgb.take();
            let c = &mut gb.cpu;
            (c.a, c.f, c.b, c.c, c.d, c.e, c.h, c.l, c.sp, c.pc) = (0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
            gb.bus.sgb = sgb;
            gb.bus.ppu.lcdc = 0;
        }
        Ok(gb)
    }

    /// CGB hardware revision 0 (the earliest): its boot ROM leaves DIV further on
    /// (Mooneye boot_div-cgb0). Revisions A-E behave alike as far as cBoy models them.
    pub fn set_cgb_revision_0(&mut self) {
        if self.bus.timer.counter == 0x2674 {
            self.bus.timer.counter = 0x2880;
        }
    }

    pub fn model_is_sgb(&self) -> bool {
        self.bus.sgb.is_some()
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
        let mut next: GameBoy = crate::state_decode(&data[8..])?;
        next.bus.cart.rom = std::mem::take(&mut self.bus.cart.rom);
        next.bus.link_loopback = self.bus.link_loopback;
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

    fn cgb_with_code(code: &[u8], cart_type: u8, rom_len: usize) -> GameBoy {
        let mut rom = vec![0u8; rom_len];
        rom[0x143] = 0x80; // CGB-compatible, like Pokemon Yellow
        rom[0x147] = cart_type;
        rom[0x148] = (rom_len / 0x8000).trailing_zeros() as u8;
        rom[0x149] = 0x03;
        rom[0x100..0x100 + code.len()].copy_from_slice(code);
        GameBoy::new(rom).unwrap()
    }

    #[test]
    fn hdma_with_lcd_off_copies_one_block_at_once() {
        let mut gb = cgb_machine();
        let b = &mut gb.bus;
        b.raw_write(0xFF40, 0x00);
        for i in 0..0x30u16 {
            b.raw_write(0xC000 + i, 0x80 | i as u8);
        }
        b.raw_write(0xFF51, 0xC0);
        b.raw_write(0xFF52, 0x00);
        b.raw_write(0xFF53, 0x08);
        b.raw_write(0xFF54, 0x00);
        b.raw_write(0xFF55, 0x82); // HBlank DMA, 3 blocks
        assert_eq!(b.ppu.vram[0x800], 0x80);
        assert_eq!(b.ppu.vram[0x80F], 0x8F);
        assert_eq!(b.ppu.vram[0x810], 0x00, "only one block with the LCD off");
        assert_eq!(b.raw_read(0xFF55), 0x01);
        b.raw_write(0xFF55, 0x00); // cancel
        assert_eq!(b.raw_read(0xFF55), 0x80);
    }

    #[test]
    fn ei_then_halt_with_a_pending_interrupt_returns_to_the_halt() {
        // EI / HALT / INC B ... with VBlank pending and enabled: the handler (at $40: INC C /
        // RETI) runs, then the CPU halts again rather than running INC B.
        let mut gb = cgb_with_code(&[0xF3, 0xFB, 0x76, 0x04, 0x18, 0xFE], 0x00, 0x8000);
        let mut rom = gb.bus.cart.rom.clone();
        rom[0x40] = 0x0C;
        rom[0x41] = 0xD9;
        gb.bus.cart.rom = rom;
        gb.bus.ie = 0x01;
        gb.bus.iflag = 0x01;
        gb.cpu.b = 0;
        gb.cpu.c = 0;
        for _ in 0..40 {
            gb.cpu.step(&mut gb.bus);
            if gb.cpu.c == 1 {
                gb.bus.iflag &= !0x01; // no second VBlank
            }
        }
        assert_eq!(gb.cpu.c, 1, "the interrupt ran once");
        assert_eq!(gb.cpu.b, 0, "and it went back to HALT, not past it");
        assert!(gb.cpu.halted);
    }

    #[test]
    fn mbc5_banks_including_bank_0_and_bit_8() {
        let mut gb = cgb_with_code(&[0x18, 0xFE], 0x1B, 0x80_0000 / 2); // 4 MB, 256 banks
        let mut rom = gb.bus.cart.rom.clone();
        for bank in 0..256usize {
            rom[bank * 0x4000 + 0x10] = bank as u8;
        }
        gb.bus.cart.rom = rom;
        let b = &mut gb.bus;
        b.raw_write(0x2000, 0x00);
        assert_eq!(b.raw_read(0x4010), 0x00, "MBC5 maps bank 0 at $4000");
        b.raw_write(0x2000, 0x3E);
        assert_eq!(b.raw_read(0x4010), 0x3E);
        b.raw_write(0x2000, 0xFF);
        assert_eq!(b.raw_read(0x4010), 0xFF);
        // RAM banks.
        b.raw_write(0x0000, 0x0A);
        b.raw_write(0x4000, 0x02);
        b.raw_write(0xA000, 0x77);
        b.raw_write(0x4000, 0x00);
        assert_ne!(b.raw_read(0xA000), 0x77);
        b.raw_write(0x4000, 0x02);
        assert_eq!(b.raw_read(0xA000), 0x77);
    }

    #[test]
    fn sgb_packets_on_joyp_change_nothing() {
        // A game sends SGB packets by pulsing P14/P15 together; with no SGB it just reads $FF
        // (no buttons, no player-2 id) and carries on.
        let mut gb = cgb_machine();
        let b = &mut gb.bus;
        for _ in 0..16 * 8 {
            b.raw_write(0xFF00, 0x00);
            b.raw_write(0xFF00, 0x30);
            b.raw_write(0xFF00, 0x10);
            b.raw_write(0xFF00, 0x30);
        }
        b.raw_write(0xFF00, 0x30);
        assert_eq!(b.raw_read(0xFF00) & 0x0F, 0x0F);
    }

    #[test]
    fn illegal_opcode_locks_and_logs() {
        let mut gb = cgb_with_code(&[0xD3], 0x00, 0x8000);
        for _ in 0..4 {
            gb.cpu.step(&mut gb.bus);
        }
        assert!(gb.cpu.locked);
        assert!(gb.bus.events.iter().any(|l| l.contains("illegal opcode D3 at 0100")), "{:?}", gb.bus.events);
    }

    #[test]
    fn speed_switch_is_logged() {
        let mut gb = cgb_machine();
        for _ in 0..8 {
            gb.cpu.step(&mut gb.bus);
        }
        assert!(gb.bus.double_speed);
        assert!(gb.bus.events.iter().any(|l| l.contains("speed switch: double")), "{:?}", gb.bus.events);
    }
}
