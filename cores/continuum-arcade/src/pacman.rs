//! Namco's Pac-Man board (Puck Man, Pac-Man, Pac-Man Plus, Ms. Pac-Man and the clones and
//! bootlegs on the same board).
//!
//! A Z80 at 3.072 MHz (18.432 MHz / 6) runs 16 KB of program. The monitor is mounted on its side:
//! its raster is 288 × 224 at a 6.144 MHz pixel clock, 384 × 264 clocks a frame (60.606 Hz), and
//! the player sees it turned a quarter clockwise, 224 × 288. The picture is a 36 × 28 tile layer of
//! 8 × 8 tiles (ROM 5E) with an odd layout in video RAM, and eight 16 × 16 sprites (ROM 5F) on top;
//! each 2-bit pixel goes through the colour lookup PROM (4A) to one of the 16 colours of the palette
//! PROM (7F), whose bits drive the red, green and blue guns through resistors. Sound is Namco's
//! three-voice waveform generator (`namco_wsg`). VBLANK interrupts the Z80 with a vector the game
//! writes to I/O port 0, and a watchdog resets the board after 16 frames without a write to 50C0h.
//!
//! Written from the board's documented behaviour and from how MAME's pacman driver models it (the
//! reference this board's tests compare against, `tests/pacman_mame.rs`).

use serde::{Deserialize, Serialize};

use crate::namco_wsg::{self, Wsg};
use crate::z80::{Bus, Z80};

/// The picture as the player sees it, upright.
pub const WIDTH: usize = 224;
pub const HEIGHT: usize = 288;
/// The monitor's own raster, before it is turned upright.
const NATIVE_W: usize = 288;
const NATIVE_H: usize = 224;

pub const CPU_HZ: u32 = 3_072_000;
/// 384 × 264 pixel clocks a frame, and the Z80 runs at half the pixel clock.
pub const CYCLES_PER_FRAME: u32 = 384 * 264 / 2;
/// 6.144 MHz / (384 × 264): 60.606 frames a second.
pub const FPS: f64 = 6_144_000.0 / (384.0 * 264.0);
/// CPU cycles per sound sample at the sound model's 192 kHz.
const CYCLES_PER_SAMPLE: u64 = (CPU_HZ / namco_wsg::RATE) as u64;
/// The sound handed out, 48 kHz: four model samples averaged into one.
pub const SAMPLE_RATE: u32 = namco_wsg::RATE / 4;
pub const RAW_SAMPLES_PER_FRAME: usize = (CYCLES_PER_FRAME as u64 / CYCLES_PER_SAMPLE) as usize;
pub const SAMPLES_PER_FRAME: usize = RAW_SAMPLES_PER_FRAME / 4;

/// How the program ROMs sit on the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Variant {
    /// The plain board: 16 KB of program at 0000h, mirrored at 8000h (the Z80's A15 isn't wired).
    Plain,
    /// Pac-Man Plus: the plain board with its program scrambled; a custom chip decodes every byte
    /// by its address.
    PacPlus,
    /// Bootlegs that wire A15: a second 16 KB of program at 8000h.
    UpperRom,
    /// Ms. Pac-Man: the Pac-Man ROMs plus a daughterboard with three more ROMs (u5, u6, u7),
    /// whose wiring scrambles them, and which patches 40 small pieces of the Pac-Man program.
    MsPacman,
}

/// What the set list hands the board: its ROM regions, laid out as on the board.
#[derive(Clone, Debug)]
pub struct Roms {
    /// The Z80's view, 64 KB: the program at 0000h-3FFFh, and for `UpperRom` 8000h-BFFFh; for
    /// `MsPacman` the daughterboard's u5 at 8000h, u6 at 9000h and u7 at B000h.
    pub program: Vec<u8>,
    /// 5E (256 tiles) then 5F (64 sprites), 4 KB each.
    pub gfx: Vec<u8>,
    /// 7F, the 32-byte palette PROM.
    pub palette: Vec<u8>,
    /// 4A, the 256-entry colour lookup PROM (low nibble used).
    pub lookup: Vec<u8>,
    /// 1M, the sound PROM: eight 32-step waveforms (low nibble used).
    pub wave: Vec<u8>,
}

/// The board's inputs, as the ports read them (active low, as wired).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ports {
    /// 5000h: up, left, right, down, rack test, coin 1, coin 2, service credit.
    pub in0: u8,
    /// 5040h: player 2's stick, the test switch, start 1, start 2, upright (1) or cocktail (0).
    pub in1: u8,
    /// 5080h: the DIP switches.
    pub dsw1: u8,
    /// 50C0h: a second switch bank on a few boards; nothing on Pac-Man, which reads 00h.
    pub dsw2: u8,
}

impl Default for Ports {
    /// Nothing pressed, an upright cabinet, and the factory switches (1 coin 1 credit, 3 lives,
    /// bonus at 10000, normal difficulty and ghost names).
    fn default() -> Self {
        Ports {
            in0: 0xFF,
            in1: 0xFF,
            dsw1: 0xC9,
            dsw2: 0x00,
        }
    }
}

