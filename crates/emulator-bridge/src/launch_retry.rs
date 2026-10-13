//! The owner's safety net for a game the default core won't open.
//!
//! When a system's DEFAULT core refuses a game at load (`retro_load_game` says no), and the system
//! has another core, the app tries that core once and tells the player in one casual line. Kart
//! Fighter is why: Mesen 2, the NES default, refuses any `.nes` whose header asks for more data
//! than the file holds, and FCEUmm plays the same file. Only the default is retried: a core the
//! player picked (Settings, or this game's own choice) is what they asked for, so its refusal is
//! shown as it is. And only once: the retry itself is never retried.
//!
//! The retry is an ordinary launch, so saves follow the normal rules: the battery save is shared
//! by the system's cores, and another core's auto-save is kept in a slot.

/// The core to retry on after `refused_core` refused a game: the first other core in
/// `system_cores`, only when `refused_was_default`. `None` when there is nothing to retry.
pub fn retry_core(system_cores: &[String], refused_core: &str, refused_was_default: bool) -> Option<String> {
    if !refused_was_default {
        return None;
    }
    system_cores.iter().find(|core| core.as_str() != refused_core).cloned()
}

/// The line the player sees when the retry worked. The names are the short emulator names.
pub fn retry_line(refused: &str, fallback: &str) -> String {
    format!("{refused} couldn\u{2019}t open this one, so it\u{2019}s running on {fallback}.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cores(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn a_refused_default_retries_on_the_other_core() {
        assert_eq!(retry_core(&cores(&["mesen2", "fceumm"]), "mesen2", true).as_deref(), Some("fceumm"));
        assert_eq!(retry_core(&cores(&["fceumm", "mesen2"]), "fceumm", true).as_deref(), Some("mesen2"));
    }

    #[test]
    fn a_core_the_player_picked_is_not_retried() {
        assert_eq!(retry_core(&cores(&["mesen2", "fceumm"]), "mesen2", false), None);
    }

    #[test]
    fn a_system_with_one_core_has_nothing_to_retry() {
        assert_eq!(retry_core(&cores(&["mgba"]), "mgba", true), None);
        assert_eq!(retry_core(&[], "mgba", true), None);
    }

    #[test]
    fn the_line_is_short_and_casual() {
        assert_eq!(retry_line("Mesen", "FCEUmm"), "Mesen couldn\u{2019}t open this one, so it\u{2019}s running on FCEUmm.");
    }
}
