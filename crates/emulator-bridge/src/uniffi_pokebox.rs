//! The Swift-facing half of the Pokémon Box (`pokebox`): free functions, nothing of the engine
//! lock. Every one converts types and delegates.

use std::path::Path;

use crate::pokebox::{self, mon, save, Change, Side};
use crate::uniffi_api::EngineError;

fn other(reason: String) -> EngineError {
    EngineError::Other { reason }
}

/// A game the Box knows.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PokeGame {
    /// "Red", "Crystal", "FireRed"...
    pub game: String,
    pub generation: u8,
}

/// One Pokémon, as the Box shows it.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PokeMon {
    pub species: u16,
    pub species_name: String,
    /// Its nickname, or its species' name when it has none.
    pub name: String,
    pub level: u8,
    pub shiny: bool,
    /// "♂", "♀" or "".
    pub gender: String,
    pub egg: bool,
    pub generation: u8,
    /// The game whose way it is kept in now.
    pub game: String,
    pub trainer: String,
    pub trainer_id: u16,
    pub moves: Vec<String>,
    /// Its held item, "" for none.
    pub item: String,
    /// Ruby to LeafGreen only, else "".
    pub nature: String,
    /// False for a damaged one (a "Bad Egg").
    pub readable: bool,
    /// HP, Attack, Defense, Speed, Sp. Atk, Sp. Def at its level (empty for an egg or a Bad Egg).
    pub stats: Vec<u16>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PokeBoxPage {
    pub name: String,
    pub capacity: u32,
    pub slots: Vec<Option<PokeMon>>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PokePc {
    pub game: String,
    pub generation: u8,
    pub trainer: String,
    pub trainer_id: u16,
    pub current_box: u32,
    pub boxes: Vec<PokeBoxPage>,
}

/// What a change will do: shown before it is made.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PokePlan {
    pub allowed: bool,
    pub title: String,
    pub changes: Vec<String>,
}

/// A change: a Pokémon from a game into the Box, from the Box into a game, within the Box, or a
/// trade between two games.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum PokeChange {
    ToStorage { rom_path: String, save_path: String, from_box: u32, slot: u32, to_box: u32 },
    ToGame { rom_path: String, save_path: String, from_box: u32, slot: u32, to_box: u32 },
    InStorage { from_box: u32, slot: u32, to_box: u32 },
    InStorageTo { from_box: u32, slot: u32, to_box: u32, to_slot: u32 },
    PartyToStorage { rom_path: String, save_path: String, slot: u32, to_box: u32 },
    StorageToParty { rom_path: String, save_path: String, from_box: u32, slot: u32 },
    /// `by`: 0 Pokédex number, 1 level, 2 name.
    Sort { box_index: u32, by: u8 },
    Trade {
        a_rom_path: String,
        a_save_path: String,
        a_box: u32,
        a_slot: u32,
        b_rom_path: String,
        b_save_path: String,
        b_box: u32,
        b_slot: u32,
    },
}

fn shown(s: &mon::Stored) -> PokeMon {
    match s.mon() {
        Some(m) => PokeMon {
            species: m.species,
            species_name: mon::species_name(m.species).to_owned(),
            name: if m.egg { "EGG".into() } else if m.nicknamed { m.nickname.clone() } else { mon::species_name(m.species).to_owned() },
            level: m.level,
            shiny: mon::shiny(&m),
            gender: mon::gender_of(&m).to_owned(),
            egg: m.egg,
            generation: s.game.gen(),
            game: s.game.name().to_owned(),
            trainer: m.ot_name.clone(),
            trainer_id: m.ot_id,
            moves: m.moves.iter().filter(|x| **x != 0).map(|x| mon::move_name(*x).to_owned()).collect(),
            item: if m.item == 0 { String::new() } else { mon::item_name(m.item).to_owned() },
            nature: mon::nature_of(&m).to_owned(),
            readable: true,
            stats: if m.egg { Vec::new() } else { mon::stats(&m).to_vec() },
        },
        None => PokeMon {
            species: 0,
            species_name: "Bad Egg".into(),
            name: "Bad Egg".into(),
            level: 0,
            shiny: false,
            gender: String::new(),
            egg: false,
            generation: s.game.gen(),
            game: s.game.name().to_owned(),
            trainer: String::new(),
            trainer_id: 0,
            moves: Vec::new(),
            item: String::new(),
            nature: String::new(),
            readable: false,
            stats: Vec::new(),
        },
    }
}

fn page(name: String, capacity: usize, slots: &[Option<mon::Stored>]) -> PokeBoxPage {
    PokeBoxPage { name, capacity: capacity as u32, slots: slots.iter().map(|s| s.as_ref().map(shown)).collect() }
}

/// Which Pokémon game a cartridge file is, or nil when the Box doesn't know it.
#[uniffi::export]
pub fn pokebox_identify(rom_path: String) -> Option<PokeGame> {
    pokebox::identify_rom(Path::new(&rom_path)).map(|(g, _)| PokeGame { game: g.name().to_owned(), generation: g.gen() })
}