/// Everything ROM-derived, rebuilt on load and never saved.
struct Rom {
    /// The Z80's view of the program, 64 KB; `None` where a Ms. Pac-Man decode image isn't used.
    image: Vec<u8>,
    /// Ms. Pac-Man's daughterboard view (decoded aux ROMs and the patched Pac-Man program).
    decoded: Option<Vec<u8>>,
    /// Tile pixels, 256 tiles × 8 rows × 8 columns, in the raster's orientation.
    tiles: Vec<u8>,
    /// Sprite pixels, 64 sprites × 16 × 16.
    sprites: Vec<u8>,
    /// The lookup PROM's 128 tile/sprite colours × 4 pens, as palette indices.
    lookup: [u8; 256],
    /// The 32 palette entries as 0x00RRGGBB.
    rgb: [u32; 32],
    wave: [u8; 256],
}

/// The board's state: what a save state holds besides the Z80.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Hw {
    variant: Variant,
    vram: Vec<u8>,
    cram: Vec<u8>,
    /// 4C00h-4FFFh; the last 16 bytes (4FF0h-4FFFh) are the sprites' code, flips and colour.
    ram: Vec<u8>,
    /// 5060h-506Fh: the sprites' positions, written only.
    sprite_xy: [u8; 16],
    /// The 74LS259 at 8K, 5000h-5007h: bit 0 interrupt enable, 1 sound enable, 3 flip screen,
    /// 4 and 5 the start lamps, 6 coin lockout, 7 coin counter.
    latch: u8,
    /// The byte the board puts on the bus when the Z80 acknowledges an interrupt (port 0).
    vector: u8,
    /// The /INT line: set at VBLANK when interrupts are enabled, held until the game clears bit 0
    /// of the latch.
    irq: bool,
    /// VBLANKs left before the watchdog resets the board.
    watchdog: u8,
    /// Ms. Pac-Man's decode latch: the daughterboard's program (true) or plain Pac-Man (false).
    decode: bool,
    wsg: Wsg,
    ports: Ports,
    /// CPU cycles since power-on.
    now: u64,
    /// When the frame being run started; a frame runs from one VBLANK to the next.
    frame_start: u64,
    /// The machine has been running for a frame: the next frame starts with a VBLANK. (The first
    /// VBLANK comes one whole frame after power-on, as in MAME, where the raster starts there.)
    vblank_due: bool,
    /// Sound samples (192 kHz) made since power-on.
    sound_pos: u64,
    /// Samples already made for the next frame: an instruction that runs past the frame's end
    /// can write the sound registers there.
    pending: Vec<i32>,
    /// When the instruction being run started, so sound writes can be placed in time.
    #[serde(skip)]
    instr_start: u64,
    /// Sound-affecting writes made by the instruction being run, applied once its length is known.
    #[serde(skip)]
    timed: Vec<Timed>,
    /// The instruction being run may straddle VBLANK: its writes are logged as (address, value
    /// before, value after), so those that come after VBLANK can be left out of the picture.
    #[serde(skip)]
    at_vblank: bool,
    #[serde(skip)]
    log: Vec<(u16, Option<u8>, Option<u8>)>,
    /// The instruction's first two opcode bytes (a prefix and the opcode).
    #[serde(skip)]
    opcode: [u8; 2],
    #[serde(skip)]
    fetches: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Timed {
    Sound(u8, u8),
    Enable(bool),
}

/// The board.
pub struct Pacman {
    cpu: Z80,
    hw: Hw,
    rom: Rom,
    /// The last frame, upright, 0x00RRGGBB.
    video: Vec<u32>,
    /// The raster's pens before the picture is turned upright.
    native: Vec<u8>,
    /// The last frame's sound at 48 kHz.
    audio: Vec<i16>,
    /// The last frame's sound model output at 192 kHz, as voice sums (see `namco_wsg`).
    raw: Vec<i32>,
}

/// Ms. Pac-Man's daughterboard: an access to any of these 8-byte ROM regions switches the board
/// back to plain Pac-Man (the "latch clear" traps of US patent 4,525,599)...
const MSP_TRAPS_OFF: [u16; 7] = [0x0038, 0x03B0, 0x1600, 0x2120, 0x3FF0, 0x8000, 0x97F0];
/// ...and an access here switches it to Ms. Pac-Man. Every VBLANK fetches its interrupt vector
/// from this region, so the game keeps itself switched on.
const MSP_TRAP_ON: u16 = 0x3FF8;

