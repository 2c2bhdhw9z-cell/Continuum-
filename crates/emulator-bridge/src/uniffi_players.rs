//! The Swift-facing half of `players` (Flash and J2ME in the bundled player view): key tables,
//! per-game remaps, J2ME screen sizes and the two save formats.
//!
//! Free functions, like `uniffi_import`: none of this touches a core, a session or the engine
//! lock, because these players never run inside the engine.

use crate::players::{flash_save, j2me_save, jar, keys};
use crate::uniffi_api::EngineError;

fn other(reason: String) -> EngineError {
    EngineError::Other { reason }
}

fn player(system: &str) -> Result<keys::Player, EngineError> {
    keys::Player::from_system(system)
        .ok_or_else(|| other(format!("'{system}' is not a bundled player system (flash or j2me)")))
}

/// One key a pad button can be set to send.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PlayerKeyChoice {
    pub token: String,
    pub label: String,
}

/// What one pad button sends. `slot` is the W3C standard-gamepad index (`PadSlot.rawValue`).
#[derive(Debug, Clone, uniffi::Record)]
pub struct PlayerBinding {
    pub slot: u8,
    /// A key token, or "none".
    pub token: String,
}

/// A keyboard event's fields for a Flash key.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FlashKeyEvent {
    pub code: String,
    pub key: String,
    pub key_code: u32,
}

/// One item of Ruffle's storage.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FlashSaveItem {
    pub key: String,
    pub value: String,
}

/// A `.json` save read for a movie, keys re-rooted onto this player page.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FlashSaveImport {
    pub items: Vec<FlashSaveItem>,
    /// How many keys were moved from another host or file name.
    pub rehosted: u32,
}

/// One file of a J2ME phone's storage.
#[derive(Debug, Clone, uniffi::Record)]
pub struct J2meFile {
    pub path: String,
    pub mtime: f64,
    pub data: Vec<u8>,
}

/// One record to seed J2meJS's storage with before the MIDlet starts.
#[derive(Debug, Clone, uniffi::Record)]
pub struct J2meSeedRecord {
    pub pathname: String,
    pub is_dir: bool,
    pub parent_dir: Option<String>,
    pub mtime: f64,
    pub data: Vec<u8>,
}

/// A J2ME screen size.
#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct J2meScreenSize {
    pub width: u32,
    pub height: u32,
}

/// Every key a pad button can send on `system` ("flash" or "j2me"), in display order.
#[uniffi::export]
pub fn player_key_choices(system: String) -> Vec<PlayerKeyChoice> {
    match keys::Player::from_system(&system) {
        Some(keys::Player::Flash) => keys::flash_keys()
            .into_iter()
            .map(|k| PlayerKeyChoice { token: k.code.into(), label: k.label.into() })
            .collect(),
        Some(keys::Player::J2me) => keys::J2ME_KEYS
            .iter()
            .map(|k| PlayerKeyChoice { token: k.token.into(), label: k.label.into() })
            .collect(),
        None => Vec::new(),
    }
}

/// The full table for a game: defaults with the stored `overrides` on top, all sixteen slots.
#[uniffi::export]
pub fn player_bindings(system: String, overrides: String) -> Result<Vec<PlayerBinding>, EngineError> {
    let table = keys::resolve(player(&system)?, &overrides).map_err(other)?;
    Ok(table.into_iter().map(|(slot, token)| PlayerBinding { slot, token }).collect())
}

/// The string to store for a full table: only what differs from the defaults.
#[uniffi::export]
pub fn player_binding_overrides(system: String, bindings: Vec<PlayerBinding>) -> Result<String, EngineError> {
    let table: Vec<(u8, String)> = bindings.into_iter().map(|b| (b.slot, b.token)).collect();
    keys::encode_overrides(player(&system)?, &table).map_err(other)
}

/// The keyboard event a Flash key token sends, or nil.
#[uniffi::export]
pub fn player_flash_key(token: String) -> Option<FlashKeyEvent> {
    keys::flash_key(&token).map(|k| FlashKeyEvent {
        code: k.code.into(),
        key: k.key.into(),
        key_code: k.key_code,
    })
}

/// The MIDP key code a phone key token sends on `phone_type` ("nokia" or "standard"), or nil.
#[uniffi::export]
pub fn player_j2me_key_code(token: String, phone_type: String) -> Option<i32> {
    keys::j2me_key_code(&token, &phone_type)
}

