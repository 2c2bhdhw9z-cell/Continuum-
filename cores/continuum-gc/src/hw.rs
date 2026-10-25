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

/// The video interface's DTV status register (VISEL): bit 0 a component cable (progressive
/// scan possible), bit 1 a Japanese console.
pub const VI_DTV: usize = 0x6E;

/// Video lines a disc drive command takes (about 130 microseconds): far quicker than a real
/// drive, so games load fast, but never inside the write that starts it.
pub const DI_COMMAND_LINES: u64 = 2;

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
    /// Polling: video lines since the last poll, and polls so far this field.
    pub poll_lines: u32,
    pub polls: u32,
}

// SISR: a byte per channel (channel 0 the top one): RDST (new input in the channel's buffer,
// until the program reads it), WRST, NOREP (no controller answered), COLL, OVRUN, UNRUN.
const SISR_RDST: u32 = 0x20;
const SISR_NOREP: u32 = 0x08;
const SISR_ERRORS: u32 = 0x0F;
// SICOMCSR: TCINT and its mask, COMERR (the last transfer failed), RDSTINT (a channel has new
// input) and its mask.
const SI_TCINT: u32 = 1 << 31;
const SI_TCINTMSK: u32 = 1 << 30;
const SI_COMERR: u32 = 1 << 29;
const SI_RDSTINT: u32 = 1 << 28;
const SI_RDSTINTMSK: u32 = 1 << 27;

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
    /// Seconds since 1 January 2000 (the console's clock): set from the phone's clock when a
    /// game is loaded (Machine::set_clock), and counting on with the video lines.
    pub rtc: u32,
    /// Toward the clock's next second, in hundredths of a video line.
    pub rtc_frac: u32,
    /// The memory card in slot A: its command state (its bytes are Hw::card).
    pub card: crate::memcard::Port,
}

impl Default for Exi {
    fn default() -> Self {
        Exi { ch: Default::default(), sram: default_sram(), rtc: 0, rtc_frac: 0, card: Default::default() }
    }
}

/// SRAM offsets: the counter bias (added to the clock), the language, the flags (bits 0-1 video
/// mode, bit 2 stereo, bit 7 progressive scan), and per slot the flash ID of the memory card
/// last unlocked there and that ID's checksum.
pub const SRAM_COUNTER_BIAS: usize = 0x0C;
pub const SRAM_LANGUAGE: usize = 0x12;
pub const SRAM_FLAGS: usize = 0x13;
pub const SRAM_FLASH_ID: usize = 0x14;
pub const SRAM_FLASH_ID_CHECKSUM: usize = 0x3A;

/// The 64-byte battery-backed SRAM with valid checksums: English, stereo, NTSC, no offsets, and
/// flash IDs of zeros for both memory card slots.
pub fn default_sram() -> Vec<u8> {
    let mut s = vec![0u8; 64];
    s[SRAM_LANGUAGE] = 0;
    // Stereo, and bits 3 and 5, which consoles have set.
    s[SRAM_FLAGS] = 0x2C;
    for slot in 0..2 {
        s[SRAM_FLASH_ID_CHECKSUM + slot] = flash_id_checksum(&[0; 12]);
    }
    sram_checksums(&mut s);
    s
}

/// The checksums over 0x0C..0x14 (big-endian halfwords): the sum and the inverted sum.
fn sram_checksums(s: &mut [u8]) {
    let mut sum = 0u16;
    let mut inv = 0u16;
    for i in (0x0C..0x14).step_by(2) {
        let w = u16::from_be_bytes([s[i], s[i + 1]]);
        sum = sum.wrapping_add(w);
        inv = inv.wrapping_add(!w);
    }
    s[0..2].copy_from_slice(&sum.to_be_bytes());
    s[2..4].copy_from_slice(&inv.to_be_bytes());
}

/// A flash ID's checksum in SRAM: its bytes summed, inverted.
fn flash_id_checksum(id: &[u8]) -> u8 {
    !id.iter().fold(0u8, |a, b| a.wrapping_add(*b))
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
    /// Video lines until the current command completes (0: idle), and the status bit it ends
    /// with (TCINT, or DEINT for an error). A real drive always takes a moment, and Nintendo's
    /// disc library relies on it: it starts a command, then sets its ten-second timeout, which
    /// the completion interrupt cancels. Completing inside the register write raised the
    /// interrupt before the timeout existed, and ten seconds later the timeout masked the
    /// drive's interrupt for good.
    pub busy: u64,
    pub ends_with: u32,
    /// Music the drive streams off the disc (dtk.rs), and where the audio interface is between
    /// two of its 32 kHz samples when it plays them at that rate.
    pub stream: crate::dtk::Stream,
    pub stream_phase: u32,
    pub stream_last: (i16, i16),
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
    /// The stream sample counter (AISCNT) as of the CPU cycle `at`. While the stream plays,
    /// it counts on by itself, one sample every `Hw::ai_cycles_per_sample` cycles from `at`:
    /// reading it works the count out from the CPU's time then (Nintendo's library times how
    /// long one step takes, to the microsecond).
    pub scnt: u32,
    pub it: u32,
    pub at: u64,
}