/// The daughterboard's 40 patches: 8 bytes of its ROM (decoded 8000h-81FFh) replace the Pac-Man
/// program at each of these addresses while the board is switched to Ms. Pac-Man.
const MSP_PATCHES: [(u16, u16); 40] = [
    (0x0410, 0x8008),
    (0x08E0, 0x81D8),
    (0x0A30, 0x8118),
    (0x0BD0, 0x80D8),
    (0x0C20, 0x8120),
    (0x0E58, 0x8168),
    (0x0EA8, 0x8198),
    (0x1000, 0x8020),
    (0x1008, 0x8010),
    (0x1288, 0x8098),
    (0x1348, 0x8048),
    (0x1688, 0x8088),
    (0x16B0, 0x8188),
    (0x16D8, 0x80C8),
    (0x16F8, 0x81C8),
    (0x19A8, 0x80A8),
    (0x19B8, 0x81A8),
    (0x2060, 0x8148),
    (0x2108, 0x8018),
    (0x21A0, 0x81A0),
    (0x2298, 0x80A0),
    (0x23E0, 0x80E8),
    (0x2418, 0x8000),
    (0x2448, 0x8058),
    (0x2470, 0x8140),
    (0x2488, 0x8080),
    (0x24B0, 0x8180),
    (0x24D8, 0x80C0),
    (0x24F8, 0x81C0),
    (0x2748, 0x8050),
    (0x2780, 0x8090),
    (0x27B8, 0x8190),
    (0x2800, 0x8028),
    (0x2B20, 0x8100),
    (0x2B30, 0x8110),
    (0x2BF0, 0x81D0),
    (0x2CC0, 0x80D0),
    (0x2CD8, 0x80E0),
    (0x2CF0, 0x81E0),
    (0x2D60, 0x8160),
];

/// Builds a value from bits of `v`: `from[0]` names the source bit of the result's top bit, and
/// so on down to bit 0. This is how a board's crossed address or data lines are written down.
fn pick_bits(v: u32, from: &[u8]) -> u32 {
    let n = from.len();
    from.iter()
        .enumerate()
        .fold(0, |acc, (i, &b)| acc | ((v >> b) & 1) << (n - 1 - i))
}

/// The daughterboard's data lines, crossed between its ROMs and the Z80.
fn msp_data(v: u8) -> u8 {
    pick_bits(u32::from(v), &[0, 4, 5, 7, 6, 3, 2, 1]) as u8
}

/// Pac-Man Plus's program decoder: the data lines are crossed and inverted in one of six ways,
/// picked by address bits 0, 2, 5, 7 and 9, and swapped for a neighbouring way when bit 11 is set.
fn pacplus_decode(addr: usize, v: u8) -> u8 {
    const WAYS: [([u8; 8], u8); 6] = [
        ([7, 6, 5, 4, 3, 2, 1, 0], 0x00),
        ([7, 6, 5, 4, 3, 2, 1, 0], 0x28),
        ([6, 1, 3, 2, 5, 7, 0, 4], 0x96),
        ([6, 1, 5, 2, 3, 7, 0, 4], 0xBE),
        ([0, 3, 7, 6, 4, 2, 1, 5], 0xD5),
        ([0, 3, 4, 6, 7, 2, 1, 5], 0xDD),
    ];
    const PICK: [u8; 32] = [
        0, 2, 4, 2, 4, 0, 4, 2, 2, 0, 2, 2, 4, 0, 4, 2, 2, 2, 4, 0, 4, 2, 4, 0, 0, 4, 0, 4, 4, 2,
        4, 2,
    ];
    let sel = (addr & 1)
        | (addr >> 2 & 1) << 1
        | (addr >> 5 & 1) << 2
        | (addr >> 7 & 1) << 3
        | (addr >> 9 & 1) << 4;
    let way = usize::from(PICK[sel]) ^ (addr >> 11 & 1);
    let (bits, xor) = WAYS[way];
    pick_bits(u32::from(v), &bits) as u8 ^ xor
}

/// One of the three red or green levels, or two blue: the palette PROM's outputs drive the guns
/// through 1 kΩ, 470 Ω and 220 Ω resistors (blue: 470 Ω and 220 Ω), so each bit weighs in
/// proportion to its resistor's conductance; all bits on is full brightness.
fn gun(bits: u8, ohms: &[f64]) -> u32 {
    let total: f64 = ohms.iter().map(|r| 1.0 / r).sum();
    let level: f64 = ohms
        .iter()
        .enumerate()
        .filter(|&(i, _)| bits >> i & 1 != 0)
        .map(|(_, r)| 255.0 / r / total)
        .sum();
    (level + 0.5) as u32
}

