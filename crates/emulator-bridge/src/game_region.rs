//! Which region a running game is in, said in plain words.
//!
//! The owner read 49.8 frames a second off Apple's own overlay while playing an NES game
//! (12 October 2026) and asked whether something was broken. Nothing was: a European NES runs at
//! 50 frames a second, not 60, and 49.8 is that. But nothing in the app said so, which is what
//! made a correct number look like a fault. This module turns the rate the running core reports
//! into one plain sentence for the (i) read-out and the activity log.
//!
//! ## Where an NES game's region actually comes from
//!
//! Read off both NES cores at the commits `scripts/build-core.sh` pins. The app never guesses it:
//! the core decides, and then reports the rate it decided on.
//!
//! - **Mesen 2**, the default NES core. Its option is `mesen_region` and its default is `Auto`. On
//!   `Auto` the cartridge's own `System` field decides (`NesConsole::UpdateRegion`): `NesPal` is
//!   PAL, `Dendy` is Dendy, anything else is NTSC. That field is set from Mesen's built-in
//!   cartridge database, keyed by the ROM's CRC32 (`Core/NES/GameDatabase.cpp`, with the table
//!   compiled into the core as `Libretro/MesenDB.inc`), else from an NES 2.0 header's region bits,
//!   else from the iNES 1.0 PAL flag (header byte 9, bit 0). `NesConsole::GetFps` then answers
//!   60.0988 for NTSC and 50.0070 for PAL and Dendy, and that is what the core hands back through
//!   `retro_get_system_av_info`.
//! - **FCEUmm**, the other NES choice. Its option is `fceumm_region`, also defaulting to `Auto`. On
//!   `Auto` an NES 2.0 header's region bits decide; for an iNES 1.0 file, which nearly every dump
//!   is, it instead looks for a region tag in the FILE NAME — `(E)`, `(Europe)`, `(PAL)`, `(F)`,
//!   `(G)`, `(I)`, `(S)`, `(Australia)` and the rest (`src/ines.c`) — and its own correction table,
//!   keyed by CRC, can override either. The app hands the core the game's real path, so the tag is
//!   there to be found (`cores::native_core::load_content`). One consequence worth knowing: an iNES
//!   1.0 European dump that has been renamed without its `(E)` runs as NTSC on FCEUmm and still
//!   runs as PAL on Mesen, because Mesen recognises the ROM itself.
//!
//! Either way the rate the engine paces to is the core's own, read straight after the content is
//! loaded (`cores::native_core::sync_descriptor_from_core`) and followed again if the core changes
//! it mid-session, so all this module does is put that number into words.

/// How far from 50 or 60 a rate may be and still count as that rate. The real figures are 50.0070
/// and 60.0988 on both NES cores; the margin only has to cover a core's own rounding.
const MARGIN: f64 = 1.5;

/// The file types this says anything about: the NES family, where the same game shipped at two
/// speeds and the core switches between them. Everything else gets nothing, because a line about
/// a region would be meaningless for a handheld and a guess for a system whose core's region
/// handling has not been read. A system joins this list when it has been.
fn two_speed(content_ext: &str) -> bool {
    matches!(content_ext, "nes" | "unf" | "unif" | "fds")
}

/// One plain sentence saying what speed the running game is at and which region that is right for,
/// or "" when there is nothing worth saying: another system, or a rate that is neither 50 nor 60.
///
/// `content_ext` is the running game's file extension and `fps` the rate the core reported.
pub fn note(content_ext: &str, fps: f64) -> String {
    let ext = content_ext.to_ascii_lowercase();
    if !two_speed(&ext) || !fps.is_finite() {
        return String::new();
    }
    // What it is RUNNING at, rather than what the file is, because that is the honest half: a
    // Russian Dendy cartridge also runs at 50, and a game whose region was picked by hand runs at
    // whatever was picked.
    if (fps - 50.0).abs() <= MARGIN {
        "It's running at 50 frames a second instead of 60, which is right for a European copy. \
         Nothing is wrong."
            .to_string()
    } else if (fps - 60.0).abs() <= MARGIN {
        "It's running at 60 frames a second, which is right for an American or Japanese copy."
            .to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_european_nes_game_is_named_as_one() {
        // 50.0070: NesConsole::GetFps, and NES_PAL_FPS in FCEUmm.
        let line = note("nes", 50.006_978_908_188_6);
        assert!(line.contains("50 frames a second"), "{line}");
        assert!(line.contains("European"), "{line}");
        // The owner's own reading off Apple's overlay has to land in the same sentence.
        assert_eq!(note("nes", 49.8), line);
    }

    #[test]
    fn an_american_nes_game_is_named_as_one() {
        // 60.0988: the same two sources.
        let line = note("nes", 60.098_811_862_348_4);
        assert!(line.contains("60 frames a second"), "{line}");
        assert!(line.contains("American or Japanese"), "{line}");
    }

    #[test]
    fn the_whole_nes_family_is_covered_and_nothing_else_is() {
        for ext in ["nes", "NES", "unf", "unif", "fds"] {
            assert!(!note(ext, 50.007).is_empty(), "{ext}");
        }
        // A handheld has one speed, so 59.73 is not a region and must not be read as one.
        assert!(note("gb", 59.727_5).is_empty());
        assert!(note("gba", 59.727_5).is_empty());
        // Systems whose cores have not been read say nothing rather than guessing.
        assert!(note("md", 49.70).is_empty());
        assert!(note("sfc", 50.0).is_empty());
    }

    #[test]
    fn a_rate_that_is_neither_says_nothing() {
        assert!(note("nes", 0.0).is_empty());
        assert!(note("nes", 55.0).is_empty());
        assert!(note("nes", 120.0).is_empty());
        assert!(note("nes", f64::NAN).is_empty());
        assert!(note("nes", f64::INFINITY).is_empty());
        assert!(note("", 60.0988).is_empty());
    }
}
