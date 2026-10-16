//! Nintendo's Donkey Kong board (the two-board TKG-04 set) and Donkey Kong Jr.'s, as MAME 0.289's
//! dkong.cpp, dkong_v.cpp and dkong_a.cpp describe them (.kiro/notes/docs/ARCADE_CORE.md).
//!
//! - **The Z80** runs at 3.072 MHz (the 61.44 MHz crystal / 20), with the program in ROM at
//!   0000h (20 KB on Donkey Kong, 24 KB plus 8000h-9FFFh, B000h-BFFFh and D000h-DFFFh on
//!   Donkey Kong Jr.'s), RAM at 6000h-6BFFh (6FFFh on Jr.'s), the sprites' RAM at 7000h-73FFh and
//!   the tiles' at 7400h-77FFh.
//! - **The picture** is 256 x 224 of a 384 x 264 raster at 6.144 MHz (60.6 frames a second),
//!   turned a quarter left in the cabinet: a 32 x 32 map of 8 x 8 tiles in two bits a pixel, whose
//!   colour comes from a PROM per four rows of each column, and 16 x 16 sprites, at most 16 on a
//!   line, chosen while the line before is drawn. VBLANK raises the NMI while the NMI latch is on.
//!   The board draws each line as the beam reaches it, so what the program writes shows from the
//!   next line on.
//! - **The colours** come from two 256 x 4 PROMs through resistors, darlington drivers and the
//!   monitor's inverting input, worked out the way MAME's resnet model does, then spread over the
//!   full brightness range as MAME does; a pixel of colour 0 in any palette is black.
//! - **The sprite DMA** is an Intel 8257 at the Z80's clock: when the program raises its request
//!   it asks for the bus, and once the Z80 lets go it copies the sprite list from RAM, a byte
//!   through the board's latch each four clocks, alternating its two channels.
//! - **The sound** is an Intel 8035 at 6 MHz (400,000 machine cycles a second) with its program
//!   in ROM in both of its banks (Jr.'s 4 KB fills both), a DAC on its port 1, and on Donkey
//!   Kong's board a second ROM of tunes it reads with MOVX through port 2's page bits (or, with
//!   bit 6 set, the command the Z80 latched). The Z80 latches a command at 7C00h, sets single bits
//!   of latch 6H at 7D00h-7D07h (the 8035 reads three of them on T0, T1 and port 2 bit 5, and the
//!   rest start the analogue effects) and holds the 8035's /INT at 7D80h. Port 2 bit 7 switches
//!   the DAC's decay circuit and bit 4 is read back by the Z80. The analogue effects and the
//!   filters are approximated in `dkong_sound`.
//!
//! **Timing** follows MAME's scheduler, which is what the tests compare against, step by step: the
//! three chips (the Z80, the 8257 while it has work, the 8035, then the 8257 if it waits) each
//! run up to the end of a timeslice in turn. A timeslice is 1/60,000 s (the board's setting), cut
//! short by the next timer (every raster line, MAME's 50 Hz sound update, the watchdog), by a chip
//! that ends short of it (a chip runs whole cycles, so the end comes back to the last one that
//! fits), and by any write that goes through MAME's scheduler (a latch, an interrupt line, the bus
//! request): that write stops the chip that made it at the end of its bus cycle, the chips after
//! it run up to that point, and only then does the write land. So the 8035 sees a latch from the
//! first instruction it starts after the Z80's write cycle ended, and the Z80 sees the 8035's
//! port 2 (IN2 bit 6) only as far as the 8035 had run before the Z80's own timeslice began. For
//! 100 µs after the program raises the DMA request the timeslices are a single Z80 cycle (MAME's
//! "perfect quantum"). The Z80 can stop inside an instruction, at the points MAME's Z80 does; the
//! rest of it, writes included, comes in the next timeslice. A watchdog (MAME's default: three
//! seconds without a read of IN2, armed by the first read) resets the board.

use serde::{Deserialize, Serialize};

use crate::dkong_sound::{self, Input, Sound};
use crate::mcs48::{self, Mcs48, Model};
use crate::z80::{Bus, Z80};

/// The picture, upright.
pub const WIDTH: usize = 224;
pub const HEIGHT: usize = 256;
const RASTER_W: usize = 256;
const FIRST_LINE: usize = 16;
const LAST_LINE: usize = 239;

pub const FPS: f64 = 6_144_000.0 / (384.0 * 264.0);
pub const SAMPLE_RATE: u32 = dkong_sound::RATE;
/// 48,000 samples a second at 60.606 frames: exactly 792.
pub const SAMPLES_PER_FRAME: usize = 792;

/// Attoseconds, as MAME counts them: a Z80 cycle, an 8035 machine cycle, a dot, a line, a frame.
const AS_PER_SECOND: i128 = 1_000_000_000_000_000_000;
pub const AS_PER_CYCLE: i128 = AS_PER_SECOND / 3_072_000;
pub const AS_PER_SOUND_CYCLE: i128 = AS_PER_SECOND / 400_000;
const AS_PER_DOT: i128 = AS_PER_SECOND / 6_144_000;
const AS_PER_LINE: i128 = AS_PER_DOT * 384;
pub const AS_PER_FRAME: i128 = AS_PER_LINE * 264;
const WATCHDOG: i128 = 3 * AS_PER_SECOND;

/// MAME's timeslice for this board (its configured maximum quantum, 60 kHz), the "perfect" one
/// (the smallest chip cycle: the Z80's and the 8257's), for how long a DMA request switches that
/// on, and its sound manager's 50 Hz update timer.
const QUANTUM: i128 = AS_PER_SECOND / 60_000;
const PERFECT: i128 = AS_PER_CYCLE;
const PERFECT_FOR: i128 = AS_PER_SECOND / 10_000;
const SOUND_UPDATE: i128 = AS_PER_SECOND / 50;

/// How many whole cycles of `per` attoseconds fit in `delta`, the way MAME's scheduler divides:
/// both shifted right until the cycle length fits 31 bits.
fn cycles_in(delta: i128, per: i128) -> u64 {
    let (mut shift, mut d) = (0, per);
    while d >= 1 << 31 {
        shift += 1;
        d >>= 1;
    }
    ((delta as u64) >> shift) / d as u64
}

/// Which board.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Variant {
    /// Donkey Kong (TKG-04).
    DonkeyKong,
    /// Donkey Kong Jr.: more ROM, a second tile bank, no tune ROM (its 8035 reads the command
    /// latch at every MOVX address) and its own analogue effects.
    DonkeyKongJr,
}

/// The chips, as MAME's regions hold them.
#[derive(Clone, Debug, Default)]
pub struct Roms {
    /// maincpu: 64 KB, the program where it maps.
    pub program: Vec<u8>,
    /// soundcpu: Donkey Kong's 2 KB program twice (0000h, 0800h) then the tune ROM at 1000h;
    /// Jr.'s 4 KB program.
    pub sound: Vec<u8>,
    /// gfx1: the tiles' two bitplanes, one after the other (4 KB, Jr.'s 8 KB).
    pub tiles: Vec<u8>,
    /// gfx2: the sprites (8 KB, or 16 KB on two Jr. bootlegs with 256 of them): the two
    /// bitplanes' halves, left halves then right halves.
    pub sprites: Vec<u8>,
    /// proms: the palette's low and high PROMs, then the tile colour codes (3 x 256).
    pub proms: Vec<u8>,
}

/// The input ports, as the program reads them: the sticks and buttons (IN0 player 1, IN1
/// player 2), IN2 (service, starts, coin; bit 6 reads the 8035 on Donkey Kong's board) and the
/// DIP switches. Everything is active high.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ports {
    pub in0: u8,
    pub in1: u8,
    pub in2: u8,
    pub dsw0: u8,
}

