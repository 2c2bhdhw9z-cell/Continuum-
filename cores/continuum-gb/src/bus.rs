//! The memory map and everything clocked with the CPU. Every CPU memory access goes through
//! `read`/`write`, which first run one M-cycle of the rest of the machine, so instruction timing
//! falls out of the CPU doing its accesses in the right order.

use serde::{Deserialize, Serialize};

use crate::apu::Apu;
use crate::cart::Cart;
use crate::ppu::Ppu;
use crate::timer::Timer;

pub const INT_VBLANK: u8 = 1;
pub const INT_STAT: u8 = 2;
pub const INT_TIMER: u8 = 4;
pub const INT_SERIAL: u8 = 8;
pub const INT_JOYPAD: u8 = 16;

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
    joyp_select: u8,
    /// Pressed buttons, bit set = pressed: 0 A, 1 B, 2 Select, 3 Start, 4 Right, 5 Left, 6 Up, 7 Down.
    pub buttons: u8,
    sb: u8,
    sc: u8,
    /// What the game sent out of the serial port (the test ROMs report through it).
    #[serde(skip)]
    pub serial_out: Vec<u8>,
    dma_src: u16,
    dma_pos: u16,
    dma_active: bool,
    dma_delay: u8,
    dma_reg: u8,
    apu_div_bit: bool,
    pub cycles: u64,
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
            wram: vec![0; 0x2000],
            hram: vec![0; 0x7F],
            ie: 0,
            iflag: 0xE1,
            joyp_select: 0x30,
            buttons: 0,
            sb: 0,
            sc: 0x7E,
            serial_out: Vec::new(),
            dma_src: 0,
            dma_pos: 0,
            dma_active: false,
            dma_delay: 0,
            dma_reg: 0xFF,
            apu_div_bit: false,
            cycles: 0,
        }
    }

    pub fn set_buttons(&mut self, pressed: u8) {
        let newly = pressed & !self.buttons;
        self.buttons = pressed;
        if newly != 0 {
            self.iflag |= INT_JOYPAD;
        }
    }

    fn joyp(&self) -> u8 {
        let mut low = 0x0F;
        if self.joyp_select & 0x20 == 0 {
            low &= !(self.buttons & 0x0F);
        }
        if self.joyp_select & 0x10 == 0 {
            low &= !(self.buttons >> 4);
        }
        0xC0 | self.joyp_select | low
    }

    /// One M-cycle of everything but the CPU.
    pub fn tick(&mut self) {
        self.cycles += 4;
        if self.timer.tick_m() {
            self.iflag |= INT_TIMER;
        }
        let bit = self.timer.counter & 0x1000 != 0;
        if self.apu_div_bit && !bit {
            self.apu.frame_step();
        }
        self.apu_div_bit = bit;
        self.apu.tick_m();
        for _ in 0..4 {
            let irq = self.ppu.tick();
            if irq.vblank {
                self.iflag |= INT_VBLANK;
            }
            if irq.stat {
                self.iflag |= INT_STAT;
            }
        }
        self.cart.tick_m();
        if self.dma_active {
            if self.dma_delay > 0 {
                self.dma_delay -= 1;
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

    pub fn read(&mut self, addr: u16) -> u8 {
        self.tick();
        if self.dma_active && self.dma_delay == 0 && (0xFE00..0xFF00).contains(&addr) {
            return 0xFF;
        }
        self.raw_read(addr)
    }

    pub fn write(&mut self, addr: u16, v: u8) {
        self.tick();
        if self.dma_active && (0xFE00..0xFEA0).contains(&addr) {
            return;
        }
        self.raw_write(addr, v);
    }

    pub fn raw_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cart.read_rom(addr),
            0x8000..=0x9FFF => self.ppu.vram[(addr - 0x8000) as usize],
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xFDFF => self.wram[(addr as usize - 0xC000) & 0x1FFF],
            0xFE00..=0xFE9F => self.ppu.oam[(addr - 0xFE00) as usize],
            0xFEA0..=0xFEFF => 0x00,
            0xFF00 => self.joyp(),
            0xFF01 => self.sb,
            0xFF02 => self.sc | 0x7E,
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
            0xFF44 => self.ppu.ly,
            0xFF45 => self.ppu.lyc,
            0xFF46 => self.dma_reg,
            0xFF47 => self.ppu.bgp,
            0xFF48 => self.ppu.obp0,
            0xFF49 => self.ppu.obp1,
            0xFF4A => self.ppu.wy,
            0xFF4B => self.ppu.wx,
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize],
            0xFFFF => self.ie,
            _ => 0xFF,
        }
    }

    pub fn raw_write(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x7FFF => self.cart.write_rom(addr, v),
            0x8000..=0x9FFF => self.ppu.vram[(addr - 0x8000) as usize] = v,
            0xA000..=0xBFFF => self.cart.write_ram(addr, v),
            0xC000..=0xFDFF => self.wram[(addr as usize - 0xC000) & 0x1FFF] = v,
            0xFE00..=0xFE9F => self.ppu.oam[(addr - 0xFE00) as usize] = v,
            0xFF00 => self.joyp_select = v & 0x30,
            0xFF01 => self.sb = v,
            0xFF02 => {
                self.sc = v;
                // No link partner: an internally clocked transfer completes at once, shifting in
                // 1s, which is what a disconnected cable reads as.
                if v & 0x81 == 0x81 {
                    self.serial_out.push(self.sb);
                    self.sb = 0xFF;
                    self.sc &= 0x7F;
                    self.iflag |= INT_SERIAL;
                }
            }
            0xFF04 => self.timer.write_div(),
            0xFF05 => self.timer.write_tima(v),
            0xFF06 => self.timer.write_tma(v),
            0xFF07 => self.timer.write_tac(v),
            0xFF0F => self.iflag = v & 0x1F,
            0xFF10..=0xFF3F => self.apu.write(addr, v),
            0xFF40 => self.ppu.write_lcdc(v),
            0xFF41 => self.ppu.write_stat(v),
            0xFF42 => self.ppu.scy = v,
            0xFF43 => self.ppu.scx = v,
            0xFF45 => self.ppu.lyc = v,
            0xFF46 => {
                self.dma_reg = v;
                self.dma_src = u16::from(v) << 8;
                if self.dma_src >= 0xE000 {
                    self.dma_src -= 0x2000;
                }
                self.dma_pos = 0;
                self.dma_active = true;
                self.dma_delay = 2;
            }
            0xFF47 => self.ppu.bgp = v,
            0xFF48 => self.ppu.obp0 = v,
            0xFF49 => self.ppu.obp1 = v,
            0xFF4A => self.ppu.wy = v,
            0xFF4B => self.ppu.wx = v,
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize] = v,
            0xFFFF => self.ie = v,
            _ => {}
        }
    }
}
