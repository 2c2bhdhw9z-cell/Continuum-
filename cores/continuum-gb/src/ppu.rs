//! The picture processing unit: modes 2/3/0 on each of lines 0-143, then VBlank (lines 144-153),
//! 456 dots a line, with LY, LYC, STAT and its interrupt line, and two drawers for the
//! background, window and sprites. Pixels are DMG shades 0-3 (after the palettes).
//!
//! Mode 3's length comes from `mode3_length` (172 dots plus the SCX, window and sprite delays),
//! whichever drawer runs, so STAT and HBlank timing never depend on the drawer. The drawer is the
//! core option "Picture drawing" (`exact`):
//!   - Standard (the default): `render_line` draws the whole line as mode 3 begins. The proven
//!     one; what every game here has been played on.
//!   - Exact (experimental): the pixel FIFO in fifo.rs, dot by dot, so mid-line register writes
//!     land on the right pixel. Unfinished and parked; see the owner rules before touching it.

use serde::{Deserialize, Serialize};

pub const W: usize = 160;
pub const H: usize = 144;

/// Switches on `Ppu::trace_land`'s output (cargo feature `trace` only).
#[cfg(feature = "trace")]
pub static TRACE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The DMG shades as XRGB8888, lightest first.
pub const SHADES: [u32; 4] = [0x00FF_FFFF, 0x00AA_AAAA, 0x0055_5555, 0x0000_0000];

#[derive(Clone, Serialize, Deserialize)]
pub struct Ppu {
    pub vram: Vec<u8>,
    pub oam: Vec<u8>,
    pub lcdc: u8,
    stat_enable: u8,
    pub scy: u8,
    pub scx: u8,
    pub ly: u8,
    pub lyc: u8,
    pub bgp: u8,
    pub obp0: u8,
    pub obp1: u8,
    pub wy: u8,
    pub wx: u8,
    pub dot: u16,
    mode: u8,
    stat_line: bool,
    /// The window's row counter. It advances every time the window starts, so it is one behind
    /// the row being drawn until the first start of the frame; 0xFF is "none yet".
    window_line: u8,
    wy_hit: bool,
    /// The LY=LYC comparison, latched: it only updates while the LCD is on (Mooneye stat_lyc_onoff).
    #[serde(default)]
    lyc_eq: bool,
    /// The first line after the LCD is switched on: no OAM scan, and mode 3 starts 4 dots later.
    #[serde(default)]
    first_line: bool,
    /// The picture is part of a state: a game that switches the LCD on mid-frame leaves lines
    /// from the frame before, and run-ahead and replays must show the same ones.
    pub frame: Vec<u32>,
    /// Set when line 144 begins (a whole frame is in `frame`).
    #[serde(skip)]
    pub frame_ready: bool,
    /// The first frame after the LCD is switched on is not shown, as on hardware.
    skip_frame: bool,
    /// The dot mode 0 starts at on this line (mode 3 is 172 dots plus SCX and sprite delays).
    #[serde(default)]
    hblank_at: u16,
    /// Game Boy Color mode (from the cartridge header, or set by a Color's own start-up file
    /// through KEY0).
    pub cgb: bool,
    /// Color hardware, whichever mode the cartridge runs in: a Color playing a Game Boy game is
    /// still a Color, so its STAT interrupt timing is the Color's and its pixels come out of its
    /// palette memory (`mono_colour`). Comes from the console the player picked, so it is not
    /// part of a save state; gb.rs sets it.
    #[serde(skip)]
    pub cgb_hardware: bool,
    pub bg_pal: Vec<u8>,
    pub obj_pal: Vec<u8>,
    pub bcps: u8,
    pub ocps: u8,
    pub opri: u8,
    /// Layer switches from the core options (bit 0 background, 1 window, 2 sprites). Not
    /// hardware state, so not in save states.
    #[serde(skip, default = "all_layers")]
    pub layers: u8,
    /// Frame skip: draw nothing this frame (not saved).
    #[serde(skip)]
    pub no_render: bool,
    /// Debug/comfort: draw more than ten sprites on a line (not saved).
    #[serde(skip)]
    pub no_sprite_limit: bool,
    /// Which drawer makes the picture: false the standard per-line one, true the exact
    /// (experimental) pixel FIFO. A core option, not hardware state, so not saved.
    #[serde(skip)]
    pub exact: bool,
    /// For each pixel of `frame` on the DMG: the shade (0-3, after the palette registers) and
    /// where it came from (0 background or window, 1 sprite palette 0, 2 sprite palette 1).
    /// The output stage uses them for palettes, colourising and the Super Game Boy.
    pub shade: Vec<u8>,
    pub source: Vec<u8>,
    /// Mode 3's pixel pipeline (never mid-line at a frame boundary, where states are taken).
    #[serde(skip)]
    fifo: fifo::Fifo,
    /// A register write the picture hardware has seen and the register does not hold yet, or
    /// the other way round (`land`).
    #[serde(skip)]
    landed: Landed,
}

