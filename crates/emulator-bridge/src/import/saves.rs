//! Save files in the formats other emulators (and Manic EMU) write, to and from what the cores
//! here actually read.
//!
//! Most systems keep the game's own save as the core's `SAVE_RAM`, which the app stores as
//! `Application Support/BatterySaves/<game>.srm` (see SaveStates.swift). For those this module
//! only converts bytes:
//!
//! | system | in | conversion |
//! | --- | --- | --- |
//! | ds | `.dsv` | DeSmuME's 122-byte footer stripped, so melonDS gets the raw chip image |
//! | ds | `.sav` `.dsg` `.srm` | raw |
//! | ps1 | `.mcr` `.mcd` `.mc` `.srm` | raw 128 KB memory card (pcsx_rearmed's card 1 is its SAVE_RAM) |
//! | ps1 | `.gme` `.vmp` `.vgs`/`.mem` | DexDrive (3904 bytes), PSP/PS3 (128 bytes) and VGS (64 bytes) headers stripped |
//! | n64 | `.eep` `.mpk` `.sra` `.fla`/`.flash` | placed into the 0x48800-byte mupen/parallel `.srm` (EEPROM 0x800, four paks 0x8000 each, SRAM 0x8000, FlashRAM 0x20000); SRAM and FlashRAM are 32-bit word swapped |
//! | everything else | `.srm` `.sav` and the system's own names (`.eep` `.flash` for GBA, `.brm` for Sega CD) | raw |
//!
//! A few systems keep saves as their core's own files under the save directory instead, and for
//! those the answer is a relative path: Dreamcast `.vmu` (flycast's `vmu_save_A1.bin`), Saturn
//! `.bkr`/`.bcr` (Beetle Saturn's `<game>.bkr`). PSP and 3DS saves are folders and travel as
//! zips; see [`location`]. Arcade saves are refused: the boards cArcade, Continuum's own arcade
//! emulator, plays keep no save files, so there is nowhere for one to go; their save states work.
//!
//! An import never destroys the save it replaces: [`keep_before_import`] and [`import_folder`]
//! first copy it to Files, Continuum, Older copies (the folder a restore uses for the copies it
//! didn't keep).

use std::path::{Path, PathBuf};

/// Where a system's save lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveLocation {
    /// The frontend's battery save (`BatterySaves/<game>.srm`), through the existing code.
    Battery,
    /// A file the core owns, relative to the save directory.
    CoreFile(String),
    /// A folder the core owns, relative to the save directory, exchanged as a zip.
    Folder(String),
}

/// What an import resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveImport {
    pub location: SaveLocation,
    pub data: Vec<u8>,
    /// Plain sentence about what was done to the bytes.
    pub note: String,
}

pub const PS1_CARD: usize = 128 * 1024;
pub const N64_SRM: usize = 0x48800;
const N64_EEP: (usize, usize) = (0, 0x800);
const N64_MPK: (usize, usize) = (0x800, 0x20000);
const N64_SRA: (usize, usize) = (0x20800, 0x8000);
const N64_FLA: (usize, usize) = (0x28800, 0x20000);

const DSV_SNIP: &[u8] = b"|<--Snip above here to create a raw sav by excluding this DeSmuME savedata footer:";
const DSV_MARK: &[u8] = b"|-DESMUME SAVE-|";

/// Every extension the save import accepts, for the picker filter and the help line. `bin` is
/// Dreamcast-only (flycast's `vmu_save_A1.bin`); ImportCenter's drop path skips it so ROM `.bin`s
/// are never taken for saves, and other systems refuse it in [`import`].
pub const SAVE_EXTENSIONS: &[&str] = &[
    "srm", "sav", "dsv", "dsg", "mcr", "mcd", "mc", "gme", "vmp", "vgs", "mem", "eep", "mpk", "sra",
    "fla", "flash", "nvr", "nv", "bkr", "bcr", "brm", "vmu", "bin", "zip",
];

