//! The Atari 7800 itself: its memory map, its clocks, and how MARIA, the processor, the TIA and
//! the RIOT share the time.
//!
//! Time is counted in MARIA clocks (7.16 MHz on NTSC). A processor cycle is four of them, a line
//! 454, a frame 263 lines on NTSC and 313 on PAL. At the start of each line MARIA lets go of a
//! processor that wrote WSYNC; seven processor cycles in, it halts the processor for that line's
//! DMA (src/maria.rs), and when a zone asks for it, raises a display list interrupt (NMI). The
//! processor's own accesses to the TIA and the RIOT can run at the slower 1.19 MHz the real board
//! switches to for them (a setting; MAME 0.289 doesn't model it, so its tests run without).
//!
//! The memory map is the 7800's: the TIA at $00-$1F and MARIA at $20-$3F (both repeated at $100,
//! $200 and $300), the two 2 KB RAM chips at $1800 and $2000 (the second also showing at $40-$FF
//! and $140-$1FF), the RIOT's ports at $280 and its RAM at $480, the cartridge from $4000, and the
//! console's BIOS over $C000-$FFFF until the INPTCTRL register switches it out and locks itself.

use crate::cart::Cart;
use crate::m6502::{Bus, M6502};
use crate::maria::Maria;
use crate::riot::Riot;
use crate::tia::TiaSound;
use serde::{Deserialize, Serialize};

pub const LINE_CLOCKS: u64 = 454;
pub const WIDTH: usize = 320;
/// Processor cycle in MARIA clocks, and the slow cycle for TIA and RIOT accesses.
const CYCLE: u64 = 4;
const SLOW_CYCLE: u64 = 6;
/// DMA starts seven processor cycles after the line begins.
const DMA_START: u64 = 7 * CYCLE;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Region {
    Ntsc,
    Pal,
}

impl Region {
    pub fn lines(self) -> u32 {
        match self {
            Region::Ntsc => 263,
            Region::Pal => 313,
        }
    }
    /// MARIA's clock in Hz (half the board's crystal).
    pub fn maria_hz(self) -> u64 {
        match self {
            Region::Ntsc => 7_159_090,
            Region::Pal => 7_093_788,
        }
    }
    /// The first and one-past-last line a TV shows (MAME's visible area).
    pub fn visible(self) -> (u32, u32) {
        match self {
            Region::Ntsc => (27, 27 + 192 + 32),
            Region::Pal => (35, 35 + 228 + 32),
        }
    }
    pub fn fps(self) -> f64 {
        self.maria_hz() as f64 / (LINE_CLOCKS as f64 * self.lines() as f64)
    }
}

/// The controls, as the hardware sees them.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Input {
    /// Per player: up, down, left, right.
    pub dirs: [[bool; 4]; 2],
    /// Per player: the left button (button 1, "fire") and the right button (button 2).
    pub buttons: [[bool; 2]; 2],
    pub reset: bool,
    pub select: bool,
    pub pause: bool,
    /// Difficulty switches: true is A ("advanced", the right position), false is B.
    pub difficulty_a: [bool; 2],
}