/// A game's PC boxes, from its save.
#[uniffi::export]
pub fn pokebox_read_game(rom_path: String, save_path: String) -> Result<PokePc, EngineError> {
    let g = pokebox::open_game(Path::new(&rom_path), Path::new(&save_path)).map_err(other)?;
    let pc = save::read_pc(g.game, &g.bytes).map_err(other)?;
    Ok(PokePc {
        game: g.game.name().to_owned(),
        generation: g.game.gen(),
        trainer: pc.trainer,
        trainer_id: pc.trainer_id,
        current_box: pc.current_box as u32,
        boxes: pc.boxes.iter().map(|b| page(b.name.clone(), b.capacity, &b.slots)).collect(),
    })
}

/// A game's team, each Pokémon as the Box shows it.
#[uniffi::export]
pub fn pokebox_read_party(rom_path: String, save_path: String) -> Result<Vec<PokeMon>, EngineError> {
    let g = pokebox::open_game(Path::new(&rom_path), Path::new(&save_path)).map_err(other)?;
    Ok(save::read_party(g.game, &g.bytes).map_err(other)?.iter().map(shown).collect())
}

/// Finishes a change that was cut off; a line for the player when there was one.
#[uniffi::export]
pub fn pokebox_recover(dir: String) -> Option<String> {
    pokebox::recover(Path::new(&dir))
}

/// The Box's own boxes.
#[uniffi::export]
pub fn pokebox_read_storage(dir: String) -> Result<Vec<PokeBoxPage>, EngineError> {
    let st = pokebox::Storage::load(Path::new(&dir)).map_err(other)?;
    Ok(st.boxes.iter().enumerate().map(|(i, b)| page(format!("BOX {}", i + 1), pokebox::STORAGE_SLOTS, b)).collect())
}

fn with_change<T>(c: &PokeChange, f: impl FnOnce(&Change) -> T) -> T {
    let u = |x: u32| x as usize;
    match c {
        PokeChange::ToStorage { rom_path, save_path, from_box, slot, to_box } => f(&Change::ToStorage {
            rom: Path::new(rom_path), save: Path::new(save_path), b: u(*from_box), slot: u(*slot), to_box: u(*to_box),
        }),
        PokeChange::ToGame { rom_path, save_path, from_box, slot, to_box } => f(&Change::ToGame {
            rom: Path::new(rom_path), save: Path::new(save_path), b: u(*from_box), slot: u(*slot), to_box: u(*to_box),
        }),
        PokeChange::InStorage { from_box, slot, to_box } => f(&Change::InStorage { b: u(*from_box), slot: u(*slot), to_box: u(*to_box) }),
        PokeChange::InStorageTo { from_box, slot, to_box, to_slot } => f(&Change::InStorageTo {
            b: u(*from_box), slot: u(*slot), to_box: u(*to_box), to_slot: u(*to_slot),
        }),
        PokeChange::PartyToStorage { rom_path, save_path, slot, to_box } => f(&Change::PartyToStorage {
            rom: Path::new(rom_path), save: Path::new(save_path), slot: u(*slot), to_box: u(*to_box),
        }),
        PokeChange::StorageToParty { rom_path, save_path, from_box, slot } => f(&Change::StorageToParty {
            rom: Path::new(rom_path), save: Path::new(save_path), b: u(*from_box), slot: u(*slot),
        }),
        PokeChange::Sort { box_index, by } => f(&Change::Sort {
            b: u(*box_index),
            by: match by {
                1 => pokebox::Order::Level,
                2 => pokebox::Order::Name,
                _ => pokebox::Order::Number,
            },
        }),
        PokeChange::Trade { a_rom_path, a_save_path, a_box, a_slot, b_rom_path, b_save_path, b_box, b_slot } => f(&Change::Trade {
            a: Side { rom: Path::new(a_rom_path), save: Path::new(a_save_path), b: u(*a_box), slot: u(*a_slot) },
            b: Side { rom: Path::new(b_rom_path), save: Path::new(b_save_path), b: u(*b_box), slot: u(*b_slot) },
        }),
    }
}

fn plan_record(p: pokebox::Plan) -> PokePlan {
    PokePlan { allowed: p.allowed, title: p.title, changes: p.changes }
}

/// What a change would do, without doing it.
#[uniffi::export]
pub fn pokebox_plan(dir: String, change: PokeChange) -> PokePlan {
    with_change(&change, |c| plan_record(pokebox::plan(Path::new(&dir), c)))
}

/// Makes the change (journaled, saves backed up first).
#[uniffi::export]
pub fn pokebox_apply(dir: String, change: PokeChange) -> Result<PokePlan, EngineError> {
    with_change(&change, |c| pokebox::apply(Path::new(&dir), c).map(plan_record).map_err(other))
}
