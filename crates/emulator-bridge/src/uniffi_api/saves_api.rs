//! The Swift-facing half of core memory, battery saves, RAM search, pokes, `.cht` import, the
//! save-state export file and the 50-slot plan.
//!
//! A child of `uniffi_api` so it can use the engine's private `lock()`, and held to the same
//! rule: nothing in here is logic. Every function converts a type and delegates to `bridge.rs`,
//! `cheats` or `saves`, which is where Android will find the same behaviour.
//!
//! Every exported method is unconditional, because `#[uniffi::export]` ignores `#[cfg]` on
//! individual methods.

use std::collections::HashMap;

use super::{ContinuumEngine, EngineError};
use crate::cheats::search::{SearchFilter, SearchWidth};

/// A libretro memory region. Mirrors the `RETRO_MEMORY_*` ids in [`crate::memory`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MemoryRegion {
    /// Battery-backed cartridge RAM: the `.srm` file.
    SaveRam,
    Rtc,
    SystemRam,
    VideoRam,
}

impl MemoryRegion {
    fn id(self) -> u32 {
        match self {
            MemoryRegion::SaveRam => crate::memory::MEMORY_SAVE_RAM,
            MemoryRegion::Rtc => crate::memory::MEMORY_RTC,
            MemoryRegion::SystemRam => crate::memory::MEMORY_SYSTEM_RAM,
            MemoryRegion::VideoRam => crate::memory::MEMORY_VIDEO_RAM,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RamSearchWidth {
    Bits8,
    Bits16,
    Bits32,
}

impl From<RamSearchWidth> for SearchWidth {
    fn from(width: RamSearchWidth) -> Self {
        match width {
            RamSearchWidth::Bits8 => SearchWidth::Bits8,
            RamSearchWidth::Bits16 => SearchWidth::Bits16,
            RamSearchWidth::Bits32 => SearchWidth::Bits32,
        }
    }
}

impl From<SearchWidth> for RamSearchWidth {
    fn from(width: SearchWidth) -> Self {
        match width {
            SearchWidth::Bits8 => RamSearchWidth::Bits8,
            SearchWidth::Bits16 => RamSearchWidth::Bits16,
            SearchWidth::Bits32 => RamSearchWidth::Bits32,
        }
    }
}

/// One RAM search filter. See `cheats::search` for which snapshot each compares with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RamSearchFilter {
    EqualToPrevious,
    NotEqualToPrevious,
    GreaterThanPrevious,
    LessThanPrevious,
    Changed,
    Unchanged,
    EqualTo { value: u32 },
    NotEqualTo { value: u32 },
    GreaterThan { value: u32 },
    LessThan { value: u32 },
    IncreasedBy { value: u32 },
    DecreasedBy { value: u32 },
}

impl From<RamSearchFilter> for SearchFilter {
    fn from(filter: RamSearchFilter) -> Self {
        match filter {
            RamSearchFilter::EqualToPrevious => SearchFilter::EqualToPrevious,
            RamSearchFilter::NotEqualToPrevious => SearchFilter::NotEqualToPrevious,
            RamSearchFilter::GreaterThanPrevious => SearchFilter::GreaterThanPrevious,
            RamSearchFilter::LessThanPrevious => SearchFilter::LessThanPrevious,
            RamSearchFilter::Changed => SearchFilter::Changed,
            RamSearchFilter::Unchanged => SearchFilter::Unchanged,
            RamSearchFilter::EqualTo { value } => SearchFilter::EqualTo(value),
            RamSearchFilter::NotEqualTo { value } => SearchFilter::NotEqualTo(value),
            RamSearchFilter::GreaterThan { value } => SearchFilter::GreaterThan(value),
            RamSearchFilter::LessThan { value } => SearchFilter::LessThan(value),
            RamSearchFilter::IncreasedBy { value } => SearchFilter::IncreasedBy(value),
            RamSearchFilter::DecreasedBy { value } => SearchFilter::DecreasedBy(value),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RamSearchHit {
    pub address: u32,
    pub current: u32,
    pub previous: u32,
}

/// One cheat read from a `.cht` file.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ChtEntry {
    pub description: String,
    pub code: String,
    pub enabled: bool,
}

/// A parsed `.cht` file: the cheats in file order, and a sentence for everything skipped.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ChtImport {
    pub cheats: Vec<ChtEntry>,
    pub declared: Option<u32>,
    pub warnings: Vec<String>,
}

/// A RAM poke, decoded from its code string.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PokeRecord {
    pub address: u32,
    pub value: u32,
    pub bytes: u8,
    /// `address` is the console's own address (a poke made from a mapped region), not an offset
    /// into system RAM.
    pub bus: bool,
}

/// One memory region the RAM search can run over.
#[derive(Debug, Clone, uniffi::Record)]
pub struct RamSearchRegion {
    /// Pass back to `ram_search_start_in`: "system" or "map:N".
    pub key: String,
    /// "System RAM", "IWRAM", "PRGRAM", ...
    pub name: String,
    /// The console address of the first byte (0 for system RAM).
    pub start: u64,
    pub size: u64,
}

/// The metadata inside a `.continuumstate` export.
#[derive(Debug, Clone, uniffi::Record)]
pub struct StateExportRecord {
    pub game_id: String,
    pub core_id: Option<String>,
    pub core_version: Option<String>,
    pub byte_count: u64,
    pub frame: u64,
    pub created_at: f64,
    pub slot: i64,
    pub label: String,
    /// The core option values the state was saved under (`SaveStateRecord.coreOptions`). `None`
    /// is unknown, a file from a build that did not carry them, and is NOT the same as an empty
    /// map: the gate checks an unknown against defaults. Last, and defaulted, so the Swift
    /// initialiser only grew a trailing `coreOptions: [String: String]? = nil`.
    #[uniffi(default = None)]
    pub core_options: Option<HashMap<String, String>>,
}

impl From<StateExportRecord> for crate::saves::StateExportMeta {
    fn from(r: StateExportRecord) -> Self {
        Self {
            game_id: r.game_id,
            core_id: r.core_id,
            core_version: r.core_version,
            byte_count: r.byte_count,
            frame: r.frame,
            created_at: r.created_at,
            slot: r.slot,
            label: r.label,
            // Into a sorted map, which is what makes the written file deterministic.
            core_options: r.core_options.map(|o| o.into_iter().collect()),
        }
    }
}

impl From<crate::saves::StateExportMeta> for StateExportRecord {
    fn from(m: crate::saves::StateExportMeta) -> Self {
        Self {
            game_id: m.game_id,
            core_id: m.core_id,
            core_version: m.core_version,
            byte_count: m.byte_count,
            frame: m.frame,
            created_at: m.created_at,
            slot: m.slot,
            label: m.label,
            core_options: m.core_options.map(|o| o.into_iter().collect()),
        }
    }
}

/// An unpacked export: the facts the gate checks, the payload, and the slot's picture (a PNG) when
/// the file carried one.
#[derive(Debug, Clone, uniffi::Record)]
pub struct UnpackedStateExport {
    pub meta: StateExportRecord,
    pub payload: Vec<u8>,
    #[uniffi(default = None)]
    pub picture: Option<Vec<u8>>,
}

/// One existing manual state, as the slot planner needs it.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ExistingSlot {
    pub slot: i64,
    pub created_at: f64,
}

/// Where one existing state goes. Same order as the input.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SlotPlan {
    Keep { slot: u32 },
    Move { from: i64, to: u32 },
    Overflow { slot: i64 },
}

