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
use super::ppu::W;
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
    /// Units still to move when a higher priority channel cut in, whether the next one
    /// follows on from the one before it (it does: the interrupted channel picks up where it
    /// stopped, as on the console), and, when the cut came between a unit's read and its
    /// write, the value already read and the address it still has to be written to.
    #[serde(skip)]
    pending_channel: Option<(u32, bool)>,
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
    #[inline(always)]
    fn tick(&mut self, n: u32) {
        self.arrived = false;
        if !self.run || self.check == 0 || self.full {
            return;
        }
        self.tick_run(n);
    }

    #[inline(never)]
    fn tick_run(&mut self, mut n: u32) {
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
    // The BIOS's own wait, at $70: a processor that halts waits here for the interrupt, and the
    // handler's return comes back into it (which is why the real one's return address is $74).
    // It has to be a loop that can be left, not `B .`: the return address the handler works back
    // to lands on $74, so $74 has to jump back to $70 and $70 has to let the interrupt be taken.
    put(0x70, 0xEAFF_FFFE); // B $70 (the wait the handler subtracts four from)
    put(0x74, 0xEAFF_FFFB); // B $70 (what the handler actually returns to)
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
    /// Bytes written to the AGBEEG Aging Cartridge's CI register ($04999990), a single-byte
    /// MMIO register the ROM uses to report test results headlessly (null-terminated test
    /// name, null-terminated "PASS"/"FAIL", repeated, then null-terminated "DONE"). Not part
    /// of a save state: it is only a log for the test harness.
    #[serde(skip)]
    pub ci_log: Vec<u8>,
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
    /// Past-the-end cartridge reads follow GBATEK's stepping rule (`unsafe/unsafe` checks it)
    /// rather than the zero fill a fixed-size ROM array gives.
    #[serde(skip)]
    pub open_fill_stepping: bool,
    gb_acc: u32,
    seq_acc: u32,
    #[serde(skip)]
    pub frame_ready: bool,
    /// An interrupt is requested and IME is on, so the processor is on its way to the handler
    /// (the seven cycles of `IRQ_DELAY`), and the cycle it gets there. Not saved: it lasts
    /// seven cycles, so a state saved in the middle of it starts afresh.
    #[serde(skip)]
    irq_pending: bool,
    #[serde(skip)]
    irq_at: u64,
    /// A pending interrupt whose delay runs out inside the three cycles an IE, IF or IME
    /// write takes to land: the write does not cancel it, and the processor still enters
    /// the handler even though the switch is off by then. Cleared when it is taken.
    #[serde(skip)]
    pub irq_committed: bool,
    /// The cycle a write to IE or IME lands on (writes take effect three cycles later).
    #[serde(skip)]
    irq_write_at: u64,
    /// Mid-scanline span state: the line a span belongs to (-1: none open) and the next dot
    /// to render from on it. Not saved: a state made mid-line re-renders the line whole.
    #[serde(skip)]
    span_line: i32,
    #[serde(skip)]
    span_x: u32,
    /// The OAM snapshot taken at the previous line's HBlank: sprite evaluation is one line
    /// ahead, so a write to OAM during a line reaches the picture a line later. Empty until
    /// the first HBlank, when the live OAM is used instead. Not saved.
    #[serde(skip)]
    oam_snap: Vec<u8>,
    /// The background-enable bits (DISPCNT 8-11) as they stand for the line being drawn:
    /// enabling a background takes three lines to reach the picture, disabling one takes
    /// one. Not saved; re-synced from DISPCNT on load.
    #[serde(skip)]
    bg_bits: u16,
    /// Pending enable changes: (absolute line they take effect on, background 0-3, on).
    /// A later write to the same background replaces an earlier pending one. Not saved.
    #[serde(skip)]
    bg_queue: Vec<(u64, u8, bool)>,
    /// Absolute line counter (never wraps), so queued effects survive the 228-line wrap.
    #[serde(skip)]
    line_abs: u64,
    /// When each sound queue's refill is due (0 = none asked for).
    #[serde(skip)]
    fifo_due: [u64; 2],
    #[serde(skip)]
    in_dma: bool,
    /// The value the last unit of a transfer carried: the data lines keep it, and channel 0,
    /// whose bus stops short of the cartridge, reads it back instead. Not saved.
    #[serde(skip)]
    dma_bus: u32,
    /// Channels that came due while a DMA was running and are waiting their turn in it.
    #[serde(skip)]
    dma_wait: u8,
    /// A SWP is between its read and its write: the bus is locked, so a DMA that comes due
    /// waits until the write has happened (the Aging Cartridge's "SWP LOCKS BUS"). Only ever
    /// true while one instruction runs, so it is not saved.
    #[serde(skip)]
    pub dma_locked: bool,
    /// HBlank/VBlank DMA channels whose trigger fell inside a SWP's lock: they run at the
    /// next bus access after the lock clears, instead of between the SWP's read and write.
    #[serde(skip)]
    pub dma_deferred: u8,
    /// The clock is being run forward right now (`advance`).
    #[serde(skip)]
    in_advance: bool,
    /// Where a halt has to go back to when it ends: the wait at $70 is the BIOS's own, and once
    /// the interrupt has been dealt with it returns the processor to the game. Saved with the
    /// rest of the short-lived state, not in the fixed layout above.
    #[serde(skip)]
    pub halt_return: Option<u32>,
    /// Saved on its own after the rest (see `BusTiming`).
    #[serde(skip)]
    pub tm: BusTiming,
    /// The link port's transfer, and whether a second phone is on the cable (`sio.rs`).
    #[serde(skip)]
    pub sio: super::sio::Sio,
    /// The BIOS sound driver's work area (SWI 0x1A `SoundDriverInit`), or 0 when no game has
    /// started it, and the mode it was last given (SWI 0x1B). Not saved: they live in the
    /// game's own memory as the SoundArea itself, which is saved, but the driver's pointer
    /// to it is the BIOS's own business; after a state is loaded the driver is quiet until
    /// the game starts it again (it only ever does that once, at start-up).
    #[serde(skip)]
    pub sound_area: u32,
    #[serde(skip)]
    pub sound_mode: u32,
    /// Whether the driver's V-Blank DMA is running (SWI 0x28/0x29). Not saved, with the
    /// rest of the driver state above.
    #[serde(skip)]
    pub sound_vsync_on: bool,
    /// The Wireless Adapter on the link port (`rfu.rs`), and whether the setting lets a game
    /// that looks for one find it.
    #[serde(skip)]
    pub rfu: super::rfu::Rfu,
    #[serde(skip)]
    pub rfu_allowed: bool,
    /// RCNT's SD pin was driven high in general-purpose mode: the start of the adapter's reset.
    #[serde(skip)]
    rfu_sd_high: bool,
    /// ROM bytes cheat codes change while they are on (cheats.rs): (offset into the cartridge,
    /// byte), sorted. Not saved: the app sets the codes.
    #[serde(skip)]
    pub rom_patches: Vec<(u32, u8)>,
    /// Speed only, never saved: the cycle the next counting timer overflows (`u64::MAX` when
    /// none is counting), so the clock isn't worked out for all four timers on every access.
    /// `None` means a timer changed and it has to be worked out again.
    #[serde(skip)]
    next_timer: Option<u64>,
    /// Speed only, never saved: the earliest cycle a DMA switched on to start straight away is
    /// due (`None`: worked out again on the next access).
    #[serde(skip)]
    next_dma: Option<u64>,
    /// Speed only, never saved: the cycle before which running the machine on changes
    /// nothing but the clock counters and the sound chip. Up to there `run_pending` moves the
    /// counters and leaves the sound for later; 0 = worked out again on the next access.
    #[serde(skip)]
    fast_until: u64,
    /// Speed only: cycles the sound chip has still to run through. No output sample, no
    /// sequencer step and no event falls inside them, so running them later in one go comes out
    /// exactly the same (`sync_sound`).
    #[serde(skip)]
    lazy_sound: u32,
    /// Speed only, never saved: one cartridge halfword's cycles in each wait-state area,
    /// [area][seq], worked out from WAITCNT when it is written (and after a state load)
    /// instead of on every cartridge access.
    #[serde(skip)]
    rom_wait: [[u32; 2]; 3],
}