/// The parts of the machine the processor reaches through its bus.
#[derive(Clone, Serialize, Deserialize)]
pub struct Sys {
    pub region: Region,
    /// RAM chip at $1800 and the one at $2000.
    pub ram1: Vec<u8>,
    pub ram2: Vec<u8>,
    pub riot: Riot,
    pub tia: TiaSound,
    pub maria: Maria,
    pub cart: Cart,
    #[serde(skip)]
    pub bios: Option<Vec<u8>>,
    pub ctrl_lock: bool,
    pub ctrl_reg: u8,
    /// Now, in MARIA clocks since power-on.
    pub now: u64,
    pub line: u32,
    next_line: u64,
    dma_due: Option<u64>,
    /// Waiting for the next line (WSYNC, or DMA that ran out of time).
    halted: bool,
    nmi_edge: bool,
    pub frame_done: bool,
    pub frames: u64,
    /// 320 x lines, MARIA colour values.
    pub screen: Vec<u8>,
    pub input: Input,
    p1_one_button: bool,
    p2_one_button: bool,
    /// Slow 1.19 MHz cycles for the TIA and the RIOT, as the real board has.
    pub slow_io: bool,
    // ---- sound ----
    /// The TIA's sound clock as a fraction of MARIA clocks: `tia_num / tia_den` per step.
    tia_num: u64,
    tia_den: u64,
    /// When the next TIA sound step is due, in MARIA clocks times `tia_den`.
    tia_next: u64,
    tia_level: u8,
    /// When the next POKEY step is due (MARIA clocks).
    pokey_next: u64,
    pokey_level: u8,
    /// Sound mixed so far: level x MARIA clocks, since `mix_from`.
    mix_tia: u64,
    mix_pokey: u64,
    mix_from: u64,
    /// Output samples (TIA level average, POKEY level average), one per `out_period` clocks.
    #[serde(skip)]
    pub out: Vec<(f32, f32)>,
    out_next: u64,
    pub out_period: u64,
    /// Every TIA sound step's level (for the tests against MAME), when switched on.
    #[serde(skip)]
    pub tia_log: Option<Vec<u8>>,
}

impl Sys {
    pub fn new(cart: Cart, region: Region, bios: Option<Vec<u8>>) -> Sys {
        let lines = region.lines();
        let mut s = Sys {
            region,
            ram1: vec![0; 0x800],
            ram2: vec![0; 0x800],
            riot: Riot::default(),
            tia: TiaSound::default(),
            maria: Maria::default(),
            cart,
            bios,
            ctrl_lock: false,
            ctrl_reg: 0,
            now: 0,
            line: 0,
            next_line: LINE_CLOCKS,
            dma_due: Some(DMA_START),
            halted: false,
            nmi_edge: false,
            frame_done: false,
            frames: 0,
            screen: vec![0; WIDTH * lines as usize],
            input: Input::default(),
            p1_one_button: true,
            p2_one_button: true,
            slow_io: false,
            tia_num: 228,
            tia_den: 1,
            tia_next: 228,
            tia_level: 0,
            pokey_next: 0,
            pokey_level: 0,
            mix_tia: 0,
            mix_pokey: 0,
            mix_from: 0,
            out: Vec::new(),
            out_next: 0,
            out_period: 0,
            tia_log: None,
        };
        s.set_output_rate(48_000);
        if s.bios.is_none() {
            // No BIOS: the console as its BIOS leaves it, switched out and locked.
            s.ctrl_lock = true;
            s.ctrl_reg = 0x07;
        }
        s
    }

    /// Sets the TIA's sound clock (MARIA clocks per step, as a fraction). The real board's is
    /// exactly 228; MAME 0.289 rounds the TIA's clock to whole hertz, which the tests copy.
    pub fn set_tia_period(&mut self, num: u64, den: u64) {
        self.tia_num = num;
        self.tia_den = den;
        self.tia_next = (self.now + 1) * den;
    }

    pub fn set_output_rate(&mut self, hz: u64) {
        // Whole MARIA clocks per output sample; the frame's sample count follows from it.
        self.out_period = (self.region.maria_hz() / hz).max(1);
        self.out_next = self.now + self.out_period;
    }

    fn bios_enabled(&self) -> bool {
        self.bios.is_some() && self.ctrl_reg & 0x04 == 0
    }

    // ---- time ------------------------------------------------------------------------------

