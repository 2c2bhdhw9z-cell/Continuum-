//! The LCD controller (an SED1565-style chip): its own picture memory of nine 132-column pages,
//! written by the PRC's copy or directly by a program through $20FE (commands) and $20FF
//! (data), with the commands for the start line, mirroring, inverting, all-on, power and the
//! contrast; and the panel's 96×64 pixels.
//!
//! As PokeMini models it (the emulator cMini replaces), including how the panel looks: the
//! contrast setting picks how dark an unlit and a lit pixel are, and a pixel fades in and out
//! over four frames ("analog"), or shows two or three shades.

use serde::{Deserialize, Serialize};

pub const WIDTH: usize = 96;
pub const HEIGHT: usize = 64;
const PAGE: usize = 256;
const MEMORY: usize = PAGE * 9;

/// How dark an unlit and a lit pixel are (0 the panel's lightest, 255 its darkest) at each of
/// the 64 contrast settings.
const CONTRAST: [(u8, u8); 64] = [
    (0, 4),
    (0, 4),
    (0, 4),
    (0, 4),
    (0, 6),
    (0, 11),
    (0, 17),
    (0, 24),
    (0, 31),
    (0, 40),
    (0, 48),
    (0, 57),
    (0, 67),
    (0, 77),
    (0, 88),
    (0, 99),
    (0, 110),
    (0, 122),
    (0, 133),
    (0, 146),
    (0, 158),
    (0, 171),
    (0, 184),
    (0, 198),
    (0, 212),
    (0, 226),
    (0, 240),
    (0, 255),
    (2, 255),
    (5, 255),
    (10, 255),
    (15, 255),
    (21, 255),
    (27, 255),
    (34, 255),
    (41, 255),
    (48, 255),
    (56, 255),
    (64, 255),
    (73, 255),
    (81, 255),
    (90, 255),
    (100, 255),
    (109, 255),
    (119, 255),
    (129, 255),
    (139, 255),
    (149, 255),
    (160, 255),
    (171, 255),
    (182, 255),
    (193, 255),
    (204, 255),
    (216, 255),
    (228, 255),
    (240, 255),
    (240, 255),
    (240, 255),
    (240, 255),
    (240, 255),
    (240, 255),
    (240, 255),
    (240, 255),
    (240, 255),
];

/// How the panel's pixels are shown.
pub const ANALOG: u8 = 0;
pub const THREE_SHADES: u8 = 1;
pub const TWO_SHADES: u8 = 2;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Lcd {
    /// The controller's picture memory: nine pages of 256 (132 columns used).
    pub memory: Vec<u8>,
    /// The panel's pixels as the controller last showed them (0 or 1).
    pub pixels: Vec<u8>,
    /// Analog: each pixel's darkness now. Three shades: the pixels the frame before.
    pub shade: Vec<u8>,
    /// Analog: each pixel's last four frames, one bit each.
    history: Vec<u8>,
    /// How dark an unlit and a lit pixel are now.
    pub off_level: i32,
    pub on_level: i32,
    column: u8,
    start_line: u8,
    /// The next command or data byte is the contrast.
    contrast_next: bool,
    contrast: u8,
    mirrored: bool,
    max_contrast: bool,
    all_on: u8,
    inverted: u8,
    pub display_on: bool,
    page: u8,
    upside_down: bool,
    read_modify: bool,
    dummy_read: bool,
    read_modify_column: u8,
    /// The last byte a data read gave (a dummy read gives it again).
    read_latch: u8,
    /// Something seen has changed since the panel was last shown.
    pub dirty: bool,
}

impl Default for Lcd {
    fn default() -> Self {
        let mut lcd = Self {
            memory: vec![0; MEMORY],
            pixels: vec![0; WIDTH * HEIGHT],
            shade: vec![0; WIDTH * HEIGHT],
            history: vec![0; WIDTH * HEIGHT],
            off_level: 0,
            on_level: 0,
            column: 0,
            start_line: 0,
            contrast_next: false,
            contrast: 0,
            mirrored: false,
            max_contrast: false,
            all_on: 0,
            inverted: 0,
            display_on: false,
            page: 0,
            upside_down: false,
            read_modify: false,
            dummy_read: false,
            read_modify_column: 0,
            read_latch: 0x40,
            dirty: false,
        };
        lcd.set_contrast(0x1F);
        lcd
    }
}

impl Lcd {
    /// The reset line: memory, pixels and settings cleared, contrast $1F. (The read latch
    /// keeps its byte.)
    pub fn reset(&mut self) {
        let latch = self.read_latch;
        *self = Self::default();
        self.read_latch = latch;
    }

    fn set_contrast(&mut self, value: u8) {
        self.contrast = value & 0x3F;
        let (off, on) = if self.max_contrast { (240, 255) } else { CONTRAST[usize::from(self.contrast)] };
        self.off_level = i32::from(off);
        self.on_level = i32::from(on);
        self.dirty = true;
    }

    /// Where the column and page point in the controller's memory (mirrored: from the right).
    fn at(&self) -> Option<usize> {
        let column = i32::from(self.column);
        let i = if self.mirrored { 131 - column } else { column } + i32::from(self.page) * PAGE as i32;
        usize::try_from(i).ok().filter(|&i| i < MEMORY)
    }

