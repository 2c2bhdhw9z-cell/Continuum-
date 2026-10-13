//! Which extra chip file a SNES cartridge needs before it can start.
//!
//! Some Super Nintendo cartridges have a second processor inside them, and an emulator can only
//! run those games with a copy of that chip's own program: bsnes looks for `dsp1b.rom`,
//! `dsp1.rom`, `dsp2.rom`, `dsp3.rom`, `dsp4.rom`, `cx4.rom`, `st010.rom`, `st011.rom` or
//! `st018.rom` in the system folder, and refuses the game when the one it wants is missing. Before
//! this, the player got a game that would not start and no idea why, so the launch names the file.
//!
//! What is read: the cartridge's own header, which says whether there is a chip and which family
//! it belongs to.
//!
//! - The header sits at `0x7FB0` (LoROM) or `0xFFB0` (HiROM), after a 512-byte copier header when
//!   the file has one. Which of the two is decided by the checksum pair the header carries, the
//!   same way every SNES emulator decides it.
//! - `0x__D6` is the cartridge type: the low nibble says a coprocessor is there (3 and up), the
//!   high nibble says which family. `0xF` means "custom", and then `0x__BF` names it.
//! - Only the DSP family and the Seta chips need a file. SuperFX, SA-1, S-DD1, OBC1, S-RTC and
//!   SPC7110 are emulated outright, so a game with one of those is not gated.
//! - The header cannot tell DSP-1 from DSP-2, DSP-3 or DSP-4, and it cannot tell ST010 from
//!   ST011, because the hardware answers the same either way. The four games that are not DSP-1
//!   are named by their own header titles, which is what every emulator's cartridge database does
//!   for them. Everything else in the family is DSP-1, and both of its file names are offered:
//!   the chip was revised mid-life and a copy of either one is what people have.

/// The chip file(s) a cartridge needs, best name first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChipFile {
    /// The names the core looks for. More than one means either will do.
    pub files: &'static [&'static str],
}

const DSP1: ChipFile = ChipFile { files: &["dsp1b.rom", "dsp1.rom"] };
const DSP2: ChipFile = ChipFile { files: &["dsp2.rom"] };
const DSP3: ChipFile = ChipFile { files: &["dsp3.rom"] };
const DSP4: ChipFile = ChipFile { files: &["dsp4.rom"] };
const ST010: ChipFile = ChipFile { files: &["st010.rom"] };
const ST011: ChipFile = ChipFile { files: &["st011.rom"] };
const ST018: ChipFile = ChipFile { files: &["st018.rom"] };
const CX4: ChipFile = ChipFile { files: &["cx4.rom"] };

/// The header's 21-byte title, upper-cased, spaces squeezed out, for the title matches below.
fn squeezed_title(rom: &[u8], header: usize) -> String {
    rom.get(header..header + 21)
        .unwrap_or_default()
        .iter()
        .filter(|b| b.is_ascii_alphanumeric())
        .map(|b| b.to_ascii_uppercase() as char)
        .collect()
}

/// Whether the header at `base` (the title's first byte) looks like a real one: its checksum and
/// the complement beside it must add up to `0xFFFF`, which is the one self-check the header has.
fn header_is_sane(rom: &[u8], base: usize) -> bool {
    let read = |at: usize| -> Option<u16> {
        let low = *rom.get(at)? as u16;
        let high = *rom.get(at + 1)? as u16;
        Some(low | high << 8)
    };
    let Some(complement) = read(base + 0x1C) else { return false };
    let Some(checksum) = read(base + 0x1E) else { return false };
    checksum != 0 && complement != 0 && checksum.wrapping_add(complement) == 0xFFFF
}

/// Where the cartridge header's title starts, or `None` when neither layout holds one.
fn header_offset(rom: &[u8]) -> Option<usize> {
    // A copier's 512-byte header, the thing that makes a .smc longer than its ROM.
    let skip = if rom.len() % 0x8000 == 512 { 512 } else { 0 };
    [0x7FC0usize, 0xFFC0]
        .into_iter()
        .map(|base| base + skip)
        .find(|base| header_is_sane(rom, *base))
}

impl ChipFile {
    /// The player's line when none of the files is in the system folder. Says the name to look
    /// for, where to put it, and the one button that installs it, like the other launch lines.
    pub fn missing_line(&self, game_name: &str) -> String {
        let named = match self.files {
            [one] => one.to_string(),
            [first, rest @ ..] => {
                let others: Vec<&str> = rest.to_vec();
                format!("{first} (or {})", others.join(" or "))
            }
            [] => return String::new(),
        };
        format!(
            "{game_name} needs an extra file called {named}: there is a chip inside that \
             cartridge. Put it in the Continuum folder in Files, then Settings, Install a BIOS \
             from the Continuum folder."
        )
    }
}

