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

/// What is plugged into a controller port.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortKind {
    /// Atari's two-button ProLine joystick (a one-button 2600 stick reads the same way).
    #[default]
    Joystick,
    /// The light gun (port 1): its sensor on INPT4, its trigger on the stick's up line.
    LightGun,
    /// A pair of paddles: their knobs on the TIA's INPT0-3, their buttons on the stick lines.
    Paddles,
}

/// The controls, as the hardware sees them.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
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
    /// Where the light gun points: the line and the MARIA clock in that line the beam is at
    /// when it passes the spot, or None when it points off the picture.
    pub gun: Option<(u32, u32)>,
    pub trigger: bool,
    /// The four paddle knobs, 0 to 255 (port 1's two, then port 2's).
    pub paddles: [u8; 4],
    /// The paddles' buttons.
    pub paddle_buttons: [bool; 4],
}

impl Default for Input {
    /// Nothing pressed, both difficulty switches at A (where MAME and ProSystem start them).
    fn default() -> Self {
        Input {
            dirs: [[false; 4]; 2],
            buttons: [[false; 2]; 2],
            reset: false,
            select: false,
            pause: false,
            difficulty_a: [true; 2],
            gun: None,
            trigger: false,
            paddles: [0; 4],
            paddle_buttons: [false; 4],
        }
    }
}

/// The parts of the machine the processor reaches through its bus.
#[derive(Clone, Serialize, Deserialize)]
pub struct Sys {
    pub region: Region,
    /// The two 2 KB RAM chips, one after the other: $1800-$1FFF, then $2000-$27FF.
    pub ram: Vec<u8>,
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
    pub ports: [PortKind; 2],
    /// The paddles' capacitors: held empty while VBLANK's bit 7 is set, charging from when it
    /// clears (MARIA clocks).
    paddle_grounded: bool,
    paddle_start: u64,
    line_start: u64,
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
    /// Output samples (TIA level average, POKEY level average), `out_rate` a second.
    #[serde(skip)]
    pub out: Vec<(f32, f32)>,
    /// When the next output sample ends, in MARIA clocks times `out_rate`.
    out_next: u64,
    out_last: u64,
    pub out_rate: u64,
    /// Every TIA sound step's level (for the tests against MAME), when switched on.
    #[serde(skip)]
    pub tia_log: Option<Vec<u8>>,
    /// Every POKEY clock's level (the four channels added), likewise.
    #[serde(skip)]
    pub pokey_log: Option<Vec<u8>>,
    /// Every write to a POKEY, likewise: (the first POKEY clock that sees it, MARIA clock of
    /// the write, address, value).
    #[serde(skip)]
    pub pokey_writes: Option<Vec<(u64, u64, u16, u8)>>,
}

impl Sys {
    pub fn new(cart: Cart, region: Region, bios: Option<Vec<u8>>) -> Sys {
        let lines = region.lines();
        let mut s = Sys {
            region,
            ram: vec![0; 0x1000],
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
            ports: [PortKind::Joystick; 2],
            paddle_grounded: false,
            paddle_start: 0,
            line_start: 0,
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
            out_last: 0,
            out_rate: 48_000,
            tia_log: None,
            pokey_log: None,
            pokey_writes: None,
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

    /// Output samples a second: each one averages the sound over its stretch of MARIA clocks,
    /// the stretches 149 or 150 clocks long so a second holds exactly `hz` of them.
    pub fn set_output_rate(&mut self, hz: u64) {
        self.out_rate = hz.max(1);
        self.out_last = self.now;
        self.out_next = self.now * self.out_rate + self.region.maria_hz();
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
            let out_at = self.out_next.div_ceil(self.out_rate);
            let next = tia_at.min(pokey_at).min(out_at);
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
                if let Some(log) = self.pokey_log.as_mut() {
                    log.push(self.pokey_level);
                }
                self.pokey_next += CYCLE;
            }
            if next == out_at {
                let p = (out_at - self.out_last).max(1) as f32;
                self.out.push((self.mix_tia as f32 / p, self.mix_pokey as f32 / p));
                self.mix_tia = 0;
                self.mix_pokey = 0;
                self.out_last = out_at;
                self.out_next += self.region.maria_hz();
            }
        }
        self.mix_to(t);
    }

