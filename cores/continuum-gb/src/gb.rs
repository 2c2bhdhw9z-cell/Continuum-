//! The machine: CPU plus bus, frame stepping and save states.

use serde::{Deserialize, Serialize};

use crate::bus::Bus;
use crate::cart::Cart;
use crate::compat;
use crate::cpu::Cpu;

/// T-cycles in one frame (154 lines of 456 dots).
pub const FRAME_CYCLES: u64 = 70_224;


#[derive(Clone, Serialize, Deserialize)]
pub struct GameBoy {
    pub cpu: Cpu,
    pub bus: Bus,
    /// The colours a Color's start-up code gives this black-and-white game: its own pick, or the
    /// scheme the player held the buttons for. Kept so that a save state from before the game was
    /// in colour - a Game Boy's, or an older cBoy's, whose palette memory nothing ever wrote -
    /// comes back in them on a Color instead of blank. Settings, not state, so not saved.
    #[serde(skip)]
    pub compat: Option<compat::Palettes>,
    /// The console it was started as (settings, not state).
    #[serde(skip)]
    pub console: Model,
    /// Skipping the game's waiting loops (core option), and what it has spotted. Not saved.
    #[serde(skip)]
    pub wait: WaitLoop,
}

/// A game waiting for something to happen, in a loop that reads only memory nothing but the
/// CPU's own writes can change (cartridge ROM, work RAM, high RAM, the interrupt switches) and
/// writes nothing: `wait: ld a,(flag) / and a / jr z,wait` until an interrupt handler sets the
/// flag. Once two turns of it in a row have gone exactly the same way - the same instructions,
/// the same cycles, the CPU in the same state at each - every later turn must go that way too
/// until an interrupt is taken, so the turns are not run instruction by instruction any more:
/// the rest of the machine is clocked through each instruction's cycles, and at every point
/// between instructions where the CPU would take an interrupt, it is handed back to the CPU in
/// exactly the state it would have had there. Nothing a game, a picture or a sound can tell
/// changes; only the work of running the CPU is saved (what bgb calls waitloop detection).
#[derive(Clone, Default)]
pub struct WaitLoop {
    pub on: bool,
    /// The loop being watched: the address a backward jump went to.
    head: Option<u16>,
    /// The turn being recorded: the CPU before each instruction, and its cycles.
    turn: Vec<(Cpu, u32)>,
    recording: bool,
    /// The last whole turn, kept to compare the next one with.
    last: Vec<(Cpu, u32)>,
    /// The CPU's writes when that turn was recorded: any write since and it may not hold.
    writes: u64,
    /// Two turns matched: skip turns starting from `last[0]`'s state.
    armed: bool,
    /// M-cycles skipped (for tests).
    pub skipped: u64,
}