/// The phone types the J2ME settings offer, default first.
#[uniffi::export]
pub fn player_j2me_phone_types() -> Vec<String> {
    keys::PHONE_TYPES.iter().map(|s| s.to_string()).collect()
}

/// The screen sizes the J2ME settings offer.
#[uniffi::export]
pub fn player_j2me_screen_sizes() -> Vec<String> {
    keys::J2ME_SCREEN_SIZES.iter().map(|s| s.to_string()).collect()
}

/// "240x320" as a size, or nil when it is not one (each side 64 to 1024).
#[uniffi::export]
pub fn player_j2me_parse_screen_size(text: String) -> Option<J2meScreenSize> {
    keys::parse_screen_size(&text).map(|(width, height)| J2meScreenSize { width, height })
}

/// The size a J2ME game starts at.
#[uniffi::export]
pub fn player_j2me_default_screen_size() -> J2meScreenSize {
    let (width, height) = keys::J2ME_DEFAULT_SCREEN;
    J2meScreenSize { width, height }
}

/// Reads a Manic `.json` Flash save for the movie `swf_file_name`.
#[uniffi::export]
pub fn player_flash_save_decode(text: String, swf_file_name: String) -> Result<FlashSaveImport, EngineError> {
    let items = flash_save::decode(&text).map_err(other)?;
    let (items, rehosted) = flash_save::rehost(items, &swf_file_name);
    Ok(FlashSaveImport {
        items: items.into_iter().map(|(key, value)| FlashSaveItem { key, value }).collect(),
        rehosted: rehosted as u32,
    })
}

/// Writes Ruffle's storage as a Manic `.json` Flash save.
#[uniffi::export]
pub fn player_flash_save_encode(items: Vec<FlashSaveItem>) -> String {
    let items: Vec<(String, String)> = items.into_iter().map(|i| (i.key, i.value)).collect();
    flash_save::encode(&items)
}

fn to_files(files: Vec<J2meFile>) -> Vec<j2me_save::PlayerFile> {
    files
        .into_iter()
        .map(|f| j2me_save::PlayerFile { path: f.path, mtime: f.mtime, data: f.data })
        .collect()
}

/// Builds a Manic `.J2meJS.srm` from the phone's files. `export_date` is ISO 8601.
#[uniffi::export]
pub fn player_j2me_save_encode(
    files: Vec<J2meFile>,
    game_name: String,
    export_date: String,
) -> Result<Vec<u8>, EngineError> {
    j2me_save::encode(&to_files(files), &game_name, &export_date).map_err(other)
}

/// Reads a Manic `.J2meJS.srm` back into the phone's files.
#[uniffi::export]
pub fn player_j2me_save_decode(data: Vec<u8>) -> Result<Vec<J2meFile>, EngineError> {
    let files = j2me_save::decode(&data).map_err(other)?;
    Ok(files
        .into_iter()
        .map(|f| J2meFile { path: f.path, mtime: f.mtime, data: f.data })
        .collect())
}

/// The records to seed J2meJS's storage with for these files, folders first.
#[uniffi::export]
pub fn player_j2me_seed(files: Vec<J2meFile>) -> Vec<J2meSeedRecord> {
    j2me_save::seed_records(&to_files(files))
        .into_iter()
        .map(|r| J2meSeedRecord {
            pathname: r.pathname,
            is_dir: r.is_dir,
            parent_dir: r.parent_dir,
            mtime: r.mtime,
            data: r.data,
        })
        .collect()
}

/// What a J2ME `.jar` declares about itself.
#[derive(Debug, Clone, uniffi::Record)]
pub struct J2meManifest {
    /// The MIDlet class the engine starts.
    pub midlet_class: String,
    pub name: String,
    pub vendor: String,
    /// The screen the game was made for, when the manifest or file name says.
    pub screen: Option<J2meScreenSize>,
}

/// Reads a `.jar`'s manifest. Refuses a jar that is not a MIDlet, with the reason.
#[uniffi::export]
pub fn player_j2me_manifest(path: String) -> Result<J2meManifest, EngineError> {
    let m = jar::read(std::path::Path::new(&path)).map_err(other)?;
    Ok(J2meManifest {
        midlet_class: m.midlet_class,
        name: m.name,
        vendor: m.vendor,
        screen: m.screen.map(|(width, height)| J2meScreenSize { width, height }),
    })
}
