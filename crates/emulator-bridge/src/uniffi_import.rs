//! The Swift-facing half of `import`: system detection, archives, save formats, WebDAV listings
//! and the Wi-Fi transfer protocol.
//!
//! Free functions rather than `ContinuumEngine` methods, because none of this touches a core or
//! the engine lock: `CoreCatalog.systemResolver` calls detection from a static closure, and the
//! Wi-Fi server parses request heads on its own queue.

use std::collections::HashMap;
use std::path::Path;

use crate::import::{archive, detect, http, saves, webdav};
use crate::uniffi_api::EngineError;

fn other(reason: String) -> EngineError {
    EngineError::Other { reason }
}

/// What the engine concluded about one file's system.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SystemDetection {
    /// A shared system id, or empty when unknown.
    pub system: String,
    /// 0 to 100.
    pub confidence: u8,
    /// True when the app may route on `system` without asking.
    pub sure: bool,
    /// One plain sentence.
    pub reason: String,
    /// Systems to offer in the picker, most likely first.
    pub candidates: Vec<String>,
    /// Copy the archive as is (arcade, DOS, Amiga) rather than unpacking it.
    pub keep_archive: bool,
}

impl From<detect::Detection> for SystemDetection {
    fn from(d: detect::Detection) -> Self {
        Self {
            sure: d.is_sure(),
            system: d.system,
            confidence: d.confidence,
            reason: d.reason,
            candidates: d.candidates,
            keep_archive: d.keep_archive,
        }
    }
}

/// Names the system of the file at `path` by looking inside it. Reads siblings (a cue's tracks,
/// a .ccd's .img) from the same folder.
#[uniffi::export]
pub fn import_detect_system(path: String) -> SystemDetection {
    detect::detect_path(Path::new(&path)).into()
}

/// The system an extension names on its own, or empty when it is shared or unknown.
#[uniffi::export]
pub fn import_system_for_extension(ext: String) -> String {
    detect::system_for_extension(&ext).unwrap_or("").to_string()
}

/// The files a .cue, .m3u, .gdi or .ccd names, as written.
#[uniffi::export]
pub fn import_referenced_files(path: String) -> Vec<String> {
    detect::referenced_files(Path::new(&path))
}

/// The entries in a .zip or .7z.
#[uniffi::export]
pub fn import_archive_list(path: String) -> Result<Vec<String>, EngineError> {
    archive::list(Path::new(&path)).map_err(other)
}

/// Unpacks a .zip or .7z flat into `dest_dir`, keeping filenames. Returns the names written.
#[uniffi::export]
pub fn import_archive_extract(path: String, dest_dir: String) -> Result<Vec<String>, EngineError> {
    archive::extract_flat(Path::new(&path), Path::new(&dest_dir)).map_err(other)
}

/// Unpacks keeping folders (save folder zips), refusing entries that would escape `dest_dir`.
#[uniffi::export]
pub fn import_archive_extract_tree(path: String, dest_dir: String) -> Result<Vec<String>, EngineError> {
    archive::extract_tree(Path::new(&path), Path::new(&dest_dir)).map_err(other)
}

/// Zips the contents of `dir` into `out_path`. Returns how many files went in.
#[uniffi::export]
pub fn import_zip_directory(dir: String, out_path: String) -> Result<u32, EngineError> {
    archive::zip_directory(Path::new(&dir), Path::new(&out_path)).map_err(other)
}