    /// Before the processor reads or writes POKEY: its clocks up to the one this cycle starts
    /// (that one already sees a write), as on MAME 0.289.
    fn pokey_sync(&mut self) {
        self.sound_to(self.now.saturating_sub(1));
    }

    fn log_pokey_write(&mut self, addr: u16, v: u8) {
        let (clock, now) = (self.pokey_next / CYCLE, self.now);
        if let Some(log) = self.pokey_writes.as_mut() {
            log.push((clock, now, addr, v));
        }
    }

    fn mix_to(&mut self, t: u64) {
        if t > self.mix_from {
            let dt = t - self.mix_from;
            self.mix_tia += self.tia_level as u64 * dt;
            self.mix_pokey += self.pokey_level as u64 * dt;
            self.mix_from = t;
        }
    }

    /// Everything due before the processor's next cycle, which starts at `self.now`: new lines,
    /// MARIA's DMA, and waiting out a halt. A cycle runs before an event only if it is over by the
    /// time the event comes (it can't be cut in two), the way MAME's scheduler runs a processor.
    fn catch_up(&mut self, len: u64) {
        loop {
            if self.now + len > self.next_line {
                self.start_line();
                continue;
            }
            if let Some(at) = self.dma_due {
                if self.now + len > at {
                    self.dma_due = None;
                    self.dma(at);
                    continue;
                }
            }
            if self.halted {
                // The processor's clock keeps its own beat while it waits: it starts again on
                // its last beat before the line begins.
                let wait = self.next_line - self.now;
                self.now += wait - wait % CYCLE;
                continue;
            }
            break;
        }
    }