/// A write that goes through MAME's scheduler: it lands when its timeslice ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Sync {
    /// The sound command latch.
    Command(u8),
    /// Latch 6H, 4H (Jr.) or 5H (Jr.: bit 1 is the falling sound), whole.
    Latch6h(u8),
    Latch4h(u8),
    Latch5h(u8),
    /// The 8035's /INT.
    Irq(bool),
    /// The NMI latch switched off drops the Z80's NMI line.
    NmiClear,
    /// The 8257's bus request to the Z80.
    BusReq(bool),
    /// The 8035's port 2 reaching the board's latch.
    Port2(u8),
}

/// What a write does to the Z80's timeslice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cut {
    /// Nothing: it goes on.
    On,
    /// It goes through MAME's scheduler (or asks for the DMA): the Z80 stops at the end of the
    /// bus cycle, and the timeslice ends there.
    Stop,
    /// It starts the 8257 from its registers: the Z80 eats the rest of its timeslice.
    Eat,
}

/// The 8257's cycle-by-cycle states, as MAME's i8257 steps them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum DmaState {
    /// Idle: looking for a channel with a request.
    Si,
    /// Asking for the bus.
    S0,
    S1,
    S2,
    /// Reads.
    S3,
    /// Writes, and moves on.
    S4,
}

/// The Intel 8257 (MAME's i8257 with its read/write modes swapped, as the board wires it).
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Dma {
    address: [u16; 4],
    count: [u16; 4],
    /// Per channel: 0 verify, 1 write (I/O to memory), 2 read (memory to I/O).
    mode: [u8; 4],
    /// The mode register: channel enables, rotating priority (bit 4), TC stop (bit 6).
    transfer_mode: u8,
    status: u8,
    msb: bool,
    last_channel: u8,
    request: u8,
    /// The board's latch between the two channels.
    latch: u8,
    state: DmaState,
    current: u8,
    hreq: bool,
    /// The Z80 has let go of the bus.
    hack: bool,
    tc: bool,
    temp: u8,
    /// Its own clock, in Z80 cycles: it runs on while it waits.
    now: u64,
    /// Waiting for a trigger (a request, the bus); `woken` once one came, from the next
    /// timeslice on.
    waiting: bool,
    woken: bool,
}

impl Default for Dma {
    /// After reset: everything clear, and channel 3 counted as the last served, so rotating
    /// priority starts with channel 0.
    fn default() -> Dma {
        Dma {
            address: [0; 4],
            count: [0; 4],
            mode: [0; 4],
            transfer_mode: 0,
            status: 0,
            msb: false,
            last_channel: 3,
            request: 0,
            latch: 0,
            state: DmaState::Si,
            current: 0xFF,
            hreq: false,
            hack: false,
            tc: false,
            temp: 0,
            now: 0,
            waiting: false,
            woken: false,
        }
    }
}

/// The sound CPU's side of the board.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct SoundBoard {
    cpu: Mcs48,
    /// Machine cycles since power-on.
    cycles: u64,
    io: SoundIo,
}

/// What the 8035 sees and drives.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct SoundIo {
    /// The virtual port 2 latch MAME puts behind the 8035's port 2: what it last wrote.
    vp2: u8,
    p2_written: Option<u8>,
    /// The command latch (LS175 at 3D; LS174 on Jr.'s board).
    command: u8,
    latch6h: u8,
    latch4h: u8,
}