/// Where a save goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SaveFileKind {
    /// The frontend battery save, through SaveStates.importBatterySave (keeps the .bak).
    Battery,
    /// A core-owned file at `relative_path` under the save directory.
    CoreFile,
    /// A core-owned folder at `relative_path`, exchanged as a zip.
    Folder,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SaveFileLocation {
    pub kind: SaveFileKind,
    pub relative_path: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SaveFileImport {
    pub kind: SaveFileKind,
    pub relative_path: String,
    pub data: Vec<u8>,
    pub note: String,
}

fn split(location: saves::SaveLocation) -> (SaveFileKind, String) {
    match location {
        saves::SaveLocation::Battery => (SaveFileKind::Battery, String::new()),
        saves::SaveLocation::CoreFile(p) => (SaveFileKind::CoreFile, p),
        saves::SaveLocation::Folder(p) => (SaveFileKind::Folder, p),
    }
}

/// Where `system` keeps a game's save.
#[uniffi::export]
pub fn save_file_location(system: String, game_stem: String) -> SaveFileLocation {
    let (kind, relative_path) = split(saves::location(&system, &game_stem));
    SaveFileLocation { kind, relative_path }
}

/// Converts an imported save file (.dsv, .mcr, .gme, .eep, .sra, ...) for `system`. `current` is
/// the stored battery save, or empty.
#[uniffi::export]
pub fn save_file_import(
    system: String,
    file_name: String,
    data: Vec<u8>,
    current: Vec<u8>,
    game_stem: String,
) -> Result<SaveFileImport, EngineError> {
    let got = saves::import(&system, &file_name, &data, &current, &game_stem).map_err(other)?;
    let (kind, relative_path) = split(got.location);
    Ok(SaveFileImport { kind, relative_path, data: got.data, note: got.note })
}

/// Converts the stored save to `format` (an extension) for export.
#[uniffi::export]
pub fn save_file_export(system: String, format: String, stored: Vec<u8>) -> Result<Vec<u8>, EngineError> {
    saves::export(&system, &format, &stored).map_err(other)
}

/// The export formats for a system, the default first.
#[uniffi::export]
pub fn save_file_formats(system: String) -> Vec<String> {
    saves::export_formats(&system).into_iter().map(String::from).collect()
}

/// Every extension the save import accepts.
#[uniffi::export]
pub fn save_file_extensions() -> Vec<String> {
    saves::SAVE_EXTENSIONS.iter().map(|s| s.to_string()).collect()
}

/// One entry of a WebDAV folder.
#[derive(Debug, Clone, uniffi::Record)]
pub struct WebDavEntry {
    pub path: String,
    pub name: String,
    pub is_folder: bool,
    pub size: u64,
    pub modified: String,
}

/// The XML body of a Depth: 1 PROPFIND.
#[uniffi::export]
pub fn webdav_propfind_body() -> String {
    webdav::PROPFIND_BODY.to_string()
}

/// Parses a 207 Multi-Status body; the asked-for folder itself is left out.
#[uniffi::export]
pub fn webdav_parse_listing(xml: String, request_path: String) -> Vec<WebDavEntry> {
    webdav::parse_multistatus(&xml, &request_path)
        .into_iter()
        .map(|e| WebDavEntry { path: e.path, name: e.name, is_folder: e.is_folder, size: e.size, modified: e.modified })
        .collect()
}

/// A parsed HTTP request head from the Wi-Fi transfer server.
#[derive(Debug, Clone, uniffi::Record)]
pub struct WifiRequestHead {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    /// Header names lowercased.
    pub headers: HashMap<String, String>,
    pub content_length: u64,
    /// Bytes of the buffer the head used; the body starts there.
    pub head_len: u32,
}

/// The upload page served at `/`.
#[uniffi::export]
pub fn wifi_page_html() -> String {
    http::PAGE.to_string()
}

/// Parses the head at the start of `data`. None means more bytes are needed.
#[uniffi::export]
pub fn wifi_parse_request_head(data: Vec<u8>) -> Result<Option<WifiRequestHead>, EngineError> {
    let head = http::parse_head(&data).map_err(other)?;
    Ok(head.map(|h| WifiRequestHead {
        method: h.method,
        path: h.path,
        query: h.query.into_iter().collect(),
        headers: h.headers.into_iter().collect(),
        content_length: h.content_length,
        head_len: h.head_len as u32,
    }))
}

/// The largest head the server reads before refusing.
#[uniffi::export]
pub fn wifi_max_head() -> u32 {
    http::MAX_HEAD as u32
}

/// A response head. The body (of `length` bytes) follows it.
#[uniffi::export]
pub fn wifi_response_head(status: u16, content_type: String, length: u64) -> Vec<u8> {
    http::response_head(status, &content_type, length, &[])
}

/// The bare filename an upload may be stored as, or None.
#[uniffi::export]
pub fn wifi_safe_file_name(name: String) -> Option<String> {
    http::safe_file_name(&name)
}

/// Escapes text for a JSON string literal (without the quotes).
#[uniffi::export]
pub fn wifi_json_escape(text: String) -> String {
    http::json_escape(&text)
}
