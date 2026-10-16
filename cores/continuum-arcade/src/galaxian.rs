//! Namco's Galaxian board and its family: Galaxian and the clones on its board, Nichibutsu's Moon
//! Cresta (the same board with its memory moved and a graphics bank), and Konami's Scramble board
//! (Scramble, Super Cobra, Frogger, Amidar and their clones), which adds two 8255 PPIs for the
//! inputs and a sound board of its own (`konami_sound`).
//!
//! A Z80 at 3.072 MHz (18.432 MHz / 6). The monitor is on its side: its raster is 256 × 224 at a
//! 6.144 MHz pixel clock, 384 × 264 clocks a frame (60.606 Hz), and the player sees it turned a
//! quarter turn, 224 × 256. The star generator runs on the 18.432 MHz clock and changes colour a
//! third of the way into a pixel, so the picture is kept at three dots a pixel along the raster,
//! as MAME draws it: 768 × 224, or 224 × 768 upright.
//!
//! The picture is a 32 × 32 layer of 8 × 8 tiles (two bitplanes in two ROMs), each column of
//! which scrolls up and down by itself and takes its colour from object RAM, eight 16 × 16
//! sprites, eight one-line shots (seven white "shells" and a yellow "missile"; Konami's boards
//! draw them as two yellow dots), and behind it all the star field (a 17-bit shift register whose
//! states pick which dots light up and in which of 64 colours), or on Konami's boards a blue
//! background, a blue river or a plain colour. Colours come from a 32-byte PROM through resistors.
//! VBLANK raises the NMI when enabled, and a watchdog resets the board after 8 frames without a
//! read of its address.
//!
//! The board draws as the beam goes. Like MAME, cArcade draws the lines the beam has already
//! passed whenever the game writes to video or object RAM or changes a setting that shows, so a
//! game that changes the picture mid-frame splits it where the real one would.
//!
//! Written from the boards' documented behaviour and from how MAME's galaxian driver models them
//! (the reference this board's tests compare against, `tests/galaxian_mame.rs`).

use serde::{Deserialize, Serialize};

use crate::z80::{Bus, Z80};

/// The picture as the player sees it, upright: three rows for each of the raster's dots.
pub const WIDTH: usize = 224;
pub const HEIGHT: usize = 768;
/// The raster: 256 dots drawn as three each, lines 16 to 239.
const RASTER_W: usize = 768;
const FIRST_LINE: usize = 16;
const LAST_LINE: usize = 239;
const RASTER_H: usize = LAST_LINE + 1 - FIRST_LINE;

pub const CPU_HZ: u32 = 3_072_000;
/// 384 × 264 pixel clocks a frame, and the Z80 runs at half the pixel clock.
pub const CYCLES_PER_FRAME: u32 = 384 * 264 / 2;
const CYCLES_PER_LINE: u64 = 192;
/// 6.144 MHz / (384 × 264): 60.606 frames a second.
pub const FPS: f64 = 6_144_000.0 / (384.0 * 264.0);
/// The sound handed out: 48 kHz, 792 samples a frame.
pub const SAMPLE_RATE: u32 = 48_000;
pub const SAMPLES_PER_FRAME: usize =
    (CYCLES_PER_FRAME as u64 * SAMPLE_RATE as u64 / CPU_HZ as u64) as usize;

/// The star generator's shift register repeats after 2^17 - 1 steps.
const STAR_PERIOD: u32 = (1 << 17) - 1;

/// Time as MAME counts it, in attoseconds: a main CPU cycle, an 18.432 MHz dot, a line and a
/// frame. The frame is a whole number of dots, each truncated to whole attoseconds, so it ends a
/// hair before the 50,688th CPU cycle does: VBLANK falls inside the frame's last CPU cycle, and
/// the end of VBLANK (line 16) inside cycle 7,679 of the next.
const AS_PER_CYCLE: i128 = 1_000_000_000_000_000_000 / CPU_HZ as i128;
const AS_PER_DOT: i128 = 1_000_000_000_000_000_000 / 18_432_000;
const AS_PER_FRAME: i128 = AS_PER_DOT * 1152 * 264;
const AS_PER_LINE: i128 = AS_PER_FRAME / 264;
/// Cycles into a frame at which the end of VBLANK counts MAME's frame number.
const VBLANK_END_CYCLE: u64 = 40 * CYCLES_PER_LINE - 1;

/// Which board a set runs on: its memory map, and how its picture is put together.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Variant {
    /// Namco's Galaxian and the clones on its board.
    Galaxian,
    /// Moon Cresta: ROM, RAM and video moved up, a graphics bank, and on Nichibutsu's own boards
    /// a program whose every byte is scrambled.
    MoonCresta { encrypted: bool },
}

/// Which way the monitor is turned in the cabinet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rotation {
    /// The raster's left edge at the top of the screen (most of the family).
    Rot90,
    /// The other way up: the raster's right edge at the top.
    Rot270,
}

/// What the set list hands the board: its ROM regions, laid out as on the board.
#[derive(Clone, Debug)]
pub struct Roms {
    /// The main Z80's program, 16 KB (32 KB on Konami's bigger boards).
    pub program: Vec<u8>,
    /// The tiles' and sprites' two bitplanes, one after the other: 4 KB, or 8 KB with a bank.
    pub gfx: Vec<u8>,
    /// The 32-byte colour PROM.
    pub palette: Vec<u8>,
}

/// The board's inputs as its ports read them (active high, as wired): IN0, IN1 and IN2, the last
/// of them all DIP switches on Namco's board.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ports {
    pub in0: u8,
    pub in1: u8,
    pub in2: u8,
    pub in3: u8,
}

/// Galaxian's sound latches: the board's analogue sound reads them (`galaxian_sound`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundLatches {
    /// The four LFO bits (6004h-6007h): the background's sweep.
    pub lfo: u8,
    /// FS1-FS3, HIT, (unused), FIRE, VOL1, VOL2 (6800h-6807h), one bit each.
    pub sound: u8,
    /// The pitch latch (7800h): the tone's divider.
    pub pitch: u8,
}

