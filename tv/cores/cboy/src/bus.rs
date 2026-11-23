//! The memory map and everything clocked with the CPU. Every CPU memory access goes through
//! `read`/`write`, which first run one M-cycle of the rest of the machine, so instruction timing
//! falls out of the CPU doing its accesses in the right order.

use serde::{Deserialize, Serialize};

use crate::apu::Apu;
use crate::cart::Cart;
use crate::ppu::{CgbRev, Land, Ppu};
use crate::timer::Timer;

pub const INT_VBLANK: u8 = 1;
pub const INT_STAT: u8 = 2;
pub const INT_TIMER: u8 = 4;
pub const INT_SERIAL: u8 = 8;
pub const INT_JOYPAD: u8 = 16;

/// How much of the serial port's output is kept. Only the test ROMs read it, and their whole
/// report is a few hundred bytes; a game that keeps sending with no link partner finishes every
/// transfer at once and used to pile up hundreds of thousands of bytes a minute, which on a phone
/// ends with the app being shut down for using too much memory. Past this the bytes are dropped,
/// oldest kept, so a test's report is never truncated at the front.
pub const SERIAL_LOG_MAX: usize = 8192;

/// How long one byte takes on a Game Boy's own link clock: 8192 bits a second, so 512 ticks a
/// bit and eight bits to the byte. A Color asking for its fast clock takes a thirty-second of
/// this, and double speed halves whichever one is in use.
pub const SERIAL_BYTE_CYCLES: u64 = 8 * 512;

/// How far ahead of DIV's count the link port's own count runs (see `serial_clock_high`).
const SERIAL_CLOCK_PHASE: u16 = 4;

#[derive(Clone, Serialize, Deserialize)]
pub struct Bus {
    pub cart: Cart,
    pub ppu: Ppu,
    pub apu: Apu,
    pub timer: Timer,
    pub wram: Vec<u8>,
    pub hram: Vec<u8>,
    pub ie: u8,
    pub iflag: u8,
    /// Which half of the pad $FF00 is reading, as the game last set it. A Color's start-up code
    /// leaves neither half selected, a Game Boy's leaves both (Mooneye boot_hwio-C against
    /// boot_hwio-dmgABCmgb).
    pub joyp_select: u8,
    /// Pressed buttons, bit set = pressed: 0 A, 1 B, 2 Select, 3 Start, 4 Right, 5 Left, 6 Up, 7 Down.
    pub buttons: u8,
    pub sb: u8,
    sc: u8,
    /// What the game sent out of the serial port (the test ROMs report through it), to at most
    /// `SERIAL_LOG_MAX` bytes.
    #[serde(skip)]
    pub serial_out: Vec<u8>,
    dma_src: u16,
    dma_pos: u16,
    dma_active: bool,
    dma_delay: u8,
    dma_reg: u8,
    /// A restarted OAM DMA: the previous transfer keeps OAM blocked until the new one starts.
    #[serde(default)]
    dma_prev_block: bool,
    /// The PPU shuts the CPU out of OAM and VRAM while it uses them (off only for the tests that
    /// predate it; not saved).
    #[serde(skip, default = "yes")]
    pub ppu_locks: bool,
    apu_div_bit: bool,
    pub cycles: u64,
    /// Game Boy Color state (unused in DMG mode).
    pub cgb: bool,
    /// The console is a Color or an Advance. True whenever `cgb` is, and also when a Game Boy
    /// cartridge puts a Color into Game Boy mode: a handful of registers belong to the console
    /// rather than to the mode, so a Game Boy game on a Color still sees them (Mooneye
    /// boot_hwio-C, unused_hwio-C). Which console it is comes from the core option, not from the
    /// save, so it is set when the machine starts and kept across a loaded state.
    #[serde(skip)]
    pub cgb_hardware: bool,
    /// The Color's own scratch bytes $FF72, $FF73 and $FF75 (that one keeps only bits 4-6).
    /// Not in a save state: adding a field to one would stop every state players already have
    /// from loading, and nothing reads these back across a save.
    #[serde(skip)]
    scratch: [u8; 3],
    pub double_speed: bool,
    pub speed_prepare: bool,
    pub vbk: u8,
    pub svbk: u8,
    hdma_src: u16,
    hdma_dst: u16,
    hdma_blocks: u8,
    hdma_active: bool,
    /// Lines for the libretro log, drained each frame (see `log`).
    #[serde(skip)]
    pub events: Vec<String>,
    #[serde(skip)]
    log_count: u32,
    /// Interrupts taken, by bit (VBlank, STAT, timer, serial, joypad); for the lockup log.
    #[serde(default)]
    pub irq_taken: [u32; 5],
    /// The CPU read something since this was last cleared that can change without it writing
    /// (an I/O register, video memory, cartridge RAM). Not saved.
    #[serde(skip)]
    pub loop_unstable: bool,
    /// The CPU's writes so far. Not saved.
    #[serde(skip)]
    pub loop_writes: u64,
    /// The Super Game Boy, when running as one (core option).
    #[serde(default)]
    pub sgb: Option<Box<crate::sgb::Sgb>>,
    /// Boot ROM image and whether it is still mapped over $0000 (core option).
    #[serde(default)]
    pub boot: Vec<u8>,
    #[serde(default)]
    pub boot_on: bool,
    /// Buttons held down for as long as the boot ROM runs, on top of the player's own: how a
    /// player picks one of a Color's colour schemes for a black-and-white game, which its start-up
    /// code reads off the pad while the logo is up (see `GameBoy::pick_colours`). A core option,
    /// so not saved.
    #[serde(skip)]
    pub boot_buttons: u8,
    /// Link cable loopback (core option): a transfer reads back the byte sent.
    #[serde(skip)]
    pub link_loopback: bool,
    /// A real partner is on the other end of the cable (the app has a two-phone session going).
    /// With nothing attached an internally clocked transfer finishes on its own reading 1s, the
    /// way a loose cable does; with a partner it holds open until the partner answers.
    #[serde(skip)]
    pub link_connected: bool,
    /// Bytes this machine has put on the cable, oldest first, for the cable to carry over.
    /// Only ever filled while a partner is attached, so a game sending into a loose cable
    /// cannot pile bytes up here.
    #[serde(skip)]
    pub link_out: std::collections::VecDeque<u8>,
    /// The transfer the port is shifting: bits still to clock, 8 down to 0 (0 = nothing going
    /// on), the byte going out (latched when it started, which is what the partner is told) and
    /// the byte coming in, whose top bit lands in SB on each clock.
    ///
    /// None of this is in a save state: adding fields to one would stop every state players
    /// already have from loading. A state taken half way through a byte starts that byte again
    /// when it is loaded (`resume_serial`), which costs at most one byte's worth of cable time.
    #[serde(skip)]
    serial_bits: u8,
    #[serde(skip)]
    serial_tx: u8,
    #[serde(skip)]
    serial_rx: u8,
    /// All eight bits are clocked and the port is holding the transfer open, waiting for the
    /// partner's answer. Only ever happens with a partner attached.
    #[serde(skip)]
    serial_waiting: bool,
    /// Picture dots since this machine was made or loaded: real time, which `cycles` is not in
    /// double speed. Two Game Boys on one phone are kept in step by it (`dual.rs`).
    #[serde(skip)]
    pub dots: u64,
    last_mode: u8,
    /// Set by the CPU before each instruction so a hardware event can name where it came from.
    #[serde(skip)]
    pub trace_pc: u16,
    #[serde(skip)]
    pub trace_bank: u16,
    /// Previous values of the five copy registers, so a change that does not start a copy
    /// still gets a line (the phone rewrites them every loop).
    #[serde(skip)]
    hdma_watch: [u8; 5],
    /// A picture register write's step that lands one dot into the next M-cycle (see
    /// `landing`). Never across a frame boundary, where states are taken, so not saved.
    #[serde(skip)]
    land_next: Option<(u8, Land)>,
    /// The cheat codes switched on (cheats.rs). Not part of a save state: the app sets them.
    #[serde(skip)]
    pub cheats: crate::cheats::Cheats,
}