fn all_layers() -> u8 {
    7
}

fn blank_bytes() -> Vec<u8> {
    vec![0; W * H]
}

fn blank() -> Vec<u32> {
    vec![SHADES[0]; W * H]
}

pub struct PpuIrq {
    pub vblank: bool,
    pub stat: bool,
}

/// What reaches the picture hardware at one step of a CPU write to a picture register. The bus
/// decides on which dot each step lands (`Bus::landing`); `Ppu::land` says what each one does.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Land {
    /// The new value.
    Value(u8),
    /// The old value OR the new one.
    Or(u8),
    /// The first of a Game Boy's two LCDC steps (see `Ppu::land`).
    LcdcFirst(u8),
    /// WX on a Game Boy: the new value, and for the next dot the window's one-pixel-late start
    /// is not taken.
    Wx(u8),
    /// LCDC on a Color, changing tile select: the new value, and for the next dot a bitplane read
    /// glitches.
    TileSel(u8),
    /// The value from before the write, for one more dot.
    Hold,
}

/// The picture registers as the picture hardware sees them while a CPU write to one of them is
/// on its way (`None`: as the register holds it), and the one-dot effects a write leaves behind.
/// It only ever lasts a dot past the M-cycle carrying the write, so it is never in a save state.
#[derive(Clone, Default)]
struct Landed {
    reg: [Option<u8>; 12],
    wx_fresh: bool,
    tile_sel_glitch: bool,
}

#[path = "fifo.rs"]
mod fifo;

impl Default for Ppu {
    fn default() -> Self {
        Ppu {
            vram: vec![0; 0x4000],
            oam: vec![0; 0xA0],
            lcdc: 0x91,
            stat_enable: 0,
            scy: 0,
            scx: 0,
            ly: 0,
            lyc: 0,
            bgp: 0xFC,
            obp0: 0xFF,
            obp1: 0xFF,
            wy: 0,
            wx: 0,
            dot: 0,
            mode: 2,
            stat_line: false,
            window_line: 0xFF,
            wy_hit: false,
            lyc_eq: true,
            first_line: false,
            frame: blank(),
            frame_ready: false,
            skip_frame: false,
            hblank_at: 252,
            cgb: false,
            cgb_hardware: false,
            bg_pal: vec![0xFF; 64],
            layers: 7,
            no_render: false,
            no_sprite_limit: false,
            exact: false,
            fifo: Default::default(),
            landed: Landed::default(),
            shade: blank_bytes(),
            source: blank_bytes(),
            obj_pal: vec![0xFF; 64],
            bcps: 0,
            ocps: 0,
            opri: 0,
        }
    }
}

impl Ppu {
    pub fn on(&self) -> bool {
        self.lcdc & 0x80 != 0
    }

    pub fn mode(&self) -> u8 {
        self.mode
    }

    /// LY as the CPU reads it: it shows the next line 4 dots before that line begins (lines
    /// 0-152; Mooneye lcdon_timing).
    pub fn ly_read(&self) -> u8 {
        if self.on() && self.dot >= 452 && self.ly < 153 {
            self.ly + 1
        } else {
            self.ly
        }
    }

    /// Whether the CPU is shut out of OAM: during the OAM scan and mode 3, and from 4 dots before
    /// a line's OAM scan begins (Mooneye lcdon_timing).
    pub fn oam_blocked(&self) -> bool {
        self.on()
            && ((self.ly < 144 && (self.mode == 2 || self.mode == 3))
                || (self.dot >= 452 && (self.ly < 143 || self.ly == 153)))
    }

    /// Whether the CPU is shut out of VRAM: mode 3, from 4 dots before it on ordinary lines.
    pub fn vram_blocked(&self) -> bool {
        self.on() && self.ly < 144 && (self.mode == 3 || (!self.first_line && (76..80).contains(&self.dot)))
    }

    /// Writes are shut out a little differently from reads (Mooneye lcdon_write_timing): OAM
    /// during the OAM scan but not its last 4 dots, and mode 3; VRAM only in mode 3.
    pub fn oam_write_blocked(&self) -> bool {
        self.on() && self.ly < 144 && ((self.mode == 2 && self.dot < 76) || self.mode == 3)
    }

    pub fn vram_write_blocked(&self) -> bool {
        self.on() && self.ly < 144 && self.mode == 3
    }

    pub fn read_stat(&self) -> u8 {
        let coinc = if self.lyc_eq { 4 } else { 0 };
        let mode = if self.on() { self.mode } else { 0 };
        0x80 | self.stat_enable | coinc | mode
    }

    pub fn write_lyc(&mut self, v: u8) -> bool {
        self.lyc = v;
        if self.on() {
            self.lyc_eq = self.ly == self.lyc;
        }
        self.restat()
    }

    /// Re-evaluates the STAT interrupt line after a register write; true on a rising edge (the
    /// interrupt is requested in the same M-cycle as the write).
    pub fn restat(&mut self) -> bool {
        if !self.on() {
            return false;
        }
        let line = self.stat_condition();
        let rise = line && !self.stat_line;
        self.stat_line = line;
        rise
    }

