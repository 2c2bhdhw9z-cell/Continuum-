//! The Game Boy Advance machine, inside the same core as the Game Boy and Game Boy Color: one
//! core for all three, chosen from the ROM.

pub mod bios;
pub mod bus;
pub mod cheats;
pub mod cpu;
pub mod mp2k;
pub mod ppu;
pub mod gpio;
pub mod save;
pub mod rfu;
pub mod sio;

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
    /// The cheat device and its codes (cheats.rs). Not saved: the app sets them.
    #[serde(skip)]
    pub cheats: cheats::Device,
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

const STATE_MAGIC: &[u8; 8] = b"CGBASTA3";
const LEGACY_STATE_MAGIC: &[u8; 8] = b"CGBASTA2";

impl Gba {
    pub fn new(rom: Vec<u8>) -> Gba {
        let mut bus = Bus::new(rom);
        bus.post_bios();
        Gba { cpu: Cpu::default(), bus, idle_skip: false, idle: IdleWatch::default(), cheats: Default::default() }
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
        Ok(Gba { cpu: Cpu::power_on(), bus, idle_skip: false, idle: IdleWatch::default(), cheats: Default::default() })
    }

    pub fn step(&mut self) {
        // The BIOS's wait has been interrupted and the handler has come back to it. The handler
        // ends with `subs pc, lr, #4`, and in Thumb that lands four past the wait, one instruction
        // on ($74); both places mean the same thing here, so treat either as the wait ending.
        if !self.bus.halted && (self.cpu.pc == 0x70 || self.cpu.pc == 0x74) {
            if let Some(back) = self.bus.halt_return.take() {
                self.cpu.pc = back;
                self.cpu.pipe.at = None;
            }
        }
        if self.bus.halted {
            if self.bus.irq_elapsed() {
                self.bus.halted = false;
                if !self.bus.irq_line() {
                    // A flag came up that no handler will be entered for (IME off, or the
                    // processor told to ignore interrupts): the wait simply ends and the game
                    // carries on where it left off.
                    if let Some(back) = self.bus.halt_return.take() {
                        self.cpu.pc = back;
                    }
                }
                self.cpu.pipe.at = None;
            } else {
                self.bus.halt_skip();
                return;
            }
        }
        if let Some(hook) = self.cheats.hook {
            // The codes run where their hook code says, as the device makes the game call it:
            // when the processor is about to run that instruction (not when an interrupt comes
            // first, which runs it later).
            let irq_first = self.bus.irq_line() && self.cpu.cpsr & 0x80 == 0;
            if self.cpu.pc == hook && !irq_first {
                self.bus.sync_all();
                self.cheats.run(&mut self.bus);
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
        self.bus.sync_all();
        let start = self.bus.cycles;
        while !self.bus.frame_ready && self.bus.cycles - start < FRAME_CYCLES * 2 {
            self.step();
        }
        // The sound chip is brought up to date, so the frame's sound is all there.
        self.bus.sync_all();
        // The blank has started: codes with no hook run now.
        if self.cheats.hook.is_none() && !self.cheats.ops.is_empty() {
            self.cheats.run(&mut self.bus);
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
        out.extend(bincode::serialize(&(self.cpu.pipe, &self.bus.tm, &self.bus.apu.chip)).unwrap_or_default());
        // And, after those, what a transfer in progress and an interrupt on its way are doing
        // (both last a handful of cycles, but a state saved in the middle of one has to carry
        // on the same).
        out.extend(bincode::serialize(&self.bus.cycle_state()).unwrap_or_default());
        out
    }

    pub fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        if data.len() < 8 || (&data[..8] != STATE_MAGIC && &data[..8] != LEGACY_STATE_MAGIC) {
            return Err("not a cBoy GBA state".into());
        }
        // A CGBASTA2 state is upgraded to the current layout first, so everything below is
        // the one CGBASTA3 path, unchanged: the current format keeps working exactly as before.
        // But the tag alone does not say which CGBASTA2 layout a state is: P2 added the
        // resampler to the sound chip without bumping this tag, so a state saved after P2
        // already has the current layout under the old tag, while a pre-P2 one is 1,113
        // bytes short of it. A post-P2 state decodes as the current layout with its
        // trailing blocks consuming the rest exactly, so it is loaded as it is; only a
        // state that does not is upgraded, and the upgrade is checked by the normal path.
        if &data[..8] == LEGACY_STATE_MAGIC {
            if looks_like_current_layout(data) {
                return self.load_current(data);
            }
            let upgraded = legacy::upgrade(data)?;
            return self.load_current(&upgraded);
        }
        self.load_current(data)
    }

    /// Loads a state in the current (CGBASTA3) layout. The tag itself is not checked here:
    /// `load_state` has already decided whether this body is current or upgraded.
    fn load_current(&mut self, data: &[u8]) -> Result<(), String> {
        let mut next: Gba = crate::state_decode(&data[8..])?;
        let used = 8 + bincode::serialized_size(&next).map_err(|e| e.to_string())? as usize;
        match data.get(used..).filter(|rest| !rest.is_empty()) {
            Some(rest) => {
                let (pipe, tm, chip): (cpu::CpuPipe, bus::BusTiming, crate::apu::Chip) = crate::state_decode(rest)?;
                next.cpu.pipe = pipe;
                next.bus.tm = tm;
                next.bus.apu.chip = chip;
                // A transfer or an interrupt in progress comes after that, in states new enough
                // to have it; an older state simply carries on from where the next event is.
                let used_here = bincode::serialized_size(&(pipe, &next.bus.tm, &next.bus.apu.chip))
                    .map(|n| n as usize)
                    .unwrap_or(rest.len());
                if let Some(tail) = rest.get(used_here..).filter(|t| !t.is_empty()) {
                    if let Ok(c) = crate::state_decode(tail) {
                        next.bus.set_cycle_state(c);
                    }
                }
            }
            // An older state: its timers' counts are the counts at the moment it was made, and
            // the sound channels carry over from the way they were saved then.
            None => {
                next.bus.tm.timer_at = [next.bus.cycles; 4];
                next.bus.apu.adopt_legacy();
            }
        }
        next.bus.apu.agb = true;
        next.bus.rom = std::mem::take(&mut self.bus.rom);
        next.bus.bios = std::mem::take(&mut self.bus.bios);
        // The cheat codes the app switched on stay on (they're not in the state).
        next.cheats = std::mem::take(&mut self.cheats);
        next.bus.rom_patches = std::mem::take(&mut self.bus.rom_patches);
        // The music engine's own notes are read back out of the machine's memory, so only the
        // player's setting and where this cartridge keeps its songs carry over.
        next.bus.mp2k = std::mem::take(&mut self.bus.mp2k);
        next.bus.mp2k.restart();
        // A state from a session with the real BIOS, loaded without one: built-in calls take over.
        next.bus.real_bios = next.bus.real_bios && self.bus.real_bios;
        next.bus.forget_cached_times();
        *self = next;
        // The span, snapshot and enable-delay state is not in the state: the enables
        // restart from the live DISPCNT, and the next line renders whole from live OAM.
        self.bus.sync_bg_bits();
        Ok(())
    }
}

/// Whether a state's body decodes as the current layout with its trailing blocks
/// consuming the rest exactly. A pre-P2 body is 1,113 bytes short of the current layout,
/// so decoding it as current either fails outright (a length field read out of the
/// shifted stream is huge, or the stream runs out) or leaves trailing bytes that do not
/// parse as the timing and cycle-state blocks. This is only a discriminator for the
/// ambiguous CGBASTA2 tag; it never accepts a state on its own — `load_current` below
/// still decodes the body it is given, and a wrong call there fails the same way.
fn looks_like_current_layout(data: &[u8]) -> bool {
    let Ok(next): Result<Gba, String> = crate::state_decode(&data[8..]) else { return false };
    let Ok(used) = bincode::serialized_size(&next).map(|n| 8 + n as usize) else { return false };
    let Some(rest) = data.get(used..) else { return false };
    if rest.is_empty() {
        return true;
    }
    let Ok((pipe, tm, chip)): Result<(cpu::CpuPipe, bus::BusTiming, crate::apu::Chip), String> =
        crate::state_decode(rest)
    else {
        return false;
    };
    let Ok(used_here) =
        bincode::serialized_size(&(pipe, &tm, &chip)).map(|n| n as usize)
    else {
        return false;
    };
    let Some(tail) = rest.get(used_here..) else { return false };
    if tail.is_empty() {
        return true;
    }
    let Ok(c): Result<(sio::Sio, (bool, u64, bool), (u8, [Option<(u32, bool)>; 4]), Option<u32>), String> =
        crate::state_decode(tail)
    else {
        return false;
    };
    bincode::serialized_size(&c).map(|n| n as usize).unwrap_or(usize::MAX) == tail.len()
}

/// Loading a CGBASTA2 state: the layout before P2 added the resampler state to the sound
/// chip (1,113 bytes, at the end of `Apu`, after `sample_acc`).
///
/// The pattern is the Game Boy side's CGBSTAT3 upgrade (gb.rs): the current format stays
/// byte-identical and a legacy body is upgraded on load, so there is exactly one current
/// format and one upgrade path. bincode writes a struct's fields in order with no framing,
/// so the splice point is found by decoding the body field by field up to the end of the
/// legacy sound chip — the CPU, then the bus's fields before the sound chip (the BIOS flag,
/// the GPIO, the six memory blocks, the save chip and the picture hardware), then the
/// legacy sound chip itself — and 1,113 zero bytes are spliced in there. The defaults are
/// the right values: the resampler refills from the next samples, and a zero ring/count/
/// pending list is exactly what power-up leaves. The upgrade is checked, not trusted: a
/// short or corrupt body fails here, and the spliced body is decoded by the normal path,
/// so a wrong splice fails there instead of loading a machine made of the wrong bytes.
mod legacy {
    use serde::{Deserialize, Serialize};

    use super::cpu::Cpu;
    use super::{gpio, ppu, save};

    /// The resampler state P2 added to `Apu`: 128 stereo samples, a count, eight pending
    /// times, a length and the DC-blocker state — all zero at power-up, so all zero bytes.
    /// 128*2*4 + 8 + 8*8 + 1 + 2*2*4 = 1,113.
    pub(crate) const RESAMP_BYTES: usize = 1113;

    /// A CGBASTA2 state, tag included, upgraded to a CGBASTA3 state.
    pub(crate) fn upgrade(data: &[u8]) -> Result<Vec<u8>, String> {
        let body = &data[8..];
        let at = resamp_offset(body)?;
        let mut out = Vec::with_capacity(data.len() + RESAMP_BYTES);
        out.extend_from_slice(super::STATE_MAGIC);
        out.extend_from_slice(&body[..at]);
        out.extend_from_slice(&vec![0u8; RESAMP_BYTES]);
        out.extend_from_slice(&body[at..]);
        debug_assert_eq!(out.len(), data.len() + RESAMP_BYTES);
        Ok(out)
    }

    /// The offset, from the start of the body (after the tag), where the resampler state
    /// sits in a current state — and where it is missing from a CGBASTA2 one: the end of
    /// the sound chip as CGBASTA2 has it. Works on either layout, because the legacy
    /// sound chip decodes from its own prefix and stops after `sample_acc`.
    pub(crate) fn resamp_offset(body: &[u8]) -> Result<usize, String> {
        let cpu: Cpu = crate::state_decode(body)?;
        let mut at = size_of(&cpu)?;
        // The bus, field by field up to the sound chip: the BIOS flag, the GPIO, the six
        // memory blocks, the save chip and the picture hardware.
        at += size_of(&crate::state_decode::<bool>(&body[at..])?)?;
        at += size_of(&crate::state_decode::<gpio::Gpio>(&body[at..])?)?;
        for _ in 0..6 {
            at += size_of(&crate::state_decode::<Vec<u8>>(&body[at..])?)?;
        }
        at += size_of(&crate::state_decode::<save::Save>(&body[at..])?)?;
        at += size_of(&crate::state_decode::<ppu::Ppu>(&body[at..])?)?;
        at += size_of(&crate::state_decode::<LegacyApu>(&body[at..])?)?;
        Ok(at)
    }

    fn size_of<T: Serialize>(v: &T) -> Result<usize, String> {
        bincode::serialized_size(v).map(|n| n as usize).map_err(|e| e.to_string())
    }

    /// The sound chip as CGBASTA2 has it: the channel state, before the resampler state
    /// was added after `sample_acc`. The channel types live in apu.rs's private `legacy`
    /// module, so they are mirrored here field for field (as gb.rs does for CGBSTAT3).
    #[derive(Clone, Default, Serialize, Deserialize)]
    struct LegacyEnvelope {
        volume: u8,
        period: u8,
        timer: u8,
        up: bool,
    }

    #[derive(Clone, Default, Serialize, Deserialize)]
    struct LegacySquare {
        on: bool,
        duty: u8,
        step: u8,
        length: u16,
        length_on: bool,
        freq: u16,
        timer: u32,
        env: LegacyEnvelope,
        sweep_period: u8,
        sweep_down: bool,
        sweep_shift: u8,
        sweep_timer: u8,
        sweep_on: bool,
        shadow: u16,
        swept_down: bool,
    }

    #[derive(Clone, Default, Serialize, Deserialize)]
    struct LegacyWave {
        on: bool,
        length: u16,
        length_on: bool,
        freq: u16,
        timer: u32,
        pos: u8,
        shift: u8,
    }

    #[derive(Clone, Default, Serialize, Deserialize)]
    struct LegacyNoise {
        on: bool,
        length: u16,
        length_on: bool,
        env: LegacyEnvelope,
        lfsr: u16,
        timer: u32,
    }

    #[derive(Clone, Default, Serialize, Deserialize)]
    struct LegacyApu {
        regs: Vec<u8>,
        power: bool,
        sq1: LegacySquare,
        sq2: LegacySquare,
        wave: LegacyWave,
        noise: LegacyNoise,
        seq_step: u8,
        sample_acc: u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A CGBASTA2 state — the layout before P2 added the resampler state to the sound
    /// chip — loads and the game runs on from it. There is no pre-P2 state file in the
    /// repo, so the test makes one the way one was made: a current state with the
    /// resampler block taken back out, under the old tag. The same machine with no
    /// trailing timing/sound-chip blocks is covered too.
    #[test]
    fn a_cgbasta2_state_loads_and_runs_on() {
        let mut rom = vec![0u8; 0x1000];
        // B . at the cartridge start, so the machine has something to run.
        rom[0..4].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
        let mut gba = Gba::new(rom.clone());
        for _ in 0..3 {
            gba.run_frame();
        }
        let full = gba.save_state();
        assert_eq!(&full[..8], b"CGBASTA3");
        let old = strip_to_cgbasta2(&full);
        assert_eq!(&old[..8], b"CGBASTA2");
        assert_eq!(old.len(), full.len() - super::legacy::RESAMP_BYTES);
        // The stripped state is not the current layout, so the discriminator must say so;
        // a post-P2 state — the current layout under the old tag, made by retagging — must
        // say it is, and load as it is (not upgraded, which would splice 1,113 bytes into
        // a body that already has them).
        assert!(!super::looks_like_current_layout(&old));
        let mut retagged = full.clone();
        retagged[..8].copy_from_slice(b"CGBASTA2");
        assert!(super::looks_like_current_layout(&retagged));
        let mut post_p2 = Gba::new(rom.clone());
        post_p2.load_state(&retagged).unwrap();
        assert_eq!(post_p2.cpu.pc, gba.cpu.pc);
        assert_eq!(post_p2.bus.cycles, gba.bus.cycles);
        let mut loaded = Gba::new(rom.clone());
        loaded.load_state(&old).unwrap();
        assert_eq!(loaded.cpu.pc, gba.cpu.pc, "the program counter comes back");
        assert_eq!(loaded.bus.cycles, gba.bus.cycles);
        loaded.run_frame();
        assert!(loaded.bus.cycles > gba.bus.cycles, "and the game runs on");
        // Saving the loaded machine gives a current state again, which loads in turn.
        let again = loaded.save_state();
        let mut third = Gba::new(rom.clone());
        third.load_state(&again).unwrap();
        assert_eq!(third.cpu.pc, loaded.cpu.pc);
        // A legacy state with no trailing blocks after the machine (the layout fixed
        // under CGBASTA2 and nothing after it) loads as well.
        let main: usize = {
            let g: Gba = crate::state_decode(&full[8..]).unwrap();
            8 + bincode::serialized_size(&g).unwrap() as usize
        };
        let mut bare = Gba::new(rom.clone());
        bare.load_state(&old[..main - super::legacy::RESAMP_BYTES]).unwrap();
        bare.run_frame();
        assert!(bare.bus.cycles > 0);
    }

    /// Takes the resampler block back out of a CGBASTA3 state, leaving a CGBASTA2 one.
    fn strip_to_cgbasta2(full: &[u8]) -> Vec<u8> {
        let body = &full[8..];
        let at = super::legacy::resamp_offset(body).unwrap();
        let mut out = b"CGBASTA2".to_vec();
        out.extend_from_slice(&body[..at]);
        out.extend_from_slice(&body[at + super::legacy::RESAMP_BYTES..]);
        assert_eq!(out.len(), full.len() - super::legacy::RESAMP_BYTES);
        out
    }
}
