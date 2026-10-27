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

use crate::ay8910::{Ay8910, NoPorts};
use crate::galaxian_sound::{self, GalaxianSound, Mixer};
use crate::i8255::{I8255, Outputs};
use crate::konami_sound::{self, KonamiSound};
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
/// The sound circuit runs at 192 kHz (`galaxian_sound`), 16 CPU cycles a sample: 3168 a frame.
const CYCLES_PER_SAMPLE: u64 = (CPU_HZ / galaxian_sound::RATE) as u64;
pub const RAW_SAMPLES_PER_FRAME: usize = (CYCLES_PER_FRAME as u64 / CYCLES_PER_SAMPLE) as usize;
/// The sound handed out: 48 kHz, 792 samples a frame, each the mean of four.
pub const SAMPLE_RATE: u32 = galaxian_sound::RATE / 4;
pub const SAMPLES_PER_FRAME: usize = RAW_SAMPLES_PER_FRAME / 4;

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
/// The period of the 555 that steps Scramble's stars' blinking: 693 × (100 kΩ + 2 × 10 kΩ) ×
/// 10 µF, in attoseconds (0.8316 s).
const BLINK_PERIOD: i128 = 831_600_000 * 1_000_000_000;
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
    /// Konami's boards: two 8255 PPIs for the controls and the sound board, the sound board of
    /// their own (`konami_sound`), and each its memory map and picture.
    Konami(Konami),
    /// Pisces and the games on its board: Galaxian's, with the coin lockout latch (6002h) wired
    /// to a graphics bank instead, choosing which half of ROMs twice the size shows. Batman Part 2
    /// (`batman2`) banks only tiles 80h-FFh, and its sprites are always in the upper half.
    Pisces { batman2: bool },
    /// Jump Bug's board: Galaxian's video moved and extended (2 KB of RAM, 32 KB of program, five
    /// graphics bank latches over 768 tiles), Scramble's shots, no watchdog and no Galaxian sound
    /// circuit; an AY-3-8910 at 5800h/5900h (`ay`, missing on the Brazilian boards), and a
    /// protection ROM at B000h that answers a few addresses.
    JumpBug { ay: bool },
    /// King & Balloon: Moon Cresta's board (unscrambled) without stars, with a speech board: a
    /// second Z80 at 2.5 MHz playing samples from its own 8 KB through a 4-bit DAC, told what to
    /// say through a latch the main CPU sets two bits of (B000h and B002h). B003h picks which of
    /// two switches IN0 bit 6 reads, and IN1 bit 5 is the sound circuit's noise.
    KingBalloon,
}

/// King & Balloon's speech board.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Speech {
    cpu: Z80,
    /// Written only: the board's RAM sits under its ROM, so reads come from the ROM.
    ram: Vec<u8>,
    /// The command latch (its top four bits read high), and when each new value came, in the
    /// speech CPU's cycles.
    latch: u8,
    latch_events: Vec<(u64, u8)>,
    /// The DAC's level, and this frame's changes (when, in main CPU cycles; the level).
    dac: u8,
    dac_events: Vec<(u64, u8)>,
    /// Speech CPU cycles run since power-on.
    now: u64,
}

/// The speech CPU's clock against the main CPU's: 2.5 MHz against 3.072 MHz.
const SPEECH_HZ: u64 = 2_500_000;

struct SpeechBus<'a> {
    rom: &'a [u8],
    ram: &'a mut Vec<u8>,
    latch: u8,
    dac: Option<u8>,
}

impl Bus for SpeechBus<'_> {
    fn read(&mut self, addr: u16) -> u8 {
        let a = usize::from(addr & 0x3FFF);
        if a < 0x2000 {
            self.rom.get(a).copied().unwrap_or(0xFF)
        } else {
            0xFF
        }
    }
    fn write(&mut self, addr: u16, val: u8) {
        let a = usize::from(addr & 0x3FFF);
        if a < 0x2000 {
            self.ram[a & 0x3FF] = val;
        }
    }
    fn input(&mut self, _port: u16) -> u8 {
        self.latch
    }
    fn output(&mut self, _port: u16, val: u8) {
        self.dac = Some(val >> 4);
    }
}

/// Konami's boards, by their memory map and picture (MAME's scramble, theend, scobra, frogger and
/// turtles machines).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Konami {
    /// Scramble: The End's map, two-dot yellow shells, a blue background that can be switched on,
    /// still stars that blink, and the protection chip on the second PPI's port C.
    Scramble,
    /// The End: Galaxian's shots and stars (the shells' blue and green swapped), the protection
    /// chip, and the coin counter on the first PPI's port C.
    TheEnd,
    /// Super Cobra and the games on its board: 32 KB of program, Scramble's picture.
    SuperCobra,
    /// Frogger: its own map, the river's blue half, colour and scroll bits wired differently, a
    /// graphics ROM with two data lines swapped, and a sound board with one AY.
    Frogger,
    /// Turtles and Amidar: 32 KB of program, a background in any of eight colours, no stars.
    Turtles,
}