    fn start_line(&mut self) {
        let at = self.next_line;
        self.sound_to(at);
        self.line_start = at;
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
            // The processor loses that many of its own cycles, rounded up: MARIA lets go of it on
            // the beat of the processor's clock it stopped it on.
            let _ = at;
            self.now += (res.cycles as u64).div_ceil(4) * CYCLE;
        }
        if res.nmi {
            self.nmi_edge = true;
        }
    }

    // ---- memory ----------------------------------------------------------------------------

    /// What MARIA's DMA reads: memory without side effects.
    fn dma_peek(&self, addr: u16) -> u8 {
        match addr {
            0x0040..=0x00FF | 0x0140..=0x01FF => self.ram[0x800 + (addr & 0x1FF) as usize],
            0x0480..=0x04FF | 0x0580..=0x05FF => self.riot.ram[(addr & 0x7F) as usize],
            0x1800..=0x1FFF => self.ram[(addr - 0x1800) as usize],
            0x2000..=0x27FF => self.ram[(addr - 0x1800) as usize],
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

    /// A paddle's capacitor is charged once the time since VBLANK let it go passes its knob
    /// (about 97 processor cycles a step, as the 7800 community's a7800 measures it).
    fn paddle_charged(&self, i: usize) -> bool {
        if self.paddle_grounded {
            return false;
        }
        let steps = (self.now - self.paddle_start) / (97 * CYCLE);
        steps > self.input.paddles[i] as u64
    }

    /// The light gun's sensor sees the beam when it passes within eight dots of where it aims.
    fn gun_lit(&self) -> bool {
        let Some((line, clock)) = self.input.gun else { return false };
        let beam = ((self.now - self.line_start) / 2) as i64;
        let dx = beam - (clock / 2) as i64;
        let dy = self.line as i64 - line as i64;
        dx * dx + dy * dy < 64
    }

    fn tia_read(&self, offset: u8) -> u8 {
        let b = &self.input.buttons;
        let paddles = |port: usize| self.ports[port] == PortKind::Paddles;
        let bit = |on: bool| (on as u8) << 7;
        match offset & 0x0F {
            0x00..=0x07 => 0x00,
            0x08 if paddles(0) => bit(self.paddle_charged(0)),
            0x09 if paddles(0) => bit(self.paddle_charged(1)),
            0x0A if paddles(1) => bit(self.paddle_charged(2)),
            0x0B if paddles(1) => bit(self.paddle_charged(3)),
            0x08 => bit(b[0][1]),
            0x09 => bit(b[0][0]),
            0x0A => bit(b[1][1]),
            0x0B => bit(b[1][0]),
            0x0C if self.ports[0] == PortKind::LightGun => bit(!self.gun_lit()),
            0x0C if paddles(0) => 0x80,
            0x0C => {
                if (b[0][0] || b[0][1]) && self.p1_one_button {
                    0x00
                } else {
                    0x80
                }
            }
            0x0D if paddles(1) => 0x80,
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
        } else if offset & 0x1F == 0x01 {
            // VBLANK: bit 7 holds the paddles' capacitors empty; letting go starts their count.
            if v & 0x80 != 0 {
                self.paddle_grounded = true;
            } else if self.paddle_grounded {
                self.paddle_grounded = false;
                self.paddle_start = self.now;
            }
        }
        self.sound_to(self.now);
        self.tia.write(offset, v);
    }

    fn pa_in(&self) -> u8 {
        let mut v = 0xFF;
        for p in 0..2 {
            let shift = if p == 0 { 4 } else { 0 };
            match self.ports[p] {
                PortKind::Joystick => {
                    for (i, &on) in self.input.dirs[p].iter().enumerate() {
                        if on {
                            v &= !(1 << (shift + i));
                        }
                    }
                }
                PortKind::LightGun => {
                    // The gun's trigger reads high on the stick's up line while pulled, low
                    // otherwise; its other lines read high.
                    v &= !(0x01 << shift);
                    if self.input.trigger {
                        v |= 1 << shift;
                    }
                }
                PortKind::Paddles => {
                    // The two paddles' buttons, on the stick's right and left lines.
                    if self.input.paddle_buttons[2 * p] {
                        v &= !(0x08 << shift);
                    }
                    if self.input.paddle_buttons[2 * p + 1] {
                        v &= !(0x04 << shift);
                    }
                }
            }
        }
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
            0x0040..=0x00FF | 0x0140..=0x01FF => self.ram[0x800 + (addr & 0x1FF) as usize],
            0x0400..=0x047F => {
                self.pokey_sync();
                self.cart.read_04xx(addr).unwrap_or(0)
            }
            0x0480..=0x04FF | 0x0580..=0x05FF => self.riot.ram[(addr & 0x7F) as usize],
            0x1800..=0x1FFF => self.ram[(addr - 0x1800) as usize],
            0x2000..=0x27FF => self.ram[(addr - 0x1800) as usize],
            0x4000..=0xFFFF => {
                if addr >= 0xC000 && self.bios_enabled() {
                    self.bios_read(addr)
                } else {
                    if addr < 0x8000 && self.cart.has_pokey() {
                        self.pokey_sync();
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
            0x0040..=0x00FF | 0x0140..=0x01FF => self.ram[0x800 + (addr & 0x1FF) as usize] = v,
            0x0400..=0x047F => {
                self.pokey_sync();
                if self.cart.write_04xx(addr, v) {
                    self.log_pokey_write(addr, v);
                }
            }
            0x0480..=0x04FF | 0x0580..=0x05FF => self.riot.ram[(addr & 0x7F) as usize] = v,
            0x1800..=0x1FFF => self.ram[(addr - 0x1800) as usize] = v,
            0x2000..=0x27FF => self.ram[(addr - 0x1800) as usize] = v,
            0x4000..=0xFFFF => {
                if addr < 0x8000 && self.cart.has_pokey() {
                    self.pokey_sync();
                    if self.cart.pokey_at_4000() {
                        self.log_pokey_write(addr, v);
                    }
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
        let len = self.cycle_len(addr);
        self.catch_up(len);
        let v = self.read_mem(addr);
        self.now += len;
        v
    }

    fn write(&mut self, addr: u16, value: u8) {
        let len = self.cycle_len(addr);
        self.catch_up(len);
        self.write_mem(addr, value);
        self.now += len;
    }

    // Nothing on the 7800 drives the processor's IRQ line (the trait's default): the cartridge
    // POKEY's isn't connected (see pokey.rs).

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