/// What the Z80 has left of an instruction it stopped in when its timeslice ended.
#[derive(Clone, Debug, Serialize, Deserialize)]
enum Tail {
    /// The rest of it: its remaining stopping points (the last is its end) and writes.
    Rest { stops: Vec<u64>, writes: Vec<(u64, u16, u8)> },
    /// It reads something another chip changes (IN2, the 8257) after the point where it
    /// stopped, with nothing written before that: it runs again from `start` next time, which
    /// is the same as going on from where it stopped.
    Redo { start: u64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Hw {
    variant: Variant,
    ports: Ports,
    ram: Vec<u8>,
    sprite_ram: Vec<u8>,
    vram: Vec<u8>,
    flip: bool,
    sprite_bank: u8,
    palette_bank: u8,
    gfx_bank: u8,
    nmi_mask: bool,
    nmi_line: bool,
    dma: Dma,
    snd: SoundBoard,
    sound: Sound,
    /// What reaches the analogue side, in time order, waiting for its frame's samples.
    sound_events: Vec<(i128, Input)>,
    /// Samples made since power-on.
    samples: u64,
    /// Latches 6H, 4H and 5H as the Z80 last set them (they land when the timeslice ends).
    latch6h: u8,
    latch4h: u8,
    latch5h: u8,
    /// When the watchdog runs out, once armed.
    watchdog: Option<i128>,
    /// Z80 cycles since power-on: where it is (it may be part-way into an instruction, `tail`).
    now: u64,
    tail: Option<Tail>,
    /// The 8257 has asked for the bus (the Z80 sees it at its next instruction), and the Z80
    /// has let go.
    busreq: bool,
    busack: bool,
    /// MAME's scheduler: the start of the timeslice (attoseconds), when the perfect quantum
    /// ends, whether a timer fired at the end of the last timeslice, and the writes waiting for
    /// this timeslice to end.
    base: i128,
    perfect_until: Option<i128>,
    fired: bool,
    pending: Vec<(i128, Sync)>,
    frames: u64,
    // The instruction being run: its bus cycles (M fetch, R read, W write, I I/O) with their
    // addresses, its writes and its first two opcode bytes.
    #[serde(skip)]
    cycles: Vec<(u8, u16)>,
    #[serde(skip)]
    writes: Vec<(u16, u8)>,
    #[serde(skip)]
    opcode: [u8; 2],
    #[serde(skip)]
    fetches: u8,
    #[serde(skip)]
    times: Vec<u64>,
    #[serde(skip)]
    stops: Vec<u64>,
}

/// The ROMs and what is worked out from them once.
struct Rom {
    program: Vec<u8>,
    sound: Vec<u8>,
    tiles: Vec<u8>,
    sprites: Vec<u8>,
    colour_codes: Vec<u8>,
    palette: [u32; 256],
}

pub struct Dkong {
    cpu: Z80,
    hw: Hw,
    rom: Rom,
    line: [u32; RASTER_W],
    video: Vec<u32>,
    audio: Vec<i16>,
}

/// One gun of the colour circuit, as MAME's resnet model works it out: the PROM bits drive
/// 1 kΩ / 470 Ω / 220 Ω (red, green) or 470 Ω / 220 Ω (blue) from TTL outputs (0.05 V low,
/// 4 V high through 50 Ω), against a pull-up to 5 V (470 Ω, blue 680 Ω); red and green go through
/// a darlington (no lower than 0.7 V), blue through one transistor (0.7 V lost); the monitor
/// (a Sanyo EZV20) inverts and takes 0.7 V to 3.6 V as its range.
fn gun(bits: u8, blue: bool) -> u8 {
    let (r, bias, minout, cut): (&[f64], f64, f64, f64) = if blue {
        (&[470.0, 220.0], 680.0, 0.0, 0.7)
    } else {
        (&[1000.0, 470.0, 220.0], 470.0, 0.7, 0.0)
    };
    let (vol, voh, ttl_r, vcc) = (0.05, 4.0, 50.0, 5.0);
    let mut total = 0.0;
    let mut v = 0.0;
    for (i, &res) in r.iter().enumerate() {
        if bits >> i & 1 == 0 {
            total += 1.0 / res;
            v += vol / res;
        }
    }
    total += 1.0 / bias;
    v += vcc / bias;
    // Above a high output's voltage, the high outputs carry no current.
    let open = v / total > voh;
    for (i, &res) in r.iter().enumerate() {
        if bits >> i & 1 != 0 && !open {
            total += 1.0 / (res + ttl_r);
            v += voh / (res + ttl_r);
        }
    }
    let mut out = (v / total - cut).max(minout);
    out = (vcc - out - 0.7).max(0.0).min(vcc - 1.4) / (vcc - 1.4) * vcc;
    (out * 255.0 / vcc + 0.4) as u8
}

/// The 256 colours: red from the high PROM's bits 1-3, green from its bit 0 and the low PROM's
/// bits 2-3, blue from the low PROM's bits 0-1; colour 0 of each four is black (the PROMs are
/// switched off there). Then MAME's normalisation: brightness stretched to 0-255, hue kept.
fn palette(proms: &[u8]) -> [u32; 256] {
    let mut rgb = [(0i32, 0i32, 0i32); 256];
    for (i, c) in rgb.iter_mut().enumerate() {
        if i & 3 == 0 {
            continue;
        }
        let (lo, hi) = (proms[i], proms[0x100 + i]);
        let r = hi >> 1 & 7;
        let g = (hi << 2 & 4) | (lo >> 2 & 3);
        let b = lo & 3;
        *c = (i32::from(gun(r, false)), i32::from(gun(g, false)), i32::from(gun(b, true)));
    }
    let y = |c: (i32, i32, i32)| 299 * c.0 + 587 * c.1 + 114 * c.2;
    let ymin = rgb.iter().map(|&c| y(c)).min().unwrap();
    let ymax = rgb.iter().map(|&c| y(c)).max().unwrap();
    let mut out = [0u32; 256];
    for (o, &c) in out.iter_mut().zip(&rgb) {
        let (mut r, mut g, mut b) = c;
        if ymax > ymin {
            let yy = y(c);
            let u = (b - yy / 1000) * 492 / 1000;
            let v = (r - yy / 1000) * 877 / 1000;
            let target = (yy - ymin) * 256 / (ymax - ymin);
            r = target + 1140 * v / 1000;
            g = target - 395 * u / 1000 - 581 * v / 1000;
            b = target + 2032 * u / 1000;
        }
        let k = |x: i32| x.clamp(0, 255) as u32;
        *o = k(r) << 16 | k(g) << 8 | k(b);
    }
    out
}

impl Rom {
    fn tile_pixel(&self, code: usize, x: usize, y: usize) -> u8 {
        let half = self.tiles.len() / 2;
        let n = half / 8;
        let at = (code % n) * 8 + y;
        let bit = 7 - x;
        (self.tiles[half + at] >> bit & 1) << 1 | (self.tiles[at] >> bit & 1)
    }

    fn sprite_pixel(&self, code: usize, x: usize, y: usize) -> u8 {
        let quarter = self.sprites.len() / 4;
        let n = quarter / 16;
        let at = (code % n) * 16 + y + (x / 8) * quarter;
        let bit = 7 - (x % 8);
        (self.sprites[2 * quarter + at] >> bit & 1) << 1 | (self.sprites[at] >> bit & 1)
    }
}

/// Where each of an instruction's bus cycles starts, and every point where MAME's Z80 can stop
/// in it (the last is its end): halfway through an opcode fetch and at its end, at the end of
/// each memory or I/O cycle, and after each internal cycle on its own. The internal cycles (the
/// length left over by the bus cycles) sit after the last fetch or read and before the first
/// write, which is where the Z80 spends them in nearly every instruction (counting, adding, a
/// relative jump); in the block moves (LDI and the like) they come after the write.
fn bus_layout(start: u64, len: u32, cycles: &[(u8, u16)], block: bool, times: &mut Vec<u64>, stops: &mut Vec<u64>) {
    times.clear();
    stops.clear();
    let bus: u32 = cycles.iter().map(|&(k, _)| if k == b'R' || k == b'W' { 3 } else { 4 }).sum();
    let mut internal = len.saturating_sub(bus);
    let first_write = if block { cycles.len() } else { cycles.iter().position(|&(k, _)| k == b'W').unwrap_or(cycles.len()) };
    let mut at = start;
    for (i, &(k, _)) in cycles.iter().enumerate() {
        if i == first_write {
            for _ in 0..internal {
                at += 1;
                stops.push(at);
            }
            internal = 0;
        }
        times.push(at);
        match k {
            b'M' => {
                stops.push(at + 2);
                at += 4;
            }
            b'I' => at += 4,
            _ => at += 3,
        }
        stops.push(at);
    }
    for _ in 0..internal {
        at += 1;
        stops.push(at);
    }
    if stops.last() != Some(&(start + u64::from(len))) {
        stops.push(start + u64::from(len));
    }
}

impl Hw {
    fn jr(&self) -> bool {
        self.variant == Variant::DonkeyKongJr
    }

    /// When a Z80 cycle starts, in attoseconds.
    fn time(cycle: u64) -> i128 {
        i128::from(cycle) * AS_PER_CYCLE
    }

    /// Port 2 as the 8035 reads it: what it wrote, with the board's inputs in place of their bits
    /// (latch 6H bit 3 on bit 5; on Jr.'s board also 6H bit 6 on bit 4 and 4H bit 1 on bit 6), all
    /// through inverters.
    fn p2_pins(snd: &SoundIo, jr: bool) -> u8 {
        let mut v = (snd.vp2 & !0x20) | (snd.latch6h >> 3 & 1) << 5;
        if jr {
            v = (v & !0x50) | (snd.latch6h >> 6 & 1) << 4 | (snd.latch4h >> 1 & 1) << 6;
            v ^ 0x70
        } else {
            v ^ 0x20
        }
    }

    /// The 8035's /RD reads (MOVX and INS A,BUS): on Donkey Kong's board the tune ROM's page
    /// that port 2 bits 0-2 pick, or with bit 6 set the command (its low four bits, inverted);
    /// on Jr.'s board the command latch's low five bits, whatever the address.
    fn tune_read(snd: &SoundIo, rom: &[u8], jr: bool, addr: u8) -> u8 {
        if jr {
            return snd.command & 0x1F;
        }
        let page = Hw::p2_pins(snd, false) & 0x47;
        if page & 0x40 != 0 {
            (snd.command & 0x0F) ^ 0x0F
        } else {
            rom.get(0x1000 + usize::from(page & 7) * 256 + usize::from(addr)).copied().unwrap_or(0)
        }
    }

    // ---- The 8257's registers --------------------------------------------------------------

    fn dma_read(&mut self, offset: u16) -> u8 {
        let d = &mut self.dma;
        if offset & 8 == 0 {
            let ch = usize::from(offset >> 1 & 3);
            let v = if offset & 1 == 0 {
                if d.msb { (d.address[ch] >> 8) as u8 } else { d.address[ch] as u8 }
            } else if d.msb {
                let mut v = (d.count[ch] >> 8) as u8;
                // Read back the way it was written: the board's modes are swapped.
                v |= match d.mode[ch] {
                    1 => 0x80,
                    2 => 0x40,
                    m => m << 6,
                };
                v
            } else {
                d.count[ch] as u8
            };
            d.msb = !d.msb;
            v
        } else if offset == 8 {
            let v = d.status;
            d.status &= 0xF0;
            v
        } else {
            0
        }
    }

    /// A register write. Returns true when a request now meets an enabled channel, which in
    /// MAME makes the Z80 give up the rest of its timeslice and wakes the 8257.
    fn dma_write(&mut self, offset: u16, val: u8) -> bool {
        let d = &mut self.dma;
        if offset & 8 == 0 {
            let ch = usize::from(offset >> 1 & 3);
            if offset & 1 == 0 {
                d.address[ch] = if d.msb {
                    u16::from(val) << 8 | (d.address[ch] & 0xFF)
                } else {
                    (d.address[ch] & 0xFF00) | u16::from(val)
                };
            } else if d.msb {
                d.count[ch] = u16::from(val & 0x3F) << 8 | (d.count[ch] & 0xFF);
                // The board wires MEMR and MEMW the other way round: a "write" channel reads.
                d.mode[ch] = match val >> 6 {
                    1 => 2,
                    2 => 1,
                    m => m,
                };
            } else {
                d.count[ch] = (d.count[ch] & 0xFF00) | u16::from(val);
            }
            d.msb = !d.msb;
        } else if offset == 8 {
            d.transfer_mode = val;
        }
        if d.transfer_mode & d.request & 0x0F != 0 {
            d.woken |= d.waiting;
            return true;
        }
        false
    }

    /// The channel the 8257 serves next: fixed priority (0 first) or, rotating, the one after
    /// the last served.
    fn dma_next(&mut self) -> bool {
        let d = &mut self.dma;
        let active = |ch: usize| d.request >> ch & 1 != 0 && d.transfer_mode >> ch & 1 != 0;
        let next = if d.transfer_mode & 0x10 != 0 {
            (1..=4).map(|i| (usize::from(d.last_channel) + i) % 4).find(|&ch| active(ch))
        } else {
            (0..4).find(|&ch| active(ch))
        };
        if let Some(ch) = next {
            d.current = ch as u8;
            d.last_channel = ch as u8;
        }
        next.is_some()
    }

    // ---- Memory ------------------------------------------------------------------------------

    /// A read without side effects (the DMA's, and the debugger's).
    fn peek_with(&self, rom: &[u8], addr: u16) -> u8 {
        let a = usize::from(addr);
        let jr = self.jr();
        match a {
            0x0000..=0x4FFF => rom[a],
            0x5000..=0x5FFF | 0x8000..=0x9FFF | 0xB000..=0xBFFF | 0xD000..=0xDFFF if jr => rom[a],
            0x6000..=0x6BFF => self.ram[a - 0x6000],
            0x6C00..=0x6FFF if jr => self.ram[a - 0x6000],
            0x7000..=0x73FF => self.sprite_ram[a - 0x7000],
            0x7400..=0x77FF => self.vram[a - 0x7400],
            0x7C00 => self.ports.in0,
            0x7C80 => self.ports.in1,
            0x7D00 => self.in2(),
            0x7D80 => self.ports.dsw0,
            _ => 0,
        }
    }

    /// IN2: on Donkey Kong's board bit 6 is port 2 bit 4 of the 8035, inverted, as far as the
    /// board's latch has it.
    fn in2(&self) -> u8 {
        if self.jr() {
            self.ports.in2
        } else {
            (self.ports.in2 & !0x40) | u8::from(self.snd.io.vp2 & 0x10 == 0) << 6
        }
    }
}

/// The Z80's view of the board. Reads are answered at once; writes are kept, to be placed in
/// time once the instruction's shape is known.
struct View<'a> {
    hw: &'a mut Hw,
    rom: &'a Rom,
}

impl Bus for View<'_> {
    fn read(&mut self, addr: u16) -> u8 {
        self.hw.cycles.push((b'R', addr));
        match addr {
            0x7800..=0x780F => self.hw.dma_read(addr & 0xF),
            _ => self.hw.peek_with(&self.rom.program, addr),
        }
    }

