//! The Game Boy Advance machine, inside the same core as the Game Boy and Game Boy Color: one
//! core for all three, chosen from the ROM.

pub mod bios;
pub mod bus;
pub mod cpu;
pub mod mp2k;
pub mod ppu;
pub mod gpio;
pub mod save;

use serde::{Deserialize, Serialize};

use bus::Bus;
use cpu::Cpu;

/// Cycles in one frame: 228 lines of 1232.
pub const FRAME_CYCLES: u64 = 280_896;

/// The Advance header's own check byte at $BD: $19, every byte of $A0-$BC and the check byte
/// itself add up to zero. Every cartridge a real Advance will start has it right, and the tool
/// homebrew is finished with puts it right too.
fn header_check_ok(rom: &[u8]) -> bool {
    let sum = rom[0xA0..=0xBC].iter().fold(0u8, |a, b| a.wrapping_add(*b));
    sum.wrapping_add(0x19).wrapping_add(rom[0xBD]) == 0
}

/// A Game Boy Advance ROM. The fixed byte $96 at $B2 is what marks one, but on a Game Boy
/// cartridge that byte is ordinary code and can be $96 by chance ($96 is a real instruction), so
/// a file that also carries a Game Boy header is only taken as an Advance game when the Advance
/// header's own check byte adds up as well. That kept Game Boy games out of the 240x160 Advance
/// machine they could not be played in.
pub fn is_gba_rom(rom: &[u8]) -> bool {
    if rom.len() < 0xC0 || rom[0xB2] != 0x96 {
        return false;
    }
    !crate::cart::looks_like_gb_rom(rom) || header_check_ok(rom)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Gba {
    pub cpu: Cpu,
    pub bus: Bus,
    /// Idle-loop skipping (core option) and its detector; not saved.
    #[serde(skip)]
    pub idle_skip: bool,
    #[serde(skip)]
    idle: IdleWatch,
}

/// Spots a short backward loop that writes nothing (a game polling a register while it waits)
/// and lets the machine jump to the end of the scanline instead of spinning through it.
#[derive(Default, Clone)]
struct IdleWatch {
    prev_pc: u32,
    head: u32,
    writes: u64,
    hits: u32,
    skipped: u64,
}

const STATE_MAGIC: &[u8; 8] = b"CGBASTA2";

impl Gba {
    pub fn new(rom: Vec<u8>) -> Gba {
        let mut bus = Bus::new(rom);
        bus.post_bios();
        Gba { cpu: Cpu::default(), bus, idle_skip: false, idle: IdleWatch::default() }
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
        Ok(Gba { cpu: Cpu::power_on(), bus, idle_skip: false, idle: IdleWatch::default() })
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
        if self.idle_skip {
            self.watch_idle();
        }
    }

    fn watch_idle(&mut self) {
        let pc = self.cpu.r[15];
        let w = &mut self.idle;
        let back = w.prev_pc.wrapping_sub(pc);
        w.prev_pc = pc;
        if back == 0 || back > 16 {
            return;
        }
        if pc == w.head && self.bus.writes == w.writes {
            w.hits += 1;
            if w.hits >= 3 && !self.bus.irq_requested() {
                let n = self.bus.cycles_to_line_end();
                self.bus.idle(n);
                self.bus.run_pending();
                w.skipped += u64::from(n);
                w.hits = 0;
            }
        } else {
            w.head = pc;
            w.hits = 0;
        }
        w.writes = self.bus.writes;
    }

    /// Cycles idle-loop skipping has jumped over (for tests).
    pub fn idle_skipped(&self) -> u64 {
        self.idle.skipped
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
        // Added after the layout above was fixed, so they come after it and a state made before
        // them still loads: the CPU's pipeline and the bus's timing (prefetch buffer, timers).
        out.extend(bincode::serialize(&(self.cpu.pipe, &self.bus.tm)).unwrap_or_default());
        out
    }

    pub fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        if data.len() < 8 || &data[..8] != STATE_MAGIC {
            return Err("not a cBoy GBA state".into());
        }
        let mut next: Gba = crate::state_decode(&data[8..])?;
        let used = 8 + bincode::serialized_size(&next).map_err(|e| e.to_string())? as usize;
        match data.get(used..).filter(|rest| !rest.is_empty()) {
            Some(rest) => {
                let (pipe, tm): (cpu::CpuPipe, bus::BusTiming) = crate::state_decode(rest)?;
                next.cpu.pipe = pipe;
                next.bus.tm = tm;
            }
            // An older state: its timers' counts are the counts at the moment it was made.
            None => next.bus.tm.timer_at = [next.bus.cycles; 4],
        }
        next.bus.rom = std::mem::take(&mut self.bus.rom);
        next.bus.bios = std::mem::take(&mut self.bus.bios);
        // The music engine's own notes are read back out of the machine's memory, so only the
        // player's setting and where this cartridge keeps its songs carry over.
        next.bus.mp2k = std::mem::take(&mut self.bus.mp2k);
        next.bus.mp2k.restart();
        // A state from a session with the real BIOS, loaded without one: built-in calls take over.
        next.bus.real_bios = next.bus.real_bios && self.bus.real_bios;
        *self = next;
        Ok(())
    }
}