// ------------------------------------------------------------------ free functions

/// Parses the text of a RetroArch `.cht` file. Never fails: a file with nothing usable comes back
/// empty with warnings saying why.
#[uniffi::export]
pub fn parse_cht_file(text: String) -> ChtImport {
    let file = crate::cheats::cht::parse(&text);
    ChtImport {
        cheats: file
            .cheats
            .into_iter()
            .map(|c| ChtEntry {
                description: c.description,
                code: c.code,
                enabled: c.enabled,
            })
            .collect(),
        declared: file.declared,
        warnings: file.warnings,
    }
}

/// The cheat-list code for "write `value` at `address` every frame". `bytes` is 1, 2 or 4.
#[uniffi::export]
pub fn make_poke_code(address: u32, value: u32, bytes: u8) -> Result<String, EngineError> {
    crate::cheats::poke::Poke::new(address, value, bytes)
        .map(|p| p.code())
        .map_err(|reason| EngineError::Cheat { reason })
}

/// Decodes a poke code, or `None` when the string is not one.
#[uniffi::export]
pub fn describe_poke_code(code: String) -> Option<PokeRecord> {
    crate::cheats::poke::Poke::parse(&code)
        .ok()
        .map(|p| PokeRecord {
            address: p.address,
            value: p.value,
            bytes: p.bytes,
            bus: p.bus,
        })
}

