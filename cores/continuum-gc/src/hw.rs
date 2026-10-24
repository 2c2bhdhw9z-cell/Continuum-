//! The GameCube's "Flipper" chip as the CPU sees it: main memory and the hardware registers at
//! 0x0C000000, written from public hardware documentation (YAGCD and the homebrew SDK headers).
//!
//! Devices here: PI (interrupts), VI (video output), SI (controllers), EXI (memory cards, clock,
//! SRAM), DI (disc drive), AI and DSP (sound, with the DSP answered at a high level), and the
//! graphics FIFO, which is only collected until the graphics engine (step G6) exists.
//!
//! Interrupt model: each device keeps its own status; PI's cause register is worked out from
//! them on every read, and the CPU's external line is updated after every access and event.

use crate::bus::Bus;
use crate::disc::Disc;
use serde::{Deserialize, Serialize};

pub const MEM1_SIZE: usize = 24 * 1024 * 1024;
pub const MEM2_SIZE: usize = 64 * 1024 * 1024;
pub const ARAM_SIZE: usize = 16 * 1024 * 1024;

/// Gekko clock (Broadway runs at 729 MHz).
pub const GC_CPU_HZ: u64 = 486_000_000;
pub const WII_CPU_HZ: u64 = 729_000_000;

// PI interrupt causes.
/// "GP runtime error": the CPU touched a physical address nothing answers (a bus error).
/// Programs use it to tell a GameCube from a Wii (Swiss reads 0x10000000, where a Wii has MEM2).
pub const INT_ERROR: u32 = 1 << 0;
pub const INT_RSW: u32 = 1 << 1;
pub const INT_DI: u32 = 1 << 2;
pub const INT_SI: u32 = 1 << 3;
pub const INT_EXI: u32 = 1 << 4;
pub const INT_AI: u32 = 1 << 5;
pub const INT_DSP: u32 = 1 << 6;
pub const INT_VI: u32 = 1 << 8;
pub const INT_PE_TOKEN: u32 = 1 << 9;
pub const INT_PE_FINISH: u32 = 1 << 10;
pub const INT_CP: u32 = 1 << 11;
pub const INT_HSP: u32 = 1 << 13;
/// The reset button's level (bit 16 of INTSR reads 1 while the button is up).
const RSWST: u32 = 1 << 16;

/// The controller buttons as SI reports them (the high half of the first reply word).
pub mod pad {
    pub const LEFT: u16 = 0x0001;
    pub const RIGHT: u16 = 0x0002;
    pub const DOWN: u16 = 0x0004;
    pub const UP: u16 = 0x0008;
    pub const Z: u16 = 0x0010;
    pub const R: u16 = 0x0020;
    pub const L: u16 = 0x0040;
    pub const A: u16 = 0x0100;
    pub const B: u16 = 0x0200;
    pub const X: u16 = 0x0400;
    pub const Y: u16 = 0x0800;
    pub const START: u16 = 0x1000;
}

/// One controller's state, set by the frontend each frame.
#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct PadState {
    pub connected: bool,
    pub buttons: u16,
    pub stick: (u8, u8),
    pub cstick: (u8, u8),
    pub triggers: (u8, u8),
    pub rumble: bool,
}

