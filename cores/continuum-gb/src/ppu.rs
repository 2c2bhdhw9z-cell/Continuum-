//! The picture processing unit: modes 2/3/0 on each of lines 0-143, then VBlank (lines 144-153),
//! 456 dots a line, with LY, LYC, STAT and its interrupt line, and a scanline renderer for the
//! background, window and sprites. Pixels are DMG shades 0-3 (after the palettes).
//!
//! Mode 3 is a fixed 172 dots. The renderer draws a whole line as mode 3 begins, which is what
//! every test here and nearly every game needs; a dot-accurate FIFO is a later step.

use serde::{Deserialize, Serialize};

pub const W: usize = 160;
pub const H: usize = 144;

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
    dot: u16,
    mode: u8,
    stat_line: bool,
    window_line: u8,
    wy_hit: bool,
    #[serde(skip, default = "blank")]
    pub frame: Vec<u8>,
    /// Set when line 144 begins (a whole frame is in `frame`).
    #[serde(skip)]
    pub frame_ready: bool,
    /// The first frame after the LCD is switched on is not shown, as on hardware.
    skip_frame: bool,
    /// The dot mode 0 starts at on this line (mode 3 is 172 dots plus SCX and sprite delays).
    #[serde(default)]
    hblank_at: u16,
}

fn blank() -> Vec<u8> {
    vec![0; W * H]
}

pub struct PpuIrq {
    pub vblank: bool,
    pub stat: bool,
}

