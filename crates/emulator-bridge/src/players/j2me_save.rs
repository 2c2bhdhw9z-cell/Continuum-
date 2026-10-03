//! J2ME saves: the MIDlet's RMS record stores (and any other file it wrote), as Manic EMU's
//! `<game>.J2meJS.srm`.
//!
//! THE ENGINE SIDE. J2meJS keeps the phone's whole file system in one IndexedDB database,
//! `asyncStorage` version 4, object store `fs4`, keyed by `pathname`, with an index on
//! `parentDir`. A file record is `{pathname, isDir:false, mtime, size, parentDir, data: Blob}` and
//! a folder `{pathname, isDir:true, mtime, parentDir}` (see `Store.prototype.init` and
//! `create`/`mkdir` in J2meJS java/bld/main-all.js at the pinned commit). Record stores live
//! under `/RecordStore/`. Its `fs.exportStore` dumps every record, file data as an array of
//! signed bytes; the player page turns that into [`PlayerFile`]s for us.
//!
//! THE FILE. Manic's `.J2meJS.srm` is a zip:
//!   * `saves/<flat>` the bytes of each file, where `<flat>` is the path without its leading `/`
//!     and with every other `/` turned into `_` (`root.dat` for an empty name);
//!   * `metadata/<flat>.json` `{"path":..,"isDir":false,"size":..,"mtime":..}` for each one,
//!     which is how the real path survives the flattening;
//!   * `info.json` `{"gameName":..,"exportDate":..,"version":"1.0"}`.
//!
//! Folders are not stored; [`seed_records`] recreates the ones the files need.

use std::io::{Cursor, Read, Write};

use super::flat_json::{self, FlatValue};

/// One file of the phone's file system.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerFile {
    /// Absolute, like `/RecordStore/hiscore.db`.
    pub path: String,
    /// Milliseconds since 1970, as JavaScript's `Date.now()`.
    pub mtime: f64,
    pub data: Vec<u8>,
}

/// One record to put into `asyncStorage`/`fs4` before the engine starts.
#[derive(Debug, Clone, PartialEq)]
pub struct SeedRecord {
    pub pathname: String,
    pub is_dir: bool,
    /// None only for `/`.
    pub parent_dir: Option<String>,
    pub mtime: f64,
    /// Empty for a folder.
    pub data: Vec<u8>,
}

/// Bounds on what a save file may unpack to, so a hostile zip cannot exhaust memory.
const MAX_FILES: usize = 4096;
const MAX_TOTAL: u64 = 64 * 1024 * 1024;

/// The path the engine's own `normalizePath` would give: no trailing `/` (except the root) and
/// no doubled `/`.
pub fn normalize(path: &str) -> String {
    let mut out = String::with_capacity(path.len() + 1);
    if !path.starts_with('/') {
        out.push('/');
    }
    let mut last_slash = false;
    for c in path.chars() {
        if c == '/' {
            if last_slash {
                continue;
            }
            last_slash = true;
        } else {
            last_slash = false;
        }
        out.push(c);
    }
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    out
}

/// The engine's `dirname`: `/a/b` is `/a`, `/a` is `/`.
pub fn dirname(path: &str) -> String {
    let path = normalize(path);
    match path.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(i) => path[..i].to_string(),
    }
}

/// Manic's flattened name for a path.
pub fn flat_name(path: &str) -> String {
    let trimmed = path.strip_prefix('/').unwrap_or(path);
    let flat = trimmed.replace('/', "_");
    if flat.is_empty() { "root.dat".to_string() } else { flat }
}

/// Builds a `.J2meJS.srm` from the phone's files. `export_date` is an ISO 8601 time.
pub fn encode(files: &[PlayerFile], game_name: &str, export_date: &str) -> Result<Vec<u8>, String> {
    let mut sorted: Vec<&PlayerFile> = files.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut used = std::collections::HashSet::new();
    for file in sorted {
        let path = normalize(&file.path);
        // Two paths can flatten to one name (`/a_b` and `/a/b`). Manic would lose one; this keeps
        // both under distinct names, and the metadata still names the real path of each.
        let base = flat_name(&path);
        let mut flat = base.clone();
        let mut n = 2;
        while !used.insert(flat.clone()) {
            flat = format!("{base}~{n}");
            n += 1;
        }
        zip.start_file(format!("saves/{flat}"), options).map_err(|e| e.to_string())?;
        zip.write_all(&file.data).map_err(|e| e.to_string())?;
        let meta = flat_json::write_object(&[
            ("path".into(), FlatValue::Str(path.clone())),
            ("isDir".into(), FlatValue::Bool(false)),
            ("size".into(), FlatValue::Num(file.data.len() as f64)),
            ("mtime".into(), FlatValue::Num(file.mtime)),
        ]);
        zip.start_file(format!("metadata/{flat}.json"), options).map_err(|e| e.to_string())?;
        zip.write_all(meta.as_bytes()).map_err(|e| e.to_string())?;
    }
    // Pretty printed with two spaces, as JSON.stringify(info, null, 2) writes it.
    let info = format!(
        "{{\n  \"gameName\": {},\n  \"exportDate\": {},\n  \"version\": \"1.0\"\n}}",
        flat_json::quote(if game_name.is_empty() { "Unknown" } else { game_name }),
        flat_json::quote(export_date)
    );
    zip.start_file("info.json", options).map_err(|e| e.to_string())?;
    zip.write_all(info.as_bytes()).map_err(|e| e.to_string())?;
    let cursor = zip.finish().map_err(|e| e.to_string())?;
    Ok(cursor.into_inner())
}

