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
        // SuperGrafx games are hashed and listed under the PC Engine on RetroAchievements.
        "sgx" => 8,        // RC_CONSOLE_PC_ENGINE, line 23
        "segacd" => 9,     // RC_CONSOLE_SEGA_CD, line 24
        "sega32x" => 10,   // RC_CONSOLE_SEGA_32X, line 25
        "lynx" => 13,      // RC_CONSOLE_ATARI_LYNX, line 28
        "ngp" => 14,       // RC_CONSOLE_NEOGEO_POCKET, line 29
        "jaguar" => 17,    // RC_CONSOLE_ATARI_JAGUAR, line 32
        "pokemini" => 24,  // RC_CONSOLE_POKEMON_MINI, line 39
        "dos" => 26,       // RC_CONSOLE_MS_DOS, line 41
        "arcade" => 27,    // RC_CONSOLE_ARCADE, line 42
        "vb" => 28,        // RC_CONSOLE_VIRTUAL_BOY, line 43
        "c64" => 30,       // RC_CONSOLE_COMMODORE_64, line 45
        "amiga" => 35,     // RC_CONSOLE_AMIGA, line 50
        "saturn" => 39,    // RC_CONSOLE_SATURN, line 54
        "dreamcast" => 40, // RC_CONSOLE_DREAMCAST, line 55
        "atari5200" => 50, // RC_CONSOLE_ATARI_5200, line 65
        "atari7800" => 51, // RC_CONSOLE_ATARI_7800, line 66
        "wswan" => 53,     // RC_CONSOLE_WONDERSWAN, line 68
        "pcecd" => 76,     // RC_CONSOLE_PC_ENGINE_CD, line 91
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

    #[test]
    fn wave_two_systems_map_to_their_consoles() {
        for (system, id) in [
            ("wswan", 53), ("ngp", 14), ("pcecd", 76), ("sgx", 8), ("amiga", 35), ("c64", 30),
            ("dos", 26), ("jaguar", 17), ("lynx", 13), ("atari7800", 51), ("atari5200", 50),
            ("arcade", 27), ("pokemini", 24), ("vb", 28), ("saturn", 39), ("segacd", 9),
            ("sega32x", 10), ("dreamcast", 40),
        ] {
            assert_eq!(console_id(system), Some(id), "{system}");
        }
        // DOOM is not a console RetroAchievements tracks.
        assert_eq!(console_id("doom"), None);
    }
}