    fn fetch(&mut self, addr: u16) -> u8 {
        let v = self.hw.peek_with(&self.rom.program, addr);
        self.hw.cycles.push((b'M', addr));
        if self.hw.fetches < 2 {
            self.hw.opcode[usize::from(self.hw.fetches)] = v;
            self.hw.fetches += 1;
        }
        v
    }

    fn write(&mut self, addr: u16, val: u8) {
        self.hw.cycles.push((b'W', addr));
        self.hw.writes.push((addr, val));
    }

    fn input(&mut self, port: u16) -> u8 {
        self.hw.cycles.push((b'I', port));
        0
    }

    fn output(&mut self, port: u16, _val: u8) {
        self.hw.cycles.push((b'I', port));
    }
}

/// The 8035's view of the board.
struct SoundView<'a> {
    io: &'a mut SoundIo,
    rom: &'a [u8],
    jr: bool,
    /// Port 1 writes (the DAC) during the instruction.
    out: &'a mut Vec<u8>,
}

impl mcs48::Bus for SoundView<'_> {
    fn program(&mut self, addr: u16) -> u8 {
        // Donkey Kong's 2 KB answers in both banks (MAME loads it twice); Jr.'s 4 KB fills both.
        self.rom.get(usize::from(addr & 0xFFF)).copied().unwrap_or(0)
    }
    fn movx_read(&mut self, addr: u8) -> u8 {
        Hw::tune_read(self.io, self.rom, self.jr, addr)
    }
    fn bus_read(&mut self) -> u8 {
        if self.jr {
            0xFF
        } else {
            Hw::tune_read(self.io, self.rom, false, 0)
        }
    }
    fn port_read(&mut self, port: u8) -> u8 {
        if port == 2 {
            Hw::p2_pins(self.io, self.jr)
        } else {
            0xFF
        }
    }
    fn port_write(&mut self, port: u8, val: u8) {
        match port {
            1 => self.out.push(val),
            _ => self.io.p2_written = Some(val),
        }
    }
    fn t0(&mut self) -> bool {
        self.io.latch6h & 0x20 == 0
    }
    fn t1(&mut self) -> bool {
        self.io.latch6h & 0x10 == 0
    }
}

impl Dkong {
    pub fn new(variant: Variant, roms: &Roms) -> Result<Dkong, String> {
        let jr = variant == Variant::DonkeyKongJr;
        let need = |name: &str, v: &[u8], len: usize| {
            if v.len() < len {
                Err(format!("the {name} ROMs are {} bytes, not {len}", v.len()))
            } else {
                Ok(())
            }
        };
        need("program", &roms.program, 0x10000)?;
        need("sound", &roms.sound, if jr { 0x1000 } else { 0x1800 })?;
        need("tile", &roms.tiles, if jr { 0x2000 } else { 0x1000 })?;
        need("sprite", &roms.sprites, 0x2000)?;
        need("colour", &roms.proms, 0x300)?;
        let sprites = if roms.sprites.len() >= 0x4000 { 0x4000 } else { 0x2000 };
        let rom = Rom {
            program: roms.program[..0x10000].to_vec(),
            sound: roms.sound.clone(),
            tiles: roms.tiles[..if jr { 0x2000 } else { 0x1000 }].to_vec(),
            sprites: roms.sprites[..sprites].to_vec(),
            colour_codes: roms.proms[0x200..0x300].to_vec(),
            palette: palette(&roms.proms),
        };
        let mut snd_cpu = Mcs48::new(Model::I8035);
        snd_cpu.reset();
        let hw = Hw {
            variant,
            ports: Ports::default(),
            ram: vec![0; 0x1000],
            sprite_ram: vec![0; 0x400],
            vram: vec![0; 0x400],
            flip: false,
            sprite_bank: 0,
            palette_bank: 0,
            gfx_bank: 0,
            nmi_mask: false,
            nmi_line: false,
            dma: Dma::default(),
            snd: SoundBoard { cpu: snd_cpu, cycles: 0, io: SoundIo::default() },
            sound: Sound::new(jr),
            sound_events: Vec::new(),
            samples: 0,
            latch6h: 0,
            latch4h: 0,
            latch5h: 0,
            watchdog: None,
            now: 0,
            tail: None,
            busreq: false,
            busack: false,
            base: 0,
            perfect_until: None,
            fired: false,
            pending: Vec::new(),
            frames: 0,
            cycles: Vec::new(),
            writes: Vec::new(),
            opcode: [0; 2],
            fetches: 0,
            times: Vec::new(),
            stops: Vec::new(),
        };
        let mut d = Dkong {
            cpu: Z80::new(),
            hw,
            rom,
            line: [0; RASTER_W],
            video: vec![0; WIDTH * HEIGHT],
            audio: vec![0; SAMPLES_PER_FRAME],
        };
        d.sound_cpu_reset(0);
        Ok(d)
    }