/// Where `system`'s save lives. `game_stem` is the game's filename without extension.
pub fn location(system: &str, game_stem: &str) -> SaveLocation {
    match system {
        // flycast with per-game VMUs off (its default, and the app does not set it) keeps the
        // shared VMUs in `<system>/dc/` (shell/libretro/oslib.cpp getVmuPath, checked at the
        // pinned commit), and the system and save directories are the same folder here.
        "dreamcast" => SaveLocation::CoreFile("dc/vmu_save_A1.bin".into()),
        "saturn" => SaveLocation::CoreFile(format!("{game_stem}.bkr")),
        "psp" => SaveLocation::Folder("PSP/SAVEDATA".into()),
        // Azahar's user dir is `<save>/Azahar/` (citra_libretro/core_settings.cpp), capital A.
        "n3ds" => SaveLocation::Folder("Azahar/sdmc/Nintendo 3DS".into()),
        _ => SaveLocation::Battery,
    }
}

fn ext_of(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

/// The formats a system's save can be exported as, first is the default.
pub fn export_formats(system: &str) -> Vec<&'static str> {
    match system {
        "ds" => vec!["sav", "dsv"],
        "ps1" => vec!["mcr", "mcd", "srm"],
        "n64" => vec!["srm", "eep", "sra", "fla", "mpk"],
        "gba" => vec!["sav", "srm"],
        "dreamcast" => vec!["vmu"],
        "saturn" => vec!["bkr"],
        "psp" | "n3ds" => vec!["zip"],
        _ => vec!["srm", "sav"],
    }
}

/// Converts an imported save file for `system`. `current` is the game's battery save as it is now
/// (empty when there is none); the N64 needs it so importing an `.eep` keeps the controller pak.
pub fn import(system: &str, file_name: &str, data: &[u8], current: &[u8], game_stem: &str) -> Result<SaveImport, String> {
    let ext = ext_of(file_name);
    if data.is_empty() {
        return Err(format!("{file_name} is empty"));
    }
    if ext == "zip" || matches!(system, "psp" | "n3ds") {
        return match location(system, game_stem) {
            SaveLocation::Folder(root) if ext == "zip" => Ok(SaveImport {
                location: SaveLocation::Folder(root),
                data: data.to_vec(),
                note: "a save folder zip, unpacked into the core's save folder".into(),
            }),
            SaveLocation::Folder(_) => Err(format!("{system} saves are folders; import them as a .zip")),
            _ => Err("a .zip is a save folder, which only PSP and 3DS saves are".to_string()),
        };
    }
    let battery = |bytes: Vec<u8>, note: &str| Ok(SaveImport { location: SaveLocation::Battery, data: bytes, note: note.into() });
    match system {
        "ds" => match ext.as_str() {
            "dsv" => battery(strip_dsv(data), "DeSmuME footer removed for melonDS"),
            "sav" | "dsg" | "srm" => battery(data.to_vec(), "raw DS save"),
            _ => Err(format!(".{ext} is not a DS save (want .sav, .dsv or .dsg)")),
        },
        "ps1" => {
            let card = ps1_card(&ext, data)?;
            battery(card, "PlayStation memory card, card 1")
        }
        "n64" => {
            let merged = n64_merge(&ext, data, current)?;
            battery(merged, "placed into the N64 save file")
        }
        "dreamcast" => match ext.as_str() {
            "vmu" | "bin" if data.len() == 128 * 1024 => Ok(SaveImport {
                location: location(system, game_stem),
                data: data.to_vec(),
                note: "VMU image for controller port A, slot 1".into(),
            }),
            "vmu" | "bin" => Err(format!("a VMU image is 131072 bytes; {file_name} is {}", data.len())),
            _ => Err(format!(".{ext} is not a Dreamcast VMU image (want .vmu or .bin; single .dci saves are not supported)")),
        },
        "saturn" => match ext.as_str() {
            "bkr" | "bcr" => Ok(SaveImport {
                location: SaveLocation::CoreFile(format!("{game_stem}.{ext}")),
                data: data.to_vec(),
                note: "Saturn backup RAM".into(),
            }),
            "srm" | "sav" => battery(data.to_vec(), "raw Saturn save"),
            _ => Err(format!(".{ext} is not a Saturn save (want .bkr)")),
        },
        "arcade" => Err(
            "These arcade games don\u{2019}t keep save files, so there\u{2019}s nothing to import. Save states work, though"
                .into(),
        ),
        _ => match ext.as_str() {
            "srm" | "sav" | "eep" | "flash" | "fla" | "sra" | "brm" | "dsg" | "nvr" => battery(data.to_vec(), "raw battery save"),
            _ => Err(format!(".{ext} is not a save format for {system}")),
        },
    }
}