impl Bus {
    pub fn new(cart: Cart) -> Bus {
        let mut timer = Timer::default();
        // DIV as the DMG boot ROM leaves it on entry to the cartridge.
        timer.counter = 0xABC8;
        Bus {
            cart,
            ppu: Ppu::default(),
            apu: Apu::default(),
            timer,
            wram: vec![0; 0x8000],
            hram: vec![0; 0x7F],
            ie: 0,
            iflag: 0xE1,
            joyp_select: 0x00,
            buttons: 0,
            sb: 0,
            sc: 0x7E,
            serial_out: Vec::new(),
            sgb: None,
            boot: Vec::new(),
            boot_on: false,
            boot_buttons: 0,
            link_loopback: false,
            link_connected: false,
            link_out: std::collections::VecDeque::new(),
            serial_bits: 0,
            serial_tx: 0,
            serial_rx: 0xFF,
            serial_waiting: false,
            dots: 0,
            dma_src: 0,
            dma_pos: 0,
            dma_active: false,
            dma_delay: 0,
            dma_reg: 0xFF,
            dma_prev_block: false,
            ppu_locks: true,
            apu_div_bit: false,
            cycles: 0,
            cgb: false,
            cgb_hardware: false,
            scratch: [0; 3],
            double_speed: false,
            speed_prepare: false,
            vbk: 0,
            svbk: 1,
            hdma_src: 0,
            hdma_dst: 0x8000,
            hdma_blocks: 0,
            hdma_active: false,
            events: Vec::new(),
            log_count: 0,
            irq_taken: [0; 5],
            loop_unstable: false,
            loop_writes: 0,
            last_mode: 0,
            trace_pc: 0,
            trace_bank: 0,
            hdma_watch: [0, 0, 0, 0x80, 0xFF],
            land_next: None,
            cheats: crate::cheats::Cheats::default(),
        }
    }

    /// The written cheat codes (GameShark, CodeBreaker, raw), at the start of a picture's blank,
    /// when the device steps in. Not while the console's start-up program runs.
    pub fn apply_cheat_pokes(&mut self) {
        if self.boot_on || self.cheats.pokes.is_empty() {
            return;
        }
        for i in 0..self.cheats.pokes.len() {
            let p = self.cheats.pokes[i];
            match p.target {
                crate::cheats::Target::CartBank(bank) => self.cart.write_ram_in_bank(bank, p.addr, p.value),
                crate::cheats::Target::WramBank(bank) if self.cgb => {
                    let bank = usize::from((bank & 7).max(1));
                    self.wram[bank * 0x1000 + usize::from(p.addr & 0x0FFF)] = p.value;
                }
                _ => self.raw_write(p.addr, p.value),
            }
        }
    }

    /// A Color's start-up code is still running: it owns a few registers no game can touch.
    fn colour_start_up(&self) -> bool {
        self.cgb_hardware && self.boot_on
    }

    pub fn set_buttons(&mut self, pressed: u8) {
        let newly = pressed & !self.buttons;
        self.buttons = pressed;
        if newly != 0 {
            self.iflag |= INT_JOYPAD;
        }
    }