impl Rom {
    fn new(variant: Variant, roms: &Roms) -> Result<Rom, String> {
        let need = |what: &str, v: &[u8], len: usize| -> Result<(), String> {
            if v.len() < len {
                Err(format!(
                    "{what} is {} bytes, the board needs {len}",
                    v.len()
                ))
            } else {
                Ok(())
            }
        };
        need("the program", &roms.program, 0xC000)?;
        need("the graphics", &roms.gfx, 0x2000)?;
        need("the palette PROM", &roms.palette, 32)?;
        need("the colour lookup PROM", &roms.lookup, 256)?;
        need("the sound PROM", &roms.wave, 256)?;

        let p = &roms.program;
        let mut image = vec![0u8; 0x10000];
        image[..0x4000].copy_from_slice(&p[..0x4000]);
        if variant == Variant::PacPlus {
            for (a, b) in image[..0x4000].iter_mut().enumerate() {
                *b = pacplus_decode(a, *b);
            }
        }
        if variant == Variant::UpperRom {
            image[0x8000..0xC000].copy_from_slice(&p[0x8000..0xC000]);
        } else {
            image.copy_within(0..0x4000, 0x8000);
        }

        let decoded = (variant == Variant::MsPacman).then(|| {
            let mut d = vec![0u8; 0x10000];
            d[..0x3000].copy_from_slice(&p[..0x3000]);
            // u7 (B000h in the region) shows at 3000h, its address lines crossed too.
            for i in 0..0x1000 {
                let a = pick_bits(i as u32, &[11, 3, 7, 9, 10, 8, 6, 5, 4, 2, 1, 0]) as usize;
                d[0x3000 + i] = msp_data(p[0xB000 + a]);
            }
            for i in 0..0x800 {
                // u5 at 8000h; u6's two halves, swapped, at 8800h and 9000h.
                let a5 = pick_bits(i as u32, &[8, 7, 5, 9, 10, 6, 3, 4, 2, 1, 0]) as usize;
                let a6 = pick_bits(i as u32, &[3, 7, 9, 10, 8, 6, 5, 4, 2, 1, 0]) as usize;
                d[0x8000 + i] = msp_data(p[0x8000 + a5]);
                d[0x8800 + i] = msp_data(p[0x9800 + a6]);
                d[0x9000 + i] = msp_data(p[0x9000 + a6]);
                // 9800h-9FFFh: the top half of the Pac-Man ROM at 1000h.
                d[0x9800 + i] = p[0x1800 + i];
            }
            // A000h-BFFFh: the Pac-Man ROMs at 2000h-3FFFh, unpatched.
            d[0xA000..0xC000].copy_from_slice(&p[0x2000..0x4000]);
            for &(to, from) in &MSP_PATCHES {
                let (to, from) = (usize::from(to), usize::from(from));
                d.copy_within(from..from + 8, to);
            }
            d
        });

        // The graphics: 2 bits a pixel, the two planes four pixels apart in each byte (high
        // nibble: bit 1 of four pixels, low nibble: bit 0), each tile in two 8-byte halves and
        // each sprite in four. Pixels here are in the raster's orientation (x along the scan).
        let px = |byte: u8, p: usize| ((byte >> (7 - p)) & 1) << 1 | ((byte >> (3 - p)) & 1);
        let g = &roms.gfx;
        let mut tiles = vec![0u8; 256 * 64];
        for code in 0..256 {
            for y in 0..8 {
                for x in 0..8 {
                    let byte = g[code * 16 + if x < 4 { 8 } else { 0 } + y];
                    tiles[code * 64 + y * 8 + x] = px(byte, x & 3);
                }
            }
        }
        let mut sprites = vec![0u8; 64 * 256];
        for code in 0..64 {
            for y in 0..16 {
                for x in 0..16 {
                    let quarter = [8, 16, 24, 0][x >> 2];
                    let byte =
                        g[0x1000 + code * 64 + quarter + (y & 7) + if y >= 8 { 32 } else { 0 }];
                    sprites[code * 256 + y * 16 + x] = px(byte, x & 3);
                }
            }
        }

        let mut lookup = [0u8; 256];
        for (l, &v) in lookup.iter_mut().zip(&roms.lookup) {
            *l = v & 0x0F;
        }
        let mut rgb = [0u32; 32];
        for (c, &v) in rgb.iter_mut().zip(&roms.palette) {
            let r = gun(v & 7, &[1000.0, 470.0, 220.0]);
            let g = gun(v >> 3 & 7, &[1000.0, 470.0, 220.0]);
            let b = gun(v >> 6 & 3, &[470.0, 220.0]);
            *c = r << 16 | g << 8 | b;
        }
        let mut wave = [0u8; 256];
        for (w, &v) in wave.iter_mut().zip(&roms.wave) {
            *w = v & 0x0F;
        }
        Ok(Rom {
            image,
            decoded,
            tiles,
            sprites,
            lookup,
            rgb,
            wave,
        })
    }
}

/// The Z80's view of the board while an instruction runs.
struct View<'a> {
    hw: &'a mut Hw,
    rom: &'a Rom,
}

impl View<'_> {
    /// Ms. Pac-Man's traps: any read, write or fetch in one switches the daughterboard.
    #[inline]
    fn trap(&mut self, addr: u16) {
        if self.rom.decoded.is_some() {
            let region = addr & 0xFFF8;
            if region == MSP_TRAP_ON {
                self.hw.decode = true;
            } else if MSP_TRAPS_OFF.contains(&region) {
                self.hw.decode = false;
            }
        }
    }
}