impl Bus {
    pub fn new(rom: Vec<u8>) -> Bus {
        let save = Save::detect(&rom);
        let mut apu = Apu::default();
        apu.agb = true;
        apu.write(0xFF26, 0); // the GBA starts with sound off
        let mp2k = super::mp2k::Mp2k::new(&rom);
        let mut bus = Bus {
            fifo_vol: [100; 2],
            fifo_smooth: false,
            mp2k,
            fifo_from: [0; 2],
            fifo_to: [0; 2],
            fifo_period: [1; 2],
            fifo_age: [0; 2],
            gbp: false,
            vba_compat: true,
            ci_log: Vec::new(),
            gbp_i: 0,
            sio: Default::default(),
            sound_area: 0,
            sound_mode: super::bios::SOUND_DRIVER_DEFAULT_MODE,
            sound_vsync_on: true,
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
            line_cycle: 500,
            line: 0,
            pending: 0,
            cycles: 0,
            last_addr: 0,
            exec_pc: 0x0800_0000,
            bios_latch: 0xE129_F000,
            open_bus: 0,
            open_fill_stepping: false,
            gb_acc: 0,
            seq_acc: 0,
            frame_ready: false,
            irq_pending: false,
            irq_at: 0,
            irq_committed: false,
            irq_write_at: 0,
            span_line: -1,
            span_x: 0,
            oam_snap: Vec::new(),
            bg_bits: 0,
            bg_queue: Vec::new(),
            line_abs: 0,
            fifo_due: [0; 2],
            in_dma: false,
            dma_bus: 0,
            dma_wait: 0,
            dma_locked: false,
            dma_deferred: 0,
            in_advance: false,
            halt_return: None,
            tm: BusTiming::default(),
            rom_patches: Vec::new(),
            next_timer: None,
            next_dma: None,
            fast_until: 0,
            lazy_sound: 0,
            rom_wait: [[0; 2]; 3],
        };
        bus.work_out_rom_wait();
        bus
    }

    /// The cartridge's wait states from WAITCNT (`rom16`'s table, worked out once).
    fn work_out_rom_wait(&mut self) {
        let wc = u32::from(self.io16(0x204));
        for ws in 0..3usize {
            let seq = if wc & [0x10, 0x80, 0x400][ws] != 0 { 2 } else { [3, 5, 9][ws] };
            let non = [5, 4, 3, 9][((wc >> (2 + ws as u32 * 3)) & 3) as usize];
            self.rom_wait[ws] = [non, seq];
        }
    }

    /// Forget the times worked out ahead (after a state load replaced the timers or the DMAs).
    pub fn forget_cached_times(&mut self) {
        self.next_timer = None;
        self.next_dma = None;
        self.fast_until = 0;
        self.work_out_rom_wait();
    }

    /// A cheat code's read: memory as the program sees it, with no time passing (cheats.rs).
    pub fn cheat_read(&self, addr: u32, width: u8) -> u32 {
        match width {
            1 => self.peek8(addr),
            2 => self.peek16(addr),
            _ => self.peek32(addr),
        }
    }

    /// A cheat code's write, made as the processor would make it (so registers answer), with no
    /// time passing.
    pub fn cheat_write(&mut self, addr: u32, width: u8, v: u32) {
        let save = addr >> 25 == 7;
        match width {
            1 => self.poke8(addr, v as u8),
            2 if save => self.save.write8(addr, (v >> ((addr & 1) * 8)) as u8),
            2 => self.poke16(addr & !1, v as u16),
            _ if save => self.save.write8(addr, (v >> ((addr & 3) * 8)) as u8),
            _ => {
                self.poke16(addr & !3, v as u16);
                self.poke16((addr & !3) + 2, (v >> 16) as u16);
            }
        }
    }

    /// A ROM halfword at `off` through the cheat codes' patches.
    fn patched(&self, off: u32, v: u32) -> u32 {
        let byte = |o: u32, own: u32| {
            self.rom_patches.binary_search_by_key(&o, |p| p.0).map_or(own, |i| u32::from(self.rom_patches[i].1))
        };
        byte(off, v & 0xFF) | byte(off + 1, v >> 8) << 8
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
        self.irq_ready();
    }

    /// The console does not enter an interrupt on the instruction the flag is raised: it takes
    /// seven cycles to work through the ARM7's own pipeline, and only then does the processor
    /// jump to the handler. Every one of the Advance's timing test programs measures that
    /// delay to the cycle, and games rely on it (a wait-for-vblank loop is written in it).
    /// Writing IE, IF or IME starts the same seven cycles from the write.
    const IRQ_DELAY: u64 = 3;

    /// The few pieces of state that live only a handful of cycles — a transfer in progress on
    /// the link port, an interrupt on its way, the units a DMA has left after a higher priority
    /// channel cut in — so a save state made in the middle of one carried on the same.
    pub fn cycle_state(
        &self,
    ) -> (super::sio::Sio, (bool, u64, bool), (u8, [Option<(u32, bool)>; 4]), Option<u32>) {
        let pend = self.dma.each_ref().map(|d| d.pending_channel);
        (self.sio.clone(), (self.irq_pending, self.irq_at, self.irq_committed), (self.dma_wait, pend), self.halt_return)
    }

    pub fn set_cycle_state(
        &mut self,
        c: (super::sio::Sio, (bool, u64, bool), (u8, [Option<(u32, bool)>; 4]), Option<u32>),
    ) {
        let (sio, (pending, at, committed), (wait, pend), halt_return) = c;
        self.sio = sio;
        self.irq_pending = pending;
        self.irq_at = at;
        self.irq_committed = committed;
        self.dma_wait = wait;
        self.halt_return = halt_return;
        for (d, p) in self.dma.iter_mut().zip(pend) {
            d.pending_channel = p;
        }
    }

    /// The interrupt's short delay does not run while the processor is held up waiting on the bus
    /// for a DMA: it can only be entered in a cycle the processor had to itself. It does run while
    /// the processor is busy inside itself (a multiply, say) and while it is halted.
    fn irq_stall(&mut self, n: u32) {
        if self.irq_pending && self.irq_requested() {
            self.irq_at += u64::from(n);
        }
    }