/// AICR bits: the stream plays (PSTAT); its rate is 48 kHz (AFR; else 32 kHz); the sample
/// interrupt is unmasked (AIINTMSK); the counter reached AIIT (AIINT, cleared by writing 1);
/// zero the counter (SCRESET); the DSP's own output is 32 kHz (AIDFR; else 48 kHz).
pub const AI_PSTAT: u32 = 0x01;
pub const AI_AFR: u32 = 0x02;
pub const AI_AIINTMSK: u32 = 0x04;
pub const AI_AIINT: u32 = 0x08;
pub const AI_SCRESET: u32 = 0x20;
pub const AI_AIDFR: u32 = 0x40;

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
    /// Code addresses invalidated (icbi, HID0 ICFI, writes into code pages): read by the block
    /// caches of the faster CPUs.
    pub invalidated: Vec<(u32, u32)>,
    /// One bit per 4 KB physical page holding cached code (MEM1, then MEM2 from bit 8192).
    pub code_pages: Vec<u64>,
    /// Lines the program printed through the debug UART (EXI channel 0, device 1).
    pub uart: Vec<u8>,
    /// Register addresses nothing answers yet, with how often they were touched (development aid).
    pub unknown: std::collections::BTreeMap<u32, u32>,
    /// Disc drive commands seen, by command byte (development aid).
    pub di_commands: std::collections::BTreeMap<u8, u32>,
    /// The first disc drive register writes (offset, value), for development.
    pub di_trace: Vec<(u32, u32)>,
    /// EXI channel 0 and 1 traffic, when a development tool turns it on (Some).
    pub exi_trace: Option<Vec<String>>,
    /// CPU cycles the program waits for that have not been counted yet: an immediate EXI
    /// transfer takes its bytes' time on the wire, which programs wait out right away
    /// (EXISync), so the machine adds it to the CPU's time once the write that started it has
    /// finished.
    pub stall: u64,
    /// Something that can change the interrupt line happened since the CPU last looked.
    pub irq_dirty: bool,
    /// The CPU's clock, in CPU cycles, at the register access being handled (the CPU tells
    /// the bus before each access; the machine before each video line).
    pub now: u64,
    /// Time spent in our stand-ins for the sound programs (the speed read-out).
    pub dsp_time: std::time::Duration,
    /// Sound for the frontend: interleaved left/right 16-bit samples at 48 kHz.
    pub audio_out: Vec<i16>,
    /// Video lines run since start (`vi_line_count`).
    vi_lines_run: u64,
    /// The last picture `vi_picture_if_new` converted: its layout and raw bytes.
    last_xfb: Option<(XfbKey, Vec<u8>)>,
    /// Telemetry counters.
    pub tel: HwTel,
    /// Audio clock: DMA samples owed, in hundredths of a video line.
    audio_phase: u64,
    /// Resampling 32 kHz to 48 kHz: the previous input frame and the position between frames.
    audio_prev: (i16, i16),
    audio_t: f32,
    /// An SD card adapter and the EXI channel it sits on (2: serial port 2, like SD2SP2).
    pub sd: Option<crate::sd::SdCard>,
    pub sd_channel: usize,
    /// The memory card in slot A, byte for byte (memcard.rs; the game's libretro save RAM, so
    /// it is never reallocated while a game runs); empty when the slot is empty (a Wii).
    pub card: Vec<u8>,
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
            code_pages: vec![0; (8192 + 16384) / 64],
            uart: Vec::new(),
            unknown: Default::default(),
            di_commands: Default::default(),
            di_trace: Vec::new(),
            exi_trace: None,
            stall: 0,
            sd: None,
            sd_channel: 2,
            card: if wii { Vec::new() } else { crate::memcard::blank(&Self::card_format(0, &default_sram(), 0)) },
            irq_dirty: true,
            now: 0,
            dsp_time: std::time::Duration::ZERO,
            audio_out: Vec::with_capacity(4096),
            vi_lines_run: 0,
            last_xfb: None,
            tel: HwTel::default(),
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
        let comcsr = self.si.comcsr;
        if comcsr & (SI_TCINT | SI_TCINTMSK) == SI_TCINT | SI_TCINTMSK || (comcsr & SI_RDSTINTMSK != 0 && self.si_new_input()) {
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
        let pa = pa & 0x1FFF_FFFF;
        if let Some(r) = self.ram(pa, data.len() as u32) {
            r.copy_from_slice(data);
            if !data.is_empty() {
                for page in (pa >> 12)..=((pa + data.len() as u32 - 1) >> 12) {
                    self.touch_page(page);
                }
            }
        }
    }

    /// The bit for a physical page in `code_pages`, if it is RAM.
    #[inline]
    fn code_bit(page: u32) -> Option<usize> {
        if page < 0x1800 {
            Some(page as usize)
        } else if (0x10000..0x14000).contains(&page) {
            Some(8192 + (page - 0x10000) as usize)
        } else {
            None
        }
    }

    /// Marks a physical page as holding cached code.
    pub fn mark_code_page(&mut self, page: u32) {
        if let Some(b) = Self::code_bit(page) {
            self.code_pages[b / 64] |= 1 << (b % 64);
        }
    }

    /// Memory in a page changed: if it held cached code, that code is dropped (and true is
    /// returned).
    #[inline]
    pub(crate) fn touch_page(&mut self, page: u32) -> bool {
        if let Some(b) = Self::code_bit(page) {
            let w = &mut self.code_pages[b / 64];
            if *w & (1 << (b % 64)) != 0 {
                *w &= !(1 << (b % 64));
                self.invalidate_code(page << 12, 0x1000);
                return true;
            }
        }
        false
    }

    pub fn dma_read(&mut self, pa: u32, len: usize) -> Vec<u8> {
        self.ram(pa & 0x1FFF_FFFF, len as u32).map(|r| r.to_vec()).unwrap_or_else(|| vec![0; len])
    }

    // ------------------------------------------------------------------ registers

    fn mmio_read(&mut self, pa: u32, n: u32) -> u32 {
        self.irq_dirty = true;
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
        // Bytes that only fill the write-gather pipe cannot change any interrupt; everything
        // else might.
        if pa & 0xFFFF_F000 != 0x0C00_8000 {
            self.irq_dirty = true;
        }
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
                    self.irq_dirty = true;
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
            // The DTV status says what the console is; programs only read it.
            if o as usize == VI_DTV {
                return;
            }
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

    /// One video line passed. Raises display interrupts and marks the end of each field; the
    /// clock and the memory card's erases and programs move on with it.
    /// Video lines run since the machine started (tests and the read-out).
    pub fn vi_line_count(&self) -> u64 {
        self.vi_lines_run
    }

    pub fn vi_line(&mut self) {
        self.vi_lines_run += 1;
        self.irq_dirty = true;
        let total = self.vi_lines_per_frame();
        // Lines a second, in hundredths: 525 x 29.97 (NTSC) or 625 x 25 (PAL).
        let per_second = if total == 625 { 1_562_500 } else { 1_573_425 };
        self.exi.rtc_frac += 100;
        if self.exi.rtc_frac >= per_second {
            self.exi.rtc_frac -= per_second;
            self.exi.rtc = self.exi.rtc.wrapping_add(1);
        }
        let before = self.exi.card.irq();
        self.exi.card.line();
        self.card_irq_edge(before);
        self.di_line();
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
        let field_start = line == 1 || line == total / 2 + 1;
        if field_start {
            self.vi.field_done = true;
            self.vi.odd_field = line != 1;
        }
        self.si_poll_line(field_start);
    }

    /// The picture VI is showing, converted from the external framebuffer's YUV to 0RGB.
    /// Returns (width, height, pixels); nothing until a program has turned the display on.
    /// `vi_picture`, but only when the picture on the TV changed since the last call: the
    /// framebuffer's bytes, where it is and its shape are compared with the last picture's (a
    /// byte compare, far cheaper than converting 300,000 pixels from YUV every frame).
    /// `Picture::Same` means the frontend can keep showing what it has.
    pub fn vi_picture_if_new(&mut self) -> Picture {
        let t = std::time::Instant::now();
        let p = self.vi_picture_if_new_inner();
        self.tel.picture_time += t.elapsed();
        match p {
            Picture::New(..) => self.tel.pictures_new += 1,
            Picture::Same => self.tel.pictures_same += 1,
            Picture::Off => {}
        }
        p
    }

    fn vi_picture_if_new_inner(&mut self) -> Picture {
        let Some((key, raw)) = self.xfb_raw() else {
            self.last_xfb = None;
            return Picture::Off;
        };
        if let Some((k, r)) = &self.last_xfb {
            if *k == key && *r == raw {
                return Picture::Same;
            }
        }
        let (width, height) = (key.3, key.4);
        let mut out = vec![0u32; (width * height) as usize];
        for (o, px) in out.chunks_exact_mut(2).zip(raw.chunks_exact(4)) {
            o[0] = yuv_to_rgb(px[0], px[1], px[3]);
            o[1] = yuv_to_rgb(px[2], px[1], px[3]);
        }
        self.last_xfb = Some((key, raw));
        Picture::New(width, height, out)
    }

    /// The framebuffer VI shows, as raw YUYV rows in display order, with what decides its layout.
    fn xfb_raw(&mut self) -> Option<(XfbKey, Vec<u8>)> {
        let dcr = self.vi.regs[1];
        if dcr & 1 == 0 {
            return None;
        }
        let progressive = dcr & 4 != 0;
        let acv = ((self.vi.regs[0] >> 4) & 0x3FF) as u32;
        let hsw = self.vi.regs[0x48 / 2];
        let width = ((hsw >> 8) & 0x7F) as u32 * 16;
        let stride = (hsw & 0xFF) as u32 * 32;
        if width == 0 || acv == 0 {
            return None;
        }
        let fb_addr = |reg: u32| if reg & 0x1000_0000 != 0 { (reg & 0x00FF_FFFF) << 5 } else { reg & 0x00FF_FFFF };
        let top = fb_addr(self.vi_reg32(0x1C));
        let bottom = fb_addr(self.vi_reg32(0x24));
        let height = if progressive { acv } else { acv * 2 };
        let row_bytes = (width * 2) as usize;
        let mut raw = Vec::with_capacity(row_bytes * height as usize);
        for y in 0..height {
            let (base, row) = if progressive { (top, y) } else if y % 2 == 0 { (top, y / 2) } else { (bottom, y / 2) };
            match self.ram((base + row * stride) & 0x1FFF_FFFF, row_bytes as u32) {
                Some(r) => raw.extend_from_slice(r),
                None => raw.resize(raw.len() + row_bytes, 0),
            }
        }
        Some(((top, bottom, stride, width, height, progressive), raw))
    }

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
                    4 => {
                        // Reading the buffer's first word takes the new input.
                        self.si.sr &= !(SISR_RDST << (8 * (3 - ch)));
                        self.irq_dirty = true;
                        self.pad_word(ch, true)
                    }
                    _ => self.pad_word(ch, false),
                }
            }
            0x30 => self.si.poll,
            0x34 => self.si.comcsr | if self.si_new_input() { SI_RDSTINT } else { 0 },
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
                // TCINT (31) is cleared by writing 1; COMERR (29) and RDSTINT (28) are status;
                // the masks and the transfer's settings are stored.
                if v & SI_TCINT != 0 {
                    self.si.comcsr &= !SI_TCINT;
                }
                self.si.comcsr = (self.si.comcsr & (SI_TCINT | SI_COMERR)) | (v & !(SI_TCINT | SI_COMERR | SI_RDSTINT | 1));
                if v & 1 != 0 {
                    self.si_transfer();
                }
            }
            // Error bits clear on writing 1; WR (copy the commands out) happens at once.
            0x38 => {
                let errors = (0..4).fold(0, |m, ch| m | SISR_ERRORS << (8 * ch));
                self.si.sr &= !(v & errors);
            }
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

    /// Whether a channel has input the program has not read yet (RDSTINT).
    fn si_new_input(&self) -> bool {
        self.si.sr & (SISR_RDST * 0x0101_0101) != 0
    }

    /// One video line of SI's polling (SIPOLL: X lines apart, Y polls a field, channels on in
    /// EN): each poll brings a channel's controller state into its input buffer (RDST), or finds
    /// nobody there (NOREP). Programs read the buffers when RDST says so, many of them from the
    /// RDSTINT interrupt.
    fn si_poll_line(&mut self, field_start: bool) {
        if field_start {
            self.si.poll_lines = 0;
            self.si.polls = 0;
        }
        let x = (self.si.poll >> 16) & 0x3FF;
        let y = (self.si.poll >> 8) & 0xFF;
        let enabled = (self.si.poll >> 4) & 0xF;
        if enabled == 0 || self.si.polls >= y.max(1) {
            return;
        }
        self.si.poll_lines += 1;
        if self.si.poll_lines < x.max(1) {
            return;
        }
        self.si.poll_lines = 0;
        self.si.polls += 1;
        for ch in 0..4 {
            if enabled & (8 >> ch) != 0 {
                let bit = if self.si.pads[ch].connected { SISR_RDST } else { SISR_NOREP };
                self.si.sr |= bit << (8 * (3 - ch));
            }
        }
        self.irq_dirty = true;
    }

    /// A command through SI's I/O buffer (COMCSR: channel bits 1-2, in length 8-14, out 16-22).
    fn si_transfer(&mut self) {
        self.si.comcsr &= !SI_COMERR;
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
            // No response on this channel (SISR NOREP bit for the channel), and the transfer
            // failed (COMERR).
            self.si.sr |= SISR_NOREP << (8 * (3 - ch));
            self.si.comcsr |= SI_COMERR;
        }
        for (k, b) in reply.iter().enumerate() {
            self.si.buf[k] = *b;
        }
        self.si.comcsr &= !1;
        self.si.comcsr |= 0x8000_0000;
    }

    // ------------------------------------------------------------------ EXI

    fn exi_read(&mut self, off: u32) -> u32 {
        // The three channels' registers repeat every 64 bytes (libogc2's EXI_Sync reads the
        // control register through the copy at 0x8C + 20 x channel).
        let off = off & 0x3F;
        let (c, r) = ((off / 0x14) as usize, off % 0x14);
        if c > 2 {
            return 0;
        }
        // EXT (bit 12): a device is plugged into the slot (channels 0 and 1 are the memory card
        // slots A and B).
        let ext = match c {
            0 => self.card_in_slot_a() || (self.sd.is_some() && self.sd_channel == 0),
            1 => self.sd.is_some() && self.sd_channel == 1,
            _ => false,
        };
        let ch = &self.exi.ch[c];
        match r {
            0x00 => ch.csr | if ext { 0x1000 } else { 0 },
            0x04 => ch.mar,
            0x08 => ch.len,
            0x0C => ch.cr,
            _ => ch.data,
        }
    }

    fn exi_write(&mut self, off: u32, v: u32) {
        let off = off & 0x3F;
        let (c, r) = ((off / 0x14) as usize, off % 0x14);
        if c > 2 {
            return;
        }
        match r {
            0x00 => {
                let ch = &mut self.exi.ch[c];
                // Interrupt status bits (1, 3, 11) clear on writing 1; EXT (12) is only read;
                // the rest is stored.
                let clear = v & 0x80A;
                let old_cs = ch.csr & 0x380;
                ch.csr = (ch.csr & 0x80A & !clear) | (v & !0x80A & 0x2FF5);
                let new_cs = ch.csr & 0x380;
                if new_cs != old_cs {
                    ch.dev_addr = None;
                }
                let csr = ch.csr;
                if let Some(t) = self.exi_trace.as_mut().filter(|_| c < 2) {
                    t.push(format!("ch{c} csr <- {v:08X} = {csr:08X}{}", if new_cs != old_cs { " (cs change)" } else { "" }));
                }
                // Device 0 of channel 0 (chip select bit 7) is the memory card in slot A.
                if c == 0 && (old_cs ^ new_cs) & 0x80 != 0 && self.card_in_slot_a() {
                    if new_cs & 0x80 != 0 {
                        self.exi.card.select();
                    } else {
                        self.exi.card.deselect(&mut self.card);
                    }
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
    /// UART chip; channel 0, device 0 the memory card in slot A (memcard.rs); an SD card
    /// adapter answers on its channel's device 0 (and, like the real adapters, also while no
    /// chip select is up); every other slot is empty.
    fn exi_transfer(&mut self, c: usize) {
        let cr = self.exi.ch[c].cr;
        let dma = cr & 2 != 0;
        let rw = (cr >> 2) & 3; // 0 read, 1 write, 2 read-write
        let len = if dma { self.exi.ch[c].len } else { ((cr >> 4) & 3) + 1 };
        let cs = (self.exi.ch[c].csr >> 7) & 7;
        if !dma {
            // 8 bits a byte at the channel's clock (CSR bits 4-6: 1 MHz doubled that many
            // times, at most 32 MHz). DMA runs alongside the program and costs it nothing here.
            let clk = ((self.exi.ch[c].csr >> 4) & 7).min(5);
            let cycles_per_us = if self.wii { 729 } else { 486 };
            self.stall += (len as u64 * 8 * cycles_per_us) >> clk;
        }
        let sd_here = self.sd.is_some() && self.sd_channel == c && (cs == 0 || cs == 1);
        if sd_here {
            self.sd_transfer(c, dma, rw, len);
        } else if c == 0 && cs == 1 && self.card_in_slot_a() {
            let out = self.exi.ch[0].data;
            self.card_transfer(dma, rw, len);
            if let Some(t) = self.exi_trace.as_mut() {
                let back = self.exi.ch[0].data;
                t.push(format!("card {} rw{rw} len {len} out {out:08X} in {back:08X} status {:02X}", if dma { "dma" } else { "imm" }, self.exi.card.status()));
            }
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
        if let Some(t) = self.exi_trace.as_mut().filter(|_| c < 2 && !(c == 0 && cs == 1)) {
            t.push(format!("ch{c} cs{cs} {} rw{rw} len {len} data {:08X}", if dma { "dma" } else { "imm" }, self.exi.ch[c].data));
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

    /// A transfer with the memory card in slot A, byte by byte through its command logic.
    fn card_transfer(&mut self, dma: bool, rw: u32, len: u32) {
        let before = self.exi.card.irq();
        if dma {
            let mar = self.exi.ch[0].mar;
            if rw == 0 {
                let bytes: Vec<u8> = (0..len).map(|_| self.exi.card.exchange(&self.card, 0)).collect();
                self.dma_write(mar, &bytes);
            } else {
                let bytes = self.dma_read(mar, len as usize);
                let back: Vec<u8> = bytes.iter().map(|b| self.exi.card.exchange(&self.card, *b)).collect();
                if rw == 2 {
                    self.dma_write(mar, &back);
                }
            }
        } else {
            let out = self.exi.ch[0].data.to_be_bytes();
            let mut back = [0u8; 4];
            for k in 0..len as usize {
                back[k] = self.exi.card.exchange(&self.card, if rw == 0 { 0 } else { out[k] });
            }
            if rw != 1 {
                self.exi.ch[0].data = u32::from_be_bytes(back);
            }
        }
        self.card_irq_edge(before);
    }

    /// The card's interrupt line was raised (or switched on while up): EXIINT on channel 0.
    fn card_irq_edge(&mut self, before: bool) {
        if !before && self.exi.card.irq() {
            self.exi.ch[0].csr |= 0x2;
            self.irq_dirty = true;
        }
    }

    /// What a blank card is formatted with on this console: `tb` (time-base ticks) as the
    /// format time, SRAM's counter bias and language, and the video interface's DTV status,
    /// whose bit 1 (a Japanese console) means Shift-JIS, as the library's OSGetFontEncode says.
    fn card_format(tb: u64, sram: &[u8], dtv: u16) -> crate::memcard::Format {
        crate::memcard::Format {
            time: tb,
            counter_bias: u32::from_be_bytes(sram[SRAM_COUNTER_BIAS..SRAM_COUNTER_BIAS + 4].try_into().unwrap()),
            language: sram[SRAM_LANGUAGE],
            dtv,
            encoding: (dtv >> 1) & 1,
        }
    }

    /// Puts a freshly formatted blank card in slot A, made `tb` time-base ticks after 2000
    /// began (on a GameCube; the frontend then puts the game's own card over it, if it has one).
    pub fn insert_blank_card(&mut self, tb: u64) {
        if self.wii {
            return;
        }
        let dtv = self.vi.regs[VI_DTV / 2];
        self.card = crate::memcard::blank(&Self::card_format(tb, &self.exi.sram, dtv));
    }

    /// Whether the memory card answers in slot A: a card is in, and no SD adapter sits there.
    fn card_in_slot_a(&self) -> bool {
        let sd_in_slot_a = self.sd.is_some() && self.sd_channel == 0;
        !self.card.is_empty() && !sd_in_slot_a
    }

    /// SRAM as a console holds it after unlocking the card in slot A: with that card's flash
    /// ID, so the card library accepts whichever console formatted the card.
    fn sync_card_flash_id(&mut self) {
        if !self.card_in_slot_a() {
            return;
        }
        let id = crate::memcard::flash_id_of(&self.card);
        self.exi.sram[SRAM_FLASH_ID..SRAM_FLASH_ID + 12].copy_from_slice(&id);
        self.exi.sram[SRAM_FLASH_ID_CHECKSUM] = flash_id_checksum(&id);
    }

    /// One byte of a DMA from the clock/SRAM chip: SRAM when the address is SRAM's.
    fn rtc_byte(&mut self, addr: u32, k: usize) -> u8 {
        let a = addr & 0x7FFF_FFFF;
        if (0x2000_0100..0x2000_0100 + (64 << 6)).contains(&a) {
            if k == 0 {
                self.sync_card_flash_id();
            }
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
                    self.sync_card_flash_id();
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
                self.tel.disc_reads += 1;
                self.tel.disc_bytes += len as u64;
                if error {
                    self.tel.disc_errors += 1;
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
            // Streamed music (dtk.rs): play or queue a track (offset in 4-byte units, length in
            // bytes; length 0 stops at the track's end), or stop at once.
            0xE1 => {
                if (self.di.cmd[0] >> 16) & 0xFF == 0 {
                    self.di.stream.play((self.di.cmd[1] as u64) << 2, self.di.cmd[2]);
                } else {
                    self.di.stream.cancel();
                }
                self.di.imm = 0;
            }
            // Streaming status: playing, position, track start, track length.
            0xE2 => self.di.imm = self.di.stream.status((self.di.cmd[0] >> 16) & 0xFF),
            // Seek, stop motor, audio buffer config: accept.
            0xAB | 0xE3 | 0xE4 => self.di.imm = 0,
            _ => error = true,
        }
        // TSTART stays set until the command completes (vi_line).
        self.di.ends_with = if error { 0x04 } else { 0x10 };
        self.di.busy = DI_COMMAND_LINES;
    }

    /// One video line of the drive: a command in progress may complete.
    fn di_line(&mut self) {
        if self.di.busy == 0 {
            return;
        }
        self.di.busy -= 1;
        if self.di.busy == 0 {
            self.di.cr &= !1;
            self.di.sr |= self.di.ends_with;
            self.irq_dirty = true;
        }
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

    pub(crate) fn dsp_deliver(&mut self) {
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
            let t = std::time::Instant::now();
            self.dsp_hle_mail(mail);
            self.dsp_time += t.elapsed();
        }
    }

    /// ARAM DMA: main memory and the 16 MB of audio RAM, done at once.
    /// An ARAM DMA. Past the console's 16 MB of ARAM is the expansion port, and a retail
    /// console has nothing there: reads give zeros, writes go nowhere. (The OS sizes ARAM by
    /// writing past the end and reading back; wrapping round to the start made it find 4 MB
    /// of expansion that was really the start of ARAM again.)
    fn aram_dma(&mut self) {
        let len = (self.dsp.ar_cnt & 0x03FF_FFE0) as usize;
        self.tel.aram_dmas += 1;
        self.tel.aram_bytes += len as u64;
        let to_main = self.dsp.ar_cnt & 0x8000_0000 != 0;
        let aram = (self.dsp.ar_addr & 0x03FF_FFE0) as usize;
        let mm = self.dsp.ar_mm & 0x03FF_FFE0;
        if to_main {
            let data: Vec<u8> = (0..len).map(|k| self.aram.get(aram + k).copied().unwrap_or(0)).collect();
            self.dma_write(mm, &data);
        } else {
            let data = self.dma_read(mm, len);
            for (k, b) in data.into_iter().enumerate() {
                if let Some(a) = self.aram.get_mut(aram + k) {
                    *a = b;
                }
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
            0x08 => {
                self.ai_sync();
                self.ai.scnt
            }
            0x0C => self.ai.it,
            _ => 0,
        }
    }

    fn ai_write(&mut self, off: u32, v: u32) {
        // Everything up to this moment counts at the old settings.
        self.ai_sync();
        let now = self.now.max(self.ai.at);
        match off {
            0x00 => {
                let old = self.ai.cr;
                // AIINT clears on 1; SCRESET zeroes the sample counter.
                let clear = v & AI_AIINT;
                self.ai.cr = (self.ai.cr & AI_AIINT & !clear) | (v & 0x57);
                if v & AI_SCRESET != 0 {
                    self.ai.scnt = 0;
                }
                // A start, a stop, a new rate or a reset: the next sample is a whole sample
                // period from now.
                if (old ^ self.ai.cr) & (AI_PSTAT | AI_AFR) != 0 || v & AI_SCRESET != 0 {
                    self.ai.at = now;
                }
            }
            0x04 => self.ai.vr = v,
            0x08 => {
                self.ai.scnt = v;
                self.ai.at = now;
            }
            0x0C => self.ai.it = v,
            _ => {}
        }
    }

    /// The CPU clock.
    pub fn cpu_hz(&self) -> u64 {
        if self.wii {
            WII_CPU_HZ
        } else {
            GC_CPU_HZ
        }
    }

    /// CPU cycles per stream sample. The sample clock is 54 MHz / 1124 at "48 kHz" (48,043
    /// Hz) and 54 MHz / 1686 at "32 kHz" (32,028 Hz); a whole number of CPU cycles on both
    /// consoles (GameCube 10,116 and 15,174, Wii 15,174 and 22,761).
    pub fn ai_cycles_per_sample(&self) -> u64 {
        let div = if self.ai.cr & AI_AFR != 0 { 1124 } else { 1686 };
        self.cpu_hz() * div / 54_000_000
    }

    /// Brings the stream sample counter up to the CPU's time now (`now`): while the stream
    /// plays, one sample every sample period since `at`, and AIINT when the counter reaches
    /// AIIT on the way.
    fn ai_sync(&mut self) {
        let now = self.now.max(self.ai.at);
        if self.ai.cr & AI_PSTAT == 0 {
            self.ai.at = now;
            return;
        }
        let per = self.ai_cycles_per_sample();
        let n = (now - self.ai.at) / per;
        if n == 0 {
            return;
        }
        self.ai.at += n * per;
        let n = n.min(u32::MAX as u64) as u32;
        let before = self.ai.scnt;
        self.ai.scnt = before.wrapping_add(n);
        // AIIT is one of the values the counter took: before + 1 ..= before + n.
        if self.ai.it.wrapping_sub(before.wrapping_add(1)) < n && self.ai.cr & AI_AIINT == 0 {
            self.ai.cr |= AI_AIINT;
            self.irq_dirty = true;
        }
    }

    /// Once a video line (the machine has set `now`): the sample counter moves on, so its
    /// interrupt comes even while nothing reads it.
    pub fn ai_line(&mut self) {
        self.ai_sync();
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

    /// One 48 kHz output frame with the drive's streamed music mixed in: while the audio
    /// interface plays the stream (AICR PSTAT), at 48 kHz (AFR) or 32 kHz, at AIVR's volume
    /// (left in bits 0-7, right in 8-15).
    fn with_stream(&mut self, dsp: (i16, i16)) -> (i16, i16) {
        if self.ai.cr & 1 == 0 || !self.di.stream.playing {
            return dsp;
        }
        let s = if self.ai.cr & 2 != 0 {
            self.di.stream.sample(self.disc.as_mut())
        } else {
            // Two stream samples for every three output frames.
            self.di.stream_phase += 2;
            if self.di.stream_phase >= 3 {
                self.di.stream_phase -= 3;
                self.di.stream_last = self.di.stream.sample(self.disc.as_mut());
            }
            self.di.stream_last
        };
        let vol = |v: i16, shift: u32| (v as i32 * ((self.ai.vr >> shift) & 0xFF) as i32) >> 8;
        let add = |a: i16, b: i32| (a as i32 + b).clamp(-32768, 32767) as i16;
        (add(dsp.0, vol(s.0, 0)), add(dsp.1, vol(s.1, 8)))
    }

    /// One video line of sound: the DMA plays at 32 or 48 kHz; the output is always 48 kHz.
    pub fn audio_line(&mut self) {
        self.irq_dirty = true;
        let lines_x100: u64 = if self.vi_lines_per_frame() == 625 { 1_562_500 } else { 1_573_426 };
        let rate: u64 = if self.ai.cr & 0x40 != 0 { 32_000 } else { 48_000 };
        self.audio_phase += rate * 100;
        while self.audio_phase >= lines_x100 {
            self.audio_phase -= lines_x100;
            let running = self.dsp.dma_ctl & 0x8000 != 0;
            let f = self.audio_dma_frame();
            if running {
                self.tel.dma_frames += 1;
            } else {
                self.tel.dma_silent += 1;
            }
            if rate == 32_000 {
                self.tel.dma_32k += 1;
            }
            if self.ai.cr & 1 != 0 && self.di.stream.playing {
                self.tel.stream_frames += 1;
            }
            if rate == 48_000 {
                let (l, r) = self.with_stream(f);
                self.audio_out.extend_from_slice(&[l, r]);
            } else {
                let step = 32_000.0 / 48_000.0;
                let p = self.audio_prev;
                while self.audio_t <= 1.0 {
                    let t = self.audio_t;
                    let mix = |a: i16, b: i16| (a as f32 + (b as f32 - a as f32) * t) as i16;
                    let (l, r) = self.with_stream((mix(p.0, f.0), mix(p.1, f.1)));
                    self.audio_out.extend_from_slice(&[l, r]);
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

/// Running totals for the telemetry line (libretro.rs): never saved, never read by the machine.
#[derive(Default, Clone, Copy)]
pub struct HwTel {
    pub disc_reads: u64,
    pub disc_bytes: u64,
    pub disc_errors: u64,
    pub aram_dmas: u64,
    pub aram_bytes: u64,
    /// Output sound frames whose DMA was running, and those it was not (silence).
    pub dma_frames: u64,
    pub dma_silent: u64,
    /// Sound frames made at the 32 kHz DMA rate.
    pub dma_32k: u64,
    pub stream_frames: u64,
    pub pictures_new: u64,
    pub pictures_same: u64,
    pub picture_time: std::time::Duration,
}

/// What decides how VI lays a framebuffer out: top and bottom field addresses, stride, width,
/// height, progressive.
type XfbKey = (u32, u32, u32, u32, u32, bool);

/// `Hw::vi_picture_if_new`'s answer.
pub enum Picture {
    /// The display is off.
    Off,
    /// The same picture as last time.
    Same,
    New(u32, u32, Vec<u32>),
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
            self.touch_page(pa >> 12);
            return;
        }
        self.mmio_write(pa, 1, v as u32);
    }
    fn write16(&mut self, pa: u32, v: u16) {
        if let Some(r) = self.ram(pa, 2) {
            r.copy_from_slice(&v.to_be_bytes());
            self.touch_page(pa >> 12);
            return;
        }
        self.mmio_write(pa, 2, v as u32);
    }
    fn write32(&mut self, pa: u32, v: u32) {
        if let Some(r) = self.ram(pa, 4) {
            r.copy_from_slice(&v.to_be_bytes());
            self.touch_page(pa >> 12);
            return;
        }
        self.mmio_write(pa, 4, v);
    }
    #[inline]
    fn clock(&mut self, cycles: u64) {
        self.now = cycles;
    }
    fn invalidate_code(&mut self, pa: u32, len: u32) {
        if self.invalidated.len() < 4096 {
            self.invalidated.push((pa, len));
        } else if self.invalidated.last() != Some(&(0, u32::MAX)) {
            // Too many to track one by one: drop everything.
            self.invalidated.push((0, u32::MAX));
        }
    }
}

#[cfg(test)]
mod sound_rate_tests {
    use super::*;

    /// One second of video lines, as `Machine::run_field` drives them: NTSC is 15734.26 a second.
    fn one_second(hw: &mut Hw) -> usize {
        hw.audio_out.clear();
        // 486 MHz / 30888 cycles a line = 15734 lines a second (Machine::cycles_per_line).
        for _ in 0..15_734 {
            hw.audio_line();
            // Taken every frame by retro_run, so it never reaches the overflow drain.
            if hw.audio_out.len() > 4096 {
                hw.audio_out.clear();
            }
        }
        hw.audio_out.clear();
        let mut frames = 0;
        for _ in 0..15_734 {
            hw.audio_line();
            frames += hw.audio_out.len() / 2;
            hw.audio_out.clear();
        }
        frames
    }

    /// The app is told the sound is 48000 Hz (`SAMPLE_RATE` in libretro.rs), so the core must hand
    /// it 48000 frames for every second it emulates, at BOTH DMA rates. On the owner's iPhone
    /// (build 227) it handed over 32031 a second, and the app had to fill the missing third with
    /// silence: 14% of a 30-second recording, in gaps of up to 1.3 seconds.
    #[test]
    fn the_core_emits_48000_frames_a_second_at_either_dma_rate() {
        for (cr, what) in [(0u32, "48 kHz"), (0x40, "32 kHz")] {
            let mut hw = Hw::new(false);
            hw.ai.cr = cr | 1;
            hw.ai.vr = 0xFFFF;
            // A running DMA buffer, so every output frame is a real sample rather than silence.
            hw.dsp.dma_addr = 0x0010_0000;
            hw.dsp.dma_ctl = 0x8000 | 0x7FFF;
            let frames = one_second(&mut hw);
            let off = (frames as i64 - 48_000).abs();
            assert!(
                off < 600,
                "{what}: the core emitted {frames} frames in one second, not ~48000 \
                 (out by {off}); the app fills the difference with silence"
            );
        }
    }
}