    pub fn write_stat(&mut self, v: u8) {
        self.stat_enable = v & 0x78;
    }

    pub fn write_lcdc(&mut self, v: u8) -> bool {
        let was = self.on();
        self.lcdc = v;
        if was && !self.on() {
            self.fifo.active = false;
            self.ly = 0;
            self.dot = 0;
            self.mode = 0;
        } else if !was && self.on() {
            self.ly = 0;
            self.lyc_eq = self.lyc == 0;
            self.first_line = true;
            self.dot = 0; // line 0 after switch-on: no OAM scan, LY ticks over 4 dots early
            self.mode = 0; // and has no OAM scan
            self.window_line = 0xFF;
            self.wy_hit = false;
            self.skip_frame = true;
        }
        self.restat()
    }

    /// The window's Y comparison. The match is sticky for the rest of the frame, but it is only
    /// made while LCDC bit 5 has the window switched on, so a game that switches the window on
    /// after the line WY names has gone by gets no window until the next frame.
    ///
    /// The hardware runs the comparison at the start of mode 2 and again whenever WY or LCDC is
    /// written. Running it on every dot instead comes to the same thing - neither register can
    /// change without a write, and LY only changes at the line boundary - and costs no state.
    fn wy_check(&mut self) {
        if self.lcdc & 0x20 != 0 && self.ly == self.wy {
            self.wy_hit = true;
        }
    }

    fn stat_condition(&self) -> bool {
        let e = self.stat_enable;
        (e & 0x40 != 0 && self.lyc_eq)
            || (e & 0x08 != 0 && (self.mode == 0 || (self.ly < 144 && self.dot + 4 >= self.hblank_at && self.mode == 3)))
            || (e & 0x10 != 0 && self.mode == 1)
            || (e & 0x20 != 0 && (self.mode == 2 || self.oam_irq_early() || (self.ly == 144 && self.dot < 4)))
    }

    /// The mode 2 STAT interrupt is raised one M-cycle before mode 2 shows in STAT, which is the
    /// M-cycle before the line it belongs to begins (so `dot >= 452` of the line before it).
    ///
    /// Line 144 has no mode 2, and its interrupt is where the two consoles part. A Color raises
    /// it one M-cycle early like every other line's, so it arrives one M-cycle BEFORE VBlank; a
    /// Game Boy holds that one back to the start of line 144, where it lands together with
    /// VBlank. That is the whole of the difference between Mooneye's vblank_stat_intr-GS (passes
    /// on DMG, MGB, SGB, SGB2) and vblank_stat_intr-C (passes on CGB, AGB, AGS); the Game Boy's
    /// late raise is the `ly == 144 && dot < 4` term in `stat_condition`, which on a Color only
    /// holds the line up that the early raise has already put high.
    fn oam_irq_early(&self) -> bool {
        self.dot >= 452 && self.ly < if self.cgb_hardware { 144 } else { 143 }
    }

    /// Mode 3's length: 172 dots, plus the SCX fine scroll, plus each sprite on the line
    /// (6 dots, more while the background tile under it is still being fetched; Pan Docs).
    /// Measured against the exact drawer's own dot count, which is the same number in whole
    /// M-cycles for every sprite layout (`the_object_stalls_are_mode_3s_length`).
    fn mode3_length(&self) -> u16 {
        let base = 172 + u16::from(self.scx & 7);
        let mut len = 0u16;
        // A DMG skips the object fetch altogether while LCDC bit 1 is clear, so it costs nothing.
        // A Color runs the fetch anyway and only drops the pixel as it leaves the FIFO, so the
        // dots are still spent and mode 3 is still longer.
        if self.lcdc & 0x02 == 0 && !self.cgb {
            return base;
        }
        let height: i16 = if self.lcdc & 0x04 != 0 { 16 } else { 8 };
        let mut xs: Vec<u8> = Vec::with_capacity(10);
        for i in 0..40 {
            let y = i16::from(self.oam[i * 4]) - 16;
            let line = i16::from(self.ly);
            if line >= y && line < y + height {
                xs.push(self.oam[i * 4 + 1]);
                if xs.len() == 10 {
                    break;
                }
            }
        }
        xs.sort_unstable();
        let mut paid_tile: Option<u16> = None;
        for x in xs {
            if x >= 168 {
                continue;
            }
            // An object at X=0 is fetched before the line's fine-scroll discard has run, because
            // the discard is held while one is pending, so it always waits a whole tile fetch
            // however many pixels SCX's low bits would have dropped.
            let pos = if x == 0 { 0 } else { u16::from(x) + u16::from(self.scx & 7) };
            let tile = pos / 8;
            if paid_tile != Some(tile) {
                paid_tile = Some(tile);
                len += 5u16.saturating_sub((pos % 8).min(5));
            }
            len += 6;
        }
        // The object penalty shows in STAT in whole M-cycles (Mooneye intr_2_mode0_timing_sprites:
        // every case fits 6 per object plus the BG-fetch wait, rounded down to 4 dots).
        base + (len & !3)
    }