impl Default for Ppu {
    fn default() -> Self {
        Ppu {
            vram: vec![0; 0x2000],
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
            frame: blank(),
            frame_ready: false,
            skip_frame: false,
            hblank_at: 252,
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

    pub fn read_stat(&self) -> u8 {
        let coinc = if self.ly == self.lyc { 4 } else { 0 };
        let mode = if self.on() { self.mode } else { 0 };
        0x80 | self.stat_enable | coinc | mode
    }

    pub fn write_stat(&mut self, v: u8) {
        self.stat_enable = v & 0x78;
    }

    pub fn write_lcdc(&mut self, v: u8) {
        let was = self.on();
        self.lcdc = v;
        if was && !self.on() {
            self.ly = 0;
            self.dot = 0;
            self.mode = 0;
            self.stat_line = false;
        } else if !was && self.on() {
            self.ly = 0;
            self.dot = 4; // the first line after switch-on starts a little late
            self.mode = 0; // and has no OAM scan
            self.window_line = 0;
            self.wy_hit = false;
            self.skip_frame = true;
        }
    }

    fn stat_condition(&self) -> bool {
        let e = self.stat_enable;
        (e & 0x40 != 0 && self.ly == self.lyc)
            || (e & 0x08 != 0 && (self.mode == 0 || (self.ly < 144 && self.dot + 4 >= self.hblank_at && self.mode == 3)))
            || (e & 0x10 != 0 && self.mode == 1)
            || (e & 0x20 != 0 && (self.mode == 2 || self.oam_irq_early()))
    }

    /// The mode 2 STAT interrupt is raised one M-cycle before mode 2 shows in STAT.
    fn oam_irq_early(&self) -> bool {
        self.dot >= 452 && self.ly < 143
    }

    /// Mode 3's length: 172 dots, plus the SCX fine scroll, plus each sprite on the line
    /// (6 dots, more while the background tile under it is still being fetched; Pan Docs).
    fn mode3_length(&self) -> u16 {
        let mut len = 172 + u16::from(self.scx & 7);
        if self.lcdc & 0x02 == 0 {
            return len;
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
            if x == 0 {
                len += 11;
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
        len
    }

    /// One dot.
    pub fn tick(&mut self) -> PpuIrq {
        let mut irq = PpuIrq { vblank: false, stat: false };
        if !self.on() {
            return irq;
        }
        self.dot += 1;
        if self.dot == 456 {
            self.dot = 0;
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
                80 => {
                    self.mode = 3;
                    self.hblank_at = 80 + self.mode3_length();
                    self.render_line();
                }
                d if d == self.hblank_at && self.mode == 3 => self.mode = 0,
                _ => {}
            }
        } else if self.ly == 144 && self.dot == 0 {
            self.mode = 1;
            irq.vblank = true;
            if self.skip_frame {
                self.skip_frame = false;
                self.frame.iter_mut().for_each(|p| *p = 0);
            }
            self.frame_ready = true;
        }
        let line = self.stat_condition();
        if line && !self.stat_line {
            irq.stat = true;
        }
        self.stat_line = line;
        irq
    }

    pub fn vram_read(&self, addr: u16) -> u8 {
        self.vram[(addr & 0x1FFF) as usize]
    }

    fn tile_row(&self, tile_addr: u16, row: u8) -> (u8, u8) {
        let a = (tile_addr + u16::from(row) * 2) as usize & 0x1FFF;
        (self.vram[a], self.vram[a + 1])
    }

    fn bg_tile_addr(&self, index: u8) -> u16 {
        if self.lcdc & 0x10 != 0 {
            0x8000 + u16::from(index) * 16
        } else {
            (0x9000i32 + i32::from(index as i8) * 16) as u16
        }
    }

    fn render_line(&mut self) {
        let ly = self.ly;
        let mut bg_index = [0u8; W];
        let row = &mut [0u8; W];
        // Background.
        if self.lcdc & 1 != 0 {
            let map = if self.lcdc & 0x08 != 0 { 0x9C00u16 } else { 0x9800 };
            let y = ly.wrapping_add(self.scy);
            for (x, px) in bg_index.iter_mut().enumerate() {
                let sx = (x as u8).wrapping_add(self.scx);
                let tile = self.vram_read(map + u16::from(y / 8) * 32 + u16::from(sx / 8));
                let (lo, hi) = self.tile_row(self.bg_tile_addr(tile), y % 8);
                let bit = 7 - (sx % 8);
                *px = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
            }
            // Window, over the background (LCDC bit 0 also gates it on the DMG).
            if self.lcdc & 0x20 != 0 && self.wy_hit && self.wx <= 166 {
                let map = if self.lcdc & 0x40 != 0 { 0x9C00u16 } else { 0x9800 };
                let wy = self.window_line;
                let start = i16::from(self.wx) - 7;
                let mut drawn = false;
                for (x, px) in bg_index.iter_mut().enumerate() {
                    let wx = x as i16 - start;
                    if wx < 0 {
                        continue;
                    }
                    drawn = true;
                    let wx = wx as u8;
                    let tile = self.vram_read(map + u16::from(wy / 8) * 32 + u16::from(wx / 8));
                    let (lo, hi) = self.tile_row(self.bg_tile_addr(tile), wy % 8);
                    let bit = 7 - (wx % 8);
                    *px = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
                }
                if drawn {
                    self.window_line += 1;
                }
            }
        }
        for x in 0..W {
            row[x] = (self.bgp >> (bg_index[x] * 2)) & 3;
        }
        // Sprites: the first ten on this line in OAM order; the lower X wins, then the lower
        // OAM index.
        if self.lcdc & 0x02 != 0 {
            let tall = self.lcdc & 0x04 != 0;
            let height: i16 = if tall { 16 } else { 8 };
            let mut picked: Vec<usize> = Vec::with_capacity(10);
            for i in 0..40 {
                let y = i16::from(self.oam[i * 4]) - 16;
                let line = i16::from(ly);
                if line >= y && line < y + height {
                    picked.push(i);
                    if picked.len() == 10 {
                        break;
                    }
                }
            }
            // Highest priority first; the first opaque sprite pixel owns the dot, and only then
            // is its behind-background bit applied (a hidden winner still hides the others).
            picked.sort_by_key(|&i| (self.oam[i * 4 + 1], i));
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
                let (lo, hi) = self.tile_row(0x8000 + u16::from(tile) * 16, r);
                let pal = if attr & 0x10 != 0 { self.obp1 } else { self.obp0 };
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
                    if attr & 0x80 != 0 && bg_index[x] != 0 {
                        continue;
                    }
                    row[x] = (pal >> (c * 2)) & 3;
                }
            }
        }
        let base = ly as usize * W;
        self.frame[base..base + W].copy_from_slice(row);
    }
}
