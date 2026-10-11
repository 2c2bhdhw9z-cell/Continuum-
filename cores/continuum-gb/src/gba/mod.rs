//! The Game Boy Advance machine, inside the same core as the Game Boy and Game Boy Color: one
//! core for all three, chosen from the ROM.

pub mod bios;
pub mod bus;
pub mod cpu;
pub mod ppu;
pub mod gpio;
pub mod save;

use serde::{Deserialize, Serialize};

use bus::Bus;
use cpu::Cpu;

/// Cycles in one frame: 228 lines of 1232.
pub const FRAME_CYCLES: u64 = 280_896;

/// A GBA ROM: the fixed header byte $96 at $B2.
pub fn is_gba_rom(rom: &[u8]) -> bool {
    rom.len() >= 0xC0 && rom[0xB2] == 0x96
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Gba {
    pub cpu: Cpu,
    pub bus: Bus,
}

const STATE_MAGIC: &[u8; 8] = b"CGBASTA1";

impl Gba {
    pub fn new(rom: Vec<u8>) -> Gba {
        let mut bus = Bus::new(rom);
        bus.post_bios();
        Gba { cpu: Cpu::default(), bus }
    }

    /// Boots through a real BIOS image (16 KiB) from address 0: the logo and chime play, then
    /// the BIOS jumps to the cartridge itself.
    pub fn with_bios(rom: Vec<u8>, bios: Vec<u8>) -> Result<Gba, String> {
        if bios.len() != 0x4000 {
            return Err(format!("a Game Boy Advance BIOS is 16384 bytes, this one is {}", bios.len()));
        }
        let mut bus = Bus::new(rom);
        bus.bios = bios;
        bus.real_bios = true;
        Ok(Gba { cpu: Cpu::power_on(), bus })
    }

    pub fn step(&mut self) {
        if self.bus.halted {
            if self.bus.irq_requested() {
                self.bus.halted = false;
            } else {
                self.bus.halt_skip();
                return;
            }
        }
        self.cpu.step(&mut self.bus);
    }

    pub fn run_frame(&mut self) {
        self.bus.frame_ready = false;
        let start = self.bus.cycles;
        while !self.bus.frame_ready && self.bus.cycles - start < FRAME_CYCLES * 2 {
            self.step();
        }
    }

    pub fn set_keys(&mut self, keys: u16) {
        self.bus.keys = keys;
        self.bus.keypad_irq();
    }

    pub fn save_state(&self) -> Vec<u8> {
        let mut out = STATE_MAGIC.to_vec();
        out.extend(bincode::serialize(self).unwrap_or_default());
        out
    }

    pub fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        if data.len() < 8 || &data[..8] != STATE_MAGIC {
            return Err("not a cBoy GBA state".into());
        }
        let mut next: Gba = bincode::deserialize(&data[8..]).map_err(|e| e.to_string())?;
        next.bus.rom = std::mem::take(&mut self.bus.rom);
        next.bus.bios = std::mem::take(&mut self.bus.bios);
        // A state from a session with the real BIOS, loaded without one: built-in calls take over.
        next.bus.real_bios = next.bus.real_bios && self.bus.real_bios;
        next.bus.ppu.frame = std::mem::take(&mut self.bus.ppu.frame);
        *self = next;
        Ok(())
    }
}