    fn joyp(&self) -> u8 {
        let held = self.buttons | if self.boot_on { self.boot_buttons } else { 0 };
        let mut low = 0x0F;
        if self.joyp_select & 0x20 == 0 {
            low &= !(held & 0x0F);
        }
        if self.joyp_select & 0x10 == 0 {
            low &= !(held >> 4);
        }
        if self.joyp_select == 0x30 {
            if let Some(sgb) = &self.sgb {
                low = sgb.joyp_idle();
            }
        }
        0xC0 | self.joyp_select | low
    }

    /// The 4 KiB a Super Game Boy VRAM transfer reads: the tiles the screen shows, in order.
    fn sgb_screen_data(&self) -> Vec<u8> {
        let map = if self.ppu.lcdc & 0x08 != 0 { 0x9C00u16 } else { 0x9800 };
        let mut out = Vec::with_capacity(4096);
        for i in 0..256u16 {
            let at = map + (i / 20) * 32 + i % 20;
            let tile = self.ppu.vram_read(at);
            let base = self.ppu.bg_tile_addr(tile);
            for b in 0..16 {
                out.push(self.ppu.vram_read(base + b));
            }
        }
        out
    }

    /// A debugging line for the frontend's log (libretro log), capped so a looping game cannot
    /// flood it.
    pub fn log(&mut self, line: String) {
        if self.log_count < 400 {
            self.log_count += 1;
            self.events.push(line);
        }
    }

    /// The link port's own clock, high or low right now.
    ///
    /// It is counted down from the console's clock and never restarted, so where in that count
    /// a transfer starts is what decides how long its first bit takes, and the port's clock and
    /// the console's never drift apart (Mooneye boot_sclk_align). Bit 8 is 8192 bits a second,
    /// the only speed a Game Boy has; a Color that asks for the fast clock (SC bit 1) taps bit 3
    /// instead, 32 times quicker. Double speed doubles both, because the count itself runs twice
    /// as fast then.
    ///
    /// The port's count runs [`SERIAL_CLOCK_PHASE`] ticks ahead of the one DIV is read from. The
    /// two boot tests together say so: boot_div pins where DIV's steps fall against the CPU,
    /// boot_sclk_align pins when a transfer started straight after boot finishes, and the gap
    /// between the two answers is that one step.
    fn serial_clock_high(&self) -> bool {
        let tap = if self.cgb && self.sc & 0x02 != 0 { 3 } else { 8 };
        (self.timer.counter.wrapping_add(SERIAL_CLOCK_PHASE) >> tap) & 1 != 0
    }

    /// Starts (or restarts) the transfer SC bit 7 has just asked for.
    fn serial_start(&mut self) {
        self.serial_bits = 8;
        self.serial_waiting = false;
        self.serial_tx = self.sb;
        // What comes back: the partner's byte once it answers, the byte just sent if the cable
        // is plugged into itself, and all 1s if nothing is attached at all.
        self.serial_rx = if self.link_loopback { self.sb } else { 0xFF };
        // This machine drives the clock, so the partner is told straight away: it has to have
        // this byte before it can answer with its own. The cap is for a game that restarts the
        // transfer in a tight loop: the cable is emptied every frame, so it is never reached in
        // normal play, and bytes past it would be ones the game had already thrown away.
        if self.link_connected && self.sc & 0x01 != 0 && self.link_out.len() < 1024 {
            self.link_out.push_back(self.sb);
        }
    }

    /// One clock pulse of an internally clocked transfer: a bit out, a bit in.
    fn serial_pulse(&mut self) {
        // Only the side driving the clock shifts by itself. The other side waits to be clocked.
        if self.serial_bits == 0 || self.sc & 0x81 != 0x81 {
            return;
        }
        self.sb = (self.sb << 1) | (self.serial_rx >> 7);
        self.serial_rx <<= 1;
        self.serial_bits -= 1;
        if self.serial_bits == 0 {
            if self.link_connected {
                // The eight bits are clocked, but the byte the partner shifted back is still on
                // its way. Hold the transfer open rather than hand the game a made-up byte.
                self.serial_waiting = true;
            } else {
                self.serial_done();
            }
        }
    }

    /// The transfer is over: the game gets its interrupt and SC bit 7 falls.
    fn serial_done(&mut self) {
        self.serial_bits = 0;
        self.serial_waiting = false;
        self.sc &= 0x7F;
        self.iflag |= INT_SERIAL;
    }

    /// The partner's answer to a byte this machine clocked out. Ignored when nothing is waiting
    /// for one, so a late answer to a byte already given up on cannot land in the next one.
    pub fn link_answer(&mut self, byte: u8) {
        if !self.serial_waiting {
            return;
        }
        self.sb = byte;
        self.serial_done();
    }

    /// The partner's clock, carrying the byte it shifted out. Answers with this machine's own
    /// byte when it had a transfer armed on the external clock, so the partner's transfer can
    /// finish too; answers with nothing when this machine was not expecting anything, which
    /// leaves its own SB alone instead of corrupting a game that is not linking.
    pub fn link_clock(&mut self, byte: u8) -> Option<u8> {
        if self.sc & 0x81 != 0x80 {
            return None;
        }
        let ours = self.sb;
        self.sb = byte;
        self.serial_tx = ours;
        self.serial_done();
        Some(ours)
    }

    /// Whether a transfer is still open: either shifting, or waiting for the partner's answer.
    pub fn link_busy(&self) -> bool {
        self.serial_bits > 0 || self.serial_waiting
    }

    /// All eight bits of a byte this machine clocked out have gone, and the transfer is held
    /// open for the partner's answer.
    pub fn link_waiting(&self) -> bool {
        self.serial_waiting
    }

