//! Why a game would not start, in words a player can do something about.
//!
//! A core that refuses a game hands back its own sentence, and some of those sentences are written
//! for whoever wrote the core: "retro_load_game rejected the content", "the Vulkan device is not
//! ready (MoltenVK did not start)". Testers were reading those, prefixed with the core's id, and
//! could not tell a damaged download from a missing BIOS from a graphics fault on their phone.
//!
//! There are only three things it can be, and each has a different thing to try, so the line says
//! which one it is and nothing else. The core's own words stay in the activity log for support.

/// Which of the three things went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trouble {
    /// The file: not a game this emulator plays, or a damaged copy.
    File,
    /// Something else the game needs that is not on the phone yet.
    ExtraFile,
    /// The phone's graphics did not come up for this emulator.
    Graphics,
}

/// Words in a refusal that mean "something is missing", rather than "this file is wrong".
const MISSING_WORDS: [&str; 6] = ["bios", "firmware", "not found", "missing", "no disc", "chip"];

/// Which trouble a refusal is. `graphics` is true when the engine raised a graphics error rather
/// than a content one, which it knows for certain; everything else is read off the reason the core
/// gave, which is the only thing there is to go on.
pub fn trouble(reason: &str, graphics: bool) -> Trouble {
    if graphics {
        return Trouble::Graphics;
    }
    let lower = reason.to_ascii_lowercase();
    // A graphics failure used to be reported as a refused file (cleanup finding RUST-09), and an
    // older build can still word it that way, so the words are checked too.
    if lower.contains("vulkan") || lower.contains("moltenvk") || lower.contains("graphics") {
        return Trouble::Graphics;
    }
    if MISSING_WORDS.iter().any(|word| lower.contains(word)) {
        return Trouble::ExtraFile;
    }
    Trouble::File
}

/// The line the player reads. `core_name` is the emulator's short name ("mGBA"), `game_name` the
/// file they tapped.
pub fn line(game_name: &str, core_name: &str, trouble: Trouble) -> String {
    match trouble {
        Trouble::File => format!(
            "{game_name} won't start on {core_name}. The file isn't one it can play, or the copy \
             is damaged. Another copy of the game usually fixes it."
        ),
        Trouble::ExtraFile => format!(
            "{game_name} won't start on {core_name}: it needs another file that isn't on the \
             phone yet. Settings, BIOS says which files each system needs."
        ),
        Trouble::Graphics => format!(
            "{game_name} won't start: the graphics didn't come up for {core_name} on this phone. \
             Closing Continuum fully and opening it again usually clears it."
        ),
    }
}

/// Both steps at once, which is what the app calls: the trouble, then its line.
pub fn line_for(game_name: &str, core_name: &str, reason: &str, graphics: bool) -> String {
    line(game_name, core_name, trouble(reason, graphics))
}

/// Cores whose refusals are already the sentence a player should read. cArcade, Continuum's own
/// arcade emulator, says which games it plays, or which zip would fix the copy ("This one needs
/// mspacman.zip next to it."), and its words ("missing a piece", "chip") would otherwise read as
/// a missing BIOS file.
const SPEAKS_FOR_ITSELF: [&str; 1] = ["continuum_arcade"];

/// What the engine says when a core refused a game without saying why (`native_core.rs`).
const UNEXPLAINED: &str = "retro_load_game rejected the content";

/// The player's line for a refused game when `core_id` is one of those cores and gave its own
/// reason (`refusal`); otherwise None, and `line_for` decides.
pub fn own_line(core_id: &str, refusal: &str) -> Option<String> {
    let own = refusal.trim();
    (SPEAKS_FOR_ITSELF.contains(&core_id) && !own.is_empty() && own != UNEXPLAINED).then(|| own.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_file_is_about_the_file() {
        assert_eq!(trouble("retro_load_game rejected the content", false), Trouble::File);
        assert_eq!(trouble("cartridge type $FF is not supported yet", false), Trouble::File);
    }

    #[test]
    fn a_missing_file_says_so() {
        assert_eq!(trouble("bios_CD_U.bin not found", false), Trouble::ExtraFile);
        assert_eq!(trouble("Missing firmware", false), Trouble::ExtraFile);
        assert_eq!(trouble("this game needs a chip file", false), Trouble::ExtraFile);
    }

    #[test]
    fn graphics_is_never_called_a_refused_file() {
        assert_eq!(trouble("anything at all", true), Trouble::Graphics);
        assert_eq!(
            trouble("graphics setup failed: the Vulkan device is not ready (MoltenVK did not start)", false),
            Trouble::Graphics
        );
    }

    #[test]
    fn every_line_is_plain() {
        for trouble in [Trouble::File, Trouble::ExtraFile, Trouble::Graphics] {
            let line = line("Ridge Racer", "Beetle PSX HW", trouble);
            assert!(line.starts_with("Ridge Racer won't start"), "{line}");
            for jargon in ["retro_", "Vulkan", "MoltenVK", "0x", "_libretro", "core_id"] {
                assert!(!line.contains(jargon), "{line} holds {jargon}");
            }
            // Two sentences of advice at most, and it always says what to try.
            assert!(line.len() < 220, "{line}");
        }
    }

    #[test]
    fn the_arcade_emulator_says_what_it_plays_in_its_own_words() {
        let why = "This copy of Pac-Man is missing a piece. A complete copy of pacman.zip fixes it.";
        assert_eq!(own_line("continuum_arcade", why).as_deref(), Some(why));
        // A refusal it gave no reason for, and every other core, get the usual line.
        assert_eq!(own_line("continuum_arcade", "retro_load_game rejected the content"), None);
        assert_eq!(own_line("continuum_arcade", " "), None);
        assert_eq!(own_line("mgba", why), None);
    }

    #[test]
    fn the_app_gets_one_call() {
        let line = line_for("Sonic CD", "BlastEm", "bios_CD_U.bin not found", false);
        assert!(line.contains("needs another file"));
        assert!(line.contains("Settings, BIOS"));
    }
}
