//! cBoy (crate continuum_gb): Continuum's own Game Boy, Game Boy Color and Game Boy Advance core,
//! one core for all three (the ROM picks the machine), exposed through the libretro API.
//!
//! Modules: `cpu` (SM83), `bus` (memory map, DMA, serial, joypad), `cart` (MBC1/3/5, RTC),
//! `ppu`, `apu`, `timer`, `gb` (the machine). Designed so the Game Boy Color fits in later:
//! VRAM and WRAM banking and colour palettes go into `ppu` and `bus`, keyed by the header's CGB flag.

pub mod apu;
pub mod bus;
pub mod cart;
pub mod cpu;
pub mod gb;
pub mod gba;
pub mod libretro;
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
