//! Continuum's system ids to RetroAchievements console ids.
//!
//! The left column is the iOS app's `GameSystem` raw value, which is also what a future Android
//! shell will send. The right column is rcheevos' `RC_CONSOLE_*` from `rc_consoles.h`, line
//! numbers cited so a renumbering upstream is checkable by eye.

/// The RetroAchievements console id for a Continuum system id, or `None` when RetroAchievements
/// has no such console.
pub fn console_id(system: &str) -> Option<u32> {
    Some(match system {
        "genesis" | "md" => 1, // RC_CONSOLE_MEGA_DRIVE, rc_consoles.h line 16
        "n64" => 2,            // RC_CONSOLE_NINTENDO_64, line 17
        "snes" => 3,           // RC_CONSOLE_SUPER_NINTENDO, line 18
        "gb" => 4,             // RC_CONSOLE_GAMEBOY, line 19
        "gba" => 5,            // RC_CONSOLE_GAMEBOY_ADVANCE, line 20
        "gbc" => 6,            // RC_CONSOLE_GAMEBOY_COLOR, line 21
        "nes" => 7,            // RC_CONSOLE_NINTENDO, line 22
        "tg16" => 8,           // RC_CONSOLE_PC_ENGINE, line 23
        "sms" => 11,           // RC_CONSOLE_MASTER_SYSTEM, line 26
        "ps1" => 12,           // RC_CONSOLE_PLAYSTATION, line 27
        "gg" => 15,            // RC_CONSOLE_GAME_GEAR, line 30
        "ds" => 18,            // RC_CONSOLE_NINTENDO_DS, line 33
        "atari2600" => 25,     // RC_CONSOLE_ATARI_2600, line 40
        "sg1000" => 33,        // RC_CONSOLE_SG1000, line 48
        "psp" => 41,           // RC_CONSOLE_PSP, line 56
        "n3ds" => 62,          // RC_CONSOLE_NINTENDO_3DS, line 77
        "fds" => 81,           // RC_CONSOLE_FAMICOM_DISK_SYSTEM, line 96
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_system_has_a_console() {
        for system in [
            "nes", "snes", "gb", "gbc", "gba", "sms", "gg", "genesis", "ps1", "ds", "fds",
            "sg1000", "tg16", "atari2600", "n64", "n3ds", "psp",
        ] {
            assert!(console_id(system).is_some(), "{system}");
        }
        assert_eq!(console_id("switch"), None);
    }
}
