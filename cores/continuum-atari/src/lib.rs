//! cAtari: Continuum's own Atari 7800 emulator, written from scratch in Rust and exposed as a
//! libretro core. Behaviour comes from Atari's own documents (the 7800 Software Guide and the
//! MARIA specification), the chips' data sheets, and how MAME 0.289's a7800 driver behaves; the
//! proofs are our own test programs run on both (.kiro/notes/docs/ATARI7800_CORE.md).

pub mod cart;
pub mod cart_list;
pub mod m6502;
pub mod machine;
pub mod maria;
pub mod pokey;
pub mod riot;
pub mod tia;