impl Default for PadState {
    fn default() -> Self {
        PadState { connected: false, buttons: 0, stick: (128, 128), cstick: (128, 128), triggers: (0, 0), rumble: false }
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Pi {
    pub mask: u32,
    /// Causes PI itself latches (the reset switch).
    pub latched: u32,
    pub fifo_base: u32,
    pub fifo_end: u32,
    pub fifo_write: u32,
    pub reset_code: u32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Vi {
    pub regs: Vec<u16>,
    /// Current line in the frame, 1-based like the DPV register.
    pub line: u32,
    /// Set when a field finished; the frontend takes the picture then.
    pub field_done: bool,
    pub odd_field: bool,
}

impl Default for Vi {
    fn default() -> Self {
        Vi { regs: vec![0; 0x80], line: 1, field_done: false, odd_field: false }
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Si {
    pub out: [u32; 4],
    pub poll: u32,
    pub comcsr: u32,
    pub sr: u32,
    pub exilk: u32,
    pub buf: Vec<u8>,
    pub pads: [PadState; 4],
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct ExiChannel {
    pub csr: u32,
    pub mar: u32,
    pub len: u32,
    pub cr: u32,
    pub data: u32,
    /// The first word of a transfer to the clock/SRAM/ROM device is its address.
    pub dev_addr: Option<u32>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Exi {
    pub ch: [ExiChannel; 3],
    pub sram: Vec<u8>,
    /// Seconds since 1 January 2000 (the console's clock), set from the host at boot.
    pub rtc: u32,
}

impl Default for Exi {
    fn default() -> Self {
        Exi { ch: Default::default(), sram: default_sram(), rtc: 0 }
    }
}

/// The 64-byte battery-backed SRAM with valid checksums: English, stereo, no offsets.
pub fn default_sram() -> Vec<u8> {
    let mut s = vec![0u8; 64];
    // 0x0C: counter bias; 0x10: display offset; 0x12: flags (bit 2 stereo); 0x13: language.
    s[0x12] = 0x2C;
    s[0x13] = 0;
    // Checksums over 0x0C..0x14 (big-endian halfwords): sum and inverted sum.
    let mut sum = 0u16;
    let mut inv = 0u16;
    for i in (0x0C..0x14).step_by(2) {
        let w = u16::from_be_bytes([s[i], s[i + 1]]);
        sum = sum.wrapping_add(w);
        inv = inv.wrapping_add(!w);
    }
    s[0..2].copy_from_slice(&sum.to_be_bytes());
    s[2..4].copy_from_slice(&inv.to_be_bytes());
    s
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Di {
    pub sr: u32,
    pub cvr: u32,
    pub cmd: [u32; 3],
    pub mar: u32,
    pub len: u32,
    pub cr: u32,
    pub imm: u32,
    pub cfg: u32,
    /// Cycles until the current command completes (0 = idle).
    pub busy: u64,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Dsp {
    pub cpu_mbox: u32,
    pub dsp_mbox: u32,
    pub dsp_mbox_full: bool,
    pub cpu_mbox_full: bool,
    pub csr: u16,
    pub ar_size: u16,
    pub ar_mode: u16,
    pub ar_refresh: u16,
    pub ar_mm: u32,
    pub ar_addr: u32,
    pub ar_cnt: u32,
    pub dma_addr: u32,
    pub dma_ctl: u16,
    pub dma_left: u16,
    /// The audio DMA's progress through the buffer it is playing.
    pub dma_cur: u32,
    pub dma_sub: u8,
    /// Mails the CPU sent to the boot ROM's upload protocol so far (pairs of command, value).
    pub rom_mails: Vec<u32>,
    /// A sound program the CPU uploaded: (main-memory address, length), once started.
    pub ucode: Option<(u32, u32)>,
    /// Mails our DSP stand-in has queued for the CPU, oldest first.
    pub outbox: std::collections::VecDeque<u32>,
    /// Our stand-in for the sound program that is running (dsp_hle.rs).
    pub hle: crate::dsp_hle::HleState,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Ai {
    pub cr: u32,
    pub vr: u32,
    pub scnt: u32,
    pub it: u32,
}

pub struct Hw {
    pub mem1: Vec<u8>,
    pub mem2: Vec<u8>,
    pub aram: Vec<u8>,
    pub wii: bool,
    pub pi: Pi,
    pub vi: Vi,
    pub si: Si,
    pub exi: Exi,
    pub di: Di,
    pub dsp: Dsp,
    pub ai: Ai,
    /// The graphics engine (command processor, transform, rasteriser, pixel engine).
    pub gx: crate::gx::Gx,
    /// Bytes in the CPU's write-gather pipe, sent on to the FIFO 32 at a time.
    pub gather: Vec<u8>,
    pub disc: Option<Box<dyn Disc>>,
    /// Code addresses invalidated (icbi, HID0 ICFI): read by the block caches of the faster CPUs.
    pub invalidated: Vec<(u32, u32)>,
    /// Lines the program printed through the debug UART (EXI channel 0, device 1).
    pub uart: Vec<u8>,
    /// Register addresses nothing answers yet, with how often they were touched (development aid).
    pub unknown: std::collections::BTreeMap<u32, u32>,
    /// Disc drive commands seen, by command byte (development aid).
    pub di_commands: std::collections::BTreeMap<u8, u32>,
    /// The first disc drive register writes (offset, value), for development.
    pub di_trace: Vec<(u32, u32)>,
    /// Sound for the frontend: interleaved left/right 16-bit samples at 48 kHz.
    pub audio_out: Vec<i16>,
    /// Audio clock: DMA samples owed, in hundredths of a video line.
    audio_phase: u64,
    /// Resampling 32 kHz to 48 kHz: the previous input frame and the position between frames.
    audio_prev: (i16, i16),
    audio_t: f32,
    /// An SD card adapter and the EXI channel it sits on (2: serial port 2, like SD2SP2).
    pub sd: Option<crate::sd::SdCard>,
    pub sd_channel: usize,
}

impl Hw {
    pub fn new(wii: bool) -> Self {
        Hw {
            mem1: vec![0; MEM1_SIZE],
            mem2: if wii { vec![0; MEM2_SIZE] } else { Vec::new() },
            aram: vec![0; ARAM_SIZE],
            wii,
            pi: Pi::default(),
            vi: Vi::default(),
            si: Si { buf: vec![0; 128], ..Default::default() },
            exi: Exi::default(),
            di: Di::default(),
            dsp: Dsp { csr: 0, ..Default::default() },
            ai: Ai::default(),
            gx: crate::gx::Gx::new(),
            gather: Vec::new(),
            disc: None,
            invalidated: Vec::new(),
            uart: Vec::new(),
            unknown: Default::default(),
            di_commands: Default::default(),
            di_trace: Vec::new(),
            sd: None,
            sd_channel: 2,
            audio_out: Vec::with_capacity(4096),
            audio_phase: 0,
            audio_prev: (0, 0),
            audio_t: 0.0,
        }
    }

    // ------------------------------------------------------------------ interrupts

    /// PI's cause register, from every device.
    pub fn cause(&self) -> u32 {
        let mut c = self.pi.latched | RSWST;
        if self.vi_irq() {
            c |= INT_VI;
        }
        if self.si.comcsr & 0xC000_0000 == 0xC000_0000 {
            c |= INT_SI;
        }
        if self.exi.ch.iter().any(|ch| {
            let csr = ch.csr;
            (csr & 0x2 != 0 && csr & 0x1 != 0) || (csr & 0x8 != 0 && csr & 0x4 != 0) || (csr & 0x800 != 0 && csr & 0x400 != 0)
        }) {
            c |= INT_EXI;
        }
        // DISR pairs (interrupt, mask): DEINT 2 / 1, TCINT 4 / 3, BRKINT 6 / 5; DICVR CVRINT 2 / 1.
        let sr = self.di.sr;
        if sr & 0x06 == 0x06 || sr & 0x18 == 0x18 || sr & 0x60 == 0x60 || self.di.cvr & 0x6 == 0x6 {
            c |= INT_DI;
        }
        let csr = self.dsp.csr;
        // DSPCR: (AIDINT, AIDINTMSK) = bits 3,4; (ARINT, ARINTMSK) = 5,6; (DSPINT, DSPINTMSK) = 7,8.
        if (csr & 0x18 == 0x18) || (csr & 0x60 == 0x60) || (csr & 0x180 == 0x180) {
            c |= INT_DSP;
        }
        // AI: AIINT (bit 3) with AIINTMSK (bit 2).
        if self.ai.cr & 0xC == 0xC {
            c |= INT_AI;
        }
        if self.gx.irq_cp() {
            c |= INT_CP;
        }
        if self.gx.irq_token() {
            c |= INT_PE_TOKEN;
        }
        if self.gx.irq_finish() {
            c |= INT_PE_FINISH;
        }
        c
    }

    /// Whether the CPU's external interrupt line is up.
    pub fn irq(&self) -> bool {
        self.cause() & self.pi.mask & 0x3FFF != 0
    }

    // ------------------------------------------------------------------ memory

    fn ram(&mut self, pa: u32, n: u32) -> Option<&mut [u8]> {
        let a = pa as usize;
        let n = n as usize;
        if a + n <= MEM1_SIZE {
            return Some(&mut self.mem1[a..a + n]);
        }
        if self.wii && (0x1000_0000..0x1000_0000 + MEM2_SIZE).contains(&a) && a + n <= 0x1000_0000 + MEM2_SIZE {
            let o = a - 0x1000_0000;
            return Some(&mut self.mem2[o..o + n]);
        }
        None
    }

    /// A DMA's view of main memory (DI, ARAM, AI and SI transfers all go through this).
    pub fn dma_write(&mut self, pa: u32, data: &[u8]) {
        if let Some(r) = self.ram(pa & 0x1FFF_FFFF, data.len() as u32) {
            r.copy_from_slice(data);
        }
    }

    pub fn dma_read(&mut self, pa: u32, len: usize) -> Vec<u8> {
        self.ram(pa & 0x1FFF_FFFF, len as u32).map(|r| r.to_vec()).unwrap_or_else(|| vec![0; len])
    }

    // ------------------------------------------------------------------ registers

    fn mmio_read(&mut self, pa: u32, n: u32) -> u32 {
        let off = pa & 0xFFFF;
        if (0x0800_0000..0x0C00_0000).contains(&pa) {
            return self.gx.efb_peek(pa);
        }
        match pa & 0xFFFF_F000 {
            // CP and PE: 16-bit registers; a 32-bit read joins two.
            0x0C00_0000 => {
                let o = off & 0xFFF;
                if n == 4 {
                    ((self.gx.cp_read(o) as u32) << 16) | self.gx.cp_read(o + 2) as u32
                } else {
                    self.gx.cp_read(o) as u32
                }
            }
            0x0C00_1000 => {
                let o = off & 0xFFF;
                if n == 4 {
                    ((self.gx.pe_read(o) as u32) << 16) | self.gx.pe_read(o + 2) as u32
                } else {
                    self.gx.pe_read(o) as u32
                }
            }
            0x0C00_2000 => self.vi_read(off & 0xFFF, n),
            0x0C00_3000 => match off & 0xFFF {
                0x00 => self.cause(),
                0x04 => self.pi.mask,
                0x0C => self.pi.fifo_base,
                0x10 => self.pi.fifo_end,
                0x14 => self.pi.fifo_write,
                0x24 => self.pi.reset_code,
                // Flipper revision C.
                0x2C => 0x2465_00B1,
                _ => 0,
            },
            0x0C00_4000 => 0,
            0x0C00_5000 => self.dsp_read(off & 0xFFF, n),
            0x0C00_6000 => match off & 0xFFF {
                0x000..=0x3FF => self.di_read(off & 0x3FF),
                0x400..=0x7FF => self.si_read(off & 0x3FF),
                0x800..=0xBFF => self.exi_read(off & 0x3FF),
                _ => self.ai_read(off & 0x3FF),
            },
            _ => {
                *self.unknown.entry(pa).or_default() += 1;
                self.pi.latched |= INT_ERROR;
                0
            }
        }
    }

    fn mmio_write(&mut self, pa: u32, n: u32, v: u32) {
        let off = pa & 0xFFFF;
        if (0x0800_0000..0x0C00_0000).contains(&pa) {
            self.gx.efb_poke(pa, v);
            return;
        }
        match pa & 0xFFFF_F000 {
            0x0C00_0000 => {
                let o = off & 0xFFF;
                if n == 4 {
                    self.cp_write(o, (v >> 16) as u16);
                    self.cp_write(o + 2, v as u16);
                } else if n == 2 {
                    self.cp_write(o, v as u16);
                }
            }
            0x0C00_1000 => {
                let o = off & 0xFFF;
                if n == 4 {
                    self.gx.pe_write(o, (v >> 16) as u16);
                    self.gx.pe_write(o + 2, v as u16);
                } else if n == 2 {
                    self.gx.pe_write(o, v as u16);
                }
            }
            0x0C00_4000 => {}
            0x0C00_2000 => self.vi_write(off & 0xFFF, n, v),
            0x0C00_3000 => match off & 0xFFF {
                // Writing 1 to the reset-switch or bus-error cause clears it.
                0x00 => self.pi.latched &= !(v & (INT_RSW | INT_ERROR)),
                0x04 => self.pi.mask = v,
                0x0C => self.pi.fifo_base = v,
                0x10 => self.pi.fifo_end = v,
                0x14 => self.pi.fifo_write = v,
                0x24 => self.pi.reset_code = v,
                _ => {}
            },
            0x0C00_5000 => self.dsp_write(off & 0xFFF, n, v),
            0x0C00_6000 => match off & 0xFFF {
                0x000..=0x3FF => self.di_write(off & 0x3FF, v),
                0x400..=0x7FF => self.si_write(off & 0x3FF, n, v),
                0x800..=0xBFF => self.exi_write(off & 0x3FF, v),
                _ => self.ai_write(off & 0x3FF, v),
            },
            // The write-gather pipe to the graphics FIFO.
            0x0C00_8000 => {
                let bytes = v.to_be_bytes();
                self.gather.extend_from_slice(&bytes[(4 - n as usize)..]);
                while self.gather.len() >= 32 {
                    let mut block = [0u8; 32];
                    block.copy_from_slice(&self.gather[..32]);
                    self.gather.drain(..32);
                    self.pi_fifo_burst(&block);
                }
            }
            _ => {
                *self.unknown.entry(pa | 0x8000_0000).or_default() += 1;
                self.pi.latched |= INT_ERROR;
            }
        }
    }

    // ------------------------------------------------------------------ graphics FIFO

    fn cp_write(&mut self, off: u32, v: u16) {
        let Hw { gx, mem1, mem2, .. } = self;
        let mut ram = crate::gx::Ram { mem1, mem2 };
        gx.cp_write(&mut ram, off, v);
    }

    /// PI writes one 32-byte burst into the CPU FIFO in memory and steps its write pointer,
    /// wrapping at the FIFO's end; the command processor follows when the FIFO is linked.
    fn pi_fifo_burst(&mut self, block: &[u8; 32]) {
        let (wrap_bit, mask) = if self.wii { (0x2000_0000, 0x1FFF_FFE0) } else { (0x0400_0000, 0x03FF_FFE0) };
        let wp = self.pi.fifo_write & mask;
        self.dma_write(wp, block);
        let mut next = wp + 32;
        let mut wrapped = self.pi.fifo_write & wrap_bit;
        if next >= (self.pi.fifo_end & 0x1FFF_FFFC) && self.pi.fifo_end > self.pi.fifo_base {
            next = self.pi.fifo_base & mask;
            wrapped = wrap_bit;
        }
        self.pi.fifo_write = next | wrapped;
        let Hw { gx, mem1, mem2, .. } = self;
        let mut ram = crate::gx::Ram { mem1, mem2 };
        gx.pi_burst(&mut ram, next);
    }

    // ------------------------------------------------------------------ VI

    /// Reads VI registers by halfword; 32-bit reads join two.
    fn vi_read(&mut self, off: u32, n: u32) -> u32 {
        let half = |vi: &Vi, o: u32| -> u32 {
            if o == 0x2C {
                return vi.line;
            }
            vi.regs.get((o / 2) as usize).copied().unwrap_or(0) as u32
        };
        if n == 4 {
            (half(&self.vi, off) << 16) | half(&self.vi, off + 2)
        } else {
            half(&self.vi, off)
        }
    }

    fn vi_write(&mut self, off: u32, n: u32, v: u32) {
        let mut put = |o: u32, h: u16| {
            if let Some(r) = self.vi.regs.get_mut((o / 2) as usize) {
                // Display interrupts (DI0-DI3, 0x30-0x3E): the status bit (31) is only ever cleared by writes.
                if (0x30..0x40).contains(&o) && o.is_multiple_of(4) {
                    *r = (h & 0x7FFF) | (*r & h & 0x8000);
                } else {
                    *r = h;
                }
            }
        };
        if n == 4 {
            put(off, (v >> 16) as u16);
            put(off + 2, v as u16);
        } else {
            put(off, v as u16);
        }
    }

    fn vi_reg32(&self, off: u32) -> u32 {
        ((self.vi.regs[(off / 2) as usize] as u32) << 16) | self.vi.regs[(off / 2 + 1) as usize] as u32
    }

    fn vi_irq(&self) -> bool {
        (0..4).any(|k| {
            let r = self.vi_reg32(0x30 + 4 * k);
            r & 0x8000_0000 != 0 && r & 0x1000_0000 != 0
        })
    }

    /// Lines in a frame (two fields): 525 for NTSC/MPAL, 625 for PAL (DCR bits 8-9 = 1).
    pub fn vi_lines_per_frame(&self) -> u32 {
        if (self.vi.regs[1] >> 8) & 3 == 1 {
            625
        } else {
            525
        }
    }

    /// One video line passed. Raises display interrupts and marks the end of each field.
    pub fn vi_line(&mut self) {
        let total = self.vi_lines_per_frame();
        self.vi.line += 1;
        if self.vi.line > total {
            self.vi.line = 1;
        }
        let line = self.vi.line;
        for k in 0..4u32 {
            let r = self.vi_reg32(0x30 + 4 * k);
            let vct = (r >> 16) & 0x3FF;
            if r & 0x1000_0000 != 0 && vct == line {
                self.vi.regs[(0x30 / 2 + 2 * k) as usize] |= 0x8000;
            }
        }
        if line == 1 || line == total / 2 + 1 {
            self.vi.field_done = true;
            self.vi.odd_field = line != 1;
        }
    }

    /// The picture VI is showing, converted from the external framebuffer's YUV to 0RGB.
    /// Returns (width, height, pixels); nothing until a program has turned the display on.
    pub fn vi_picture(&mut self) -> Option<(u32, u32, Vec<u32>)> {
        let dcr = self.vi.regs[1];
        if dcr & 1 == 0 {
            return None;
        }
        let progressive = dcr & 4 != 0;
        // ACV: active lines per field (VTR bits 4-13).
        let acv = ((self.vi.regs[0] >> 4) & 0x3FF) as u32;
        // HSW (0x48): WPL (bits 8-14) and STD (bits 0-7), both in 16-pixel (32-byte) units.
        let hsw = self.vi.regs[0x48 / 2];
        let width = ((hsw >> 8) & 0x7F) as u32 * 16;
        let stride = (hsw & 0xFF) as u32 * 32;
        if width == 0 || acv == 0 {
            return None;
        }
        let fb_addr = |reg: u32| {
            // Bit 28 (POFF): the address is in 32-byte units.
            if reg & 0x1000_0000 != 0 {
                (reg & 0x00FF_FFFF) << 5
            } else {
                reg & 0x00FF_FFFF
            }
        };
        let top = fb_addr(self.vi_reg32(0x1C));
        let bottom = fb_addr(self.vi_reg32(0x24));
        let height = if progressive { acv } else { acv * 2 };
        let mut out = vec![0u32; (width * height) as usize];
        for y in 0..height {
            let (base, row) = if progressive { (top, y) } else if y % 2 == 0 { (top, y / 2) } else { (bottom, y / 2) };
            let line = self.dma_read(base + row * stride, (width * 2) as usize);
            for xp in (0..width as usize).step_by(2) {
                let (y0, u, y1, v) = (line[xp * 2], line[xp * 2 + 1], line[xp * 2 + 2], line[xp * 2 + 3]);
                out[(y * width) as usize + xp] = yuv_to_rgb(y0, u, v);
                out[(y * width) as usize + xp + 1] = yuv_to_rgb(y1, u, v);
            }
        }
        Some((width, height, out))
    }

    // ------------------------------------------------------------------ SI (controllers)

    fn si_read(&mut self, off: u32) -> u32 {
        match off {
            0x00..=0x2F => {
                let ch = (off / 12) as usize;
                match off % 12 {
                    0 => self.si.out[ch],
                    4 => self.pad_word(ch, true),
                    _ => self.pad_word(ch, false),
                }
            }
            0x30 => self.si.poll,
            0x34 => self.si.comcsr,
            0x38 => self.si.sr,
            0x3C => self.si.exilk,
            0x80..=0xFF => {
                let i = (off - 0x80) as usize;
                u32::from_be_bytes(self.si.buf[i..i + 4].try_into().unwrap())
            }
            _ => 0,
        }
    }

    /// A standard controller's reply to polling (mode 3): buttons, stick, C-stick, triggers.
    fn pad_word(&self, ch: usize, high: bool) -> u32 {
        let p = &self.si.pads[ch];
        if !p.connected {
            // "No response" error bit for this channel.
            return if high { 0x8000_0000 } else { 0 };
        }
        if high {
            // Bit 7 of the low button byte is always set by a real controller.
            (((p.buttons | 0x0080) as u32) << 16) | ((p.stick.0 as u32) << 8) | p.stick.1 as u32
        } else {
            ((p.cstick.0 as u32) << 24) | ((p.cstick.1 as u32) << 16) | ((p.triggers.0 as u32) << 8) | p.triggers.1 as u32
        }
    }

    fn si_write(&mut self, off: u32, n: u32, v: u32) {
        match off {
            0x00..=0x2F => {
                let ch = (off / 12) as usize;
                if off.is_multiple_of(12) {
                    self.si.out[ch] = v;
                    // Rumble is bit 0 of the polling command's last byte.
                    self.si.pads[ch].rumble = v & 1 != 0;
                }
            }
            0x30 => self.si.poll = v,
            0x34 => {
                // TCINT (31) is cleared by writing 1; TCINTMSK (30) is stored.
                if v & 0x8000_0000 != 0 {
                    self.si.comcsr &= !0x8000_0000;
                }
                self.si.comcsr = (self.si.comcsr & 0x8000_0000) | (v & 0x7FFF_FFFE);
                if v & 1 != 0 {
                    self.si_transfer();
                }
            }
            0x38 => self.si.sr = v & 0x8080_8080,
            0x3C => self.si.exilk = v,
            0x80..=0xFF => {
                let i = (off - 0x80) as usize;
                if n == 4 {
                    self.si.buf[i..i + 4].copy_from_slice(&v.to_be_bytes());
                } else if n == 2 {
                    self.si.buf[i..i + 2].copy_from_slice(&(v as u16).to_be_bytes());
                } else {
                    self.si.buf[i] = v as u8;
                }
            }
            _ => {}
        }
    }

    /// A command through SI's I/O buffer (COMCSR: channel bits 1-2, in length 8-14, out 16-22).
    fn si_transfer(&mut self) {
        let ch = ((self.si.comcsr >> 1) & 3) as usize;
        let reply: Vec<u8> = if !self.si.pads[ch].connected {
            Vec::new()
        } else {
            match self.si.buf[0] {
                // Reset / identify: a standard controller.
                0x00 | 0xFF => vec![0x09, 0x00, 0x00],
                // Origin and calibrate: the resting position.
                0x41 | 0x42 => vec![0x00, 0x80, 128, 128, 128, 128, 0, 0, 0, 0],
                _ => vec![0; 8],
            }
        };
        if reply.is_empty() {
            // No response on this channel (SISR NOREP bit for the channel).
            self.si.sr |= 0x08 << (8 * (3 - ch));
        }
        for (k, b) in reply.iter().enumerate() {
            self.si.buf[k] = *b;
        }
        self.si.comcsr &= !1;
        self.si.comcsr |= 0x8000_0000;
    }

    // ------------------------------------------------------------------ EXI

    fn exi_read(&mut self, off: u32) -> u32 {
        let (c, r) = ((off / 0x14) as usize, off % 0x14);
        if c > 2 {
            return 0;
        }
        let ch = &self.exi.ch[c];
        match r {
            // EXT (bit 12) stays clear: no memory card is plugged in yet (step G9).
            0x00 => ch.csr,
            0x04 => ch.mar,
            0x08 => ch.len,
            0x0C => ch.cr,
            _ => ch.data,
        }
    }

    fn exi_write(&mut self, off: u32, v: u32) {
        let (c, r) = ((off / 0x14) as usize, off % 0x14);
        if c > 2 {
            return;
        }
        match r {
            0x00 => {
                let ch = &mut self.exi.ch[c];
                // Interrupt status bits (1, 3, 11) clear on writing 1; the rest is stored.
                let clear = v & 0x80A;
                let old_cs = ch.csr & 0x380;
                ch.csr = (ch.csr & 0x80A & !clear) | (v & !0x80A & 0x3FF5);
                if ch.csr & 0x380 != old_cs {
                    ch.dev_addr = None;
                }
            }
            0x04 => self.exi.ch[c].mar = v & 0x03FF_FFE0,
            0x08 => self.exi.ch[c].len = v & 0x03FF_FFE0,
            0x0C => {
                self.exi.ch[c].cr = v;
                if v & 1 != 0 {
                    self.exi_transfer(c);
                }
            }
            _ => self.exi.ch[c].data = v,
        }
    }

    /// One EXI transfer, completed at once. Channel 0, device 1 is the clock / SRAM / IPL ROM /
    /// UART chip; an SD card adapter answers on its channel's device 0 (and, like the real
    /// adapters, also while no chip select is up); every other slot is empty for now (memory
    /// cards come with step G9).
    fn exi_transfer(&mut self, c: usize) {
        let cr = self.exi.ch[c].cr;
        let dma = cr & 2 != 0;
        let rw = (cr >> 2) & 3; // 0 read, 1 write, 2 read-write
        let len = if dma { self.exi.ch[c].len } else { ((cr >> 4) & 3) + 1 };
        let cs = (self.exi.ch[c].csr >> 7) & 7;
        let sd_here = self.sd.is_some() && self.sd_channel == c && (cs == 0 || cs == 1);
        if sd_here {
            self.sd_transfer(c, dma, rw, len);
        } else if c == 0 && cs == 2 && !dma {
            // Device select bit 1 (value 2 in CS) is the clock/SRAM chip.
            let data = self.exi.ch[c].data;
            match self.exi.ch[c].dev_addr {
                None => self.exi.ch[c].dev_addr = Some(data),
                Some(addr) => {
                    let reply = self.rtc_chip(addr, rw, data, len);
                    self.exi.ch[c].data = reply;
                    // SRAM and the clock step on per byte; the UART is one port.
                    if addr & 0x7FFF_FF00 != 0x2001_0000 {
                        self.exi.ch[c].dev_addr = Some(addr.wrapping_add(len << 6));
                    }
                }
            }
        } else if c == 0 && cs == 2 && dma {
            if let Some(addr) = self.exi.ch[c].dev_addr {
                let n = len as usize;
                if rw == 0 {
                    let bytes: Vec<u8> = (0..n).map(|k| self.rtc_byte(addr, k)).collect();
                    let mar = self.exi.ch[c].mar;
                    self.dma_write(mar, &bytes);
                }
            }
        } else if !dma && rw != 1 {
            // Nothing answers: with no device selected the lines float high, an empty slot
            // reads as zero.
            self.exi.ch[c].data = if cs == 0 { 0xFFFF_FFFF } else { 0 };
        }
        self.exi.ch[c].cr &= !1;
        // TCINT.
        self.exi.ch[c].csr |= 0x8;
    }

    fn sd_transfer(&mut self, c: usize, dma: bool, rw: u32, len: u32) {
        let Some(sd) = self.sd.as_mut() else { return };
        if dma {
            let mar = self.exi.ch[c].mar;
            if rw == 0 {
                let bytes: Vec<u8> = (0..len).map(|_| sd.exchange(0xFF)).collect();
                self.dma_write(mar, &bytes);
            } else {
                let bytes = self.dma_read(mar, len as usize);
                let sd = self.sd.as_mut().unwrap();
                let back: Vec<u8> = bytes.iter().map(|b| sd.exchange(*b)).collect();
                if rw == 2 {
                    self.dma_write(mar, &back);
                }
            }
        } else {
            let out = self.exi.ch[c].data.to_be_bytes();
            let mut back = [0xFFu8; 4];
            for k in 0..len as usize {
                back[k] = sd.exchange(if rw == 0 { 0xFF } else { out[k] });
            }
            if rw != 1 {
                self.exi.ch[c].data = u32::from_be_bytes(back);
            }
        }
    }

    /// One byte of a DMA from the clock/SRAM chip: SRAM when the address is SRAM's.
    fn rtc_byte(&self, addr: u32, k: usize) -> u8 {
        let a = addr & 0x7FFF_FFFF;
        if (0x2000_0100..0x2000_0100 + (64 << 6)).contains(&a) {
            self.exi.sram.get(((a - 0x2000_0100) >> 6) as usize + k).copied().unwrap_or(0)
        } else {
            0
        }
    }

    /// The clock/SRAM chip for immediate transfers. Addresses: 0x20000000 the clock,
    /// 0x20000100 SRAM, 0x20010000 the UART; anything below 0x200000 shifted is the IPL ROM.
    fn rtc_chip(&mut self, addr: u32, rw: u32, data: u32, len: u32) -> u32 {
        let write = addr & 0x8000_0000 != 0;
        let a = addr & 0x7FFF_FF00;
        match a {
            0x2000_0000 => {
                if write {
                    self.exi.rtc = data;
                    0
                } else {
                    self.exi.rtc
                }
            }
            0x2001_0000 => {
                if write {
                    for b in data.to_be_bytes().iter().take(len as usize) {
                        self.uart.push(*b);
                    }
                }
                0
            }
            _ if (0x2000_0100..0x2000_0100 + (64 << 6)).contains(&(addr & 0x7FFF_FFFF)) => {
                let base = (((addr & 0x7FFF_FFFF) - 0x2000_0100) >> 6) as usize;
                if write || rw == 1 {
                    for (k, b) in data.to_be_bytes().iter().take(len as usize).enumerate() {
                        if let Some(s) = self.exi.sram.get_mut(base + k) {
                            *s = *b;
                        }
                    }
                    0
                } else {
                    let mut w = [0u8; 4];
                    for (k, b) in w.iter_mut().enumerate().take(len as usize) {
                        *b = self.exi.sram.get(base + k).copied().unwrap_or(0);
                    }
                    u32::from_be_bytes(w)
                }
            }
            // The IPL ROM (fonts and the boot program) is the player's own file; none is loaded.
            _ => 0,
        }
    }

    // ------------------------------------------------------------------ DI (disc drive)

    fn di_read(&mut self, off: u32) -> u32 {
        match off {
            0x00 => self.di.sr,
            0x04 => self.di.cvr,
            0x08 => self.di.cmd[0],
            0x0C => self.di.cmd[1],
            0x10 => self.di.cmd[2],
            0x14 => self.di.mar,
            0x18 => self.di.len,
            0x1C => self.di.cr,
            0x20 => self.di.imm,
            0x24 => self.di.cfg,
            _ => 0,
        }
    }

    fn di_write(&mut self, off: u32, v: u32) {
        if self.di_trace.len() < 400 {
            self.di_trace.push((off, v));
        }
        match off {
            0x00 => {
                // DEINT (2), TCINT (4), BRKINT (6) clear on 1; masks (1, 3, 5) and BRK (0) stored.
                let clear = v & 0x54;
                self.di.sr = (self.di.sr & 0x54 & !clear) | (v & 0x2B);
            }
            0x04 => {
                let clear = v & 0x4;
                self.di.cvr = (self.di.cvr & 0x5 & !clear) | (v & 0x2);
            }
            0x08 => self.di.cmd[0] = v,
            0x0C => self.di.cmd[1] = v,
            0x10 => self.di.cmd[2] = v,
            0x14 => self.di.mar = v & 0x03FF_FFE0,
            0x18 => self.di.len = v & !0x1F,
            0x1C => {
                self.di.cr = v & 7;
                if v & 1 != 0 {
                    self.di_command();
                }
            }
            0x20 => self.di.imm = v,
            _ => {}
        }
    }

    fn di_command(&mut self) {
        let cmd = self.di.cmd[0] >> 24;
        *self.di_commands.entry(cmd as u8).or_default() += 1;
        let mut error = false;
        match cmd {
            // Read: offset in 4-byte units, length in bytes.
            0xA8 => {
                let offset = (self.di.cmd[1] as u64) << 2;
                let len = self.di.len.min(self.di.cmd[2].max(self.di.len)) as usize;
                let mut buf = vec![0u8; len];
                match self.disc.as_mut() {
                    Some(d) => {
                        if d.read(offset, &mut buf).is_err() {
                            error = true;
                        }
                    }
                    None => error = true,
                }
                if !error {
                    let mar = self.di.mar;
                    self.dma_write(mar, &buf);
                    self.di.mar += len as u32;
                    self.di.len = 0;
                }
            }
            // Inquiry: drive date and revision.
            0x12 => {
                let mar = self.di.mar;
                let mut info = [0u8; 32];
                info[0..4].copy_from_slice(&0x0002_0000u32.to_be_bytes());
                info[4..8].copy_from_slice(&0x2001_0608u32.to_be_bytes());
                info[8..12].copy_from_slice(&0x6100_0000u32.to_be_bytes());
                self.dma_write(mar, &info);
                self.di.len = 0;
            }
            // Request error: none.
            0xE0 => self.di.imm = 0,
            // Seek, stop motor, audio streaming, audio buffer config: accept.
            0xAB | 0xE3 | 0xE1 | 0xE2 | 0xE4 => self.di.imm = 0,
            _ => error = true,
        }
        self.di.cr &= !1;
        self.di.sr |= if error { 0x04 } else { 0x10 };
    }

    // ------------------------------------------------------------------ DSP and ARAM

    fn dsp_read(&mut self, off: u32, n: u32) -> u32 {
        let half = |hw: &mut Hw, o: u32| -> u32 {
            match o {
                0x00 => hw.dsp.cpu_mbox >> 16 | if hw.dsp.cpu_mbox_full { 0x8000 } else { 0 },
                0x02 => hw.dsp.cpu_mbox & 0xFFFF,
                0x04 => (hw.dsp.dsp_mbox >> 16) & 0x7FFF | if hw.dsp.dsp_mbox_full { 0x8000 } else { 0 },
                0x06 => {
                    // Reading the low half takes the mail.
                    let v = hw.dsp.dsp_mbox & 0xFFFF;
                    hw.dsp.dsp_mbox_full = false;
                    hw.dsp_deliver();
                    v
                }
                0x0A => hw.dsp.csr as u32,
                0x12 => hw.dsp.ar_size as u32,
                0x16 => hw.dsp.ar_mode as u32 | 1, // ARAM ready
                0x1A => hw.dsp.ar_refresh as u32,
                0x20 => hw.dsp.ar_mm >> 16,
                0x22 => hw.dsp.ar_mm & 0xFFFF,
                0x24 => hw.dsp.ar_addr >> 16,
                0x26 => hw.dsp.ar_addr & 0xFFFF,
                0x28 => hw.dsp.ar_cnt >> 16,
                0x2A => hw.dsp.ar_cnt & 0xFFFF,
                0x30 => hw.dsp.dma_addr >> 16,
                0x32 => hw.dsp.dma_addr & 0xFFFF,
                0x36 => hw.dsp.dma_ctl as u32,
                0x3A => hw.dsp.dma_left as u32,
                _ => 0,
            }
        };
        if n == 4 {
            (half(self, off) << 16) | half(self, off + 2)
        } else {
            half(self, off)
        }
    }

    fn dsp_write(&mut self, off: u32, n: u32, v: u32) {
        if n == 4 {
            self.dsp_write16(off, (v >> 16) as u16);
            self.dsp_write16(off + 2, v as u16);
        } else {
            self.dsp_write16(off, v as u16);
        }
    }

    fn dsp_write16(&mut self, off: u32, v: u16) {
        let v32 = v as u32;
        match off {
            0x00 => self.dsp.cpu_mbox = (self.dsp.cpu_mbox & 0xFFFF) | ((v32 & 0x7FFF) << 16),
            0x02 => {
                self.dsp.cpu_mbox = (self.dsp.cpu_mbox & 0xFFFF_0000) | v32;
                self.dsp.cpu_mbox_full = true;
                self.dsp_cpu_mail();
            }
            0x0A => {
                // Interrupt bits (3, 5, 7) clear on 1; RES (0) resets; HALT (2) and masks are stored.
                let clear = v & 0xA8;
                let was_halted = self.dsp.csr & 4 != 0;
                let mut csr = (self.dsp.csr & 0xA8 & !clear) | (v & !0xA8 & 0x0B57);
                if v & 1 != 0 {
                    // Reset: the boot ROM starts again, says it is ready, and waits for an upload.
                    csr &= !1;
                    self.dsp.rom_mails.clear();
                    self.dsp.ucode = None;
                    self.dsp.outbox.clear();
                    self.dsp.dsp_mbox_full = false;
                    self.dsp_post(0x8071_FEED);
                }
                self.dsp.csr = csr;
                if was_halted && v & 4 == 0 && v & 0x800 == 0 && self.dsp.ucode.is_none() {
                    // Released from halt after the "init code" reset (DSPRESET cleared): the small
                    // program the OS put at 0x81000000 runs and sends one mail back.
                    self.dsp_post(0x8071_FEED);
                }
            }
            0x12 => self.dsp.ar_size = v,
            0x16 => self.dsp.ar_mode = v,
            0x1A => self.dsp.ar_refresh = v,
            0x20 => self.dsp.ar_mm = (self.dsp.ar_mm & 0xFFFF) | (v32 << 16),
            0x22 => self.dsp.ar_mm = (self.dsp.ar_mm & 0xFFFF_0000) | v32,
            0x24 => self.dsp.ar_addr = (self.dsp.ar_addr & 0xFFFF) | (v32 << 16),
            0x26 => self.dsp.ar_addr = (self.dsp.ar_addr & 0xFFFF_0000) | v32,
            0x28 => self.dsp.ar_cnt = (self.dsp.ar_cnt & 0xFFFF) | (v32 << 16),
            0x2A => {
                self.dsp.ar_cnt = (self.dsp.ar_cnt & 0xFFFF_0000) | v32;
                self.aram_dma();
            }
            0x30 => self.dsp.dma_addr = (self.dsp.dma_addr & 0xFFFF) | (v32 << 16),
            0x32 => self.dsp.dma_addr = (self.dsp.dma_addr & 0xFFFF_0000) | (v32 & !0x1F),
            0x36 => {
                let was_on = self.dsp.dma_ctl & 0x8000 != 0;
                self.dsp.dma_ctl = v;
                if v & 0x8000 != 0 && !was_on {
                    // Starting: the first buffer is loaded at once (and says so).
                    self.audio_dma_load();
                }
            }
            _ => {}
        }
    }

    /// Queues a mail from the DSP to the CPU (delivered when the mailbox is free).
    pub(crate) fn dsp_post(&mut self, mail: u32) {
        self.dsp.outbox.push_back(mail);
        self.dsp_deliver();
    }

    fn dsp_deliver(&mut self) {
        if !self.dsp.dsp_mbox_full {
            if let Some(m) = self.dsp.outbox.pop_front() {
                self.dsp.dsp_mbox = m & 0x7FFF_FFFF;
                self.dsp.dsp_mbox_full = true;
            }
        }
    }

    /// A mail from the CPU to the DSP. The boot ROM's upload protocol is answered here: pairs of
    /// 0x80F3A001 (main-memory address of the code), 0x80F3C002 (IRAM address), 0x80F3A002
    /// (length), 0x80F3B002 (DRAM address), 0x80F3D001 (start vector), after which the
    /// uploaded sound program runs. A started program says it is ready (0xDCD10000, the mail
    /// Nintendo's sound programs send first). Until our own stand-ins for the common sound
    /// programs exist (step G5, sound), every other mail is just taken.
    fn dsp_cpu_mail(&mut self) {
        let mail = self.dsp.cpu_mbox | 0x8000_0000;
        self.dsp.cpu_mbox_full = false;
        if self.dsp.ucode.is_none() {
            self.dsp.rom_mails.push(mail);
            let m = &self.dsp.rom_mails;
            if m.len().is_multiple_of(2) && m[m.len() - 2] == 0x80F3_D001 {
                let find = |cmd: u32| m.chunks(2).find(|p| p[0] == cmd).map(|p| p[1] & 0x7FFF_FFFF).unwrap_or(0);
                let (maddr, len) = (find(0x80F3_A001), find(0x80F3_A002));
                self.dsp.ucode = Some((maddr, len));
                self.dsp.rom_mails.clear();
                self.dsp_hle_start(maddr, len);
            }
        } else {
            self.dsp_hle_mail(mail);
        }
    }

    /// ARAM DMA: main memory and the 16 MB of audio RAM, done at once.
    fn aram_dma(&mut self) {
        let len = (self.dsp.ar_cnt & 0x03FF_FFE0) as usize;
        let to_main = self.dsp.ar_cnt & 0x8000_0000 != 0;
        let aram = (self.dsp.ar_addr & 0x00FF_FFE0) as usize;
        let mm = self.dsp.ar_mm & 0x03FF_FFE0;
        if to_main {
            let data: Vec<u8> = (0..len).map(|k| self.aram[(aram + k) % ARAM_SIZE]).collect();
            self.dma_write(mm, &data);
        } else {
            let data = self.dma_read(mm, len);
            for (k, b) in data.into_iter().enumerate() {
                self.aram[(aram + k) % ARAM_SIZE] = b;
            }
        }
        self.dsp.ar_cnt &= 0x8000_0000;
        // ARINT.
        self.dsp.csr |= 0x20;
    }

    // ------------------------------------------------------------------ AI

    fn ai_read(&mut self, off: u32) -> u32 {
        match off {
            0x00 => self.ai.cr,
            0x04 => self.ai.vr,
            0x08 => self.ai.scnt,
            0x0C => self.ai.it,
            _ => 0,
        }
    }

    fn ai_write(&mut self, off: u32, v: u32) {
        match off {
            0x00 => {
                // AIINT (3) clears on 1; SCRESET (5) zeroes the sample counter.
                let clear = v & 0x8;
                self.ai.cr = (self.ai.cr & 0x8 & !clear) | (v & 0x57);
                if v & 0x20 != 0 {
                    self.ai.scnt = 0;
                }
            }
            0x04 => self.ai.vr = v,
            0x08 => self.ai.scnt = v,
            0x0C => self.ai.it = v,
            _ => {}
        }
    }

    /// Streaming-audio samples passed (the sample counter runs while AI is playing).
    pub fn ai_samples(&mut self, n: u32) {
        if self.ai.cr & 1 == 0 {
            return;
        }
        let before = self.ai.scnt;
        self.ai.scnt = self.ai.scnt.wrapping_add(n);
        if self.ai.it != 0 && before < self.ai.it && self.ai.scnt >= self.ai.it {
            self.ai.cr |= 0x8;
        }
    }

    /// The audio DMA takes the buffer the registers describe and raises AIDINT, so the program
    /// can queue the next one while this one plays.
    fn audio_dma_load(&mut self) {
        self.dsp.dma_cur = ((self.dsp.dma_addr >> 16) << 16 | (self.dsp.dma_addr & 0xFFE0)) & 0x03FF_FFE0;
        self.dsp.dma_left = self.dsp.dma_ctl & 0x7FFF;
        self.dsp.dma_sub = 0;
        self.dsp.csr |= 0x08;
    }

    /// One frame from the audio DMA (right then left, as the hardware stores them), or silence.
    fn audio_dma_frame(&mut self) -> (i16, i16) {
        if self.dsp.dma_ctl & 0x8000 == 0 {
            return (0, 0);
        }
        if self.dsp.dma_left == 0 {
            self.audio_dma_load();
            if self.dsp.dma_left == 0 {
                return (0, 0);
            }
        }
        let a = self.dsp.dma_cur + self.dsp.dma_sub as u32 * 4;
        let w = self.read_ram32(a);
        let (r, l) = ((w >> 16) as i16, w as i16);
        self.dsp.dma_sub += 1;
        if self.dsp.dma_sub == 8 {
            self.dsp.dma_sub = 0;
            self.dsp.dma_cur += 32;
            self.dsp.dma_left -= 1;
        }
        (l, r)
    }

    fn read_ram32(&mut self, pa: u32) -> u32 {
        self.ram(pa, 4).map(|r| u32::from_be_bytes([r[0], r[1], r[2], r[3]])).unwrap_or(0)
    }

    /// One video line of sound: the DMA plays at 32 or 48 kHz; the output is always 48 kHz.
    pub fn audio_line(&mut self) {
        let lines_x100: u64 = if self.vi_lines_per_frame() == 625 { 1_562_500 } else { 1_573_426 };
        let rate: u64 = if self.ai.cr & 0x40 != 0 { 32_000 } else { 48_000 };
        self.audio_phase += rate * 100;
        while self.audio_phase >= lines_x100 {
            self.audio_phase -= lines_x100;
            let f = self.audio_dma_frame();
            if rate == 48_000 {
                self.audio_out.extend_from_slice(&[f.0, f.1]);
            } else {
                let step = 32_000.0 / 48_000.0;
                let p = self.audio_prev;
                while self.audio_t <= 1.0 {
                    let t = self.audio_t;
                    let mix = |a: i16, b: i16| (a as f32 + (b as f32 - a as f32) * t) as i16;
                    self.audio_out.extend_from_slice(&[mix(p.0, f.0), mix(p.1, f.1)]);
                    self.audio_t += step;
                }
                self.audio_t -= 1.0;
                self.audio_prev = f;
            }
            if self.audio_out.len() > 48_000 * 2 {
                // Nobody is taking the sound: keep only the newest second.
                self.audio_out.drain(..48_000);
            }
        }
    }
}

/// BT.601 limited-range YUV to 0RGB.
pub fn yuv_to_rgb(y: u8, u: u8, v: u8) -> u32 {
    let c = y as i32 - 16;
    let d = u as i32 - 128;
    let e = v as i32 - 128;
    let clamp = |x: i32| x.clamp(0, 255) as u32;
    let r = clamp((298 * c + 409 * e + 128) >> 8);
    let g = clamp((298 * c - 100 * d - 208 * e + 128) >> 8);
    let b = clamp((298 * c + 516 * d + 128) >> 8);
    (r << 16) | (g << 8) | b
}

impl Bus for Hw {
    fn read8(&mut self, pa: u32) -> u8 {
        if let Some(r) = self.ram(pa, 1) {
            return r[0];
        }
        let v = self.mmio_read(pa & !3, 4);
        (v >> (8 * (3 - (pa & 3)))) as u8
    }
    fn read16(&mut self, pa: u32) -> u16 {
        if let Some(r) = self.ram(pa, 2) {
            return u16::from_be_bytes([r[0], r[1]]);
        }
        if matches!(pa & 0xFFFF_F000, 0x0C00_0000 | 0x0C00_1000 | 0x0C00_2000 | 0x0C00_5000) {
            return self.mmio_read(pa, 2) as u16;
        }
        let v = self.mmio_read(pa & !3, 4);
        (v >> (8 * (2 - (pa & 2)))) as u16
    }
    fn read32(&mut self, pa: u32) -> u32 {
        if let Some(r) = self.ram(pa, 4) {
            return u32::from_be_bytes(r.try_into().unwrap());
        }
        self.mmio_read(pa, 4)
    }
    fn write8(&mut self, pa: u32, v: u8) {
        if let Some(r) = self.ram(pa, 1) {
            r[0] = v;
            return;
        }
        self.mmio_write(pa, 1, v as u32);
    }
    fn write16(&mut self, pa: u32, v: u16) {
        if let Some(r) = self.ram(pa, 2) {
            r.copy_from_slice(&v.to_be_bytes());
            return;
        }
        self.mmio_write(pa, 2, v as u32);
    }
    fn write32(&mut self, pa: u32, v: u32) {
        if let Some(r) = self.ram(pa, 4) {
            r.copy_from_slice(&v.to_be_bytes());
            return;
        }
        self.mmio_write(pa, 4, v);
    }
    fn invalidate_code(&mut self, pa: u32, len: u32) {
        if self.invalidated.len() < 4096 {
            self.invalidated.push((pa, len));
        }
    }
}