/// Whether a cheat-list entry is a RAM poke (even a malformed one).
#[uniffi::export]
pub fn is_poke_code(code: String) -> bool {
    crate::cheats::poke::Poke::is_poke_code(&code)
}

/// What one emulator does with a typed code. See `cheats::formats`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CheatCodeSupport {
    /// False when the emulator throws typed codes away (the 3DS, Dreamcast, Atari 2600 and more).
    pub reads_typed_codes: bool,
    /// The kinds of code it reads, in plain words. Empty when not checked.
    pub kinds: String,
}

/// Which typed codes the core `core_id` reads.
#[uniffi::export]
pub fn cheat_code_support(core_id: String) -> CheatCodeSupport {
    let support = crate::cheats::formats::code_support(&core_id);
    CheatCodeSupport {
        reads_typed_codes: support.reads_typed_codes,
        kinds: support.kinds.to_string(),
    }
}

/// Manual save slots per game.
#[uniffi::export]
pub fn save_slot_count() -> u32 {
    crate::saves::SLOT_COUNT
}

/// Fits one game's existing manual states into the 50-slot grid without losing any.
#[uniffi::export]
pub fn plan_save_slots(existing: Vec<ExistingSlot>) -> Vec<SlotPlan> {
    let input: Vec<(i64, f64)> = existing.iter().map(|e| (e.slot, e.created_at)).collect();
    crate::saves::plan_slots(&input)
        .into_iter()
        .map(|p| match p {
            crate::saves::SlotPlacement::Keep { slot } => SlotPlan::Keep { slot },
            crate::saves::SlotPlacement::Move { from, to } => SlotPlan::Move { from, to },
            crate::saves::SlotPlacement::Overflow { slot } => SlotPlan::Overflow { slot },
        })
        .collect()
}

/// Wraps a state and its metadata into one `.continuumstate` file.
#[uniffi::export]
pub fn pack_state_export(meta: StateExportRecord, payload: Vec<u8>) -> Vec<u8> {
    crate::saves::pack_state_export(&meta.into(), &payload)
}

/// [`pack_state_export`] with the slot's picture (a PNG) carried after the payload, so the slot
/// shows it as soon as the file is imported. `None` writes exactly what `pack_state_export` does.
#[uniffi::export]
pub fn pack_state_export_with_picture(
    meta: StateExportRecord,
    payload: Vec<u8>,
    picture: Option<Vec<u8>>,
) -> Vec<u8> {
    crate::saves::pack_state_export_with_picture(&meta.into(), &payload, picture.as_deref())
}

/// Reads a `.continuumstate` file. The compatibility gate still has to run on the result.
#[uniffi::export]
pub fn unpack_state_export(bytes: Vec<u8>) -> Result<UnpackedStateExport, EngineError> {
    crate::saves::unpack_state_export_with_picture(&bytes)
        .map(|unpacked| UnpackedStateExport {
            meta: unpacked.meta.into(),
            payload: unpacked.payload,
            picture: unpacked.picture,
        })
        .map_err(|reason| EngineError::SaveState { reason })
}

// ----------------------------------------------------------------------- methods

#[uniffi::export]
impl ContinuumEngine {
    /// Bytes in one of the running core's memory regions, `0` when it has none.
    pub fn memory_size(&self, region: MemoryRegion) -> u64 {
        self.lock().memory_size(region.id()) as u64
    }

