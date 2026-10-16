//! Which typed codes each emulator reads, in plain words for the cheat screen.
//!
//! A typed code goes to the core through `retro_cheat_set` and the core decides what it means, so
//! the app never checks a code's shape (see `CheatStore.swift`). What it CAN say is what each core
//! does with one, and that was read from each core's own `retro_cheat_set`:
//!
//! - the cores built from source, at the commits `scripts/build-core.sh` pins (5 October 2026;
//!   bsnes, Gearsystem, BlastEm and the patched Mesen 2 on 12 October 2026);
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

/// The SNES codes bsnes reads, and Mesen 2 on an SNES game (the Continuum patch,
/// scripts/patches/mesen2-snes-memory-cheats-saves.patch, takes the same spellings).
const SNES_CODES: CodeSupport = reads("Game Genie, Pro Action Replay, or a raw address:value like 7E0019:02");

/// The answer for one core id. Unknown cores read codes, kinds not checked: the safe default, since
/// saying "this emulator ignores codes" about one that does not would be the worse mistake.
pub fn code_support(core_id: &str) -> CodeSupport {
    match core_id {
        "fceumm" | "mesen2" => {
            reads("Game Genie, Pro Action Rocky, or a raw address:value like 0075:09")
        }
        "bsnes" => SNES_CODES,
        "mgba" => reads(
            "GameShark, Action Replay or CodeBreaker on the GBA; GameShark or Game Genie on the \
             Game Boy",
        ),
        "gearsystem" => reads("Game Genie, Pro Action Replay, or a raw address:value like C000:FF"),
        // The Continuum patch (scripts/patches/blastem-continuum-memory.patch) reads these;
        // BlastEm's own retro_cheat_set does nothing.
        "blastem" => {
            reads("Game Genie, Pro Action Replay, or a raw address:value like FFC000:FF")
        }
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
        | "vice_x64sc" | "handy" | "a5200" | "pokemini" | "yabause" => IGNORES,
        _ => reads(""),
    }
}

/// The answer for one core playing one system (the engine's system id, "snes"). Mesen 2 plays the
/// NES and the SNES and reads each one's own codes; every other core reads the same kinds
/// whatever it plays, so this is [`code_support`] for them.
pub fn code_support_on(core_id: &str, system: &str) -> CodeSupport {
    match (core_id, system) {
        ("mesen2", "snes") => SNES_CODES,
        _ => code_support(core_id),
    }
}

/// A typed code as `core_id` reads it, split into the entries handed to `retro_cheat_set`.
///
/// - Gearsystem takes one code per entry and reads Pro Action Replay as `00AAAAVV`, so a
///   `+`-joined code becomes one entry per part and a raw `AAAA:VV` its Pro Action Replay form.
/// - bsnes reads Pro Action Replay `AAAAAADD` and Game Genie, not `AAAAAA:DD`, so a raw part is
///   rewritten and the `+` join kept.
/// - BlastEm reads one code per entry, so a `+`-joined code becomes one entry per part. The
///   shapes themselves are already what the Continuum patch reads.
/// - Every other core gets the code exactly as typed.
pub fn normalise_for_core(core_id: &str, code: &str) -> Vec<String> {
    let is_hex = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit());
    match core_id {
        "blastem" => code
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect(),
        "gearsystem" => code
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(|part| match part.split_once(':') {
                Some((a, v)) if a.len() == 4 && v.len() == 2 && is_hex(a) && is_hex(v) => {
                    format!("00{}{}", a.to_ascii_uppercase(), v.to_ascii_uppercase())
                }
                _ => part.to_owned(),
            })
            .collect(),
        "bsnes" => vec![code
            .split('+')
            .map(str::trim)
            .map(|part| {
                let split = part.split_once(':').or_else(|| part.split_once('='));
                match split {
                    Some((a, d)) if a.len() == 6 && d.len() == 2 && is_hex(a) && is_hex(d) => {
                        format!("{}{}", a.to_ascii_uppercase(), d.to_ascii_uppercase())
                    }
                    _ => part.to_owned(),
                }
            })
            .collect::<Vec<_>>()
            .join("+")],
        _ => vec![code.to_owned()],
    }
}

/// Which exact cartridge a Game Boy Advance or Game Boy file is, read from its own header.
///
/// Cheat codes are written for one exact game, region AND version: Pokemon FireRed's walk-through-
/// walls code for version 1.1 freezes version 1.0 the moment the player moves, and the same code on
/// Emerald does the same. The header says which one a file is, so the cheat screen can say it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CartridgeIdentity {
    /// A name for the commonest games, else the header's own title.
    pub title: String,
    /// The four-letter game code (GBA), empty on the Game Boy.
    pub code: String,
    /// "USA", "Europe", "Japan" and so on, from the code's last letter. Empty when unknown.
    pub region: String,
    /// "1.0", "1.1" ... from the header's version byte.
    pub version: String,
}

