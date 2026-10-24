//! Continuum's own GameCube and Wii emulator (working name continuum-gc).
//!
//! Written from scratch. The plan, the owner's rules and progress are in
//! `.kiro/notes/tasks/gamecube.json`; the settings inventory is
//! `.kiro/notes/docs/GC_OPTIONS_INVENTORY.md`.

pub mod bus;
pub mod cpu;
pub mod disc;
pub mod dol;