    pub fn read(&mut self, reg: usize, cpu: bool) -> u8 {
        if reg == 0xFE {
            if self.contrast_next {
                self.contrast_next = false;
                self.set_contrast(0x3F);
                return 0;
            }
            return 0x40 | if self.display_on { 0x20 } else { 0 };
        }
        if self.contrast_next {
            self.contrast_next = false;
            self.set_contrast(0x3F);
            self.read_latch = 0;
        } else if !self.dummy_read && cpu {
            self.read_latch = self.at().map_or(0, |i| self.memory[i]);
            if self.page >= 8 {
                self.read_latch &= 0x01;
            }
            if !self.read_modify {
                self.column = self.column.wrapping_add(1).min(131);
                self.dummy_read = true;
            }
        } else {
            self.dummy_read = false;
        }
        self.read_latch
    }

    pub fn write(&mut self, reg: usize, value: u8) {
        if reg == 0xFE {
            self.command(value);
            return;
        }
        if self.contrast_next {
            self.contrast_next = false;
            self.set_contrast(value & 0x3F);
            return;
        }
        if let Some(i) = self.at() {
            self.memory[i] = value;
        }
        self.column = self.column.wrapping_add(1).min(131);
        self.dummy_read = true;
        self.dirty = true;
    }

    fn command(&mut self, c: u8) {
        if self.contrast_next {
            self.contrast_next = false;
            self.set_contrast(c & 0x3F);
            return;
        }
        match c {
            0x00..=0x0F if !self.read_modify => {
                self.column = (self.column & 0xF0) | (c & 0x0F);
                self.dummy_read = true;
            }
            0x10..=0x1F if !self.read_modify => {
                self.column = (self.column & 0x0F) | ((c & 0x0F) << 4);
                self.dummy_read = true;
            }
            0x40..=0x7F => {
                self.start_line = c - 0x40;
                self.dirty = true;
            }
            0x81 => self.contrast_next = true,
            0xA0 | 0xA1 => {
                self.mirrored = c == 0xA1;
                self.dirty = true;
            }
            0xA2 | 0xA3 => {
                self.max_contrast = c == 0xA3;
                self.set_contrast(self.contrast);
            }
            0xA4 | 0xA5 => {
                self.all_on = c & 1;
                self.dirty = true;
            }
            0xA6 | 0xA7 => {
                self.inverted = c & 1;
                self.dirty = true;
            }
            0xAE | 0xAF => {
                self.display_on = c == 0xAF;
                self.dirty = true;
            }
            0xB0..=0xB8 => self.page = c & 0x0F,
            0xC0..=0xCF => {
                self.upside_down = c >= 0xC8;
                self.dirty = true;
            }
            0xE0 => {
                self.read_modify = true;
                self.read_modify_column = self.column;
            }
            0xE2 => {
                // The controller's reset: the contrast is set before its maximum is cleared.
                self.contrast_next = false;
                self.set_contrast(0x20);
                self.column = 0;
                self.start_line = 0;
                self.mirrored = false;
                self.max_contrast = false;
                self.all_on = 0;
                self.inverted = 0;
                self.display_on = false;
                self.page = 0;
                self.upside_down = false;
                self.read_modify = false;
                self.dummy_read = true;
                self.read_modify_column = 0;
            }
            0xEE => {
                self.read_modify = false;
                self.column = self.read_modify_column;
            }
            _ => {}
        }
    }

    /// The PRC's copy of its frame buffer (8 rows of 96 bytes) into pages 0-7.
    pub fn copy_frame(&mut self, buffer: &[u8]) {
        for page in 0..8 {
            for i in 0..WIDTH {
                let to = page * PAGE + if self.mirrored { 131 - i } else { i };
                self.memory[to] = buffer[page * WIDTH + i];
            }
        }
        self.page = 7;
        self.column = 96;
        self.dummy_read = true;
        self.read_modify = false;
        self.dirty = true;
    }

    /// The panel shows the controller's memory.
    pub fn show(&mut self) {
        if self.display_on {
            for y in 0..HEIGHT {
                let mut line = (y + usize::from(self.start_line)) & 63;
                if self.upside_down {
                    line = 63 - line;
                }
                for x in 0..WIDTH {
                    let bit = (self.memory[(line >> 3) * PAGE + x] >> (line & 7)) & 1;
                    self.pixels[y * WIDTH + x] = (bit ^ self.inverted) | self.all_on;
                }
            }
        } else {
            self.pixels.fill(0);
        }
        self.dirty = false;
    }

    /// The end of a frame: the panel catches up, and the analog panel's pixels fade.
    pub fn end_frame(&mut self, mode: u8, worked: bool) {
        if mode == THREE_SHADES && worked {
            self.shade.copy_from_slice(&self.pixels);
        }
        if self.dirty {
            self.show();
        }
        if mode == ANALOG {
            for i in 0..WIDTH * HEIGHT {
                let lit = if self.display_on && self.pixels[i] != 0 { 0x08 } else { 0 };
                let recent = lit | (self.history[i] >> 1);
                self.history[i] = recent;
                let n = recent.count_ones() as i32;
                self.shade[i] = ((self.off_level * (4 - n) + self.on_level * n) >> 2) as u8;
            }
        }
    }

    /// How dark each pixel looks (0-255) in the given mode.
    pub fn darkness(&self, mode: u8, i: usize) -> u8 {
        let (off, on) = (self.off_level, self.on_level);
        (match mode {
            ANALOG => i32::from(self.shade[i]),
            THREE_SHADES => match self.pixels[i] + self.shade[i] {
                2 => on,
                1 => (off + on) >> 1,
                _ => off,
            },
            _ => {
                if self.pixels[i] != 0 {
                    on
                } else {
                    off
                }
            }
        }) as u8
    }
}
