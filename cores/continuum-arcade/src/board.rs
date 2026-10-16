//! What every board does, as the libretro core sees it: run a frame with the cabinet's controls and
//! switches, hand over the picture (upright, 0x00RRGGBB) and the sound (mono, 48 kHz), and save
//! and restore its state. One implementation per board; `Image` builds one from a recognised set.

use crate::galaxian::{self, Galaxian};
use crate::pacman::{self, Pacman, Ports, Variant};
use crate::sets::{Board, Control, Found, SetSpec};

/// One player's side of the control panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Player {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// The panel's buttons, first to sixth.
    pub buttons: [bool; 6],
    pub coin: bool,
    pub start: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Controls {
    pub players: [Player; 2],
}

/// The cabinet's switches as the board's input ports see them before any control is pressed: one
/// byte per port, the board's own order (the Pac-Man board: IN0, IN1, DSW1, DSW2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Switches {
    pub ports: [u8; 4],
}

pub trait Machine: Send {
    /// Runs one frame, with the controls and switches as they are for all of it.
    fn run_frame(&mut self, controls: &Controls, switches: &Switches);
    /// The last frame, upright: pixels, width, height.
    fn picture(&self) -> (&[u32], usize, usize);
    /// The last frame's sound, mono, 48 kHz.
    fn sound(&self) -> &[i16];
    fn frames_per_second(&self) -> f64;
    /// The cabinet's stick only goes four ways (up, down, left, right, never a diagonal).
    fn four_way(&self) -> bool;
    fn save_state(&self) -> Vec<u8>;
    /// Restores `save_state`'s bytes; on an error the board is unchanged.
    fn load_state(&mut self, data: &[u8]) -> Result<(), String>;
}

/// A four-way stick's gate: it can only lean one way at a time. Pressing a diagonal on a pad keeps
/// the direction that was just added (moving from left to up through up-left turns up at once, as
/// leaning a real four-way stick round its gate does); straight into a diagonal from the middle,
/// the stick goes sideways.
#[derive(Clone, Copy, Debug, Default)]
pub struct FourWay {
    held: [bool; 4],
    out: [bool; 4],
}

impl FourWay {
    /// Up, down, left, right as pressed; returns them as the stick passes them on.
    pub fn update(&mut self, mut dirs: [bool; 4]) -> [bool; 4] {
        // A pad can't press both ways of one axis on a stick; neither counts.
        if dirs[0] && dirs[1] {
            dirs[0] = false;
            dirs[1] = false;
        }
        if dirs[2] && dirs[3] {
            dirs[2] = false;
            dirs[3] = false;
        }
        if dirs != self.held {
            let mut out = dirs;
            let vertical = out[0] || out[1];
            let horizontal = out[2] || out[3];
            if vertical && horizontal {
                // Keep only what changed since the last frame.
                for (o, h) in out.iter_mut().zip(self.held) {
                    *o &= !h;
                }
                if (out[0] || out[1]) && (out[2] || out[3]) {
                    out[0] = false;
                    out[1] = false;
                }
            }
            self.out = out;
            self.held = dirs;
        }
        self.out
    }
}

/// What a recognised set's chips make: everything needed to switch the board on, again and again
/// (the app's Restart is the power switch).
pub enum Image {
    Pacman(Variant, pacman::Roms),
    Galaxian(galaxian::Variant, galaxian::Rotation, galaxian::Roms, &'static SetSpec),
}

/// The Galaxian family's picture as the core hands it out: the board's 224 x 768 (three dots a
/// pixel along the raster, for the stars) averaged three by three into 224 x 256, shown at 3:4.
pub const GALAXIAN_HEIGHT: usize = galaxian::HEIGHT / 3;

impl Image {
    pub fn from_found(found: &Found) -> Result<Image, String> {
        let regions = found.regions()?;
        Ok(match found.spec.board {
            Board::Pacman(v) => Image::Pacman(v, found.pacman_roms(&regions)),
            Board::Galaxian(v, r, protection) => {
                Image::Galaxian(v, r, found.galaxian_roms(&regions, protection), found.spec)
            }
        })
    }

