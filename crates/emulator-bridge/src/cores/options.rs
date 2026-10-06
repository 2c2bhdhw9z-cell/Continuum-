//! Core settings: what a libretro core declares about itself, what the user chose, and what the
//! engine answers when the core asks.
//!
//! ## The policy, and why it changed
//!
//! The engine used to refuse every `GET_VARIABLE`, so each core ran on its C initialisers. That was
//! reversed by the owner (docs/MANIC_PARITY.md, "Core settings are answered now"): the engine now
//! answers with the user's choice, else the core's own declared default. Two kinds of exception
//! survive from the old override table, and both are here as [`HostRule`]s rather than scattered:
//!
//! - **Host defaults.** melonDS advertises `melonds_touch_mode = "Mouse"` and
//!   `melonds_boot_directly = "enabled"`, but its C initialisers are `Disabled` and `0`. The engine
//!   answers `Touch` and `enabled` unless the user picks something else. These are DEFAULTS: the
//!   settings screen shows them as the default and the user may change them.
//! - **Locked keys.** The N64 renderer, RSP and CPU, and the PSP CPU, where the only values that work
//!   on a phone without JIT are the ones the engine names (see the notes on `host_rules`). These are
//!   hidden from the settings list and always answered (or refused) the same way, because picking
//!   any other value freezes or crashes the app.
//!
//! ## Shape
//!
//! Everything a core hands over is copied out of its memory at once (libretro.h: "the frontend
//! must maintain its own copy"), into [`OptionTable`]. Answers go back as `CString`s owned by
//! [`State`], which outlive the pointer the core holds until the next core load.
//!
//! The C structs are declared here, not in `native_core.rs`, so the parsers and their tests run in
//! the plain host test suite with fake tables in all three formats, with no dylib in sight.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

// ------------------------------------------------------------------ C layouts

/// `struct retro_variable` (libretro.h:7108).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RetroVariable {
    pub key: *const c_char,
    pub value: *const c_char,
}

/// `struct retro_core_option_display` (libretro.h:7152).
#[repr(C)]
pub struct RetroCoreOptionDisplay {
    pub key: *const c_char,
    pub visible: bool,
}

/// `RETRO_NUM_CORE_OPTION_VALUES_MAX` (libretro.h:7198).
pub const NUM_CORE_OPTION_VALUES_MAX: usize = 128;

/// `struct retro_core_option_value` (libretro.h:7209).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RetroCoreOptionValue {
    pub value: *const c_char,
    pub label: *const c_char,
}

/// `struct retro_core_option_definition` (libretro.h:7265), the v1 table entry.
#[repr(C)]
pub struct RetroCoreOptionDefinition {
    pub key: *const c_char,
    pub desc: *const c_char,
    pub info: *const c_char,
    pub values: [RetroCoreOptionValue; NUM_CORE_OPTION_VALUES_MAX],
    pub default_value: *const c_char,
}

/// `struct retro_core_options_intl` (libretro.h:7299).
#[repr(C)]
pub struct RetroCoreOptionsIntl {
    pub us: *const RetroCoreOptionDefinition,
    pub local: *const RetroCoreOptionDefinition,
}

/// `struct retro_core_option_v2_category` (libretro.h:7325).
#[repr(C)]
pub struct RetroCoreOptionV2Category {
    pub key: *const c_char,
    pub desc: *const c_char,
    pub info: *const c_char,
}

/// `struct retro_core_option_v2_definition` (libretro.h:7372).
#[repr(C)]
pub struct RetroCoreOptionV2Definition {
    pub key: *const c_char,
    pub desc: *const c_char,
    pub desc_categorized: *const c_char,
    pub info: *const c_char,
    pub info_categorized: *const c_char,
    pub category_key: *const c_char,
    pub values: [RetroCoreOptionValue; NUM_CORE_OPTION_VALUES_MAX],
    pub default_value: *const c_char,
}

/// `struct retro_core_options_v2` (libretro.h:7497).
#[repr(C)]
pub struct RetroCoreOptionsV2 {
    pub categories: *const RetroCoreOptionV2Category,
    pub definitions: *const RetroCoreOptionV2Definition,
}

/// `struct retro_core_options_v2_intl` (libretro.h:7528).
#[repr(C)]
pub struct RetroCoreOptionsV2Intl {
    pub us: *const RetroCoreOptionsV2,
    pub local: *const RetroCoreOptionsV2,
}

/// `retro_core_options_update_display_callback_t` (libretro.h:7585).
pub type UpdateDisplayCallback = unsafe extern "C" fn() -> bool;

/// `struct retro_core_options_update_display_callback` (libretro.h:7594).
#[repr(C)]
pub struct RetroCoreOptionsUpdateDisplayCallback {
    pub callback: Option<UpdateDisplayCallback>,
}

/// Entries read from any one core table before giving up on a missing terminator. The largest
/// table in the app (Beetle PSX HW) declares about 120; a missing NULL must not walk off into
/// memory.
const MAX_DEFINITIONS: usize = 1024;