    /// One dot.
    pub fn tick(&mut self) -> PpuIrq {
        let mut irq = PpuIrq { vblank: false, stat: false };
        if !self.on() {
            self.landed.wx_fresh = false;
            self.landed.tile_sel_glitch = false;
            return irq;
        }
        self.dot += 1;
        if self.dot == 456 {
            self.fifo_force_end();
            self.dot = 0;
            self.first_line = false;
            self.ly += 1;
            if self.ly == 154 {
                self.ly = 0;
                self.window_line = 0xFF;
                self.wy_hit = false;
            }
        }
        // The window's Y comparison: at the start of mode 2 and on every write to WY or LCDC,
        // which comes to every dot (see `wy_check`).
        self.wy_check();
        if self.ly < 144 {
            match self.dot {
                0 => self.mode = 2,
                d @ 80 => {
                    self.mode = 3;
                    self.hblank_at = d + self.mode3_length();
                    // A skipped frame draws nothing either way; the per-line drawer keeps the
                    // window's line count right for the frames after it.
                    if self.exact && !self.no_render {
                        self.fifo_start();
                    } else {
                        self.render_line();
                    }
                }
                d if d == self.hblank_at && self.mode == 3 => self.mode = 0,
                _ => {}
            }
        } else if self.ly == 144 && self.dot == 0 {
            self.fifo_force_end();
            self.mode = 1;
            irq.vblank = true;
            if self.skip_frame {
                self.skip_frame = false;
                self.frame.iter_mut().for_each(|p| *p = 0x00FF_FFFF);
            }
            self.frame_ready = true;
        }
        if self.fifo.active {
            self.fifo_dot();
        }
        // A write's one-dot effects last for the dot after it lands, and no longer.
        self.landed.wx_fresh = false;
        self.landed.tile_sel_glitch = false;
        // While LY ticks over (the last 4 dots of a line) the comparison reads false.
        self.lyc_eq = !(self.dot >= 452 && self.ly < 153) && self.ly == self.lyc;
        let line = self.stat_condition();
        if line && !self.stat_line {
            irq.stat = true;
        }
        self.stat_line = line;
        irq
    }

    /// BCPD/OCPD reads and writes, with the auto-increment of BCPS/OCPS.
    pub fn read_pal(&self, obj: bool) -> u8 {
        let (spec, pal) = if obj { (self.ocps, &self.obj_pal) } else { (self.bcps, &self.bg_pal) };
        pal[(spec & 0x3F) as usize]
    }

    pub fn write_pal(&mut self, obj: bool, v: u8) {
        let (spec, pal) = if obj {
            (&mut self.ocps, &mut self.obj_pal)
        } else {
            (&mut self.bcps, &mut self.bg_pal)
        };
        pal[(*spec & 0x3F) as usize] = v;
        if *spec & 0x80 != 0 {
            *spec = 0x80 | ((*spec + 1) & 0x3F);
        }
    }

    pub fn vram_read(&self, addr: u16) -> u8 {
        self.vram[(addr & 0x1FFF) as usize]
    }

    fn tile_row(&self, bank: usize, tile_addr: u16, row: u8) -> (u8, u8) {
        let a = bank * 0x2000 + ((tile_addr + u16::from(row) * 2) as usize & 0x1FFF);
        (self.vram[a], self.vram[a + 1])
    }

    pub fn bg_tile_addr(&self, index: u8) -> u16 {
        if self.lcdc & 0x10 != 0 {
            0x8000 + u16::from(index) * 16
        } else {
            (0x9000i32 + i32::from(index as i8) * 16) as u16
        }
    }

    /// One background or window pixel: (colour 0-3, CGB attribute byte).
    fn map_pixel(&self, map: u16, x: u8, y: u8) -> (u8, u8) {
        let at = map + u16::from(y / 8) * 32 + u16::from(x / 8);
        let tile = self.vram_read(at);
        let attr = if self.cgb { self.vram[0x2000 + (at & 0x1FFF) as usize] } else { 0 };
        let row = if attr & 0x40 != 0 { 7 - y % 8 } else { y % 8 };
        let bank = usize::from(attr & 0x08 != 0);
        let (lo, hi) = self.tile_row(bank, self.bg_tile_addr(tile), row);
        let bit = if attr & 0x20 != 0 { x % 8 } else { 7 - x % 8 };
        (((hi >> bit) & 1) << 1 | ((lo >> bit) & 1), attr)
    }

