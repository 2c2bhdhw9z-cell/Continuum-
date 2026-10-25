//! cBoy (crate continuum_gb): Continuum's own Game Boy, Game Boy Color and Game Boy Advance core,
//! one core for all three (the ROM picks the machine), exposed through the libretro API.
//!
//! Modules: `cpu` (SM83), `bus` (memory map, DMA and HDMA, the link port, joypad), `cart` (no
//! mapper, MBC1, MBC2, MBC3 and MBC30 with the clock, MBC5, MBC7, HuC1, HuC3, the Game Boy
//! Camera, and the cut-off-file check), `ppu` (with the two picture drawers; fifo.rs is the exact
//! one), `apu`, `timer`, `sgb` (Super Game Boy), `link` (the cable between two phones), `dual` (two
//! Game Boys on one phone), `gb` (the machine), `compat` (the colours a Game Boy Color picks for a black-and-white game), `gba` (the
//! Advance), `options` (the core options) and `libretro` (the entry points).
//! Game Boy Color mode is keyed by the header's CGB flag (VRAM and WRAM banks and colour palettes
//! in `ppu` and `bus`).

pub mod apu;
pub mod bus;
pub mod cart;
pub mod compat;
pub mod cpu;
pub mod dual;
pub mod gb;
pub mod gba;
pub mod libretro;
pub mod link;
pub mod options;
pub mod ppu;
pub mod sgb;
pub mod timer;

/// Decodes a save state body. Same encoding as `bincode::serialize`, but no allocation may
/// exceed the input's size, so a corrupt length field is refused instead of exhausting memory.
pub(crate) fn state_decode<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, String> {
    use bincode::Options;
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .with_limit(body.len() as u64)
        .deserialize(body)
        .map_err(|e| e.to_string())
}