    /// The byte this machine last put on the cable (what the partner is told). For the tests.
    pub fn link_sent(&self) -> u8 {
        self.serial_tx
    }

    /// Gives up on a partner that is not answering: the transfer finishes reading 1s, the way a
    /// cable that has been pulled out does, instead of leaving the game waiting for ever.
    pub fn link_give_up(&mut self) {
        if self.serial_waiting {
            self.sb = 0xFF;
            self.serial_done();
        }
    }

    /// After a save state is loaded: the state does not carry the half-shifted byte, so a
    /// transfer the game still has open is started again from the top.
    pub fn resume_serial(&mut self) {
        if self.sc & 0x80 != 0 {
            self.serial_start();
        }
    }

    /// Where, inside the M-cycle carrying it, a CPU write to a picture register reaches the
    /// picture hardware: up to two steps, each as (how many of the M-cycle's dots run before it
    /// lands, what lands). 4 is the M-cycle's end, which is where cBoy's registers themselves
    /// change and so where everything the CPU can see, and the Standard drawer, are timed; 5 is
    /// one dot into the next M-cycle. Only the exact drawer looks at the steps (`Ppu::land`).
    ///
    /// This is SameBoy's model of the CPU's write conflicts (Core/sm83_cpu.c, `cycle_write`, read
    /// and not copied; .kiro/notes/docs/CBOY_EXACT_DRAWER.md has the table). A register missing
    /// here lands at the end of the M-cycle, which is SameBoy's default too. Double speed has its
    /// own table there, in half dots, which a whole-dot drawer cannot place, so in double speed
    /// every write lands at the end, as it always has here.
    fn landing(&self, addr: u16, v: u8) -> [Option<(u8, Land)>; 2] {
        if self.double_speed {
            return [None, None];
        }
        if self.cgb_hardware {
            return match addr {
                // A Color takes LCDC whole at the end; changing tile select, either way, glitches
                // a bitplane read on the next dot (`Ppu::bitplane_read`).
                0xFF40 if (self.ppu.lcdc ^ v) & 0x10 != 0 => [Some((4, Land::TileSel(v))), None],
                // A Color (CPU CGB C and earlier) writes a palette register a dot before the end,
                // whole: no old-OR-new dot as on a Game Boy. A CGB D or later (and an Advance)
                // a dot earlier still (SameBoy's GB_CONFLICT_PALETTE_CGB).
                0xFF47..=0xFF49 if self.ppu.cgb_rev == CgbRev::C => [Some((3, Land::Value(v))), None],
                0xFF47..=0xFF49 => [Some((2, Land::Value(v))), None],
                // A Color's WX write wins over the PPU's read of it, which puts it one dot into
                // the next M-cycle.
                0xFF4B => [Some((4, Land::Hold)), Some((5, Land::Value(v)))],
                // SCX, SCY and WY at the end: on a Color, SCY lands two dots after a Game Boy's
                // (the Mealybug PPU documentation).
                _ => [None, None],
            };
        }
        match addr {
            // A Game Boy writes LCDC in two steps a dot apart (see `Land::LcdcFirst`).
            0xFF40 => [Some((2, Land::LcdcFirst(v))), Some((3, Land::Value(v)))],
            // A Game Boy's SCY and SCX reach the fetcher two dots before the end.
            0xFF42 | 0xFF43 => [Some((2, Land::Value(v))), None],
            // A Game Boy palette register reads as the old value OR the new one for a dot, two
            // dots before the end, then as the new one.
            0xFF47..=0xFF49 => [Some((2, Land::Or(v))), Some((3, Land::Value(v)))],
            // WX at the end, and the window's one-pixel-late start is not taken on the next dot.
            0xFF4B => [Some((4, Land::Wx(v))), None],
            _ => [None, None],
        }
    }

    /// One M-cycle of everything but the CPU.
    pub fn tick(&mut self) {
        self.tick_landing(0, [None, None]);
    }

    /// One M-cycle, handing the picture hardware the steps of a write to register `reg` on their
    /// dots (see `landing`).
    fn tick_landing(&mut self, reg: u8, steps: [Option<(u8, Land)>; 2]) {
        self.cycles += 4;
        let serial_clock_was_high = self.serial_clock_high();
        if self.timer.tick_m() {
            self.iflag |= INT_TIMER;
        }
        // The port's own clock, so a transfer this machine drives shifts a bit here.
        if serial_clock_was_high && !self.serial_clock_high() {
            self.serial_pulse();
        }
        // In double speed the CPU, timer and DMA run twice as fast; the PPU, sound and clock do not.
        let dots = if self.double_speed { 2 } else { 4 };
        self.dots += u64::from(dots);
        let mask = if self.double_speed { 0x2000 } else { 0x1000 };
        let bit = self.timer.counter & mask != 0;
        self.apu.cgb = self.cgb_hardware;
        self.apu.double_speed = self.double_speed;
        if self.apu_div_bit && !bit {
            self.apu.frame_step();
        } else if !self.apu_div_bit && bit {
            self.apu.frame_rise();
        }
        self.apu_div_bit = bit;
        self.apu.div_high = bit;
        self.apu.tick(dots);
        for dot in 1..=dots {
            let irq = self.ppu.tick();
            // The step of the last write that lands one dot into this M-cycle, then this one's.
            if dot == 1 {
                if let Some((r, step)) = self.land_next.take() {
                    self.ppu.land(r, step);
                    self.ppu.settle(r);
                }
            }
            for (at, step) in steps.iter().flatten() {
                if u32::from(*at) == dot {
                    self.ppu.land(reg, *step);
                }
            }
            if irq.vblank {
                self.iflag |= INT_VBLANK;
                if self.sgb.as_mut().is_some_and(|s| s.wants_transfer()) {
                    let data = self.sgb_screen_data();
                    if let Some(s) = self.sgb.as_mut() {
                        s.take_transfer(&data);
                    }
                }
            }
            if irq.stat {
                self.iflag |= INT_STAT;
            }
        }
        self.cart.tick(dots);
        let mode = self.ppu.mode();
        if self.hdma_active && mode == 0 && self.last_mode == 3 && self.ppu.ly < 144 {
            self.hdma_block();
            self.hdma_blocks -= 1;
            if self.hdma_blocks == 0 {
                self.hdma_active = false;
            }
        }
        self.last_mode = mode;
        if self.dma_active {
            if self.dma_delay > 0 {
                self.dma_delay -= 1;
                if self.dma_delay == 0 {
                    self.dma_prev_block = false;
                }
            } else {
                let v = self.raw_read(self.dma_src + self.dma_pos);
                self.ppu.oam[self.dma_pos as usize] = v;
                self.dma_pos += 1;
                if self.dma_pos == 0xA0 {
                    self.dma_active = false;
                }
            }
        }
    }

