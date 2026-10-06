//! Which typed codes each emulator reads, in plain words for the cheat screen.
//!
//! A typed code goes to the core through `retro_cheat_set` and the core decides what it means, so
//! the app never checks a code's shape (see `CheatStore.swift`). What it CAN say is what each core
//! does with one, and that was read from each core's own `retro_cheat_set`:
//!
//! - the 13 cores built from source, at the commits `scripts/build-core.sh` pins (5 October 2026);
//! - the downloaded cores, from their upstream sources on the same day. A core whose source could
//!   not be found is left as "not checked" rather than guessed.
//!
//! Several cores have an EMPTY `retro_cheat_set`: they take the code and do nothing with it. That
//! includes the 3DS (Azahar), Dreamcast (Flycast) and the Atari 2600 (Stella). For those the RAM
//! search still works where the core shares its memory, because a search result is a poke the
//! engine writes itself (see `cheats::poke`), not a code the core has to understand.

/// What one emulator does with a typed code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeSupport {
    /// False when the core throws typed codes away. They are still stored, and do nothing.
    pub reads_typed_codes: bool,
    /// The kinds of code it reads, for a sentence ("Game Genie, Pro Action Replay ..."). Empty when
    /// the core reads codes but which kinds was not checked.
    pub kinds: &'static str,
}

const IGNORES: CodeSupport = CodeSupport {
    reads_typed_codes: false,
    kinds: "",
};

const fn reads(kinds: &'static str) -> CodeSupport {
    CodeSupport {
        reads_typed_codes: true,
        kinds,
    }
}

/// The answer for one core id. Unknown cores read codes, kinds not checked: the safe default, since
/// saying "this emulator ignores codes" about one that does not would be the worse mistake.
pub fn code_support(core_id: &str) -> CodeSupport {
    match core_id {
        "fceumm" => reads("Game Genie, Pro Action Rocky, or a raw address:value like 0075:09"),
        "snes9x" => reads("Game Genie, Pro Action Replay, or a raw code"),
        "mgba" => reads(
            "GameShark, Action Replay or CodeBreaker on the GBA; GameShark or Game Genie on the \
             Game Boy",
        ),
        "genesis_plus_gx" => reads("Game Genie, Pro Action Replay, or a raw address:value"),
        "picodrive" => reads("Game Genie or Pro Action Replay"),
        "pcsx_rearmed" | "mednafen_psx_hw" => reads("PlayStation GameShark codes"),
        "melonds" => reads("Action Replay DS"),
        "parallel_n64" => reads("N64 GameShark codes"),
        "ppsspp" => reads("CWCheat (the lines that start _L)"),
        "mednafen_pce_fast" | "mednafen_supergrafx" => {
            reads("a raw address:value like 1F0000:FF or F80000:FF")
        }
        "prboom" => reads("DOOM's own cheat words, like iddqd or idkfa"),
        "virtualjaguar" => reads(""),
        "azahar" | "flycast" | "stella2023" | "mednafen_pce" | "mednafen_vb" | "puae"
        | "vice_x64sc" | "handy" | "a5200" | "fbneo" | "mame2003_plus" | "pokemini"
        | "yabause" => IGNORES,
        _ => reads(""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cores_that_throw_codes_away_say_so() {
        for core in ["azahar", "flycast", "stella2023", "fbneo", "pokemini", "yabause"] {
            assert!(!code_support(core).reads_typed_codes, "{core}");
        }
    }

    #[test]
    fn the_main_cores_name_their_code_types() {
        assert!(code_support("mgba").kinds.contains("GameShark"));
        assert!(code_support("mgba").kinds.contains("CodeBreaker"));
        assert!(code_support("fceumm").kinds.contains("Game Genie"));
        assert!(code_support("snes9x").kinds.contains("Pro Action Replay"));
        assert!(code_support("pcsx_rearmed").kinds.contains("GameShark"));
        assert!(code_support("melonds").kinds.contains("Action Replay"));
        assert!(code_support("ppsspp").kinds.contains("CWCheat"));
    }

    #[test]
    fn an_unknown_core_is_assumed_to_read_codes() {
        let unknown = code_support("some_future_core");
        assert!(unknown.reads_typed_codes);
        assert!(unknown.kinds.is_empty());
    }
}