/// A read with no side effects: what the Z80 sees at `addr` (after any trap has switched).
fn read_board(hw: &Hw, rom: &Rom, addr: u16) -> u8 {
    if addr & 0x4000 == 0 {
        // ROM: 0000h-3FFFh and 8000h-BFFFh.
        return match (&rom.decoded, hw.decode) {
            (Some(d), true) => d[usize::from(addr)],
            _ => rom.image[usize::from(addr)],
        };
    }
    // 4000h-7FFFh, and again at C000h-FFFFh: A13 and A15 aren't decoded here.
    let a = usize::from(addr & 0x1FFF);
    match a {
        0x000..=0x3FF => hw.vram[a],
        0x400..=0x7FF => hw.cram[a - 0x400],
        // Nothing drives the bus at 4800h-4BFFh; MAME reads BFh there.
        0x800..=0xBFF => 0xBF,
        0xC00..=0xFFF => hw.ram[a - 0xC00],
        _ => match a & 0xC0 {
            0x00 => hw.ports.in0,
            0x40 => hw.ports.in1,
            0x80 => hw.ports.dsw1,
            _ => hw.ports.dsw2,
        },
    }
}

impl Bus for View<'_> {
    fn read(&mut self, addr: u16) -> u8 {
        if addr & 0x4000 == 0 {
            self.trap(addr);
        }
        read_board(self.hw, self.rom, addr)
    }

    fn fetch(&mut self, addr: u16) -> u8 {
        let v = self.read(addr);
        if self.hw.fetches < 2 {
            self.hw.opcode[usize::from(self.hw.fetches)] = v;
            self.hw.fetches += 1;
        }
        v
    }

    fn write(&mut self, addr: u16, val: u8) {
        if self.hw.at_vblank {
            let before = self.hw.cell(addr).map(|c| *c);
            self.store(addr, val);
            let after = self.hw.cell(addr).map(|c| *c);
            self.hw.log.push((addr, before, after));
        } else {
            self.store(addr, val);
        }
    }

    fn input(&mut self, _port: u16) -> u8 {
        // No I/O port is read on this board; MAME's unmapped I/O reads 00h.
        0x00
    }

    fn output(&mut self, port: u16, val: u8) {
        // Port 0 latches the interrupt vector (A0-A7 decoded, as MAME maps it).
        if port & 0xFF == 0 {
            self.hw.vector = val;
        }
    }

    fn int_ack(&mut self) -> u8 {
        self.hw.vector
    }
}

impl View<'_> {
    /// A write's effect on the board.
    fn store(&mut self, addr: u16, val: u8) {
        if addr & 0x4000 == 0 {
            self.trap(addr);
            return;
        }
        let a = usize::from(addr & 0x1FFF);
        match a {
            0x000..=0x3FF => self.hw.vram[a] = val,
            0x400..=0x7FF => self.hw.cram[a - 0x400] = val,
            0x800..=0xBFF => {}
            0xC00..=0xFFF => self.hw.ram[a - 0xC00] = val,
            _ => match a & 0xFF {
                // The latch: the address picks the bit, data bit 0 is its new value.
                0x00..=0x3F => {
                    let bit = a & 7;
                    let on = val & 1 != 0;
                    self.hw.latch = (self.hw.latch & !(1 << bit)) | u8::from(on) << bit;
                    match bit {
                        0 if !on => self.hw.irq = false,
                        1 => self.hw.timed.push(Timed::Enable(on)),
                        _ => {}
                    }
                }
                0x40..=0x5F => self.hw.timed.push(Timed::Sound((a & 0x1F) as u8, val)),
                0x60..=0x6F => self.hw.sprite_xy[a & 0x0F] = val,
                0x70..=0xBF => {}
                // 50C0h: kicks the watchdog.
                _ => self.hw.watchdog = 16,
            },
        }
    }
}

impl Hw {
    fn new(variant: Variant) -> Hw {
        Hw {
            variant,
            vram: vec![0; 0x400],
            cram: vec![0; 0x400],
            ram: vec![0; 0x400],
            sprite_xy: [0; 16],
            latch: 0,
            vector: 0,
            irq: false,
            watchdog: 16,
            decode: true,
            wsg: Wsg::new(),
            ports: Ports::default(),
            now: 0,
            frame_start: 0,
            vblank_due: false,
            sound_pos: 0,
            pending: Vec::new(),
            instr_start: 0,
            timed: Vec::new(),
            at_vblank: false,
            log: Vec::new(),
            opcode: [0; 2],
            fetches: 0,
        }
    }