    /// Copies one 16-byte HDMA/GDMA block into VRAM.
    fn hdma_block(&mut self) {
        for _ in 0..16 {
            let v = self.raw_read(self.hdma_src);
            let bank = usize::from(self.vbk & 1) * 0x2000;
            self.ppu.vram[bank + (self.hdma_dst & 0x1FFF) as usize] = v;
            self.hdma_src = self.hdma_src.wrapping_add(1);
            self.hdma_dst = 0x8000 | (self.hdma_dst.wrapping_add(1) & 0x1FFF);
        }
    }

    fn wram_index(&self, addr: u16) -> usize {
        let a = (addr as usize - 0xC000) & 0x1FFF;
        if a < 0x1000 {
            a
        } else {
            let bank = if self.cgb { usize::from(self.svbk.max(1)) } else { 1 };
            bank * 0x1000 + (a - 0x1000)
        }
    }

    fn vram_index(&self, addr: u16) -> usize {
        usize::from(self.vbk & 1) * 0x2000 + (addr - 0x8000) as usize
    }

    /// STOP: on the CGB with KEY1 armed, switches CPU speed.
    pub fn stop(&mut self) {
        self.log(format!("STOP (KEY1 armed: {})", self.speed_prepare));
        if self.cgb && self.speed_prepare {
            self.double_speed = !self.double_speed;
            self.log(format!("speed switch: {}", if self.double_speed { "double" } else { "normal" }));
            self.speed_prepare = false;
        }
        self.timer.write_div();
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        // For spotting a waiting loop (gb::WaitLoop): memory only the CPU's own writes change.
        if !((addr < 0x8000 && !self.boot_on) || (0xC000..0xFE00).contains(&addr) || addr >= 0xFF80) {
            self.loop_unstable = true;
        }
        self.tick();
        if self.dma_active && (self.dma_delay == 0 || self.dma_prev_block) && (0xFE00..0xFF00).contains(&addr) {
            return 0xFF;
        }
        if self.ppu_locks && ((0xFE00..0xFEA0).contains(&addr) && self.ppu.oam_blocked()
            || (0x8000..0xA000).contains(&addr) && self.ppu.vram_blocked())
        {
            return 0xFF;
        }
        self.raw_read(addr)
    }

    pub fn write(&mut self, addr: u16, v: u8) {
        self.loop_writes = self.loop_writes.wrapping_add(1);
        if (0xFF40..=0xFF4B).contains(&addr) {
            // A picture register: the picture hardware sees the write on the dots `landing`
            // gives, and the register itself changes at the end of the M-cycle as before.
            let steps = self.landing(addr, v);
            let reg = addr as u8;
            self.tick_landing(reg, steps);
            #[cfg(feature = "trace")]
            if steps[0].is_none() {
                self.ppu.trace_land(reg, v, self.raw_read(addr));
            }
            self.raw_write(addr, v);
            match steps[1] {
                Some((5, step)) => self.land_next = Some((reg, step)),
                _ => self.ppu.settle(reg),
            }
            return;
        }
        self.tick();
        if self.dma_active && (0xFE00..0xFEA0).contains(&addr) {
            return;
        }
        if self.ppu_locks && ((0xFE00..0xFEA0).contains(&addr) && self.ppu.oam_write_blocked()
            || (0x8000..0xA000).contains(&addr) && self.ppu.vram_write_blocked())
        {
            return;
        }
        self.raw_write(addr, v);
    }