/// The board's state: what a save state holds besides the Z80.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Hw {
    variant: Variant,
    ram: Vec<u8>,
    vram: Vec<u8>,
    /// 256 bytes: 00h-3Fh each column's scroll and colour, 40h-5Fh the sprites, 60h-7Fh the
    /// shots, the rest unused RAM.
    objram: Vec<u8>,
    flip_x: bool,
    flip_y: bool,
    stars_enabled: bool,
    /// Where the star generator's sequence starts at the top-left of line 0, and the frame
    /// number that is for (MAME's model of the generator's free-running count).
    star_origin: u32,
    star_origin_frame: u32,
    /// Moon Cresta's graphics bank latches (A000h-A002h), whole bytes as MAME keeps them.
    gfxbank: [u8; 5],
    /// The NMI ON latch, and the NMI line (the Z80 takes an NMI on its rising edge).
    nmi_enabled: bool,
    nmi_line: bool,
    lamps: [bool; 2],
    /// The coin lockout (6002h): writing 0 energises the coin mechs' lockout coils, so a coin
    /// dropped in is turned away and never reaches the coin switch. Unlocked at power-on.
    coins_locked: bool,
    /// The lockout as it stood at this frame's VBLANK, which is when the coin inputs are read
    /// out for the frame (MAME samples its inputs, and the lockout with them, once a frame).
    coins_locked_now: bool,
    coin_counter: [bool; 2],
    sound: SoundLatches,
    /// VBLANKs left before the watchdog resets the board.
    watchdog: u8,
    ports: Ports,
    /// CPU cycles since power-on.
    now: u64,
    /// When the frame being run started; a frame runs from one VBLANK to the next.
    frame_start: u64,
    /// Frames finished since power-on.
    frames: u64,
    /// The machine has been running for a frame: the next frame starts with a VBLANK. (The first
    /// VBLANK comes one whole frame after power-on, as in MAME, where the raster starts there.)
    vblank_due: bool,
    /// The first raster line not yet drawn this frame.
    next_line: usize,
    /// The CPU cycle just after the last write that switched the NMI off. MAME drops the line
    /// through its scheduler, which cuts the CPU's timeslice there, so the slice that runs into
    /// VBLANK can start at that write rather than at the last timer (see `vblank_cycle`).
    sync_at: u64,
    /// Writes the last instruction of a frame made after VBLANK: they land in the next frame.
    late: Vec<(u16, u8, u64)>,
    /// Where this frame's CPU timeslice into VBLANK ended (`vblank_cycle`), and the last
    /// instruction run: its first cycle, its length and the kinds of its bus cycles in order
    /// (`M` an opcode fetch, `R` a read, `W` a write, `I` an I/O cycle). A watchdog reset at
    /// VBLANK cuts that instruction short at the first point after the timeslice where MAME's Z80
    /// can stop (`pause_point`).
    slice_end: u64,
    last_start: u64,
    last_len: u32,
    #[serde(skip)]
    last_cycles: Vec<u8>,
    #[serde(skip)]
    cycles: Vec<u8>,
    /// Writes made by the instruction being run, applied once its length is known.
    #[serde(skip)]
    writes: Vec<(u16, u8)>,
    /// The instruction's first two opcode bytes (a prefix and the opcode).
    #[serde(skip)]
    opcode: [u8; 2],
    #[serde(skip)]
    fetches: u8,
}

/// Everything ROM-derived, rebuilt on load and never saved.
struct Rom {
    /// The Z80's view of the program, 64 KB (decrypted where the board decrypts it).
    program: Vec<u8>,
    /// Tile pens, 8 × 8 per tile, and sprite pens, 16 × 16 per sprite.
    tiles: Vec<u8>,
    sprites: Vec<u8>,
    tile_count: usize,
    sprite_count: usize,
    /// The 32 colours as 0x00RRGGBB.
    rgb: [u32; 32],
    /// The star generator's states, one per step: bit 7 lit, bits 0-5 the colour.
    stars: Vec<u8>,
    star_rgb: [u32; 64],
}

/// The board.
pub struct Galaxian {
    cpu: Z80,
    hw: Hw,
    rom: Rom,
    rotation: Rotation,
    /// The raster's dots this frame, 0x00RRGGBB, lines 16-239.
    raster: Vec<u32>,
    /// The last frame, upright.
    video: Vec<u32>,
    audio: Vec<i16>,
}

/// The weights of the resistors on each colour gun, as MAME's resistor network model works them
/// out: each PROM output drives its gun through its resistor (red and green 1 kΩ, 470 Ω and
/// 220 Ω, blue 470 Ω and 220 Ω) into a 470 Ω load, scaled so all of a gun's outputs on give 224
/// (MAME keeps headroom for the stars and shots, which add their own resistors in parallel).
fn gun_weights() -> [[f64; 3]; 3] {
    let nets: [&[f64]; 3] = [
        &[1000.0, 470.0, 220.0],
        &[1000.0, 470.0, 220.0],
        &[470.0, 220.0],
    ];
    let (minval, maxval) = (0.0f64, 224.0f64);
    let mut w = [[0.0f64; 3]; 3];
    for (i, r) in nets.iter().enumerate() {
        for (n, wn) in w[i].iter_mut().enumerate().take(r.len()) {
            // The one output that is on pulls up; the others and the load pull down.
            let mut r0 = 1.0 / 470.0;
            let mut r1 = 1.0 / 1e12;
            for (j, &rj) in r.iter().enumerate() {
                if j == n {
                    r1 += 1.0 / rj;
                } else {
                    r0 += 1.0 / rj;
                }
            }
            r0 = 1.0 / r0;
            r1 = 1.0 / r1;
            let vout = (maxval - minval) * r0 / (r1 + r0) + minval;
            *wn = vout.clamp(minval, maxval);
        }
    }
    let sums: Vec<f64> = nets
        .iter()
        .enumerate()
        .map(|(i, r)| w[i][..r.len()].iter().sum())
        .collect();
    let mut best = 0;
    let mut max = 0.0;
    for (i, &s) in sums.iter().enumerate() {
        if max < s {
            max = s;
            best = i;
        }
    }
    let scale = maxval / sums[best];
    for (i, r) in nets.iter().enumerate() {
        for x in w[i][..r.len()].iter_mut() {
            *x *= scale;
        }
    }
    w
}

/// A gun's level from its PROM bits: the weights summed, rounded to the nearest whole level.
fn combine(w: &[f64], bits: &[u8]) -> u32 {
    // Summed from the last weight back, as MAME's combine_weights does, so the rounding agrees.
    let mut sum = 0.0;
    for (wi, &b) in w.iter().zip(bits).rev() {
        sum += wi * f64::from(b);
    }
    (sum + 0.5) as u32
}

/// The 32 colours from the PROM: bits 0-2 red, 3-5 green, 6-7 blue.
fn palette(prom: &[u8]) -> [u32; 32] {
    let w = gun_weights();
    let mut rgb = [0u32; 32];
    for (c, &v) in rgb.iter_mut().zip(prom) {
        let bit = |b: u8| (v >> b) & 1;
        let r = combine(&w[0], &[bit(0), bit(1), bit(2)]);
        let g = combine(&w[1], &[bit(3), bit(4), bit(5)]);
        let b = combine(&w[2][..2], &[bit(6), bit(7)]);
        *c = r << 16 | g << 8 | b;
    }
    rgb
}