/// Converts the stored save to `format` for export. For core-file systems `stored` is that file.
pub fn export(system: &str, format: &str, stored: &[u8]) -> Result<Vec<u8>, String> {
    if stored.is_empty() {
        return Err("there is no save yet".into());
    }
    let format = format.trim_start_matches('.').to_ascii_lowercase();
    match (system, format.as_str()) {
        ("ds", "dsv") => Ok(wrap_dsv(stored)),
        ("n64", "eep") => {
            let eep = region(stored, N64_EEP)?;
            // 4 kbit carts use 512 bytes; trim when the rest is blank.
            if eep[0x200..].iter().all(|&b| b == 0) || eep[0x200..].iter().all(|&b| b == 0xFF) {
                Ok(eep[..0x200].to_vec())
            } else {
                Ok(eep)
            }
        }
        ("n64", "sra") => Ok(word_swap(&region(stored, N64_SRA)?)),
        ("n64", "fla") => Ok(word_swap(&region(stored, N64_FLA)?)),
        ("n64", "mpk") => Ok(region(stored, (N64_MPK.0, 0x8000))?),
        _ if export_formats(system).contains(&format.as_str()) => Ok(stored.to_vec()),
        _ => Err(format!("{system} saves cannot be exported as .{format}")),
    }
}

fn region(stored: &[u8], (start, len): (usize, usize)) -> Result<Vec<u8>, String> {
    if stored.len() < N64_SRM {
        return Err(format!("the N64 save is {} bytes, not the {N64_SRM} the core writes", stored.len()));
    }
    Ok(stored[start..start + len].to_vec())
}

/// mupen64plus/parallel keep SRAM and FlashRAM as native-endian 32-bit words, so each 4-byte group
/// is reversed. A short trailing group (a truncated dump) is zero-padded to a whole word first, so
/// its bytes land where the core reads them instead of staying unswapped at the wrong offsets; the
/// N64 regions are all word multiples, so the padded length still fits.
fn word_swap(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    out.resize(data.len().div_ceil(4) * 4, 0);
    for chunk in out.chunks_exact_mut(4) {
        chunk.reverse();
    }
    out
}

fn n64_merge(ext: &str, data: &[u8], current: &[u8]) -> Result<Vec<u8>, String> {
    if ext == "srm" {
        if data.len() != N64_SRM {
            return Err(format!("an N64 .srm is {N64_SRM} bytes; this one is {}", data.len()));
        }
        return Ok(data.to_vec());
    }
    let mut out = if current.len() == N64_SRM { current.to_vec() } else { vec![0u8; N64_SRM] };
    let (start, cap, swap) = match ext {
        "eep" => (N64_EEP.0, N64_EEP.1, false),
        "mpk" => (N64_MPK.0, N64_MPK.1, false),
        "sra" => (N64_SRA.0, N64_SRA.1, true),
        "fla" | "flash" => (N64_FLA.0, N64_FLA.1, true),
        _ => return Err(format!(".{ext} is not an N64 save (want .eep, .sra, .fla, .mpk or .srm)")),
    };
    if data.len() > cap {
        return Err(format!("a .{ext} is at most {cap} bytes; this one is {}", data.len()));
    }
    let bytes = if swap { word_swap(data) } else { data.to_vec() };
    out[start..start + bytes.len()].copy_from_slice(&bytes);
    Ok(out)
}

fn ps1_card(ext: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    let body: &[u8] = if data.len() == PS1_CARD {
        data
    } else if data.len() == PS1_CARD + 3904 && data.starts_with(b"123-456-STD") {
        &data[3904..]
    } else if data.len() == PS1_CARD + 0x80 && data.starts_with(b"\0PMV") {
        &data[0x80..]
    } else if data.len() == PS1_CARD + 64 && data.starts_with(b"VgsM") {
        &data[64..]
    } else {
        return Err(format!(
            "a PlayStation memory card is {PS1_CARD} bytes (plus a header for .gme, .vmp or .vgs); this .{ext} is {}",
            data.len()
        ));
    };
    if !body.starts_with(b"MC") {
        return Err("this does not start like a PlayStation memory card (no MC header)".into());
    }
    Ok(body.to_vec())
}

