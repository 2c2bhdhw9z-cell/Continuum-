//! cMini, Continuum's own Pokémon Mini emulator. Its yardstick is PokeMini, the emulator it
//! replaces: the processor and every part of the console behave as PokeMini's do (whose timings
//! Team Pokeme measured on the console), checked instruction by instruction and frame by frame
//! against PokeMini itself (`tests/`, with the harnesses `.kiro/notes/wip/tools/pokemini` builds).
//! Written for cMini; nothing of PokeMini's is in it.

pub mod audio;
pub mod bios;
pub mod irq;
pub mod lcd;
pub mod libretro;
pub mod link;
pub mod machine;
pub mod minx;
pub mod options;
pub mod ports;
pub mod prc;
pub mod timers;