    /// The boot expands the cartridge logo into tiles 1-24 and leaves the ® mark in tile 25.
    pub(crate) fn boot_logo(&mut self, rom: &[u8]) {
        if rom.len() < 0x134 {
            return;
        }
        let mut hl = 0x10usize;
        let mut carry = false;
        let rot = |v: u8, carry: bool| -> (u8, bool) {
            (((v << 1) | u8::from(carry)), v & 0x80 != 0)
        };
        for &data in &rom[0x104..0x134] {
            let mut c = data;
            for _ in 0..2 {
                let mut a = 0u8;
                for _ in 0..4 {
                    let saved = c;
                    let (_, bit) = rot(c, carry);
                    carry = bit;
                    let (a1, abit) = rot(a, carry);
                    a = a1;
                    carry = abit;
                    c = saved;
                    let (c2, bit) = rot(c, carry);
                    c = c2;
                    carry = bit;
                    let (a2, abit) = rot(a, carry);
                    a = a2;
                    carry = abit;
                }
                self.vram[hl] = a;
                self.vram[hl + 2] = a;
                hl += 4;
            }
        }
        for b in [0x3C, 0x42, 0xB9, 0xA5, 0xB9, 0xA5, 0x42, 0x3C] {
            self.vram[hl] = b;
            hl += 2;
        }
    }

    fn cgb_colour(pal: &[u8], index: usize) -> u32 {
        let c = u16::from(pal[index * 2]) | u16::from(pal[index * 2 + 1]) << 8;
        let ch = |v: u16| -> u32 {
            let v = u32::from(v & 0x1F);
            (v << 3) | (v >> 2)
        };
        ch(c) << 16 | ch(c >> 5) << 8 | ch(c >> 10)
    }

    /// The colour of a Game Boy-mode pixel, from its shade after BGP, OBP0 or OBP1. A Game Boy
    /// shows the shade as it is. A Color in Game Boy mode still draws through its palette memory,
    /// with the shade as the colour number: the background through palette 0 and an object
    /// through palette 0 or 1 as its OBP0/OBP1 bit says, which is where its start-up code wrote
    /// the colours it picked for the game (Pan Docs, "Compatibility palettes").
    fn mono_colour(&self, obj: bool, pal: u8, shade: u8) -> u32 {
        if !self.cgb_hardware {
            return SHADES[usize::from(shade & 3)];
        }
        let memory = if obj { &self.obj_pal } else { &self.bg_pal };
        Self::cgb_colour(memory, usize::from(pal & 1) * 4 + usize::from(shade & 3))
    }

    /// The Game Boy logo's map, as a Game Boy's own start-up code leaves it: the logo's two rows
    /// of twelve tiles in the middle of the first map, with the trademark tile at the end of the
    /// top row. A Color's start-up code draws it only for the two titles that need it
    /// (`compat::draws_the_logo`).
    pub(crate) fn dmg_logo_map(&mut self) {
        for i in 0..12u8 {
            self.vram[0x1904 + usize::from(i)] = 1 + i;
            self.vram[0x1924 + usize::from(i)] = 13 + i;
        }
        self.vram[0x1910] = 0x19;
    }