impl Variant {
    fn konami(self) -> Option<Konami> {
        match self {
            Variant::Konami(k) => Some(k),
            _ => None,
        }
    }
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
    /// Konami's boards: the sound board's program (8 KB). Empty on the others.
    pub sound: Vec<u8>,
    /// Konami's boards: the bits of IN2 that read the protection chip instead of a control (on
    /// Scramble 20h and 80h, as MAME's set has them wired), 0 elsewhere.
    pub protection_bits: u8,
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    /// The sound circuit the latches drive.
    snd: GalaxianSound,
    /// Konami's boards: the PPIs, the background controls (Scramble's blue switch, Turtles' red,
    /// green and blue), the protection chip, the sound board and what was sent to it this frame
    /// (each with the moment it happened, in attoseconds).
    ppi: [I8255; 2],
    background: bool,
    background_rgb: [bool; 3],
    protection_state: u16,
    protection_result: u8,
    konami: Option<KonamiSound>,
    konami_events: Vec<(i128, konami_sound::Event)>,
    /// The 48 kHz samples the sound board has made and the frame hasn't handed out yet.
    konami_out: Vec<i16>,
    /// Jump Bug's AY-3-8910, and this frame's writes to it: (when, the address latch or data,
    /// the value).
    ay: Option<Ay8910>,
    ay_events: Vec<(u64, bool, u8)>,
    /// King & Balloon: its speech board, the two bits the main CPU sends it, which switch IN0
    /// bit 6 reads, and the noise line's generator.
    speech: Option<Speech>,
    speech_bits: u8,
    speech_dip: u8,
    noise: u32,
    /// The stars' blink counter on Scramble's boards (a 555 that steps it every 0.8316 s, from
    /// each reset on), as it stood at the last reset, and when that was.
    blinks_at_reset: u32,
    reset_at: u64,
    protection_bits: u8,
    /// The moment the lines being drawn are drawn at (MAME draws them when something changes).
    draw_at: u64,
    /// The latches as they changed this frame, with the cycle of each change: the sound is
    /// worked out at the end of the frame, each change from the sample after it on.
    #[serde(skip)]
    sound_events: Vec<(u64, SoundLatches)>,
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
    /// The second PPI's port B as last driven (the sound board's control).
    konami_control: u8,
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
    /// King & Balloon's speech program (8 KB); empty on the other boards.
    sound: Vec<u8>,
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
    /// The last frame's sound at 192 kHz, and at 48 kHz.
    raw: Vec<i16>,
    audio: Vec<i16>,
    /// For debugging against MAME's trace: when set, each main CPU instruction's first cycle and
    /// PC (an interrupt shows as the instruction it ran first).
    #[doc(hidden)]
    pub main_trace: Option<Vec<(u64, u16)>>,
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

