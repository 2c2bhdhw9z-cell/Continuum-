//! Pokémon Mini PRC tile/sprite renderer and 96×64 LCD memory.

use serde::{Deserialize, Serialize};

pub const WIDTH: usize = 96;
pub const HEIGHT: usize = 64;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prc {
    pub mode: u8,
    pub rate: u8,
    pub count: u8,
    phase: u32,
    active: bool,
    pub frame_divider: u8,
    pub bg_base: u32,
    pub sprite_base: u32,
    pub scroll_x: u8,
    pub scroll_y: u8,
    pub lcd: Vec<u8>,
    pub direct_column: u8,
    pub direct_page: u8,
}

impl Default for Prc {
    fn default() -> Self {
        Self {
            mode: 0,
            rate: 0,
            count: 0,
            phase: 0,
            active: false,
            frame_divider: 0,
            bg_base: 0,
            sprite_base: 0,
            scroll_x: 0,
            scroll_y: 0,
            lcd: vec![0; WIDTH * HEIGHT],
            direct_column: 0,
            direct_page: 0,
        }
    }
}

impl Prc {
    pub fn write_reg(&mut self, reg: u8, value: u8) {
        match reg {
            0x80 => self.mode = value & 0x3f,
            0x81 => self.rate = (self.rate & 0xf0) | (value & 0x0f),
            0x82 => self.bg_base = (self.bg_base & !0xff) | (value & 0xf8) as u32,
            0x83 => self.bg_base = (self.bg_base & !0xff00) | ((value as u32) << 8),
            0x84 => self.bg_base = (self.bg_base & 0xffff) | (((value & 0x1f) as u32) << 16),
            0x85 => self.scroll_y = value & 0x7f,
            0x86 => self.scroll_x = value & 0x7f,
            0x87 => self.sprite_base = (self.sprite_base & !0xff) | (value & 0xc0) as u32,
            0x88 => self.sprite_base = (self.sprite_base & !0xff00) | ((value as u32) << 8),
            0x89 => {
                self.sprite_base = (self.sprite_base & 0xffff) | (((value & 0x1f) as u32) << 16)
            }
            0xfe => match value {
                0xb0..=0xb7 => self.direct_page = value - 0xb0,
                0x00..=0x0f => self.direct_column = value & 0x0f,
                0x10..=0x1f => {
                    self.direct_column = (self.direct_column & 0x0f) | ((value & 0x0f) << 4)
                }
                _ => {}
            },
            0xff => {
                let x = self.direct_column as usize;
                if x < WIDTH {
                    for bit in 0..8 {
                        self.lcd[(self.direct_page as usize * 8 + bit) * WIDTH + x] =
                            (value >> bit) & 1;
                    }
                }
                self.direct_column = self.direct_column.wrapping_add(1);
            }
            _ => {}
        }
    }

    pub fn read_reg(&self, reg: u8) -> u8 {
        match reg {
            0x80 => self.mode,
            0x81 => (self.rate & 0x0f) | (self.frame_divider << 4),
            0x8a => self.count,
            _ => 0,
        }
    }

    pub fn tick(&mut self, cycles: u32, ram: &mut [u8], read: &impl Fn(u32) -> u8) -> (bool, bool) {
        self.phase += cycles;
        let mut draw_irq = false;
        let mut copy_irq = false;
        while self.phase >= 856 {
            self.phase -= 856;
            self.count = self.count.wrapping_add(1);
            if self.count == 0x18 {
                let limits = [3, 6, 9, 12, 2, 4, 6, 8];
                self.active = self.frame_divider >= limits[((self.rate >> 1) & 7) as usize];
                if self.active {
                    self.frame_divider = 0;
                    self.render(ram, read);
                    draw_irq = true;
                }
            } else if self.count == 0x39 && self.active && self.mode & 8 != 0 {
                self.copy(ram);
                copy_irq = true;
            } else if self.count >= 0x42 {
                self.count = 0;
                self.frame_divider = self.frame_divider.wrapping_add(1);
                self.active = false;
            }
        }
        (draw_irq, copy_irq)
    }

    fn render(&self, ram: &mut [u8], read: &impl Fn(u32) -> u8) {
        if self.mode & 2 != 0 {
            let map_w = match self.mode >> 4 & 3 {
                0 => 12,
                1 => 16,
                _ => 24,
            };
            for y in 0..8 {
                for x in 0..12 {
                    let tile = ram[0x360
                        + ((y + self.scroll_y as usize / 8) * map_w
                            + x
                            + self.scroll_x as usize / 8)
                            % (map_w * 16)];
                    for col in 0..8 {
                        ram[y * WIDTH + x * 8 + col] =
                            read(self.bg_base + tile as u32 * 8 + col as u32);
                    }
                }
            }
        }
        if self.mode & 4 != 0 {
            for n in (0..24).rev() {
                let o = 0x300 + n * 4;
                let x0 = (ram[o] & 0x7f) as i16 - 16;
                let y0 = (ram[o + 1] & 0x7f) as i16 - 16;
                let tile = ram[o + 2] as u32;
                let flags = ram[o + 3];
                if flags & 8 == 0 {
                    continue;
                }
                for x in 0..16i16 {
                    for y in 0..16i16 {
                        let sx = if flags & 1 != 0 { 15 - x } else { x } as u32;
                        let a = self.sprite_base + tile * 64 + ((sx & 8) << 2) + (sx & 7);
                        let mask = read(a + (y as u32 / 8) * 8);
                        if mask & (1 << (y & 7)) == 0 {
                            continue;
                        }
                        let bit =
                            read(a + 16 + (y as u32 / 8) * 8) >> (y & 7) & 1 ^ ((flags >> 2) & 1);
                        let px = x0 + x;
                        let py = y0 + y;
                        if (0..96).contains(&px) && (0..64).contains(&py) {
                            let idx = (py as usize / 8) * WIDTH + px as usize;
                            let m = 1 << (py as usize & 7);
                            if bit != 0 {
                                ram[idx] |= m;
                            } else {
                                ram[idx] &= !m;
                            }
                        }
                    }
                }
            }
        }
    }

    fn copy(&mut self, ram: &[u8]) {
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let bit = ram[(y / 8) * WIDTH + x] >> (y & 7) & 1;
                self.lcd[y * WIDTH + x] = bit ^ (self.mode & 1);
            }
        }
    }
}
