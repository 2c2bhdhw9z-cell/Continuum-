//! The Swift-facing half of `core_updates`: whether an old save state loads on a newer core, and
//! the heads-up line after an emulator update.
//!
//! Free functions, like `uniffi_import.rs`: none of this touches a core or the engine lock.

use std::path::Path;

use crate::{core_updates, launch_retry};

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