/// The star generator: a 17-bit shift register fed from bit 12 XOR the inverse of bit 0. A dot
/// lights when the top eight bits are 1 and bit 0 is 0; its colour is the inverse of bits 3-8.
fn star_states() -> Vec<u8> {
    let mut t = vec![0u8; STAR_PERIOD as usize];
    let mut sr: u32 = 0;
    for e in t.iter_mut() {
        let lit = (sr & 0x1FE01) == 0x1FE00;
        let colour = ((!sr & 0x1F8) >> 3) as u8;
        *e = colour | u8::from(lit) << 7;
        sr = (sr >> 1) | ((((sr >> 12) ^ !sr) & 1) << 16);
    }
    t
}

/// The stars' colours: each gun gets two bits through 150 Ω and 100 Ω, which on top of the
/// palette's resistors would overdrive it, so MAME squeezes the four levels into 194-255.
fn star_colours() -> [u32; 64] {
    let (minval, midval, maxval) = (224 * 130 / 150, 224 * 130 / 100, 224 * 130 / 60);
    let map = [
        0,
        minval,
        minval + (255 - minval) * (midval - minval) / (maxval - minval),
        255,
    ];
    let mut c = [0u32; 64];
    for (i, x) in c.iter_mut().enumerate() {
        let level = |lo: usize, hi: usize| map[(i >> hi & 1) << 1 | (i >> lo & 1)] as u32;
        *x = level(5, 4) << 16 | level(3, 2) << 8 | level(1, 0);
    }
    c
}

/// Moon Cresta's program scrambling, undone: bits 6 and 2 are flipped by bits 1 and 5, and on
/// even addresses bits 2 and 6 trade places.
fn mooncrst_decrypt(addr: usize, v: u8) -> u8 {
    let mut r = v;
    if v & 0x02 != 0 {
        r ^= 0x40;
    }
    if v & 0x20 != 0 {
        r ^= 0x04;
    }
    if addr & 1 == 0 {
        r = (r & 0xBB) | (r & 0x40) >> 4 | (r & 0x04) << 4;
    }
    r
}

impl Rom {
    fn new(variant: Variant, roms: &Roms) -> Result<Rom, String> {
        if roms.program.len() < 0x4000 {
            return Err(format!(
                "the program is {} bytes, the board needs 16384",
                roms.program.len()
            ));
        }
        if roms.gfx.len() < 0x1000 || !roms.gfx.len().is_multiple_of(0x1000) {
            return Err(format!(
                "the graphics are {} bytes, the board needs 4096 or 8192",
                roms.gfx.len()
            ));
        }
        if roms.palette.len() < 32 {
            return Err("the colour PROM is missing".into());
        }
        let mut program = vec![0u8; 0x10000];
        let n = roms.program.len().min(0x10000);
        program[..n].copy_from_slice(&roms.program[..n]);
        if variant == (Variant::MoonCresta { encrypted: true }) {
            for (a, b) in program[..0x4000].iter_mut().enumerate() {
                *b = mooncrst_decrypt(a, *b);
            }
        }

        // Two bitplanes, the first half of the ROMs the high bit. A tile is 8 bytes a plane, a row
        // a byte, its leftmost pixel bit 7; a sprite is four tiles: left half bytes 0-7 and 16-23,
        // right half 8-15 and 24-31.
        let g = &roms.gfx;
        let half = g.len() / 2;
        let pen = |offset: usize, bit: usize| {
            ((g[offset] >> (7 - bit)) & 1) << 1 | ((g[half + offset] >> (7 - bit)) & 1)
        };
        let tile_count = half / 8;
        let mut tiles = vec![0u8; tile_count * 64];
        for code in 0..tile_count {
            for y in 0..8 {
                for x in 0..8 {
                    tiles[code * 64 + y * 8 + x] = pen(code * 8 + y, x);
                }
            }
        }
        let sprite_count = half / 32;
        let mut sprites = vec![0u8; sprite_count * 256];
        for code in 0..sprite_count {
            for y in 0..16 {
                for x in 0..16 {
                    let offset = code * 32
                        + (y & 7)
                        + if y >= 8 { 16 } else { 0 }
                        + if x >= 8 { 8 } else { 0 };
                    sprites[code * 256 + y * 16 + x] = pen(offset, x & 7);
                }
            }
        }
        Ok(Rom {
            program,
            tiles,
            sprites,
            tile_count,
            sprite_count,
            rgb: palette(&roms.palette),
            stars: star_states(),
            star_rgb: star_colours(),
        })
    }
}

/// The colour of the seven shells and of the missile.
const SHELL: u32 = 0xFFFFFF;
const MISSILE: u32 = 0xFFFF00;

impl Hw {
    fn new(variant: Variant) -> Hw {
        Hw {
            variant,
            ram: vec![0; 0x800],
            vram: vec![0; 0x400],
            objram: vec![0; 0x100],
            flip_x: false,
            flip_y: false,
            stars_enabled: false,
            star_origin: 0,
            star_origin_frame: 0,
            gfxbank: [0; 5],
            nmi_enabled: false,
            nmi_line: false,
            lamps: [false; 2],
            coins_locked: false,
            coins_locked_now: false,
            coin_counter: [false; 2],
            sound: SoundLatches::default(),
            watchdog: 8,
            ports: Ports::default(),
            now: 0,
            frame_start: 0,
            frames: 0,
            vblank_due: false,
            next_line: FIRST_LINE,
            sync_at: 0,
            slice_end: 0,
            last_start: 0,
            last_len: 0,
            last_cycles: Vec::new(),
            cycles: Vec::new(),
            late: Vec::new(),
            writes: Vec::new(),
            opcode: [0; 2],
            fetches: 0,
        }
    }
}

/// The Z80's view of the board while an instruction runs. Reads take effect at once; writes are
/// collected and applied after the instruction, each at the moment it happened (every Z80
/// instruction does its reads before its writes).
struct View<'a> {
    hw: &'a mut Hw,
    rom: &'a Rom,
}