/// The longest loop watched, in instructions.
const WAIT_LOOP_MAX: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Model {
    #[default]
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

const STATE_MAGIC: &[u8; 8] = b"CGBSTAT3";

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
    /// do (in Game Boy mode, telling the game which console it is through the registers, and in
    /// the colours its start-up code picks for the game); a Game Boy model given a Color
    /// cartridge runs it in Game Boy mode.
    pub fn with_model(rom: Vec<u8>, model: Model, boot: Option<Vec<u8>>) -> Result<GameBoy, String> {
        let cart = Cart::new(rom)?;
        let colour_cart = cart.cgb_flag & 0x80 != 0;
        let model = if model == Model::Auto {
            if colour_cart { Model::Cgb } else { Model::Dmg }
        } else {
            model
        };
        let cgb = colour_cart && matches!(model, Model::Cgb | Model::Agb);
        let console = model;
        // A Color's own start-up file runs in colour mode whatever the cartridge is, and puts the
        // console into Game Boy mode itself at the end (KEY0) when the cartridge is a
        // black-and-white one, after writing the colours it picked.
        let colour_start = !cgb && matches!(model, Model::Cgb | Model::Agb) && boot.as_ref().is_some_and(|b| b.len() == 0x900);
        let mut gb = Self::build(cart, cgb || colour_start);
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
                // B is the title sum the start-up code looked the game up by, when the game is
                // Nintendo's (0 when it did not look), and HL is left past the logo map for the
                // two titles it draws one for (Pan Docs, "Power Up Sequence").
                let sum = compat::title_checksum(&gb.bus.cart.rom);
                let (h, l) = if compat::draws_the_logo(sum) { (0x99, 0x1A) } else { (0x00, 0x7C) };
                (c.a, c.f, c.b, c.c, c.d, c.e, c.h, c.l) = (0x11, 0x80, sum.unwrap_or(0), 0x00, 0x00, 0x08, h, l);
                // The CGB boot ROM spends longer on a DMG cartridge (palette pick); DIV after it
                // (Mooneye boot_div-cgbABCDE, boot_div-A).
                gb.bus.timer.counter = 0x2674;
                if model == Model::Agb {
                    // An Advance's start-up code ends with one more `inc b`, and its flags are
                    // that instruction's: zero when B wraps, half-carry out of the low four bits.
                    let b = c.b;
                    c.b = b.wrapping_add(1);
                    c.f = (if c.b == 0 { 0x80 } else { 0 }) | (if b & 0x0F == 0x0F { 0x20 } else { 0 });
                    gb.bus.timer.counter = 0x2678;
                }
                // It is still Color hardware, so the registers the console owns stay readable,
                // holding what its start-up code left: neither half of the pad selected, and the
                // two palette pointers part-way through the palette it picked for the game
                // (Mooneye boot_hwio-C).
                gb.bus.cgb_hardware = true;
                gb.bus.ppu.cgb_hardware = true;
                gb.bus.joyp_select = 0x30;
                gb.bus.ppu.bcps = 0x88;
                gb.bus.ppu.ocps = 0x90;
                // It also switches on the Game Boy's object order (OPRI) and writes the colours
                // it picked into palette memory, which is what the game's BGP, OBP0 and OBP1 then
                // index. A Color's own start-up file does all of that itself as it runs.
                let picked = compat::picked(&gb.bus.cart.rom);
                gb.compat = Some(picked);
                if !colour_start {
                    gb.bus.ppu.opri = 1;
                    gb.load_compat_palettes(&picked);
                    if compat::draws_the_logo(sum) {
                        gb.bus.ppu.dmg_logo_map();
                    }
                }
            }
            _ => {}
        }
        if let Some(boot) = boot {
            let want = if cgb || colour_start { 0x900 } else { 0x100 };
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
        // An Advance draws a Color game's picture the way a CGB E does (the Mealybug PPU
        // documentation; SameBoy).
        if model == Model::Agb {
            gb.bus.ppu.cgb_rev = crate::ppu::CgbRev::E;
        }
        gb.console = console;
        Ok(gb)
    }

    /// CGB hardware revision 0 (the earliest): its boot ROM leaves DIV further on
    /// (Mooneye boot_div-cgb0). Revisions A-E behave alike as far as cBoy models them.
    pub fn set_cgb_revision_0(&mut self) {
        if self.bus.timer.counter == 0x2674 {
            self.bus.timer.counter = 0x2880;
        }
    }

    /// Which Color revision draws the picture (`ppu::CgbRev`). An Advance is not a Color of
    /// any revision and keeps its own (`with_model`).
    pub fn set_cgb_revision(&mut self, rev: crate::ppu::CgbRev) {
        if self.console != Model::Agb {
            self.bus.ppu.cgb_rev = rev;
        }
    }

    pub fn model_is_sgb(&self) -> bool {
        self.bus.sgb.is_some()
    }

    /// The colours a Color's start-up code writes for a black-and-white game: its pick for the
    /// background into background palette 0, and the two object palettes into object palettes
    /// 0 and 1, which are the only ones Game Boy mode reads. It fades every background palette
    /// to white before that, so the other seven are left white.
    fn load_compat_palettes(&mut self, palettes: &compat::Palettes) {
        let ppu = &mut self.bus.ppu;
        ppu.bg_pal = [0xFF, 0x7F].repeat(32);
        ppu.obj_pal = [0xFF, 0x7F].repeat(32);
        let [bg, obj0, obj1] = palettes.bytes();
        ppu.bg_pal[..8].copy_from_slice(&bg);
        ppu.obj_pal[..8].copy_from_slice(&obj0);
        ppu.obj_pal[8..16].copy_from_slice(&obj1);
    }

    /// The player holding one of the twelve button combinations while a Color starts, which makes
    /// its start-up code use that scheme instead of the one it picked for the game. A Color's own
    /// start-up file is handed the buttons to read, the way a player's thumbs would hand them
    /// over; without one the scheme goes straight into palette memory. Only a black-and-white
    /// game on a Color has a scheme to pick, so anything else is left exactly as it is.
    pub fn pick_colours(&mut self, pick: &compat::Pick) {
        if !self.bus.cgb_hardware || self.bus.cart.cgb_flag & 0x80 != 0 {
            return;
        }
        self.compat = Some(pick.palettes);
        if self.bus.boot_on && self.bus.boot.len() == 0x900 {
            self.bus.boot_buttons = pick.buttons;
        } else {
            self.load_compat_palettes(&pick.palettes);
        }
    }

    fn build(cart: Cart, cgb: bool) -> GameBoy {
        let mut bus = Bus::new(cart);
        bus.ppu.boot_logo(&bus.cart.rom);
        let cpu = if cgb {
            bus.cgb = true;
            bus.cgb_hardware = true;
            bus.ppu.cgb_hardware = true;
            bus.ppu.cgb = true;
            // The CGB boot ROM leaves every background palette white; objects too, here.
            bus.ppu.bg_pal = vec![0xFF, 0x7F].repeat(32);
            bus.ppu.obj_pal = vec![0xFF, 0x7F].repeat(32);
            Cpu::cgb()
        } else {
            Cpu::default()
        };
        GameBoy { cpu, bus, compat: None, console: Model::Auto, wait: WaitLoop::default() }
    }

    /// Runs until the PPU finishes a frame (or one frame's worth of cycles with the LCD off).
    pub fn run_frame(&mut self) {
        let start = self.bus.cycles;
        self.bus.ppu.frame_ready = false;
        while !self.bus.ppu.frame_ready && self.bus.cycles - start < FRAME_CYCLES * 2 {
            if self.wait.on {
                self.step_watching(start);
            } else {
                self.cpu.step(&mut self.bus);
            }
            if !self.bus.ppu.on() && self.bus.cycles - start >= FRAME_CYCLES {
                break;
            }
        }
        // The blank has started: a GameShark writes its codes now.
        self.bus.apply_cheat_pokes();
    }

    /// One instruction, or a run of skipped turns of a waiting loop (see `WaitLoop`).
    fn step_watching(&mut self, start: u64) {
        let w = &self.wait;
        let ready = w.armed && w.writes == self.bus.loop_writes && w.last.first().is_some_and(|(c, _)| c.same_state(&self.cpu));
        if ready && self.skip_turns(start) > 0 {
            return;
        }
        let recording = self.wait.recording;
        let before = if recording { Some(self.cpu.clone()) } else { None };
        let (pc0, plain0) = (self.cpu.pc, self.cpu.plain());
        let cycles = self.bus.cycles;
        self.bus.loop_unstable = false;
        let writes = self.bus.loop_writes;
        self.cpu.step(&mut self.bus);
        let back = self.cpu.pc < pc0 && pc0 - self.cpu.pc <= 64;
        if before.is_none() && !back {
            return;
        }
        let pure = !self.bus.loop_unstable && self.bus.loop_writes == writes && plain0 && self.cpu.plain() && {
            let op = self.bus.raw_read(pc0);
            op != 0x10 && op != 0x76
        };
        let w = &mut self.wait;
        if let Some(before) = before {
            let spent = ((self.bus.cycles - cycles) / 4) as u32;
            if !pure || w.turn.len() >= WAIT_LOOP_MAX {
                w.recording = false;
                w.turn.clear();
                w.last.clear();
                w.armed = false;
            } else {
                w.turn.push((before, spent));
                if Some(self.cpu.pc) == w.head {
                    // A whole turn: the same as the one before it?
                    let same = w.turn.len() == w.last.len()
                        && w.turn.iter().zip(&w.last).all(|(a, b)| a.1 == b.1 && a.0.same_state(&b.0));
                    w.last = std::mem::take(&mut w.turn);
                    w.writes = self.bus.loop_writes;
                    w.armed = same;
                    w.recording = !same;
                }
            }
            return;
        }
        // A short jump backwards: watch the loop it closes.
        let still_armed = w.armed && w.head == Some(self.cpu.pc) && w.writes == self.bus.loop_writes;
        if pure && back && !still_armed {
            w.head = Some(self.cpu.pc);
            w.recording = true;
            w.turn.clear();
            w.last.clear();
            w.armed = false;
        }
    }

    /// Clocks the machine through turns of the loop without running the CPU, until the CPU
    /// would take an interrupt between two of its instructions or the frame is done. Returns
    /// the M-cycles skipped: none means the CPU has something to do right away.
    fn skip_turns(&mut self, start: u64) -> u64 {
        let turn = std::mem::take(&mut self.wait.last);
        let mut skipped = 0;
        'turns: loop {
            for (cpu, m) in &turn {
                let irq = cpu.ime && self.bus.ie & self.bus.iflag & 0x1F != 0;
                let frame_done = self.bus.ppu.frame_ready
                    || self.bus.cycles - start >= FRAME_CYCLES * 2
                    || (!self.bus.ppu.on() && self.bus.cycles - start >= FRAME_CYCLES);
                if irq || frame_done {
                    self.cpu = cpu.clone();
                    break 'turns;
                }
                for _ in 0..*m {
                    self.bus.tick();
                }
                skipped += u64::from(*m);
            }
        }
        self.wait.last = turn;
        self.wait.skipped += skipped;
        skipped
    }

    pub fn frame_xrgb(&self, out: &mut [u32]) {
        out.copy_from_slice(&self.bus.ppu.frame);
    }

    pub fn save_state(&self) -> Vec<u8> {
        let mut out = STATE_MAGIC.to_vec();
        out.extend(bincode::serialize(self).unwrap_or_default());
        // Added after the layout above was fixed, so it comes after it and a state made before
        // it still loads: the sound chip, modelled tick by tick.
        out.extend(bincode::serialize(&self.bus.apu.chip).unwrap_or_default());
        out
    }

    pub fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        if data.len() < 8 || &data[..8] != STATE_MAGIC {
            return Err("not a cBoy Game Boy state".into());
        }
        let mut next: GameBoy = crate::state_decode(&data[8..])?;
        let used = 8 + bincode::serialized_size(&next).map_err(|e| e.to_string())? as usize;
        match data.get(used..).filter(|rest| !rest.is_empty()) {
            Some(rest) => next.bus.apu.chip = crate::state_decode(rest)?,
            // An older state: the sound channels carry over from the way they were saved then.
            None => next.bus.apu.adopt_legacy(),
        }
        next.bus.cart.rom = std::mem::take(&mut self.bus.cart.rom);
        // The cheat codes the app switched on stay on (they're not in the state).
        next.bus.cheats = std::mem::take(&mut self.bus.cheats);
        next.bus.link_loopback = self.bus.link_loopback;
        next.bus.link_connected = self.bus.link_connected;
        // Which console this is comes from the core option, not from the state, and so do the
        // buttons held for a Color's colour scheme.
        next.bus.cgb_hardware = self.bus.cgb_hardware;
        next.bus.ppu.cgb_hardware = self.bus.ppu.cgb_hardware;
        next.bus.ppu.cgb_rev = self.bus.ppu.cgb_rev;
        next.bus.boot_buttons = self.bus.boot_buttons;
        next.compat = self.compat;
        next.console = self.console;
        next.wait.on = self.wait.on;
        // A state from before this game was in colour carries palette memory nothing ever wrote,
        // which every start-up code on a Color does write (its high bytes are never $FF). On a
        // Color it comes back in the colours the game would have had from the start.
        let p = &next.bus.ppu;
        let unwritten = p.bg_pal.iter().chain(&p.obj_pal).all(|b| *b == 0xFF);
        if let (Some(colours), true, false, false) = (self.compat, unwritten, next.bus.cgb, next.bus.boot_on) {
            next.load_compat_palettes(&colours);
        }
        *self = next;
        // A byte the game had half way down the cable is not in the state; start it again.
        self.bus.resume_serial();
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

    /// A Color running a Game Boy cartridge plays it in Game Boy mode, but it is still Color
    /// hardware: the registers the console owns answer, holding what its start-up code left,
    /// while the ones that belong to colour mode stay shut away. Both Mooneye tests that measure
    /// this (boot_hwio-C, unused_hwio-C) are Game Boy cartridges, which is the point of them.
    #[test]
    fn a_game_boy_game_on_a_colour_keeps_the_colours_own_registers() {
        let mut gb = GameBoy::with_model(vec![0u8; 0x8000], Model::Cgb, None).unwrap();
        assert!(gb.bus.cgb_hardware && !gb.bus.cgb, "Game Boy mode on Color hardware");
        let b = &mut gb.bus;
        assert_eq!(b.raw_read(0xFF00), 0xFF, "neither half of the pad selected");
        assert_eq!(b.raw_read(0xFF4F), 0xFE, "the video bank register, stuck on the first bank");
        assert_eq!((b.raw_read(0xFF68), b.raw_read(0xFF6A)), (0xC8, 0xD0), "the palette pointers");
        assert_eq!((b.raw_read(0xFF72), b.raw_read(0xFF73), b.raw_read(0xFF75)), (0x00, 0x00, 0x8F));
        assert_eq!((b.raw_read(0xFF76), b.raw_read(0xFF77)), (0x00, 0x00), "no sound yet");
        // Two of the scratch bytes keep whatever is written; the third keeps three bits of it.
        b.raw_write(0xFF72, 0x5A);
        b.raw_write(0xFF73, 0xA5);
        b.raw_write(0xFF75, 0xFF);
        assert_eq!((b.raw_read(0xFF72), b.raw_read(0xFF73), b.raw_read(0xFF75)), (0x5A, 0xA5, 0xFF));
        b.raw_write(0xFF75, 0x00);
        assert_eq!(b.raw_read(0xFF75), 0x8F);
        // Colour mode's own registers are not there: speed, the copier, the palettes behind the
        // pointers, sprite order and the work-RAM bank.
        for addr in [0xFF4Du16, 0xFF55, 0xFF69, 0xFF6B, 0xFF6C, 0xFF70] {
            assert_eq!(b.raw_read(addr), 0xFF, "{addr:04X} belongs to colour mode");
        }
        // And the game cannot reach the second video bank by writing the bank register.
        b.raw_write(0xFF4F, 0x01);
        assert_eq!(b.raw_read(0xFF4F), 0xFE);
        b.raw_write(0x8000, 0xAB);
        assert_eq!(b.ppu.vram[0], 0xAB, "still the first bank");
    }

    /// A black-and-white cartridge with this title, from Nintendo or not.
    fn mono_cart(title: &[u8], nintendo: bool) -> Vec<u8> {
        let mut rom = vec![0u8; 0x8000];
        rom[0x134..0x134 + title.len()].copy_from_slice(title);
        rom[0x14B] = if nintendo { 0x01 } else { 0x00 };
        rom
    }

    /// The palettes in palette memory, as the start-up code's three runs of eight bytes.
    fn palette_memory(gb: &GameBoy) -> [Vec<u8>; 3] {
        let p = &gb.bus.ppu;
        [p.bg_pal[..8].to_vec(), p.obj_pal[..8].to_vec(), p.obj_pal[8..16].to_vec()]
    }

    /// What a Color's start-up code leaves for a black-and-white game when the game itself
    /// starts (Pan Docs, "Power Up Sequence"): Game Boy mode, its colours for the game in
    /// palette memory, the Game Boy's object order, and in B the title sum it looked the game up
    /// by - but only for Nintendo's games, which are the only ones it looks up.
    #[test]
    fn a_colour_leaves_a_black_and_white_game_its_colours_and_registers() {
        let gb = GameBoy::with_model(mono_cart(b"TETRIS", true), Model::Cgb, None).unwrap();
        assert!(gb.bus.cgb_hardware && !gb.bus.cgb && !gb.bus.ppu.cgb, "Game Boy mode on a Color");
        let c = &gb.cpu;
        assert_eq!((c.a, c.f, c.b, c.c, c.d, c.e, c.h, c.l), (0x11, 0x80, 0xDB, 0x00, 0x00, 0x08, 0x00, 0x7C));
        let tetris = compat::picked(&mono_cart(b"TETRIS", true));
        assert_eq!(palette_memory(&gb), tetris.bytes().map(|r| r.to_vec()));
        assert_eq!(tetris.bg[1], 0xFFFF00, "Tetris is yellow and red");
        assert_eq!((gb.bus.ppu.opri, gb.bus.ppu.bcps, gb.bus.ppu.ocps), (1, 0x88, 0x90));
        // The other seven background palettes are left white, as its fade leaves them.
        assert!(gb.bus.ppu.bg_pal[8..].chunks(2).all(|c| c == [0xFF, 0x7F]));
        // Someone else's game: no title sum in B, and the default colours.
        let other = GameBoy::with_model(mono_cart(b"TETRIS", false), Model::Cgb, None).unwrap();
        assert_eq!(other.cpu.b, 0);
        assert_eq!(palette_memory(&other), compat::DEFAULT.bytes().map(|r| r.to_vec()));
        // The two titles it draws the Game Boy logo's map for: the map, and HL left past it.
        let x = GameBoy::with_model(mono_cart(b"X", true), Model::Cgb, None).unwrap();
        assert_eq!((x.cpu.b, x.cpu.h, x.cpu.l), (0x58, 0x99, 0x1A));
        assert_eq!(x.bus.ppu.vram[0x1904..0x1911], [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 0x19]);
        assert_eq!(x.bus.ppu.vram[0x1924..0x1930], [13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24]);
        assert!(gb.bus.ppu.vram[0x1800..0x2000].iter().all(|b| *b == 0), "no map for any other game");
    }

    /// An Advance's start-up code ends on one more `inc b`, so B is one past the Color's and the
    /// flags are that instruction's (Pan Docs).
    #[test]
    fn an_advance_leaves_b_one_further_on() {
        let regs = |title: &[u8]| {
            let gb = GameBoy::with_model(mono_cart(title, true), Model::Agb, None).unwrap();
            (gb.cpu.b, gb.cpu.f)
        };
        assert_eq!(regs(b"TETRIS"), (0xDC, 0x00));
        // $0F + 1 carries out of the low four bits; $FF + 1 is zero as well.
        assert_eq!(regs(&[0x0F]), (0x10, 0x20));
        assert_eq!(regs(&[0xFF]), (0x00, 0xA0));
        let gb = GameBoy::with_model(mono_cart(b"TETRIS", false), Model::Agb, None).unwrap();
        assert_eq!((gb.cpu.b, gb.cpu.f), (0x01, 0x00), "someone else's game, as Mooneye boot_regs-A has it");
    }

    /// Holding a button combination picks one of the twelve schemes instead of the game's own,
    /// and changes nothing else. A colour game on a Color, and any game on a Game Boy, have no
    /// scheme to pick, so they are left exactly as they were.
    #[test]
    fn a_button_scheme_replaces_the_colours_and_nothing_else() {
        let mut gb = GameBoy::with_model(mono_cart(b"TETRIS", true), Model::Cgb, None).unwrap();
        let rest = |gb: &GameBoy| {
            let c = &gb.cpu;
            ([c.a, c.f, c.b, c.c, c.d, c.e, c.h, c.l], c.sp, c.pc, gb.bus.ppu.opri, gb.bus.ppu.bcps, gb.bus.ppu.ocps, gb.bus.cgb)
        };
        let before = rest(&gb);
        for pick in &compat::PICKS {
            gb.pick_colours(pick);
            assert_eq!(palette_memory(&gb), pick.palettes.bytes().map(|r| r.to_vec()));
        }
        assert_eq!(rest(&gb), before);
        let mut colour = cgb_machine();
        let colour_before = (colour.bus.ppu.bg_pal.clone(), colour.bus.ppu.obj_pal.clone());
        colour.pick_colours(&compat::PICKS[3]);
        assert_eq!((colour.bus.ppu.bg_pal.clone(), colour.bus.ppu.obj_pal.clone()), colour_before);
        let mut dmg = GameBoy::with_model(mono_cart(b"TETRIS", true), Model::Dmg, None).unwrap();
        dmg.pick_colours(&compat::PICKS[3]);
        assert_eq!(dmg.bus.ppu.bg_pal, vec![0xFF; 64], "a Game Boy has no palette memory to fill");
    }

    /// A save state from before a black-and-white game was in colour - made on a Game Boy, or by
    /// an older cBoy, so its palette memory was never written - loads on a Color in the colours
    /// the game has there, not blank white. A state that has colours of its own keeps them, and
    /// a Game Boy loading any state stays a Game Boy.
    #[test]
    fn an_old_state_comes_back_in_colour_on_a_colour() {
        let rom = mono_cart(b"TETRIS", true);
        let old = GameBoy::with_model(rom.clone(), Model::Dmg, None).unwrap().save_state();
        let mut colour = GameBoy::with_model(rom.clone(), Model::Cgb, None).unwrap();
        colour.pick_colours(&compat::PICKS[3]);
        colour.load_state(&old).unwrap();
        assert_eq!(palette_memory(&colour), compat::PICKS[3].palettes.bytes().map(|r| r.to_vec()));
        let coloured = colour.save_state();
        let mut other = GameBoy::with_model(rom.clone(), Model::Cgb, None).unwrap();
        other.load_state(&coloured).unwrap();
        assert_eq!(palette_memory(&other), compat::PICKS[3].palettes.bytes().map(|r| r.to_vec()), "the state's own colours");
        let mut dmg = GameBoy::with_model(rom, Model::Dmg, None).unwrap();
        dmg.load_state(&coloured).unwrap();
        assert!(!dmg.bus.cgb_hardware && dmg.compat.is_none());
    }

    /// A Color's own start-up file runs in colour mode, puts the console into Game Boy mode
    /// itself through KEY0, and can still write the palettes and the object order afterwards,
    /// for as long as it is mapped. Once it is gone, a game can do none of that. Played here by a
    /// few instructions in a 2304-byte start-up file of our own.
    #[test]
    fn a_colour_start_up_file_switches_the_console_into_game_boy_mode() {
        let mut boot = vec![0u8; 0x900];
        let code = [
            0x3E, 0x04, 0xE0, 0x4C, // LD A,4 / LDH (KEY0),A: Game Boy mode
            0x3E, 0x01, 0xE0, 0x6C, // LD A,1 / LDH (OPRI),A
            0x3E, 0x80, 0xE0, 0x68, // LD A,$80 / LDH (BCPS),A
            0x3E, 0x1F, 0xE0, 0x69, // LD A,$1F / LDH (BCPD),A: colour 0 red...
            0x3E, 0x00, 0xE0, 0x69, // ...and its high byte
            0xF0, 0x00, 0x47, // LDH A,(P1) / LD B,A: the pad, as it reads the buttons
            0x3E, 0x11, 0xE0, 0x50, // LD A,$11 / LDH ($FF50),A: hand over
        ];
        boot[..code.len()].copy_from_slice(&code);
        let mut gb = GameBoy::with_model(mono_cart(b"TETRIS", true), Model::Cgb, Some(boot)).unwrap();
        assert!(gb.bus.cgb && gb.bus.boot_on, "the start-up file starts in colour mode");
        gb.pick_colours(&compat::PICKS[0]); // Up
        gb.bus.write(0xFF00, 0x20); // the d-pad half, as the start-up code would select it
        while gb.bus.boot_on {
            gb.cpu.step(&mut gb.bus);
        }
        assert!(!gb.bus.cgb && !gb.bus.ppu.cgb && gb.bus.cgb_hardware, "Game Boy mode, on a Color");
        assert_eq!((gb.bus.ppu.bg_pal[0], gb.bus.ppu.bg_pal[1], gb.bus.ppu.opri), (0x1F, 0x00, 1));
        assert_eq!(gb.cpu.b & 0x0F, 0x0B, "the start-up code read Up off the pad");
        // The game cannot reach any of it.
        gb.bus.write(0xFF4C, 0x00);
        assert!(!gb.bus.cgb, "KEY0 is locked once the start-up code is gone");
        gb.bus.write(0xFF68, 0x80);
        gb.bus.write(0xFF69, 0x55);
        assert_eq!(gb.bus.ppu.bg_pal[0], 0x1F, "palette memory is shut in Game Boy mode");
        assert_eq!(gb.bus.read(0xFF00) & 0x0F, 0x0F, "and the start-up code's buttons are let go");
    }

    /// On a Game Boy none of those registers exist at all (Mooneye boot_hwio-dmgABCmgb).
    #[test]
    fn a_game_boy_console_has_none_of_the_colours_registers() {
        let mut gb = GameBoy::with_model(vec![0u8; 0x8000], Model::Dmg, None).unwrap();
        assert!(!gb.bus.cgb_hardware);
        let b = &mut gb.bus;
        assert_eq!(b.raw_read(0xFF00), 0xCF, "a Game Boy leaves both halves of the pad selected");
        for addr in [0xFF4Fu16, 0xFF68, 0xFF6A, 0xFF72, 0xFF73, 0xFF75, 0xFF76, 0xFF77] {
            b.raw_write(addr, 0x5A);
            assert_eq!(b.raw_read(addr), 0xFF, "{addr:04X} is not on a Game Boy");
        }
    }

    /// $FF76 and $FF77 are what the four sound channels are putting out, two channels to a byte.
    #[test]
    fn a_colour_reads_the_sound_chips_output_back() {
        let mut gb = GameBoy::with_model(vec![0u8; 0x8000], Model::Cgb, None).unwrap();
        let b = &mut gb.bus;
        b.raw_write(0xFF30, 0x9C); // the first two samples of the wave channel
        b.raw_write(0xFF1A, 0x80); // channel 3 on
        b.raw_write(0xFF1C, 0x20); // at full volume
        b.raw_write(0xFF1D, 0xFF); // the shortest period
        b.raw_write(0xFF1E, 0x87); // start it
        // It starts reading three ticks late, and the first sample it reads is the second one
        // (the low half of $9C); until then it still puts out what it had before.
        assert_eq!(b.raw_read(0xFF77), 0x00, "nothing read yet");
        b.tick();
        b.tick();
        assert_eq!(b.raw_read(0xFF77), 0x0C, "channel 3 is the low four bits");
        assert_eq!(b.raw_read(0xFF76), 0x00, "channels 1 and 2 are still silent");
        b.raw_write(0xFF26, 0x00); // sound off
        assert_eq!((b.raw_read(0xFF76), b.raw_read(0xFF77)), (0x00, 0x00));
    }

    /// Which console it is comes from the player's setting, not from the save, so loading a
    /// state must not turn a Color back into a Game Boy half way through a game.
    #[test]
    fn which_console_it_is_survives_a_loaded_state() {
        let gb = GameBoy::with_model(vec![0u8; 0x8000], Model::Cgb, None).unwrap();
        let state = gb.save_state();
        let mut other = GameBoy::with_model(vec![0u8; 0x8000], Model::Cgb, None).unwrap();
        other.load_state(&state).unwrap();
        assert!(other.bus.cgb_hardware);
        assert_eq!(other.bus.raw_read(0xFF4F), 0xFE);
    }

    /// A game that keeps sending down the cable with nothing plugged in finished every transfer
    /// at once and piled the bytes up for ever (470 MB an hour in a spin loop, enough for the
    /// phone to shut the app down). The log stops at its limit, keeping what came first, which is
    /// what the test ROMs report through.
    #[test]
    fn the_serial_log_stops_growing_with_nothing_plugged_in() {
        let mut gb = GameBoy::new(vec![0u8; 0x8000]).unwrap();
        for i in 0..crate::bus::SERIAL_LOG_MAX * 2 {
            gb.bus.write(0xFF01, if i == 0 { 0x11 } else { 0x5A });
            gb.bus.write(0xFF02, 0x81);
            // One byte at a time, the way a game does: the port is a shift register, so a game
            // that wrote the next byte mid-transfer would be writing over the one in flight.
            while gb.bus.link_busy() {
                gb.bus.tick();
            }
        }
        assert_eq!(gb.bus.serial_out.len(), crate::bus::SERIAL_LOG_MAX);
        assert_eq!(gb.bus.serial_out[0], 0x11, "the first bytes sent are the ones kept");
        assert!(gb.bus.serial_out[1..].iter().all(|b| *b == 0x5A));
    }

    /// A transfer takes the time a real cable takes: eight bits at 8192 bits a second. It used
    /// to finish inside the write that started it, so a game got its interrupt before the
    /// instruction after it had run.
    #[test]
    fn a_byte_down_the_cable_takes_a_real_cables_time() {
        let mut gb = GameBoy::new(vec![0u8; 0x8000]).unwrap();
        gb.bus.iflag = 0;
        gb.bus.write(0xFF01, 0x5A);
        let started = gb.bus.cycles;
        gb.bus.write(0xFF02, 0x81);
        assert_eq!(gb.bus.raw_read(0xFF02) & 0x80, 0x80, "still going");
        assert_eq!(gb.bus.iflag & crate::bus::INT_SERIAL, 0, "no interrupt yet");
        while gb.bus.iflag & crate::bus::INT_SERIAL == 0 && gb.bus.cycles - started < 20_000 {
            gb.bus.tick();
        }
        let took = gb.bus.cycles - started;
        // The port's clock is a tap of the console's own counter, so the first bit is however
        // much of a bit is left when the game starts the transfer: a byte takes a whole byte's
        // time, give or take that one bit.
        assert!(took > crate::bus::SERIAL_BYTE_CYCLES - 512 && took <= crate::bus::SERIAL_BYTE_CYCLES + 8,
            "{took} ticks");
        assert_eq!(gb.bus.sb, 0xFF, "nothing attached, so all 1s come back");
        assert_eq!(gb.bus.raw_read(0xFF02) & 0x80, 0, "and the start bit falls");
    }

    /// The cable plugged into itself (the Loopback setting some test programs want) reads back
    /// exactly what was sent, and still takes the cable's time over it.
    #[test]
    fn loopback_reads_back_what_was_sent() {
        let mut gb = GameBoy::new(vec![0u8; 0x8000]).unwrap();
        gb.bus.link_loopback = true;
        gb.bus.write(0xFF01, 0x3C);
        gb.bus.write(0xFF02, 0x81);
        for _ in 0..crate::bus::SERIAL_BYTE_CYCLES / 4 + 1 {
            gb.bus.tick();
        }
        assert_eq!(gb.bus.sb, 0x3C);
        assert!(!gb.bus.link_busy());
    }

    /// Set to the external clock, a console does nothing by itself: the other end of the cable
    /// is what moves the bits, so with nothing attached the transfer simply stays open. A game
    /// waiting to be linked to must not be handed a byte that never came.
    #[test]
    fn the_external_clock_waits_for_the_other_end() {
        let mut gb = GameBoy::new(vec![0u8; 0x8000]).unwrap();
        gb.bus.iflag = 0;
        gb.bus.write(0xFF01, 0x42);
        gb.bus.write(0xFF02, 0x80);
        for _ in 0..4000 {
            gb.bus.tick();
        }
        assert_eq!(gb.bus.sb, 0x42, "nothing has shifted");
        assert_eq!(gb.bus.iflag & crate::bus::INT_SERIAL, 0);
        assert_eq!(gb.bus.raw_read(0xFF02) & 0x80, 0x80, "and the game is still waiting");
        assert!(gb.bus.link_busy());
        // Until the other end clocks it, and then both ends have each other's byte.
        assert_eq!(gb.bus.link_clock(0x99), Some(0x42));
        assert_eq!(gb.bus.sb, 0x99);
        assert_eq!(gb.bus.iflag & crate::bus::INT_SERIAL, crate::bus::INT_SERIAL);
        assert!(!gb.bus.link_busy());
    }

    /// A Color can ask for a faster cable (SC bit 1), which a Game Boy has no bit for.
    #[test]
    fn a_colour_can_ask_for_the_fast_cable() {
        let mut slow = cgb_machine();
        slow.bus.write(0xFF02, 0x81);
        let mut fast = cgb_machine();
        fast.bus.write(0xFF02, 0x83);
        assert_eq!(fast.bus.raw_read(0xFF02) & 0x02, 0x02, "a Color reads the bit back");
        let took = |gb: &mut GameBoy| {
            let start = gb.bus.cycles;
            while gb.bus.link_busy() && gb.bus.cycles - start < 20_000 {
                gb.bus.tick();
            }
            gb.bus.cycles - start
        };
        let (slow_took, fast_took) = (took(&mut slow), took(&mut fast));
        assert!(fast_took * 8 < slow_took, "fast {fast_took} ticks against slow {slow_took}");
        // A Game Boy has no such bit: it reads as 1 whatever is written, and changes nothing.
        let mut dmg = GameBoy::with_model(vec![0u8; 0x8000], Model::Dmg, None).unwrap();
        dmg.bus.write(0xFF02, 0x81);
        assert_eq!(dmg.bus.raw_read(0xFF02) & 0x02, 0x02, "no fast-cable bit on a Game Boy");
        assert_eq!(took(&mut dmg), slow_took);
    }

    /// A save state taken with a byte half way down the cable does not carry the half-shifted
    /// byte (adding to the state would stop every state players already have from loading), so
    /// the transfer starts again on load instead of being left open for ever.
    #[test]
    fn a_state_saved_mid_byte_finishes_its_transfer_after_loading() {
        let mut gb = GameBoy::new(vec![0u8; 0x8000]).unwrap();
        gb.bus.write(0xFF01, 0x5A);
        gb.bus.write(0xFF02, 0x81);
        for _ in 0..200 {
            gb.bus.tick();
        }
        assert!(gb.bus.link_busy(), "saved mid-byte");
        let state = gb.save_state();
        let mut other = GameBoy::new(vec![0u8; 0x8000]).unwrap();
        other.load_state(&state).unwrap();
        assert!(other.bus.link_busy(), "and the transfer is still the game's");
        other.bus.iflag = 0;
        for _ in 0..crate::bus::SERIAL_BYTE_CYCLES / 4 + 1 {
            other.bus.tick();
        }
        assert_eq!(other.bus.iflag & crate::bus::INT_SERIAL, crate::bus::INT_SERIAL, "it finished");
        assert_eq!(other.bus.raw_read(0xFF02) & 0x80, 0);
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