    /// The 8035's reset: its ports go to all ones, and port 2 reaches the board's latch (which
    /// the reset left at 0) at once. The sound circuits are reset after it (`Input::Reset`).
    fn sound_cpu_reset(&mut self, at: i128) {
        self.hw.snd.cpu.reset();
        self.hw.snd.io.p2_written = None;
        self.hw.snd.io.vp2 = 0xFF;
        self.hw.sound_events.push((at, Input::Reset));
    }

    pub fn variant(&self) -> Variant {
        self.hw.variant
    }

    pub fn set_ports(&mut self, ports: Ports) {
        self.hw.ports = ports;
    }

    pub fn ports(&self) -> Ports {
        self.hw.ports
    }

    /// The last frame, upright, 0x00RRGGBB.
    pub fn video(&self) -> &[u32] {
        &self.video
    }

    /// The last frame's sound: 792 samples at 48 kHz.
    pub fn audio(&self) -> &[i16] {
        &self.audio
    }

    pub fn cpu(&self) -> &Z80 {
        &self.cpu
    }

    pub fn sound_cpu(&self) -> &Mcs48 {
        &self.hw.snd.cpu
    }

    /// The 8035's machine cycles since power-on.
    pub fn sound_cycles(&self) -> u64 {
        self.hw.snd.cycles
    }

    pub fn frame_count(&self) -> u64 {
        self.hw.frames
    }

    /// A byte as the Z80 would read it, without side effects.
    pub fn peek(&self, addr: u16) -> u8 {
        self.hw.peek_with(&self.rom.program, addr)
    }

    /// The latches the analogue effects hang on, and the DAC: latch 6H, the 8035's port 1 and
    /// port 2.
    pub fn sound_state(&self) -> (u8, u8, u8) {
        (self.hw.snd.io.latch6h, self.hw.snd.cpu.p1, self.hw.snd.io.vp2)
    }

    pub fn run_frame(&mut self) {
        self.frame(&mut |_, _| {});
    }

    /// The same, calling `trace` before each 8035 instruction with the CPU and its cycles.
    pub fn run_frame_traced(&mut self, trace: &mut dyn FnMut(&Mcs48, u64)) {
        self.frame(trace);
    }

    fn frame(&mut self, trace: &mut dyn FnMut(&Mcs48, u64)) {
        let end = i128::from(self.hw.frames + 1) * AS_PER_FRAME;
        while self.hw.base < end {
            self.slice(trace);
        }
        self.hw.frames += 1;
        self.make_sound();
    }

    /// The next timer after the timeslice's start: a raster line, the sound update, the watchdog.
    fn next_timer(&self) -> i128 {
        let b = self.hw.base;
        let mut t = ((b / AS_PER_LINE + 1) * AS_PER_LINE).min((b / SOUND_UPDATE + 1) * SOUND_UPDATE);
        if let Some(w) = self.hw.watchdog {
            t = t.min(w.max(b));
        }
        t
    }

    /// One of MAME's timeslices: the Z80, the 8257 and the 8035 each run up to its end (which a
    /// write through the scheduler brings forward), then what was waiting for it lands and the
    /// timers due fire.
    fn slice(&mut self, trace: &mut dyn FnMut(&Mcs48, u64)) {
        let base = self.hw.base;
        // MAME drops an expired quantum when its scheduler comes back from firing timers.
        if self.hw.fired && self.hw.perfect_until.is_some_and(|p| base >= p) {
            self.hw.perfect_until = None;
        }
        self.hw.fired = false;
        // A trigger that came while the 8257 waited takes effect from this timeslice.
        if self.hw.dma.woken {
            self.hw.dma.woken = false;
            self.hw.dma.waiting = false;
        }
        let quantum = if self.hw.perfect_until.is_some() { PERFECT } else { QUANTUM };
        let next = self.next_timer();
        let mut target = (base + quantum).min(next);
        // MAME runs its chips in the order the machine made them (the Z80, the 8257, the 8035),
        // with one that is waiting moved to the end. Each chip given at least a whole cycle runs
        // (or, waiting, counts the cycles off) and ends where it got to; one that ends short of
        // the end (it stopped, or the end isn't on its clock) brings the end back to that point,
        // never before the start.
        let pull = |target: &mut i128, local: i128| {
            if local < *target {
                *target = local.max(base);
            }
        };
        let dma_active = !self.hw.dma.waiting;
        if target - Hw::time(self.hw.now) >= AS_PER_CYCLE {
            self.run_z80(target);
            pull(&mut target, Hw::time(self.hw.now));
        }
        if dma_active && target - Hw::time(self.hw.dma.now) >= AS_PER_CYCLE {
            self.run_dma(target);
            pull(&mut target, Hw::time(self.hw.dma.now));
        }
        if target - i128::from(self.hw.snd.cycles) * AS_PER_SOUND_CYCLE >= AS_PER_SOUND_CYCLE {
            self.run_sound(target, trace);
            pull(&mut target, i128::from(self.hw.snd.cycles) * AS_PER_SOUND_CYCLE);
        }
        if !dma_active && target - Hw::time(self.hw.dma.now) >= AS_PER_CYCLE {
            self.run_dma(target);
            pull(&mut target, Hw::time(self.hw.dma.now));
        }
        self.hw.base = target;
        if !self.hw.pending.is_empty() {
            self.hw.fired = true;
            let mut pending = std::mem::take(&mut self.hw.pending);
            pending.sort_by_key(|p| p.0);
            for (_, s) in pending.drain(..) {
                self.land(s, target);
            }
            self.hw.pending = pending;
        }
        if target >= next {
            self.hw.fired = true;
            self.timers(target);
        }
    }

    /// A write through the scheduler lands, at the end of its timeslice `at`.
    fn land(&mut self, s: Sync, at: i128) {
        let hw = &mut self.hw;
        match s {
            Sync::Command(v) => hw.snd.io.command = v,
            Sync::Latch6h(v) => {
                hw.snd.io.latch6h = v;
                hw.sound_events.push((at, Input::Latch6h(v)));
            }
            Sync::Latch4h(v) => hw.snd.io.latch4h = v,
            Sync::Latch5h(v) => hw.sound_events.push((at, Input::Sound9(v & 2 != 0))),
            Sync::Irq(on) => hw.snd.cpu.set_irq(on),
            Sync::NmiClear => hw.nmi_line = false,
            Sync::BusReq(on) => hw.busreq = on,
            Sync::Port2(v) => {
                if (v ^ hw.snd.io.vp2) & 0x80 != 0 {
                    hw.sound_events.push((at, Input::Decay(v & 0x80 != 0)));
                }
                hw.snd.io.vp2 = v;
            }
        }
    }