/// What the main CPU reads at `addr`, without side effects.
fn read_board(hw: &Hw, rom: &Rom, addr: u16) -> u8 {
    let a = usize::from(addr);
    match hw.variant {
        Variant::Galaxian => match a {
            0x0000..=0x3FFF => rom.program[a],
            0x4000..=0x47FF => hw.ram[a & 0x3FF],
            0x5000..=0x57FF => hw.vram[a & 0x3FF],
            0x5800..=0x5FFF => hw.objram[a & 0xFF],
            // Coins 1 and 2 are bits 0 and 1; a locked-out mech never closes its switch.
            0x6000..=0x67FF => hw.ports.in0 & if hw.coins_locked_now { !0x03 } else { 0xFF },
            0x6800..=0x6FFF => hw.ports.in1,
            0x7000..=0x77FF => hw.ports.in2,
            // 7800h is the watchdog; nothing drives the bus there or anywhere unmapped.
            _ => 0xFF,
        },
        Variant::MoonCresta { .. } => match a {
            0x0000..=0x3FFF => rom.program[a],
            0x8000..=0x87FF => hw.ram[a & 0x3FF],
            0x9000..=0x97FF => hw.vram[a & 0x3FF],
            0x9800..=0x9FFF => hw.objram[a & 0xFF],
            0xA000..=0xA7FF => hw.ports.in0,
            0xA800..=0xAFFF => hw.ports.in1,
            0xB000..=0xB7FF => hw.ports.in2,
            _ => 0xFF,
        },
    }
}

impl Bus for View<'_> {
    fn read(&mut self, addr: u16) -> u8 {
        let watchdog = match self.hw.variant {
            Variant::Galaxian => (0x7800..=0x7FFF).contains(&addr),
            Variant::MoonCresta { .. } => (0xB800..=0xBFFF).contains(&addr),
        };
        if watchdog {
            self.hw.watchdog = 8;
        }
        self.hw.cycles.push(b'R');
        read_board(self.hw, self.rom, addr)
    }

    fn fetch(&mut self, addr: u16) -> u8 {
        let v = self.read(addr);
        if let Some(k) = self.hw.cycles.last_mut() {
            *k = b'M';
        }
        if self.hw.fetches < 2 {
            self.hw.opcode[usize::from(self.hw.fetches)] = v;
            self.hw.fetches += 1;
        }
        v
    }

    fn write(&mut self, addr: u16, val: u8) {
        self.hw.cycles.push(b'W');
        self.hw.writes.push((addr, val));
    }

    fn input(&mut self, _port: u16) -> u8 {
        self.hw.cycles.push(b'I');
        // Nothing is on the I/O ports; MAME's unmapped I/O reads 00h.
        0x00
    }

    fn output(&mut self, _port: u16, _val: u8) {
        self.hw.cycles.push(b'I');
    }
}

/// What a write does, by the board's address decoding.
enum Effect {
    None,
    Ram(usize),
    Video(usize),
    Object(usize),
    /// The latch bits with an effect on the picture, and the rest.
    NmiEnable,
    Stars,
    FlipX,
    FlipY,
    GfxBank(usize),
    Lamp(usize),
    CoinLock,
    CoinCounter(usize),
    Lfo(usize),
    Sound(usize),
    Pitch,
}

fn decode_write(variant: Variant, addr: u16) -> Effect {
    let a = usize::from(addr);
    match variant {
        Variant::Galaxian => match a {
            0x4000..=0x47FF => Effect::Ram(a & 0x3FF),
            0x5000..=0x57FF => Effect::Video(a & 0x3FF),
            0x5800..=0x5FFF => Effect::Object(a & 0xFF),
            0x6000..=0x67FF => match a & 7 {
                0 | 1 => Effect::Lamp(a & 1),
                2 => Effect::CoinLock,
                3 => Effect::CoinCounter(0),
                n => Effect::Lfo(n - 4),
            },
            0x6800..=0x6FFF => Effect::Sound(a & 7),
            0x7000..=0x77FF => match a & 7 {
                1 => Effect::NmiEnable,
                4 => Effect::Stars,
                6 => Effect::FlipX,
                7 => Effect::FlipY,
                _ => Effect::None,
            },
            0x7800..=0x7FFF => Effect::Pitch,
            _ => Effect::None,
        },
        Variant::MoonCresta { .. } => match a {
            0x8000..=0x87FF => Effect::Ram(a & 0x3FF),
            0x9000..=0x97FF => Effect::Video(a & 0x3FF),
            0x9800..=0x9FFF => Effect::Object(a & 0xFF),
            0xA000..=0xA7FF => match a & 7 {
                n @ 0..=2 => Effect::GfxBank(n),
                3 => Effect::CoinCounter(0),
                n => Effect::Lfo(n - 4),
            },
            0xA800..=0xAFFF => Effect::Sound(a & 7),
            0xB000..=0xB7FF => match a & 7 {
                0 => Effect::NmiEnable,
                4 => Effect::Stars,
                6 => Effect::FlipX,
                7 => Effect::FlipY,
                _ => Effect::None,
            },
            0xB800..=0xBFFF => Effect::Pitch,
            _ => Effect::None,
        },
    }
}