    pub fn raw_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x00FF | 0x0200..=0x08FF if self.boot_on && usize::from(addr) < self.boot.len() => {
                self.boot[usize::from(addr)]
            }
            0x0000..=0x7FFF => {
                // Through any Game Genie codes, which sit on the cartridge (cheats.rs).
                let v = self.cart.read_rom(addr);
                if self.cheats.genie.is_empty() { v } else { self.cheats.read(addr, v) }
            }
            0x8000..=0x9FFF => self.ppu.vram[self.vram_index(addr)],
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xFDFF => self.wram[self.wram_index(addr)],
            0xFE00..=0xFE9F => self.ppu.oam[(addr - 0xFE00) as usize],
            0xFEA0..=0xFEFF => 0x00,
            0xFF00 => self.joyp(),
            0xFF01 => self.sb,
            // A Game Boy has one cable speed, so only the start bit and the clock choice are
            // real and everything between them reads as 1. In colour mode the fast-clock bit is
            // a real bit the game can read back.
            0xFF02 => self.sc | if self.cgb { 0x7C } else { 0x7E },
            0xFF04 => self.timer.div(),
            0xFF05 => self.timer.tima,
            0xFF06 => self.timer.tma,
            0xFF07 => self.timer.tac,
            0xFF0F => self.iflag | 0xE0,
            0xFF10..=0xFF3F => self.apu.read(addr),
            0xFF40 => self.ppu.lcdc,
            0xFF41 => self.ppu.read_stat(),
            0xFF42 => self.ppu.scy,
            0xFF43 => self.ppu.scx,
            0xFF44 => self.ppu.ly_read(),
            0xFF45 => self.ppu.lyc,
            0xFF46 => self.dma_reg,
            0xFF47 => self.ppu.bgp,
            0xFF48 => self.ppu.obp0,
            0xFF49 => self.ppu.obp1,
            0xFF4A => self.ppu.wy,
            0xFF4B => self.ppu.wx,
            0xFF4D if self.cgb => {
                0x7E | if self.double_speed { 0x80 } else { 0 } | u8::from(self.speed_prepare)
            }
            0xFF4F if self.cgb => 0xFE | self.vbk,
            // In Game Boy mode a Color still answers for the video bank, stuck on the first one.
            0xFF4F if self.cgb_hardware => 0xFE,
            0xFF55 if self.cgb => {
                (self.hdma_blocks.wrapping_sub(1) & 0x7F) | if self.hdma_active { 0 } else { 0x80 }
            }
            // The two palette pointers are the console's, so a Game Boy game on a Color reads
            // them back as its start-up code left them; the palettes behind them are not.
            0xFF68 if self.cgb_hardware => self.ppu.bcps | 0x40,
            0xFF69 if self.cgb || self.colour_start_up() => self.ppu.read_pal(false),
            0xFF6A if self.cgb_hardware => self.ppu.ocps | 0x40,
            0xFF6B if self.cgb || self.colour_start_up() => self.ppu.read_pal(true),
            0xFF6C if self.cgb => 0xFE | self.ppu.opri,
            0xFF70 if self.cgb => 0xF8 | self.svbk,
            0xFF72 if self.cgb_hardware => self.scratch[0],
            0xFF73 if self.cgb_hardware => self.scratch[1],
            0xFF75 if self.cgb_hardware => 0x8F | self.scratch[2],
            // The sound chip's current output, which only a Color can read.
            0xFF76 if self.cgb_hardware => self.apu.pcm12(),
            0xFF77 if self.cgb_hardware => self.apu.pcm34(),
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize],
            0xFFFF => self.ie,
            _ => 0xFF,
        }
    }

    pub fn raw_write(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x7FFF => self.cart.write_rom(addr, v),
            0x8000..=0x9FFF => {
                let i = self.vram_index(addr);
                self.ppu.vram[i] = v;
            }
            0xA000..=0xBFFF => self.cart.write_ram(addr, v),
            0xC000..=0xFDFF => {
                let i = self.wram_index(addr);
                self.wram[i] = v;
            }
            0xFE00..=0xFE9F => self.ppu.oam[(addr - 0xFE00) as usize] = v,
            0xFF00 => {
                self.joyp_select = v & 0x30;
                if let Some(sgb) = self.sgb.as_mut() {
                    if let Some(line) = sgb.joyp_write(v) {
                        self.log(line);
                    }
                }
            }
            0xFF50 if self.boot_on && v != 0 => self.boot_on = false,
            0xFF01 => self.sb = v,
            0xFF02 => {
                self.sc = v;
                if v & 0x80 != 0 {
                    // Writing the start bit again part way through a byte starts it over, which
                    // is what the hardware's bit counter does.
                    self.serial_start();
                    // The test ROMs report through this port, and they do not all wait for the
                    // transfer to finish, so the log takes the byte as it is sent rather than
                    // when it lands.
                    if v & 0x01 != 0 && self.serial_out.len() < SERIAL_LOG_MAX {
                        self.serial_out.push(self.sb);
                    }
                } else {
                    // The game called the transfer off.
                    self.serial_bits = 0;
                    self.serial_waiting = false;
                }
            }
            0xFF04 => {
                self.timer.write_div();
                // DIV going to 0 drops the sequencer's bit at once if it was high.
                if self.apu_div_bit {
                    self.apu.frame_step();
                    self.apu_div_bit = false;
                    self.apu.div_high = false;
                }
            }
            0xFF05 => self.timer.write_tima(v),
            0xFF06 => self.timer.write_tma(v),
            0xFF07 => self.timer.write_tac(v),
            0xFF0F => self.iflag = v & 0x1F,
            0xFF10..=0xFF3F => {
                self.apu.addr_bus = addr;
                self.apu.write(addr, v);
            }
            0xFF40 => {
                if self.ppu.write_lcdc(v) {
                    self.iflag |= INT_STAT;
                }
            }
            0xFF41 => self.ppu.write_stat(v),
            0xFF42 => self.ppu.scy = v,
            0xFF43 => self.ppu.scx = v,
            0xFF45 => {
                if self.ppu.write_lyc(v) {
                    self.iflag |= INT_STAT;
                }
            }
            0xFF46 => {
                self.dma_reg = v;
                self.dma_src = u16::from(v) << 8;
                if self.dma_src >= 0xE000 {
                    self.dma_src -= 0x2000;
                }
                self.dma_prev_block = self.dma_active && self.dma_delay == 0;
                self.dma_pos = 0;
                self.dma_active = true;
                self.dma_delay = 2;
            }
            0xFF47 => self.ppu.write_bgp(v),
            0xFF48 => self.ppu.obp0 = v,
            0xFF49 => self.ppu.obp1 = v,
            0xFF4A => self.ppu.wy = v,
            0xFF4B => self.ppu.wx = v,
            // KEY0: a Color's start-up code says here which mode the cartridge runs in - Game Boy
            // mode when bit 2 or 3 is set - and only it can: once it is unmapped this is locked.
            0xFF4C if self.colour_start_up() => {
                let colour = v & 0x0C == 0;
                self.cgb = colour;
                self.ppu.cgb = colour;
            }
            0xFF4D if self.cgb => self.speed_prepare = v & 1 != 0,
            0xFF4F if self.cgb => self.vbk = v & 1,
            0xFF51 if self.cgb => {
                self.hdma_src = (self.hdma_src & 0x00F0) | u16::from(v) << 8;
                self.watch_hdma(0x51, v);
            }
            0xFF52 if self.cgb => {
                self.hdma_src = (self.hdma_src & 0xFF00) | u16::from(v & 0xF0);
                self.watch_hdma(0x52, v);
            }
            0xFF53 if self.cgb => {
                self.hdma_dst = 0x8000 | (self.hdma_dst & 0x00F0) | u16::from(v & 0x1F) << 8;
                self.watch_hdma(0x53, v);
            }
            0xFF54 if self.cgb => {
                self.hdma_dst = (self.hdma_dst & 0xFF00) | u16::from(v & 0xF0);
                self.watch_hdma(0x54, v);
            }
            0xFF55 if self.cgb => {
                self.watch_hdma(0x55, v);
                if self.hdma_active && v & 0x80 == 0 {
                    // Cancelled: FF55 then reads bit 7 set and the length just written
                    // (SameSuite hdma_lcd_off / hdma_mode0 read $80 after writing 0).
                    self.hdma_active = false;
                    self.hdma_blocks = (v & 0x7F) + 1;
                    self.log(format!("HBlank DMA cancelled, LY {}", self.ppu.ly));
                } else if v & 0x80 != 0 {
                    self.hdma_active = true;
                    self.hdma_blocks = (v & 0x7F) + 1;
                    self.log(format!("HBlank DMA {:04X}->{:04X}, {} blocks, LY {}",
                        self.hdma_src, self.hdma_dst, self.hdma_blocks, self.ppu.ly));
                    // Started with the LCD off, or already inside an HBlank: one block goes
                    // straight away (SameSuite hdma_lcd_off, hdma_mode0).
                    if !self.ppu.on() || (self.ppu.mode() == 0 && self.ppu.ly < 144) {
                        self.hdma_block();
                        self.hdma_blocks -= 1;
                        if self.hdma_blocks == 0 {
                            self.hdma_active = false;
                        }
                    }
                } else {
                    self.log(format!("general DMA {:04X}->{:04X}, {} bytes, VBK {}, LY {}, PC {:02X}:{:04X}",
                        self.hdma_src, self.hdma_dst, (u16::from(v & 0x7F) + 1) * 16, self.vbk, self.ppu.ly,
                        self.trace_bank, self.trace_pc));
                    // General DMA: all at once, the CPU stopped 8 M-cycles a block (16 at
                    // double speed).
                    let blocks = (v & 0x7F) + 1;
                    for _ in 0..blocks {
                        self.hdma_block();
                        let stall = if self.double_speed { 16 } else { 8 };
                        for _ in 0..stall {
                            self.tick();
                        }
                    }
                    self.hdma_blocks = 0; // FF55 reads $FF when done
                }
            }
            // A Color's start-up code still reaches the palettes and the object order after it has
            // put the console into Game Boy mode, for as long as it is mapped: that is when it
            // writes a black-and-white game's colours (Pan Docs, "Power Up Sequence").
            0xFF68 if self.cgb || self.colour_start_up() => self.ppu.bcps = v & 0xBF,
            0xFF69 if self.cgb || self.colour_start_up() => self.ppu.write_pal(false, v),
            0xFF6A if self.cgb || self.colour_start_up() => self.ppu.ocps = v & 0xBF,
            0xFF6B if self.cgb || self.colour_start_up() => self.ppu.write_pal(true, v),
            0xFF6C if self.cgb || self.colour_start_up() => self.ppu.opri = v & 1,
            0xFF70 if self.cgb => self.svbk = v & 7,
            0xFF72 if self.cgb_hardware => self.scratch[0] = v,
            0xFF73 if self.cgb_hardware => self.scratch[1] = v,
            0xFF75 if self.cgb_hardware => self.scratch[2] = v & 0x70,
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize] = v,
            0xFFFF => self.ie = v,
            _ => {}
        }
    }

    /// Logs a change to FF51–FF55 that is not one of the two known good copies.
    fn watch_hdma(&mut self, reg: u8, v: u8) {
        let i = (reg - 0x51) as usize;
        if self.hdma_watch[i] == v {
            return;
        }
        self.hdma_watch[i] = v;
        let known = matches!((reg, v, self.trace_bank, self.trace_pc),
            (0x51, 0x54 | 0x6B, 0x2F, 0x748F | 0x74BE)
            | (0x52, 0xB0 | 0xC0, 0x2F, 0x748F | 0x74BE)
            | (0x53, 0x98 | 0x9C, 0x2F, 0x748F | 0x74BE)
            | (0x54, 0x00, 0x2F, 0x748F | 0x74BE)
            | (0x55, 0x23 | 0x3F, 0x2F, 0x748F | 0x74BE));
        if known {
            return;
        }
        self.log(format!("HDMA FF{reg:02X}={v:02X} from {:02X}:{:04X} VBK {} LY {}",
            self.trace_bank, self.trace_pc, self.vbk, self.ppu.ly));
    }
}

