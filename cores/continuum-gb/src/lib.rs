//! Continuum GB: a Game Boy core written for Continuum, exposed through the libretro API.
//!
//! Modules: `cpu` (SM83), `bus` (memory map, DMA, serial, joypad), `cart` (MBC1/3/5, RTC),
//! `ppu`, `apu`, `timer`, `gb` (the machine). Designed so the Game Boy Color fits in later:
//! VRAM and WRAM banking and colour palettes go into `ppu` and `bus`, keyed by the header's CGB flag.

pub mod apu;
pub mod bus;
pub mod cart;
pub mod cpu;
pub mod gb;
pub mod libretro;
pub mod ppu;
pub mod timer;