impl Galaxian {
    pub fn new(variant: Variant, rotation: Rotation, roms: &Roms) -> Result<Galaxian, String> {
        let rom = Rom::new(variant, roms)?;
        Ok(Galaxian {
            cpu: Z80::new(),
            hw: Hw::new(variant),
            rom,
            rotation,
            raster: vec![0; RASTER_W * RASTER_H],
            video: vec![0; WIDTH * HEIGHT],
            audio: Vec::with_capacity(SAMPLES_PER_FRAME),
        })
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

    /// The last frame, upright, 224 × 768, 0x00RRGGBB.
    pub fn video(&self) -> &[u32] {
        &self.video
    }

    /// The last frame's sound, mono, 48 kHz (792 samples).
    pub fn audio(&self) -> &[i16] {
        &self.audio
    }

    /// The sound latches as they are now.
    pub fn sound_latches(&self) -> SoundLatches {
        self.hw.sound
    }

    pub fn frame_count(&self) -> u64 {
        self.hw.frames
    }

    pub fn cpu(&self) -> &Z80 {
        &self.cpu
    }

    /// A byte as the Z80 would read it.
    pub fn peek(&self, addr: u16) -> u8 {
        read_board(&self.hw, &self.rom, addr)
    }

    /// The watchdog's reset (and the reset button): the Z80 restarts at 0000h and the watchdog
    /// starts counting again. The board's latches, unlike Pac-Man's, aren't on the reset line:
    /// they keep their settings, as in MAME. Memory keeps what it held.
    fn reset(&mut self) {
        self.cpu.reset();
        self.hw.watchdog = 8;
    }

    /// Runs one frame, VBLANK to VBLANK.
    pub fn run_frame(&mut self) {
        self.audio.clear();
        if self.hw.vblank_due {
            self.vblank();
        }
        self.hw.vblank_due = true;
        self.hw.coins_locked_now = self.hw.coins_locked;
        self.hw.next_line = FIRST_LINE;
        // The writes the last frame's last instruction made after its VBLANK.
        for (addr, val, at) in std::mem::take(&mut self.hw.late) {
            self.apply_write(addr, val, at);
        }
        let end = self.hw.frame_start + u64::from(CYCLES_PER_FRAME);
        // Worked out again before every instruction: switching the NMI off moves it.
        loop {
            let vblank = self.vblank_cycle(end);
            self.hw.slice_end = vblank;
            if self.hw.now >= vblank {
                break;
            }
            self.step(vblank);
        }
        // VBLANK: the beam has drawn the rest of the frame.
        self.draw_until(LAST_LINE);
        self.hw.frame_start = end;
        self.hw.frames += 1;
        self.turn_upright();
        self.audio.resize(SAMPLES_PER_FRAME, 0);
    }

    /// Runs one instruction (and any interrupt before it), placing its writes in time.
    fn step(&mut self, vblank: u64) {
        let start = self.hw.now;
        self.hw.fetches = 0;
        self.hw.cycles.clear();
        let t = self.cpu.step(&mut View {
            hw: &mut self.hw,
            rom: &self.rom,
        });
        self.hw.now += u64::from(t);
        self.hw.last_start = start;
        self.hw.last_len = t;
        std::mem::swap(&mut self.hw.last_cycles, &mut self.hw.cycles);
        if !self.hw.writes.is_empty() {
            self.place_writes(start, t, vblank);
        }
    }

    /// The CPU cycle from which on the CPU acts after this frame's VBLANK (`end` is the frame's
    /// nominal end). VBLANK falls inside the frame's last cycle, so normally that cycle and
    /// everything after it come after VBLANK: an instruction that would start there takes the
    /// NMI first, and a write that would happen there lands in the next frame.
    ///
    /// MAME runs the CPU in timeslices that end at its next timer, and works out how many cycles
    /// fit with a shortcut (time and cycle length both shifted right 8 bits) that for a slice of
    /// n cycles ending at VBLANK k comes out one cycle long when 53 n >= 50,688 k: the frame is
    /// 50,688 attoseconds short of 50,688 cycles, and the shortcut drops 53 attoseconds a cycle.
    /// Then the last cycle comes before VBLANK too. This only happens in the first 44 frames.
    /// The timers that end slices here are the start of VBLANK, line 0 (where MAME's partial
    /// drawing restarts), the end of VBLANK (line 16, from the second frame on) and the sound
    /// system's update, 50 times a second from power-on.
    fn vblank_cycle(&self, end: u64) -> u64 {
        let k = i128::from(self.hw.frames) + 1;
        let vblank = k * AS_PER_FRAME;
        let start = (k - 1) * AS_PER_FRAME;
        let mut last = start + 24 * AS_PER_LINE;
        if k >= 2 {
            last = last.max(start + 40 * AS_PER_LINE);
        }
        const SOUND_UPDATE: i128 = 1_000_000_000_000_000_000 / 50;
        last = last.max((vblank - 1) / SOUND_UPDATE * SOUND_UPDATE);
        // The slice starts at the CPU's first cycle at or after that timer, or where a write that
        // switched the NMI off cut the slice before, if that came later.
        let mut slice_start = ((last + AS_PER_CYCLE - 1) / AS_PER_CYCLE) as u64;
        if self.hw.sync_at < end {
            slice_start = slice_start.max(self.hw.sync_at);
        }
        let n = end.saturating_sub(slice_start);
        if 53 * n >= u64::from(CYCLES_PER_FRAME) * (self.hw.frames + 1) {
            end
        } else {
            end - 1
        }
    }

    /// Applies an instruction's writes where they fell. A Z80 write is the first T-state of the
    /// instruction's last three (the one before it three earlier, and so on), except LDI, LDD,
    /// LDIR and LDDR, which write at T-state 11 and then spend 5 (or 10) more counting. A write
    /// from VBLANK's cycle on lands in the next frame.
    fn place_writes(&mut self, start: u64, t: u32, vblank: u64) {
        let writes = std::mem::take(&mut self.hw.writes);
        let block =
            self.hw.opcode[0] == 0xED && matches!(self.hw.opcode[1], 0xA0 | 0xA8 | 0xB0 | 0xB8);
        let n = writes.len() as u64;
        for (i, &(addr, val)) in writes.iter().enumerate() {
            let at = if block {
                start + 11
            } else {
                (start + u64::from(t)).saturating_sub(3 * (n - i as u64))
            };
            if at >= vblank {
                self.hw.late.push((addr, val, at));
            } else {
                self.apply_write(addr, val, at);
            }
        }
        self.hw.writes = writes;
        self.hw.writes.clear();
    }

    /// The beam's position at a moment, as MAME works it out: the time since the VBLANK that
    /// started the frame, plus half a dot, in lines (240 at VBLANK, through 263 and 0 to 239) and
    /// dots along the line (18.432 MHz clocks). Just before VBLANK the dot count can come out
    /// slightly negative, as it does in MAME.
    fn beam(&self, at: u64) -> (i32, i32) {
        let vblank = i128::from(self.hw.frames) * AS_PER_FRAME;
        let delta = i128::from(at) * AS_PER_CYCLE - vblank + AS_PER_DOT / 2;
        let lines = delta / AS_PER_LINE;
        let dots = (delta - lines * AS_PER_LINE) / AS_PER_DOT;
        (((240 + lines) % 264) as i32, dots as i32)
    }

    /// MAME's frame number at a moment: it counts at the end of each VBLANK (line 16), and the
    /// first VBLANK comes after the first frame.
    fn frame_number(&self, at: u64) -> u32 {
        let n = self.hw.frames + 1;
        let f = if n == 1 {
            0
        } else if at < self.hw.frame_start + VBLANK_END_CYCLE {
            n - 2
        } else {
            n - 1
        };
        f as u32
    }

    /// Draws the lines the beam has passed by `at`, before a write that would show changes them
    /// (MAME's update_partial: up to and including the beam's line; during VBLANK nothing).
    fn catch_up(&mut self, at: u64) {
        let (vpos, _) = self.beam(at);
        if (FIRST_LINE as i32..=LAST_LINE as i32).contains(&vpos) {
            self.draw_until(vpos as usize);
        }
    }

    fn apply_write(&mut self, addr: u16, val: u8, at: u64) {
        let bit = val & 1 != 0;
        match decode_write(self.hw.variant, addr) {
            Effect::None => {}
            Effect::Ram(a) => self.hw.ram[a] = val,
            Effect::Video(a) => {
                self.catch_up(at);
                self.hw.vram[a] = val;
            }
            Effect::Object(a) => {
                self.catch_up(at);
                self.hw.objram[a] = val;
            }
            Effect::NmiEnable => {
                // The latched bit holds the NMI flip-flop clear while it's 0.
                self.hw.nmi_enabled = bit;
                if !bit {
                    self.hw.nmi_line = false;
                    // The CPU stops at the end of the write's machine cycle and a new timeslice
                    // starts there (MAME's set_input_line synchronises the scheduler).
                    self.hw.sync_at = at + 3;
                }
            }
            Effect::Stars => {
                if self.hw.stars_enabled != bit {
                    self.catch_up(at);
                }
                if !self.hw.stars_enabled && bit {
                    // Switching the stars on releases the generator's reset: its count starts
                    // now, wherever the beam is.
                    let (vpos, hpos) = self.beam(at);
                    self.hw.star_origin = STAR_PERIOD.wrapping_sub((vpos * 512 + hpos) as u32);
                    self.hw.star_origin_frame = self.frame_number(at);
                }
                self.hw.stars_enabled = bit;
            }
            Effect::FlipX => {
                if self.hw.flip_x != bit {
                    self.catch_up(at);
                    // The stars count a different number of clocks a frame each way round.
                    self.stars_update_origin(self.frame_number(at));
                    self.hw.flip_x = bit;
                }
            }
            Effect::FlipY => {
                if self.hw.flip_y != bit {
                    self.catch_up(at);
                    self.hw.flip_y = bit;
                }
            }
            Effect::GfxBank(n) => {
                if self.hw.gfxbank[n] != val {
                    self.catch_up(at);
                    self.hw.gfxbank[n] = val;
                }
            }
            Effect::Lamp(n) => self.hw.lamps[n] = bit,
            Effect::CoinLock => self.hw.coins_locked = !bit,
            Effect::CoinCounter(n) => self.hw.coin_counter[n] = bit,
            Effect::Lfo(n) => {
                self.hw.sound.lfo = (self.hw.sound.lfo & !(1 << n)) | u8::from(bit) << n
            }
            Effect::Sound(n) => {
                self.hw.sound.sound = (self.hw.sound.sound & !(1 << n)) | u8::from(bit) << n
            }
            Effect::Pitch => self.hw.sound.pitch = val,
        }
    }

    /// VBLANK begins: the NMI line rises if enabled, and the watchdog counts it. A watchdog reset
    /// lands after the line has risen, so the Z80's reset drops the NMI it had just latched while
    /// the line stays high: no NMI comes until the game drops the NMI ON latch and VBLANK raises
    /// the line again. That is how the Z80's edge-triggered /NMI behaves, and how MAME has it (its
    /// soft reset runs after the VBLANK callbacks and clears the pending NMI, not the line).
    fn vblank(&mut self) {
        if self.hw.nmi_enabled && !self.hw.nmi_line {
            self.hw.nmi_line = true;
            self.cpu.nmi();
        }
        self.hw.watchdog -= 1;
        if self.hw.watchdog == 0 {
            // The reset cuts into the instruction running at VBLANK where the CPU stopped for
            // VBLANK, and that instruction's later writes never happen. The Z80's clock carries on
            // from that point, not from where the instruction would have ended.
            let end = self.hw.last_start + u64::from(self.hw.last_len);
            if self.hw.now == end
                && self.hw.last_start < self.hw.slice_end
                && end > self.hw.slice_end
            {
                self.hw.now = pause_point(
                    self.hw.last_start,
                    self.hw.last_len,
                    &self.hw.last_cycles,
                    self.hw.slice_end,
                );
            }
            self.hw.late.clear();
            self.reset();
        }
    }

    /// Moves the stars' origin to frame `now`: the generator runs 2^17 clocks a frame flipped,
    /// one more than its period, and 2^17 - 2 the right way round, one fewer, which is what makes
    /// the stars drift sideways.
    fn stars_update_origin(&mut self, now: u32) {
        if now != self.hw.star_origin_frame {
            let per_frame: i32 = if self.hw.flip_x { 1 } else { -1 };
            let mut total =
                per_frame.wrapping_mul(now.wrapping_sub(self.hw.star_origin_frame) as i32);
            while total < 0 {
                total += STAR_PERIOD as i32;
            }
            self.hw.star_origin = self.hw.star_origin.wrapping_add(total as u32) % STAR_PERIOD;
            self.hw.star_origin_frame = now;
        }
    }

    /// Draws raster lines up to and including `line`, from where the last drawing stopped.
    fn draw_until(&mut self, line: usize) {
        let line = line.min(LAST_LINE);
        if line < self.hw.next_line {
            return;
        }
        // Lines are only drawn once the beam is past VBLANK's end, where MAME's frame number has
        // already counted this frame: one less than the frame being run.
        self.stars_update_origin(self.hw.frames as u32);
        for y in self.hw.next_line..=line {
            self.draw_line(y);
        }
        self.hw.next_line = line + 1;
    }

    /// One raster line: the background and stars, the tiles, the sprites, then the shots.
    fn draw_line(&mut self, y: usize) {
        let hw = &self.hw;
        let rom = &self.rom;
        let row = (y - FIRST_LINE) * RASTER_W;
        let out = &mut self.raster[row..row + RASTER_W];

        // The background: black, and the stars when they're on.
        out.fill(0);
        if hw.stars_enabled {
            let offs = hw.star_origin.wrapping_add(y as u32 * 512);
            stars_row(out, &rom.stars, &rom.star_rgb, y, offs, 0xFF);
        }

        // The tiles. Each column scrolls by its own byte of object RAM and takes its colour from
        // the next. The screen flips mirror the whole layer about the visible area's centre.
        let my = if hw.flip_y { 255 - y } else { y };
        for col_on_screen in 0..32 {
            let col = if hw.flip_x {
                31 - col_on_screen
            } else {
                col_on_screen
            };
            let scroll = hw.objram[col * 2];
            let ty = (my + usize::from(scroll)) & 0xFF;
            let mut code = u16::from(hw.vram[(ty >> 3) * 32 + col]);
            let mut colour = hw.objram[col * 2 + 1] & 7;
            extend_tile(hw, &mut code, &mut colour);
            let pens = &rom.tiles[(usize::from(code) % rom.tile_count) * 64 + (ty & 7) * 8..][..8];
            let base = usize::from(colour) * 4;
            for dx in 0..24 {
                let x = col_on_screen * 24 + dx;
                let mx = if hw.flip_x { 767 - x } else { x };
                let pen = pens[(mx % 24) / 3];
                if pen != 0 {
                    out[x] = rom.rgb[base + usize::from(pen)];
                }
            }
        }

        // The sprites: the last drawn first, so sprite 0 is on top. The line buffer is cut off
        // 17 dots in from the left edge (the right edge when flipped). Sprites 0-2 are matched
        // against the line before, so they sit one line lower than the rest.
        let (clip_lo, clip_hi) = if hw.flip_x {
            (0, (256 - 17) * 3 - 1)
        } else {
            (17 * 3, RASTER_W - 1)
        };
        for n in (0..8).rev() {
            let b = &hw.objram[0x40 + n * 4..0x40 + n * 4 + 4];
            let mut sy = 240u8.wrapping_sub(b[0].wrapping_sub(u8::from(n < 3)));
            let mut code = u16::from(b[1] & 0x3F);
            let mut fx = b[1] & 0x40 != 0;
            let mut fy = b[1] & 0x80 != 0;
            let mut colour = b[2] & 7;
            // The sprites sit one dot right of the tiles.
            let mut sx = b[3].wrapping_add(1);
            extend_sprite(hw, &mut code, &mut colour);
            if hw.flip_x {
                sx = 240u8.wrapping_sub(sx);
                fx = !fx;
            }
            if hw.flip_y {
                sy = 240u8.wrapping_sub(sy);
                fy = !fy;
            }
            let r = y as i32 - i32::from(sy);
            if !(0..16).contains(&r) {
                continue;
            }
            let sr = if fy { 15 - r } else { r } as usize;
            let pens = &rom.sprites[(usize::from(code) % rom.sprite_count) * 256 + sr * 16..][..16];
            let base = usize::from(colour) * 4;
            for dx in 0..48 {
                let x = usize::from(sx) * 3 + dx;
                if x < clip_lo || x > clip_hi {
                    continue;
                }
                let pen = pens[if fx { 15 - dx / 3 } else { dx / 3 }];
                if pen != 0 {
                    out[x] = rom.rgb[base + usize::from(pen)];
                }
            }
        }

        // The shots: each line shows at most one shell and one missile, the last entry that
        // matches the line winning. The first three entries match the line before.
        let shots = &hw.objram[0x60..0x80];
        let (mut shell, mut missile) = (None, None);
        let line = |d: i32| -> u8 {
            let v = y as i32 - d;
            (if hw.flip_y { v ^ 255 } else { v }) as u8
        };
        for w in 0..3 {
            if shots[w * 4 + 1].wrapping_add(line(1)) == 0xFF {
                shell = Some(w);
            }
        }
        for w in 3..8 {
            if shots[w * 4 + 1].wrapping_add(line(0)) == 0xFF {
                if w == 7 {
                    missile = Some(w);
                } else {
                    shell = Some(w);
                }
            }
        }
        for (w, colour) in [(shell, SHELL), (missile, MISSILE)] {
            if let Some(w) = w {
                // A shot shows from its counter's FCh to 00h: four dots ending at its position.
                let x = 255 - i32::from(shots[w * 4 + 3]) - 4;
                for dot in x..x + 4 {
                    for sub in 0..3 {
                        let px = dot * 3 + sub;
                        if (0..RASTER_W as i32).contains(&px) {
                            out[px as usize] = colour;
                        }
                    }
                }
            }
        }
    }

    /// The raster turned the way the cabinet shows it.
    fn turn_upright(&mut self) {
        for uy in 0..HEIGHT {
            for ux in 0..WIDTH {
                let (x, y) = match self.rotation {
                    // A quarter turn clockwise: the raster's left edge is the screen's top, its
                    // bottom edge the screen's left.
                    Rotation::Rot90 => (uy, RASTER_H - 1 - ux),
                    Rotation::Rot270 => (RASTER_W - 1 - uy, ux),
                };
                self.video[uy * WIDTH + ux] = self.raster[y * RASTER_W + x];
            }
        }
    }

    /// A save state: the Z80 and the board, bincode.
    pub fn save_state(&self) -> Vec<u8> {
        bincode::serialize(&(&self.cpu, &self.hw)).expect("a board state always serialises")
    }

    /// Restores `save_state`'s bytes. On an error the board is unchanged.
    pub fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        let (cpu, hw): (Z80, Hw) =
            bincode::deserialize(data).map_err(|e| format!("not a Galaxian board state: {e}"))?;
        if hw.variant != self.hw.variant {
            return Err("that state is from a different board".into());
        }
        self.cpu = cpu;
        self.hw = hw;
        Ok(())
    }
}