    /// Starts the interrupt's delay running, if a flag is now set that IE allows and none is
    /// already on its way. A flag that is cleared again inside the seven cycles is never taken.
    pub fn irq_ready(&mut self) {
        if !self.irq_requested() && !self.irq_committed {
            self.irq_pending = false;
        } else if !self.irq_pending {
            self.irq_pending = true;
            // A write to IE or IME only lands three cycles later, and the handler's own delay
            // starts from that landing.
            let from = self.now().max(self.irq_write_at);
            self.irq_at = from + Self::IRQ_DELAY;
        }
    }

    /// A write that would switch a pending interrupt off does not cancel it when its
    /// delay runs out inside the three cycles the write takes to land: it is committed
    /// and the processor still enters the handler.
    fn irq_commit_check(&mut self) {
        // Only an interrupt whose delay has only just run out: one taken long ago has
        // already been serviced, and its stale time must not commit it again.
        if self.irq_pending && !self.irq_requested() && self.irq_at <= self.cycles + 2 && self.irq_at + 4 >= self.cycles {
            self.irq_committed = true;
        }
    }

    /// The seven cycles are gone: the processor enters the handler now, if the master switch
    /// is on and its own interrupts are not masked. A committed interrupt enters even
    /// though the switch was turned off after it was committed.
    pub fn irq_line(&self) -> bool {
        self.irq_elapsed() && (self.io[0x208] & 1 != 0 || self.irq_committed)
    }

    /// An interrupt is requested and its seven cycles are gone (the master switch is not looked
    /// at: a halted console wakes for the flag either way, as the real one does).
    pub fn irq_elapsed(&self) -> bool {
        self.irq_pending && (self.irq_requested() || self.irq_committed) && self.now() >= self.irq_at
    }

    pub fn irq_requested(&self) -> bool {
        self.io16(0x200) & self.io16(0x202) & 0x3FFF != 0
    }

    /// Cycles until the interrupt's seven cycles are gone, if one is on its way.
    pub fn cycles_to_irq(&self) -> Option<u64> {
        (self.irq_pending && self.irq_requested()).then(|| self.irq_at.saturating_sub(self.now()))
    }

    // ------------------------------------------------------------ timing

    /// Cycles of one 16-bit cartridge read in wait-state area `ws` (0 to 2), from WAITCNT.
    #[inline(always)]
    fn rom16(&self, ws: u32, seq: bool) -> u32 {
        self.rom_wait[ws as usize][seq as usize]
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
            0x05 => if word { 2 } else { 1 },
            // VRAM belongs to the picture unit, which is using it all through the visible part
            // of a line: a CPU access has to wait a cycle for it, whether it reads or writes.
            // In the blank the unit is idle, so nothing is added there.
            0x06 => {
                let n = if word { 2 } else { 1 };
                n + u32::from(self.line < 160 && self.line_cycle < HBLANK_AT)
            }
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
        if self.in_dma {
            self.irq_stall(c);
        }
        self.tm.after_code = false;
        if !self.in_dma {
            self.tm.bus_end = self.now();
        }
    }

    /// Runs a DMA switched on to start straight away once its start-up time has passed: when
    /// the CPU next wants the bus. If it came due while the CPU was only busy inside itself,
    /// it ran alongside those cycles, so only what is left of it holds the CPU up.
    #[inline(always)]
    fn dma_catch_up(&mut self) {
        // A SWP holds the bus for its read and write: nothing starts until it is done.
        if self.dma_locked {
            return;
        }
        // The usual case, nothing due: decided here without a call.
        if let Some(n) = self.next_dma {
            if n > self.now() && self.dma_deferred == 0 {
                return;
            }
        }
        self.dma_catch_up_due();
    }