    /// Brings the sound chips up to `t`.
    fn sound_to(&mut self, t: u64) {
        loop {
            let tia_at = self.tia_next.div_ceil(self.tia_den);
            let pokey_at = if self.cart.has_pokey() { self.pokey_next } else { u64::MAX };
            let next = tia_at.min(pokey_at).min(self.out_next);
            if next > t {
                break;
            }
            self.mix_to(next);
            if next == tia_at {
                self.tia_level = self.tia.clock();
                if let Some(log) = self.tia_log.as_mut() {
                    log.push(self.tia_level);
                }
                self.tia_next += self.tia_num;
            }
            if next == pokey_at {
                self.pokey_level = self.cart.clock_pokeys();
                self.pokey_next += CYCLE;
            }
            if next == self.out_next {
                let p = self.out_period as f32;
                self.out.push((self.mix_tia as f32 / p, self.mix_pokey as f32 / p));
                self.mix_tia = 0;
                self.mix_pokey = 0;
                self.out_next += self.out_period;
            }
        }
        self.mix_to(t);
    }

    fn mix_to(&mut self, t: u64) {
        if t > self.mix_from {
            let dt = t - self.mix_from;
            self.mix_tia += self.tia_level as u64 * dt;
            self.mix_pokey += self.pokey_level as u64 * dt;
            self.mix_from = t;
        }
    }

    /// Everything due before a processor cycle that starts at `self.now`: new lines, MARIA's
    /// DMA, and waiting out a halt.
    fn catch_up(&mut self) {
        loop {
            if self.now >= self.next_line {
                self.start_line();
                continue;
            }
            if let Some(at) = self.dma_due {
                if self.now >= at {
                    self.dma_due = None;
                    self.dma(at);
                    continue;
                }
            }
            if self.halted {
                self.now = self.next_line;
                continue;
            }
            break;
        }
    }

    fn start_line(&mut self) {
        let at = self.next_line;
        self.sound_to(at);
        self.line += 1;
        let lines = self.region.lines();
        if self.line >= lines {
            self.line = 0;
            self.frame_done = true;
            self.frames += 1;
        }
        self.maria.line_start(self.line, lines);
        if self.halted {
            self.halted = false;
            self.maria.wsync = false;
        }
        self.dma_due = Some(at + DMA_START);
        self.next_line = at + LINE_CLOCKS;
    }

    fn dma(&mut self, at: u64) {
        let lines = self.region.lines();
        let line = self.line;
        let row_start = line as usize * WIDTH;
        let mut maria = std::mem::take(&mut self.maria);
        let mut row = [0u8; WIDTH];
        let res = {
            let me = &*self;
            let mut mem = |a: u16| me.dma_peek(a);
            maria.line(line, lines, &mut mem, &mut row)
        };
        self.maria = maria;
        if line > 15 && line < lines - 5 {
            self.screen[row_start..row_start + WIDTH].copy_from_slice(&row);
        }
        if res.overrun {
            self.halted = true;
        } else if res.cycles > 0 && !self.halted {
            let resume = at + (res.cycles as u64).div_ceil(4) * CYCLE;
            if resume > self.now {
                self.now = resume;
            }
        }
        if res.nmi {
            self.nmi_edge = true;
        }
    }

    // ---- memory ----------------------------------------------------------------------------

    /// What MARIA's DMA reads: memory without side effects.
    fn dma_peek(&self, addr: u16) -> u8 {
        match addr {
            0x0040..=0x00FF | 0x0140..=0x01FF => self.ram2[(addr & 0x1FF) as usize],
            0x0480..=0x04FF | 0x0580..=0x05FF => self.riot.ram[(addr & 0x7F) as usize],
            0x1800..=0x1FFF => self.ram1[(addr - 0x1800) as usize],
            0x2000..=0x27FF => self.ram2[(addr - 0x2000) as usize],
            0x4000..=0xFFFF => {
                if addr >= 0xC000 && self.bios_enabled() {
                    self.bios_read(addr)
                } else {
                    self.cart.peek(addr)
                }
            }
            _ => 0,
        }
    }