    /// The board, switched on.
    pub fn power_on(&self) -> Result<Box<dyn Machine>, String> {
        match self {
            Image::Pacman(v, roms) => Ok(Box::new(PacmanBoard(Pacman::new(*v, roms)?))),
            Image::Galaxian(v, r, roms, set) => Ok(Box::new(GalaxianBoard {
                board: Galaxian::new(*v, *r, roms)?,
                set,
                picture: vec![0; galaxian::WIDTH * GALAXIAN_HEIGHT],
            })),
        }
    }

    /// The picture's size, upright.
    pub fn size(&self) -> (usize, usize) {
        match self {
            Image::Pacman(..) => (pacman::WIDTH, pacman::HEIGHT),
            Image::Galaxian(..) => (galaxian::WIDTH, GALAXIAN_HEIGHT),
        }
    }
}

/// A board of the Galaxian family in its cabinet: the controls go to the input bits MAME's port
/// list gives for the set, each active high or low as wired.
pub struct GalaxianBoard {
    pub board: Galaxian,
    set: &'static SetSpec,
    picture: Vec<u32>,
}

impl GalaxianBoard {
    /// The ports for these controls and switches.
    pub fn ports(set: &SetSpec, controls: &Controls, switches: &Switches) -> galaxian::Ports {
        let mut p = switches.ports;
        for bit in set.inputs {
            let pl = |n: u8| &controls.players[usize::from(n).min(1)];
            let pressed = match bit.control {
                Control::Up(n) => pl(n).up,
                Control::Down(n) => pl(n).down,
                Control::Left(n) => pl(n).left,
                Control::Right(n) => pl(n).right,
                Control::Button(n, b) => pl(n).buttons.get(usize::from(b)).copied().unwrap_or(false),
                Control::Coin(n) => pl(n).coin,
                Control::Start(n) => pl(n).start,
            };
            if pressed {
                let r = &mut p[usize::from(bit.port)];
                if bit.active_high {
                    *r |= bit.mask;
                } else {
                    *r &= !bit.mask;
                }
            }
        }
        galaxian::Ports { in0: p[0], in1: p[1], in2: p[2], in3: p[3] }
    }
}

/// Averages each three rows of the board's picture (three dots of one raster pixel) into one.
pub fn galaxian_picture(video: &[u32], out: &mut [u32]) {
    let w = galaxian::WIDTH;
    for (y, row) in out.chunks_exact_mut(w).enumerate() {
        for (x, o) in row.iter_mut().enumerate() {
            let mut sum = [0u32; 3];
            for k in 0..3 {
                let p = video[(y * 3 + k) * w + x];
                sum[0] += p >> 16 & 0xFF;
                sum[1] += p >> 8 & 0xFF;
                sum[2] += p & 0xFF;
            }
            *o = ((sum[0] + 1) / 3) << 16 | ((sum[1] + 1) / 3) << 8 | ((sum[2] + 1) / 3);
        }
    }
}

impl Machine for GalaxianBoard {
    fn run_frame(&mut self, controls: &Controls, switches: &Switches) {
        self.board.set_ports(Self::ports(self.set, controls, switches));
        self.board.run_frame();
        galaxian_picture(self.board.video(), &mut self.picture);
    }

    fn picture(&self) -> (&[u32], usize, usize) {
        (&self.picture, galaxian::WIDTH, GALAXIAN_HEIGHT)
    }

    fn sound(&self) -> &[i16] {
        self.board.audio()
    }

    fn frames_per_second(&self) -> f64 {
        galaxian::FPS
    }

    fn four_way(&self) -> bool {
        self.set.four_way
    }