    #[inline(never)]
    fn dma_catch_up_due(&mut self) {
        let now = self.now();
        let next = match self.next_dma {
            Some(v) => v,
            None => {
                let v = self.tm.dma_due.iter().copied().min().unwrap_or(u64::MAX);
                self.next_dma = Some(v);
                v
            }
        };
        if next > now && self.dma_deferred == 0 {
            return;
        }
        self.next_dma = None;
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
            // How long the transfer itself took, measured as the wall time it added: the
            // old `pending - before` counted only what was left un-run, which is 1 minus
            // the CPU's own pending cycles — 0 for a one-cycle idle, and a wrap to ~2^32
            // for any more (the machine clock then jumped by billions of cycles, which
            // only stayed hidden because 2^32 is a multiple of a 16-bit timer's period).
            let total_before = self.cycles + u64::from(self.pending);
            self.run_dma(ch);
            let took = (self.cycles + u64::from(self.pending)).saturating_sub(total_before);
            // The part of the transfer that already ran alongside the CPU's own cycles.
            let overlap = total_before.saturating_sub(start).min(took) as u32;
            // Take those cycles back out of the time charged: the trailing idle first,
            // then the machine clock (and the scanline position with it, so the two stay
            // in step), with the fast-forward horizon dropped so it is recomputed.
            let mut rem = overlap;
            let take = rem.min(self.pending);
            self.pending -= take;
            rem -= take;
            if rem > 0 {
                let lc = self.line_cycle;
                // But not across a line edge the machine already crossed: its HBlank /
                // new-line work already ran and would run twice. Then the handful of
                // cycles stays in, and the timer reads a touch high for this one catch-up.
                if lc >= rem && !(lc >= HBLANK_AT && lc - HBLANK_AT < rem) {
                    self.cycles -= u64::from(rem);
                    self.line_cycle -= rem;
                    self.fast_until = 0;
                }
            }
        }
        // A HBlank/VBlank trigger that fell inside a SWP's lock runs now that it is clear.
        if self.dma_deferred != 0 {
            let deferred = std::mem::take(&mut self.dma_deferred);
            for ch in 0..4 {
                if deferred & (1 << ch) != 0
                    && self.dma[ch].active
                    && self.io16(0xBA + ch * 12) & 0x8000 != 0
                {
                    self.run_dma(ch);
                }
            }
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
            let v = self.fetch16(addr);
            // What an unmapped read returns afterwards: the last opcode read, arranged by where
            // the code runs from (two halves of the same opcode for most of the memory map).
            let other = |s: &Self, a: u32| s.fetch16(a);
            self.open_bus = match addr >> 24 {
                0x00 | 0x07 if addr & 2 == 0 => v | other(self, addr + 2) << 16,
                0x00 | 0x07 => other(self, addr.wrapping_sub(2)) | v << 16,
                0x03 if addr & 2 == 0 => v | other(self, addr.wrapping_sub(2)) << 16,
                0x03 => other(self, addr.wrapping_sub(2)) | v << 16,
                _ => v | v << 16,
            };
            v
        } else {
            let v = self.fetch32(addr);
            self.open_bus = v;
            if addr < 0x4000 {
                // What a BIOS read from outside returns: the last opcode the BIOS read.
                self.bios_latch = v;
            }
            v
        }
    }

    // ------------------------------------------------------------ CPU access

    /// Before the processor reads a register: the sound chip is brought up to date (the
    /// speed path leaves it behind between events).
    #[inline(always)]
    fn before_io_read(&mut self, addr: u32) {
        if addr >> 24 == 0x04 {
            self.sync_sound();
        }
    }

    /// Before a register is written: the sound chip is brought up to date, and what lies ahead
    /// is worked out again afterwards (the write can start a timer, a transfer or the sound).
    fn before_io_write(&mut self) {
        self.sync_sound();
        self.fast_until = 0;
    }

    /// Start and end of a frame: everything owed is run, and what lies ahead worked out afresh.
    pub fn sync_all(&mut self) {
        self.sync_sound();
        self.fast_until = 0;
    }

    pub fn read8(&mut self, addr: u32) -> u32 {
        self.data(addr, false, false);
        self.run_pending();
        self.before_io_read(addr);
        self.peek8(addr)
    }

    pub fn read16(&mut self, addr: u32) -> u32 {
        self.data(addr, false, false);
        self.run_pending();
        self.before_io_read(addr);
        if self.eeprom_at(addr) {
            return u32::from(self.save.eeprom_read());
        }
        self.fetch16(addr)
    }

    pub fn read32(&mut self, addr: u32) -> u32 {
        self.read32_seq(addr, false)
    }

    /// A word read; `seq` for the second word on of a block transfer.
    pub fn read32_seq(&mut self, addr: u32, seq: bool) -> u32 {
        self.data(addr, true, seq);
        self.run_pending();
        self.before_io_read(addr);
        if self.eeprom_at(addr) {
            // The cartridge bus is 16 bits wide: a word is two reads, two bits.
            let lo = u32::from(self.save.eeprom_read());
            return lo | u32::from(self.save.eeprom_read()) << 16;
        }
        self.fetch32(addr)
    }

    /// The EEPROM's own address range (cartridges up to 16 MiB put it all over $0D000000).
    /// Reading it takes the chip's next bit: the answer to a read request, or 1 (ready).
    /// Before this the phone core always answered 1 here, so a game read back nothing but
    /// $FF and called its save broken (Super Mario Advance 2: "Your save data is corrupt").
    /// `peek16` still answers 1: it does not consume a bit, so opcode fetches and cheat
    /// reads through it leave the chip alone.
    #[inline(always)]
    fn eeprom_at(&self, addr: u32) -> bool {
        addr >> 24 == 0x0D && self.save.kind == SaveKind::Eeprom && self.rom.len() <= 0x100_0000
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
        // A single store to a channel's control register sets the count and switches it on in
        // one go: the halfwords are written together, not one after the other with the machine
        // running in between (which would start the channel with half the setting in place).
        let a = (addr & 0xFF_FFFF) as usize;
        let dma_cnt = (0xB8..0xE0).contains(&a) && (a - 0xB8) % 12 == 8;
        if addr >> 24 == 0x04 && dma_cnt {
            self.before_io_write();
            self.io_write16(a, v as u16);
            self.io_write16(a + 2, (v >> 16) as u16);
            self.run_pending();
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

    /// `peek16` with the work RAMs and the plain part of the cartridge read straight (the
    /// speed path); everything else goes through `peek16` itself. Same values.
    #[inline(always)]
    fn fetch16(&self, addr: u32) -> u32 {
        let r = |m: &[u8], i: usize| u32::from(m[i]) | u32::from(m[i + 1]) << 8;
        match addr >> 24 {
            0x03 => r(&self.iwram, (addr & 0x7FFE) as usize),
            0x02 => r(&self.ewram, (addr & 0x3FFFE) as usize),
            0x08..=0x0C => {
                let a = (addr & 0x1FF_FFFE) as usize;
                if a >= 0x100 && a + 1 < self.rom.len() && self.rom_patches.is_empty() {
                    r(&self.rom, a)
                } else {
                    self.peek16(addr)
                }
            }
            _ => self.peek16(addr),
        }
    }

    /// `peek32` the same way.
    #[inline(always)]
    fn fetch32(&self, addr: u32) -> u32 {
        let a = addr & !3;
        match a >> 24 {
            0x02 | 0x03 | 0x08..=0x0C => self.fetch16(a) | self.fetch16(a + 2) << 16,
            _ => self.peek32(addr),
        }
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
                } else if a < 0x1000 {
                    0
                } else {
                    // Above the mirrors the region is unmapped, and the shared lines answer.
                    self.open_bus & 0xFFFF
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
                    let v = r(&self.rom, a);
                    if self.rom_patches.is_empty() { v } else { self.patched(a as u32, v) }
                } else {
                    // Past the end of the cartridge image the space is zero-filled (GBAHawk keeps
                    // a full-size ROM array and indexes it), while the jsmolka program that checks
                    // the other rule (values that step by one) wants that one instead.
                    // Past the end of the cartridge the bus keeps counting, as GBATEK describes:
                    // the shared address/data lines leave 16-bit values that step by one.
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
                self.before_io_write();
                let a = (addr & 0xFF_FFFF) as usize;
                if a == 0x999990 {
                    // The AGBEEG Aging Cartridge's CI register: a byte of a test result.
                    self.ci_log.push(v);
                } else if a < 0x400 {
                    self.io_write8(a, v);
                } else if a & 0xFFFC == 0x800 {
                    let sh = (a & 3) * 8;
                    self.tm.memcnt = (self.tm.memcnt & !(0xFF << sh)) | u32::from(v) << sh;
                }
            }
            // Byte writes to palette and background VRAM write the byte to both halves;
            // to OBJ VRAM and OAM they are ignored.
            0x05 => {
                self.flush_span();
                let a = (addr & 0x3FE) as usize;
                self.pal[a] = v;
                self.pal[a + 1] = v;
            }
            0x06 => {
                self.flush_span();
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
                self.before_io_write();
                let a = (addr & 0xFF_FFFF) as usize;
                if a == 0x999990 {
                    // The AGBEEG Aging Cartridge's CI register: the low byte of a result.
                    self.ci_log.push(v as u8);
                } else if a < 0x400 {
                    self.io_write16(a, v);
                } else if a & 0xFFFC == 0x800 {
                    let sh = (a & 2) * 8;
                    self.tm.memcnt = (self.tm.memcnt & !(0xFFFF << sh)) | u32::from(v) << sh;
                }
            }
            0x05 => {
                self.flush_span();
                w(&mut self.pal, (addr & 0x3FF) as usize)
            }
            0x06 => {
                self.flush_span();
                w(&mut self.vram, Self::vram_index(addr))
            }
            0x07 => {
                self.flush_span();
                w(&mut self.oam, (addr & 0x3FF) as usize)
            }
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

    /// The switches a sound register of the Advance really has: the bits it lacks read zero,
    /// whatever was written to them. (The Game Boy's chip raises the bits it lacks instead, so
    /// the raw read is not what the console answers here.)
    fn sound_mask(a: usize) -> u16 {
        match a {
            0x60 => 0x007F,
            0x62 | 0x68 => 0xFFC0,
            0x64 | 0x6C | 0x74 => 0x4000,
            0x70 => 0x00E0,
            0x72 => 0xE000,
            0x78 => 0xFF00,
            0x7C => 0x40FF,
            _ => 0xFFFF,
        }
    }

    fn io_read8(&self, a: usize) -> u8 {
        // The halves the Advance has no sound register behind read zero, byte by byte as well
        // as by the halfword.
        if matches!(a, 0x66 | 0x6A | 0x6E | 0x76 | 0x7A | 0x7E | 0x86 | 0x8A) {
            return 0;
        }
        if a == 0x84 {
            // A byte of SOUNDCNT_X answers as the halfword does.
            return self.io_read16(0x84) as u8;
        }
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
            // Bits the console does not implement read back as zero, whatever was written: BG0
            // and BG1 have no affine wrap bit (13), a window register only holds six bits a
            // byte (its top two read zero), BLDCNT's top two bits and BLDALPHA's bits 5-7 and
            // 13-15 likewise. The mGBA suite writes FFFF to each and reads the mask back.
            0x08 | 0x0A => self.io16(a) & 0xDFFF,
            0x48 | 0x4A => self.io16(a) & 0x3F3F,
            0x50 => self.io16(a) & 0x3FFF,
            0x52 => self.io16(a) & 0x1F1F,
            0x100 | 0x104 | 0x108 | 0x10C => self.timer_count((a - 0x100) / 4, self.cycles),
            0x102 | 0x106 | 0x10A | 0x10E => self.timers[(a - 0x100) / 4].ctrl,
            0x128 => self.siocnt_read(),
            0x130 => 0x3FF & !self.keys,
            // The Advance's own sound registers, not the Game Boy sound chip's mirror of them:
            // several bits are fixed, which is how a game tells the Advance's registers from the
            // ones it just wrote.
            0x80 => {
                if self.io16(0x84) & 0x80 != 0 {
                    self.io16(0x80) & 0xFF77
                } else {
                    0
                }
            }
            0x82 => self.io16(0x82) & 0x770F,
            // SOUNDCNT_X: bit 7 is the master switch and the four bits below it say which
            // channels the chip is running. The three bits between them read zero on the
            // Advance, where the Game Boy's own chip raises them — the suite writes FFFF here
            // and reads 0080 back once the channel registers it restored have stopped their
            // channels. The write below already reaches the chip (the sound area's arm does it),
            // so this is only the answer the register gives.
            0x84 => u16::from(self.io[0x84] & 0x80) | u16::from(self.apu.read(0xFF26) & 0x0F),
            // The sound registers are the Game Boy's chip's, but the halves around them are the
            // Advance's own: a register answers with the switches it really has (the bits it
            // lacks read zero, where the Game Boy's chip raises them), a halfword with no
            // register behind it reads zero, and the two in the gap after SOUNDBIAS read the
            // shared lines. The mGBA suite writes FFFF to each and reads the mask back.
            0x60 | 0x62 | 0x64 | 0x68 | 0x6C | 0x70 | 0x72 | 0x74 | 0x78 | 0x7C => {
                self.io16(a) & Self::sound_mask(a)
            }
            0x66 | 0x6A | 0x6E | 0x76 | 0x7A | 0x7E | 0x86 | 0x8A => 0,
            0x8C | 0x8E => (self.open_bus & 0xFFFF) as u16,
            0x90 | 0x92 | 0x94 | 0x96 | 0x98 | 0x9A | 0x9C | 0x9E => self.io16(a),
            // A channel's count is written, not read back (the count and the switch are two
            // halfwords, and only the switch's answers), and the switch leaves out the bits the
            // Advance's channel does not have: the Game Pak's DRQ bit sits on the fourth
            // channel, whose first three do not have it.
            0xB8 | 0xC4 | 0xD0 | 0xDC => 0,
            0xBA | 0xC6 | 0xD2 => self.io16(a) & 0xF7E0,
            0xDE => self.io16(a) & 0xFFE0,
            // A halfword the Advance has no register behind reads zero where the lines are
            // driven; the ones a game may write but never read (the video and sound FIFO
            // registers, the address registers, and the whole gap above them) read the shared
            // lines instead.
            0x136 | 0x142 | 0x15A | 0x206 | 0x20A | 0x302 => 0,
            0x10..=0x1F | 0x20..=0x3F | 0x40..=0x47 | 0x4C..=0x4F | 0x54..=0x5F
            | 0xA0..=0xB7 | 0xBC..=0xC3 | 0xC8..=0xCF | 0xD4..=0xDB | 0xE0..=0xFF => {
                (self.open_bus & 0xFFFF) as u16
            }
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
        // A write to a picture register during HDraw moves the picture from this dot on:
        // draw up to here with the state as it stands first. (DISPSTAT and VCOUNT are not
        // picture registers; the DMA control registers live above this range.)
        if a <= 0x54 && a != 0x04 && a != 0x06 {
            self.flush_span();
        }
        match a {
            0 => {
                // Background enables reach the picture late: a background switched on takes
                // three lines to appear, one switched off takes one line to go. The live
                // register changes at once (mode, forced blank and the rest are immediate);
                // only bits 8-11 are queued, per background, the latest write winning.
                let old = self.io16(0);
                self.set_io16(0, v);
                let changed = (old ^ v) & 0xF00;
                if changed != 0 {
                    for b in 0..4u8 {
                        let bit = 0x100u16 << b;
                        if changed & bit != 0 {
                            let on = v & bit != 0;
                            let delay = if on { 3 } else { 1 };
                            // The enable is latched once a line, at the first dot of HDraw, so a
                            // write in the first dots of a line belongs to the latch that has just
                            // been taken and is dated from the line before. A game that waits on
                            // the HBlank flag lands exactly there (`while (DISPCNT & HBL);` and then
                            // the write), and dating it from the line it arrives in puts its
                            // enable one line late: the mGBA suite's "Layer toggle 2" differs on
                            // precisely those lines (VCOUNT 0x40 + 3, 0x49 + 3, ... and no others).
                            let from = if self.line_cycle < 4 {
                                self.line_abs.saturating_sub(1)
                            } else {
                                self.line_abs
                            };
                            self.bg_queue.retain(|&(_, qb, _)| qb != b);
                            self.bg_queue.push((from + delay, b, on));
                        }
                    }
                }
            }
            0x04 => {
                let cur = self.io16(4);
                self.set_io16(4, (cur & 7) | (v & 0xFF38));
            }
            0x06 => {}
            0x28..=0x2F | 0x38..=0x3F => {
                self.set_io16(a, v);
                self.ppu.reload_affine(&self.io, false);
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
            0xB8 | 0xC4 | 0xD0 | 0xDC | 0xBA | 0xC6 | 0xD2 | 0xDE => {
                // A channel's control register can be written a halfword at a time, and the low
                // half carries the count and the switch: the test programs do exactly that.
                // The control register is at an even address and its high halfword one on; the
                // switch lives in the high halfword, so only a write there can start a channel.
                let ch = (a.wrapping_sub(0xB8)) / 12;
                let hi = (a.wrapping_sub(0xB8)) & 3 == 2;
                let was = self.io16(a) & 0x8000 != 0;
                self.set_io16(a, v);
                let on = self.io16(a) & 0x8000 != 0;
                if hi && !was && on {
                    self.dma_enable(ch);
                }
                if !on {
                    self.tm.dma_due[ch] = u64::MAX;
                    self.next_dma = None;
                }
            }
            0x84 => {
                // SOUNDCNT_X: bit 7 switches the sound circuit on, and the four bits below it
                // report which channels are running. The rest reads back as zero.
                let started = self.io[0x84] & 0x80 == 0 && v & 0x80 != 0;
                self.io[0x84] = (v as u8 & 0x80) | 0x70;
                if started {
                    self.apu.write(0xFF26, 0x80);
                } else if v & 0x80 == 0 {
                    self.apu.write(0xFF26, 0x00);
                }
            }
            0x100 | 0x104 | 0x108 | 0x10C => self.timers[(a - 0x100) / 4].reload = v,
            0x102 | 0x106 | 0x10A | 0x10E => self.timer_control((a - 0x100) / 4, v),
            0x204 => {
                // Bit 13 is unused and bit 15 says it is an Advance (0), read-only.
                self.set_io16(0x204, v & 0x5FFF);
                self.work_out_rom_wait();
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
                self.irq_commit_check();
                // Clearing a flag inside the interrupt's seven cycles cancels it.
                if self.io16(0x202) & self.io16(0x200) == 0 && !self.irq_committed {
                    self.irq_pending = false;
                }
            }
            0x300 => {
                self.io[0x300] = v as u8;
                if v & 0x8000 == 0 {
                    // HALTCNT in the high byte; a 16-bit write to $300 halts too.
                }
                self.halted = true;
            }
            // Switching interrupts on (IE) or the master switch (IME) starts the seven cycles
            // from the write, not from whenever the flag was raised.
            0x200 | 0x208 => {
                // A change to IE or IME lands three cycles after the write (GBAHawk,
                // GBA_System.cpp: "changes to IRQ that happen due to writes should take place in
                // 3 cycles"). The delay before the handler is entered starts from there.
                self.set_io16(a, v);
                self.irq_write_at = self.cycles + 3;
                self.irq_commit_check();
                self.irq_ready();
                // A flag that was raised while interrupts were masked has already run
                // its delay out: turning the switch on starts the seven cycles again
                // from this write, it does not enter the handler at once.
                if self.irq_requested() && self.irq_pending && self.now() >= self.irq_at {
                    self.irq_at = self.now().max(self.irq_write_at);
                }
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
        // The address a transfer starts at is kept whole, the cartridge's bits included:
        // channel 0 cannot reach the cartridge, but that is its bus falling short (see the
        // read in `dma_channel`), not the address being rewritten — masked there, the suite's
        // cartridge loads read the BIOS area instead.
        d.src = src & 0x0FFF_FFFF;
        d.dst = dst & if ch == 3 { 0x0FFF_FFFF } else { 0x07FF_FFFF };
        d.active = true;
        self.dma_reload_count(ch);
        if (self.io16(base + 10) >> 12) & 3 == 0 {
            // It starts two cycles after the write that switched it on has ended.
            // P7 exp: channel 0 needs two more (see start-delay vs prefetcher_dma).
            self.tm.dma_due[ch] = self.cycles + if ch == 0 { 4 } else { 2 };
            self.next_dma = None;
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
                if self.dma_locked {
                    // A SWP is holding the bus: this channel waits until the lock clears
                    // (the next bus access runs it) instead of landing between the SWP's
                    // read and write. Sound DMA is left alone: its destination is always
                    // the sound FIFO, so it can never write the SWP's address.
                    self.dma_deferred |= 1 << ch;
                    continue;
                }
                self.run_dma(ch);
            }
        }
    }

    /// Runs channel `ch`'s transfer, one unit at a time, letting the rest of the machine catch
    /// up after each unit. That matters: a line's HBlank, a timer's overflow and the sound all
    /// happen while a long transfer is under way, and a higher-numbered priority channel that
    /// comes due then takes the bus between two of its units, as it does on the console.
    fn run_dma(&mut self, ch: usize) {
        if self.in_dma {
            // A DMA came due while another is running: it waits its turn in that run.
            self.dma_wait |= 1 << ch;
            return;
        }
        // A transfer can read and write the sound registers: the sound chip catches up first.
        self.sync_sound();
        self.fast_until = 0;
        self.in_dma = true;
        self.dma_wait = 1 << ch;
        while self.dma_wait != 0 {
            // The lowest channel number waiting is the highest priority, as on the console.
            let running = (0..4).find(|c| self.dma_wait & (1 << c) != 0).unwrap();
            self.dma_wait &= !(1 << running);
            // A channel cut into part way puts itself back in the queue, to carry on after.
            self.dma_channel(running);
        }
        self.in_dma = false;
    }

    /// Lets the rest of the machine run through the cycles a DMA has spent, so everything that
    /// happens at its own cycle happens at it.
    fn dma_advance(&mut self) {
        // Already inside the clock: whatever this DMA spent is run through by the loop that is
        // running, and advancing again here would count the same cycles twice.
        if self.in_advance {
            return;
        }
        while self.pending > 0 {
            let n = self.pending;
            self.pending = 0;
            self.advance(n);
        }
    }

    /// One channel's transfer, when it is picked to run: from its first unit, or from where a
    /// higher priority channel interrupted it.
    fn dma_channel(&mut self, ch: usize) {
        let cntaddr = 0xBA + ch * 12;
        let cnt = self.io16(cntaddr);
        let fifo = (cnt >> 12) & 3 == 3 && (ch == 1 || ch == 2);
        let word = cnt & 0x400 != 0 || fifo;
        let step: u32 = if word { 4 } else { 2 };
        let dctl = if fifo { 2 } else { (cnt >> 5) & 3 };
        let sctl = (cnt >> 7) & 3;
        let resume = self.dma[ch].pending_channel.take();
        #[cfg(feature = "gba_trace")]
        eprintln!("DMA{ch} start now={} src={:08X} dst={:08X} cnt={:04X} resume={:?}", self.now(), self.dma[ch].src, self.dma[ch].dst, cnt, resume);
        let (mut s, mut d) = (self.dma[ch].src, self.dma[ch].dst);
        // Two internal cycles (four when both ends are on the cartridge), half before the
        // transfer and half after it; the first unit is a non-sequential read and write, and
        // every one after it sequential.
        let cart = |a: u32| (0x08..=0x0F).contains(&(a >> 24));
        let internal = if cart(s) && cart(d) { 2 } else { 1 };
        let (mut left, first_seq) = match resume {
            Some((left, seq)) => (left, seq),
            None => {
                let count = if fifo { 4 } else { self.dma[ch].count };
                if self.dma[ch].dst >> 24 == 0x0D {
                    self.save.eeprom_dma(count);
                }
                self.idle(internal);
                (count, false)
            }
        };
        while left > 0 {
            let seq = first_seq || left < if fifo { 4 } else { self.dma[ch].count };
            let cart_read = ch == 0 && (0x08..=0x0F).contains(&(s >> 24));
            let v = if word {
                self.data(s & !3, true, seq);
                if cart_read {
                    // Channel 0's bus stops short of the cartridge, so a read there takes what
                    // the data lines carry: the last unit a transfer moved, aligned down
                    // whatever the address was. (The mGBA suite loads from a cartridge with
                    // channel 0 and expects the word EWRAM held, the same for all three
                    // unaligned addresses; its DMA tests expect the IWRAM word the same way.)
                    self.dma_bus
                } else if self.eeprom_at(s) {
                    let lo = u32::from(self.save.eeprom_read());
                    lo | u32::from(self.save.eeprom_read()) << 16
                } else {
                    self.peek32(s & !3)
                }
            } else {
                self.data(s & !1, false, seq);
                if cart_read {
                    self.dma_bus & 0xFFFF
                } else if self.eeprom_at(s) {
                    u32::from(self.save.eeprom_read())
                } else {
                    self.peek16(s & !1)
                }
            };
            // A halfword taken as a fallback leaves the rest of the lines as they were: the
            // word a transfer put there is what the next four-byte read still finds.
            if !cart_read {
                self.dma_bus = v;
            }
            if word {
                self.data(d & !3, true, seq);
                self.poke16(d & !3, v as u16);
                self.poke16((d & !3) + 2, (v >> 16) as u16);
            } else {
                self.data(d & !1, false, seq);
                self.poke16(d & !1, v as u16);
            }
            s = match sctl { 0 | 3 => s.wrapping_add(step), 1 => s.wrapping_sub(step), _ => s };
            d = match dctl { 0 | 3 => d.wrapping_add(step), 1 => d.wrapping_sub(step), _ => d };
            self.dma[ch].src = s;
            self.dma[ch].dst = d;
            left -= 1;
            self.dma_advance();
            if left > 0 && (0..ch).any(|c| self.dma_wait & (1 << c) != 0) {
                // A higher priority channel has come due: this one stops here, part way, and
                // carries on from its next unit once that one has run.
                self.dma[ch].pending_channel = Some((left, true));
                self.dma_wait |= 1 << ch;
                return;
            }
        }
        if resume.is_none() {
            // The second half of the transfer's internal cycles, charged only when the
            // transfer started fresh, as the console charges them.
            self.idle(internal);
        }
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

    /// Asks the channel for a refill now: the copy that keeps its queue full.
    fn fifo_refill(&mut self, n: usize) {
        let want = 0x0400_00A0 + 4 * n as u32;
        for ch in 1..3 {
            let cnt = self.io16(0xBA + ch * 12);
            if cnt & 0x8000 != 0 && (cnt >> 12) & 3 == 3 && self.dma[ch].dst == want {
                self.run_dma(ch);
            }
        }
    }

    fn fifo_push(&mut self, n: usize, v: i8) {
        // A write into a queue is ignored while the sound circuit is off (GBAHawk,
        // GBA_System.h `snd_Write_FIFO_Data`: the whole write is gated on `snd_CTRL_power`).
        // That is why a game that fills its queues first and switches sound on afterwards starts
        // with them empty, which the timing programs check.
        if self.io[0x84] & 0x80 == 0 {
            return;
        }
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
            // The refill is asked for once the queue is down to fourteen units, and the copy
            // starts three cycles after that (GBAHawk, GBA_System.cpp), which is what the timing
            // programs measure.
            if self.fifo[n].data.len() <= 14 {
                self.fifo_due[n] = self.cycles + 2;
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

    /// How far into its prescaler period the divider already is when our cycle 0 arrives.
    const TIMER_PHASE: u64 = 41;

    /// The last cycle at or before `now` on which a prescaler of `bits` is on one of its
    /// boundaries. One divider runs free from power-on and every timer ticks on its boundaries,
    /// so a timer's ticks fall at a fixed phase however long ago it was switched on; our cycles
    /// count from the first instruction the game runs, a fixed time after the console's own
    /// power-on where the divider starts, so the boundaries sit this far off.
    fn prescaler_at(now: u64, bits: u32) -> u64 {
        let mask = (1u64 << bits) - 1;
        now - (now.wrapping_sub(Self::TIMER_PHASE) & mask)
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
        self.next_timer = None;
        self.fast_until = 0;
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
        let bits = Self::timer_shift(ctrl);
        let boundary = Self::prescaler_at(now, bits);
        if !was_on {
            if self.timers[i].counter == 0xFFFF && boundary == now && (i == 0 || ctrl & 4 == 0) {
                // Loading the reload value takes a cycle, and a timer stopped at $FFFF can tick
                // in it: it overflows (interrupt and all) before it starts from the reload value.
                self.tm.timer_at[i] = now;
                return;
            }
            self.timers[i].counter = self.timers[i].reload;
            self.tm.timer_at[i] = boundary + 1;
        } else {
            self.tm.timer_at[i] = boundary;
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

    /// The cycle the next counting timer overflows, `u64::MAX` when none is counting.
    fn next_timer_at(&mut self) -> u64 {
        if let Some(t) = self.next_timer {
            return t;
        }
        let mut m = u64::MAX;
        for i in 0..4 {
            if self.timer_free(i) {
                m = m.min(self.timer_overflow_at(i));
            }
        }
        self.next_timer = Some(m);
        m
    }

    fn timer_overflow(&mut self, i: usize, at: u64) {
        self.next_timer = None;
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

    /// Runs the rest of the machine through the cycles spent so far. The usual case (the
    /// speed path): nothing happens before `fast_until`, so only the clock counters move and
    /// the sound chip's share waits in `lazy_sound`. Everything else goes the full way.
    #[inline(always)]
    pub fn run_pending(&mut self) {
        if self.in_dma {
            return;
        }
        let end = self.cycles + u64::from(self.pending);
        if end < self.fast_until {
            let n = self.pending;
            self.pending = 0;
            self.cycles = end;
            self.line_cycle += n;
            self.lazy_sound += n;
            return;
        }
        self.run_pending_full();
    }

    /// Runs the sound chip through the cycles it was left behind by. They hold no output
    /// sample, no sequencer step and no event, so this is what running them one access at a
    /// time would have done.
    #[inline]
    pub fn sync_sound(&mut self) {
        if self.lazy_sound != 0 {
            let n = self.lazy_sound;
            self.lazy_sound = 0;
            self.sound(n);
        }
    }

    /// The cycle before which running on changes nothing but the counters and the sound chip:
    /// the next line edge, timer overflow, sound sequencer step and output sample. 0 (always
    /// the full way) while anything shorter-lived is under way.
    fn work_out_fast_until(&mut self) -> u64 {
        if self.in_dma
            || self.fifo_due != [0; 2]
            || self.irq_write_at != 0
            || self.cycles_to_sio().is_some()
            || self.apu.hq
            || self.apu.out_hz == 0
            || self.apu.settling()
        {
            return 0;
        }
        let boundary = if self.line_cycle < HBLANK_AT { HBLANK_AT } else { LINE };
        let mut until = self.cycles + u64::from(boundary - self.line_cycle);
        until = until.min(self.next_timer_at());
        // `sound` with the cycles still owed to it counted in.
        let seq = self.seq_acc + self.lazy_sound;
        let to_seq = if seq < 16384 { 16384 - seq } else { 32768 - seq };
        until = until.min(self.cycles + u64::from(to_seq));
        let hz = self.apu.out_hz;
        let left = crate::apu::CPU_HZ - self.apu.sample_acc();
        let ticks = left.div_ceil(hz);
        let to_sample = (4 * u64::from(ticks)).saturating_sub(u64::from(self.gb_acc + self.lazy_sound));
        until.min(self.cycles + to_sample)
    }

    #[inline(never)]
    fn run_pending_full(&mut self) {
        self.sync_sound();
        // A write to IE or IME only takes effect three cycles after it was made, so the line is
        // looked at again once that has passed (the timing programs write IE around the moment a
        // flag comes up and measure which wins).
        for n in 0..2 {
            if self.fifo_due[n] != 0 && self.cycles >= self.fifo_due[n] {
                self.fifo_due[n] = 0;
                self.fifo_refill(n);
            }
        }
        if self.irq_write_at != 0 && self.cycles >= self.irq_write_at {
            self.irq_write_at = 0;
            self.irq_ready();
        }
        while self.pending > 0 {
            let n = self.pending;
            self.pending = 0;
            self.advance(n);
        }
        self.fast_until = self.work_out_fast_until();
    }

    /// Runs the rest of the machine for `n` cycles. It stops at every point something happens
    /// at its own cycle — a timer overflow, the link port's transfer ending, a scanline's edge —
    /// so a register read taken at that moment sees the count the console would show.
    fn advance(&mut self, n: u32) {
        self.sync_sound();
        self.fast_until = 0;
        self.in_advance = true;
        let mut left = n;
        while left > 0 {
            let boundary = if self.line_cycle < HBLANK_AT { HBLANK_AT } else { LINE };
            let mut step = left.min(boundary - self.line_cycle);
            // The same as cycles_to_event, with the timers' part remembered between accesses.
            let timer_at = self.next_timer_at();
            let timer = (timer_at != u64::MAX).then(|| timer_at.saturating_sub(self.cycles));
            let event = match (timer, self.cycles_to_sio()) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            if let Some(d) = event {
                step = step.min(d.clamp(1, u64::from(u32::MAX)) as u32);
            }
            self.cycles += u64::from(step);
            // Only an overflow that is due now has anything to run.
            if timer_at <= self.cycles {
                self.run_timers();
            }
            self.line_cycle += step;
            left -= step;
            if self.line_cycle == HBLANK_AT {
                self.hblank();
            } else if self.line_cycle == LINE {
                self.line_cycle = 0;
                self.next_line();
            }
            self.sio_tick();
        }
        self.sound(n);
        self.in_advance = false;
    }

    /// Cycles until the next thing that happens at a cycle of its own: a running timer's
    /// overflow, or the link port's transfer ending.
    fn cycles_to_event(&self) -> Option<u64> {
        let timer = (0..4)
            .filter(|&i| self.timer_free(i))
            .map(|i| self.timer_overflow_at(i).saturating_sub(self.cycles))
            .min();
        match (timer, self.cycles_to_sio()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// The background enables as they stand after a state load: the live DISPCNT, with no
    /// change pending and no span open. (A state made mid-line re-renders the line whole.)
    pub fn sync_bg_bits(&mut self) {
        self.bg_bits = self.io16(0) & 0xF00;
        self.bg_queue.clear();
        self.span_line = -1;
        self.span_x = 0;
        self.oam_snap.clear();
    }

    /// Applies the background-enable changes whose line has come. This runs at render time
    /// (a span flush, or HBlank), not at the line edge: a write made during the line itself
    /// can still supersede a change queued for that line before the line is drawn.
    fn apply_bg_queue(&mut self) {
        let now = self.line_abs;
        let mut i = 0;
        while i < self.bg_queue.len() {
            let (at, b, on) = self.bg_queue[i];
            if at <= now {
                self.bg_queue.remove(i);
                if on {
                    self.bg_bits |= 0x100 << b;
                } else {
                    self.bg_bits &= !(0x100 << b);
                }
            } else {
                i += 1;
            }
        }
    }

    /// Renders the dots of the current line up to the dot the machine has reached, with the
    /// state as it stands before the write that called this. Only during HDraw of a visible
    /// line; a write in HBlank or VBlank finds the line already drawn (or not drawn at all).
    /// The objects come from the snapshot taken at the previous line's HBlank, and the
    /// background enables are the delayed ones — so the span shows the picture as it was,
    /// not as the write is about to make it.
    fn flush_span(&mut self) {
        if self.line >= 160 || self.line_cycle >= HBLANK_AT || self.ppu.no_render {
            return;
        }
        if self.span_line != self.line as i32 {
            self.span_line = self.line as i32;
            self.span_x = 0;
        }
        let x = (self.line_cycle / 4).min(240);
        if x <= self.span_x {
            return;
        }
        let y = self.line as usize;
        let x0 = self.span_x as usize;
        let x1 = x as usize;
        self.apply_bg_queue();
        let (io, vram, pal) = (&self.io, &self.vram, &self.pal);
        let oam = if self.oam_snap.is_empty() { &self.oam } else { &self.oam_snap };
        let bg = self.bg_bits;
        self.ppu.render_span(io, vram, pal, oam, y, x0, x1, bg);
        self.span_x = x;
    }

    fn hblank(&mut self) {
        if self.line < 160 && !self.ppu.no_render {
            // Finish the line: the spans already drawn stop where the last mid-line write
            // left off, and the rest renders with the state as it stands at HBlank.
            let y = self.line as usize;
            let x0 = if self.span_line == self.line as i32 { self.span_x as usize } else { 0 };
            if x0 < W {
                self.apply_bg_queue();
                let (io, vram, pal) = (&self.io, &self.vram, &self.pal);
                let oam = if self.oam_snap.is_empty() { &self.oam } else { &self.oam_snap };
                let bg = self.bg_bits;
                self.ppu.render_span(io, vram, pal, oam, y, x0, W, bg);
            }
            self.span_line = -1;
            self.span_x = 0;
            // Sprite evaluation is one line ahead: the OAM as it stands at this HBlank is
            // what the next line's sprites are drawn from. A write to OAM after this point
            // (in HBlank, or during the next line) reaches the picture a line later still.
            self.oam_snap = self.oam.clone();
        }
        // The HBlank IF flag is set at the start of HBlank only when DISPSTAT
        // bit 4 is set (same gate as VBlank bit 3, VCount bit 5). An earlier
        // P7 experiment ungated VBlank and HBlank because the suite's menu and
        // DMA tests hung without the bits set — that was a scaffold bug, not an
        // emulator bug: real libgba's irqEnable sets the DISPSTAT bits (see
        // scripts/mgba-suite-build/src/interrupt.c), and ungating regressed
        // Alyosha's Halt_DMA_IRQ_Read_OAM. The gates stay.
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
        self.line_abs += 1;
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
            self.ppu.reload_affine(&self.io, true);
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
        // The link port's transfer ending is an event of its own: never jump past it, or the
        // interrupt it raises lands later than the console raises it.
        if let Some(d) = self.cycles_to_sio() {
            to = to.min(d.min(256) as u32);
        }
        // A halt also ends on the interrupt's own delay running out, which is when the
        // processor actually enters the handler.
        if let Some(d) = self.cycles_to_irq() {
            to = to.min(d.min(256) as u32);
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