    fn bios_read(&self, addr: u16) -> u8 {
        let b = self.bios.as_ref().unwrap();
        // A 4 KB BIOS sits at $F000 with $FF below it; a 16 KB one fills $C000-$FFFF.
        let off = (addr - 0xC000) as usize;
        let start = 0x4000 - b.len().min(0x4000);
        if off >= start {
            b[off - start]
        } else {
            0xFF
        }
    }

    fn is_slow(addr: u16) -> bool {
        // The board drops to 1.19 MHz for the TIA ($00-$1F and its mirrors) and the RIOT.
        matches!(addr, 0x0000..=0x001F | 0x0100..=0x011F | 0x0200..=0x021F | 0x0300..=0x031F)
            || Self::riot_io(addr)
            || matches!(addr, 0x0480..=0x04FF | 0x0580..=0x05FF)
    }

    fn riot_io(addr: u16) -> bool {
        addr & !0x0160 & 0xFF80 == 0x0280 && addr < 0x0400
    }

    fn tia_read(&self, offset: u8) -> u8 {
        let b = &self.input.buttons;
        match offset & 0x0F {
            0x00..=0x07 => 0x00,
            0x08 => (b[0][1] as u8) << 7,
            0x09 => (b[0][0] as u8) << 7,
            0x0A => (b[1][1] as u8) << 7,
            0x0B => (b[1][0] as u8) << 7,
            0x0C => {
                if (b[0][0] || b[0][1]) && self.p1_one_button {
                    0x00
                } else {
                    0x80
                }
            }
            0x0D => {
                if (b[1][0] || b[1][1]) && self.p2_one_button {
                    0x00
                } else {
                    0x80
                }
            }
            _ => 0xFF,
        }
    }

    fn tia_write(&mut self, offset: u8, v: u8) {
        if !self.ctrl_lock {
            // INPTCTRL answers every TIA address until it locks itself.
            self.ctrl_lock = v & 0x01 != 0;
            self.ctrl_reg = v;
        }
        self.sound_to(self.now);
        self.tia.write(offset, v);
    }

    fn pa_in(&self) -> u8 {
        let mut v = 0xFF;
        for (p, d) in self.input.dirs.iter().enumerate() {
            let shift = if p == 0 { 4 } else { 0 };
            for (i, &on) in d.iter().enumerate() {
                if on {
                    v &= !(1 << (shift + i));
                }
            }
        }
        // Opposite directions at once can't happen on a real stick.
        v
    }

    fn pb_in(&self) -> u8 {
        let i = &self.input;
        let mut v = 0xFF;
        if i.reset {
            v &= !0x01;
        }
        if i.select {
            v &= !0x02;
        }
        if i.pause {
            v &= !0x08;
        }
        if !i.difficulty_a[0] {
            v &= !0x40;
        }
        if !i.difficulty_a[1] {
            v &= !0x80;
        }
        v
    }

    fn update_buttons_mode(&mut self) {
        let pb = self.riot.pb_drive();
        self.p1_one_button = pb & 0x04 != 0;
        self.p2_one_button = pb & 0x10 != 0;
    }