/// Names for the games people most often look up codes for. Anything else shows the header title.
fn known_game(code: &str) -> Option<&'static str> {
    match code.get(..3)? {
        "BPR" => Some("Pokemon FireRed"),
        "BPG" => Some("Pokemon LeafGreen"),
        "BPE" => Some("Pokemon Emerald"),
        "AXV" => Some("Pokemon Ruby"),
        "AXP" => Some("Pokemon Sapphire"),
        _ => None,
    }
}

fn region_letter(letter: char) -> &'static str {
    match letter {
        'E' => "USA",
        'P' => "Europe",
        'J' => "Japan",
        'D' => "Germany",
        'F' => "France",
        'I' => "Italy",
        'S' => "Spain",
        'K' => "Korea",
        'U' => "Australia",
        _ => "",
    }
}

fn header_text(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| if b.is_ascii_graphic() || *b == b' ' { *b as char } else { '?' })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Reads the header at the start of a `.gba`, `.gb` or `.gbc` file. `None` when the bytes are not
/// one: the header's own check byte or checksum must agree, so a random file is never named.
pub fn identify_cartridge(bytes: &[u8]) -> Option<CartridgeIdentity> {
    // GBA: the fixed value 0x96 at 0xB2, and the complement check at 0xBD over 0xA0..=0xBC.
    if bytes.len() >= 0xC0 && bytes[0xB2] == 0x96 {
        let sum = bytes[0xA0..=0xBC]
            .iter()
            .fold(0u8, |acc, b| acc.wrapping_sub(*b))
            .wrapping_sub(0x19);
        if sum == bytes[0xBD] {
            let code = header_text(&bytes[0xAC..0xB0]);
            let region = code.chars().nth(3).map(region_letter).unwrap_or("").to_owned();
            let title = known_game(&code)
                .map(str::to_owned)
                .unwrap_or_else(|| header_text(&bytes[0xA0..0xAC]));
            return Some(CartridgeIdentity {
                title,
                code,
                region,
                version: format!("1.{}", bytes[0xBC]),
            });
        }
    }
    // Game Boy and Game Boy Color: the header checksum at 0x14D over 0x134..=0x14C.
    if bytes.len() >= 0x150 {
        let sum = bytes[0x134..=0x14C]
            .iter()
            .fold(0u8, |acc, b| acc.wrapping_sub(*b).wrapping_sub(1));
        if sum == bytes[0x14D] {
            // The title is 16 bytes on the original, 15 or 11 once a colour flag is there.
            let end = if bytes[0x143] & 0x80 != 0 { 0x143 } else { 0x144 };
            let title = header_text(&bytes[0x134..end]);
            if !title.is_empty() {
                return Some(CartridgeIdentity {
                    title,
                    code: String::new(),
                    region: if bytes[0x14A] == 0 { "Japan".into() } else { String::new() },
                    version: format!("1.{}", bytes[0x14C]),
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GBA header with a correct check byte.
    fn gba_header(title: &[u8], code: &[u8; 4], version: u8) -> Vec<u8> {
        let mut rom = vec![0u8; 0xC0];
        rom[0xA0..0xA0 + title.len()].copy_from_slice(title);
        rom[0xAC..0xB0].copy_from_slice(code);
        rom[0xB2] = 0x96;
        rom[0xBC] = version;
        let sum = rom[0xA0..=0xBC]
            .iter()
            .fold(0u8, |acc, b| acc.wrapping_sub(*b))
            .wrapping_sub(0x19);
        rom[0xBD] = sum;
        rom
    }

    #[test]
    fn a_gba_header_names_the_exact_game_and_version() {
        let fire_red = identify_cartridge(&gba_header(b"POKEMON FIRE", b"BPRE", 1)).unwrap();
        assert_eq!(fire_red.title, "Pokemon FireRed");
        assert_eq!(fire_red.code, "BPRE");
        assert_eq!(fire_red.region, "USA");
        assert_eq!(fire_red.version, "1.1");
        let other = identify_cartridge(&gba_header(b"SOME GAME", b"AXYP", 0)).unwrap();
        assert_eq!(other.title, "SOME GAME");
        assert_eq!(other.region, "Europe");
        assert_eq!(other.version, "1.0");
        // A wrong check byte is not a header.
        let mut broken = gba_header(b"POKEMON FIRE", b"BPRE", 1);
        broken[0xBD] ^= 1;
        assert_eq!(identify_cartridge(&broken), None);
        assert_eq!(identify_cartridge(&[0u8; 0x100]), None);
    }

    #[test]
    fn a_game_boy_header_is_read_too() {
        let mut rom = vec![0u8; 0x150];
        rom[0x134..0x134 + 12].copy_from_slice(b"POKEMON YELL");
        rom[0x143] = 0x80;
        rom[0x14A] = 1;
        rom[0x14C] = 0;
        rom[0x14D] = rom[0x134..=0x14C]
            .iter()
            .fold(0u8, |acc, b| acc.wrapping_sub(*b).wrapping_sub(1));
        let id = identify_cartridge(&rom).unwrap();
        assert_eq!(id.title, "POKEMON YELL");
        assert_eq!(id.version, "1.0");
        assert_eq!(id.region, "");
    }

    #[test]
    fn the_cores_that_throw_codes_away_say_so() {
        for core in ["azahar", "flycast", "stella2023", "pokemini", "yabause"] {
            assert!(!code_support(core).reads_typed_codes, "{core}");
        }
    }

    #[test]
    fn the_main_cores_name_their_code_types() {
        assert!(code_support("mgba").kinds.contains("GameShark"));
        assert!(code_support("mgba").kinds.contains("CodeBreaker"));
        assert!(code_support("fceumm").kinds.contains("Game Genie"));
        assert!(code_support("mesen2").kinds.contains("Game Genie"));
        assert!(code_support("bsnes").kinds.contains("Pro Action Replay"));
        assert!(code_support("gearsystem").kinds.contains("C000:FF"));
        assert!(code_support("pcsx_rearmed").kinds.contains("GameShark"));
        assert!(code_support("melonds").kinds.contains("Action Replay"));
        assert!(code_support("ppsspp").kinds.contains("CWCheat"));
    }

    #[test]
    fn mesen_names_the_snes_codes_on_an_snes_game_and_the_nes_ones_on_an_nes_game() {
        assert_eq!(code_support_on("mesen2", "snes"), code_support("bsnes"));
        assert!(code_support_on("mesen2", "snes").kinds.contains("7E0019:02"));
        assert_eq!(code_support_on("mesen2", "nes"), code_support("mesen2"));
        assert_eq!(code_support_on("mesen2", "fds"), code_support("mesen2"));
        assert_eq!(code_support_on("mesen2", ""), code_support("mesen2"));
        assert_eq!(code_support_on("bsnes", "snes"), code_support("bsnes"));
        assert_eq!(code_support_on("mgba", "gb"), code_support("mgba"));
    }

    #[test]
    fn raw_codes_are_rewritten_the_way_each_core_reads_them() {
        assert_eq!(normalise_for_core("gearsystem", "c000:ff"), vec!["00C000FF"]);
        assert_eq!(normalise_for_core("gearsystem", "C000:FF+C001:01"), vec!["00C000FF", "00C00101"]);
        // Game Genie and Pro Action Replay pass through untouched.
        assert_eq!(normalise_for_core("gearsystem", "3A6-5BF+00C0A0FF"), vec!["3A6-5BF", "00C0A0FF"]);
        assert_eq!(normalise_for_core("bsnes", "7e0019:02"), vec!["7E001902"]);
        assert_eq!(normalise_for_core("bsnes", "7E0019=02+7E001A:03"), vec!["7E001902+7E001A03"]);
        assert_eq!(normalise_for_core("bsnes", "DD32-6DAD"), vec!["DD32-6DAD"]);
        assert_eq!(normalise_for_core("fceumm", "0075:09"), vec!["0075:09"]);
        assert_eq!(normalise_for_core("mesen2", "SXIOPO+0075:09"), vec!["SXIOPO+0075:09"]);
        // BlastEm takes one code per entry, unchanged.
        assert_eq!(normalise_for_core("blastem", "SCRA-BAX0"), vec!["SCRA-BAX0"]);
        assert_eq!(
            normalise_for_core("blastem", "SCRA-BAX0 + FFC000:FF"),
            vec!["SCRA-BAX0", "FFC000:FF"]
        );
    }

    #[test]
    fn the_mega_drive_cores_name_their_code_types() {
        let blastem = code_support("blastem");
        assert!(blastem.reads_typed_codes);
        assert!(blastem.kinds.contains("Game Genie"));
        assert!(blastem.kinds.contains("Pro Action Replay"));
    }

    #[test]
    fn an_unknown_core_is_assumed_to_read_codes() {
        let unknown = code_support("some_future_core");
        assert!(unknown.reads_typed_codes);
        assert!(unknown.kinds.is_empty());
    }
}