    /// The timers due at `at`: a raster line drawn (and at its end VBLANK raising the NMI while
    /// the NMI latch is on), the watchdog.
    fn timers(&mut self, at: i128) {
        if at % AS_PER_LINE == 0 {
            let n = (at / AS_PER_LINE) % 264;
            if n == 0 {
                // VBLANK: the NMI line rises (the Z80's NMI is edge-triggered).
                if self.hw.nmi_mask && !self.hw.nmi_line {
                    self.hw.nmi_line = true;
                    self.cpu.nmi();
                }
            } else {
                let line = ((240 + n) % 264) as usize;
                if (FIRST_LINE..=LAST_LINE).contains(&line) {
                    self.draw_line(line);
                }
            }
        }
        if self.hw.watchdog.is_some_and(|w| w <= at) {
            self.watchdog_reset(at);
        }
    }

    // ---- The Z80 -------------------------------------------------------------------------------

    /// The Z80's turn in the timeslice ending at `target`: as many cycles as fit, from where it
    /// is (it may stop a little past them, at the end of a bus cycle). Returns where it stopped
    /// if a write through the scheduler cut the timeslice short.
    fn run_z80(&mut self, target: i128) -> Option<i128> {
        let local = Hw::time(self.hw.now);
        if target - local < AS_PER_CYCLE {
            return None;
        }
        let budget = self.hw.now + cycles_in(target - local, AS_PER_CYCLE);
        if let Some(stop) = self.finish_tail(budget) {
            return Some(stop);
        }
        while self.hw.tail.is_none() && self.hw.now < budget {
            // The 8257 has asked for the bus: the Z80 lets go at the start of an instruction
            // and sits out the rest of the timeslice.
            if self.hw.busreq {
                if !self.hw.busack {
                    self.hw.busack = true;
                    self.hlda(true);
                }
                self.hw.now = budget;
                return None;
            }
            if self.hw.busack {
                self.hw.busack = false;
                self.hlda(false);
            }
            if let Some(stop) = self.step(budget) {
                return Some(stop);
            }
        }
        None
    }

    /// The 8257 hears that it has the bus (or has lost it).
    fn hlda(&mut self, on: bool) {
        self.hw.dma.hack = on;
        self.hw.dma.woken |= self.hw.dma.waiting;
    }

    /// Carries on with an instruction the Z80 stopped in, up to `budget`.
    fn finish_tail(&mut self, budget: u64) -> Option<i128> {
        match self.hw.tail.take() {
            None => None,
            Some(Tail::Redo { start }) => self.run_instruction(start, budget, true),
            Some(Tail::Rest { stops, writes }) => self.carry_on(stops, writes, budget),
        }
    }

    /// Goes on through an instruction's remaining `stops` and `writes` up to `budget`: writes
    /// land at their bus cycles, and one through the scheduler stops the Z80 at that cycle's end.
    /// One that makes the 8257 start eats the rest of the timeslice instead: the write cycle
    /// ends three cycles after the timeslice's, and the rest of the instruction comes that much
    /// later.
    fn carry_on(&mut self, stops: Vec<u64>, writes: Vec<(u64, u16, u8)>, budget: u64) -> Option<i128> {
        let end = *stops.last().expect("an instruction has an end");
        let pause = stops.iter().copied().find(|&p| p >= budget).unwrap_or(end);
        for (i, &(at, addr, val)) in writes.iter().enumerate() {
            if at >= pause {
                self.hw.tail = Some(Tail::Rest { stops: stops.into_iter().filter(|&p| p > pause).collect(), writes: writes[i..].to_vec() });
                self.hw.now = pause;
                return None;
            }
            match self.apply_write(addr, val, at) {
                Cut::On => {}
                Cut::Stop => {
                    let stop = at + 3;
                    if stop < end {
                        self.hw.tail = Some(Tail::Rest { stops: stops.into_iter().filter(|&p| p > stop).collect(), writes: writes[i + 1..].to_vec() });
                    }
                    self.hw.now = stop;
                    return Some(Hw::time(stop));
                }
                Cut::Eat => {
                    let late = budget.max(at) - at;
                    let stop = at + 3 + late;
                    if at + 3 < end {
                        self.hw.tail = Some(Tail::Rest {
                            stops: stops.into_iter().filter(|&p| p > at + 3).map(|p| p + late).collect(),
                            writes: writes[i + 1..].iter().map(|&(t, a, v)| (t + late, a, v)).collect(),
                        });
                    }
                    self.hw.now = stop;
                    return None;
                }
            }
        }
        if pause < end {
            self.hw.tail = Some(Tail::Rest { stops: stops.into_iter().filter(|&p| p > pause).collect(), writes: Vec::new() });
        }
        self.hw.now = pause;
        None
    }

    /// One Z80 instruction (or the interrupt response before one) from `now`.
    fn step(&mut self, budget: u64) -> Option<i128> {
        let start = self.hw.now;
        self.run_instruction(start, budget, false)
    }

    /// Runs the instruction that starts at `start` and places it in time against `budget`. When
    /// it is `again` (it had started already), no interrupt is taken before it.
    fn run_instruction(&mut self, start: u64, budget: u64, again: bool) -> Option<i128> {
        self.hw.cycles.clear();
        self.hw.writes.clear();
        self.hw.fetches = 0;
        let saved = (self.cpu.clone(), self.hw.dma.msb, self.hw.dma.status, self.hw.watchdog);
        let mut view = View { hw: &mut self.hw, rom: &self.rom };
        let mut t = if again { 0 } else { self.cpu.take_interrupt(&mut view) };
        if t == 0 {
            t = self.cpu.step(&mut view);
        }
        let block = self.hw.opcode[0] == 0xED && matches!(self.hw.opcode[1], 0xA0 | 0xA8 | 0xB0 | 0xB8);
        let mut times = std::mem::take(&mut self.hw.times);
        let mut stops = std::mem::take(&mut self.hw.stops);
        bus_layout(start, t, &self.hw.cycles, block, &mut times, &mut stops);
        let end = start + u64::from(t);
        let pause = if end <= budget { end } else { stops.iter().copied().find(|&p| p >= budget).unwrap_or(end) };
        // A read of something another chip changes, after the point where the Z80 stops, with
        // nothing written before it: the Z80 makes it in a later timeslice, so it runs again.
        let late_read = self.hw.cycles.iter().zip(&times).position(|(&(k, a), &at)| {
            k == b'R' && (a == 0x7D00 || (0x7800..=0x780F).contains(&a)) && at >= pause
        });
        let first_write = self.hw.cycles.iter().position(|&(k, _)| k == b'W');
        if let Some(r) = late_read {
            if first_write.is_none_or(|w| w > r) {
                self.cpu = saved.0;
                self.hw.dma.msb = saved.1;
                self.hw.dma.status = saved.2;
                self.hw.watchdog = saved.3;
                self.hw.tail = Some(Tail::Redo { start });
                self.hw.now = pause;
                self.hw.times = times;
                self.hw.stops = stops;
                return None;
            }
        }
        // Reading IN2 feeds the watchdog, from the read's own bus cycle.
        if let Some(i) = self.hw.cycles.iter().position(|&(k, a)| k == b'R' && a == 0x7D00) {
            self.hw.watchdog = Some(Hw::time(times[i]) + WATCHDOG);
        }
        let writes: Vec<(u64, u16, u8)> = self
            .hw
            .cycles
            .iter()
            .zip(&times)
            .filter(|(&(k, _), _)| k == b'W')
            .map(|(_, &at)| at)
            .zip(self.hw.writes.iter().copied())
            .map(|(at, (a, v))| (at, a, v))
            .collect();
        let rest: Vec<u64> = stops.iter().copied().filter(|&p| p > start).collect();
        self.hw.times = times;
        self.hw.stops = stops;
        self.carry_on(rest, writes, budget)
    }

