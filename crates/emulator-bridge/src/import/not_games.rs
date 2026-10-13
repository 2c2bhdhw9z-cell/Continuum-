//! Moving library files that are text, not games, into the Not games folder.
//!
//! The app runs this on every library rescan (so the first launch of a new build clears out what
//! an older one let in), with `Documents/Not games` as the folder: Files shows it as Continuum,
//! Not games. File names are kept, a clash gets " 2", " 3" before the extension, and nothing is
//! ever deleted or overwritten. Every file is checked again here with [`detect::not_a_game`], so
//! a binary file is never moved, whatever the caller thought.

use std::path::{Path, PathBuf};

use super::detect;

/// One file that was moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    /// Where it was, exactly as the caller named it.
    pub from: PathBuf,
    /// Where it is now.
    pub to: PathBuf,
    /// Why it is not a game, in one plain sentence.
    pub reason: String,
}

/// Whether something (a file, a folder, even a broken link) already has this name.
fn taken(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// A name in `folder` nothing has yet: `name` itself when it is free, otherwise `stem 2.ext`,
/// `stem 3.ext` and so on (`name 2` when there is no extension).
pub fn free_name(folder: &Path, name: &str) -> PathBuf {
    let first = folder.join(name);
    if !taken(&first) {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
        _ => (name, None),
    };
    let mut n: u32 = 2;
    loop {
        let candidate = match ext {
            Some(ext) => folder.join(format!("{stem} {n}.{ext}")),
            None => folder.join(format!("{stem} {n}")),
        };
        if !taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Moves each of `paths` that is not a game into `folder`, and returns what moved.
///
/// A file that is a game (or can't be read) stays where it is. A file that can't be moved stays
/// too, and the next rescan tries again. When the folder can't be made, nothing more is moved.
pub fn move_not_games(paths: &[PathBuf], folder: &Path) -> Vec<Moved> {
    let mut moved = Vec::new();
    for path in paths {
        let Some(reason) = detect::not_a_game(path) else { continue };
        if std::fs::create_dir_all(folder).is_err() {
            return moved;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        let to = free_name(folder, name);
        if std::fs::rename(path, &to).is_ok() {
            moved.push(Moved { from: path.clone(), to, reason });
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::fixtures;
    use crate::import::testdir::TestDir;

    fn markdown(title: &str) -> Vec<u8> {
        format!("# {title}\n\nNotes that are not a game.\n\n```sh\ncargo test\n```\n").repeat(30).into_bytes()
    }

    #[test]
    fn binary_roms_stay_and_text_files_move() {
        let dir = TestDir::new("notgames");
        let mut ines = b"NES\x1A\x02\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
        ines.extend(fixtures::rom_bytes(0x8000, 21));
        let mut gb = fixtures::rom_bytes(0x8000, 22);
        gb[0x104..0x108].copy_from_slice(&[0xCE, 0xED, 0x66, 0x66]);
        let mut gba = fixtures::rom_bytes(0x8000, 23);
        gba[0x04..0x08].copy_from_slice(&[0x24, 0xFF, 0xAE, 0x51]);
        let games: Vec<(&str, Vec<u8>)> = vec![
            ("Sonic (USA).md", fixtures::mega_drive_rom()),
            ("Streets.smd", fixtures::smd_dump(&fixtures::mega_drive_rom())),
            ("Odd Header (Proto).gen", fixtures::rom_bytes(0x8000, 24)),
            ("No Zeros.sfc", fixtures::nul_free_rom(0x8000)),
            ("Mario.nes", ines),
            ("Pong.a26", vec![0xEA; 4096]),
            ("Tetris.gb", gb),
            ("Zelda.gba", gba),
        ];
        let text: Vec<(&str, Vec<u8>)> = vec![
            ("README_ru.md", fixtures::readme_ru()),
            ("README_zh-TW.md", fixtures::readme_zh()),
            ("LICENSE.md", fixtures::license_text()),
            ("mcp-contracts.md", markdown("MCP contracts")),
            ("jeb-analysis.md", markdown("JEB analysis")),
            ("notes.gba", markdown("notes")),
            ("otool.bin", b"otool -L Continuum\n\tlibSystem.B.dylib\n".repeat(20)),
        ];
        let mut paths = Vec::new();
        for (name, bytes) in games.iter().chain(text.iter()) {
            paths.push(dir.write(&format!("Documents/{name}"), bytes));
        }
        // Already there from an earlier move, to force a clash.
        let earlier = b"an earlier LICENSE.md".to_vec();
        dir.write("Documents/Not games/LICENSE.md", &earlier);
        let documents = dir.path().join("Documents");
        let folder = documents.join("Not games");

        let moved = move_not_games(&paths, &folder);

        for (name, bytes) in &games {
            assert_eq!(&std::fs::read(documents.join(name)).unwrap(), bytes, "{name} must stay as it was");
            assert!(!folder.join(name).exists(), "{name} must not be in Not games");
        }
        for (name, bytes) in &text {
            assert!(!documents.join(name).exists(), "{name} must be gone from Documents");
            let landed = if *name == "LICENSE.md" { "LICENSE 2.md".to_string() } else { name.to_string() };
            assert_eq!(&std::fs::read(folder.join(&landed)).unwrap(), bytes, "{name} must be in Not games as {landed}");
        }
        assert_eq!(std::fs::read(folder.join("LICENSE.md")).unwrap(), earlier, "the earlier file is untouched");
        let mut from: Vec<String> =
            moved.iter().map(|m| m.from.file_name().unwrap().to_string_lossy().into_owned()).collect();
        from.sort();
        let mut want: Vec<String> = text.iter().map(|(n, _)| n.to_string()).collect();
        want.sort();
        assert_eq!(from, want, "exactly the text files moved");
        assert!(moved.iter().all(|m| m.reason.contains("text")));
        let license = moved.iter().find(|m| m.from.ends_with("LICENSE.md")).unwrap();
        assert_eq!(license.to, folder.join("LICENSE 2.md"));

        assert!(move_not_games(&paths, &folder).is_empty(), "a second call moves nothing");
    }

    #[test]
    fn legit_text_formats_never_move() {
        let dir = TestDir::new("notgames-legit");
        let folder = dir.path().join("Not games");
        let paths: Vec<PathBuf> = ["cue", "m3u", "gdi", "toc", "ccd", "cmd", "bat", "ini", "cfg", "lua", "js", "conf", "txt"]
            .iter()
            .map(|ext| dir.write(&format!("Game.{ext}"), &fixtures::license_text()))
            .collect();
        assert!(move_not_games(&paths, &folder).is_empty());
        assert!(paths.iter().all(|p| p.exists()));
        assert!(!folder.exists(), "the folder is only made when something moves");
    }

    #[test]
    fn free_name_numbers_a_clash() {
        let dir = TestDir::new("notgames-free");
        assert_eq!(free_name(dir.path(), "README.md"), dir.path().join("README.md"));
        dir.write("README.md", b"1");
        assert_eq!(free_name(dir.path(), "README.md"), dir.path().join("README 2.md"));
        dir.write("README 2.md", b"2");
        assert_eq!(free_name(dir.path(), "README.md"), dir.path().join("README 3.md"));
        dir.write("NOTES", b"1");
        assert_eq!(free_name(dir.path(), "NOTES"), dir.path().join("NOTES 2"));
        dir.write(".md", b"1");
        assert_eq!(free_name(dir.path(), ".md"), dir.path().join(".md 2"));
    }

    #[test]
    fn nothing_moves_when_the_folder_cannot_be_made() {
        let dir = TestDir::new("notgames-nofolder");
        let text = dir.write("README.md", &fixtures::license_text());
        let folder = dir.write("Not games", b"a file where the folder should be");
        assert!(move_not_games(std::slice::from_ref(&text), &folder).is_empty());
        assert_eq!(std::fs::read(&text).unwrap(), fixtures::license_text());
    }
}
