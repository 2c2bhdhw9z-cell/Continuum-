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

/// How much of the serial port's output is kept. Only the test ROMs read it, and their whole
/// report is a few hundred bytes; a game that keeps sending with no link partner finishes every
/// transfer at once and used to pile up hundreds of thousands of bytes a minute, which on a phone
/// ends with the app being shut down for using too much memory. Past this the bytes are dropped,
/// oldest kept, so a test's report is never truncated at the front.
pub const SERIAL_LOG_MAX: usize = 8192;

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
    /// The Super Game Boy, when running as one (core option).
    #[serde(default)]
    pub sgb: Option<Box<crate::sgb::Sgb>>,
    /// Boot ROM image and whether it is still mapped over $0000 (core option).
    #[serde(default)]
    pub boot: Vec<u8>,
    #[serde(default)]
    pub boot_on: bool,
    /// Link cable loopback (core option): a transfer reads back the byte sent.
    #[serde(skip)]
    pub link_loopback: bool,
    /// Bytes the other machine has sent, and bytes this machine has sent for it.
    #[serde(skip)]
    pub link_in: std::collections::VecDeque<u8>,
    #[serde(skip)]
    pub link_out: Vec<u8>,
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
            link_loopback: false,
            link_in: std::collections::VecDeque::new(),
            link_out: Vec::new(),
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
            last_mode: 0,
            trace_pc: 0,
            trace_bank: 0,
            hdma_watch: [0, 0, 0, 0x80, 0xFF],
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

    /// One M-cycle of everything but the CPU.
    pub fn tick(&mut self) {
        self.cycles += 4;
        if self.timer.tick_m() {
            self.iflag |= INT_TIMER;
        }
        // In double speed the CPU, timer and DMA run twice as fast; the PPU, sound and clock do not.
        let dots = if self.double_speed { 2 } else { 4 };
        let mask = if self.double_speed { 0x2000 } else { 0x1000 };
        let bit = self.timer.counter & mask != 0;
        if self.apu_div_bit && !bit {
            self.apu.frame_step();
        }
        self.apu_div_bit = bit;
        self.apu.tick(dots);
        for _ in 0..dots {
            let irq = self.ppu.tick();
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
            0x0000..=0x7FFF => self.cart.read_rom(addr),
            0x8000..=0x9FFF => self.ppu.vram[self.vram_index(addr)],
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xFDFF => self.wram[self.wram_index(addr)],
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
            0xFF69 if self.cgb => self.ppu.read_pal(false),
            0xFF6A if self.cgb_hardware => self.ppu.ocps | 0x40,
            0xFF6B if self.cgb => self.ppu.read_pal(true),
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
                // No link partner: an internally clocked transfer completes at once, shifting in
                // 1s, which is what a disconnected cable reads as.
                if v & 0x81 == 0x81 {
                    if self.serial_out.len() < SERIAL_LOG_MAX {
                        self.serial_out.push(self.sb);
                    }
                    if let Some(b) = self.link_in.pop_front() {
                        self.link_out.push(self.sb);
                        self.sb = b;
                    } else if !self.link_loopback {
                        self.sb = 0xFF;
                    }
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
            0xFF4B => {
                self.ppu.note_wx(v);
                self.ppu.wx = v;
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
            0xFF68 if self.cgb => self.ppu.bcps = v & 0xBF,
            0xFF69 if self.cgb => self.ppu.write_pal(false, v),
            0xFF6A if self.cgb => self.ppu.ocps = v & 0xBF,
            0xFF6B if self.cgb => self.ppu.write_pal(true, v),
            0xFF6C if self.cgb => self.ppu.opri = v & 1,
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