// ------------------------------------------------------------------ the model

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionValue {
    pub value: String,
    /// What to show. The value itself when the core gave no label.
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionDef {
    pub key: String,
    pub label: String,
    pub info: String,
    /// Category key, empty for none.
    pub category: String,
    pub values: Vec<OptionValue>,
    /// The core's own default. For a v0 table this is the first value, as libretro.h:1003 says.
    pub default: String,
}

impl OptionDef {
    fn accepts(&self, value: &str) -> bool {
        // An empty list is a runtime-populated option (mGBA's palettes), so anything is let through.
        self.values.is_empty() || self.values.iter().any(|v| v.value == value)
    }

    /// The core said in its own words that a change only lands on the next start, in the label or
    /// in the description. Azahar says it only in the description ("System Model" ... "Restart
    /// required."), and reading only the label is how build 126 lost the restart button for it.
    pub fn says_restart(&self) -> bool {
        let text = format!("{} {}", self.label, self.info).to_ascii_lowercase();
        const NOT: [&str; 6] = [
            "no restart",
            "without restart",
            "without a restart",
            "not require a restart",
            "n't require a restart",
            "n't need a restart",
        ];
        if NOT.iter().any(|phrase| text.contains(phrase)) {
            return false;
        }
        text.contains("restart") || text.contains("(reload")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionCategory {
    pub key: String,
    pub label: String,
    pub info: String,
}

/// Everything one core declared, in the order it declared it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OptionTable {
    /// 0 for `SET_VARIABLES`, 1 for `SET_CORE_OPTIONS`, 2 for `SET_CORE_OPTIONS_V2`.
    pub version: u8,
    pub categories: Vec<OptionCategory>,
    pub defs: Vec<OptionDef>,
}

impl OptionTable {
    pub fn get(&self, key: &str) -> Option<&OptionDef> {
        self.defs.iter().find(|d| d.key == key)
    }
}

/// How the engine treats one key regardless of the core's table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostRule {
    /// Answered with this unless the user picked otherwise. Shown as the default.
    Default(&'static str),
    /// Always answered with this. Hidden from the list; the user cannot change it.
    Locked(&'static str),
    /// Always refused, so the core's own fallback runs. Hidden from the list.
    LockedRefused,
}

/// The engine's own rules per core. The history behind each lives in SESSION_HANDOFF and in the
/// git log of `native_core.rs`, where this table used to be the whole of the option support.
pub fn host_rules(core_id: &str) -> &'static [(&'static str, HostRule)] {
    match core_id {
        // melonDS: both C initialisers differ from the advertised default. `Disabled` switches
        // the touch screen off inside the core; `DirectBoot = 0` boots a firmware menu that a
        // generated firmware cannot leave. The engine's values are the defaults the user sees.
        "melonds" => &[
            ("melonds_touch_mode", HostRule::Default("Touch")),
            ("melonds_boot_directly", HostRule::Default("enabled")),
        ],
        // parallel_n64: answering the renderer key with anything, even "angrylion", leaves
        // `gfx_plugin` at GFX_GLIDE64 in a GL-less build and hangs; refusing lets the core's
        // autoselect pick angrylion. Multithreaded angrylion hangs under the display-link tick,
        // and the dynarec CPU and the parallel RSP need executable memory this app does not have.
        "parallel_n64" => &[
            ("parallel-n64-gfxplugin", HostRule::LockedRefused),
            ("parallel-n64-rspplugin", HostRule::Locked("hle")),
            ("parallel-n64-angrylion-multithread", HostRule::Locked("off")),
            ("parallel-n64-cpucore", HostRule::Locked("cached_interpreter")),
        ],
        // PPSSPP: "IR JIT" is the IR interpreter with compile-to-native off. "JIT" is the dynarec.
        // Refusing does not give the advertised default either: the core sets the slow interpreter
        // before the read. Locked so nobody can pick the dynarec.
        "ppsspp" => &[("ppsspp_cpu_core", HostRule::Locked("IR JIT"))],
        // Beetle PSX HW: refusing `beetle_psx_hw_renderer` left `hw_renderer = false` (libretro.c),
        // so every PlayStation frame a phone has shown on this core came from the SOFTWARE
        // renderer. The advertised default is "hardware", whose Vulkan hand-over has not been seen
        // on a phone. The engine keeps software as the default; the user may pick hardware.
        "mednafen_psx_hw" => &[("beetle_psx_hw_renderer", HostRule::Default("software"))],
        _ => &[],
    }
}

fn host_rule(core_id: &str, key: &str) -> Option<HostRule> {
    host_rules(core_id)
        .iter()
        .find(|(candidate, _)| *candidate == key)
        .map(|(_, rule)| *rule)
}

/// What the engine answers for `key` right now. `None` means "refuse".
///
/// Order: a lock, then a value the core forced with `SET_VARIABLE`, then this game's choice, then
/// the core-wide choice, then the engine's default, then the core's own default. A stored choice
/// the core no longer offers is skipped rather than handed over.
pub fn resolve(
    core_id: &str,
    table: Option<&OptionTable>,
    forced: &BTreeMap<String, String>,
    game: &BTreeMap<String, String>,
    core: &BTreeMap<String, String>,
    key: &str,
) -> Option<String> {
    let rule = host_rule(core_id, key);
    match rule {
        Some(HostRule::Locked(value)) => return Some(value.to_owned()),
        Some(HostRule::LockedRefused) => return None,
        _ => {}
    }
    let def = table.and_then(|t| t.get(key));
    let valid = |value: &String| def.is_none_or(|d| d.accepts(value));
    for layer in [forced, game, core] {
        if let Some(value) = layer.get(key).filter(|v| valid(v)) {
            return Some(value.clone());
        }
    }
    if let Some(HostRule::Default(value)) = rule {
        return Some(value.to_owned());
    }
    def.map(|d| d.default.clone()).filter(|v| !v.is_empty())
}

/// The default the settings screen shows: the engine's when it has one, else the core's.
pub fn shown_default(core_id: &str, def: &OptionDef) -> String {
    match host_rule(core_id, &def.key) {
        Some(HostRule::Default(value)) | Some(HostRule::Locked(value)) => value.to_owned(),
        _ => def.default.clone(),
    }
}

/// Whether the key is hidden from the user entirely.
pub fn is_locked(core_id: &str, key: &str) -> bool {
    matches!(
        host_rule(core_id, key),
        Some(HostRule::Locked(_)) | Some(HostRule::LockedRefused)
    )
}

// ------------------------------------------------------------------ parsing

/// Copies a C string, `None` for null.
///
/// # Safety
/// `ptr` is null or a NUL-terminated string.
unsafe fn opt_str(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned())
    }
}

unsafe fn str_or_empty(ptr: *const c_char) -> String {
    unsafe { opt_str(ptr) }.unwrap_or_default()
}

/// Parses one v0 value string, `"Description; a|b|c"`. The first value is the default.
pub fn parse_v0_value(key: &str, text: &str) -> OptionDef {
    let (label, list) = match text.split_once(';') {
        Some((label, list)) => (label.trim().to_owned(), list.trim_start()),
        None => (text.trim().to_owned(), ""),
    };
    let values: Vec<OptionValue> = list
        .split('|')
        .filter(|v| !v.is_empty())
        .map(|v| OptionValue {
            value: v.to_owned(),
            label: v.to_owned(),
        })
        .collect();
    OptionDef {
        key: key.to_owned(),
        label: if label.is_empty() { key.to_owned() } else { label },
        info: String::new(),
        category: String::new(),
        default: values.first().map(|v| v.value.clone()).unwrap_or_default(),
        values,
    }
}

/// `SET_VARIABLES` (libretro.h:1020): an array of `retro_variable` ending at a NULL key.
///
/// # Safety
/// `vars` is null or points at such an array.
pub unsafe fn parse_v0(vars: *const RetroVariable) -> OptionTable {
    let mut table = OptionTable::default();
    if vars.is_null() {
        return table;
    }
    for index in 0..MAX_DEFINITIONS {
        let var = unsafe { &*vars.add(index) };
        if var.key.is_null() {
            break;
        }
        let key = unsafe { str_or_empty(var.key) };
        let text = unsafe { str_or_empty(var.value) };
        table.defs.push(parse_v0_value(&key, &text));
    }
    table
}

unsafe fn parse_values(values: &[RetroCoreOptionValue; NUM_CORE_OPTION_VALUES_MAX]) -> Vec<OptionValue> {
    let mut out = Vec::new();
    for entry in values.iter() {
        if entry.value.is_null() {
            break;
        }
        let value = unsafe { str_or_empty(entry.value) };
        let label = unsafe { opt_str(entry.label) }
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| value.clone());
        out.push(OptionValue { value, label });
    }
    out
}

fn default_or_first(default: Option<String>, values: &[OptionValue]) -> String {
    // libretro.h:7280: a NULL or unmatched default means the first value.
    match default {
        Some(d) if values.is_empty() || values.iter().any(|v| v.value == d) => d,
        _ => values.first().map(|v| v.value.clone()).unwrap_or_default(),
    }
}

/// `SET_CORE_OPTIONS` (libretro.h:1928): `retro_core_option_definition` entries to a NULL key.
///
/// # Safety
/// `defs` is null or points at such an array.
pub unsafe fn parse_v1(defs: *const RetroCoreOptionDefinition) -> OptionTable {
    let mut table = OptionTable {
        version: 1,
        ..Default::default()
    };
    if defs.is_null() {
        return table;
    }
    for index in 0..MAX_DEFINITIONS {
        let def = unsafe { &*defs.add(index) };
        if def.key.is_null() {
            break;
        }
        let values = unsafe { parse_values(&def.values) };
        let key = unsafe { str_or_empty(def.key) };
        table.defs.push(OptionDef {
            label: unsafe { opt_str(def.desc) }.unwrap_or_else(|| key.clone()),
            key,
            info: unsafe { str_or_empty(def.info) },
            category: String::new(),
            default: default_or_first(unsafe { opt_str(def.default_value) }, &values),
            values,
        });
    }
    table
}

/// `SET_CORE_OPTIONS_INTL` (libretro.h:1951). Only the US table is read.
///
/// # Safety
/// `intl` is null or a valid `retro_core_options_intl`.
pub unsafe fn parse_v1_intl(intl: *const RetroCoreOptionsIntl) -> OptionTable {
    if intl.is_null() {
        return OptionTable {
            version: 1,
            ..Default::default()
        };
    }
    unsafe { parse_v1((*intl).us) }
}