fn yes() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bus(colour_console: bool) -> Bus {
        let mut b = Bus::new(Cart::new(vec![0; 0x8000]).unwrap());
        b.cgb_hardware = colour_console;
        b.ppu.cgb_hardware = colour_console;
        b
    }

    /// The table, as SameBoy's write-conflict model has it (and, for a Game Boy's LCDC first step
    /// and SCY, as the Game Boy pictures measure it; .kiro/notes/docs/CBOY_EXACT_DRAWER.md).
    #[test]
    fn each_picture_register_lands_on_its_own_dot() {
        let gb = bus(false);
        assert_eq!(gb.landing(0xFF40, 0x91), [Some((2, Land::LcdcFirst(0x91))), Some((3, Land::Value(0x91)))]);
        assert_eq!(gb.landing(0xFF42, 7), [Some((2, Land::Value(7))), None]);
        assert_eq!(gb.landing(0xFF43, 7), [Some((2, Land::Value(7))), None]);
        for reg in 0xFF47..=0xFF49 {
            assert_eq!(gb.landing(reg, 0xE4), [Some((2, Land::Or(0xE4))), Some((3, Land::Value(0xE4)))]);
        }
        assert_eq!(gb.landing(0xFF4B, 7), [Some((4, Land::Wx(7))), None]);
        for reg in [0xFF41, 0xFF44, 0xFF45, 0xFF46, 0xFF4A] {
            assert_eq!(gb.landing(reg, 7), [None, None], "{reg:04X} lands at the end");
        }
        let mut cgb = bus(true);
        cgb.ppu.lcdc = 0x91;
        assert_eq!(cgb.landing(0xFF40, 0x81), [Some((4, Land::TileSel(0x81))), None], "clearing tile select");
        cgb.ppu.lcdc = 0x81;
        assert_eq!(cgb.landing(0xFF40, 0x91), [Some((4, Land::TileSel(0x91))), None], "setting it");
        assert_eq!(cgb.landing(0xFF40, 0x83), [None, None]);
        for reg in 0xFF47..=0xFF49 {
            assert_eq!(cgb.landing(reg, 0xE4), [Some((3, Land::Value(0xE4))), None]);
        }
        assert_eq!(cgb.landing(0xFF4B, 7), [Some((4, Land::Hold)), Some((5, Land::Value(7)))]);
        for reg in [0xFF42, 0xFF43, 0xFF4A] {
            assert_eq!(cgb.landing(reg, 7), [None, None], "{reg:04X} lands at the end on a Color");
        }
        cgb.ppu.cgb_rev = CgbRev::E;
        for reg in 0xFF47..=0xFF49 {
            assert_eq!(cgb.landing(reg, 0xE4), [Some((2, Land::Value(0xE4))), None], "a CGB D or later, a dot earlier");
        }
        cgb.double_speed = true;
        assert_eq!(cgb.landing(0xFF47, 0xE4), [None, None], "double speed lands at the end");
    }

    /// The register itself still changes at the end of the M-cycle, which is what the CPU, STAT
    /// and the Standard drawer see; and once a write has landed nothing is left over for the
    /// drawer, so it reads the register again (one dot into the next M-cycle for a Color's WX).
    #[test]
    fn a_landed_write_leaves_the_drawer_reading_the_register() {
        for colour in [false, true] {
            let mut b = bus(colour);
            for reg in 0xFF40..=0xFF4Bu16 {
                // STAT, LY, LYC and the DMA register are not the drawer's.
                if matches!(reg, 0xFF41 | 0xFF44 | 0xFF45 | 0xFF46) {
                    continue;
                }
                // Keep the LCD on, so the PPU runs while the writes land.
                let v = if reg == 0xFF40 { 0x93 } else { 0x5A };
                b.write(reg, v);
                assert_eq!(b.raw_read(reg), v, "{reg:04X} changed at the end of its M-cycle");
                if colour && reg == 0xFF4B {
                    assert_eq!(b.ppu.drawer_sees(0x4B), 0, "a Color's WX is still the old one for a dot");
                    b.tick();
                }
                assert_eq!(b.ppu.drawer_sees(reg as u8), v, "{reg:04X}: nothing left over (colour {colour})");
            }
        }
    }

    /// What lands at each step: a Game Boy palette's old-OR-new dot, the first LCDC step's mix of
    /// old and new bits, and a Color's WX held back.
    #[test]
    fn what_each_step_carries() {
        let mut p = Ppu::default();
        p.bgp = 0x0C;
        p.land(0x47, Land::Or(0x30));
        assert_eq!(p.drawer_sees(0x47), 0x3C, "old OR new");
        p.land(0x47, Land::Value(0x30));
        assert_eq!(p.drawer_sees(0x47), 0x30);
        p.settle(0x47);
        assert_eq!(p.drawer_sees(0x47), 0x0C, "settled: the register again");
        // Tile map, tile select and object size new; background and objects old; the window on
        // only if it was on and stays on.
        p.lcdc = 0x80 | 0x20 | 0x03;
        p.land(0x40, Land::LcdcFirst(0x80 | 0x18 | 0x04));
        assert_eq!(p.drawer_sees(0x40), 0x80 | 0x18 | 0x04 | 0x03);
        p.settle(0x40);
        p.lcdc = 0x80 | 0x01;
        p.land(0x40, Land::LcdcFirst(0x80 | 0x20 | 0x01));
        assert_eq!(p.drawer_sees(0x40) & 0x20, 0, "switching the window on waits for the second step");
        p.settle(0x40);
        p.wx = 9;
        p.land(0x4B, Land::Hold);
        p.wx = 20;
        assert_eq!(p.drawer_sees(0x4B), 9, "held");
    }
}
