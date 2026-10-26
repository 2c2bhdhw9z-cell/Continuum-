//! The GBA memory map and everything clocked by the CPU: waitstates (WAITCNT and the work RAM's
//! own control, every access sequential or not as the CPU says, and the cartridge's prefetch
//! buffer), I/O registers, the four DMA channels, the four timers, interrupts, the keypad, the
//! display timing, sound (the four PSG channels from the Game Boy core, plus the two DirectSound
//! FIFOs) and the save chip.
//!
//! Every access first spends its cycles and then happens, the way the real bus finishes a read
//! or a write at the end of its last cycle; so a timer read sees the time the read ends.

use serde::{Deserialize, Serialize};

use super::ppu::Ppu;
use super::save::{Save, SaveKind};
use crate::apu::Apu;

pub const CPU_HZ: u64 = 16_777_216;
pub(super) const LINE: u32 = 1232;
const HBLANK_AT: u32 = 1006;

pub const IRQ_VBLANK: u16 = 1;
pub const IRQ_HBLANK: u16 = 2;
pub const IRQ_VCOUNT: u16 = 4;
pub const IRQ_TIMER0: u16 = 8;
pub const IRQ_DMA0: u16 = 0x100;
pub const IRQ_KEYPAD: u16 = 0x1000;

#[derive(Clone, Default, Serialize, Deserialize)]
struct Dma {
    src: u32,
    dst: u32,
    count: u32,
    active: bool,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Timer {
    counter: u16,
    reload: u16,
    ctrl: u16,
    acc: u32,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Fifo {
    data: Vec<i8>,
    current: i8,
}

/// The cartridge's prefetch buffer (WAITCNT bit 14). While the CPU runs from the cartridge, any
/// cycle in which the CPU leaves the cartridge bus alone (an internal cycle, or a read or write
/// somewhere else) is used to read the next halfwords ahead, one at a time with sequential
/// timing, into a queue of eight. An opcode waiting there then takes the CPU one cycle instead of
/// a cartridge read (an ARM opcode is two halfwords).
///
/// The rules, as measured on a real console by the author of the timing test programs in
/// tests/gba.rs (Alyosha's), whose own emulator models them this way:
/// - an opcode read that is not the oldest one held empties the queue: the CPU reads the
///   cartridge itself, and reading ahead starts again after it;
/// - an opcode read of the halfword on its way waits for the rest of that read, except a jump
///   to it in the very first cycle of the first read after such a restart, which is read
///   non-sequentially by the CPU instead;
/// - when the queue is full it stops; the CPU can still take what it holds, and once it has
///   taken the last one the next opcode read is a non-sequential one that starts it again;
/// - at a 128 KiB boundary it keeps reading but never gets anything into the queue, and the
///   CPU reads the boundary itself, non-sequentially;
/// - the CPU reading or writing the cartridge or its save chip throws the queue away, and if
///   that happens in the cycle a halfword being read ahead arrives, it costs one cycle more.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Prefetch {
    /// Allowed to read ahead (it is switched on again by the next opcode read it misses).
    run: bool,
    /// The halfword being read ahead, or the next one to read.
    read: u32,
    /// The oldest halfword held: the next opcode it can hand out. 0 when it has nothing to hand
    /// out (thrown away, or emptied after it was full).
    check: u32,
    /// Halfwords held.
    held: u32,
    /// Cycles spent on the read in progress, 0 when none has started.
    done: u32,
    /// Cycles that read takes.
    wait: u32,
    /// It found the queue full and stopped.
    full: bool,
    /// The read in progress is at a 128 KiB boundary: it never arrives.
    boundary: bool,
    /// It has got past the first cycle of a read since it last started again.
    following: bool,
    /// The next opcode read it cannot hand out is a non-sequential one.
    force_n: bool,
    /// A read arrived in the cycle just ticked.
    arrived: bool,
    /// WAITCNT bit 14.
    enabled: bool,
}

/// Room in the queue, in halfwords.
const PREFETCH_HALVES: u32 = 8;

impl Prefetch {
    /// `n` cycles in which the cartridge bus is free for it, each doing what the console's
    /// prefetch unit does in a cycle: start a read if there is room, or carry one on.
    fn tick(&mut self, mut n: u32) {
        self.arrived = false;
        if !self.run || self.check == 0 || self.full {
            return;
        }
        while n > 0 {
            if self.done == 0 {
                if self.held == PREFETCH_HALVES {
                    self.full = true;
                    return;
                }
                self.boundary = self.read & 0x1_FFFE == 0;
                self.done = 1;
                self.arrived = false;
                n -= 1;
            } else {
                self.following = true;
                let s = n.min(self.wait - self.done);
                self.done += s;
                n -= s;
                if self.done == self.wait {
                    self.done = 0;
                    if !self.boundary {
                        self.held += 1;
                        self.read = self.read.wrapping_add(2);
                    }
                    self.arrived = n == 0;
                    if !self.enabled {
                        self.run = false;
                        return;
                    }
                }
            }
        }
    }

    /// The CPU took the last halfword it held.
    fn emptied(&mut self) {
        if self.full {
            self.check = 0;
            self.force_n = true;
        }
        if !self.enabled {
            self.check = 0;
        }
    }