/// Where MAME's Z80 stops when its timeslice runs out at `budget` inside an instruction that
/// started at `start`: at the first point at or after the budget where the core checks its cycle
/// count. It checks halfway through an opcode fetch and at its end, at the end of each memory
/// or I/O cycle, and after each internal cycle on its own. The internal cycles (the length left
/// over by the bus cycles) sit after the last fetch or read and before the first write, which is
/// where the Z80 spends them in nearly every instruction (counting, adding, a relative jump).
fn pause_point(start: u64, len: u32, cycles: &[u8], budget: u64) -> u64 {
    let bus: u32 = cycles
        .iter()
        .map(|&k| if k == b'R' || k == b'W' { 3 } else { 4 })
        .sum();
    let mut internal = len.saturating_sub(bus);
    let first_write = cycles
        .iter()
        .position(|&k| k == b'W')
        .unwrap_or(cycles.len());
    let mut at = start;
    let mut stops: Vec<u64> = Vec::with_capacity(24);
    for (i, &k) in cycles.iter().enumerate() {
        if i == first_write {
            for _ in 0..internal {
                at += 1;
                stops.push(at);
            }
            internal = 0;
        }
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
    stops
        .into_iter()
        .find(|&p| p >= budget)
        .unwrap_or(start + u64::from(len))
}

/// Moon Cresta's graphics bank: with bank latch 2 set, tiles 80h-BFh and sprites 20h-2Fh come
/// from the second half of the ROMs, at the block bank latches 0 and 1 pick.
fn extend_tile(hw: &Hw, code: &mut u16, _colour: &mut u8) {
    if let Variant::MoonCresta { .. } = hw.variant {
        let g = &hw.gfxbank;
        if g[2] != 0 && *code & 0xC0 == 0x80 {
            *code = (*code & 0x3F) | u16::from(g[0]) << 6 | u16::from(g[1]) << 7 | 0x100;
        }
    }
}

fn extend_sprite(hw: &Hw, code: &mut u16, _colour: &mut u8) {
    if let Variant::MoonCresta { .. } = hw.variant {
        let g = &hw.gfxbank;
        if g[2] != 0 && *code & 0x30 == 0x20 {
            *code = (*code & 0x0F) | u16::from(g[0]) << 4 | u16::from(g[1]) << 5 | 0x40;
        }
    }
}

/// One line of stars, from the generator's state `offs` at the line's left edge. The generator
/// steps twice a pixel, on the 18.432 MHz clock's first and second ticks of three, so the first
/// state lights one dot and the second two. Stars only show where line bit 0 and dot bit 3
/// differ, and only colours with a bit in `mask` set (Scramble's blinking).
fn stars_row(out: &mut [u32], stars: &[u8], rgb: &[u32; 64], y: usize, offs: u32, mask: u8) {
    let mut offs = (offs % STAR_PERIOD) as usize;
    let period = STAR_PERIOD as usize;
    for x in 0..256 {
        let shown = (y ^ (x >> 3)) & 1 != 0;
        let s = stars[offs];
        offs = if offs + 1 >= period { 0 } else { offs + 1 };
        if shown && s & 0x80 != 0 && s & mask != 0 {
            out[x * 3] = rgb[usize::from(s & 0x3F)];
        }
        let s = stars[offs];
        offs = if offs + 1 >= period { 0 } else { offs + 1 };
        if shown && s & 0x80 != 0 && s & mask != 0 {
            out[x * 3 + 1] = rgb[usize::from(s & 0x3F)];
            out[x * 3 + 2] = rgb[usize::from(s & 0x3F)];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_roms(program: &[u8]) -> Roms {
        let mut p = vec![0u8; 0x4000];
        p[..program.len()].copy_from_slice(program);
        Roms {
            program: p,
            gfx: vec![0; 0x1000],
            palette: vec![0; 32],
        }
    }

    #[test]
    fn frame_timing() {
        assert_eq!(CYCLES_PER_FRAME, 50_688);
        assert!((FPS - 60.606_06).abs() < 1e-4);
        assert_eq!(SAMPLES_PER_FRAME, 792);
        // VBLANK falls in the frame's last CPU cycle.
        let frame_end = i128::from(CYCLES_PER_FRAME) * AS_PER_CYCLE;
        assert!(AS_PER_FRAME < frame_end && AS_PER_FRAME > frame_end - AS_PER_CYCLE);
    }

    #[test]
    fn palette_levels_from_the_resistors() {
        // All of a gun's bits on is MAME's 224; the blue gun's two resistors give less.
        let p = palette(&[0x00, 0x07, 0x38, 0xC0, 0xFF, 0x01, 0x02, 0x04]);
        assert_eq!(p[0], 0);
        assert_eq!(p[1] >> 16, 224);
        assert_eq!(p[2] >> 8 & 0xFF, 224);
        assert!(p[3] & 0xFF < 224 && p[3] & 0xFF > 150, "{:06x}", p[3]);
        assert_eq!(p[4] >> 16, 224);
        assert!(p[5] >> 16 < p[6] >> 16 && p[6] >> 16 < p[7] >> 16);
    }

    #[test]
    fn the_star_generator_repeats_and_lights_one_dot_in_512() {
        let s = star_states();
        assert_eq!(s.len(), 131_071);
        let lit = s.iter().filter(|&&v| v & 0x80 != 0).count();
        // A dot lights for 1 of the 512 settings of the nine bits that decide it.
        assert_eq!(lit, 256);
        let c = star_colours();
        assert_eq!(c[0], 0);
        assert_eq!(c[0x3F], 0xFFFFFF);
        assert_eq!(c[0x20] >> 16, 194);
        assert_eq!(c[0x10] >> 16, 214);
    }

    #[test]
    fn mooncrst_decryption() {
        // Odd addresses: only the two XORs. Even: the XORs, then bits 2 and 6 swapped.
        assert_eq!(mooncrst_decrypt(1, 0x02), 0x42);
        assert_eq!(mooncrst_decrypt(1, 0x20), 0x24);
        assert_eq!(mooncrst_decrypt(0, 0x40), 0x04);
        assert_eq!(mooncrst_decrypt(0, 0x02), 0x06);
        // One to one on both kinds of address.
        for a in 0..2 {
            let mut seen = [false; 256];
            for v in 0..=255u8 {
                seen[usize::from(mooncrst_decrypt(a, v))] = true;
            }
            assert!(seen.iter().all(|&s| s));
        }
    }

    #[test]
    fn watchdog_resets_after_8_vblanks_without_a_read() {
        // JR $ : spin forever without reading 7800h.
        let mut b = Galaxian::new(
            Variant::Galaxian,
            Rotation::Rot90,
            &blank_roms(&[0x18, 0xFE]),
        )
        .unwrap();
        for _ in 0..8 {
            b.run_frame();
        }
        assert_eq!(b.hw.watchdog, 1);
        b.cpu.pc = 0x1234;
        b.run_frame();
        assert_eq!(b.hw.watchdog, 8);
        assert!(b.cpu.pc < 0x10, "the Z80 restarted");
    }

    #[test]
    fn the_nmi_comes_on_the_lines_rising_edge() {
        // 0000: LD A,1 / LD (7001),A / JR $ ; NMI at 0066: JR $ (never cleared)
        let mut p = vec![0x3E, 0x01, 0x32, 0x01, 0x70, 0x18, 0xFE];
        p.resize(0x66, 0);
        p.extend_from_slice(&[0x3A, 0x00, 0x78, 0x18, 0xFB]);
        let mut b = Galaxian::new(Variant::Galaxian, Rotation::Rot90, &blank_roms(&p)).unwrap();
        b.run_frame();
        b.run_frame();
        assert!(b.cpu.pc >= 0x66, "the first VBLANK took the NMI");
        let sp = b.cpu.sp;
        b.run_frame();
        assert_eq!(b.cpu.sp, sp, "a line that stays high gives no second NMI");
    }

    #[test]
    fn save_state_round_trip() {
        let mut b = Galaxian::new(
            Variant::Galaxian,
            Rotation::Rot90,
            &blank_roms(&[0x3C, 0x18, 0xFD]),
        )
        .unwrap();
        b.run_frame();
        let s = b.save_state();
        b.run_frame();
        let after = b.save_state();
        b.load_state(&s).unwrap();
        assert_eq!(b.save_state(), s);
        b.run_frame();
        assert_eq!(b.save_state(), after);
    }
}