/// The path a `saves/` entry stands for when its metadata file is missing. Manic flattens every
/// `/`, so the split is a guess, made the one way that is right for record stores.
fn guessed_path(flat: &str) -> String {
    match flat.strip_prefix("RecordStore_") {
        Some(rest) => format!("/RecordStore/{rest}"),
        None => format!("/{flat}"),
    }
}

/// Reads a `.J2meJS.srm` back into files, sorted by path.
pub fn decode(bytes: &[u8]) -> Result<Vec<PlayerFile>, String> {
    if bytes.len() < 4 || bytes[..2] != *b"PK" {
        return Err("not a J2ME save: a .J2meJS.srm is a zip, and this is not one".into());
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("not a readable J2ME save zip: {e}"))?;
    if archive.len() > MAX_FILES * 2 + 1 {
        return Err(format!("the save holds {} entries, more than a phone save ever has", archive.len()));
    }
    let mut data: Vec<(String, Vec<u8>)> = Vec::new();
    let mut meta: std::collections::HashMap<String, (String, f64, bool)> = Default::default();
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        total = total.saturating_add(entry.size());
        if total > MAX_TOTAL {
            return Err("the save unpacks to more than 64 MB, which no phone save is".into());
        }
        let mut bytes = Vec::with_capacity(entry.size().min(MAX_TOTAL) as usize);
        entry
            .by_ref()
            .take(MAX_TOTAL + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("{name}: {e}"))?;
        if let Some(flat) = name.strip_prefix("saves/") {
            if !flat.is_empty() && !flat.contains('/') {
                data.push((flat.to_string(), bytes));
            }
        } else if let Some(flat) = name.strip_prefix("metadata/").and_then(|n| n.strip_suffix(".json")) {
            let text = String::from_utf8_lossy(&bytes);
            let object = flat_json::parse_object(&text).map_err(|e| format!("{name}: {e}"))?;
            let get = |k: &str| object.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
            let path = get("path").and_then(|v| v.as_str().map(String::from));
            let mtime = get("mtime").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let is_dir = get("isDir").and_then(|v| v.as_bool()).unwrap_or(false);
            if let Some(path) = path {
                meta.insert(flat.to_string(), (path, mtime, is_dir));
            }
        }
    }
    let mut files = Vec::with_capacity(data.len());
    for (flat, bytes) in data {
        let (path, mtime) = match meta.get(&flat) {
            Some((_, _, true)) => continue,
            Some((path, mtime, false)) => (normalize(path), *mtime),
            None => (guessed_path(&flat), 0.0),
        };
        if path.split('/').any(|part| part == "..") {
            return Err(format!("{flat}: the path {path} climbs out of the phone's storage"));
        }
        files.push(PlayerFile { path, mtime, data: bytes });
    }
    if files.len() > MAX_FILES {
        return Err(format!("the save holds {} files, more than a phone save ever has", files.len()));
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// The records to seed the engine's storage with: every file, and every folder above them
/// (`/` included), folders first.
pub fn seed_records(files: &[PlayerFile]) -> Vec<SeedRecord> {
    let mut dirs: std::collections::BTreeMap<String, f64> = Default::default();
    dirs.insert("/".into(), 0.0);
    let mut out_files = Vec::with_capacity(files.len());
    for file in files {
        let path = normalize(&file.path);
        if path == "/" {
            continue;
        }
        let mut dir = dirname(&path);
        loop {
            let entry = dirs.entry(dir.clone()).or_insert(file.mtime);
            if *entry < file.mtime {
                *entry = file.mtime;
            }
            if dir == "/" {
                break;
            }
            dir = dirname(&dir);
        }
        out_files.push(SeedRecord {
            parent_dir: Some(dirname(&path)),
            pathname: path,
            is_dir: false,
            mtime: file.mtime,
            data: file.data.clone(),
        });
    }
    let mut out: Vec<SeedRecord> = dirs
        .into_iter()
        .map(|(pathname, mtime)| SeedRecord {
            parent_dir: (pathname != "/").then(|| dirname(&pathname)),
            pathname,
            is_dir: true,
            mtime,
            data: Vec::new(),
        })
        .collect();
    out.extend(out_files);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, mtime: f64, data: &[u8]) -> PlayerFile {
        PlayerFile { path: path.into(), mtime, data: data.to_vec() }
    }

    #[test]
    fn paths_follow_the_engine() {
        assert_eq!(normalize("/RecordStore//hi.db"), "/RecordStore/hi.db");
        assert_eq!(normalize("/RecordStore/"), "/RecordStore");
        assert_eq!(normalize("a/b"), "/a/b");
        assert_eq!(normalize("/"), "/");
        assert_eq!(dirname("/RecordStore/hi.db"), "/RecordStore");
        assert_eq!(dirname("/RecordStore"), "/");
        assert_eq!(flat_name("/RecordStore/hi.db"), "RecordStore_hi.db");
        assert_eq!(flat_name("/"), "root.dat");
    }

    #[test]
    fn srm_round_trips_in_manics_layout() {
        let files = vec![
            file("/RecordStore/score.db", 1_700_000_000_123.0, &[1, 2, 3, 0xFF]),
            file("/Persistent/cfg.txt", 5.0, b"lang=en"),
        ];
        let bytes = encode(&files, "Bounce", "2026-10-03T12:00:00.000Z").unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let names: Vec<String> = zip.file_names().map(String::from).collect();
        for want in ["saves/RecordStore_score.db", "metadata/RecordStore_score.db.json",
                     "saves/Persistent_cfg.txt", "metadata/Persistent_cfg.txt.json", "info.json"] {
            assert!(names.iter().any(|n| n == want), "{want} in {names:?}");
        }
        let mut meta = String::new();
        zip.by_name("metadata/RecordStore_score.db.json").unwrap().read_to_string(&mut meta).unwrap();
        assert_eq!(meta, r#"{"path":"/RecordStore/score.db","isDir":false,"size":4,"mtime":1700000000123}"#);
        let mut info = String::new();
        zip.by_name("info.json").unwrap().read_to_string(&mut info).unwrap();
        assert!(info.contains("\"gameName\": \"Bounce\"") && info.contains("\"version\": \"1.0\""));

        let back = decode(&bytes).unwrap();
        let mut want = files.clone();
        want.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(back, want);
    }

    #[test]
    fn colliding_flat_names_keep_both_files() {
        let files = vec![file("/a_b", 1.0, b"one"), file("/a/b", 2.0, b"two")];
        let back = decode(&encode(&files, "x", "d").unwrap()).unwrap();
        assert_eq!(back.len(), 2);
        assert!(back.iter().any(|f| f.path == "/a_b" && f.data == b"one"));
        assert!(back.iter().any(|f| f.path == "/a/b" && f.data == b"two"));
    }

    #[test]
    fn missing_metadata_guesses_record_store_paths() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("saves/RecordStore_level.db", options).unwrap();
        zip.write_all(b"L").unwrap();
        zip.start_file("saves/other.bin", options).unwrap();
        zip.write_all(b"O").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let back = decode(&bytes).unwrap();
        assert_eq!(back[0].path, "/RecordStore/level.db");
        assert_eq!(back[1].path, "/other.bin");
    }

    #[test]
    fn refuses_what_is_not_a_save() {
        assert!(decode(b"").is_err());
        assert!(decode(b"{\"version\":1}").is_err());
        assert!(decode(b"PK\x03\x04garbage").is_err());
        // A metadata path that climbs out is refused.
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("saves/x", options).unwrap();
        zip.write_all(b"1").unwrap();
        zip.start_file("metadata/x.json", options).unwrap();
        zip.write_all(br#"{"path":"/../etc/x","isDir":false}"#).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(decode(&bytes).is_err());
    }

    #[test]
    fn seed_has_every_parent_folder_first() {
        let seed = seed_records(&[file("/RecordStore/a.db", 9.0, b"a"), file("/x/y/z.bin", 3.0, b"z")]);
        let paths: Vec<&str> = seed.iter().map(|r| r.pathname.as_str()).collect();
        assert_eq!(paths, vec!["/", "/RecordStore", "/x", "/x/y", "/RecordStore/a.db", "/x/y/z.bin"]);
        assert_eq!(seed[0].parent_dir, None);
        assert_eq!(seed[1].parent_dir.as_deref(), Some("/"));
        assert_eq!(seed[3].parent_dir.as_deref(), Some("/x"));
        assert!(seed[1].is_dir && !seed[4].is_dir);
        assert_eq!(seed[4].parent_dir.as_deref(), Some("/RecordStore"));
        assert_eq!(seed[1].mtime, 9.0);
        assert_eq!(seed[4].data, b"a");
    }
}