    fn render_line(&mut self) {
        if self.no_render {
            // Frame skip: only the window's line counter, which later lines depend on.
            if (self.cgb || self.lcdc & 1 != 0) && self.lcdc & 0x20 != 0 && self.wy_hit && self.wx <= 166 {
                self.window_line = self.window_line.wrapping_add(1);
            }
            return;
        }
        let ly = self.ly;
        let cgb = self.cgb;
        let mut bg_index = [0u8; W];
        let mut bg_attr = [0u8; W];
        let row = &mut [0u32; W];
        // On the DMG LCDC bit 0 switches the background and window off; on the CGB it only
        // takes their priority over sprites away.
        let bg_on = cgb || self.lcdc & 1 != 0;
        if bg_on {
            let map = if self.lcdc & 0x08 != 0 { 0x9C00u16 } else { 0x9800 };
            let y = ly.wrapping_add(self.scy);
            if self.layers & 1 != 0 {
                for x in 0..W {
                    let sx = (x as u8).wrapping_add(self.scx);
                    (bg_index[x], bg_attr[x]) = self.map_pixel(map, sx, y);
                }
            }
            if self.lcdc & 0x20 != 0 && self.wy_hit && self.wx <= 166 {
                let map = if self.lcdc & 0x40 != 0 { 0x9C00u16 } else { 0x9800 };
                // The counter advances when the window starts, which here is once a line.
                self.window_line = self.window_line.wrapping_add(1);
                let wy = self.window_line;
                let start = i16::from(self.wx) - 7;
                for x in 0..W {
                    let wx = x as i16 - start;
                    if wx < 0 {
                        continue;
                    }
                    if self.layers & 2 != 0 {
                        (bg_index[x], bg_attr[x]) = self.map_pixel(map, wx as u8, wy);
                    }
                }
            }
        }
        let mut shade = [0u8; W];
        let mut source = [0u8; W];
        for x in 0..W {
            row[x] = if cgb {
                Self::cgb_colour(&self.bg_pal, usize::from(bg_attr[x] & 7) * 4 + usize::from(bg_index[x]))
            } else {
                shade[x] = (self.bgp >> (bg_index[x] * 2)) & 3;
                self.mono_colour(false, 0, shade[x])
            };
        }
        if self.lcdc & 0x02 != 0 && self.layers & 4 != 0 {
            let tall = self.lcdc & 0x04 != 0;
            let height: i16 = if tall { 16 } else { 8 };
            let mut picked: Vec<usize> = Vec::with_capacity(10);
            for i in 0..40 {
                let y = i16::from(self.oam[i * 4]) - 16;
                let line = i16::from(ly);
                if line >= y && line < y + height {
                    picked.push(i);
                    if picked.len() == 10 && !self.no_sprite_limit {
                        break;
                    }
                }
            }
            // The DMG ranks by X, then OAM index; the CGB by OAM index alone (unless OPRI says
            // DMG order). The first opaque sprite pixel owns the dot, even when it is hidden.
            if !cgb || self.opri & 1 != 0 {
                picked.sort_by_key(|&i| (self.oam[i * 4 + 1], i));
            }
            let mut owned = [false; W];
            for &i in picked.iter() {
                let y = i16::from(self.oam[i * 4]) - 16;
                let x0 = i16::from(self.oam[i * 4 + 1]) - 8;
                let mut tile = self.oam[i * 4 + 2];
                let attr = self.oam[i * 4 + 3];
                let mut r = (i16::from(ly) - y) as u8;
                if attr & 0x40 != 0 {
                    r = (height as u8 - 1) - r;
                }
                if tall {
                    tile &= 0xFE;
                }
                let bank = usize::from(cgb && attr & 0x08 != 0);
                let (lo, hi) = self.tile_row(bank, 0x8000 + u16::from(tile) * 16, r);
                for px in 0..8i16 {
                    let x = x0 + px;
                    if !(0..W as i16).contains(&x) || owned[x as usize] {
                        continue;
                    }
                    let bit = if attr & 0x20 != 0 { px } else { 7 - px } as u8;
                    let c = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
                    if c == 0 {
                        continue;
                    }
                    let x = x as usize;
                    owned[x] = true;
                    let behind = if cgb {
                        self.lcdc & 1 != 0
                            && bg_index[x] != 0
                            && (attr & 0x80 != 0 || bg_attr[x] & 0x80 != 0)
                    } else {
                        attr & 0x80 != 0 && bg_index[x] != 0
                    };
                    if behind {
                        continue;
                    }
                    row[x] = if cgb {
                        Self::cgb_colour(&self.obj_pal, usize::from(attr & 7) * 4 + usize::from(c))
                    } else {
                        let pal = if attr & 0x10 != 0 { self.obp1 } else { self.obp0 };
                        shade[x] = (pal >> (c * 2)) & 3;
                        source[x] = 1 + u8::from(attr & 0x10 != 0);
                        self.mono_colour(true, source[x] - 1, shade[x])
                    };
                }
            }
        }
        let base = ly as usize * W;
        self.frame[base..base + W].copy_from_slice(row);
        if self.shade.len() == W * H {
            self.shade[base..base + W].copy_from_slice(&shade);
            self.source[base..base + W].copy_from_slice(&source);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line of dots.
    const LINE: usize = 456;

    /// The window's Y match is only made while the window is switched on, so a game that
    /// switches it on after the line WY names has gone by waits for the next frame.
    #[test]
    fn the_window_waits_for_the_next_frame_when_it_is_switched_on_late() {
        // LCD on, background on, window off.
        let mut p = Ppu { lcdc: 0x91, wy: 5, ..Ppu::default() };
        for _ in 0..LINE * 20 {
            p.tick();
        }
        assert!(!p.wy_hit, "line 5 went by with the window switched off");
        p.lcdc |= 0x20;
        for _ in 0..LINE * 100 {
            p.tick();
        }
        assert!(!p.wy_hit, "switched on at line 20, so this frame gets no window");
        while !(p.ly == 5 && p.dot > 0) {
            p.tick();
        }
        assert!(p.wy_hit, "the next frame reaches line 5 with the window on");
    }

    /// The comparison runs again on a write, so switching the window on during the line WY
    /// names still catches the match.
    #[test]
    fn switching_the_window_on_during_its_own_line_catches_the_match() {
        let mut p = Ppu { lcdc: 0x91, wy: 5, ..Ppu::default() };
        while p.ly != 5 {
            p.tick();
        }
        for _ in 0..40 {
            p.tick();
        }
        assert!(!p.wy_hit);
        p.lcdc |= 0x20;
        p.tick();
        assert!(p.wy_hit, "a write to LCDC runs the comparison again");
    }

    /// A machine on the exact drawer whose background map is a tile of colour 1 and whose window
    /// map is a tile of colour 3, so every drawn pixel says which of the two it came from (and a
    /// pixel the LCD never got stays white).
    fn window_machine() -> Ppu {
        let mut p = Ppu {
            exact: true,
            // LCD on, window map $9C00, window on, tile data from $8000, background on.
            lcdc: 0x80 | 0x40 | 0x20 | 0x10 | 0x01,
            bgp: 0xE4, // colour 1 -> shade 1, colour 3 -> shade 3
            wy: 0,
            wx: 166,
            ..Ppu::default()
        };
        for row in 0..8 {
            p.vram[0x0010 + row * 2] = 0xFF; // tile 1: both bitplanes set, colour 3
            p.vram[0x0011 + row * 2] = 0xFF;
            p.vram[0x0020 + row * 2] = 0xFF; // tile 2: bitplane 0 only, colour 1
        }
        p.vram[0x1800..0x1C00].fill(2); // the background map
        p.vram[0x1C00..0x2000].fill(1); // the window map
        p
    }

    /// WX 166 on a DMG: no window pixel fits on the line itself, but the window is left running
    /// when the line ends, so the whole of the next line is window.
    #[test]
    fn wx_166_leaves_the_window_running_into_the_next_line() {
        let mut p = window_machine();
        for _ in 0..LINE * 2 {
            p.tick();
        }
        assert!(p.frame[..W].iter().all(|c| *c == SHADES[1]), "line 0 is all background");
        assert!(p.frame[W..W * 2].iter().all(|c| *c == SHADES[3]), "line 1 is all window");
    }

    /// Runs line 0 on the exact drawer with the given objects (OAM Y, X) and returns the dot the
    /// drawer handed the LCD its last pixel on together with the dot `mode3_length` puts mode 0
    /// at. Both are measured from the same line, so the difference between two runs is the cost
    /// of what changed between them.
    fn mode3_dots(objs: &[(u8, u8)], scx: u8, tall: bool, cgb: bool, obj_en: bool) -> (u16, u16) {
        let mut p = Ppu {
            exact: true,
            cgb,
            // LCD on, tile data $8000, background on, plus the objects under test.
            lcdc: 0x80 | 0x10 | 0x01 | u8::from(obj_en) << 1 | u8::from(tall) << 2,
            scx,
            ..Ppu::default()
        };
        for (i, (y, x)) in objs.iter().enumerate() {
            p.oam[i * 4] = *y;
            p.oam[i * 4 + 1] = *x;
        }
        while !p.fifo.active {
            p.tick();
        }
        let (hblank_at, mut end) = (p.hblank_at, 0);
        while p.fifo.active && p.dot < 455 {
            p.tick();
            end = p.dot;
        }
        (end, hblank_at)
    }

    /// Sprite layouts to measure: one object at every X, pairs that share a tile and pairs that
    /// do not, and the ten-object limit at every spacing (inserted in the reverse of X order, so
    /// the OAM order is not the fetch order).
    fn object_layouts() -> Vec<Vec<(u8, u8)>> {
        let mut out: Vec<Vec<(u8, u8)>> = (0..=24u8).map(|x| vec![(16, x)]).collect();
        out.extend([40u8, 83, 159, 160, 164, 167, 168, 175].map(|x| vec![(16, x)]));
        for a in 0..=16u8 {
            for b in a..=20u8 {
                out.push(vec![(16, a), (16, b)]);
            }
        }
        for step in 0..=9u8 {
            out.push((0..10u8).map(|i| (16, (9 - i) * step)).collect());
        }
        out
    }

    /// Mode 3 ends on the exact drawer where STAT says it does, for every sprite layout: an
    /// object's stall IS mode 3's extra length, not a number worked out beside it. Both drawers
    /// take their STAT and HBlank timing from `mode3_length`, so this is what stops the picture
    /// and the timing the CPU can see from drifting apart. The drawer counts single dots and
    /// STAT shows whole M-cycles, which is the only difference allowed here.
    #[test]
    fn the_object_stalls_are_mode_3s_length() {
        // A DMG, a Color, a Color with objects switched off (which still fetches them), and
        // 8x16 objects.
        for (cgb, obj_en, tall) in [(false, true, false), (true, true, false), (true, false, false), (false, true, true)] {
            for scx in 0..8u8 {
                let (base_end, base_hb) = mode3_dots(&[], scx, tall, cgb, obj_en);
                for objs in object_layouts() {
                    let (end, hb) = mode3_dots(&objs, scx, tall, cgb, obj_en);
                    assert_eq!(
                        (end - base_end) & !3,
                        hb - base_hb,
                        "cgb {cgb} objects on {obj_en} tall {tall} scx {scx}: the drawer took \
                         {} dots over the objects {objs:?} and STAT allowed {}",
                        end - base_end,
                        hb - base_hb
                    );
                }
            }
        }
    }

    /// An object at X=0 is fetched before the line's fine-scroll discard has run, because the
    /// discard is held while one is pending, so it always waits a whole tile fetch: five dots of
    /// background fetch and six of its own, whatever SCX's low bits say.
    #[test]
    fn an_object_at_x_0_waits_a_whole_tile_fetch_whatever_scx_says() {
        for scx in 0..8u8 {
            let (base_end, base_hb) = mode3_dots(&[], scx, false, false, true);
            let (end, hb) = mode3_dots(&[(16, 0)], scx, false, false, true);
            assert_eq!(end - base_end, 11, "scx {scx}: five dots of tile fetch, six of object");
            assert_eq!(hb - base_hb, 8, "scx {scx}: the same, in the whole M-cycles STAT shows");
        }
    }

    /// A Color runs the object fetch even with LCDC bit 1 clear - it only drops the pixel as it
    /// leaves the FIFO - so mode 3 is just as long as with objects switched on. A DMG skips the
    /// fetch outright, and pays nothing.
    #[test]
    fn a_colour_pays_for_an_object_it_has_been_told_not_to_draw() {
        let cost = |cgb: bool, obj_en: bool| {
            let (base_end, base_hb) = mode3_dots(&[], 0, false, cgb, obj_en);
            let (end, hb) = mode3_dots(&[(16, 8)], 0, false, cgb, obj_en);
            (end - base_end, hb - base_hb)
        };
        assert_eq!(cost(true, false), cost(true, true), "a Color fetches the object either way");
        assert_eq!(cost(true, false), (11, 8), "and mode 3 is eleven dots longer for it");
        assert_eq!(cost(false, false), (0, 0), "a DMG skips the fetch, so it costs nothing");
        assert_eq!(cost(false, true), (11, 8));
    }

    /// WX 167 never starts the window at all: it would need a position of 161.
    #[test]
    fn wx_167_never_starts_the_window() {
        let mut p = window_machine();
        p.wx = 167;
        for _ in 0..LINE * 3 {
            p.tick();
        }
        assert!(p.frame[..W * 3].iter().all(|c| *c == SHADES[1]), "every line is background");
    }

    /// Runs line 143 in whole M-cycles, four dots at a time with the two interrupts OR-ed
    /// together, which is exactly how `Bus::tick` hands them to the CPU. Returns the M-cycle of
    /// the line that asks for the mode 2 STAT interrupt and the one that asks for VBlank, both
    /// counted from the start of line 143. Mode 2 is the only STAT source switched on, and the
    /// line is already high as line 143 begins, so the first rise seen is line 144's.
    fn line_144_interrupt_m_cycles(cgb_hardware: bool) -> (u32, u32) {
        let mut p = Ppu { lcdc: 0x80, cgb_hardware, ..Ppu::default() };
        p.write_stat(0x20); // STAT bit 5: mode 2 only
        while !(p.ly == 143 && p.dot == 0) {
            p.tick();
        }
        let (mut stat_at, mut vblank_at) = (None, None);
        for m in 0..LINE as u32 / 4 {
            let (mut stat, mut vblank) = (false, false);
            for _ in 0..4 {
                let irq = p.tick();
                stat |= irq.stat;
                vblank |= irq.vblank;
            }
            if stat && stat_at.is_none() {
                stat_at = Some(m);
            }
            if vblank && vblank_at.is_none() {
                vblank_at = Some(m);
            }
        }
        (stat_at.expect("the mode 2 interrupt was never asked for"),
         vblank_at.expect("VBlank was never asked for"))
    }

    /// Line 144's mode 2 STAT interrupt against VBlank. A Color raises it one M-cycle early, like
    /// every other line's, so it beats VBlank; a Game Boy holds it to the start of line 144 and
    /// the two land together. The M-cycle numbers also pin the alignment the rule rests on: a
    /// 456-dot line run four dots at a time puts dot 452 at the end of M-cycle 112 and dot 0 of
    /// the next line at the end of M-cycle 113.
    ///
    /// This is the whole of what Mooneye's vblank_stat_intr-GS and -C disagree about: both time
    /// the interrupt with DIV against a run of `nop`s, and the Color test uses one `nop` fewer.
    #[test]
    fn line_144s_mode_2_interrupt_beats_vblank_only_on_a_colour() {
        assert_eq!(line_144_interrupt_m_cycles(false), (113, 113),
                   "a Game Boy: the mode 2 interrupt and VBlank are asked for together");
        assert_eq!(line_144_interrupt_m_cycles(true), (112, 113),
                   "a Color: the mode 2 interrupt is asked for one M-cycle before VBlank");
    }

    /// The Color's early raise is line 144's own interrupt moved, not a second one: across lines
    /// 143 and 144 together each console asks for the mode 2 interrupt exactly once. Line 143's
    /// own interrupt is already asked for before this count starts, at the end of line 142.
    #[test]
    fn a_colour_gets_no_extra_mode_2_interrupt_at_line_144() {
        let count = |cgb_hardware: bool| {
            let mut p = Ppu { lcdc: 0x80, cgb_hardware, ..Ppu::default() };
            p.write_stat(0x20);
            while !(p.ly == 143 && p.dot == 0) {
                p.tick();
            }
            (0..LINE * 2).filter(|_| p.tick().stat).count()
        };
        assert_eq!(count(false), 1, "a Game Boy: one, at the start of line 144");
        assert_eq!(count(true), 1, "a Color: the same one, an M-cycle earlier");
    }
}