/// Strips DeSmuME's footer. A file without one is returned as is.
pub fn strip_dsv(data: &[u8]) -> Vec<u8> {
    if data.ends_with(DSV_MARK) {
        if let Some(pos) = data.windows(DSV_SNIP.len()).rposition(|w| w == DSV_SNIP) {
            return data[..pos].to_vec();
        }
        if data.len() >= 122 {
            return data[..data.len() - 122].to_vec();
        }
    }
    data.to_vec()
}

/// DeSmuME's save type index for a chip size (its `save_types` table), and the address width.
fn desmume_type(size: usize) -> (u32, u32) {
    match size {
        512 => (1, 1),
        8192 => (2, 2),
        65536 => (3, 2),
        32768 => (4, 2),
        262144 => (5, 3),
        524288 => (6, 3),
        1048576 => (7, 3),
        2097152 => (8, 3),
        4194304 => (9, 3),
        8388608 => (10, 3),
        16777216 => (11, 3),
        33554432 => (12, 3),
        67108864 => (13, 3),
        _ => (0, 0),
    }
}

/// Appends DeSmuME's footer: the snip line, six little-endian words (size, padded size, type,
/// address width, memory size, version 0) and the marker. 122 bytes in all.
pub fn wrap_dsv(raw: &[u8]) -> Vec<u8> {
    let (kind, addr) = desmume_type(raw.len());
    let mut out = raw.to_vec();
    out.extend_from_slice(DSV_SNIP);
    for word in [raw.len() as u32, raw.len() as u32, kind, addr, raw.len() as u32, 0] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(DSV_MARK);
    out
}

/// Keeps the save at `path` before an import replaces it with `incoming`: a copy at
/// `<older>/<folder>/<stem> (before an import, <date>).<ext>`, numbered (`... 2.srm`, `... 3.srm`)
/// rather than ever written over an earlier copy. Nothing is kept when there is no save there yet
/// or it already holds exactly `incoming`, because then nothing would be lost. Returns where the
/// copy went.
///
/// An error means the copy could not be made, and the caller must leave the save alone.
pub fn keep_before_import(
    path: &Path,
    incoming: &[u8],
    older: &Path,
    folder: &str,
    now_ms: i64,
) -> Result<Option<PathBuf>, String> {
    let current = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("could not read {}: {e}", path.display())),
    };
    if current.is_empty() || current == incoming {
        return Ok(None);
    }
    let dir = folder
        .split('/')
        .filter(|part| !part.is_empty() && *part != "." && *part != "..")
        .fold(older.to_path_buf(), |dir, part| dir.join(part));
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not make {}: {e}", dir.display()))?;
    let name = path.file_name().map_or_else(|| "save".to_string(), |n| n.to_string_lossy().into_owned());
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), format!(".{ext}")),
        _ => (name.clone(), String::new()),
    };
    let stamp = crate::sync::date_stamp(now_ms);
    let mut number = 1u32;
    loop {
        let suffix = if number == 1 { String::new() } else { format!(" {number}") };
        let target = dir.join(format!("{stem} (before an import, {stamp}){suffix}{ext}"));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&target) {
            Ok(mut file) => {
                use std::io::Write;
                return match file.write_all(&current).and_then(|()| file.sync_all()) {
                    Ok(()) => Ok(Some(target)),
                    Err(e) => {
                        drop(file);
                        let _ = std::fs::remove_file(&target);
                        Err(format!("could not keep a copy at {}: {e}", target.display()))
                    }
                };
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => number += 1,
            Err(e) => return Err(format!("could not keep a copy at {}: {e}", target.display())),
        }
    }
}

/// What [`import_folder`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderImport {
    /// The files written, relative to the save folder.
    pub written: Vec<String>,
    /// The copies kept of the files they replaced.
    pub kept: Vec<PathBuf>,
}