    /// The CPU read the halfword (or the ARM opcode) it was in the middle of reading: it starts
    /// on the next one after it.
    fn taken_over(&mut self, bytes: u32) {
        self.read = self.read.wrapping_add(bytes);
        self.check = self.check.wrapping_add(bytes);
        self.done = 0;
        if !self.enabled {
            self.check = 0;
            self.run = false;
        }
    }
}

/// Machine state added after the save-state layout was fixed. It is saved after the rest of the
/// machine (`Gba::save_state`), so a state made before it was added still loads, and these then
/// start from their defaults.
#[derive(Clone, Serialize, Deserialize)]
pub struct BusTiming {
    pub prefetch: Prefetch,
    /// The bus cycle just gone was an opcode read, so the next opcode read can be sequential.
    after_code: bool,
    /// The last opcode read was a Thumb one.
    thumb: bool,
    /// The cycle each timer's `counter` was last pinned to: a running timer counts on from it.
    pub timer_at: [u64; 4],
    /// Internal memory control ($04000800): bits 24-27 set the work RAM's wait states.
    pub memcnt: u32,
    /// The cycle each channel switched on to run straight away really starts (two cycles
    /// after the write; the CPU carries on until then), `u64::MAX` for none.
    dma_due: [u64; 4],
    /// The cycle the CPU's last bus access ended: a DMA that comes due while the CPU is only
    /// busy inside itself runs alongside it.
    bus_end: u64,
}

impl Default for BusTiming {
    fn default() -> Self {
        BusTiming { prefetch: Prefetch::default(), after_code: false, thumb: false, timer_at: [0; 4], memcnt: 0x0D00_0020, dma_due: [u64::MAX; 4], bus_end: 0 }
    }
}

/// The mini BIOS this core puts at $00000000: the IRQ vector and the real BIOS's IRQ dispatcher
/// (save registers, call the handler at [$03FFFFFC], restore, return). Everything else the BIOS
/// does is high-level (`bios.rs`).
fn full_fifo() -> [u8; 2] {
    [100; 2]
}

fn yes() -> bool {
    true
}

fn mini_bios() -> Vec<u8> {
    let mut b = vec![0u8; 0x4000];
    let mut put = |a: usize, op: u32| b[a..a + 4].copy_from_slice(&op.to_le_bytes());
    for v in (0..0x20).step_by(4) {
        put(v, 0xEAFF_FFFE); // B . (unused vectors)
    }
    put(0x18, 0xEA00_0042); // B $128
    put(0x128, 0xE92D_500F); // STMFD SP!, {R0-R3, R12, LR}
    put(0x12C, 0xE3A0_0301); // MOV R0, #$04000000
    put(0x130, 0xE28F_E000); // ADD LR, PC, #0
    put(0x134, 0xE510_F004); // LDR PC, [R0, #-4]
    put(0x138, 0xE8BD_500F); // LDMFD SP!, {R0-R3, R12, LR}
    put(0x13C, 0xE25E_F004); // SUBS PC, LR, #4
    // Never run; there so a BIOS read from outside after an IRQ or SWI returns what the real
    // BIOS's prefetch would (games and tests read it).
    put(0x144, 0xE55E_C002);
    put(0x190, 0xE3A0_2004);
    b
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Bus {
    /// DirectSound A/B volume from the core options, percent. Output only, not saved.
    #[serde(skip, default = "full_fifo")]
    pub fifo_vol: [u8; 2],
    /// Smooth DirectSound between samples (core option). Output only.
    #[serde(skip)]
    pub fifo_smooth: bool,
    /// The game's own music engine, mixed again at the output rate ("Advance music quality").
    /// Output only: it reads the engine's state out of the machine's memory every frame.
    #[serde(skip)]
    pub mp2k: super::mp2k::Mp2k,
    #[serde(skip)]
    fifo_from: [i32; 2],
    #[serde(skip)]
    fifo_to: [i32; 2],
    #[serde(skip)]
    fifo_period: [u32; 2],
    #[serde(skip)]
    fifo_age: [u32; 2],
    /// Answer the Game Boy Player handshake and take its rumble bit (core option).
    #[serde(skip)]
    pub gbp: bool,
    /// Keep the BIOS's unpacking inside the size the data asks for, the way VisualBoy Advance
    /// did, instead of spilling over it like a real console (core option). Output only.
    #[serde(skip, default = "yes")]
    pub vba_compat: bool,
    #[serde(skip)]
    gbp_i: u8,
    #[serde(skip)]
    pub gbp_motor: bool,
    #[serde(skip, default = "mini_bios")]
    pub bios: Vec<u8>,
    /// Running a real BIOS file (core option): SWIs go through it instead of the built-in calls.
    #[serde(default)]
    pub real_bios: bool,
    /// Cartridge extras: clock, solar, gyro, rumble or tilt.
    #[serde(default)]
    pub gpio: super::gpio::Gpio,
    /// CPU writes so far, for idle-loop detection (not saved).
    #[serde(skip)]
    pub writes: u64,
    pub ewram: Vec<u8>,
    pub iwram: Vec<u8>,
    pub io: Vec<u8>,
    pub pal: Vec<u8>,
    pub vram: Vec<u8>,
    pub oam: Vec<u8>,
    #[serde(skip)]
    pub rom: Vec<u8>,
    pub save: Save,
    pub ppu: Ppu,
    pub apu: Apu,
    dma: [Dma; 4],
    timers: [Timer; 4],
    fifo: [Fifo; 2],
    pub halted: bool,
    /// Pressed keys, bit set = pressed, in KEYINPUT order.
    pub keys: u16,
    line_cycle: u32,
    pub line: u32,
    /// Cycles spent but not yet run through the rest of the machine.
    pending: u32,
    pub cycles: u64,
    last_addr: u32,
    pub exec_pc: u32,
    pub bios_latch: u32,
    open_bus: u32,
    gb_acc: u32,
    seq_acc: u32,
    #[serde(skip)]
    pub frame_ready: bool,
    #[serde(skip)]
    in_dma: bool,
    /// Saved on its own after the rest (see `BusTiming`).
    #[serde(skip)]
    pub tm: BusTiming,
    /// The link port's transfer, and whether a second phone is on the cable (`sio.rs`).
    #[serde(skip)]
    pub sio: super::sio::Sio,
    /// The Wireless Adapter on the link port (`rfu.rs`), and whether the setting lets a game
    /// that looks for one find it.
    #[serde(skip)]
    pub rfu: super::rfu::Rfu,
    #[serde(skip)]
    pub rfu_allowed: bool,
    /// RCNT's SD pin was driven high in general-purpose mode: the start of the adapter's reset.
    #[serde(skip)]
    rfu_sd_high: bool,
}

impl Bus {
    pub fn new(rom: Vec<u8>) -> Bus {
        let save = Save::detect(&rom);
        let mut apu = Apu::default();
        apu.agb = true;
        apu.write(0xFF26, 0); // the GBA starts with sound off
        let mp2k = super::mp2k::Mp2k::new(&rom);
        Bus {
            fifo_vol: [100; 2],
            fifo_smooth: false,
            mp2k,
            fifo_from: [0; 2],
            fifo_to: [0; 2],
            fifo_period: [1; 2],
            fifo_age: [0; 2],
            gbp: false,
            vba_compat: true,
            gbp_i: 0,
            sio: Default::default(),
            rfu: Default::default(),
            rfu_allowed: true,
            rfu_sd_high: false,
            gbp_motor: false,
            bios: mini_bios(),
            real_bios: false,
            writes: 0,
            gpio: super::gpio::Gpio::new(super::gpio::detect(&rom)),
            ewram: vec![0; 0x40000],
            iwram: vec![0; 0x8000],
            io: vec![0; 0x400],
            pal: vec![0; 0x400],
            vram: vec![0; 0x18000],
            oam: vec![0; 0x400],
            rom,
            save,
            ppu: Ppu::default(),
            apu,
            dma: Default::default(),
            timers: Default::default(),
            fifo: Default::default(),
            halted: false,
            keys: 0,
            line_cycle: 0,
            line: 0,
            pending: 0,
            cycles: 0,
            last_addr: 0,
            exec_pc: 0x0800_0000,
            bios_latch: 0xE129_F000,
            open_bus: 0,
            gb_acc: 0,
            seq_acc: 0,
            frame_ready: false,
            in_dma: false,
            tm: BusTiming::default(),
        }
    }

    pub fn post_bios(&mut self) {
        self.io[0x300] = 1; // POSTFLG
        self.io[0x88] = 0x00;
        self.io[0x89] = 0x02; // SOUNDBIAS $200
        // BG2/BG3 affine identity.
        for base in [0x20usize, 0x30] {
            self.io[base] = 0;
            self.io[base + 1] = 1;
            self.io[base + 6] = 0;
            self.io[base + 7] = 1;
        }
    }

    pub(super) fn io16(&self, a: usize) -> u16 {
        u16::from(self.io[a]) | u16::from(self.io[a + 1]) << 8
    }

    pub(super) fn set_io16(&mut self, a: usize, v: u16) {
        self.io[a] = v as u8;
        self.io[a + 1] = (v >> 8) as u8;
    }

    pub fn request_irq(&mut self, bits: u16) {
        let v = self.io16(0x202) | bits;
        self.set_io16(0x202, v);
    }

    /// IME on and an enabled interrupt requested.
    pub fn irq_line(&self) -> bool {
        self.io[0x208] & 1 != 0 && self.io16(0x200) & self.io16(0x202) & 0x3FFF != 0
    }

    pub fn irq_requested(&self) -> bool {
        self.io16(0x200) & self.io16(0x202) & 0x3FFF != 0
    }

    // ------------------------------------------------------------ timing

    /// Cycles of one 16-bit cartridge read in wait-state area `ws` (0 to 2), from WAITCNT.
    fn rom16(&self, ws: u32, seq: bool) -> u32 {
        let wc = u32::from(self.io16(0x204));
        if seq {
            if wc & [0x10, 0x80, 0x400][ws as usize] != 0 { 2 } else { [3, 5, 9][ws as usize] }
        } else {
            [5, 4, 3, 9][((wc >> (2 + ws * 3)) & 3) as usize]
        }
    }

    /// Cycles one access takes: 32 bits wide or not, sequential (following on from the one
    /// before, which only the cartridge cares about) or not.
    fn cost(&self, addr: u32, word: bool, seq: bool) -> u32 {
        match addr >> 24 {
            0x02 => {
                // 15 minus the setting is the number of wait states (15 would lock the console up).
                let w = 16 - ((self.tm.memcnt >> 24) & 15).min(14);
                if word { 2 * w } else { w }
            }
            0x05 | 0x06 => if word { 2 } else { 1 },
            0x08..=0x0D => {
                // The cartridge bus is 16 bits wide: a word is a halfword and the one after it.
                let ws = (addr >> 25) - 4;
                let first = self.rom16(ws, seq);
                if word { first + self.rom16(ws, true) } else { first }
            }
            0x0E | 0x0F => [5, 4, 3, 9][(self.io16(0x204) & 3) as usize],
            _ => 1,
        }
    }

    /// Lets `n` cycles pass with the cartridge bus free: the prefetch buffer reads ahead in them.
    fn spend(&mut self, n: u32) {
        self.pending += n;
        self.tm.prefetch.tick(n);
    }

    /// The first cycle of a cartridge access is still one the prefetch unit has to itself (it
    /// gives the bus up at the end of it). Says whether a halfword it was reading arrived in
    /// that cycle, which holds the bus one cycle longer.
    fn cart_first_cycle(&mut self) -> bool {
        self.tm.prefetch.tick(1);
        self.tm.prefetch.arrived
    }

    /// A data read or write (by the CPU or by DMA) spends its cycles. `seq` is the access
    /// following on from the one before: the second word on of a block transfer or a DMA.
    fn data(&mut self, addr: u32, word: bool, seq: bool) {
        if !self.in_dma {
            self.dma_catch_up();
        }
        let seq = seq && addr & 0x1_FFFF != 0;
        let mut c = self.cost(addr, word, seq);
        if (0x08..=0x0F).contains(&(addr >> 24)) {
            // The cartridge or its save chip: whatever was being read ahead is thrown away.
            c += u32::from(self.cart_first_cycle());
            let p = &mut self.tm.prefetch;
            p.done = 0;
            p.check = 0;
            self.pending += c;
        } else {
            self.spend(c);
        }
        #[cfg(feature = "gba_trace")]
        eprintln!("  {:6} data {addr:08X} {} {} +{c}", self.now(), if word { 32 } else { 16 }, if seq { "S" } else { "N" });
        self.tm.after_code = false;
        if !self.in_dma {
            self.tm.bus_end = self.now();
        }
    }

    /// Runs a DMA switched on to start straight away once its start-up time has passed: when
    /// the CPU next wants the bus. If it came due while the CPU was only busy inside itself,
    /// it ran alongside those cycles, so only what is left of it holds the CPU up.
    fn dma_catch_up(&mut self) {
        let now = self.now();
        for ch in 0..4 {
            let due = self.tm.dma_due[ch];
            if due > now {
                continue;
            }
            self.tm.dma_due[ch] = u64::MAX;
            if self.io16(0xBA + ch * 12) & 0x8000 == 0 || !self.dma[ch].active {
                continue;
            }
            let start = due.max(self.tm.bus_end);
            let before = self.pending;
            self.run_dma(ch);
            let took = self.pending - before;
            let overlap = (now.saturating_sub(start)).min(u64::from(took)) as u32;
            self.pending -= overlap;
        }
    }

    /// An opcode read from the cartridge, from the prefetch buffer when it can. Returns the
    /// cycles it takes, its first cycle included.
    fn cart_code(&mut self, addr: u32, thumb: bool, seq: bool) -> u32 {
        let glitch = u32::from(self.cart_first_cycle());
        let ws = (addr >> 25) - 4;
        let (n16, s16) = (self.rom16(ws, false), self.rom16(ws, true));
        let p = &mut self.tm.prefetch;
        #[cfg(feature = "gba_trace")]
        eprintln!("  {:6} code {addr:08X} {} pf run={} read={:08X} check={:08X} held={} done={}/{} full={} fol={} glitch={glitch}", self.cycles + u64::from(self.pending), if seq { "S" } else { "N" }, p.run, p.read, p.check, p.held, p.done, p.wait, p.full, p.following);
        let (step, edge) = if thumb { (2, 0x1_FFFE) } else { (4, 0x1_FFFC) };
        if p.check != 0 && addr & !(step - 1) == p.check {
            if p.check != p.read && p.held > 0 {
                p.check = p.check.wrapping_add(2);
                p.held -= 1;
                if thumb || (p.check != p.read && p.held > 0) {
                    if !thumb {
                        p.check = p.check.wrapping_add(2);
                        p.held -= 1;
                    }
                    if p.held == 0 {
                        p.emptied();
                    }
                    return 1;
                }
                // An ARM opcode whose second half is still on its way.
                let wait = p.wait - p.done + 1;
                p.held = 0;
                p.taken_over(2);
                p.run = p.enabled;
                return wait;
            }
            if addr & edge == 0 {
                // A 128 KiB boundary: the CPU reads it itself, non-sequentially.
                p.taken_over(step);
                return if thumb { n16 } else { n16 + s16 } + glitch;
            }
            // On its way: wait for the rest of that read, unless this is a non-sequential read
            // in the very first cycle of the first read since it last started again.
            let first = if !seq && p.done == 1 && !p.following { n16 } else { p.wait - p.done + 1 };
            p.taken_over(step);
            if !thumb {
                p.run = p.enabled;
            }
            return if thumb { first } else { first + s16 };
        }
        // Nothing it can hand out: the CPU reads the cartridge itself, and reading ahead starts
        // again after it.
        let seq = seq && !p.force_n && addr & edge != 0;
        p.force_n = false;
        let c = match (thumb, seq) {
            (true, true) => s16,
            (true, false) => n16,
            (false, true) => 2 * s16,
            (false, false) => n16 + s16,
        } + glitch;
        let enabled = p.enabled;
        *p = Prefetch {
            run: enabled,
            read: if enabled { addr.wrapping_add(step) } else { 0 },
            check: if enabled { addr.wrapping_add(step) } else { 0 },
            wait: s16,
            enabled,
            ..Prefetch::default()
        };
        c
    }

    /// Cycles run so far, counting those spent but not yet run through the machine.
    pub fn now(&self) -> u64 {
        self.cycles + u64::from(self.pending)
    }

    /// Cycles left in the current scanline.
    pub fn cycles_to_line_end(&self) -> u32 {
        LINE.saturating_sub(self.line_cycle).max(1)
    }

    /// Internal CPU cycles: the bus is free (the prefetch buffer reads ahead), and the opcode
    /// read after them is non-sequential.
    pub fn idle(&mut self, n: u32) {
        #[cfg(feature = "gba_trace")]
        eprintln!("  {:6} idle +{n}", self.now());
        self.spend(n);
        self.tm.after_code = false;
    }

    /// An opcode read at `addr`, Thumb or ARM. `seq` is the CPU saying it follows on from the
    /// opcode before (false for the first read after a jump); it only does when the bus cycle
    /// before was an opcode read too.
    pub fn code(&mut self, addr: u32, thumb: bool, seq: bool) -> u32 {
        self.dma_catch_up();
        let seq = seq && self.tm.after_code;
        self.exec_pc = addr;
        self.tm.thumb = thumb;
        if (0x08..=0x0D).contains(&(addr >> 24)) {
            let c = self.cart_code(addr, thumb, seq);
            self.pending += c;
        } else {
            let c = self.cost(addr, !thumb, seq);
            self.spend(c);
        }
        self.tm.after_code = true;
        self.tm.bus_end = self.now();
        self.run_pending();
        if thumb {
            let v = self.peek16(addr);
            // What an unmapped read returns afterwards: the last opcode read, arranged by where
            // the code runs from (two halves of the same opcode for most of the memory map).
            let other = |s: &Self, a: u32| s.peek16(a);
            self.open_bus = match addr >> 24 {
                0x00 | 0x07 if addr & 2 == 0 => v | other(self, addr + 2) << 16,
                0x00 | 0x07 => other(self, addr.wrapping_sub(2)) | v << 16,
                0x03 if addr & 2 == 0 => v | other(self, addr.wrapping_sub(2)) << 16,
                0x03 => other(self, addr.wrapping_sub(2)) | v << 16,
                _ => v | v << 16,
            };
            v
        } else {
            let v = self.peek32(addr);
            self.open_bus = v;
            if addr < 0x4000 {
                // What a BIOS read from outside returns: the last opcode the BIOS read.
                self.bios_latch = v;
            }
            v
        }
    }

    // ------------------------------------------------------------ CPU access

    pub fn read8(&mut self, addr: u32) -> u32 {
        self.data(addr, false, false);
        self.run_pending();
        self.peek8(addr)
    }

    pub fn read16(&mut self, addr: u32) -> u32 {
        self.data(addr, false, false);
        self.run_pending();
        self.peek16(addr)
    }

    pub fn read32(&mut self, addr: u32) -> u32 {
        self.read32_seq(addr, false)
    }

    /// A word read; `seq` for the second word on of a block transfer.
    pub fn read32_seq(&mut self, addr: u32, seq: bool) -> u32 {
        self.data(addr, true, seq);
        self.run_pending();
        self.peek32(addr)
    }

    pub fn write8(&mut self, addr: u32, v: u8) {
        self.writes += 1;
        self.data(addr, false, false);
        self.run_pending();
        self.poke8(addr, v);
        self.run_pending();
    }

    pub fn write16(&mut self, addr: u32, v: u16) {
        self.writes += 1;
        self.data(addr, false, false);
        self.run_pending();
        if addr >> 25 == 7 {
            // The save chip's 8-bit bus takes the byte the address lands on.
            self.save.write8(addr, (v >> ((addr & 1) * 8)) as u8);
            return;
        }
        self.poke16(addr & !1, v);
        self.run_pending();
    }

    pub fn write32(&mut self, addr: u32, v: u32) {
        self.write32_seq(addr, v, false);
    }

    /// A word write; `seq` for the second word on of a block transfer.
    pub fn write32_seq(&mut self, addr: u32, v: u32, seq: bool) {
        self.writes += 1;
        self.data(addr, true, seq);
        self.run_pending();
        if addr >> 25 == 7 {
            self.save.write8(addr, (v >> ((addr & 3) * 8)) as u8);
            return;
        }
        self.poke16(addr & !3, v as u16);
        self.poke16((addr & !3) + 2, (v >> 16) as u16);
        self.run_pending();
    }

    fn vram_index(addr: u32) -> usize {
        let a = (addr & 0x1FFFF) as usize;
        if a >= 0x18000 { a - 0x8000 } else { a }
    }

    pub fn peek8(&self, addr: u32) -> u32 {
        if addr >> 24 == 0x0E {
            if let Some(v) = self.gpio.tilt_read(addr) {
                return u32::from(v);
            }
        }
        if addr >> 25 == 7 {
            return u32::from(self.save.read8(addr));
        }
        let h = self.peek16(addr & !1);
        if addr & 1 != 0 { h >> 8 } else { h & 0xFF }
    }

    pub fn peek16(&self, addr: u32) -> u32 {
        if addr >> 25 == 7 {
            return u32::from(self.save.read8(addr)) * 0x0101;
        }
        let addr = addr & !1;
        let r = |m: &[u8], i: usize| u32::from(m[i]) | u32::from(m[i + 1]) << 8;
        match addr >> 24 {
            0x00 => {
                if addr < 0x4000 {
                    if self.exec_pc < 0x4000 {
                        r(&self.bios, addr as usize)
                    } else {
                        (self.bios_latch >> ((addr & 2) * 8)) & 0xFFFF
                    }
                } else {
                    self.open_bus & 0xFFFF
                }
            }
            0x02 => r(&self.ewram, (addr & 0x3FFFF) as usize),
            0x03 => r(&self.iwram, (addr & 0x7FFF) as usize),
            0x04 => {
                let a = addr & 0xFF_FFFF;
                if a < 0x400 {
                    u32::from(self.io_read16(a as usize))
                } else if a & 0xFFFC == 0x800 {
                    // Internal memory control, mirrored every 64 KiB.
                    (self.tm.memcnt >> ((a & 2) * 8)) & 0xFFFF
                } else {
                    0
                }
            }
            0x05 => r(&self.pal, (addr & 0x3FF) as usize),
            0x06 => r(&self.vram, Self::vram_index(addr)),
            0x07 => r(&self.oam, (addr & 0x3FF) as usize),
            0x08..=0x0D => {
                if self.save.kind == SaveKind::Eeprom && addr >> 24 == 0x0D && self.rom.len() <= 0x100_0000 {
                    return 1;
                }
                if (0x0800_00C4..=0x0800_00C9).contains(&addr) {
                    if let Some(v) = self.gpio.read(addr) {
                        return u32::from(v);
                    }
                }
                let a = (addr & 0x1FF_FFFF) as usize;
                if a + 1 < self.rom.len() {
                    r(&self.rom, a)
                } else {
                    (addr >> 1) & 0xFFFF
                }
            }
            0x0E | 0x0F => u32::from(self.save.read8(addr)) * 0x0101,
            _ => self.open_bus & 0xFFFF,
        }
    }

    pub fn peek32(&self, addr: u32) -> u32 {
        let a = addr & !3;
        if a >> 24 == 0x0E || a >> 24 == 0x0F {
            return u32::from(self.save.read8(addr)) * 0x0101_0101;
        }
        self.peek16(a) | self.peek16(a + 2) << 16
    }

    fn poke8(&mut self, addr: u32, v: u8) {
        match addr >> 24 {
            0x02 => self.ewram[(addr & 0x3FFFF) as usize] = v,
            0x03 => self.iwram[(addr & 0x7FFF) as usize] = v,
            0x04 => {
                let a = (addr & 0xFF_FFFF) as usize;
                if a < 0x400 {
                    self.io_write8(a, v);
                } else if a & 0xFFFC == 0x800 {
                    let sh = (a & 3) * 8;
                    self.tm.memcnt = (self.tm.memcnt & !(0xFF << sh)) | u32::from(v) << sh;
                }
            }
            // Byte writes to palette and background VRAM write the byte to both halves;
            // to OBJ VRAM and OAM they are ignored.
            0x05 => {
                let a = (addr & 0x3FE) as usize;
                self.pal[a] = v;
                self.pal[a + 1] = v;
            }
            0x06 => {
                let a = Self::vram_index(addr) & !1;
                let limit = if self.io[0] & 7 >= 3 { 0x14000 } else { 0x10000 };
                if a < limit {
                    self.vram[a] = v;
                    self.vram[a + 1] = v;
                }
            }
            0x0E | 0x0F => {
                if !self.gpio.tilt_write(addr, v) {
                    self.save.write8(addr, v);
                }
            }
            _ => {}
        }
    }

    fn poke16(&mut self, addr: u32, v: u16) {
        let w = |m: &mut [u8], i: usize| {
            m[i] = v as u8;
            m[i + 1] = (v >> 8) as u8;
        };
        match addr >> 24 {
            0x02 => w(&mut self.ewram, (addr & 0x3FFFF) as usize),
            0x03 => w(&mut self.iwram, (addr & 0x7FFF) as usize),
            0x04 => {
                let a = (addr & 0xFF_FFFF) as usize;
                if a < 0x400 {
                    self.io_write16(a, v);
                } else if a & 0xFFFC == 0x800 {
                    let sh = (a & 2) * 8;
                    self.tm.memcnt = (self.tm.memcnt & !(0xFFFF << sh)) | u32::from(v) << sh;
                }
            }
            0x05 => w(&mut self.pal, (addr & 0x3FF) as usize),
            0x06 => w(&mut self.vram, Self::vram_index(addr)),
            0x07 => w(&mut self.oam, (addr & 0x3FF) as usize),
            0x08 if (0x0800_00C4..=0x0800_00C9).contains(&addr) => {
                // The clock reads the time on a write, so tell it how long the game has run.
                self.gpio.run_secs = self.cycles / CPU_HZ;
                self.gpio.write(addr, v);
            }
            0x0D if self.save.kind == SaveKind::Eeprom => self.save.eeprom_write(v),
            0x0E | 0x0F => self.save.write8(addr, (v >> ((addr & 1) * 8)) as u8),
            _ => {}
        }
    }

    // ------------------------------------------------------------ I/O

    /// Game Boy sound register for a GBA sound address, if any.
    fn psg_reg(a: usize) -> Option<u16> {
        Some(match a {
            0x60 => 0xFF10, 0x62 => 0xFF11, 0x63 => 0xFF12, 0x64 => 0xFF13, 0x65 => 0xFF14,
            0x68 => 0xFF16, 0x69 => 0xFF17, 0x6C => 0xFF18, 0x6D => 0xFF19,
            0x70 => 0xFF1A, 0x72 => 0xFF1B, 0x73 => 0xFF1C, 0x74 => 0xFF1D, 0x75 => 0xFF1E,
            0x78 => 0xFF20, 0x79 => 0xFF21, 0x7C => 0xFF22, 0x7D => 0xFF23,
            0x80 => 0xFF24, 0x81 => 0xFF25, 0x84 => 0xFF26,
            0x90..=0x9F => 0xFF30 + (a - 0x90) as u16,
            _ => return None,
        })
    }

    fn io_read8(&self, a: usize) -> u8 {
        if let Some(r) = Self::psg_reg(a) {
            return self.apu.read(r);
        }
        if (0x60..0xA0).contains(&a) {
            return self.io[a];
        }
        match a {
            0x04..=0x07 | 0x100..=0x10F | 0x128 | 0x129 | 0x130 | 0x131 | 0x200..=0x203 => {
                let h = self.io_read16(a & !1);
                (if a & 1 != 0 { h >> 8 } else { h }) as u8
            }
            _ => self.io[a],
        }
    }

    fn io_read16(&self, a: usize) -> u16 {
        match a {
            0x04 => {
                let mut v = self.io16(4) & 0xFF38;
                if (160..227).contains(&self.line) { v |= 1 }
                if self.line_cycle >= HBLANK_AT { v |= 2 }
                if self.line == u32::from(self.io[5]) { v |= 4 }
                v
            }
            0x06 => self.line as u16,
            0x100 | 0x104 | 0x108 | 0x10C => self.timer_count((a - 0x100) / 4, self.cycles),
            0x102 | 0x106 | 0x10A | 0x10E => self.timers[(a - 0x100) / 4].ctrl,
            0x128 => self.siocnt_read(),
            0x130 => 0x3FF & !self.keys,
            0x60..=0x9F => u16::from(self.io_read8(a)) | u16::from(self.io_read8(a + 1)) << 8,
            // Write-only registers read as 0.
            0x10..=0x1F | 0x20..=0x3F | 0x40..=0x47 | 0x4C..=0x4F | 0x54..=0x5F | 0xA0..=0xB9 => 0,
            0xBC..=0xC5 | 0xC8..=0xD1 | 0xD4..=0xDD => 0,
            0xBA | 0xC6 | 0xD2 | 0xDE => self.io16(a),
            _ => self.io16(a),
        }
    }

    fn io_write8(&mut self, a: usize, v: u8) {
        match a {
            0xA0..=0xA3 => self.fifo_push(0, v as i8),
            0xA4..=0xA7 => self.fifo_push(1, v as i8),
            0x202 | 0x203 => self.io[a] &= !v,
            0x301 => self.halted = true,
            _ => {
                if let Some(r) = Self::psg_reg(a) {
                    self.apu.write(r, v);
                    self.io[a] = v;
                    return;
                }
                let h = a & !1;
                let mut cur = self.io16(h);
                if a & 1 != 0 {
                    cur = (cur & 0x00FF) | u16::from(v) << 8;
                } else {
                    cur = (cur & 0xFF00) | u16::from(v);
                }
                self.io_write16(h, cur);
            }
        }
    }

    pub(super) fn io_write16(&mut self, a: usize, v: u16) {
        match a {
            0x04 => {
                let cur = self.io16(4);
                self.set_io16(4, (cur & 7) | (v & 0xFF38));
            }
            0x06 => {}
            0x28..=0x2F | 0x38..=0x3F => {
                self.set_io16(a, v);
                self.ppu.reload_affine(&self.io);
            }
            0x82 => {
                self.set_io16(a, v);
                if v & 0x0800 != 0 {
                    self.fifo[0].data.clear();
                }
                if v & 0x8000 != 0 {
                    self.fifo[1].data.clear();
                }
            }
            0xA0..=0xA7 => {
                self.io_write8(a, v as u8);
                self.io_write8(a + 1, (v >> 8) as u8);
            }
            0x60..=0x9F => {
                for (i, b) in [(a, v as u8), (a + 1, (v >> 8) as u8)] {
                    if let Some(r) = Self::psg_reg(i) {
                        self.apu.write(r, b);
                    }
                    self.io[i] = b;
                }
            }
            0xBA | 0xC6 | 0xD2 | 0xDE => {
                let ch = (a - 0xBA) / 12;
                let was = self.io16(a) & 0x8000 != 0;
                self.set_io16(a, v);
                if !was && v & 0x8000 != 0 {
                    self.dma_enable(ch);
                }
                if v & 0x8000 == 0 {
                    self.tm.dma_due[ch] = u64::MAX;
                }
            }
            0x100 | 0x104 | 0x108 | 0x10C => self.timers[(a - 0x100) / 4].reload = v,
            0x102 | 0x106 | 0x10A | 0x10E => self.timer_control((a - 0x100) / 4, v),
            0x204 => {
                // Bit 13 is unused and bit 15 says it is an Advance (0), read-only.
                self.set_io16(0x204, v & 0x5FFF);
                let thumb = self.tm.thumb;
                let p = &mut self.tm.prefetch;
                let on = v & 0x4000 != 0;
                if !p.enabled && on {
                    p.check = 0;
                    p.held = 0;
                    p.done = 0;
                    p.run = true;
                }
                if p.enabled && !on {
                    // Switched off: a read in progress is finished (and in ARM, a lone half of
                    // an opcode gets its other half), what is held can still be used, and the
                    // next opcode read it cannot hand out is non-sequential.
                    p.force_n = true;
                    if p.done == 0 {
                        if p.held & 1 == 0 || thumb {
                            p.run = false;
                        }
                        if p.held == 0 {
                            p.check = 0;
                        }
                    }
                }
                p.enabled = on;
            }
            0x128 => {
                // The busy bit of a transfer still running is the port's, not the write's.
                let busy = if self.sio_busy() { 0x80 } else { 0 };
                self.set_io16(a, v | busy);
                if self.gbp && v & 0x80 != 0 && (v >> 12) & 3 == 1 && v & 1 == 0 {
                    self.gbp_clock();
                } else if v & 0x80 != 0 && busy == 0 {
                    self.sio_start();
                }
            }
            0x130 => {}
            0x134 => {
                self.set_io16(a, v);
                // General-purpose mode with SD an output: high then low resets the adapter,
                // which is how a game looks for one (gba-link-connection's pingAdapter).
                if v & 0xC000 == 0x8000 && v & 0x20 != 0 {
                    let high = v & 0x02 != 0;
                    if self.rfu_sd_high && !high && self.rfu_allowed {
                        self.rfu.reset();
                    }
                    self.rfu_sd_high = high;
                }
            }
            0x202 => {
                let cur = self.io16(0x202);
                self.set_io16(0x202, cur & !v);
            }
            0x300 => {
                self.io[0x300] = v as u8;
                if v & 0x8000 == 0 {
                    // HALTCNT in the high byte; a 16-bit write to $300 halts too.
                }
                self.halted = true;
            }
            _ => self.set_io16(a, v),
        }
    }

    // ------------------------------------------------------------ DMA

    fn dma_enable(&mut self, ch: usize) {
        let base = 0xB0 + ch * 12;
        let src = u32::from(self.io16(base)) | u32::from(self.io16(base + 2)) << 16;
        let dst = u32::from(self.io16(base + 4)) | u32::from(self.io16(base + 6)) << 16;
        let d = &mut self.dma[ch];
        d.src = src & if ch == 0 { 0x07FF_FFFF } else { 0x0FFF_FFFF };
        d.dst = dst & if ch == 3 { 0x0FFF_FFFF } else { 0x07FF_FFFF };
        d.active = true;
        self.dma_reload_count(ch);
        if (self.io16(base + 10) >> 12) & 3 == 0 {
            // It starts two cycles after the write that switched it on has ended.
            self.tm.dma_due[ch] = self.cycles + 2;
        }
    }

    fn dma_reload_count(&mut self, ch: usize) {
        let base = 0xB0 + ch * 12;
        let c = u32::from(self.io16(base + 8));
        let max = if ch == 3 { 0x10000 } else { 0x4000 };
        self.dma[ch].count = if c == 0 { max } else { c & (max - 1) }.max(1);
        if c != 0 && c & (max - 1) == 0 {
            self.dma[ch].count = max;
        }
    }

    fn trigger_dma(&mut self, timing: u16) {
        for ch in 0..4 {
            let cnt = self.io16(0xBA + ch * 12);
            if cnt & 0x8000 != 0 && (cnt >> 12) & 3 == timing && self.dma[ch].active {
                if timing == 3 && (ch == 1 || ch == 2) {
                    continue; // sound DMA is triggered by its FIFO
                }
                self.run_dma(ch);
            }
        }
    }

    fn run_dma(&mut self, ch: usize) {
        let cntaddr = 0xBA + ch * 12;
        let cnt = self.io16(cntaddr);
        let fifo = (cnt >> 12) & 3 == 3 && (ch == 1 || ch == 2);
        let word = cnt & 0x400 != 0 || fifo;
        let step: u32 = if word { 4 } else { 2 };
        let dctl = if fifo { 2 } else { (cnt >> 5) & 3 };
        let sctl = (cnt >> 7) & 3;
        let count = if fifo { 4 } else { self.dma[ch].count };
        if self.dma[ch].dst >> 24 == 0x0D {
            self.save.eeprom_dma(count);
        }
        let was = self.in_dma;
        self.in_dma = true;
        let (mut s, mut d) = (self.dma[ch].src, self.dma[ch].dst);
        // Two internal cycles (four when both ends are on the cartridge), half before the
        // transfer and half after it; the first unit is a non-sequential read and write, and
        // every one after it sequential.
        let cart = |a: u32| (0x08..=0x0F).contains(&(a >> 24));
        let internal = if cart(s) && cart(d) { 2 } else { 1 };
        self.idle(internal);
        for i in 0..count {
            let seq = i > 0;
            if word {
                self.data(s & !3, true, seq);
                let v = self.peek32(s & !3);
                self.data(d & !3, true, seq);
                self.poke16(d & !3, v as u16);
                self.poke16((d & !3) + 2, (v >> 16) as u16);
            } else {
                self.data(s & !1, false, seq);
                let v = self.peek16(s & !1);
                self.data(d & !1, false, seq);
                self.poke16(d & !1, v as u16);
            }
            s = match sctl { 0 | 3 => s.wrapping_add(step), 1 => s.wrapping_sub(step), _ => s };
            d = match dctl { 0 | 3 => d.wrapping_add(step), 1 => d.wrapping_sub(step), _ => d };
        }
        self.idle(internal);
        self.in_dma = was;
        self.dma[ch].src = s;
        self.dma[ch].dst = d;
        if cnt & 0x4000 != 0 {
            self.request_irq(IRQ_DMA0 << ch);
        }
        let timing = (cnt >> 12) & 3;
        if cnt & 0x200 != 0 && timing != 0 {
            if !fifo {
                self.dma_reload_count(ch);
            }
            if dctl == 3 {
                let base = 0xB0 + ch * 12;
                let dst = u32::from(self.io16(base + 4)) | u32::from(self.io16(base + 6)) << 16;
                self.dma[ch].dst = dst & if ch == 3 { 0x0FFF_FFFF } else { 0x07FF_FFFF };
            }
        } else {
            self.dma[ch].active = false;
            self.set_io16(cntaddr, cnt & 0x7FFF);
        }
    }

    // ------------------------------------------------------------ sound

    fn fifo_push(&mut self, n: usize, v: i8) {
        let f = &mut self.fifo[n];
        if f.data.len() < 32 {
            f.data.push(v);
        }
    }

    fn gbp_clock(&mut self) {
        // Words the Game Boy Player shifts in. The game answers, and 0x40000026 means the motor is on.
        const WORDS: [u32; 19] = [
            0x0000_494E, 0xB6B1_494E, 0xB6B1_494E, 0xB6B1_544E, 0xABB1_544E, 0xABB1_4E45, 0xB1BA_4E45,
            0xB1BA_4F44, 0xB0BB_4F44, 0xB0BB_8002, 0x1000_0010, 0x2000_0013, 0x4000_0004, 0x3000_0003,
            0x3000_0003, 0x3000_0003, 0x3000_0003, 0x3000_0003, 0x3000_0003,
        ];
        let sent = u32::from(self.io16(0x120)) | u32::from(self.io16(0x122)) << 16;
        if sent & 0xFFFF_FF00 == 0x4000_0000 {
            self.gbp_motor = sent & 0xFF == 0x26;
        }
        let word = WORDS[usize::from(self.gbp_i) % WORDS.len()];
        self.gbp_i = self.gbp_i.wrapping_add(1);
        self.set_io16(0x120, word as u16);
        self.set_io16(0x122, (word >> 16) as u16);
        let cnt = self.io16(0x128);
        self.set_io16(0x128, cnt & !0x80);
        if cnt & 0x4000 != 0 {
            let iflag = self.io16(0x202);
            self.set_io16(0x202, iflag | 0x0080);
        }
    }

    /// Reads the game's music engine out of the machine's memory, ready to be played again at the
    /// output rate. Lifted out of the bus for a moment so it can read the machine it lives in.
    pub fn mp2k_sync(&mut self) {
        let mut m = std::mem::take(&mut self.mp2k);
        m.sync(self, self.apu.out_hz);
        self.mp2k = m;
    }

    fn timer_overflow_sound(&mut self, timer: usize) {
        let cnth = self.io16(0x82);
        for n in 0..2 {
            let sel = (cnth >> (10 + n * 4)) & 1;
            if sel as usize != timer {
                continue;
            }
            let f = &mut self.fifo[n];
            if !f.data.is_empty() {
                let sample = f.data.remove(0);
                f.current = sample;
                let arrived = i32::from(sample);
                self.fifo_from[n] = self.fifo_to[n];
                self.fifo_to[n] = arrived;
                self.fifo_period[n] = self.fifo_age[n].max(1);
                self.fifo_age[n] = 0;
            }
            if self.fifo[n].data.len() <= 16 {
                let want = 0x0400_00A0 + 4 * n as u32;
                for ch in 1..3 {
                    let cnt = self.io16(0xBA + ch * 12);
                    if cnt & 0x8000 != 0 && (cnt >> 12) & 3 == 3 && self.dma[ch].dst == want {
                        self.run_dma(ch);
                    }
                }
            }
        }
    }

    fn sound(&mut self, cycles: u32) {
        // The PSG runs at the Game Boy clock (a quarter of the GBA's); its frame sequencer is
        // clocked at 512 Hz.
        // The sequencer's clock rises half way through each period and falls at its end.
        let before = self.seq_acc;
        self.seq_acc += cycles;
        if before < 16384 && self.seq_acc >= 16384 {
            self.apu.frame_rise();
        }
        while self.seq_acc >= 32768 {
            self.seq_acc -= 32768;
            self.apu.frame_step();
            if self.seq_acc >= 16384 {
                self.apu.frame_rise();
            }
        }
        self.gb_acc += cycles;
        let t = self.gb_acc / 4;
        if t == 0 {
            return;
        }
        self.gb_acc -= t * 4;
        let before = self.apu.samples.len();
        self.apu.tick(t);
        let cnth = self.io16(0x82);
        let psg_shift = [2, 1, 0, 0][(cnth & 3) as usize];
        let hq = self.mp2k.live;
        let mut i = before;
        while i + 1 < self.apu.samples.len() {
            if hq {
                self.mp2k.step();
            }
            let mut l = i32::from(self.apu.samples[i]) >> psg_shift;
            let mut r = i32::from(self.apu.samples[i + 1]) >> psg_shift;
            for n in 0..2 {
                let full = cnth & (4 << n) != 0;
                // The game's own music engine, mixed again at the output rate, in place of what
                // the engine itself squeezed into this channel. Only where it is really the
                // engine feeding it, and only with the setting on High.
                if let Some(hq) = if hq { self.mp2k.lane(n) } else { None } {
                    let mut v = hq * if full { 128 } else { 64 };
                    v = v * i32::from(self.fifo_vol[n]) / 100;
                    if cnth & (0x100 << (n * 4)) != 0 { r += v }
                    if cnth & (0x200 << (n * 4)) != 0 { l += v }
                    continue;
                }
                let mut v = if self.fifo_smooth {
                    let span = self.fifo_period[n].max(1) as i32;
                    let along = (self.fifo_age[n] as i32).min(span);
                    let blended = self.fifo_from[n] + (self.fifo_to[n] - self.fifo_from[n]) * along / span;
                    self.fifo_age[n] = self.fifo_age[n].saturating_add(1);
                    blended * if full { 128 } else { 64 }
                } else {
                    i32::from(self.fifo[n].current) * if full { 128 } else { 64 }
                };
                v = v * i32::from(self.fifo_vol[n]) / 100;
                if cnth & (0x100 << (n * 4)) != 0 { r += v }
                if cnth & (0x200 << (n * 4)) != 0 { l += v }
            }
            self.apu.samples[i] = l.clamp(-32768, 32767) as i16;
            self.apu.samples[i + 1] = r.clamp(-32768, 32767) as i16;
            i += 2;
        }
    }

    // ------------------------------------------------------------ the clock

    fn timer_shift(ctrl: u16) -> u32 {
        [0, 6, 8, 10][(ctrl & 3) as usize]
    }

    /// A timer that is on and counts cycles (not another timer's overflows).
    fn timer_free(&self, i: usize) -> bool {
        let c = self.timers[i].ctrl;
        c & 0x80 != 0 && (i == 0 || c & 4 == 0)
    }

    /// What timer `i` reads at cycle `now`. A running timer's `counter` is its count at
    /// `timer_at`, and every overflow up to the present has already been run.
    fn timer_count(&self, i: usize, now: u64) -> u16 {
        let t = &self.timers[i];
        let at = self.tm.timer_at[i];
        if !self.timer_free(i) || now <= at {
            return t.counter;
        }
        let n = (now - at) >> Self::timer_shift(t.ctrl);
        (u64::from(t.counter) + n).min(0xFFFF) as u16
    }

    /// The cycle a counting timer next overflows.
    fn timer_overflow_at(&self, i: usize) -> u64 {
        let t = &self.timers[i];
        self.tm.timer_at[i] + ((0x1_0000 - u64::from(t.counter)) << Self::timer_shift(t.ctrl))
    }

    /// A write to a timer's control register. It lands a cycle after the write; a timer just
    /// switched on loads its reload value and counts its first tick on the prescaler's next
    /// step from the cycle after that (the prescaler is one free-running divider all four
    /// timers share, so a slow timer's first tick can come early).
    fn timer_control(&mut self, i: usize, v: u16) {
        let now = self.cycles + 1;
        let was_on = self.timers[i].ctrl & 0x80 != 0;
        if self.timer_free(i) {
            self.timers[i].counter = self.timer_count(i, now);
        }
        let ctrl = v & 0xC7;
        self.timers[i].ctrl = ctrl;
        if ctrl & 0x80 == 0 {
            return;
        }
        let step = (1u64 << Self::timer_shift(ctrl)) - 1;
        if !was_on {
            if self.timers[i].counter == 0xFFFF && now & step == 0 && (i == 0 || ctrl & 4 == 0) {
                // Loading the reload value takes a cycle, and a timer stopped at $FFFF can tick
                // in it: it overflows (interrupt and all) before it starts from the reload value.
                self.tm.timer_at[i] = now;
                return;
            }
            self.timers[i].counter = self.timers[i].reload;
            self.tm.timer_at[i] = (now & !step) + 1;
        } else {
            self.tm.timer_at[i] = now & !step;
        }
    }

    /// Runs every timer overflow due by now, earliest first.
    fn run_timers(&mut self) {
        loop {
            let mut next: Option<(u64, usize)> = None;
            for i in 0..4 {
                if self.timer_free(i) {
                    let at = self.timer_overflow_at(i);
                    if at <= self.cycles && next.is_none_or(|(n, _)| at < n) {
                        next = Some((at, i));
                    }
                }
            }
            let Some((at, i)) = next else { break };
            self.timer_overflow(i, at);
        }
    }

    fn timer_overflow(&mut self, i: usize, at: u64) {
        self.timers[i].counter = self.timers[i].reload;
        self.tm.timer_at[i] = at;
        if self.timers[i].ctrl & 0x40 != 0 {
            self.request_irq(IRQ_TIMER0 << i);
        }
        if i < 2 {
            self.timer_overflow_sound(i);
        }
        if i < 3 && self.timers[i + 1].ctrl & 0x84 == 0x84 {
            if self.timers[i + 1].counter == 0xFFFF {
                self.timer_overflow(i + 1, at);
            } else {
                self.timers[i + 1].counter += 1;
            }
        }
    }

    /// Cycles until the next timer overflow that raises an interrupt, if any.
    fn cycles_to_timer_irq(&self) -> Option<u64> {
        (0..4)
            .filter(|&i| self.timer_free(i) && self.timers[i].ctrl & 0x40 != 0)
            .map(|i| self.timer_overflow_at(i).saturating_sub(self.cycles))
            .min()
    }

    pub fn run_pending(&mut self) {
        if self.in_dma {
            return;
        }
        while self.pending > 0 {
            let n = self.pending;
            self.pending = 0;
            self.advance(n);
        }
    }

    /// Runs the rest of the machine for `n` cycles.
    fn advance(&mut self, mut n: u32) {
        self.cycles += u64::from(n);
        self.run_timers();
        self.sound(n);
        while n > 0 {
            let boundary = if self.line_cycle < HBLANK_AT { HBLANK_AT } else { LINE };
            let step = n.min(boundary - self.line_cycle);
            self.line_cycle += step;
            n -= step;
            if self.line_cycle == HBLANK_AT {
                self.hblank();
            } else if self.line_cycle == LINE {
                self.line_cycle = 0;
                self.next_line();
            }
        }
    }

    fn hblank(&mut self) {
        if self.line < 160 {
            let (io, vram, pal, oam) = (&self.io, &self.vram, &self.pal, &self.oam);
            if !self.ppu.no_render {
                self.ppu.render_line(io, vram, pal, oam, self.line as usize);
            }
        }
        if self.io16(4) & 0x10 != 0 {
            self.request_irq(IRQ_HBLANK);
        }
        if self.line < 160 {
            self.trigger_dma(2);
        }
    }

    fn next_line(&mut self) {
        self.sio_line();
        self.line += 1;
        if self.line == 228 {
            self.line = 0;
            // The start of a frame, by which time a game has done its once-a-frame sound work:
            // the music engine's own notes are at their freshest here.
            if self.mp2k.on {
                self.mp2k_sync();
            }
        }
        let stat = self.io16(4);
        if self.line == 160 {
            self.frame_ready = true;
            self.ppu.reload_affine(&self.io);
            if stat & 0x08 != 0 {
                self.request_irq(IRQ_VBLANK);
            }
            self.trigger_dma(1);
        }
        if self.line == u32::from(self.io[5]) && stat & 0x20 != 0 {
            self.request_irq(IRQ_VCOUNT);
        }
        // Video-capture DMA (DMA3 special timing) runs on lines 2-161.
        if (2..162).contains(&self.line) {
            let cnt = self.io16(0xDE);
            if cnt & 0x8000 != 0 && (cnt >> 12) & 3 == 3 && self.dma[3].active {
                self.run_dma(3);
            }
        }
    }

    /// While halted: jump to the next display event (timers and sound run in step).
    pub fn halt_skip(&mut self) {
        self.dma_catch_up();
        self.run_pending();
        let mut to = if self.line_cycle < HBLANK_AT { HBLANK_AT - self.line_cycle } else { LINE - self.line_cycle };
        if let Some(t) = self.cycles_to_timer_irq() {
            to = to.min(t.min(256) as u32);
        }
        self.pending += to.clamp(1, 256);
        self.run_pending();
    }

    pub fn keypad_irq(&mut self) {
        let cnt = self.io16(0x132);
        if cnt & 0x4000 == 0 {
            return;
        }
        let want = cnt & 0x3FF;
        let hit = if cnt & 0x8000 != 0 { self.keys & want == want } else { self.keys & want != 0 };
        if hit {
            self.request_irq(IRQ_KEYPAD);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_boy_player_handshake_sets_the_motor() {
        let mut bus = Bus::new(vec![0u8; 0x200]);
        bus.gbp = true;
        bus.write32(0x0400_0120, 0);
        bus.write16(0x0400_0128, 0x5080);
        assert_eq!(bus.read32(0x0400_0120), 0x0000_494E, "first word from the Player");
        assert_eq!(bus.read16(0x0400_0202) & 0x80, 0x80, "serial interrupt");
        assert_eq!(bus.read16(0x0400_0128) & 0x80, 0, "the transfer finishes");
        bus.write32(0x0400_0120, 0x4000_0026);
        bus.write16(0x0400_0128, 0x5080);
        assert!(bus.gbp_motor);
        bus.write32(0x0400_0120, 0x4000_0004);
        bus.write16(0x0400_0128, 0x5080);
        assert!(!bus.gbp_motor);
    }
}