    /// The byte a write to `addr` lands in, where it shows in the picture: video and colour RAM,
    /// work RAM (the sprites' codes are at its end), the sprite positions, and the latch (for
    /// its flip bit).
    fn cell(&mut self, addr: u16) -> Option<&mut u8> {
        if addr & 0x4000 == 0 {
            return None;
        }
        let a = usize::from(addr & 0x1FFF);
        match a {
            0x000..=0x3FF => Some(&mut self.vram[a]),
            0x400..=0x7FF => Some(&mut self.cram[a - 0x400]),
            0xC00..=0xFFF => Some(&mut self.ram[a - 0xC00]),
            0x1000.. => match a & 0xFF {
                0x00..=0x3F if a & 7 == 3 => Some(&mut self.latch),
                0x60..=0x6F => Some(&mut self.sprite_xy[a & 0x0F]),
                _ => None,
            },
            _ => None,
        }
    }
}

impl Pacman {
    pub fn new(variant: Variant, roms: &Roms) -> Result<Pacman, String> {
        let rom = Rom::new(variant, roms)?;
        Ok(Pacman {
            cpu: Z80::new(),
            hw: Hw::new(variant),
            rom,
            video: vec![0; WIDTH * HEIGHT],
            native: vec![0; NATIVE_W * NATIVE_H],
            audio: Vec::with_capacity(SAMPLES_PER_FRAME),
            raw: Vec::with_capacity(RAW_SAMPLES_PER_FRAME),
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

    /// The last frame, upright, 224 × 288, 0x00RRGGBB.
    pub fn video(&self) -> &[u32] {
        &self.video
    }

    /// The last frame's sound, mono, 48 kHz (792 samples).
    pub fn audio(&self) -> &[i16] {
        &self.audio
    }

    /// The last frame's sound model output before it is cut to 48 kHz: 3168 samples at 192 kHz,
    /// each the voices' sum (-360 to 315; `namco_wsg::to_i16` makes it a 16-bit sample).
    pub fn raw_audio(&self) -> &[i32] {
        &self.raw
    }

    /// Frames run since power-on (the watchdog's resets don't restart the count).
    pub fn frame_count(&self) -> u64 {
        self.hw.frame_start / u64::from(CYCLES_PER_FRAME)
    }

    pub fn cpu(&self) -> &Z80 {
        &self.cpu
    }

    /// A byte as the Z80 would read it, without setting off Ms. Pac-Man's traps.
    pub fn peek(&self, addr: u16) -> u8 {
        read_board(&self.hw, &self.rom, addr)
    }

    /// Ms. Pac-Man's decode latch: true while the daughterboard's program shows.
    pub fn decoding(&self) -> bool {
        self.hw.decode
    }

    /// The reset button (and the watchdog's reset): the Z80 restarts at 0000h and the latch
    /// clears, which switches off interrupts, sound and the screen flip. Memory, the sound
    /// registers and Ms. Pac-Man's decode latch keep what they held.
    pub fn reset(&mut self) {
        self.cpu.reset();
        self.cpu.set_irq(false);
        self.hw.latch = 0;
        self.hw.irq = false;
        self.hw.watchdog = 16;
        self.hw.wsg.enabled = false;
    }

    /// Runs one frame, VBLANK to VBLANK: the interrupt and the watchdog at its start, the picture
    /// as it stands at its end (the next VBLANK, where the beam has drawn the whole screen).
    pub fn run_frame(&mut self) {
        self.audio.clear();
        if self.hw.vblank_due {
            self.vblank();
        }
        self.hw.vblank_due = true;
        let end = self.hw.frame_start + u64::from(CYCLES_PER_FRAME);
        // VBLANK comes during the frame's last CPU cycle: the pixel clock runs at twice the
        // Z80's, from the same crystal, and MAME puts it there too. An instruction ending on that
        // cycle's start already sees the interrupt.
        let vblank = end - 1;
        let mut late = Vec::new();
        while self.hw.now < vblank {
            let start = self.hw.now;
            self.hw.instr_start = start;
            // No instruction is longer than 23 T-states: one starting this close may straddle
            // VBLANK, so its writes are logged.
            self.hw.at_vblank = start + 24 > vblank;
            self.hw.fetches = 0;
            let t = self.cpu.step(&mut View {
                hw: &mut self.hw,
                rom: &self.rom,
            });
            self.hw.now += u64::from(t);
            if !self.hw.timed.is_empty() {
                self.apply_timed(start, t);
            }
            if self.hw.at_vblank {
                let mut log = std::mem::take(&mut self.hw.log);
                if start + u64::from(t) > vblank {
                    late = self.after_vblank(start, t, vblank, &log);
                }
                log.clear();
                self.hw.log = log;
            }
            self.cpu.set_irq(self.hw.irq);
        }
        self.hw.at_vblank = false;
        self.sound_until(end / CYCLES_PER_SAMPLE);
        self.hw.frame_start = end;
        // This frame's 3168 samples; any made past its end wait for the next one.
        let rest = self.hw.pending.split_off(RAW_SAMPLES_PER_FRAME);
        self.raw = std::mem::replace(&mut self.hw.pending, rest);
        for chunk in self.raw.chunks_exact(4) {
            let sum: i32 = chunk.iter().map(|&s| i32::from(namco_wsg::to_i16(s))).sum();
            self.audio.push((sum / 4) as i16);
        }
        // The picture is taken at VBLANK: without the writes the last instruction made after it.
        for &(addr, before, _) in late.iter().rev() {
            if let (Some(c), Some(b)) = (self.hw.cell(addr), before) {
                *c = b;
            }
        }
        self.render();
        for &(addr, _, after) in &late {
            if let (Some(c), Some(a)) = (self.hw.cell(addr), after) {
                *c = a;
            }
        }
    }

    /// Which of the writes of an instruction that straddled VBLANK (at `vblank`) came after it. The
    /// Z80 writes at the start of a write's machine cycle: the last three T-states of most
    /// instructions (the write before it three earlier, and so on), and T-state 11 of LDI, LDD,
    /// LDIR and LDDR, which spend their last 5 (or 10) T-states counting.
    fn after_vblank(
        &self,
        start: u64,
        t: u32,
        vblank: u64,
        log: &[(u16, Option<u8>, Option<u8>)],
    ) -> Vec<(u16, Option<u8>, Option<u8>)> {
        let block =
            self.hw.opcode[0] == 0xED && matches!(self.hw.opcode[1], 0xA0 | 0xA8 | 0xB0 | 0xB8);
        let n = log.len() as u64;
        log.iter()
            .enumerate()
            .filter(|&(i, _)| {
                let at = if block {
                    start + 11
                } else {
                    (start + u64::from(t)).saturating_sub(3 * (n - i as u64))
                };
                at >= vblank
            })
            .map(|(_, &w)| w)
            .collect()
    }

    /// VBLANK begins: the watchdog counts it, then the interrupt is raised if enabled.
    fn vblank(&mut self) {
        self.hw.watchdog -= 1;
        if self.hw.watchdog == 0 {
            // The reset cuts into the instruction running at VBLANK at its next machine cycle;
            // a halted Z80's NOP has one halfway through. (The rest of an instruction runs to its
            // end first here, so a reset can come a cycle or two late, but not from HALT.)
            let vblank = self.hw.frame_start - 1;
            if self.cpu.halted && self.hw.now > vblank && self.hw.now - 4 < vblank {
                let s = self.hw.now - 4;
                self.hw.now = s + 2 * (vblank - s).div_ceil(2);
            }
            self.reset();
        }
        if self.hw.latch & 1 != 0 {
            self.hw.irq = true;
        }
        self.cpu.set_irq(self.hw.irq);
    }

    /// Applies an instruction's sound writes where they fell: a Z80 write is the last three
    /// T-states of its instruction (the one before it three earlier, and so on; exact for the
    /// stores games use), and the sound model catches up to that moment first.
    fn apply_timed(&mut self, start: u64, t: u32) {
        let timed = std::mem::take(&mut self.hw.timed);
        let n = timed.len() as u64;
        for (i, w) in timed.iter().enumerate() {
            let at = (start + u64::from(t)).saturating_sub(3 * (n - i as u64));
            // It changes the 192 kHz samples that start at or after it.
            self.sound_until(at.div_ceil(CYCLES_PER_SAMPLE));
            match *w {
                Timed::Sound(reg, val) => self.hw.wsg.write(reg, val),
                Timed::Enable(on) => self.hw.wsg.enabled = on,
            }
        }
        self.hw.timed = timed;
        self.hw.timed.clear();
    }

    /// Makes the sound up to (not including) 192 kHz sample `upto`.
    fn sound_until(&mut self, upto: u64) {
        while self.hw.sound_pos < upto {
            let s = self.hw.wsg.sample(&self.rom.wave);
            self.hw.pending.push(s);
            self.hw.sound_pos += 1;
        }
    }

    /// Draws the frame: the tile layer, then the sprites, in the raster's orientation, then turns
    /// it upright for the player.
    fn render(&mut self) {
        let rom = &self.rom;
        let hw = &self.hw;
        let flip = hw.latch & 8 != 0;
        // The tile layer: 36 columns across the raster (rows on the upright screen) by 28. The
        // middle 32 columns are stored column by column; the two at each end (the score and
        // status rows at the top and bottom of the upright screen) are stored row-wise at the
        // end and start of video RAM.
        for row in 0..28usize {
            for col in 0..36usize {
                let r = row + 2;
                let c = col.wrapping_sub(2);
                let offset = if c & 0x20 != 0 {
                    r + ((c & 0x1F) << 5)
                } else {
                    c + (r << 5)
                };
                let code = usize::from(hw.vram[offset]);
                let colour = usize::from(hw.cram[offset] & 0x1F) * 4;
                let pix = &rom.tiles[code * 64..code * 64 + 64];
                for y in 0..8 {
                    for x in 0..8 {
                        let (nx, ny) = (col * 8 + x, row * 8 + y);
                        let (nx, ny) = if flip {
                            (NATIVE_W - 1 - nx, NATIVE_H - 1 - ny)
                        } else {
                            (nx, ny)
                        };
                        self.native[ny * NATIVE_W + nx] =
                            rom.lookup[colour + usize::from(pix[y * 8 + x])];
                    }
                }
            }
        }
        // The sprites: eight, the last drawn first so sprite 0 is on top, kept off the two status
        // columns at each end. A pen whose lookup colour is 0 is see-through. Sprites 0-2 sit one
        // pixel further along than the rest, a quirk of the sprite hardware's line buffer. Each is
        // drawn again 256 pixels back, so one leaving one edge comes in at the other.
        for i in (0..8usize).rev() {
            let attr = hw.ram[0x3F0 + 2 * i];
            let code = usize::from(attr >> 2);
            let (fx, fy) = (attr & 1 != 0, attr & 2 != 0);
            let colour = usize::from(hw.ram[0x3F1 + 2 * i] & 0x1F) * 4;
            let sx = 272 - i32::from(hw.sprite_xy[2 * i + 1]);
            let sy = i32::from(hw.sprite_xy[2 * i]) - 31 + i32::from(i <= 2);
            let pix = &rom.sprites[code * 256..code * 256 + 256];
            for x0 in [sx, sx - 256] {
                for dy in 0..16i32 {
                    let y = sy + dy;
                    if !(0..NATIVE_H as i32).contains(&y) {
                        continue;
                    }
                    let syy = if fy { 15 - dy } else { dy } as usize;
                    for dx in 0..16i32 {
                        let x = x0 + dx;
                        if !(16..272).contains(&x) {
                            continue;
                        }
                        let sxx = if fx { 15 - dx } else { dx } as usize;
                        let pen = rom.lookup[colour + usize::from(pix[syy * 16 + sxx])];
                        if pen != 0 {
                            self.native[y as usize * NATIVE_W + x as usize] = pen;
                        }
                    }
                }
            }
        }
        // Turned a quarter clockwise: the raster's left edge is the screen's top, its bottom
        // edge the screen's left.
        for uy in 0..HEIGHT {
            for ux in 0..WIDTH {
                let pen = self.native[(NATIVE_H - 1 - ux) * NATIVE_W + uy];
                self.video[uy * WIDTH + ux] = rom.rgb[usize::from(pen)];
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
            bincode::deserialize(data).map_err(|e| format!("not a Pac-Man board state: {e}"))?;
        if hw.variant != self.hw.variant {
            return Err("that state is from a different board".into());
        }
        self.cpu = cpu;
        self.hw = hw;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_roms(program: &[u8]) -> Roms {
        let mut p = vec![0u8; 0x10000];
        p[..program.len()].copy_from_slice(program);
        Roms {
            program: p,
            gfx: vec![0; 0x2000],
            palette: vec![0; 32],
            lookup: vec![0; 256],
            wave: vec![0; 256],
        }
    }

    #[test]
    fn frame_timing() {
        assert_eq!(CYCLES_PER_FRAME, 50_688);
        assert!((FPS - 60.606_06).abs() < 1e-4);
        assert_eq!(RAW_SAMPLES_PER_FRAME, 3168);
        assert_eq!(SAMPLES_PER_FRAME, 792);
    }

    #[test]
    fn palette_levels_from_the_resistors() {
        let reds: Vec<u32> = (0..8).map(|b| gun(b, &[1000.0, 470.0, 220.0])).collect();
        assert_eq!(reds, [0, 33, 71, 104, 151, 184, 222, 255]);
        let blues: Vec<u32> = (0..4).map(|b| gun(b, &[470.0, 220.0])).collect();
        assert_eq!(blues, [0, 81, 174, 255]);
    }

    #[test]
    fn crossed_lines() {
        assert_eq!(
            pick_bits(0b1000_0000, &[0, 4, 5, 7, 6, 3, 2, 1]),
            0b0001_0000
        );
        assert_eq!(msp_data(0x01), 0x80);
        // Pac-Man Plus way 0 is the identity; way 1 only inverts bits 3 and 5.
        assert_eq!(pacplus_decode(0, 0x5A), 0x5A);
        assert_eq!(pacplus_decode(0x800, 0x5A), 0x5A ^ 0x28);
    }

    #[test]
    fn watchdog_resets_after_16_vblanks_without_a_kick() {
        // JR $ : spin forever without touching 50C0h.
        let mut b = Pacman::new(Variant::Plain, &blank_roms(&[0x18, 0xFE])).unwrap();
        for _ in 0..16 {
            b.run_frame();
        }
        // VBLANK 15 (frame 16's start) has counted down to 1; the next one resets.
        assert_eq!(b.hw.watchdog, 1);
        b.hw.latch = 0x02;
        b.run_frame();
        assert_eq!(b.hw.watchdog, 16);
        assert_eq!(b.hw.latch, 0, "the reset clears the latch");
    }

    #[test]
    fn save_state_round_trip() {
        let mut b = Pacman::new(Variant::Plain, &blank_roms(&[0x3C, 0x18, 0xFD])).unwrap();
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
