//! cArcade (crate continuum_arcade): Continuum's own arcade emulator, written from scratch. The
//! plan, the boards in the order they come, and how each is proved are in docs/ARCADE_CORE.md.
//!
//! Modules: `z80` (the Zilog Z80 the boards run on), `mcs48` (the Intel 8035 that plays
//! Donkey Kong's music).

pub mod mcs48;
pub mod z80;
