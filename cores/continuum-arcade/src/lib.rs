//! cArcade (crate continuum_arcade): Continuum's own arcade emulator, written from scratch. The
//! plan, the boards in the order they come, and how each is proved are in .kiro/notes/docs/ARCADE_CORE.md.
//!
//! Modules: `z80` (the Zilog Z80 the boards run on), `mcs48` (the Intel 8035 that plays
//! Donkey Kong's music), `ay8910` (the AY-3-8910 sound chip on Konami's Scramble boards),
//! `namco_wsg` (Namco's three-voice waveform sound) and `pacman` (Namco's Pac-Man board); and the
//! libretro core around them: `zip` (reading a set's zip), `sets` (the sets it knows, and how a zip
//! is recognised as one), `board` (what every board does for the core), `options` (the DIP
//! switches as settings) and `libretro` (the entry points).

pub mod ay8910;
pub mod board;
pub mod galaxian;
pub mod galaxian_sound;
pub mod libretro;
pub mod mcs48;
pub mod namco_wsg;
pub mod options;
pub mod pacman;
mod set_list;
pub mod sets;
pub mod z80;
pub mod zip;