    fn read_mem(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x001F | 0x0100..=0x011F | 0x0200..=0x021F | 0x0300..=0x031F => self.tia_read(addr as u8),
            0x0020..=0x003F | 0x0120..=0x013F | 0x0220..=0x023F | 0x0320..=0x033F => self.maria.read(addr as u8),
            0x0040..=0x00FF | 0x0140..=0x01FF => self.ram2[(addr & 0x1FF) as usize],
            0x0400..=0x047F => {
                self.sound_to(self.now);
                self.cart.read_04xx(addr).unwrap_or(0)
            }
            0x0480..=0x04FF | 0x0580..=0x05FF => self.riot.ram[(addr & 0x7F) as usize],
            0x1800..=0x1FFF => self.ram1[(addr - 0x1800) as usize],
            0x2000..=0x27FF => self.ram2[(addr - 0x2000) as usize],
            0x4000..=0xFFFF => {
                if addr >= 0xC000 && self.bios_enabled() {
                    self.bios_read(addr)
                } else {
                    if addr < 0x8000 && self.cart.has_pokey() {
                        self.sound_to(self.now);
                    }
                    self.cart.read(addr)
                }
            }
            a if Self::riot_io(a) => {
                let (pa, pb) = (self.pa_in(), self.pb_in());
                self.riot.read_io(a, self.now, pa, pb)
            }
            _ => 0,
        }
    }

    fn write_mem(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x001F | 0x0100..=0x011F | 0x0200..=0x021F | 0x0300..=0x031F => self.tia_write(addr as u8, v),
            0x0020..=0x003F | 0x0120..=0x013F | 0x0220..=0x023F | 0x0320..=0x033F => {
                if self.maria.write(addr as u8, v) {
                    self.halted = true;
                }
            }
            0x0040..=0x00FF | 0x0140..=0x01FF => self.ram2[(addr & 0x1FF) as usize] = v,
            0x0400..=0x047F => {
                self.sound_to(self.now);
                self.cart.write_04xx(addr, v);
            }
            0x0480..=0x04FF | 0x0580..=0x05FF => self.riot.ram[(addr & 0x7F) as usize] = v,
            0x1800..=0x1FFF => self.ram1[(addr - 0x1800) as usize] = v,
            0x2000..=0x27FF => self.ram2[(addr - 0x2000) as usize] = v,
            0x4000..=0xFFFF => {
                if addr < 0x8000 && self.cart.has_pokey() {
                    self.sound_to(self.now);
                }
                self.cart.write(addr, v);
            }
            a if Self::riot_io(a) => {
                self.riot.write_io(a, v, self.now);
                self.update_buttons_mode();
            }
            _ => {}
        }
    }

    fn cycle_len(&self, addr: u16) -> u64 {
        if self.slow_io && Self::is_slow(addr) {
            SLOW_CYCLE
        } else {
            CYCLE
        }
    }
}

impl Bus for Sys {
    fn read(&mut self, addr: u16) -> u8 {
        self.catch_up();
        let v = self.read_mem(addr);
        self.now += self.cycle_len(addr);
        v
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.catch_up();
        self.write_mem(addr, value);
        self.now += self.cycle_len(addr);
    }

    fn irq(&self) -> bool {
        self.cart.irq()
    }

    fn take_nmi(&mut self) -> bool {
        std::mem::take(&mut self.nmi_edge)
    }
}

/// The whole console.
#[derive(Clone, Serialize, Deserialize)]
pub struct Atari7800 {
    pub cpu: M6502,
    pub sys: Sys,
}

impl Atari7800 {
    pub fn new(cart: Cart, region: Region, bios: Option<Vec<u8>>) -> Atari7800 {
        let mut m = Atari7800 { cpu: M6502::new(), sys: Sys::new(cart, region, bios) };
        m.reset();
        m
    }

    /// The console's reset (the Reset switch on a 7800 is a game input; this is power-on/reset).
    pub fn reset(&mut self) {
        self.sys.cart.reset();
        self.sys.maria = Maria::default();
        if self.sys.bios.is_some() {
            self.sys.ctrl_lock = false;
            self.sys.ctrl_reg = 0;
        }
        self.cpu.reset(&mut self.sys);
    }

    /// Runs until the current frame is finished (the start of line 0).
    pub fn run_frame(&mut self) {
        self.sys.frame_done = false;
        while !self.sys.frame_done {
            self.cpu.step(&mut self.sys);
        }
        let now = self.sys.now;
        self.sys.sound_to(now);
    }

    /// The visible picture: `rows` lines of 320 MARIA colour values from line `first`.
    pub fn picture(&self, first: u32, rows: u32) -> &[u8] {
        let s = first as usize * WIDTH;
        &self.sys.screen[s..s + rows as usize * WIDTH]
    }
}
