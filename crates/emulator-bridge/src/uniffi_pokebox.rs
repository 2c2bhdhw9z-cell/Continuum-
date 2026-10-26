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
    /// The ball it is in (Ruby to LeafGreen), "" otherwise.
    pub ball: String,
    /// Whether it can hold an item (Gold on, not an egg).
    pub can_hold: bool,
}

/// An item: PokeAPI's number, its name, how many, and its pocket ("" in the locker).
#[derive(Debug, Clone, uniffi::Record)]
pub struct PokeItem {
    pub item: u16,
    pub name: String,
    pub count: u32,
    pub pocket: String,
    pub is_ball: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PokeGift {
    pub now: u32,
    pub peak: u32,
    pub claimed: u8,
    /// The next egg's Pokémon ("" when all four are had) and how many it needs.
    pub next_species: String,
    pub next_need: u32,
    pub ready: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PokeEvent {
    pub id: String,
    pub name: String,
    pub place: String,
    pub have: bool,
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
    /// Items (PokeAPI numbers, as `PokeItem.item`).
    Deposit { rom_path: String, save_path: String, item: u16, count: u16 },
    Withdraw { rom_path: String, save_path: String, item: u16, count: u16 },
    GiveHeld { from_box: u32, slot: u32, item: u16 },
    TakeHeld { from_box: u32, slot: u32 },
    SwapBall { from_box: u32, slot: u32, ball: u16 },
    ClaimGift { rom_path: String, save_path: String, to_box: u32 },
    ReceiveEvent { rom_path: String, save_path: String, id: String },
    /// `to`: a game's name ("Emerald"); `nature`: 0-24 (`pokebox_natures`), for Ruby to LeafGreen.
    Migrate { from_box: u32, slot: u32, to: String, nature: Option<u8> },
    Restore { from_box: u32, slot: u32, index: u32 },
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
            ball: pokebox::items::ball(s).filter(|b| *b > 0).map(|b| mon::item_name(mon::item_from_game(3, b)).to_owned()).unwrap_or_default(),
            can_hold: pokebox::items::held(s).is_some(),
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
            ball: String::new(),
            can_hold: false,
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

/// A Pokémon file (.pk1/.pk2/.pk3) of the Box Pokémon in `from_box`, `slot`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PokeFile {
    pub file_name: String,
    pub bytes: Vec<u8>,
}

#[uniffi::export]
pub fn pokebox_export(dir: String, from_box: u32, slot: u32) -> Result<PokeFile, EngineError> {
    pokebox::export(Path::new(&dir), from_box as usize, slot as usize)
        .map(|(file_name, bytes)| PokeFile { file_name, bytes })
        .map_err(other)
}

/// A Pokémon file into the Box.
#[uniffi::export]
pub fn pokebox_import(dir: String, bytes: Vec<u8>, to_box: u32) -> Result<PokePlan, EngineError> {
    pokebox::import(Path::new(&dir), &bytes, to_box as usize).map(plan_record).map_err(other)
}

/// The items in a game's bag that can go in the locker.
#[uniffi::export]
pub fn pokebox_read_bag(rom_path: String, save_path: String) -> Result<Vec<PokeItem>, EngineError> {
    let g = pokebox::open_game(Path::new(&rom_path), Path::new(&save_path)).map_err(other)?;
    Ok(pokebox::items::read_bag(g.game, &g.bytes).map_err(other)?.into_iter().map(|b| PokeItem {
        item: b.item, name: mon::item_name(b.item).to_owned(), count: u32::from(b.count), pocket: b.pocket.to_owned(),
        is_ball: mon::item_pocket(b.item) == 3,
    }).collect())
}

/// The Box's item locker.
#[uniffi::export]
pub fn pokebox_read_locker(dir: String) -> Result<Vec<PokeItem>, EngineError> {
    let st = pokebox::Storage::load(Path::new(&dir)).map_err(other)?;
    Ok(st.items.iter().map(|(i, n)| PokeItem {
        item: *i, name: mon::item_name(*i).to_owned(), count: *n, pocket: String::new(), is_ball: mon::item_pocket(*i) == 3,
    }).collect())
}

/// Where a Ruby-to-LeafGreen trainer stands with the Box's gift eggs.
#[uniffi::export]
pub fn pokebox_gift_progress(dir: String, rom_path: String, save_path: String) -> Result<PokeGift, EngineError> {
    let g = pokebox::open_game(Path::new(&rom_path), Path::new(&save_path)).map_err(other)?;
    let st = pokebox::Storage::load(Path::new(&dir)).map_err(other)?;
    let p = pokebox::gifts::progress(&st, g.game, &g.bytes).map_err(other)?;
    Ok(PokeGift {
        now: p.now,
        peak: p.peak,
        claimed: p.claimed,
        next_species: p.next.map(|(sp, _)| mon::species_name(sp).to_owned()).unwrap_or_default(),
        next_need: p.next.map_or(0, |(_, n)| n),
        ready: p.next.is_some_and(|(_, n)| p.peak >= n),
    })
}

/// The event tickets this game and language really had, each with whether the save has it; and
/// where the tickets are used (the last entry's `place`, with an empty id).
#[uniffi::export]
pub fn pokebox_events(rom_path: String, save_path: String) -> Result<Vec<PokeEvent>, EngineError> {
    let g = pokebox::open_game(Path::new(&rom_path), Path::new(&save_path)).map_err(other)?;
    let mut out: Vec<PokeEvent> = pokebox::events::list(g.game, g.language, &g.bytes).map_err(other)?.into_iter()
        .map(|(e, have)| PokeEvent { id: e.id.into(), name: e.name.into(), place: e.place.into(), have })
        .collect();
    out.push(PokeEvent { id: String::new(), name: String::new(), place: pokebox::events::how(g.game).into(), have: false });
    Ok(out)
}

/// The forms kept of a Box Pokémon by Migration, oldest first.
#[uniffi::export]
pub fn pokebox_archived(dir: String, from_box: u32, slot: u32) -> Result<Vec<PokeMon>, EngineError> {
    Ok(pokebox::archived(Path::new(&dir), from_box as usize, slot as usize).map_err(other)?.iter().map(shown).collect())
}

/// The 25 Natures, in their numbers' order.
#[uniffi::export]
pub fn pokebox_natures() -> Vec<String> {
    mon::NATURES.iter().map(|n| (*n).to_owned()).collect()
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
        PokeChange::Deposit { rom_path, save_path, item, count } => f(&Change::Deposit { rom: Path::new(rom_path), save: Path::new(save_path), item: *item, count: *count }),
        PokeChange::Withdraw { rom_path, save_path, item, count } => f(&Change::Withdraw { rom: Path::new(rom_path), save: Path::new(save_path), item: *item, count: *count }),
        PokeChange::GiveHeld { from_box, slot, item } => f(&Change::GiveHeld { b: u(*from_box), slot: u(*slot), item: *item }),
        PokeChange::TakeHeld { from_box, slot } => f(&Change::TakeHeld { b: u(*from_box), slot: u(*slot) }),
        PokeChange::SwapBall { from_box, slot, ball } => f(&Change::SwapBall { b: u(*from_box), slot: u(*slot), ball: *ball }),
        PokeChange::ClaimGift { rom_path, save_path, to_box } => f(&Change::ClaimGift { rom: Path::new(rom_path), save: Path::new(save_path), to_box: u(*to_box) }),
        PokeChange::ReceiveEvent { rom_path, save_path, id } => f(&Change::ReceiveEvent { rom: Path::new(rom_path), save: Path::new(save_path), id }),
        PokeChange::Migrate { from_box, slot, to, nature } => {
            let game = mon::Game::ALL.iter().copied().find(|g| g.name() == to).unwrap_or(mon::Game::Emerald);
            f(&Change::Migrate { b: u(*from_box), slot: u(*slot), to: game, nature: *nature })
        }
        PokeChange::Restore { from_box, slot, index } => f(&Change::Restore { b: u(*from_box), slot: u(*slot), index: u(*index) }),
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