    // ---- The 8257 ------------------------------------------------------------------------------

    /// The 8257's turn: one state a cycle, as MAME's i8257 steps them. Returns where it stopped
    /// if it cut the timeslice short (asking for the bus, or giving it back).
    fn run_dma(&mut self, target: i128) -> Option<i128> {
        let local = Hw::time(self.hw.dma.now);
        if target - local < AS_PER_CYCLE {
            return None;
        }
        let n = cycles_in(target - local, AS_PER_CYCLE);
        if self.hw.dma.waiting {
            // Waiting, its clock runs on.
            self.hw.dma.now += n;
            return None;
        }
        let end = self.hw.dma.now + n;
        loop {
            let at = self.hw.dma.now;
            let mut stop = false;
            match self.hw.dma.state {
                DmaState::Si => {
                    self.hw.dma.tc = false;
                    if self.hw.dma_next() {
                        self.hw.dma.state = DmaState::S0;
                    } else {
                        // Nothing to do: it waits, and its turn is over.
                        self.hw.dma.waiting = true;
                        self.hw.dma.woken = false;
                        self.hw.dma.now = end + 1;
                        return None;
                    }
                }
                DmaState::S0 => {
                    // Asking for the bus goes through the scheduler and ends its turn.
                    if !self.hw.dma.hreq {
                        self.hw.dma.hreq = true;
                        self.hw.pending.push((Hw::time(at), Sync::BusReq(true)));
                        stop = true;
                    }
                    if self.hw.dma.hack {
                        self.hw.dma.state = DmaState::S1;
                    } else {
                        // It waits for the Z80 to let go.
                        self.hw.dma.waiting = true;
                        self.hw.dma.woken = false;
                        if stop {
                            self.hw.dma.now = at + 1;
                            return Some(Hw::time(at + 1));
                        }
                        self.hw.dma.now = end + 1;
                        return None;
                    }
                }
                DmaState::S1 => {
                    self.hw.dma.tc = false;
                    self.hw.dma.state = DmaState::S2;
                }
                DmaState::S2 => self.hw.dma.state = DmaState::S3,
                DmaState::S3 => {
                    let ch = usize::from(self.hw.dma.current & 3);
                    let addr = self.hw.dma.address[ch];
                    match self.hw.dma.mode[ch] {
                        1 => self.hw.dma.temp = if ch == 1 { self.hw.dma.latch } else { 0 },
                        2 => self.hw.dma.temp = self.hw.peek_with(&self.rom.program, addr),
                        _ => {}
                    }
                    if self.hw.dma.count[ch] == 0 && self.hw.dma.mode[ch] != 2 {
                        self.hw.dma.tc = true;
                    }
                    self.hw.dma.state = DmaState::S4;
                }
                DmaState::S4 => {
                    let ch = usize::from(self.hw.dma.current & 3);
                    let addr = self.hw.dma.address[ch];
                    let v = self.hw.dma.temp;
                    match self.hw.dma.mode[ch] {
                        1 => {
                            self.apply_write(addr, v, at);
                        }
                        2 => {
                            if ch == 0 {
                                self.hw.dma.latch = v;
                            }
                        }
                        _ => {}
                    }
                    let d = &mut self.hw.dma;
                    if d.count[ch] == 0 && d.mode[ch] == 2 {
                        d.tc = true;
                    }
                    if d.tc {
                        d.status |= 1 << ch;
                        if d.transfer_mode & 0x40 != 0 {
                            d.transfer_mode &= !(1 << ch);
                        }
                    }
                    d.tc = false;
                    d.count[ch] = d.count[ch].wrapping_sub(1) & 0x3FFF;
                    d.address[ch] = d.address[ch].wrapping_add(1);
                    if self.hw.dma.hack && self.hw.dma_next() {
                        self.hw.dma.state = DmaState::S1;
                    } else {
                        // Done: it gives the bus back.
                        self.hw.dma.hreq = false;
                        self.hw.pending.push((Hw::time(at), Sync::BusReq(false)));
                        self.hw.dma.current = 0xFF;
                        self.hw.dma.state = DmaState::Si;
                        stop = true;
                    }
                }
            }
            self.hw.dma.now = at + 1;
            if stop {
                return Some(Hw::time(at + 1));
            }
            if self.hw.dma.now >= end {
                return None;
            }
        }
    }

    // ---- The 8035 ------------------------------------------------------------------------------

    /// The 8035's turn: instructions while there are cycles left (at least one). A port 2 write
    /// goes through the scheduler and ends its turn.
    fn run_sound(&mut self, target: i128, trace: &mut dyn FnMut(&Mcs48, u64)) -> Option<i128> {
        let local = i128::from(self.hw.snd.cycles) * AS_PER_SOUND_CYCLE;
        if target - local < AS_PER_SOUND_CYCLE {
            return None;
        }
        let budget = self.hw.snd.cycles + cycles_in(target - local, AS_PER_SOUND_CYCLE);
        let jr = self.hw.jr();
        let mut out = Vec::new();
        loop {
            let mut bus = SoundView { io: &mut self.hw.snd.io, rom: &self.rom.sound, jr, out: &mut out };
            let cpu = &mut self.hw.snd.cpu;
            let mut c = cpu.interrupt(&mut bus);
            trace(cpu, self.hw.snd.cycles + u64::from(c));
            c += cpu.instruction(&mut bus);
            self.hw.snd.cycles += u64::from(c);
            let end = i128::from(self.hw.snd.cycles) * AS_PER_SOUND_CYCLE;
            for v in out.drain(..) {
                self.hw.sound_events.push((end, Input::Dac(v)));
            }
            if let Some(v) = self.hw.snd.io.p2_written.take() {
                self.hw.pending.push((end, Sync::Port2(v)));
                return Some(end);
            }
            if self.hw.snd.cycles >= budget {
                return None;
            }
        }
    }

