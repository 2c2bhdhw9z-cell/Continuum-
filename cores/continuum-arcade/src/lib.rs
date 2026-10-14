//! cArcade (crate continuum_arcade): Continuum's own arcade emulator, written from scratch. The
//! plan, the boards in the order they come, and how each is proved are in docs/ARCADE_CORE.md.
//!
//! Modules: `z80` (the Zilog Z80 the boards run on), `mcs48` (the Intel 8035 that plays
//! Donkey Kong's music), `ay8910` (the AY-3-8910 sound chip on Konami's Scramble boards).

pub mod ay8910;
pub mod mcs48;
pub mod z80;