/// `SET_CORE_OPTIONS_V2` (libretro.h:2345).
///
/// # Safety
/// `options` is null or a valid `retro_core_options_v2`.
pub unsafe fn parse_v2(options: *const RetroCoreOptionsV2) -> OptionTable {
    let mut table = OptionTable {
        version: 2,
        ..Default::default()
    };
    if options.is_null() {
        return table;
    }
    let options = unsafe { &*options };
    if !options.categories.is_null() {
        for index in 0..MAX_DEFINITIONS {
            let category = unsafe { &*options.categories.add(index) };
            if category.key.is_null() {
                break;
            }
            let key = unsafe { str_or_empty(category.key) };
            table.categories.push(OptionCategory {
                label: unsafe { opt_str(category.desc) }.unwrap_or_else(|| key.clone()),
                key,
                info: unsafe { str_or_empty(category.info) },
            });
        }
    }
    if options.definitions.is_null() {
        return table;
    }
    for index in 0..MAX_DEFINITIONS {
        let def = unsafe { &*options.definitions.add(index) };
        if def.key.is_null() {
            break;
        }
        let key = unsafe { str_or_empty(def.key) };
        let category = unsafe { str_or_empty(def.category_key) };
        // A category the core never declared is treated as none, as RetroArch does.
        let in_category = !category.is_empty() && table.categories.iter().any(|c| c.key == category);
        // Inside a category the screen is grouped, so the shorter categorised wording is the one
        // written for it (libretro.h:7390).
        let label = if in_category {
            unsafe { opt_str(def.desc_categorized) }.filter(|s| !s.is_empty())
        } else {
            None
        }
        .or_else(|| unsafe { opt_str(def.desc) })
        .unwrap_or_else(|| key.clone());
        let info = if in_category {
            unsafe { opt_str(def.info_categorized) }.filter(|s| !s.is_empty())
        } else {
            None
        }
        .or_else(|| unsafe { opt_str(def.info) })
        .unwrap_or_default();
        let values = unsafe { parse_values(&def.values) };
        table.defs.push(OptionDef {
            key,
            label,
            info,
            category: if in_category { category } else { String::new() },
            default: default_or_first(unsafe { opt_str(def.default_value) }, &values),
            values,
        });
    }
    table
}

/// `SET_CORE_OPTIONS_V2_INTL` (libretro.h:2362). Only the US table is read.
///
/// # Safety
/// `intl` is null or a valid `retro_core_options_v2_intl`.
pub unsafe fn parse_v2_intl(intl: *const RetroCoreOptionsV2Intl) -> OptionTable {
    if intl.is_null() {
        return OptionTable {
            version: 2,
            ..Default::default()
        };
    }
    unsafe { parse_v2((*intl).us) }
}

// ------------------------------------------------------------------ persistence

/// RetroArch's `.opt` format: one `key = "value"` per line. Kept so a user who knows RetroArch can
/// read the file, and because it needs no parser crate.
pub fn write_opt(values: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    for (key, value) in values {
        out.push_str(key);
        out.push_str(" = \"");
        out.push_str(&value.replace('"', "'"));
        out.push_str("\"\n");
    }
    out
}

pub fn read_opt(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(value);
        if !key.is_empty() {
            out.insert(key.to_owned(), value.to_owned());
        }
    }
    out
}

fn escape_field(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn unescape_field(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// A declared table, written to disk so Settings can list a core's options without loading it.
pub fn write_table(table: &OptionTable) -> String {
    let mut out = format!("version\t{}\n", table.version);
    for c in &table.categories {
        out.push_str(&format!(
            "category\t{}\t{}\t{}\n",
            escape_field(&c.key),
            escape_field(&c.label),
            escape_field(&c.info)
        ));
    }
    for d in &table.defs {
        out.push_str(&format!(
            "option\t{}\t{}\t{}\t{}\t{}\n",
            escape_field(&d.key),
            escape_field(&d.label),
            escape_field(&d.info),
            escape_field(&d.category),
            escape_field(&d.default)
        ));
        for v in &d.values {
            out.push_str(&format!(
                "value\t{}\t{}\n",
                escape_field(&v.value),
                escape_field(&v.label)
            ));
        }
    }
    out
}

pub fn read_table(text: &str) -> OptionTable {
    let mut table = OptionTable::default();
    for line in text.lines() {
        let fields: Vec<String> = line.split('\t').map(unescape_field).collect();
        match (fields.first().map(String::as_str), fields.len()) {
            (Some("version"), 2) => table.version = fields[1].parse().unwrap_or(0),
            (Some("category"), 4) => table.categories.push(OptionCategory {
                key: fields[1].clone(),
                label: fields[2].clone(),
                info: fields[3].clone(),
            }),
            (Some("option"), 6) => table.defs.push(OptionDef {
                key: fields[1].clone(),
                label: fields[2].clone(),
                info: fields[3].clone(),
                category: fields[4].clone(),
                default: fields[5].clone(),
                values: Vec::new(),
            }),
            (Some("value"), 3) => {
                if let Some(def) = table.defs.last_mut() {
                    def.values.push(OptionValue {
                        value: fields[1].clone(),
                        label: fields[2].clone(),
                    });
                }
            }
            _ => {}
        }
    }
    table
}

/// A game name safe as a file name: path separators and control characters become `_`.
pub fn game_file_stem(game: &str) -> String {
    let stem: String = game
        .chars()
        .map(|c| if c == '/' || c == '\\' || c == ':' || c.is_control() { '_' } else { c })
        .collect();
    let stem = stem.trim().trim_start_matches('.').to_owned();
    if stem.is_empty() {
        "game".to_owned()
    } else {
        stem
    }
}

fn core_dir(root: &Path, core_id: &str) -> PathBuf {
    root.join(game_file_stem(core_id))
}

fn core_opt_path(root: &Path, core_id: &str) -> PathBuf {
    core_dir(root, core_id).join(format!("{}.opt", game_file_stem(core_id)))
}

fn game_opt_path(root: &Path, core_id: &str, game: &str) -> PathBuf {
    core_dir(root, core_id).join("games").join(format!("{}.opt", game_file_stem(game)))
}

fn table_path(root: &Path, core_id: &str) -> PathBuf {
    core_dir(root, core_id).join("declared-options.txt")
}

fn read_map(path: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .map(|text| read_opt(&text))
        .unwrap_or_default()
}

fn write_map(path: &Path, values: &BTreeMap<String, String>) -> Result<(), String> {
    if values.is_empty() {
        return match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("could not remove {}: {err}", path.display())),
        };
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("could not create {}: {err}", parent.display()))?;
    }
    std::fs::write(path, write_opt(values))
        .map_err(|err| format!("could not write {}: {err}", path.display()))
}

// ------------------------------------------------------------------ live state

/// Per core: its declared table, which keys it hid, and its display callback.
#[derive(Default)]
struct CoreEntry {
    table: OptionTable,
    hidden: HashSet<String>,
    display_callback: Option<UpdateDisplayCallback>,
}

/// The process-wide option state. A global for the reason `DIRECTORIES` is one in
/// `native_core.rs`: the core asks from inside `retro_set_environment` and `retro_load_game`,
/// under the engine lock and possibly on another thread, through a callback with no user data.
#[derive(Default)]
pub struct State {
    /// The core the next environment call belongs to.
    active_core: String,
    /// The game whose overrides apply, as its file name.
    active_game: Option<String>,
    cores: HashMap<String, CoreEntry>,
    /// Where `.opt` files live. `None` until the host says.
    root: Option<PathBuf>,
    core_values: BTreeMap<String, String>,
    game_values: BTreeMap<String, String>,
    /// Values the core forced with `SET_VARIABLE` this session.
    forced: BTreeMap<String, String>,
    /// The `CString` last handed out per key. Kept so the pointer stays valid.
    answered: HashMap<String, CString>,
    /// Strings a changed key used to point at. Never freed during the core's life, because a core
    /// may keep the old pointer until it next asks. Grows only with user changes.
    retired: Vec<CString>,
    update_pending: bool,
    /// Keys changed since the core last read them.
    unread: BTreeSet<String>,
    /// Frames run since the last change. A key still unread a few frames after the core was told
    /// is a key it only reads at start.
    frames_since_change: u32,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = match STATE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    f(guard.get_or_insert_with(State::default))
}

/// Frames after which a changed key the core has not re-read counts as "needs a restart".
pub const RESTART_DETECT_FRAMES: u32 = 3;

/// Where the `.opt` files go. Swift passes a folder under Application Support.
pub fn set_root(root: Option<PathBuf>) {
    with_state(|s| {
        s.root = root;
        if !s.active_core.is_empty() {
            let (core, game) = (s.active_core.clone(), s.active_game.clone());
            load_prefs(s, &core, game.as_deref());
        }
    });
}

pub fn root() -> Option<PathBuf> {
    with_state(|s| s.root.clone())
}