    /// A Z80 (or DMA) write at cycle `at`, by the board's address decoding, and what it does to
    /// the Z80's timeslice.
    fn apply_write(&mut self, addr: u16, val: u8, at: u64) -> Cut {
        let hw = &mut self.hw;
        let jr = hw.jr();
        let a = usize::from(addr);
        let when = Hw::time(at);
        let bit = val & 1 != 0;
        let set = |latch: u8, n: usize| (latch & !(1 << n)) | u8::from(bit) << n;
        let mut cut = Cut::On;
        match a {
            0x6000..=0x6BFF => hw.ram[a - 0x6000] = val,
            0x6C00..=0x6FFF if jr => hw.ram[a - 0x6000] = val,
            0x7000..=0x73FF => hw.sprite_ram[a - 0x7000] = val,
            0x7400..=0x77FF => hw.vram[a - 0x7400] = val,
            0x7800..=0x780F => {
                if hw.dma_write(addr & 0xF, val) {
                    // The Z80 gives up the rest of its timeslice (MAME eats its cycles).
                    return Cut::Eat;
                }
            }
            0x7C00 => {
                hw.pending.push((when, Sync::Command(val)));
                cut = Cut::Stop;
            }
            // On Jr.'s board 7C80h-7C87h are latch 4H's bits (bit 1 reaches the 8035's port 2
            // bit 6), except that 7C80h is the tile bank (MAME's map puts that over the latch).
            0x7C80 if jr => hw.gfx_bank = val & 1,
            0x7C81..=0x7C87 if jr => {
                hw.latch4h = set(hw.latch4h, a & 7);
                hw.pending.push((when, Sync::Latch4h(hw.latch4h)));
                cut = Cut::Stop;
            }
            0x7D00..=0x7D07 => {
                hw.latch6h = set(hw.latch6h, a & 7);
                hw.pending.push((when, Sync::Latch6h(hw.latch6h)));
                cut = Cut::Stop;
            }
            0x7D80 => {
                hw.pending.push((when, Sync::Irq(val != 0)));
                cut = Cut::Stop;
            }
            // Jr.: latch 5H's bit 1, the fall (the rest of 5H is under the board's other
            // switches in MAME's map).
            0x7D81 if jr => {
                hw.latch5h = set(hw.latch5h, 1);
                hw.pending.push((when, Sync::Latch5h(hw.latch5h)));
                cut = Cut::Stop;
            }
            0x7D82 => hw.flip = bit,
            0x7D83 => hw.sprite_bank = val & 1,
            0x7D84 => {
                hw.nmi_mask = bit;
                if !bit {
                    hw.pending.push((when, Sync::NmiClear));
                    cut = Cut::Stop;
                }
            }
            0x7D85 => {
                // The DMA request: the 8257 wakes, the Z80 stops at once, and MAME runs the
                // chips a cycle at a time for the next 100 µs.
                hw.dma.request = (hw.dma.request & !3) | if bit { 3 } else { 0 };
                hw.dma.woken |= hw.dma.waiting;
                let until = when + PERFECT_FOR;
                hw.perfect_until = Some(hw.perfect_until.map_or(until, |p| p.max(until)));
                cut = Cut::Stop;
            }
            0x7D86 | 0x7D87 => {
                let n = a - 0x7D86;
                hw.palette_bank = (hw.palette_bank & !(1 << n)) | u8::from(bit) << n;
            }
            _ => {}
        }
        cut
    }

    /// The watchdog ran out: MAME's soft reset, which resets every device where it lands. The Z80
    /// restarts at 0000h from where it stopped (the rest of its instruction is lost); the 8035
    /// too, its ports back to all ones; the latches to 0 (without telling the sound circuits,
    /// which MAME resets on their own); the 8257. Memory and the video latches keep what they
    /// held, and the watchdog waits for the next read of IN2.
    fn watchdog_reset(&mut self, at: i128) {
        self.hw.tail = None;
        self.cpu.reset();
        self.hw.watchdog = None;
        self.hw.pending.clear();
        self.hw.busreq = false;
        self.hw.busack = false;
        let dma_now = self.hw.dma.now;
        self.hw.dma = Dma { now: dma_now, waiting: true, ..Dma::default() };
        self.hw.snd.io.command = 0;
        self.hw.snd.io.latch6h = 0;
        self.hw.snd.io.latch4h = 0;
        self.hw.latch6h = 0;
        self.hw.latch4h = 0;
        self.hw.latch5h = 0;
        self.hw.snd.cpu.set_irq(false);
        self.sound_cpu_reset(at);
    }

    /// One line of the picture, as the beam draws it.
    fn draw_line(&mut self, line: usize) {
        let hw = &self.hw;
        let rom = &self.rom;
        let pal = &rom.palette;
        let flip = hw.flip;
        // The tiles: the map is turned round, both ways, when the screen is flipped.
        let ty = if flip { 255 - line } else { line };
        for x in 0..RASTER_W {
            let tx = if flip { 255 - x } else { x };
            let index = (ty / 8) * 32 + tx / 8;
            let code = usize::from(hw.vram[index]) + 256 * usize::from(hw.gfx_bank);
            let colour = usize::from(rom.colour_codes[index % 32 + 32 * (index / 32 / 4)] & 0x0F)
                + 0x10 * usize::from(hw.palette_bank);
            let px = rom.tile_pixel(code, tx % 8, ty % 8);
            self.line[x] = pal[(colour * 4 + usize::from(px)) & 0xFF];
        }
        // The sprites: the first 16 whose rows cover the line, chosen during the line before.
        let mut vf = (line as u32).wrapping_sub(1) & 0xFF;
        let add_y: u32 = if flip { 0xF7 } else { 0xF9 };
        if flip {
            vf ^= 0xFF;
        }
        let base = usize::from(hw.sprite_bank) << 9;
        let mut shown = 0;
        let mut offs = base;
        while shown < 16 && offs < base + 0x200 {
            let s = &hw.sprite_ram[offs..offs + 4];
            offs += 4;
            let sum = u32::from(s[0]) + add_y + 1 + vf;
            if sum & 0xF0 != 0xF0 {
                continue;
            }
            shown += 1;
            let code = usize::from(s[1] & 0x7F) + (usize::from(s[2] & 0x40) << 1);
            let colour = usize::from(s[2] & 0x0F) + 16 * usize::from(hw.palette_bank);
            let mut flipx = s[2] & 0x80 != 0;
            let flipy = s[1] & 0x80 != 0;
            let mut x = ((u32::from(s[3]) + 0xF7 + 1) & 0xFF) as i32;
            if flip {
                x = (x ^ 0xFF) - 15;
                flipx = !flipx;
            }
            let row = (sum & 0x0F) as usize;
            let sy = if flipy { 15 - row } else { row };
            let wrap = if flip { x + 256 } else { x - 256 };
            for origin in [x, wrap] {
                for i in 0..16 {
                    let px = origin + i;
                    if !(0..RASTER_W as i32).contains(&px) {
                        continue;
                    }
                    let sx = if flipx { 15 - i as usize } else { i as usize };
                    let p = rom.sprite_pixel(code, sx, sy);
                    if p != 0 {
                        self.line[px as usize] = pal[(colour * 4 + usize::from(p)) & 0xFF];
                    }
                }
            }
        }
        // Turned a quarter left in the cabinet: the raster's top is the screen's left.
        let col = line - FIRST_LINE;
        for (x, &p) in self.line.iter().enumerate() {
            self.video[(RASTER_W - 1 - x) * WIDTH + col] = p;
        }
    }

    /// The frame's sound: each change lands on the samples from its moment on.
    fn make_sound(&mut self) {
        let mut events = std::mem::take(&mut self.hw.sound_events);
        events.sort_by_key(|e| e.0);
        let mut next = events.into_iter().peekable();
        self.audio.clear();
        for _ in 0..SAMPLES_PER_FRAME {
            let at = i128::from(self.hw.samples) * AS_PER_SECOND / i128::from(SAMPLE_RATE);
            while let Some(&(_, e)) = next.peek().filter(|e| e.0 <= at) {
                self.hw.sound.set(e);
                next.next();
            }
            let s = self.hw.sound.next();
            self.audio.push((s.clamp(-1.0, 1.0) * 32767.0) as i16);
            self.hw.samples += 1;
        }
        self.hw.sound_events = next.collect();
    }

    pub fn save_state(&self) -> Vec<u8> {
        bincode::serialize(&(&self.cpu, &self.hw, &self.video)).expect("a board state always serialises")
    }

    pub fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        let (cpu, hw, video): (Z80, Hw, Vec<u32>) =
            bincode::deserialize(data).map_err(|e| format!("not a Donkey Kong board state: {e}"))?;
        if hw.variant != self.hw.variant || video.len() != WIDTH * HEIGHT {
            return Err("that state is from a different board".into());
        }
        self.cpu = cpu;
        self.hw = hw;
        self.video = video;
        Ok(())
    }
}