/// Unpacks a save folder zip (PSP `SAVEDATA`, 3DS) into `dest`, keeping every file it replaces
/// with [`keep_before_import`] first, under `<older>/<folder>/` and the file's own subfolders.
/// Unpacked into a staging folder beside `dest` first, so an archive that can't be read changes
/// nothing.
pub fn import_folder(
    archive: &Path,
    dest: &Path,
    older: &Path,
    folder: &str,
    now_ms: i64,
) -> Result<FolderImport, String> {
    let parent = dest.parent().ok_or_else(|| format!("{} has no parent folder", dest.display()))?;
    std::fs::create_dir_all(parent).map_err(|e| format!("could not make {}: {e}", parent.display()))?;
    // A dot name: the sync and the cores both skip it, should the app be stopped half way.
    let staging = parent.join(format!(".continuum-save-import-{}-{now_ms}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let result = (|| {
        let names = super::archive::extract_tree(archive, &staging)?;
        let mut done = FolderImport::default();
        for name in names {
            let from = staging.join(&name);
            let to = dest.join(&name);
            let incoming = std::fs::read(&from).map_err(|e| format!("could not read {name}: {e}"))?;
            let sub = Path::new(&name).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
            let kept_in = if sub.is_empty() { folder.to_string() } else { format!("{folder}/{sub}") };
            if let Some(kept) = keep_before_import(&to, &incoming, older, &kept_in, now_ms)? {
                done.kept.push(kept);
            }
            if let Some(dir) = to.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("could not make {}: {e}", dir.display()))?;
            }
            std::fs::rename(&from, &to).map_err(|e| format!("could not write {}: {e}", to.display()))?;
            done.written.push(name);
        }
        Ok(done)
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::testdir::TestDir;

    /// 2026-10-12 14:15:59 UTC.
    const NOW: i64 = 1_791_814_559_000;

    #[test]
    fn an_import_keeps_the_save_it_replaces() {
        let dir = TestDir::new("keep");
        let older = dir.path().join("Older copies");
        let save = dir.write("BatterySaves/Zelda.srm", b"the player's own save");
        let kept = keep_before_import(&save, b"imported", &older, "BatterySaves", NOW).unwrap().unwrap();
        assert_eq!(
            kept,
            older.join("BatterySaves").join(format!("Zelda (before an import, {}).srm", crate::sync::date_stamp(NOW)))
        );
        assert_eq!(std::fs::read(&kept).unwrap(), b"the player's own save");
        // A copy, not a move: the import itself replaces the file.
        assert_eq!(std::fs::read(&save).unwrap(), b"the player's own save");
        // A second import in the same second gets a number, never the first copy's place.
        std::fs::write(&save, b"imported").unwrap();
        let second = keep_before_import(&save, b"another", &older, "BatterySaves", NOW).unwrap().unwrap();
        assert!(second.to_string_lossy().ends_with(" UTC) 2.srm"), "{}", second.display());
        assert_eq!(std::fs::read(&second).unwrap(), b"imported");
        assert_eq!(std::fs::read(&kept).unwrap(), b"the player's own save");
    }

    #[test]
    fn nothing_is_kept_when_nothing_would_be_lost() {
        let dir = TestDir::new("keepnone");
        let older = dir.path().join("Older copies");
        // No save yet, an empty one, or the same bytes again.
        assert_eq!(keep_before_import(&dir.path().join("none.srm"), b"x", &older, "BatterySaves", NOW).unwrap(), None);
        let empty = dir.write("empty.srm", b"");
        assert_eq!(keep_before_import(&empty, b"x", &older, "BatterySaves", NOW).unwrap(), None);
        let same = dir.write("same.srm", b"x");
        assert_eq!(keep_before_import(&same, b"x", &older, "BatterySaves", NOW).unwrap(), None);
        assert!(!older.exists());
        // A folder that tries to climb out stays inside Older copies.
        let save = dir.write("a.bkr", b"old");
        let kept = keep_before_import(&save, b"new", &older, "../../x/./", NOW).unwrap().unwrap();
        assert_eq!(kept.parent().unwrap(), older.join("x"));
        // A save that can't be copied is an error, so the caller leaves it alone.
        std::fs::write(dir.path().join("blocked"), b"a file where the folder would go").unwrap();
        assert!(keep_before_import(&save, b"new", &dir.path().join("blocked"), "", NOW).is_err());
    }

    #[test]
    fn a_save_folder_import_keeps_every_file_it_replaces() {
        let dir = TestDir::new("keepfolder");
        let older = dir.path().join("Older copies");
        let dest = dir.path().join("PSP/SAVEDATA");
        dir.write("PSP/SAVEDATA/ULUS10041DATA00/DATA.BIN", b"old progress");
        dir.write("PSP/SAVEDATA/ULUS10041DATA00/PARAM.SFO", b"same");
        let zip_path = dir.path().join("save.zip");
        {
            use std::io::Write;
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            for (name, bytes) in [
                ("ULUS10041DATA00/DATA.BIN", &b"new progress"[..]),
                ("ULUS10041DATA00/PARAM.SFO", b"same"),
                ("ULUS10041DATA00/ICON0.PNG", b"png"),
            ] {
                zip.start_file(name, options).unwrap();
                zip.write_all(bytes).unwrap();
            }
            zip.finish().unwrap();
        }
        let done = import_folder(&zip_path, &dest, &older, "PSP/SAVEDATA", NOW).unwrap();
        assert_eq!(done.written.len(), 3);
        assert_eq!(std::fs::read(dest.join("ULUS10041DATA00/DATA.BIN")).unwrap(), b"new progress");
        assert_eq!(std::fs::read(dest.join("ULUS10041DATA00/ICON0.PNG")).unwrap(), b"png");
        // Only the file whose bytes changed was kept, in its own folder under Older copies.
        assert_eq!(done.kept.len(), 1);
        assert!(done.kept[0].starts_with(older.join("PSP/SAVEDATA/ULUS10041DATA00")));
        assert_eq!(std::fs::read(&done.kept[0]).unwrap(), b"old progress");
        // The staging folder is gone, and a zip that can't be read changes nothing.
        let left: Vec<_> = std::fs::read_dir(dir.path().join("PSP")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left, vec![std::ffi::OsString::from("SAVEDATA")]);
        let bad = dir.write("bad.zip", b"not a zip");
        assert!(import_folder(&bad, &dest, &older, "PSP/SAVEDATA", NOW).is_err());
        assert_eq!(std::fs::read(dest.join("ULUS10041DATA00/DATA.BIN")).unwrap(), b"new progress");
    }

    #[test]
    fn dsv_round_trip() {
        let raw = vec![0xABu8; 65536];
        let dsv = wrap_dsv(&raw);
        assert_eq!(dsv.len(), raw.len() + 122);
        assert_eq!(strip_dsv(&dsv), raw);
        let got = import("ds", "Pokemon.dsv", &dsv, &[], "Pokemon").unwrap();
        assert_eq!(got.location, SaveLocation::Battery);
        assert_eq!(got.data, raw);
        assert_eq!(export("ds", "dsv", &raw).unwrap(), dsv);
        assert_eq!(import("ds", "a.sav", &raw, &[], "a").unwrap().data, raw);
        assert!(import("ds", "a.mcr", &raw, &[], "a").is_err());
    }

    fn card() -> Vec<u8> {
        let mut c = vec![0u8; PS1_CARD];
        c[0] = b'M';
        c[1] = b'C';
        c
    }

    #[test]
    fn ps1_cards_with_and_without_headers() {
        let c = card();
        assert_eq!(import("ps1", "x.mcr", &c, &[], "x").unwrap().data, c);
        let mut gme = b"123-456-STD".to_vec();
        gme.resize(3904, 0);
        gme.extend_from_slice(&c);
        assert_eq!(import("ps1", "x.gme", &gme, &[], "x").unwrap().data, c);
        let mut vmp = b"\0PMV".to_vec();
        vmp.resize(0x80, 0);
        vmp.extend_from_slice(&c);
        assert_eq!(import("ps1", "x.vmp", &vmp, &[], "x").unwrap().data, c);
        assert!(import("ps1", "x.mcr", &[0u8; 100], &[], "x").is_err());
        assert!(import("ps1", "x.mcr", &vec![0u8; PS1_CARD], &[], "x").unwrap_err().contains("MC"));
        assert_eq!(export("ps1", "mcd", &c).unwrap(), c);
    }

    #[test]
    fn n64_parts_merge_and_export() {
        let eep: Vec<u8> = (0..512u32).map(|i| i as u8).collect();
        let first = import("n64", "Mario.eep", &eep, &[], "Mario").unwrap().data;
        assert_eq!(first.len(), N64_SRM);
        assert_eq!(&first[..512], &eep[..]);
        // A pak imported later keeps the EEPROM.
        let pak = vec![0x11u8; 0x8000];
        let second = import("n64", "Mario.mpk", &pak, &first, "Mario").unwrap().data;
        assert_eq!(&second[..512], &eep[..]);
        assert_eq!(&second[0x800..0x8800], &pak[..]);
        assert_eq!(export("n64", "eep", &second).unwrap(), eep);
        assert_eq!(export("n64", "mpk", &second).unwrap(), pak);
        // SRAM is word swapped on the way in and back on the way out.
        let sra: Vec<u8> = (0..0x8000u32).map(|i| (i % 251) as u8).collect();
        let merged = import("n64", "Zelda.sra", &sra, &[], "Zelda").unwrap().data;
        assert_eq!(&merged[N64_SRA.0..N64_SRA.0 + 4], &[sra[3], sra[2], sra[1], sra[0]]);
        assert_eq!(export("n64", "sra", &merged).unwrap(), sra);
        let fla = vec![0x5Au8; 0x20000];
        let merged = import("n64", "x.flash", &fla, &merged, "x").unwrap().data;
        assert_eq!(export("n64", "fla", &merged).unwrap(), fla);
        assert!(import("n64", "x.eep", &vec![0u8; 0x900], &[], "x").is_err());
        assert!(export("n64", "eep", &[1, 2, 3]).is_err());
    }

    #[test]
    fn core_file_systems_and_folders() {
        let vmu = vec![0u8; 128 * 1024];
        let got = import("dreamcast", "a.vmu", &vmu, &[], "Sonic Adventure").unwrap();
        assert_eq!(got.location, SaveLocation::CoreFile("dc/vmu_save_A1.bin".into()));
        assert!(import("dreamcast", "a.vmu", &[0u8; 10], &[], "x").is_err());
        let got = import("saturn", "Nights.bkr", &[1u8; 32768], &[], "Nights").unwrap();
        assert_eq!(got.location, SaveLocation::CoreFile("Nights.bkr".into()));
        let got = import("psp", "save.zip", b"PK..", &[], "Game").unwrap();
        assert_eq!(got.location, SaveLocation::Folder("PSP/SAVEDATA".into()));
        assert!(import("psp", "save.sav", b"x", &[], "Game").is_err());
        assert!(import("snes", "a.zip", b"x", &[], "a").is_err());
        // The arcade boards cArcade plays keep no save files: every arcade save is refused.
        for name in ["sf2.nvr", "sf2.nv", "sf2.srm", "sf2.sav"] {
            let err = import("arcade", name, b"nv", &[], "sf2").unwrap_err();
            assert!(err.contains("don\u{2019}t keep save files"), "{name}: {err}");
        }
    }

    #[test]
    fn word_swap_pads_a_short_tail() {
        assert_eq!(word_swap(&[1, 2, 3, 4, 5, 6]), vec![4, 3, 2, 1, 0, 0, 6, 5]);
        let got = import("n64", "a.sra", &[1, 2, 3, 4, 5], &[], "x").unwrap();
        assert_eq!(&got.data[N64_SRA.0..N64_SRA.0 + 8], &[4, 3, 2, 1, 0, 0, 0, 5]);
        assert!(SAVE_EXTENSIONS.contains(&"bin"));
        assert!(import("dreamcast", "vmu_save_A1.bin", &vec![0u8; 128 * 1024], &[], "x").is_ok());
        assert!(import("snes", "a.bin", &[0u8; 8], &[], "x").is_err());
    }

    #[test]
    fn generic_raw_and_errors() {
        assert_eq!(import("gba", "a.flash", &[9u8; 131072], &[], "a").unwrap().data.len(), 131072);
        assert_eq!(import("snes", "a.srm", &[1u8; 8192], &[], "a").unwrap().location, SaveLocation::Battery);
        assert!(import("snes", "a.srm", &[], &[], "a").is_err());
        assert!(import("snes", "a.txt", b"x", &[], "a").is_err());
        assert!(export("snes", "srm", &[]).is_err());
        assert!(export("snes", "dsv", &[1]).is_err());
        assert_eq!(export_formats("ds")[0], "sav");
    }
}