    fn save_state(&self) -> Vec<u8> {
        self.board.save_state()
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        self.board.load_state(data)?;
        galaxian_picture(self.board.video(), &mut self.picture);
        Ok(())
    }
}

/// The Pac-Man board in its cabinet: the controls go to its input ports, active low as wired.
struct PacmanBoard(Pacman);

impl Machine for PacmanBoard {
    fn run_frame(&mut self, controls: &Controls, switches: &Switches) {
        let [p1, p2] = controls.players;
        let bit = |on: bool, b: u8| if on { b } else { 0 };
        // IN0: player 1's stick, coin 1 and coin 2 (player 2's coin slot).
        let in0 = switches.ports[0]
            & !(bit(p1.up, 0x01) | bit(p1.left, 0x02) | bit(p1.right, 0x04) | bit(p1.down, 0x08))
            & !(bit(p1.coin, 0x20) | bit(p2.coin, 0x40));
        // IN1: player 2's stick (a cocktail table's other side), start 1 and start 2.
        let in1 = switches.ports[1]
            & !(bit(p2.up, 0x01) | bit(p2.left, 0x02) | bit(p2.right, 0x04) | bit(p2.down, 0x08))
            & !(bit(p1.start, 0x20) | bit(p2.start, 0x40));
        self.0.set_ports(Ports { in0, in1, dsw1: switches.ports[2], dsw2: switches.ports[3] });
        self.0.run_frame();
    }

    fn picture(&self) -> (&[u32], usize, usize) {
        (self.0.video(), pacman::WIDTH, pacman::HEIGHT)
    }

    fn sound(&self) -> &[i16] {
        self.0.audio()
    }

    fn frames_per_second(&self) -> f64 {
        pacman::FPS
    }

    fn four_way(&self) -> bool {
        true
    }

    fn save_state(&self) -> Vec<u8> {
        self.0.save_state()
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        self.0.load_state(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const U: [bool; 4] = [true, false, false, false];
    const D: [bool; 4] = [false, true, false, false];
    const L: [bool; 4] = [false, false, true, false];
    const R: [bool; 4] = [false, false, false, true];
    const UL: [bool; 4] = [true, false, true, false];
    const UR: [bool; 4] = [true, false, false, true];
    const NONE: [bool; 4] = [false; 4];

    /// The controls reach each set's input bits as MAME's port list wires them: Galaxian's are
    /// active high, Scramble's active low.
    #[test]
    fn the_controls_reach_the_generated_input_bits() {
        let rest = |set: &SetSpec| Switches { ports: set.rest };
        let galaxian = crate::sets::find("galaxian").unwrap();
        let mut c = Controls::default();
        c.players[0].left = true;
        c.players[0].coin = true;
        c.players[1].buttons[0] = true;
        let p = GalaxianBoard::ports(galaxian, &c, &rest(galaxian));
        assert_eq!((p.in0, p.in1, p.in2), (0x05, 0x10, 0x04));
        let scramble = crate::sets::find("scramble").unwrap();
        let mut c = Controls::default();
        c.players[0].buttons[0] = true;
        c.players[1].start = true;
        c.players[0].up = true;
        let p = GalaxianBoard::ports(scramble, &c, &rest(scramble));
        assert_eq!((p.in0, p.in1, p.in2), (0xF7, 0xBC, 0xE1));
    }

    #[test]
    fn a_four_way_stick_takes_the_newer_direction() {
        let mut s = FourWay::default();
        assert_eq!(s.update(L), L);
        // Left, then up is added: up.
        assert_eq!(s.update(UL), U);
        assert_eq!(s.update(UL), U, "held, it stays up");
        assert_eq!(s.update(U), U);
        // Up, then left is added again: left.
        assert_eq!(s.update(UL), L);
        // Straight from the middle into a diagonal: sideways.
        assert_eq!(s.update(NONE), NONE);
        assert_eq!(s.update(UR), R);
        // Both ways of an axis count as neither.
        assert_eq!(s.update([true, true, false, false]), NONE);
        assert_eq!(s.update(D), D);
    }
}
