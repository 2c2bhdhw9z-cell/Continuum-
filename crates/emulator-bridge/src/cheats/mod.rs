//! Cheats the engine owns, as opposed to the ones a core owns.
//!
//! A typed code (Game Genie, Action Replay, address:value) is handed to the core through
//! `retro_cheat_set` and the core decides what it means. Three things here are the engine's own,
//! because they have to behave the same on every core and every platform:
//!
//! - [`cht`]: reading a RetroArch `.cht` file into an ordered list.
//! - [`search`]: the classic RAM search over `SYSTEM_RAM`.
//! - [`poke`]: "write this value at this address every frame", which is what a search result
//!   becomes when the user makes a cheat out of it. Applied by the engine after each `run_frame`.
//!
//! [`formats`] is not engine behaviour but a description for the cheat screen: which kinds of
//! typed code each core reads, and which exact cartridge a GBA or Game Boy file is.

pub mod cht;
pub mod formats;
pub mod poke;
pub mod search;
