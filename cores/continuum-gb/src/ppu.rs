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
    /// Game Boy Color mode (from the cartridge header).
    pub cgb: bool,
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
            window_line: 0,
            wy_hit: false,
            lyc_eq: true,
            first_line: false,
            frame: blank(),
            frame_ready: false,
            skip_frame: false,
            hblank_at: 252,
            cgb: false,
            bg_pal: vec![0xFF; 64],
            layers: 7,
            no_render: false,
            no_sprite_limit: false,
            exact: false,
            fifo: Default::default(),
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
            self.window_line = 0;
            self.wy_hit = false;
            self.skip_frame = true;
        }
        self.restat()
    }

    fn stat_condition(&self) -> bool {
        let e = self.stat_enable;
        (e & 0x40 != 0 && self.lyc_eq)
            || (e & 0x08 != 0 && (self.mode == 0 || (self.ly < 144 && self.dot + 4 >= self.hblank_at && self.mode == 3)))
            || (e & 0x10 != 0 && self.mode == 1)
            || (e & 0x20 != 0 && (self.mode == 2 || self.oam_irq_early() || (self.ly == 144 && self.dot < 4)))
    }

    /// The mode 2 STAT interrupt is raised one M-cycle before mode 2 shows in STAT.
    fn oam_irq_early(&self) -> bool {
        self.dot >= 452 && self.ly < 143
    }

    /// Mode 3's length: 172 dots, plus the SCX fine scroll, plus each sprite on the line
    /// (6 dots, more while the background tile under it is still being fetched; Pan Docs).
    fn mode3_length(&self) -> u16 {
        let base = 172 + u16::from(self.scx & 7);
        let mut len = 0u16;
        if self.lcdc & 0x02 == 0 {
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
            let pos = u16::from(x) + u16::from(self.scx & 7);
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
                self.window_line = 0;
                self.wy_hit = false;
            }
        }
        if self.ly < 144 {
            match self.dot {
                0 => {
                    self.mode = 2;
                    if self.ly == self.wy {
                        self.wy_hit = true;
                    }
                }
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

    fn render_line(&mut self) {
        if self.no_render {
            // Frame skip: only the window's line counter, which later lines depend on.
            if (self.cgb || self.lcdc & 1 != 0) && self.lcdc & 0x20 != 0 && self.wy_hit && self.wx <= 166 {
                self.window_line += 1;
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
                let wy = self.window_line;
                let start = i16::from(self.wx) - 7;
                let mut drawn = false;
                for x in 0..W {
                    let wx = x as i16 - start;
                    if wx < 0 {
                        continue;
                    }
                    drawn = true;
                    if self.layers & 2 != 0 {
                        (bg_index[x], bg_attr[x]) = self.map_pixel(map, wx as u8, wy);
                    }
                }
                if drawn {
                    self.window_line += 1;
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
                SHADES[shade[x] as usize]
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
                        SHADES[shade[x] as usize]
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