    pub fn read_memory(
        &self,
        region: MemoryRegion,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, EngineError> {
        Ok(self.lock().read_memory(region.id(), offset, length)?)
    }

    pub fn write_memory(
        &self,
        region: MemoryRegion,
        offset: u64,
        bytes: Vec<u8>,
    ) -> Result<(), EngineError> {
        Ok(self.lock().write_memory(region.id(), offset, &bytes)?)
    }

    /// The running game's battery save, byte for byte (a `.srm`).
    pub fn battery_save(&self) -> Result<Vec<u8>, EngineError> {
        Ok(self.lock().battery_save()?)
    }

    /// Replaces the running game's battery save. Returns the save RAM size.
    pub fn restore_battery_save(&self, data: Vec<u8>) -> Result<u64, EngineError> {
        Ok(self.lock().restore_battery_save(&data)? as u64)
    }

    /// Writes the battery save to `path` atomically. `0` when the game has no battery RAM.
    pub fn persist_battery_save(&self, path: String) -> Result<u64, EngineError> {
        Ok(self.lock().persist_battery_save(&path)? as u64)
    }

    /// Restores the battery save from `path`. `None` when there is no file yet.
    pub fn load_battery_save_file(&self, path: String) -> Result<Option<u64>, EngineError> {
        Ok(self
            .lock()
            .load_battery_save_file(&path)?
            .map(|n| n as u64))
    }

    /// Starts a RAM search over system RAM. Returns how many addresses are candidates.
    pub fn ram_search_start(
        &self,
        width: RamSearchWidth,
        aligned: bool,
    ) -> Result<u64, EngineError> {
        Ok(self.lock().search_start(width.into(), aligned)? as u64)
    }

    /// Applies one filter. Returns how many candidates survive.
    pub fn ram_search_filter(&self, filter: RamSearchFilter) -> Result<u64, EngineError> {
        Ok(self.lock().search_filter(filter.into())? as u64)
    }

    /// Candidates left, or `None` when no search is running.
    pub fn ram_search_count(&self) -> Option<u64> {
        self.lock().search_count().map(|n| n as u64)
    }

    /// The running search's width, or `None`.
    pub fn ram_search_width(&self) -> Option<RamSearchWidth> {
        self.lock().search_width().map(Into::into)
    }

    /// Up to `limit` surviving addresses, in address order.
    pub fn ram_search_results(&self, limit: u32) -> Vec<RamSearchHit> {
        self.lock()
            .search_results(limit as usize)
            .into_iter()
            .map(|h| RamSearchHit {
                address: h.address,
                current: h.current,
                previous: h.previous,
            })
            .collect()
    }

    pub fn ram_search_clear(&self) {
        self.lock().search_clear();
    }

    /// The regions a search can run over: system RAM, then every writable region of the core's
    /// memory map (GBA IWRAM and EWRAM, SNES work RAM, ...). Empty with no game running.
    pub fn ram_search_regions(&self) -> Vec<RamSearchRegion> {
        self.lock()
            .search_regions()
            .into_iter()
            .map(|r| RamSearchRegion {
                key: r.key,
                name: r.name,
                start: r.start,
                size: r.size,
            })
            .collect()
    }

    /// Starts a search over one region from `ram_search_regions`. Hits from a mapped region carry
    /// the console's own addresses.
    pub fn ram_search_start_in(
        &self,
        region: String,
        width: RamSearchWidth,
        aligned: bool,
    ) -> Result<u64, EngineError> {
        Ok(self.lock().search_start_in(&region, width.into(), aligned)? as u64)
    }

    /// The key of the region the running search covers, or `None`.
    pub fn ram_search_region(&self) -> Option<String> {
        self.lock().search_region_key()
    }

    /// The cheat code that pins `value` at a hit's `address`, right for the region searched.
    pub fn ram_search_poke_code(
        &self,
        address: u32,
        value: u32,
        bytes: u8,
    ) -> Result<String, EngineError> {
        Ok(self.lock().search_poke_code(address, value, bytes)?)
    }
}
