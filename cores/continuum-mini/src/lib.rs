//! cMini, Continuum's original Pokémon Mini emulator. The implementation follows the public
//! MINX instruction descriptions, hardware register observations, and measured MAME 0.289 events.

pub mod audio;
pub mod eeprom;
pub mod libretro;
pub mod link;
pub mod machine;
pub mod minx;
pub mod options;
pub mod prc;