/// The chip file this ROM needs, or `None` when it needs none (which is almost every game).
pub fn chip_file(rom: &[u8]) -> Option<ChipFile> {
    let header = header_offset(rom)?;
    let cartridge_type = *rom.get(header + 0x16)?;
    if cartridge_type & 0x0F < 3 {
        return None;
    }
    let title = squeezed_title(rom, header);
    match cartridge_type >> 4 {
        // `contains`, not `starts_with`: a header title is cut off at 21 characters, so
        // "The Planet's Champ TG3000" never reaches its number.
        0x0 => Some(match title.as_str() {
            t if t.contains("DUNGEONMASTER") => DSP2,
            t if t.contains("SDGUNDAMGX") => DSP3,
            t if t.contains("TOPGEAR3000") || t.contains("PLANETSCHAMP") => DSP4,
            _ => DSP1,
        }),
        0xF => match *rom.get(header - 1)? {
            0x01 if title.contains("MORIT") => Some(ST011),
            0x01 => Some(ST010),
            0x02 => Some(ST018),
            0x10 => Some(CX4),
            // SPC7110 (0x00) is emulated outright.
            _ => None,
        },
        // SuperFX, OBC1, SA-1, S-DD1, S-RTC, and the Super Game Boy / Satellaview "other"
        // group: all emulated outright, so none of them gates a launch.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A LoROM file with a valid header: `title`, cartridge type `kind`, custom subtype `sub`.
    fn rom(title: &str, kind: u8, sub: u8) -> Vec<u8> {
        let mut rom = vec![0u8; 0x80000];
        let header = 0x7FC0;
        let bytes = title.as_bytes();
        for i in 0..21 {
            rom[header + i] = if i < bytes.len() { bytes[i] } else { b' ' };
        }
        rom[header - 1] = sub;
        rom[header + 0x16] = kind;
        // The checksum pair, the only thing header_is_sane insists on.
        let checksum: u16 = 0x1234;
        let complement = !checksum;
        rom[header + 0x1C] = complement as u8;
        rom[header + 0x1D] = (complement >> 8) as u8;
        rom[header + 0x1E] = checksum as u8;
        rom[header + 0x1F] = (checksum >> 8) as u8;
        rom
    }

    #[test]
    fn a_plain_cartridge_needs_nothing() {
        assert_eq!(chip_file(&rom("SUPER MARIO WORLD", 0x00, 0)), None);
        assert_eq!(chip_file(&rom("ZELDA", 0x02, 0)), None);
        // SuperFX and SA-1 are emulated outright.
        assert_eq!(chip_file(&rom("STAR FOX", 0x15, 0)), None);
        assert_eq!(chip_file(&rom("SUPER MARIO RPG", 0x35, 0)), None);
    }

    #[test]
    fn the_dsp_games_name_their_file() {
        assert_eq!(chip_file(&rom("SUPER MARIOKART", 0x03, 0)), Some(DSP1));
        assert_eq!(chip_file(&rom("PILOTWINGS", 0x03, 0)), Some(DSP1));
        assert_eq!(chip_file(&rom("DUNGEON MASTER", 0x05, 0)), Some(DSP2));
        assert_eq!(chip_file(&rom("SD GUNDAM GX", 0x05, 0)), Some(DSP3));
        assert_eq!(chip_file(&rom("TOP GEAR 3000", 0x03, 0)), Some(DSP4));
        assert_eq!(chip_file(&rom("THE PLANETS CHAMP TG3000", 0x03, 0)), Some(DSP4));
    }

    #[test]
    fn the_seta_and_capcom_chips_name_theirs() {
        assert_eq!(chip_file(&rom("F1 ROC II", 0xF3, 0x01)), Some(ST010));
        // A title is cut off at 21 characters, which is where this one ends.
        assert_eq!(chip_file(&rom("HAYAZASI NIDAN MORITA", 0xF5, 0x01)), Some(ST011));
        assert_eq!(chip_file(&rom("HAYAZASI NIDAN MORITA", 0xF5, 0x02)), Some(ST018));
        assert_eq!(chip_file(&rom("MEGAMAN X2", 0xF3, 0x10)), Some(CX4));
        // SPC7110 is emulated outright.
        assert_eq!(chip_file(&rom("FAR EAST OF EDEN", 0xF5, 0x00)), None);
    }

    #[test]
    fn a_copier_header_and_hirom_are_both_found() {
        let mut with_copier = vec![0u8; 512];
        with_copier.extend_from_slice(&rom("SUPER MARIOKART", 0x03, 0));
        assert_eq!(with_copier.len() % 0x8000, 512);
        assert_eq!(chip_file(&with_copier), Some(DSP1));

        // HiROM: the same header 0x8000 further in.
        let mut hirom = vec![0u8; 0x100000];
        let header = 0xFFC0;
        hirom[header..header + 14].copy_from_slice(b"DUNGEON MASTER");
        hirom[header + 0x16] = 0x05;
        hirom[header + 0x1C] = 0xCB;
        hirom[header + 0x1D] = 0xED;
        hirom[header + 0x1E] = 0x34;
        hirom[header + 0x1F] = 0x12;
        assert_eq!(chip_file(&hirom), Some(DSP2));
    }

    #[test]
    fn the_line_names_the_file_and_the_two_steps() {
        let line = DSP1.missing_line("Super Mario Kart");
        assert!(line.starts_with("Super Mario Kart needs an extra file called dsp1b.rom (or dsp1.rom)"));
        assert!(line.contains("Continuum folder in Files"));
        assert!(line.contains("Install a BIOS"));
        assert!(CX4.missing_line("Mega Man X2").contains("called cx4.rom:"));
        // Plain words only: no file paths, no hex, no core names.
        for line in [DSP1.missing_line("A"), ST010.missing_line("B"), ST018.missing_line("C")] {
            assert!(!line.contains("0x"), "{line}");
            assert!(!line.contains("bsnes"), "{line}");
        }
    }

    #[test]
    fn rubbish_is_not_guessed_at() {
        assert_eq!(chip_file(&[]), None);
        assert_eq!(chip_file(&[0u8; 0x8000]), None);
        assert_eq!(chip_file(&[0xFFu8; 0x40000]), None);
    }
}
