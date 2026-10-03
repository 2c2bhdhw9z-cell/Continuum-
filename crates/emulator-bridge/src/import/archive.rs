//! `.zip` and `.7z`: list, unpack, and zip a folder back up.
//!
//! Unpacking is FLAT and keeps each file's own name, because the Library is flat: a cue sheet
//! names its tracks by bare filename, and they must land beside it. Entries are streamed to disk
//! one at a time, never held in memory, so a 700 MB track inside a zip costs a buffer, not 700 MB.
//!
//! Folder zips (PSP and 3DS saves) keep their relative paths instead, with every `..` and absolute
//! component refused, so a hostile archive cannot write outside the folder it is unpacked into.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

/// What kind of archive a path is, from its first bytes. None for neither.
fn kind(path: &Path) -> Result<Option<&'static str>, String> {
    let mut head = [0u8; 6];
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let n = file.read(&mut head).map_err(|e| e.to_string())?;
    if n >= 4 && (head[..4] == [b'P', b'K', 3, 4] || head[..4] == [b'P', b'K', 5, 6]) {
        return Ok(Some("zip"));
    }
    if n >= 6 && head == [b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C] {
        return Ok(Some("7z"));
    }
    Ok(None)
}

/// The entry names in an archive, folders ending in `/`.
pub fn list(path: &Path) -> Result<Vec<String>, String> {
    match kind(path)? {
        Some("zip") => {
            let archive = zip::ZipArchive::new(File::open(path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("not a readable zip: {e}"))?;
            Ok(archive.file_names().map(String::from).collect())
        }
        Some("7z") => {
            let archive = sevenz_rust2::Archive::open(path).map_err(|e| format!("not a readable 7z: {e}"))?;
            Ok(archive
                .files
                .iter()
                .map(|f| if f.is_directory() { format!("{}/", f.name()) } else { f.name().to_string() })
                .collect())
        }
        _ => Err("this is neither a zip nor a 7z archive".into()),
    }
}

/// The bare filename an entry unpacks to, or None for folders, Mac resource forks and names that
/// would escape the folder.
fn flat_name(entry: &str) -> Option<String> {
    let normal = entry.replace('\\', "/");
    if normal.ends_with('/') || normal.to_ascii_lowercase().starts_with("__macosx/") {
        return None;
    }
    let base = normal.rsplit('/').next()?.trim();
    if base.is_empty() || base == "." || base == ".." || base.starts_with("._") || base == ".DS_Store" {
        return None;
    }
    Some(base.to_string())
}

/// A relative path that stays inside the destination, or None.
fn safe_relative(entry: &str) -> Option<PathBuf> {
    let normal = entry.replace('\\', "/");
    if normal.to_ascii_lowercase().starts_with("__macosx/") {
        return None;
    }
    let path = Path::new(&normal);
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

fn write_stream(reader: &mut dyn Read, target: &Path) -> Result<u64, String> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut out = File::create(target).map_err(|e| format!("{}: {e}", target.display()))?;
    let n = std::io::copy(reader, &mut out).map_err(|e| format!("{}: {e}", target.display()))?;
    out.flush().map_err(|e| e.to_string())?;
    Ok(n)
}

/// Unpacks every file flat into `dest`, keeping each file's own name. Returns the names written,
/// in archive order. A later entry with the same name replaces an earlier one.
pub fn extract_flat(path: &Path, dest: &Path) -> Result<Vec<String>, String> {
    extract(path, dest, true)
}

/// Unpacks keeping relative folders, refusing any entry that would land outside `dest`.
pub fn extract_tree(path: &Path, dest: &Path) -> Result<Vec<String>, String> {
    extract(path, dest, false)
}

fn extract(path: &Path, dest: &Path, flat: bool) -> Result<Vec<String>, String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let target_for = |name: &str| -> Option<(String, PathBuf)> {
        if flat {
            flat_name(name).map(|n| (n.clone(), dest.join(n)))
        } else {
            if name.ends_with('/') {
                return None;
            }
            safe_relative(name).map(|r| (r.to_string_lossy().into_owned(), dest.join(r)))
        }
    };
    let mut written: Vec<String> = Vec::new();
    match kind(path)? {
        Some("zip") => {
            let mut archive = zip::ZipArchive::new(File::open(path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("not a readable zip: {e}"))?;
            for i in 0..archive.len() {
                let mut entry = archive.by_index(i).map_err(|e| format!("zip entry {i}: {e}"))?;
                if entry.is_dir() {
                    continue;
                }
                let Some((label, target)) = target_for(entry.name()) else { continue };
                write_stream(&mut entry, &target)?;
                written.retain(|w| w != &label);
                written.push(label);
            }
        }
        Some("7z") => {
            let mut reader = sevenz_rust2::ArchiveReader::open(path, sevenz_rust2::Password::empty())
                .map_err(|e| format!("not a readable 7z: {e}"))?;
            let mut failure: Option<String> = None;
            reader
                .for_each_entries(|entry, data| {
                    if entry.is_directory() {
                        return Ok(true);
                    }
                    match target_for(entry.name()) {
                        Some((label, target)) => match write_stream(data, &target) {
                            Ok(_) => {
                                written.retain(|w| w != &label);
                                written.push(label);
                            }
                            Err(e) => {
                                failure = Some(e);
                                return Ok(false);
                            }
                        },
                        None => {
                            std::io::copy(data, &mut std::io::sink())?;
                        }
                    }
                    Ok(true)
                })
                .map_err(|e| format!("7z: {e}"))?;
            if let Some(e) = failure {
                return Err(e);
            }
        }
        _ => return Err("this is neither a zip nor a 7z archive".into()),
    }
    Ok(written)
}

/// Zips the CONTENTS of `dir` (paths relative to it) into `out`. Returns how many files went in.
pub fn zip_directory(dir: &Path, out: &Path) -> Result<u32, String> {
    let file = File::create(out).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut count = 0u32;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&current)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for path in entries {
            let rel = path.strip_prefix(dir).map_err(|e| e.to_string())?;
            let name = rel.to_string_lossy().replace('\\', "/");
            if path.is_dir() {
                zip.add_directory(format!("{name}/"), options).map_err(|e| e.to_string())?;
                stack.push(path);
            } else {
                zip.start_file(name, options).map_err(|e| e.to_string())?;
                let mut src = File::open(&path).map_err(|e| e.to_string())?;
                std::io::copy(&mut src, &mut zip).map_err(|e| e.to_string())?;
                count += 1;
            }
        }
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::testdir::TestDir;

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in entries {
            if name.ends_with('/') {
                zip.add_directory(*name, options).unwrap();
            } else {
                zip.start_file(*name, options).unwrap();
                zip.write_all(bytes).unwrap();
            }
        }
        zip.finish().unwrap();
    }

    #[test]
    fn zip_lists_and_unpacks_flat() {
        let dir = TestDir::new("zipflat");
        let z = dir.path().join("game.zip");
        make_zip(&z, &[
            ("Game/", b""),
            ("Game/Game.cue", b"FILE \"Game.bin\" BINARY"),
            ("Game/Game.bin", &[7u8; 5000]),
            ("__MACOSX/Game/._Game.bin", b"junk"),
        ]);
        let names = list(&z).unwrap();
        assert!(names.contains(&"Game/Game.cue".to_string()));
        let out = dir.path().join("out");
        let written = extract_flat(&z, &out).unwrap();
        assert_eq!(written, vec!["Game.cue", "Game.bin"]);
        assert_eq!(std::fs::read(out.join("Game.bin")).unwrap().len(), 5000);
        assert!(!out.join("._Game.bin").exists());
    }

    #[test]
    fn tree_unpack_refuses_escape() {
        let dir = TestDir::new("ziptree");
        let z = dir.path().join("save.zip");
        make_zip(&z, &[("ULUS10041DATA00/PARAM.SFO", b"sfo"), ("../evil.txt", b"x"), ("/abs.txt", b"y")]);
        let out = dir.path().join("SAVEDATA");
        let written = extract_tree(&z, &out).unwrap();
        assert_eq!(written, vec!["ULUS10041DATA00/PARAM.SFO"]);
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn zip_directory_round_trips() {
        let dir = TestDir::new("zipdir");
        dir.write("src/A/one.bin", b"1");
        dir.write("src/A/B/two.bin", b"22");
        let z = dir.path().join("x.zip");
        assert_eq!(zip_directory(&dir.path().join("src"), &z).unwrap(), 2);
        let out = dir.path().join("back");
        let mut written = extract_tree(&z, &out).unwrap();
        written.sort();
        assert_eq!(written, vec!["A/B/two.bin", "A/one.bin"]);
        assert_eq!(std::fs::read(out.join("A/B/two.bin")).unwrap(), b"22");
    }

    #[test]
    fn sevenz_lists_and_unpacks() {
        let dir = TestDir::new("7z");
        dir.write("src/Sonic.md", &[3u8; 3000]);
        dir.write("src/readme.txt", b"hi");
        let archive = dir.path().join("sonic.7z");
        sevenz_rust2::compress_to_path(dir.path().join("src"), &archive).unwrap();
        let mut names = list(&archive).unwrap();
        names.sort();
        assert!(names.iter().any(|n| n.ends_with("Sonic.md")), "{names:?}");
        let out = dir.path().join("out");
        let written = extract_flat(&archive, &out).unwrap();
        assert!(written.contains(&"Sonic.md".to_string()));
        assert_eq!(std::fs::read(out.join("Sonic.md")).unwrap().len(), 3000);
        let d = crate::import::detect::detect_path(&archive);
        assert_eq!(d.system, "genesis", "{}", d.reason);
    }

    #[test]
    fn not_an_archive_says_so() {
        let dir = TestDir::new("notzip");
        let p = dir.write("x.zip", b"hello");
        assert!(list(&p).unwrap_err().contains("neither"));
    }
}