        // Frogger's second graphics ROM has its D0 and D1 swapped.
        let mut gfx = roms.gfx.clone();
        if variant == Variant::Konami(Konami::Frogger) {
            for b in gfx[0x800..0x1000].iter_mut() {
                *b = (*b & 0xFC) | (*b >> 1 & 1) | (*b << 1 & 2);
            }
        }
        // Two bitplanes, the first half of the ROMs the high bit. A tile is 8 bytes a plane, a row
        // a byte, its leftmost pixel bit 7; a sprite is four tiles: left half bytes 0-7 and 16-23,
        // right half 8-15 and 24-31.
        let g = &gfx;
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
            sound: if variant == Variant::KingBalloon { roms.sound.clone() } else { Vec::new() },
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
            snd: GalaxianSound::new(match variant {
                Variant::MoonCresta { .. } | Variant::KingBalloon => Mixer::MoonCresta,
                _ => Mixer::Galaxian,
            }),
            sound_events: Vec::new(),
            ppi: [I8255::new(), I8255::new()],
            background: false,
            background_rgb: [false; 3],
            protection_state: 0,
            protection_result: 0,
            konami: None,
            konami_events: Vec::new(),
            konami_out: Vec::new(),
            ay: (variant == Variant::JumpBug { ay: true }).then(Ay8910::new),
            ay_events: Vec::new(),
            speech: (variant == Variant::KingBalloon).then(|| Speech {
                cpu: Z80::new(),
                ram: vec![0; 0x400],
                latch: 0xF0,
                latch_events: Vec::new(),
                dac: 0,
                dac_events: Vec::new(),
                now: 0,
            }),
            speech_bits: 0,
            speech_dip: 0,
            noise: 0x1_2345,
            blinks_at_reset: 0,
            reset_at: 0,
            protection_bits: 0,
            draw_at: 0,
            watchdog: 8,
            ports: Ports::default(),
            now: 0,
            frame_start: 0,
            frames: 0,
            vblank_due: false,
            next_line: FIRST_LINE,
            sync_at: 0,
            konami_control: 0,
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
        Variant::Galaxian | Variant::Pisces { .. } => match a {
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
        // Jump Bug: unmapped reads are FFh (MAME's map sets them high).
        Variant::JumpBug { .. } => match a {
            0x0000..=0x3FFF | 0x8000..=0xAFFF => rom.program[a],
            0x4000..=0x47FF => hw.ram[a & 0x7FF],
            0x4800..=0x4FFF => hw.vram[a & 0x3FF],
            0x5000..=0x57FF => hw.objram[a & 0xFF],
            0x6000..=0x67FF => hw.ports.in0,
            0x6800..=0x6FFF => hw.ports.in1,
            0x7000..=0x77FF => hw.ports.in2,
            // The protection: a few addresses answer, the rest read FFh.
            0xB000..=0xBFFF => match a - 0xB000 {
                0x114 => 0x4F,
                0x118 => 0xD3,
                0x214 => 0xCF,
                0x235 => 0x02,
                _ => 0xFF,
            },
            _ => 0xFF,
        },
        Variant::KingBalloon => match a {
            0x0000..=0x3FFF => rom.program[a],
            0x8000..=0x87FF => hw.ram[a & 0x3FF],
            0x9000..=0x97FF => hw.vram[a & 0x3FF],
            0x9800..=0x9FFF => hw.objram[a & 0xFF],
            // Bit 6: the service switch (B003h = 0) or the speech switch (1), both off and on
            // as the factory left them (service off, speech on).
            0xA000..=0xA7FF => {
                let mux = if hw.speech_dip == 1 { 1 } else { 0 };
                (hw.ports.in0 & !0x40) | mux << 6
            }
            // Bit 5: the noise line (the game only checks it moves).
            0xA800..=0xAFFF => (hw.ports.in1 & !0x20) | ((hw.noise & 1) as u8) << 5,
            0xB000..=0xB7FF => hw.ports.in2,
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
        Variant::Konami(k) => match konami_decode(k, addr) {
            Decoded::Rom => rom.program[a],
            Decoded::Ram(o) => hw.ram[o],
            Decoded::Video(o) => hw.vram[o],
            Decoded::Object(o) => hw.objram[o],
            Decoded::Ppi(sel, reg) => {
                // A read that selects both PPIs gets both on the bus at once.
                let mut v = 0xFF;
                for n in 0..2 {
                    if sel & (1 << n) != 0 {
                        v &= hw.ppi[n].read(reg, ppi_pins(hw, k, n));
                    }
                }
                v
            }
            _ => 0xFF,
        },
    }
}

/// What a PPI's input pins read: the first PPI the controls and switches (IN0-IN2, with
/// Scramble's protection bits), the second IN3 on port C, or on Scramble's and The End's boards
/// the protection chip's answer.
fn ppi_pins(hw: &Hw, k: Konami, n: usize) -> [u8; 3] {
    if n == 0 {
        // The protection bits read the chip's top bit inverted (MAME has them active low).
        let bits = hw.protection_bits;
        let prot = if hw.protection_result & 0x80 != 0 { 0 } else { bits };
        [hw.ports.in0, hw.ports.in1, (hw.ports.in2 & !bits) | prot]
    } else {
        let c = match k {
            Konami::Scramble | Konami::TheEnd => hw.protection_result,
            _ => hw.ports.in3,
        };
        [0xFF, 0xFF, c]
    }
}

/// Where an address lands on Konami's boards.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Decoded {
    None,
    Rom,
    Ram(usize),
    Video(usize),
    Object(usize),
    /// Which PPIs (bit 0 the first, bit 1 the second) and which register.
    Ppi(u8, u8),
    /// One of the board's latches, by what it does.
    Latch(KonamiLatch),
    Watchdog,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum KonamiLatch {
    Nmi,
    CoinCounter(usize),
    Background,
    Stars,
    FlipX,
    FlipY,
    Red,
    Green,
    Blue,
}

/// The address decoding of each of Konami's boards (from MAME's memory maps, which follow the
/// schematics): mirrors, the PPIs' select lines and the latches.
fn konami_decode(k: Konami, addr: u16) -> Decoded {
    let a = usize::from(addr);
    use KonamiLatch as L;
    match k {
        Konami::Scramble | Konami::TheEnd => match a {
            0x0000..=0x3FFF => Decoded::Rom,
            0x4000..=0x47FF => Decoded::Ram(a & 0x7FF),
            0x4800..=0x4FFF => Decoded::Video(a & 0x3FF),
            0x5000..=0x57FF => Decoded::Object(a & 0xFF),
            0x6800..=0x6FFF => match a & 7 {
                1 => Decoded::Latch(L::Nmi),
                2 if k == Konami::Scramble => Decoded::Latch(L::CoinCounter(0)),
                3 => Decoded::Latch(L::Background),
                4 => Decoded::Latch(L::Stars),
                6 => Decoded::Latch(L::FlipX),
                7 => Decoded::Latch(L::FlipY),
                _ => Decoded::None,
            },
            0x7000..=0x77FF => Decoded::Watchdog,
            // A8 selects the first PPI and A9 the second (both at once if both are set).
            0x8000..=0xFFFF => Decoded::Ppi((a >> 8 & 3) as u8, (a & 3) as u8),
            _ => Decoded::None,
        },
        // 32 KB of ROM, and everything above it mirrored with A14 ignored.
        Konami::SuperCobra if a < 0x8000 => Decoded::Rom,
        Konami::SuperCobra => match a & 0xBFFF {
            0x8000..=0x87FF => Decoded::Ram(a & 0x7FF),
            0x8800..=0x8FFF => Decoded::Video(a & 0x3FF),
            0x9000..=0x97FF => Decoded::Object(a & 0xFF),
            0x9800..=0x9FFF => Decoded::Ppi(1, (a & 3) as u8),
            0xA000..=0xA7FF => Decoded::Ppi(2, (a & 3) as u8),
            0xA800..=0xAFFF => match a & 7 {
                1 => Decoded::Latch(L::Nmi),
                2 => Decoded::Latch(L::CoinCounter(0)),
                3 => Decoded::Latch(L::Background),
                4 => Decoded::Latch(L::Stars),
                6 => Decoded::Latch(L::FlipX),
                7 => Decoded::Latch(L::FlipY),
                _ => Decoded::None,
            },
            0xB000..=0xB7FF => Decoded::Watchdog,
            _ => Decoded::None,
        },
        Konami::Frogger => match a {
            0x0000..=0x3FFF => Decoded::Rom,
            0x8000..=0x87FF => Decoded::Ram(a & 0x7FF),
            0x8800..=0x8FFF => Decoded::Watchdog,
            0xA800..=0xAFFF => Decoded::Video(a & 0x3FF),
            0xB000..=0xB7FF => Decoded::Object(a & 0xFF),
            0xB800..=0xBFFF => match a & 0x1C {
                0x08 => Decoded::Latch(L::Nmi),
                0x0C => Decoded::Latch(L::FlipY),
                0x10 => Decoded::Latch(L::FlipX),
                0x18 => Decoded::Latch(L::CoinCounter(0)),
                0x1C => Decoded::Latch(L::CoinCounter(1)),
                _ => Decoded::None,
            },
            // A13 selects the first PPI and A12 the second; A1-A2 the register.
            0xC000..=0xFFFF => {
                let sel = u8::from(a & 0x2000 != 0) | u8::from(a & 0x1000 != 0) << 1;
                Decoded::Ppi(sel, (a >> 1 & 3) as u8)
            }
            _ => Decoded::None,
        },
        Konami::Turtles if a < 0x8000 => Decoded::Rom,
        Konami::Turtles => match a & 0xBFFF {
            0x8000..=0x87FF => Decoded::Ram(a & 0x7FF),
            0x9000..=0x97FF => Decoded::Video(a & 0x3FF),
            0x9800..=0x9FFF => Decoded::Object(a & 0xFF),
            0xA000..=0xA7FF => match a & 0x38 {
                0x00 => Decoded::Latch(L::Red),
                0x08 => Decoded::Latch(L::Nmi),
                0x10 => Decoded::Latch(L::FlipY),
                0x18 => Decoded::Latch(L::FlipX),
                0x20 => Decoded::Latch(L::Green),
                0x28 => Decoded::Latch(L::Blue),
                0x30 => Decoded::Latch(L::CoinCounter(0)),
                _ => Decoded::Latch(L::CoinCounter(1)),
            },
            0xA800..=0xAFFF => Decoded::Watchdog,
            // A4-A5 pick the register.
            0xB000..=0xB7FF => Decoded::Ppi(1, (a >> 4 & 3) as u8),
            0xB800..=0xBFFF => Decoded::Ppi(2, (a >> 4 & 3) as u8),
            _ => Decoded::None,
        },
    }
}

impl Bus for View<'_> {
    fn read(&mut self, addr: u16) -> u8 {
        let watchdog = match self.hw.variant {
            Variant::Galaxian | Variant::Pisces { .. } => (0x7800..=0x7FFF).contains(&addr),
            // No watchdog on Jump Bug's board.
            Variant::JumpBug { .. } => false,
            Variant::MoonCresta { .. } | Variant::KingBalloon => (0xB800..=0xBFFF).contains(&addr),
            Variant::Konami(k) => konami_decode(k, addr) == Decoded::Watchdog,
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
    /// King & Balloon: the speech command's two bits, and the switch select.
    Speech(u8),
    SpeechDip,
    /// Jump Bug's AY-3-8910: its address latch, its data.
    AyAddress,
    AyData,
    /// Konami's boards.
    Background,
    BackgroundRgb(usize),
    Ppi(u8, u8),
}

fn decode_write(variant: Variant, addr: u16) -> Effect {
    let a = usize::from(addr);
    match variant {
        Variant::Galaxian | Variant::Pisces { .. } => match a {
            0x4000..=0x47FF => Effect::Ram(a & 0x3FF),
            0x5000..=0x57FF => Effect::Video(a & 0x3FF),
            0x5800..=0x5FFF => Effect::Object(a & 0xFF),
            0x6000..=0x67FF => match a & 7 {
                0 | 1 => Effect::Lamp(a & 1),
                2 if variant != Variant::Galaxian => Effect::GfxBank(0),
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
        Variant::JumpBug { ay } => match a {
            0x4000..=0x47FF => Effect::Ram(a & 0x7FF),
            0x4800..=0x4FFF => Effect::Video(a & 0x3FF),
            0x5000..=0x57FF => Effect::Object(a & 0xFF),
            0x5800..=0x58FF if ay => Effect::AyData,
            0x5900..=0x59FF if ay => Effect::AyAddress,
            0x6000..=0x67FF => match a & 7 {
                n @ 2..=6 => Effect::GfxBank(n - 2),
                _ => Effect::None,
            },
            0x7000..=0x77FF => match a & 7 {
                1 => Effect::NmiEnable,
                2 => Effect::CoinCounter(0),
                4 => Effect::Stars,
                6 => Effect::FlipX,
                7 => Effect::FlipY,
                _ => Effect::None,
            },
            _ => Effect::None,
        },
        Variant::KingBalloon => match a {
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
                0 => Effect::Speech(0),
                1 => Effect::NmiEnable,
                2 => Effect::Speech(1),
                3 => Effect::SpeechDip,
                6 => Effect::FlipX,
                7 => Effect::FlipY,
                // 4 would be the stars: not on this board.
                _ => Effect::None,
            },
            0xB800..=0xBFFF => Effect::Pitch,
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
        Variant::Konami(k) => match konami_decode(k, addr) {
            Decoded::Ram(o) => Effect::Ram(o),
            Decoded::Video(o) => Effect::Video(o),
            Decoded::Object(o) => Effect::Object(o),
            Decoded::Ppi(sel, reg) => Effect::Ppi(sel, reg),
            Decoded::Latch(l) => match l {
                KonamiLatch::Nmi => Effect::NmiEnable,
                KonamiLatch::CoinCounter(n) => Effect::CoinCounter(n),
                KonamiLatch::Background => Effect::Background,
                KonamiLatch::Stars => Effect::Stars,
                KonamiLatch::FlipX => Effect::FlipX,
                KonamiLatch::FlipY => Effect::FlipY,
                KonamiLatch::Red => Effect::BackgroundRgb(0),
                KonamiLatch::Green => Effect::BackgroundRgb(1),
                KonamiLatch::Blue => Effect::BackgroundRgb(2),
            },
            _ => Effect::None,
        },
    }
}

impl Galaxian {
    pub fn new(variant: Variant, rotation: Rotation, roms: &Roms) -> Result<Galaxian, String> {
        let rom = Rom::new(variant, roms)?;
        let mut hw = Hw::new(variant);
        if let Some(k) = variant.konami() {
            if roms.sound.len() < 0x800 {
                return Err("the sound board's program is missing".into());
            }
            let kind = if k == Konami::Frogger {
                konami_sound::Kind::Frogger
            } else {
                konami_sound::Kind::TwoAy
            };
            hw.konami = Some(KonamiSound::new(kind, &roms.sound));
            hw.protection_bits = roms.protection_bits;
        }
        let mut board = Galaxian {
            cpu: Z80::new(),
            hw,
            rom,
            rotation,
            raster: vec![0; RASTER_W * RASTER_H],
            video: vec![0; WIDTH * HEIGHT],
            raw: Vec::with_capacity(RAW_SAMPLES_PER_FRAME),
            audio: Vec::with_capacity(SAMPLES_PER_FRAME),
            main_trace: None,
        };
        if variant.konami().is_some() {
            // The PPIs' reset at power-on: every port an input, so the sound board's latch and
            // control read FFh (which silences it until the game sets the PPI up).
            board.reset_ppis(0);
        }
        Ok(board)
    }

    /// The PPIs come out of reset: their ports float high, and the second one's drive the sound
    /// board.
    fn reset_ppis(&mut self, at: u64) {
        for n in 0..2 {
            let out = self.hw.ppi[n].reset();
            self.ppi_outputs(n, out, None, i128::from(at) * AS_PER_CYCLE);
        }
    }

    /// The sound board, on Konami's boards.
    pub fn konami_sound(&self) -> Option<&KonamiSound> {
        self.hw.konami.as_ref()
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

    /// The last frame's sound as the circuit makes it, 192 kHz (3168 samples).
    pub fn raw_audio(&self) -> &[i16] {
        &self.raw
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
        // MAME's soft reset resets the speech CPU too.
        if let Some(sp) = self.hw.speech.as_mut() {
            sp.cpu.reset();
        }
        if self.hw.variant.konami().is_some() {
            // MAME's soft reset resets every device: the sound board (its Z80 and AYs) and the
            // PPIs, and restarts the stars' blink timer, which steps the counter at once.
            let at = self.hw.now;
            self.hw.blinks_at_reset = self.blinks(at);
            self.hw.reset_at = at;
            let when = i128::from(at) * AS_PER_CYCLE;
            self.hw.konami_events.push((when, konami_sound::Event::Reset));
            self.reset_ppis(at);
        }
    }

    /// The stars' blink counter at a moment, on Scramble's boards: its 555 (100 kΩ, 10 kΩ and
    /// 10 µF: 0.8316 s) steps it when the board is reset and every period after.
    fn blinks(&self, at: u64) -> u32 {
        let since = (i128::from(at) - i128::from(self.hw.reset_at)) * AS_PER_CYCLE;
        self.hw.blinks_at_reset.wrapping_add(1 + (since / BLINK_PERIOD) as u32)
    }

    /// Runs one frame, VBLANK to VBLANK.
    pub fn run_frame(&mut self) {
        self.frame(None);
    }

    /// The same, calling `trace` before each of the sound board's instructions (Konami's boards).
    pub fn run_frame_traced(&mut self, trace: &mut dyn FnMut(&Z80, u64)) {
        self.frame(Some(trace));
    }

    fn frame(&mut self, trace: Option<konami_sound::Trace>) {
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
        self.hw.draw_at = end - 1;
        self.draw_until(LAST_LINE);
        if self.hw.konami.is_some() {
            self.run_sound_board(trace);
        } else {
            self.run_speech(end);
            self.make_sound();
        }
        self.hw.frame_start = end;
        self.hw.frames += 1;
        self.turn_upright();
    }

    /// King & Balloon's speech board runs up to the frame's end (`end`, in main CPU cycles),
    /// each new command reaching its latch at the moment the main CPU sent it; its DAC's changes
    /// are kept for the frame's sound.
    fn run_speech(&mut self, end: u64) {
        // The noise line moves (a 17-bit shift register, stepped once a frame).
        let n = self.hw.noise;
        self.hw.noise = (n >> 1) | (((n ^ (n >> 3)) & 1) << 16);
        let rom = &self.rom.sound;
        let Some(sp) = self.hw.speech.as_mut() else { return };
        let stop = (u128::from(end) * u128::from(SPEECH_HZ) / u128::from(CPU_HZ)) as u64;
        let mut events = std::mem::take(&mut sp.latch_events);
        events.sort_by_key(|e| e.0);
        let mut next = events.into_iter().peekable();
        sp.dac_events.clear();
        while sp.now < stop {
            while let Some(&(t, v)) = next.peek() {
                if t > sp.now {
                    break;
                }
                sp.latch = v;
                next.next();
            }
            let mut bus = SpeechBus { rom, ram: &mut sp.ram, latch: sp.latch, dac: None };
            let t = sp.cpu.step(&mut bus);
            let dac = bus.dac;
            sp.now += u64::from(t);
            if let Some(d) = dac {
                // At the end of the OUT: back in main CPU cycles.
                let at = (u128::from(sp.now) * u128::from(CPU_HZ) / u128::from(SPEECH_HZ)) as u64 + 32;
                sp.dac_events.push((at, d));
            }
        }
        sp.latch_events = next.collect();
    }

    /// Jump Bug's sound: its AY-3-8910, clocked at 1.536 MHz, makes a sample every 8 of its clocks,
    /// 192 kHz, one per 16 CPU cycles; each write lands on the samples that start at or after it.
    /// The three channels are mixed the way MAME's model of the board's output turns each level
    /// into a voltage (measured from MAME, tests/ay8910/jumpbug.mame), each channel's rise above
    /// silence added; every sample comes within 2 of MAME's (tests/jumpbug_sound.rs).
    fn make_ay_sound(&mut self) {
        const FIXED: [i32; 16] = [
            -6144, -6032, -5999, -5955, -5886, -5786, -5664, -5364, -5181, -4568, -3962, -3246,
            -2272, -1239, 348, 2048,
        ];
        const ENVELOPE: [i32; 16] = [
            -6144, -6070, -6036, -5991, -5923, -5823, -5700, -5398, -5215, -4598, -3990, -3271,
            -2292, -1254, 340, 2048,
        ];
        let mut events = std::mem::take(&mut self.hw.ay_events);
        events.sort_by_key(|e| e.0);
        let mut next = events.iter().peekable();
        self.raw.clear();
        for n in 0..RAW_SAMPLES_PER_FRAME as u64 {
            let at = self.hw.frame_start + n * CYCLES_PER_SAMPLE;
            let mut v = 0i32;
            if let Some(ay) = self.hw.ay.as_mut() {
                while let Some(&&(_, address, val)) = next.peek().filter(|e| e.0 <= at) {
                    if address {
                        ay.write_address(val);
                    } else {
                        ay.write_data(val, &mut NoPorts);
                    }
                    next.next();
                }
                let levels = ay.tick();
                for (ch, &l) in levels.iter().enumerate() {
                    let table = if ay.regs[8 + ch] & 0x10 != 0 { &ENVELOPE } else { &FIXED };
                    v += table[usize::from(l & 0x0F)] - table[0];
                }
            }
            self.raw.push(v.clamp(-32768, 32767) as i16);
        }
        self.audio.clear();
        for c in self.raw.chunks_exact(4) {
            let sum: i32 = c.iter().map(|&s| i32::from(s)).sum();
            self.audio.push((sum / 4) as i16);
        }
    }

    /// Konami's sound board runs up to this frame's VBLANK, taking in what the main board sent.
    fn run_sound_board(&mut self, trace: Option<konami_sound::Trace>) {
        let vblank = i128::from(self.hw.frames + 1) * AS_PER_FRAME;
        let mut events = std::mem::take(&mut self.hw.konami_events);
        // The ones from after VBLANK (the last instruction's late writes) wait for the next frame.
        let later: Vec<_> = events.iter().copied().filter(|e| e.0 > vblank).collect();
        events.retain(|e| e.0 <= vblank);
        events.sort_by_key(|e| e.0);
        if let Some(snd) = self.hw.konami.as_mut() {
            snd.run(vblank, &events, &mut self.hw.konami_out, trace);
        }
        self.hw.konami_events = later;
        let n = SAMPLES_PER_FRAME.min(self.hw.konami_out.len());
        self.audio.clear();
        self.audio.extend(self.hw.konami_out.drain(..n));
        let last = self.audio.last().copied().unwrap_or(0);
        self.audio.resize(SAMPLES_PER_FRAME, last);
        self.raw.clear();
    }

    /// The frame's sound: each latch change applies to the samples that start at or after it.
    fn make_sound(&mut self) {
        if let Variant::JumpBug { .. } = self.hw.variant {
            self.make_ay_sound();
            return;
        }
        let events = std::mem::take(&mut self.hw.sound_events);
        let mut next = events.iter().peekable();
        self.raw.clear();
        for n in 0..RAW_SAMPLES_PER_FRAME as u64 {
            let at = self.hw.frame_start + n * CYCLES_PER_SAMPLE;
            while let Some(&&(_, l)) = next.peek().filter(|e| e.0 <= at) {
                self.hw.snd.set_latches(l);
                next.next();
            }
            let mut sample = i32::from(galaxian_sound::to_sample(self.hw.snd.step()));
            if let Some(sp) = self.hw.speech.as_mut() {
                while let Some(&(t, d)) = sp.dac_events.first() {
                    if t > at {
                        break;
                    }
                    sp.dac = d;
                    sp.dac_events.remove(0);
                }
                sample += speech_level(sp.dac);
            }
            self.raw.push(sample.clamp(-32768, 32767) as i16);
        }
        for &(_, l) in next {
            self.hw.snd.set_latches(l);
        }
        self.audio.clear();
        for four in self.raw.chunks_exact(4) {
            let sum: i32 = four.iter().map(|&s| i32::from(s)).sum();
            self.audio.push((sum / 4) as i16);
        }
    }

    /// Runs one instruction (and any interrupt before it), placing its writes in time.
    fn step(&mut self, vblank: u64) {
        let start = self.hw.now;
        if let Some(t) = self.main_trace.as_mut() {
            // MAME's trace leaves out the cycles a HALT waits; an NMI shows as its handler's
            // first instruction, 11 T-states on.
            if self.cpu.nmi_pending {
                t.push((start + 11, 0x66));
            } else if !self.cpu.halted {
                t.push((start, self.cpu.pc));
            }
        }
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
        self.hw.draw_at = at;
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
                self.hw.sound.lfo = (self.hw.sound.lfo & !(1 << n)) | u8::from(bit) << n;
                self.hw.sound_events.push((at, self.hw.sound));
            }
            Effect::Sound(n) => {
                self.hw.sound.sound = (self.hw.sound.sound & !(1 << n)) | u8::from(bit) << n;
                self.hw.sound_events.push((at, self.hw.sound));
            }
            Effect::Pitch => {
                self.hw.sound.pitch = val;
                self.hw.sound_events.push((at, self.hw.sound));
            }
            Effect::Speech(n) => {
                // B000h sets bit 0 (MAME ORs the whole byte in), B002h bit 1 and sends the latch.
                if n == 0 {
                    self.hw.speech_bits = (self.hw.speech_bits & !0x01) | val;
                } else {
                    self.hw.speech_bits = (self.hw.speech_bits & !0x02) | val << 1;
                    let v = self.hw.speech_bits | 0xF0;
                    if let Some(sp) = self.hw.speech.as_mut() {
                        let t = (u128::from(at) * u128::from(SPEECH_HZ) / u128::from(CPU_HZ)) as u64;
                        sp.latch_events.push((t, v));
                    }
                }
            }
            Effect::SpeechDip => self.hw.speech_dip = val,
            Effect::AyAddress => self.hw.ay_events.push((at, true, val)),
            Effect::AyData => self.hw.ay_events.push((at, false, val)),
            Effect::Background => {
                if self.hw.background != bit {
                    self.catch_up(at);
                    self.hw.background = bit;
                }
            }
            Effect::BackgroundRgb(n) => {
                if self.hw.background_rgb[n] != bit {
                    self.catch_up(at);
                    self.hw.background_rgb[n] = bit;
                }
            }
            Effect::Ppi(sel, reg) => {
                for n in 0..2 {
                    if sel & (1 << n) != 0 {
                        let out = self.hw.ppi[n].write(reg, val);
                        // The write stops the main CPU at the end of its bus cycle.
                        self.ppi_outputs(n, out, Some(at), i128::from(at + 3) * AS_PER_CYCLE);
                    }
                }
            }
        }
    }

    /// What a PPI's outputs drive. The second PPI's port A is the sound board's command latch and
    /// its port B the sound board's control; both reach the sound board through MAME's scheduler,
    /// which cuts the main CPU's timeslice at the end of the write (as switching the NMI off does).
    /// Port C is the protection chip on Scramble's and The End's boards, and The End's coin counter
    /// on the first PPI.
    /// `at` is the write's cycle (None for a reset), `when` the moment the sound board sees it.
    fn ppi_outputs(&mut self, n: usize, out: Outputs, at: Option<u64>, when: i128) {
        let Some(k) = self.hw.variant.konami() else { return };
        if n == 1 {
            if let Some(v) = out.a {
                self.hw.konami_events.push((when, konami_sound::Event::Latch(v)));
                if let Some(at) = at {
                    self.hw.sync_at = at + 3;
                }
            }
            if let Some(v) = out.b {
                self.hw.konami_events.push((when, konami_sound::Event::Control(v)));
                if self.hw.konami_control & 0x08 != 0 && v & 0x08 == 0 {
                    // The interrupt goes to the other CPU through the scheduler.
                    if let Some(at) = at {
                        self.hw.sync_at = at + 3;
                    }
                }
                self.hw.konami_control = v;
            }
            if let Some(v) = out.c {
                if matches!(k, Konami::Scramble | Konami::TheEnd) {
                    self.protection(v);
                }
            }
        } else if let Some(v) = out.c {
            if k == Konami::TheEnd {
                self.hw.coin_counter[0] = v & 0x80 != 0;
            }
        }
    }

    /// The protection chip (a PAL at 6J on Scramble's and The End's boards): it takes the low
    /// nibble of each write to port C into a three-nibble history and answers on the high
    /// nibble. Its logic isn't documented; this is what the games are known to expect of it (as
    /// MAME has it).
    fn protection(&mut self, data: u8) {
        let st = (self.hw.protection_state << 4) | u16::from(data & 0x0F);
        self.hw.protection_state = st;
        let num1 = (st >> 8 & 0x0F) as i32;
        let num2 = (st >> 4 & 0x0F) as i32;
        let r = &mut self.hw.protection_result;
        match st & 0x0F {
            0x6 => *r ^= 0x80,
            0x9 => *r = ((num1 + 1).min(0xF) << 4) as u8,
            0xB => *r = ((num2 - num1).max(0) << 4) as u8,
            0xA => *r = 0x00,
            0xF => *r = ((num1 - num2).max(0) << 4) as u8,
            _ => {}
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
        // Jump Bug's board has no watchdog.
        let counts = !matches!(self.hw.variant, Variant::JumpBug { .. });
        self.hw.watchdog -= u8::from(counts);
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
        let blink = (self.blinks(self.hw.draw_at) & 3) as usize;
        let hw = &self.hw;
        let rom = &self.rom;
        let row = (y - FIRST_LINE) * RASTER_W;
        let out = &mut self.raster[row..row + RASTER_W];
        let frogger = hw.variant == Variant::Konami(Konami::Frogger);

        match hw.variant {
            // Scramble's boards: blue through 390 Ω when switched on, and stars that stay put
            // (the generator restarts each line) and blink: the 555's count picks which colours
            // show, or on every other pair of lines none.
            Variant::Konami(Konami::Scramble | Konami::SuperCobra) => {
                out.fill(if hw.background { 0x00_00_56 } else { 0 });
                if hw.stars_enabled && (blink != 2 || y & 2 != 0) {
                    let mask = [0x20, 0x08, 0xFF, 0xFF][blink];
                    stars_row(out, &rom.stars, &rom.star_rgb, y, y as u32 * 512, mask);
                }
            }
            // Frogger: the river's half of the screen is blue (through 470 Ω), split at the
            // 128th dot (the other half when the screen is flipped).
            Variant::Konami(Konami::Frogger) => {
                let (river, road) = out.split_at_mut(128 * 3);
                if hw.flip_x {
                    river.fill(0);
                    road.fill(0x00_00_47);
                } else {
                    river.fill(0x00_00_47);
                    road.fill(0);
                }
            }
            // Turtles and Amidar: red, green and blue switched on by latches (390, 470 and 390 Ω).
            Variant::Konami(Konami::Turtles) => {
                let [r, g, b] = hw.background_rgb.map(u32::from);
                out.fill((r * 0x55) << 16 | (g * 0x47) << 8 | (b * 0x55));
            }
            // Jump Bug: black (its blue background has no switch on this board), and Galaxian's
            // stars kept out of the status area (the last 24 dots of the raster).
            Variant::JumpBug { .. } => {
                out.fill(0);
                if hw.stars_enabled {
                    let offs = hw.star_origin.wrapping_add(y as u32 * 512);
                    let (field, _) = out.split_at_mut(232 * 3);
                    stars_row(field, &rom.stars, &rom.star_rgb, y, offs, 0xFF);
                }
            }
            // The others: black, and the stars when they're on.
            _ => {
                out.fill(0);
                if hw.stars_enabled {
                    let offs = hw.star_origin.wrapping_add(y as u32 * 512);
                    stars_row(out, &rom.stars, &rom.star_rgb, y, offs, 0xFF);
                }
            }
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
            let mut scroll = hw.objram[col * 2];
            if frogger {
                // Frogger: the scroll's top and bottom four bits swapped on the way to the adder.
                scroll = scroll.rotate_left(4);
            }
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
            let y0 = if frogger { b[0].rotate_left(4) } else { b[0] };
            let mut sy = 240u8.wrapping_sub(y0.wrapping_sub(u8::from(n < 3)));
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
                let x = 255 - i32::from(shots[w * 4 + 3]);
                let (dots, colour) = match hw.variant {
                    // Scramble's boards: only "shells", yellow, two dots from FAh.
                    Variant::Konami(Konami::Scramble | Konami::SuperCobra) | Variant::JumpBug { .. } => {
                        (x - 6..x - 4, MISSILE)
                    }
                    // The End: Galaxian's, with blue and green swapped.
                    Variant::Konami(Konami::TheEnd) => {
                        (x - 4..x, colour & 0xFF0000 | (colour & 0xFF) << 8 | (colour >> 8 & 0xFF))
                    }
                    // A shot shows from its counter's FCh to 00h: four dots ending at its position.
                    _ => (x - 4..x, colour),
                };
                for dot in dots {
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
        let mut hw = hw;
        if let (Some(new), Some(old)) = (hw.konami.as_mut(), self.hw.konami.as_ref()) {
            // The sound board's program isn't in the state.
            new.restore_rom(old);
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
pub(crate) fn pause_point(start: u64, len: u32, cycles: &[u8], budget: u64) -> u64 {
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
fn extend_tile(hw: &Hw, code: &mut u16, colour: &mut u8) {
    if hw.variant == Variant::Konami(Konami::Frogger) {
        *colour = frogger_colour(*colour);
    }
    if let Variant::MoonCresta { .. } = hw.variant {
        let g = &hw.gfxbank;
        if g[2] != 0 && *code & 0xC0 == 0x80 {
            *code = (*code & 0x3F) | u16::from(g[0]) << 6 | u16::from(g[1]) << 7 | 0x100;
        }
    }
    if let Variant::JumpBug { .. } = hw.variant {
        let g = &hw.gfxbank;
        if *code & 0xC0 == 0x80 && g[2] & 1 != 0 {
            *code += 128 + (u16::from(g[0] & 1) << 6) + (u16::from(g[1] & 1) << 7) + (u16::from(!g[4] & 1) << 8);
        }
    }
    match hw.variant {
        Variant::Pisces { batman2: false } => *code |= u16::from(hw.gfxbank[0]) << 8,
        Variant::Pisces { batman2: true } if *code & 0x80 != 0 => *code |= u16::from(hw.gfxbank[0]) << 8,
        _ => {}
    }
}

/// King & Balloon's speech DAC (4 bits through an R-2R ladder) as a sample, alongside the sound
/// circuit's. The step, and the 32 main CPU cycles the DAC's changes come after its writes, are
/// measured from MAME's output (tests/kingball_sound.rs); the speech is an approximation of MAME's.
fn speech_level(dac: u8) -> i32 {
    i32::from(dac & 0x0F) * SPEECH_STEP
}

/// One step of the speech DAC, in the sound's 16-bit units.
const SPEECH_STEP: i32 = 1173;

/// Frogger's colour bits are wired one place round: bit 0 is the top one.
fn frogger_colour(c: u8) -> u8 {
    ((c >> 1) & 0x03) | ((c << 2) & 0x04)
}

fn extend_sprite(hw: &Hw, code: &mut u16, colour: &mut u8) {
    if hw.variant == Variant::Konami(Konami::Frogger) {
        *colour = frogger_colour(*colour);
    }
    if let Variant::MoonCresta { .. } = hw.variant {
        let g = &hw.gfxbank;
        if g[2] != 0 && *code & 0x30 == 0x20 {
            *code = (*code & 0x0F) | u16::from(g[0]) << 4 | u16::from(g[1]) << 5 | 0x40;
        }
    }
    if let Variant::JumpBug { .. } = hw.variant {
        let g = &hw.gfxbank;
        if *code & 0x30 == 0x20 && g[2] & 1 != 0 {
            *code += 32 + (u16::from(g[0] & 1) << 4) + (u16::from(g[1] & 1) << 5) + (u16::from(!g[4] & 1) << 6);
        }
    }
    match hw.variant {
        Variant::Pisces { batman2: false } => *code |= u16::from(hw.gfxbank[0]) << 6,
        Variant::Pisces { batman2: true } => *code += 0x40,
        _ => {}
    }
}

/// One line of stars, from the generator's state `offs` at the line's left edge. The generator
/// steps twice a pixel, on the 18.432 MHz clock's first and second ticks of three, so the first
/// state lights one dot and the second two. Stars only show where line bit 0 and dot bit 3
/// differ, and only colours with a bit in `mask` set (Scramble's blinking).
fn stars_row(out: &mut [u32], stars: &[u8], rgb: &[u32; 64], y: usize, offs: u32, mask: u8) {
    let mut offs = (offs % STAR_PERIOD) as usize;
    let period = STAR_PERIOD as usize;
    // As many dots as the row is given (Jump Bug's stop short of the status area).
    for x in 0..out.len() / 3 {
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
            sound: Vec::new(),
            protection_bits: 0,
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