fn load_prefs(s: &mut State, core_id: &str, game: Option<&str>) {
    let (core_values, game_values) = match &s.root {
        Some(root) => (
            read_map(&core_opt_path(root, core_id)),
            game.map(|g| read_map(&game_opt_path(root, core_id, g)))
                .unwrap_or_default(),
        ),
        None => (BTreeMap::new(), BTreeMap::new()),
    };
    s.core_values = core_values;
    s.game_values = game_values;
}

/// Called before `retro_set_environment` (`game: None`) and again right before `retro_load_game`
/// with the game, which is the point where per-game choices take effect.
///
/// `fresh_core` is true when the dylib was just opened: the old table, visibility and callback
/// belong to a previous instance and are dropped.
pub fn install(core_id: &str, game: Option<&str>, fresh_core: bool) {
    with_state(|s| {
        s.active_core = core_id.to_owned();
        s.active_game = game.map(str::to_owned);
        if fresh_core {
            s.cores.insert(core_id.to_owned(), CoreEntry::default());
        }
        s.forced.clear();
        // Old answers stay alive: a core opened earlier and still resident may hold them.
        let old: Vec<CString> = s.answered.drain().map(|(_, v)| v).collect();
        s.retired.extend(old);
        s.update_pending = false;
        s.unread.clear();
        s.frames_since_change = 0;
        load_prefs(s, core_id, game);
    });
    let rules = host_rules(core_id);
    if !rules.is_empty() {
        log::info!(
            "core '{core_id}' has {} engine rule(s): {}",
            rules.len(),
            rules
                .iter()
                .map(|(k, r)| format!("{k}={r:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

/// The core's dylib is going away: its callback must never be called again.
pub fn forget_core(core_id: &str) {
    with_state(|s| {
        if let Some(entry) = s.cores.get_mut(core_id) {
            entry.display_callback = None;
        }
    });
}

/// Records a table the core declared, and caches it on disk.
pub fn declare(table: OptionTable) {
    let (root, core) = with_state(|s| {
        let core = s.active_core.clone();
        let entry = s.cores.entry(core.clone()).or_default();
        entry.table = table.clone();
        entry.hidden.clear();
        (s.root.clone(), core)
    });
    log::info!(
        "core '{core}' declared {} option(s) in {} categor(ies), format v{}",
        table.defs.len(),
        table.categories.len(),
        table.version
    );
    if let Some(root) = root {
        if core.is_empty() || table.defs.is_empty() {
            return;
        }
        let path = table_path(&root, &core);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(err) = std::fs::write(&path, write_table(&table)) {
            log::warn!("could not cache the options of '{core}': {err}");
        }
    }
}

/// `SET_CORE_OPTIONS_DISPLAY`.
pub fn set_visible(key: &str, visible: bool) {
    with_state(|s| {
        let core = s.active_core.clone();
        let entry = s.cores.entry(core).or_default();
        if visible {
            entry.hidden.remove(key);
        } else {
            entry.hidden.insert(key.to_owned());
        }
    });
}

/// `SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK`.
pub fn set_display_callback(callback: Option<UpdateDisplayCallback>) {
    with_state(|s| {
        let core = s.active_core.clone();
        s.cores.entry(core).or_default().display_callback = callback;
    });
}

/// Asks the core to refresh which options it shows. Only for the core that is running a session,
/// and only from under the engine lock, so it cannot overlap `retro_run`. The state lock is NOT
/// held while the core runs, because the core answers by calling `SET_CORE_OPTIONS_DISPLAY`.
pub fn refresh_visibility(core_id: &str) -> bool {
    let callback = with_state(|s| {
        if s.active_core != core_id {
            return None;
        }
        s.cores.get(core_id).and_then(|e| e.display_callback)
    });
    match callback {
        Some(callback) => unsafe { callback() },
        None => false,
    }
}

/// `GET_VARIABLE`. Returns the pointer to hand over, or `None` to refuse.
///
/// A RESTART-REQUIRED OPTION IS HELD for the rest of the session: once the core has been given a
/// value for it, it keeps getting that value until the next load, whatever the user chose since.
/// The core said the change needs a restart, and a core that re-reads everything on an update
/// anyway (Azahar re-parses every option) would otherwise switch the emulated 3DS model under a
/// running game, which crashed the app on build 126. The new choice is stored at once and lands at
/// the next start; `list` reports it as waiting for a restart until then. A value the core forced
/// itself (`SET_VARIABLE`) is never held, because the core expects to read it back.
pub fn answer(key: &str) -> Option<*const c_char> {
    with_state(|s| {
        s.unread.remove(key);
        let core = s.active_core.clone();
        let table = s.cores.get(&core).map(|e| &e.table);
        let held = table
            .and_then(|t| t.get(key))
            .is_some_and(OptionDef::says_restart)
            && !s.forced.contains_key(key);
        if held {
            if let Some(previous) = s.answered.get(key) {
                return Some(previous.as_ptr());
            }
        }
        let value = resolve(&core, table, &s.forced, &s.game_values, &s.core_values, key)?;
        let cstring = CString::new(value).ok()?;
        let reuse = s.answered.get(key).is_some_and(|old| old.as_c_str() == cstring.as_c_str());
        if !reuse {
            if let Some(old) = s.answered.insert(key.to_owned(), cstring) {
                s.retired.push(old);
            }
        }
        s.answered.get(key).map(|c| c.as_ptr())
    })
}

/// `GET_VARIABLE_UPDATE`: true once after a change, as libretro.h:1030 asks.
pub fn take_update() -> bool {
    with_state(|s| std::mem::take(&mut s.update_pending))
}

/// `SET_VARIABLE` (libretro.h:2417): the core forcing one of its own options.
pub fn core_sets(key: &str, value: &str) -> bool {
    with_state(|s| {
        let core = s.active_core.clone();
        let Some(def) = s.cores.get(&core).and_then(|e| e.table.get(key)) else {
            return false;
        };
        if value.is_empty() || !def.accepts(value) {
            return false;
        }
        s.forced.insert(key.to_owned(), value.to_owned());
        s.update_pending = true;
        true
    })
}

/// One frame ran. Feeds the restart detection.
pub fn note_frame() {
    with_state(|s| {
        if !s.unread.is_empty() {
            s.frames_since_change = s.frames_since_change.saturating_add(1);
        }
    });
}

// ------------------------------------------------------------------ the settings screen

/// One option as the settings screen shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionView {
    pub key: String,
    pub label: String,
    pub info: String,
    pub category: String,
    pub values: Vec<OptionValue>,
    pub current: String,
    pub default: String,
    pub visible: bool,
    /// Set for this game rather than for the whole core.
    pub game_override: bool,
    /// The core will not see this change until the game restarts.
    pub needs_restart: bool,
    /// The value the running core was last given, which is what the game is really running with.
    /// The same as `current` unless a change is waiting (for a restart, or for the core to re-read
    /// it). A save state belongs to this value, not to `current`.
    pub in_effect: String,
}

/// The table for a core: live when it declared one this run, else the cached copy on disk.
fn table_for(s: &State, core_id: &str) -> Option<OptionTable> {
    if let Some(entry) = s.cores.get(core_id).filter(|e| !e.table.defs.is_empty()) {
        return Some(entry.table.clone());
    }
    let root = s.root.as_ref()?;
    let text = std::fs::read_to_string(table_path(root, core_id)).ok()?;
    let table = read_table(&text);
    (!table.defs.is_empty()).then_some(table)
}

/// Whether a table for this core is known at all (live or cached).
pub fn has_table(core_id: &str) -> bool {
    with_state(|s| table_for(s, core_id).is_some())
}

pub fn categories(core_id: &str) -> Vec<OptionCategory> {
    with_state(|s| table_for(s, core_id).map(|t| t.categories).unwrap_or_default())
}

/// The game whose overrides are live for this core, if any.
pub fn active_game(core_id: &str) -> Option<String> {
    with_state(|s| (s.active_core == core_id).then(|| s.active_game.clone()).flatten())
}

/// Every option of a core, in declared order, with what the user sees. Locked keys are left out.
pub fn list(core_id: &str) -> Vec<OptionView> {
    with_state(|s| {
        let Some(table) = table_for(s, core_id) else {
            return Vec::new();
        };
        let live = s.active_core == core_id;
        let (core_values, game_values) = if live {
            (s.core_values.clone(), s.game_values.clone())
        } else {
            let core = s
                .root
                .as_ref()
                .map(|r| read_map(&core_opt_path(r, core_id)))
                .unwrap_or_default();
            (core, BTreeMap::new())
        };
        let forced = if live { s.forced.clone() } else { BTreeMap::new() };
        let hidden = s.cores.get(core_id).map(|e| e.hidden.clone()).unwrap_or_default();
        let restart_window = s.frames_since_change >= RESTART_DETECT_FRAMES;
        table
            .defs
            .iter()
            .filter(|d| !is_locked(core_id, &d.key))
            .map(|d| {
                let current = resolve(core_id, Some(&table), &forced, &game_values, &core_values, &d.key)
                    .unwrap_or_else(|| d.default.clone());
                let unread = live && s.unread.contains(&d.key);
                // What the core was last handed this session, if it has asked.
                let given = live
                    .then(|| s.answered.get(&d.key))
                    .flatten()
                    .map(|c| c.to_string_lossy().into_owned());
                // A held option waits for a restart for exactly as long as the user's choice
                // differs from what the core was given. Anything else waits while the core has not
                // re-read it a few frames after being told.
                let needs_restart = match (&given, d.says_restart()) {
                    (Some(given), true) if !forced.contains_key(&d.key) => *given != current,
                    (_, says) => unread && (restart_window || says),
                };
                let in_effect = given.unwrap_or_else(|| current.clone());
                OptionView {
                    key: d.key.clone(),
                    label: d.label.clone(),
                    info: d.info.clone(),
                    category: d.category.clone(),
                    values: d.values.clone(),
                    current,
                    default: shown_default(core_id, d),
                    visible: !hidden.contains(&d.key),
                    game_override: game_values.contains_key(&d.key),
                    needs_restart,
                    in_effect,
                }
            })
            .collect()
    })
}

/// Stores a choice and, for the running core, tells it. `for_game` stores it for the running game
/// only. Returns a plain line for the status bar either way.
pub fn set(core_id: &str, key: &str, value: &str, for_game: bool) -> Result<String, String> {
    with_state(|s| {
        if is_locked(core_id, key) {
            return Err(format!("{key} is fixed by Continuum for this core and cannot be changed"));
        }
        let table = table_for(s, core_id);
        let def = table.as_ref().and_then(|t| t.get(key).cloned());
        if let Some(def) = &def {
            if !def.accepts(value) {
                return Err(format!("{} does not offer \"{value}\" for {}", core_id, def.label));
            }
        } else if table.is_some() {
            return Err(format!("{core_id} has no setting called {key}"));
        }
        let live = s.active_core == core_id;
        let root = s.root.clone();
        let label = def.as_ref().map(|d| d.label.clone()).unwrap_or_else(|| key.to_owned());
        if for_game {
            let game = s
                .active_game
                .clone()
                .filter(|_| live)
                .ok_or_else(|| "a per-game setting needs that game running".to_owned())?;
            s.game_values.insert(key.to_owned(), value.to_owned());
            if let Some(root) = &root {
                write_map(&game_opt_path(root, core_id, &game), &s.game_values)?;
            }
        } else {
            let mut values = if live {
                s.core_values.clone()
            } else {
                root.as_ref().map(|r| read_map(&core_opt_path(r, core_id))).unwrap_or_default()
            };
            values.insert(key.to_owned(), value.to_owned());
            // A core-wide change replaces this game's own choice, or it would seem to do nothing.
            if live {
                s.game_values.remove(key);
                if let (Some(root), Some(game)) = (&root, s.active_game.clone()) {
                    write_map(&game_opt_path(root, core_id, &game), &s.game_values)?;
                }
            }
            if let Some(root) = &root {
                write_map(&core_opt_path(root, core_id), &values)?;
            }
            if live {
                s.core_values = values;
            }
        }
        if live {
            s.forced.remove(key);
            s.update_pending = true;
            s.unread.insert(key.to_owned());
            s.frames_since_change = 0;
        }
        let scope = if for_game { "for this game" } else { "for every game" };
        let saved = if root.is_some() { "" } else { " (not saved: no settings folder yet)" };
        Ok(format!("{label}: {value}, {scope}{saved}"))
    })
}

/// Clears every core-wide choice, and for the running core tells it.
pub fn reset_core(core_id: &str) -> Result<String, String> {
    with_state(|s| {
        if let Some(root) = &s.root {
            write_map(&core_opt_path(root, core_id), &BTreeMap::new())?;
        }
        if s.active_core == core_id {
            let changed: Vec<String> = s.core_values.keys().cloned().collect();
            s.core_values.clear();
            mark_changed(s, changed);
        }
        Ok(format!("{core_id}: every setting back to its default"))
    })
}

/// Clears the running game's own choices.
pub fn reset_game(core_id: &str) -> Result<String, String> {
    with_state(|s| {
        let game = s
            .active_game
            .clone()
            .filter(|_| s.active_core == core_id)
            .ok_or_else(|| "no game of that core is running".to_owned())?;
        if let Some(root) = &s.root {
            write_map(&game_opt_path(root, core_id, &game), &BTreeMap::new())?;
        }
        let changed: Vec<String> = s.game_values.keys().cloned().collect();
        s.game_values.clear();
        mark_changed(s, changed);
        Ok(format!("{game}: its own settings cleared, the core-wide ones apply"))
    })
}

fn mark_changed(s: &mut State, keys: Vec<String>) {
    if keys.is_empty() {
        return;
    }
    s.update_pending = true;
    s.frames_since_change = 0;
    s.unread.extend(keys);
}

/// The current value of one key of the running core, as the engine would answer it.
pub fn current(core_id: &str, key: &str) -> Option<String> {
    with_state(|s| {
        let table = table_for(s, core_id);
        let live = s.active_core == core_id;
        let empty = BTreeMap::new();
        resolve(
            core_id,
            table.as_ref(),
            if live { &s.forced } else { &empty },
            if live { &s.game_values } else { &empty },
            if live { &s.core_values } else { &empty },
            key,
        )
    })
}

/// The declared definition of one key, if the core has it.
pub fn definition(core_id: &str, key: &str) -> Option<OptionDef> {
    with_state(|s| table_for(s, core_id).and_then(|t| t.get(key).cloned()))
}

/// Moves one option to its next (or previous) value and stores it core-wide. The status line names
/// the new value by its label. Used by the palette and resolution cycle actions.
pub fn cycle(core_id: &str, key: &str, forward: bool) -> Result<(String, String), String> {
    let def = definition(core_id, key).ok_or_else(|| format!("{core_id} has no {key} setting"))?;
    if def.values.is_empty() {
        return Err(format!("{} lists no values to cycle", def.label));
    }
    let now = current(core_id, key).unwrap_or_else(|| def.default.clone());
    let index = def.values.iter().position(|v| v.value == now).unwrap_or(0);
    let n = def.values.len();
    let next = if forward { (index + 1) % n } else { (index + n - 1) % n };
    let value = def.values[next].clone();
    set(core_id, key, &value.value, false)?;
    Ok((value.value, value.label))
}

/// Test-only reset of the whole global.
#[cfg(test)]
pub fn reset_for_tests() {
    let mut guard = match STATE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = None;
}

/// Serialises tests that touch the global, here and in `native_core`.
#[cfg(test)]
pub static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
pub fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    match TEST_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::null;

    fn c(s: &str) -> CString {
        CString::new(s).unwrap()
    }

    const NO_VALUE: RetroCoreOptionValue = RetroCoreOptionValue {
        value: std::ptr::null(),
        label: std::ptr::null(),
    };

    /// Owns every string a fake table points at.
    struct Strings(Vec<CString>);
    impl Strings {
        fn p(&mut self, s: &str) -> *const c_char {
            self.0.push(c(s));
            self.0.last().unwrap().as_ptr()
        }
    }

    fn values(strings: &mut Strings, list: &[(&str, Option<&str>)]) -> [RetroCoreOptionValue; NUM_CORE_OPTION_VALUES_MAX] {
        let mut out = [NO_VALUE; NUM_CORE_OPTION_VALUES_MAX];
        for (slot, (value, label)) in out.iter_mut().zip(list) {
            *slot = RetroCoreOptionValue {
                value: strings.p(value),
                label: label.map_or(null(), |l| strings.p(l)),
            };
        }
        out
    }

    /// A fresh, empty directory for one test, under the system temp dir rather than the source
    /// tree (cargo runs tests from the crate directory, so a relative `target/` here once put test
    /// output into git). The process id keeps concurrent `cargo test` runs apart, and the counter
    /// gives every call its own folder, so two tests passing the same `name` cannot share state.
    fn temp_root(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir()
            .join(format!("continuum-options-tests-{}", std::process::id()))
            .join(format!("{name}-{}", NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn v0_fake(strings: &mut Strings) -> Vec<RetroVariable> {
        vec![
            RetroVariable { key: strings.p("fake_palette"), value: strings.p("Palette; grey|green|amber") },
            RetroVariable { key: strings.p("fake_frameskip"), value: strings.p("Frameskip (Restart); 0|1|2") },
            RetroVariable { key: strings.p("fake_bare"), value: strings.p("no list at all") },
            RetroVariable { key: null(), value: null() },
        ]
    }

    #[test]
    fn v0_takes_the_label_values_and_the_first_value_as_default() {
        let mut s = Strings(Vec::new());
        let vars = v0_fake(&mut s);
        let table = unsafe { parse_v0(vars.as_ptr()) };
        assert_eq!(table.version, 0);
        assert_eq!(table.defs.len(), 3);
        let palette = &table.defs[0];
        assert_eq!(palette.label, "Palette");
        assert_eq!(palette.default, "grey");
        assert_eq!(palette.values.iter().map(|v| v.value.as_str()).collect::<Vec<_>>(), ["grey", "green", "amber"]);
        assert!(table.defs[1].says_restart());
        assert!(table.defs[2].values.is_empty());
        assert_eq!(table.defs[2].label, "no list at all");
    }

    #[test]
    fn v0_value_parser_handles_odd_spacing() {
        let d = parse_v0_value("k", "Thing;   on|off");
        assert_eq!(d.label, "Thing");
        assert_eq!(d.default, "on");
        let d = parse_v0_value("k", "; a");
        assert_eq!(d.label, "k");
        assert_eq!(d.values.len(), 1);
    }

    #[test]
    fn v1_reads_labels_info_and_a_default_that_is_not_first() {
        let mut s = Strings(Vec::new());
        let defs = [
            RetroCoreOptionDefinition {
                key: s.p("fake_res"),
                desc: s.p("Internal Resolution"),
                info: s.p("Higher is sharper"),
                values: values(&mut s, &[("1x", Some("Native")), ("2x", None), ("4x", Some("4x (slow)"))]),
                default_value: s.p("2x"),
            },
            RetroCoreOptionDefinition {
                key: s.p("fake_bad_default"),
                desc: null(),
                info: null(),
                values: values(&mut s, &[("a", None), ("b", None)]),
                default_value: s.p("zzz"),
            },
            RetroCoreOptionDefinition {
                key: null(),
                desc: null(),
                info: null(),
                values: [NO_VALUE; NUM_CORE_OPTION_VALUES_MAX],
                default_value: null(),
            },
        ];
        let table = unsafe { parse_v1(defs.as_ptr()) };
        assert_eq!(table.version, 1);
        assert_eq!(table.defs.len(), 2);
        let res = &table.defs[0];
        assert_eq!(res.label, "Internal Resolution");
        assert_eq!(res.info, "Higher is sharper");
        assert_eq!(res.default, "2x");
        assert_eq!(res.values[0].label, "Native");
        assert_eq!(res.values[1].label, "2x", "a missing label shows the value");
        // An unmatched default falls back to the first value; a null desc shows the key.
        assert_eq!(table.defs[1].default, "a");
        assert_eq!(table.defs[1].label, "fake_bad_default");

        let intl = RetroCoreOptionsIntl { us: defs.as_ptr(), local: null() };
        let from_intl = unsafe { parse_v1_intl(&intl) };
        assert_eq!(from_intl, table, "INTL reads the US table");
    }

    #[test]
    fn v1_stops_at_the_value_limit() {
        let mut s = Strings(Vec::new());
        let names: Vec<String> = (0..NUM_CORE_OPTION_VALUES_MAX).map(|i| format!("v{i}")).collect();
        let list: Vec<(&str, Option<&str>)> = names.iter().map(|n| (n.as_str(), None)).collect();
        let defs = [
            RetroCoreOptionDefinition {
                key: s.p("full"),
                desc: s.p("Full"),
                info: null(),
                values: values(&mut s, &list),
                default_value: null(),
            },
            RetroCoreOptionDefinition {
                key: null(),
                desc: null(),
                info: null(),
                values: [NO_VALUE; NUM_CORE_OPTION_VALUES_MAX],
                default_value: null(),
            },
        ];
        let table = unsafe { parse_v1(defs.as_ptr()) };
        assert_eq!(table.defs[0].values.len(), NUM_CORE_OPTION_VALUES_MAX);
        assert_eq!(table.defs[0].default, "v0");
    }

    fn v2_fake(s: &mut Strings) -> (Vec<RetroCoreOptionV2Category>, Vec<RetroCoreOptionV2Definition>) {
        let categories = vec![
            RetroCoreOptionV2Category { key: s.p("video"), desc: s.p("Video"), info: s.p("Picture settings") },
            RetroCoreOptionV2Category { key: s.p("audio"), desc: s.p("Audio"), info: null() },
            RetroCoreOptionV2Category { key: null(), desc: null(), info: null() },
        ];
        let definitions = vec![
            RetroCoreOptionV2Definition {
                key: s.p("fake_colors"),
                desc: s.p("Video > Palette"),
                desc_categorized: s.p("Palette"),
                info: s.p("Long info"),
                info_categorized: null(),
                category_key: s.p("video"),
                values: values(s, &[("Grayscale", None), ("Green", None)]),
                default_value: s.p("Grayscale"),
            },
            RetroCoreOptionV2Definition {
                key: s.p("fake_lowpass"),
                desc: s.p("Low pass"),
                desc_categorized: null(),
                info: null(),
                info_categorized: null(),
                category_key: s.p("audio"),
                values: values(s, &[("disabled", Some("Off")), ("enabled", Some("On"))]),
                default_value: s.p("enabled"),
            },
            RetroCoreOptionV2Definition {
                key: s.p("fake_orphan"),
                desc: s.p("Orphan"),
                desc_categorized: s.p("Should not be used"),
                info: null(),
                info_categorized: null(),
                category_key: s.p("nonexistent"),
                values: values(s, &[("x", None)]),
                default_value: null(),
            },
            RetroCoreOptionV2Definition {
                key: null(),
                desc: null(),
                desc_categorized: null(),
                info: null(),
                info_categorized: null(),
                category_key: null(),
                values: [NO_VALUE; NUM_CORE_OPTION_VALUES_MAX],
                default_value: null(),
            },
        ];
        (categories, definitions)
    }

    #[test]
    fn v2_reads_categories_and_uses_the_categorised_wording_inside_one() {
        let mut s = Strings(Vec::new());
        let (categories, definitions) = v2_fake(&mut s);
        let options = RetroCoreOptionsV2 { categories: categories.as_ptr(), definitions: definitions.as_ptr() };
        let table = unsafe { parse_v2(&options) };
        assert_eq!(table.version, 2);
        assert_eq!(table.categories.len(), 2);
        assert_eq!(table.categories[0].label, "Video");
        assert_eq!(table.categories[0].info, "Picture settings");
        assert_eq!(table.defs.len(), 3);
        assert_eq!(table.defs[0].label, "Palette");
        assert_eq!(table.defs[0].category, "video");
        assert_eq!(table.defs[0].info, "Long info", "no categorised info falls back to info");
        assert_eq!(table.defs[1].label, "Low pass");
        assert_eq!(table.defs[1].default, "enabled");
        assert_eq!(table.defs[1].values[0].label, "Off");
        assert_eq!(table.defs[2].category, "", "an undeclared category counts as none");
        assert_eq!(table.defs[2].label, "Orphan");

        let intl = RetroCoreOptionsV2Intl { us: &options, local: null() };
        assert_eq!(unsafe { parse_v2_intl(&intl) }, table);
    }

    #[test]
    fn v2_with_no_categories_and_null_pointers_is_safe() {
        let mut s = Strings(Vec::new());
        let (_, definitions) = v2_fake(&mut s);
        let options = RetroCoreOptionsV2 { categories: null(), definitions: definitions.as_ptr() };
        let table = unsafe { parse_v2(&options) };
        assert!(table.categories.is_empty());
        assert!(table.defs.iter().all(|d| d.category.is_empty()));
        assert_eq!(table.defs[0].label, "Video > Palette");
        assert!(unsafe { parse_v2(std::ptr::null()) }.defs.is_empty());
        assert!(unsafe { parse_v2_intl(std::ptr::null()) }.defs.is_empty());
        assert!(unsafe { parse_v1(std::ptr::null()) }.defs.is_empty());
        assert!(unsafe { parse_v0(std::ptr::null()) }.defs.is_empty());
    }

    fn sample_table() -> OptionTable {
        let mut s = Strings(Vec::new());
        let (categories, definitions) = v2_fake(&mut s);
        let options = RetroCoreOptionsV2 { categories: categories.as_ptr(), definitions: definitions.as_ptr() };
        unsafe { parse_v2(&options) }
    }

    #[test]
    fn resolve_order_is_lock_forced_game_core_engine_default_core_default() {
        let table = sample_table();
        let mut forced = BTreeMap::new();
        let mut game = BTreeMap::new();
        let mut core = BTreeMap::new();
        let r = |f: &BTreeMap<String, String>, g: &BTreeMap<String, String>, c: &BTreeMap<String, String>| {
            resolve("fake", Some(&table), f, g, c, "fake_colors")
        };
        assert_eq!(r(&forced, &game, &core).as_deref(), Some("Grayscale"));
        core.insert("fake_colors".into(), "Green".into());
        assert_eq!(r(&forced, &game, &core).as_deref(), Some("Green"));
        game.insert("fake_colors".into(), "Grayscale".into());
        assert_eq!(r(&forced, &game, &core).as_deref(), Some("Grayscale"));
        forced.insert("fake_colors".into(), "Green".into());
        assert_eq!(r(&forced, &game, &core).as_deref(), Some("Green"));
        // A stale stored value the core no longer offers is skipped.
        let mut stale = BTreeMap::new();
        stale.insert("fake_colors".to_string(), "Purple".to_string());
        assert_eq!(resolve("fake", Some(&table), &BTreeMap::new(), &stale, &BTreeMap::new(), "fake_colors").as_deref(), Some("Grayscale"));
        // An undeclared key with no rule is refused, as it always was.
        assert_eq!(resolve("fake", Some(&table), &BTreeMap::new(), &BTreeMap::new(), &BTreeMap::new(), "unknown"), None);
    }

    #[test]
    fn melonds_forced_values_are_defaults_the_user_can_change() {
        let empty = BTreeMap::new();
        assert_eq!(resolve("melonds", None, &empty, &empty, &empty, "melonds_touch_mode").as_deref(), Some("Touch"));
        assert_eq!(resolve("melonds", None, &empty, &empty, &empty, "melonds_boot_directly").as_deref(), Some("enabled"));
        let mut core = BTreeMap::new();
        core.insert("melonds_touch_mode".to_string(), "Mouse".to_string());
        assert_eq!(resolve("melonds", None, &empty, &empty, &core, "melonds_touch_mode").as_deref(), Some("Mouse"));
        assert!(!is_locked("melonds", "melonds_touch_mode"));
    }

    #[test]
    fn beetle_psx_keeps_the_software_renderer_a_phone_has_shown() {
        let empty = BTreeMap::new();
        assert_eq!(
            resolve("mednafen_psx_hw", None, &empty, &empty, &empty, "beetle_psx_hw_renderer").as_deref(),
            Some("software")
        );
        assert!(!is_locked("mednafen_psx_hw", "beetle_psx_hw_renderer"));
    }

    #[test]
    fn locked_keys_ignore_the_user() {
        let mut core = BTreeMap::new();
        core.insert("ppsspp_cpu_core".to_string(), "JIT".to_string());
        core.insert("parallel-n64-gfxplugin".to_string(), "angrylion".to_string());
        let empty = BTreeMap::new();
        assert_eq!(resolve("ppsspp", None, &empty, &empty, &core, "ppsspp_cpu_core").as_deref(), Some("IR JIT"));
        assert_eq!(resolve("parallel_n64", None, &empty, &empty, &core, "parallel-n64-gfxplugin"), None);
        assert_eq!(resolve("parallel_n64", None, &empty, &empty, &empty, "parallel-n64-angrylion-multithread").as_deref(), Some("off"));
        assert!(is_locked("parallel_n64", "parallel-n64-cpucore"));
    }

    #[test]
    fn opt_files_round_trip() {
        let mut map = BTreeMap::new();
        map.insert("a_key".to_string(), "some value".to_string());
        map.insert("b".to_string(), "".to_string());
        let text = write_opt(&map);
        assert_eq!(read_opt(&text), map);
        assert_eq!(read_opt("# comment\nx = y\n bad line\nq=\"r\"").get("x").map(String::as_str), Some("y"));
    }

    #[test]
    fn declared_tables_round_trip_through_the_cache() {
        let mut table = sample_table();
        table.defs[0].info = "tab\there\nand a newline \\ slash".into();
        assert_eq!(read_table(&write_table(&table)), table);
    }

    #[test]
    fn game_names_become_safe_file_names() {
        assert_eq!(game_file_stem("Zelda: Link/Awakening"), "Zelda_ Link_Awakening");
        assert_eq!(game_file_stem("..hidden"), "hidden");
        assert_eq!(game_file_stem(""), "game");
    }

    /// Azahar's System Model as it declares it: "Restart required." only in the description.
    fn model_table() -> OptionTable {
        let value = |v: &str, l: &str| OptionValue { value: v.into(), label: l.into() };
        OptionTable {
            version: 2,
            categories: Vec::new(),
            defs: vec![
                OptionDef {
                    key: "citra_is_new_3ds".into(),
                    label: "System Model".into(),
                    info: "Select whether to emulate the original 3DS or New 3DS. Restart required."
                        .into(),
                    category: String::new(),
                    values: vec![value("New 3DS", "New 3DS"), value("Old 3DS", "Original 3DS")],
                    default: "New 3DS".into(),
                },
                OptionDef {
                    key: "citra_swap".into(),
                    label: "Swap screens".into(),
                    info: "Applied at once, no restart needed.".into(),
                    category: String::new(),
                    values: vec![value("Top", "Top"), value("Bottom", "Bottom")],
                    default: "Top".into(),
                },
            ],
        }
    }

    #[test]
    fn a_restart_option_is_held_until_the_next_start() {
        let _g = test_guard();
        reset_for_tests();
        // A settings folder, as on the phone: a restart reads the choices back from it.
        set_root(Some(temp_root("held-restart")));
        install("azahar_like", None, true);
        declare(model_table());
        install("azahar_like", Some("Mario Kart 7.3ds"), false);
        let ask = |key: &str| {
            answer(key).map(|p| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
        };
        // The game starts on the New 3DS.
        assert_eq!(ask("citra_is_new_3ds").as_deref(), Some("New 3DS"));
        assert_eq!(ask("citra_swap").as_deref(), Some("Top"));

        // Changed mid-game. The core re-reads EVERYTHING on the update, as Azahar does.
        set("azahar_like", "citra_is_new_3ds", "Old 3DS", false).unwrap();
        set("azahar_like", "citra_swap", "Bottom", false).unwrap();
        assert!(take_update());
        assert_eq!(ask("citra_is_new_3ds").as_deref(), Some("New 3DS"), "held for the session");
        assert_eq!(ask("citra_swap").as_deref(), Some("Bottom"), "a live option changes at once");

        let view = list("azahar_like");
        let model = view.iter().find(|v| v.key == "citra_is_new_3ds").unwrap();
        assert_eq!(model.current, "Old 3DS", "the choice is stored");
        assert_eq!(model.in_effect, "New 3DS", "the game is still a New 3DS");
        assert!(model.needs_restart, "so the restart button shows, even after the re-read");
        let swap = view.iter().find(|v| v.key == "citra_swap").unwrap();
        assert!(!swap.needs_restart);
        assert_eq!(swap.in_effect, "Bottom");

        // Changed back: nothing is waiting any more.
        set("azahar_like", "citra_is_new_3ds", "New 3DS", false).unwrap();
        assert!(!list("azahar_like")[0].needs_restart);
        set("azahar_like", "citra_is_new_3ds", "Old 3DS", false).unwrap();

        // The restart: a new load, and the new model is what the core gets.
        install("azahar_like", Some("Mario Kart 7.3ds"), false);
        assert_eq!(ask("citra_is_new_3ds").as_deref(), Some("Old 3DS"));
        let model = list("azahar_like").into_iter().next().unwrap();
        assert!(!model.needs_restart);
        assert_eq!(model.in_effect, "Old 3DS");
    }

    #[test]
    fn restart_wording_is_read_from_the_description_too() {
        let table = model_table();
        assert!(table.defs[0].says_restart(), "Azahar says it only in the description");
        assert!(!table.defs[1].says_restart(), "\"no restart needed\" is not a restart");
    }

    #[test]
    fn the_live_flow_answers_persists_and_detects_a_restart() {
        let _g = test_guard();
        reset_for_tests();
        let root = temp_root("live-flow");
        set_root(Some(root.clone()));
        install("fakecore", None, true);
        declare(sample_table());
        install("fakecore", Some("Game One.gb"), false);

        let ask = |key: &str| answer(key).map(|p| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned());
        assert_eq!(ask("fake_colors").as_deref(), Some("Grayscale"));
        assert!(!take_update());

        let line = set("fakecore", "fake_colors", "Green", false).unwrap();
        assert!(line.contains("Green"), "{line}");
        assert!(take_update(), "the core is told once");
        assert!(!take_update());
        // Not re-read yet, and the frames have not passed: not flagged.
        let view = list("fakecore");
        assert!(!view[0].needs_restart);
        for _ in 0..RESTART_DETECT_FRAMES {
            note_frame();
        }
        let view = list("fakecore");
        assert!(view[0].needs_restart, "a core that never re-reads needs a restart");
        assert_eq!(view[0].current, "Green");
        // Reading it clears the flag.
        assert_eq!(ask("fake_colors").as_deref(), Some("Green"));
        assert!(!list("fakecore")[0].needs_restart);

        // A per-game choice wins over the core-wide one, and both are on disk.
        set("fakecore", "fake_lowpass", "disabled", true).unwrap();
        assert_eq!(ask("fake_lowpass").as_deref(), Some("disabled"));
        assert!(list("fakecore")[1].game_override);
        assert!(core_opt_path(&root, "fakecore").exists());
        assert!(game_opt_path(&root, "fakecore", "Game One.gb").exists());

        // A fresh run: the choices come back from disk before the game loads.
        reset_for_tests();
        set_root(Some(root.clone()));
        install("fakecore", Some("Game One.gb"), true);
        declare(sample_table());
        assert_eq!(ask("fake_colors").as_deref(), Some("Green"));
        assert_eq!(ask("fake_lowpass").as_deref(), Some("disabled"));
        // Another game has only the core-wide choice.
        install("fakecore", Some("Game Two.gb"), false);
        assert_eq!(ask("fake_lowpass").as_deref(), Some("enabled"));
        install("fakecore", Some("Game One.gb"), false);

        reset_game("fakecore").unwrap();
        assert_eq!(ask("fake_lowpass").as_deref(), Some("enabled"));
        reset_core("fakecore").unwrap();
        assert_eq!(ask("fake_colors").as_deref(), Some("Grayscale"));
        assert!(!core_opt_path(&root, "fakecore").exists());
        reset_for_tests();
    }

    #[test]
    fn settings_without_the_core_loaded_use_the_cached_table() {
        let _g = test_guard();
        reset_for_tests();
        let root = temp_root("cached");
        set_root(Some(root.clone()));
        install("cachedcore", None, true);
        declare(sample_table());
        install("othercore", None, true);
        reset_for_tests();
        set_root(Some(root));
        assert!(has_table("cachedcore"));
        let view = list("cachedcore");
        assert_eq!(view.len(), 3);
        set("cachedcore", "fake_colors", "Green", false).unwrap();
        assert_eq!(list("cachedcore")[0].current, "Green");
        assert!(set("cachedcore", "fake_colors", "Purple", false).is_err());
        assert!(set("cachedcore", "fake_colors", "Green", true).is_err(), "per game needs the game");
        assert!(set("cachedcore", "no_such_key", "x", false).is_err());
        reset_for_tests();
    }

    #[test]
    fn visibility_hides_options_and_locked_keys_are_never_listed() {
        let _g = test_guard();
        reset_for_tests();
        install("melonds", None, true);
        let mut s = Strings(Vec::new());
        let vars = [
            RetroVariable { key: s.p("melonds_touch_mode"), value: s.p("Touch mode; Mouse|Touch|Joystick") },
            RetroVariable { key: s.p("melonds_other"), value: s.p("Other; a|b") },
            RetroVariable { key: null(), value: null() },
        ];
        declare(unsafe { parse_v0(vars.as_ptr()) });
        set_visible("melonds_other", false);
        let view = list("melonds");
        assert_eq!(view[0].default, "Touch", "the engine's default is the one shown");
        assert_eq!(view[0].current, "Touch");
        assert!(!view[1].visible);
        set_visible("melonds_other", true);
        assert!(list("melonds")[1].visible);

        install("ppsspp", None, true);
        let vars = [
            RetroVariable { key: s.p("ppsspp_cpu_core"), value: s.p("CPU; JIT|IR JIT|Interpreter") },
            RetroVariable { key: null(), value: null() },
        ];
        declare(unsafe { parse_v0(vars.as_ptr()) });
        assert!(list("ppsspp").is_empty(), "the locked PSP CPU is not offered");
        assert!(set("ppsspp", "ppsspp_cpu_core", "JIT", false).is_err());
        reset_for_tests();
    }

    #[test]
    fn the_core_forcing_a_value_is_answered_and_flagged() {
        let _g = test_guard();
        reset_for_tests();
        install("fakecore", None, true);
        declare(sample_table());
        assert!(!core_sets("fake_colors", "Purple"));
        assert!(!core_sets("unknown", "x"));
        assert!(core_sets("fake_colors", "Green"));
        assert!(take_update());
        assert_eq!(current("fakecore", "fake_colors").as_deref(), Some("Green"));
        reset_for_tests();
    }

    #[test]
    fn cycle_walks_the_values_and_wraps() {
        let _g = test_guard();
        reset_for_tests();
        install("fakecore", None, true);
        declare(sample_table());
        assert_eq!(cycle("fakecore", "fake_colors", true).unwrap().0, "Green");
        assert_eq!(cycle("fakecore", "fake_colors", true).unwrap().0, "Grayscale");
        assert_eq!(cycle("fakecore", "fake_colors", false).unwrap().0, "Green");
        assert!(cycle("fakecore", "missing", true).is_err());
        reset_for_tests();
    }

    #[test]
    fn display_callback_is_only_called_for_the_active_core_and_forgotten_on_unload() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static CALLS: AtomicU32 = AtomicU32::new(0);
        unsafe extern "C" fn cb() -> bool {
            CALLS.fetch_add(1, Ordering::SeqCst);
            true
        }
        let _g = test_guard();
        reset_for_tests();
        install("fakecore", None, true);
        set_display_callback(Some(cb));
        assert!(refresh_visibility("fakecore"));
        assert!(!refresh_visibility("othercore"));
        forget_core("fakecore");
        assert!(!refresh_visibility("fakecore"));
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
        reset_for_tests();
    }

    #[test]
    fn repeated_reads_do_not_grow_the_retired_list() {
        let _g = test_guard();
        reset_for_tests();
        install("fakecore", None, true);
        declare(sample_table());
        let first = answer("fake_colors").unwrap();
        for _ in 0..1000 {
            assert_eq!(answer("fake_colors").unwrap(), first, "the same pointer is reused");
        }
        let retired = with_state(|s| s.retired.len());
        assert_eq!(retired, 0);
        reset_for_tests();
    }
}
