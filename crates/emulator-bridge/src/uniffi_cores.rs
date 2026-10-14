//! The Swift-facing half of `core_updates`: whether an old save state loads on a newer core, and
//! the heads-up line after an emulator update.
//!
//! Free functions, like `uniffi_import.rs`: none of this touches a core or the engine lock.

use std::path::Path;

use crate::{core_updates, launch_failure, launch_retry, snes_chips};

/// Once per app start, before the feedback session files in `dir` are rewritten. Returns "" or one
/// line for the activity log naming the emulators that changed since the last install.
#[uniffi::export]
pub fn core_updates_open(dir: String) -> String {
    core_updates::open(Path::new(&dir))
}

/// The heads-up for `core_id` if its emulator changed since the last install, once.
#[uniffi::export]
pub fn core_updates_take_heads_up(dir: String, core_id: String) -> Option<String> {
    core_updates::take_heads_up(Path::new(&dir), &core_id)
}

/// Whether a save state recorded with `saved_version` may load on `core_id` running
/// `running_version` (equal, missing, or a pair the engine records as proven).
#[uniffi::export]
pub fn save_state_version_loads(core_id: String, saved_version: Option<String>, running_version: Option<String>) -> bool {
    core_updates::state_version_loads(&core_id, saved_version.as_deref(), running_version.as_deref())
}

/// The core to try once after `refused_core` refused a game, or none (see `launch_retry`).
#[uniffi::export]
pub fn launch_retry_core(system_cores: Vec<String>, refused_core: String, refused_was_default: bool) -> Option<String> {
    launch_retry::retry_core(&system_cores, &refused_core, refused_was_default)
}

/// The player's line when the retry worked.
#[uniffi::export]
pub fn launch_retry_line(refused_name: String, fallback_name: String) -> String {
    launch_retry::retry_line(&refused_name, &fallback_name)
}

/// The player's line when a game will not start: which of the three things went wrong (the file,
/// another file it needs, or the phone's graphics), in plain words. `reason` is the engine's own
/// technical sentence, which belongs in the activity log and not on screen. `graphics` is true
/// when the engine raised a graphics error rather than a refused file.
#[uniffi::export]
pub fn launch_failure_line(game_name: String, core_name: String, reason: String, graphics: bool) -> String {
    launch_failure::line_for(&game_name, &core_name, &reason, graphics)
}

/// The player's line when `core_id` refused a game with `refusal` (the reason of a rejected
/// content error), for a core whose refusals are already written for players (cArcade); none for
/// every other core, whose line is `launch_failure_line`'s.
#[uniffi::export]
pub fn launch_refusal_line(core_id: String, refusal: String) -> Option<String> {
    launch_failure::own_line(&core_id, &refusal)
}

/// What a SNES cartridge with a chip inside it needs before it can start.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SnesChipNeed {
    /// The file names the core looks for, best first. Any one of them is enough.
    pub files: Vec<String>,
    /// The player's line, for when none of them is installed.
    pub line: String,
}

/// The chip file the SNES game at `path` needs, or none for a game that needs none (almost all of
/// them). The app checks the system folder for the names and shows `line` when none is there.
#[uniffi::export]
pub fn snes_chip_need(path: String, game_name: String) -> Option<SnesChipNeed> {
    let rom = std::fs::read(&path).ok()?;
    let chip = snes_chips::chip_file(&rom)?;
    Some(SnesChipNeed {
        files: chip.files.iter().map(|name| (*name).to_string()).collect(),
        line: chip.missing_line(&game_name),
    })
}
