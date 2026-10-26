//! One Pokémon: as each generation stores it in a PC box, and moved from one generation's way to
//! another's. Written from Bulbapedia's "Pokémon data structure (Generation I)", "(Generation II)"
//! and "(Generation III)" with its "substructures" page.
//!
//! A Pokémon moving between games of the same generation is copied byte for byte. Between
//! generations it goes through [`Mon`], what all three have in common, and every difference the
//! player should know about comes back as a line of plain English for the preview.

use super::data::{BASE_STATS, EXPERIENCE, ITEMS_GEN2, ITEMS_GEN3, MOVES, SPECIES};
use super::text;

/// The games the Pokémon Box knows (English and European cartridges).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Game {
    Red,
    Blue,
    Yellow,
    Gold,
    Silver,
    Crystal,
    Ruby,
    Sapphire,
    Emerald,
    FireRed,
    LeafGreen,
}

impl Game {
    pub const ALL: [Game; 11] = [
        Game::Red, Game::Blue, Game::Yellow, Game::Gold, Game::Silver, Game::Crystal, Game::Ruby,
        Game::Sapphire, Game::Emerald, Game::FireRed, Game::LeafGreen,
    ];

    pub fn gen(self) -> u8 {
        match self {
            Game::Red | Game::Blue | Game::Yellow => 1,
            Game::Gold | Game::Silver | Game::Crystal => 2,
            _ => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Game::Red => "Red",
            Game::Blue => "Blue",
            Game::Yellow => "Yellow",
            Game::Gold => "Gold",
            Game::Silver => "Silver",
            Game::Crystal => "Crystal",
            Game::Ruby => "Ruby",
            Game::Sapphire => "Sapphire",
            Game::Emerald => "Emerald",
            Game::FireRed => "FireRed",
            Game::LeafGreen => "LeafGreen",
        }
    }

    /// Its number in a stored Pokémon (the Box file).
    pub fn code(self) -> u8 {
        Game::ALL.iter().position(|g| *g == self).unwrap_or(0) as u8
    }

    pub fn from_code(c: u8) -> Option<Game> {
        Game::ALL.get(usize::from(c)).copied()
    }

    /// The "game of origin" number Ruby to LeafGreen write for a Pokémon caught in them.
    fn origin_code(self) -> u16 {
        match self {
            Game::Sapphire => 1,
            Game::Ruby => 2,
            Game::Emerald => 3,
            Game::FireRed => 4,
            Game::LeafGreen => 5,
            _ => 0,
        }
    }

    fn from_origin_code(c: u16) -> Option<Game> {
        Some(match c {
            1 => Game::Sapphire,
            2 => Game::Ruby,
            3 => Game::Emerald,
            4 => Game::FireRed,
            5 => Game::LeafGreen,
            _ => return None,
        })
    }

    /// The highest Pokédex number and move number the generation has.
    pub fn last_species(self) -> u16 {
        match self.gen() {
            1 => 151,
            2 => 251,
            _ => 386,
        }
    }

    pub fn last_move(self) -> u16 {
        match self.gen() {
            1 => 165,
            2 => 251,
            _ => 354,
        }
    }
}

/// A Pokémon as a PC box keeps it, in the generation of `game`: Red to Crystal, its 33 or 32
/// bytes then the trainer's name and its own (11 bytes each); Ruby to LeafGreen, its 80 bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
    pub game: Game,
    /// Gold to Crystal mark an egg in the box's list, not in the Pokémon.
    pub egg: bool,
    pub data: Vec<u8>,
}

pub const GEN1_LEN: usize = 33;
pub const GEN2_LEN: usize = 32;
pub const GEN3_LEN: usize = 80;
pub const NAME_LEN: usize = 11;

/// What every generation's Pokémon has, enough to make it again in another's.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Mon {
    pub gen: u8,
    pub species: u16,
    pub nickname: String,
    pub nicknamed: bool,
    pub ot_name: String,
    pub ot_id: u16,
    pub ot_secret: u16,
    pub ot_female: bool,
    pub exp: u32,
    pub level: u8,
    pub moves: [u16; 4],
    pub pp_ups: [u8; 4],
    /// PokeAPI's item number; 0 for none.
    pub item: u16,
    pub friendship: u8,
    pub pokerus: u8,
    pub egg: bool,
    /// Red to Crystal: Attack, Defense, Speed and Special, 0-15.
    pub dvs: [u8; 4],
    pub stat_exp: [u16; 5],
    /// Ruby to LeafGreen.
    pub pid: u32,
    /// HP, Attack, Defense, Speed, Sp. Atk, Sp. Def, 0-31.
    pub ivs: [u8; 6],
    pub evs: [u8; 6],
    pub language: u8,
    /// Ruby to LeafGreen: the game it was caught in, and its ribbons (a count, for the preview).
    pub origin: Option<Game>,
    pub ribbons: u32,
}

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from(b[at]) << 8 | u16::from(b[at + 1])
}

fn put_be16(b: &mut [u8], at: usize, v: u16) {
    b[at] = (v >> 8) as u8;
    b[at + 1] = v as u8;
}

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from(b[at]) | u16::from(b[at + 1]) << 8
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from(le16(b, at)) | u32::from(le16(b, at + 2)) << 16
}

fn put_le16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn put_le32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

pub fn species_name(dex: u16) -> &'static str {
    SPECIES.get(usize::from(dex)).map_or("?", |s| if s.name.is_empty() { "?" } else { s.name })
}

pub fn move_name(m: u16) -> &'static str {
    MOVES.get(usize::from(m)).map_or("?", |(n, _)| n)
}

/// The Pokédex number of Red's own species number.
pub fn gen1_dex(index: u8) -> u16 {
    SPECIES.iter().position(|s| s.gen1_index == index && index != 0).unwrap_or(0) as u16
}

pub fn gen3_dex(index: u16) -> u16 {
    if index == 0 {
        return 0;
    }
    SPECIES.iter().position(|s| s.gen3_index == index).unwrap_or(0) as u16
}

/// An item's PokeAPI number from its number in Gold to Crystal (`gen` 2) or Ruby to LeafGreen (3).
pub fn item_from_game(gen: u8, index: u16) -> u16 {
    if index == 0 {
        return 0;
    }
    let list: &[(u16, u16, &str)] = if gen == 2 { &ITEMS_GEN2 } else { &ITEMS_GEN3 };
    list.iter().find(|(g, _, _)| *g == index).map_or(0, |(_, id, _)| *id)
}

pub fn item_to_game(gen: u8, item: u16) -> Option<u16> {
    if item == 0 {
        return Some(0);
    }
    let list: &[(u16, u16, &str)] = match gen {
        2 => &ITEMS_GEN2,
        3 => &ITEMS_GEN3,
        _ => return None,
    };
    list.iter().find(|(_, id, _)| *id == item).map(|(g, _, _)| *g)
}

pub fn item_name(item: u16) -> &'static str {
    ITEMS_GEN3.iter().chain(ITEMS_GEN2.iter()).find(|(_, id, _)| *id == item).map_or("an item", |(_, _, n)| n)
}

pub fn level_for(species: u16, exp: u32) -> u8 {
    let g = usize::from(SPECIES.get(usize::from(species)).map_or(2, |s| s.growth));
    let table = &EXPERIENCE[g.min(6)];
    (1..=100u8).rev().find(|l| table[usize::from(*l)] <= exp).unwrap_or(1)
}

fn exp_cap(species: u16) -> u32 {
    let g = usize::from(SPECIES.get(usize::from(species)).map_or(2, |s| s.growth));
    EXPERIENCE[g.min(6)][100]
}

/// PP with `ups` PP Ups, as Red to Emerald work it out (Red to Crystal stop at 61).
fn full_pp(m: u16, gen: u8, ups: u8) -> u8 {
    let base = u32::from(MOVES.get(usize::from(m)).map_or(0, |(_, pp)| pp[usize::from(gen - 1)]));
    let pp = base + base / 5 * u32::from(ups);
    if gen < 3 { pp.min(61) as u8 } else { pp.min(255) as u8 }
}

/// Gold and Silver's gender rule: female when the Attack DV is at most this (None: one gender
/// only, or neither).
fn gen2_female_max(species: u16) -> Option<u8> {
    match SPECIES.get(usize::from(species)).map_or(-1, |s| s.female_eighths) {
        1 => Some(1),
        2 => Some(3),
        4 => Some(7),
        6 => Some(11),
        _ => None,
    }
}

/// Ruby's gender rule: female when the personality's low byte is below this.
fn gen3_female_below(species: u16) -> Option<u32> {
    match SPECIES.get(usize::from(species)).map_or(-1, |s| s.female_eighths) {
        1 => Some(31),
        2 => Some(63),
        4 => Some(127),
        6 => Some(191),
        _ => None,
    }
}

/// "♂", "♀" or "" (one with no gender).
pub fn gender_of(m: &Mon) -> &'static str {
    match SPECIES.get(usize::from(m.species)).map_or(-1, |s| s.female_eighths) {
        -1 => "",
        0 => "♂",
        8 => "♀",
        _ => {
            let female = if m.gen == 3 {
                gen3_female_below(m.species).is_some_and(|t| m.pid & 0xFF < t)
            } else {
                gen2_female_max(m.species).is_some_and(|t| m.dvs[0] <= t)
            };
            if female { "♀" } else { "♂" }
        }
    }
}

fn gen2_shiny(d: [u8; 4]) -> bool {
    d[1] == 10 && d[2] == 10 && d[3] == 10 && [2, 3, 6, 7, 10, 11, 14, 15].contains(&d[0])
}

fn gen3_shiny(pid: u32, id: u16, secret: u16) -> bool {
    (u32::from(id) ^ u32::from(secret) ^ (pid >> 16) ^ (pid & 0xFFFF)) < 8
}

pub fn shiny(m: &Mon) -> bool {
    if m.gen == 3 { gen3_shiny(m.pid, m.ot_id, m.ot_secret) } else { m.gen == 2 && gen2_shiny(m.dvs) }
}

fn unown_gen2(d: [u8; 4]) -> u8 {
    ((((d[0] & 6) << 5) | ((d[1] & 6) << 3) | ((d[2] & 6) << 1) | ((d[3] & 6) >> 1)) / 10) as u8
}

fn unown_gen3(pid: u32) -> u8 {
    let v = ((pid >> 24) & 3) << 6 | ((pid >> 16) & 3) << 4 | ((pid >> 8) & 3) << 2 | (pid & 3);
    (v % 28) as u8
}

const NATURES: [&str; 25] = [
    "Hardy", "Lonely", "Brave", "Adamant", "Naughty", "Bold", "Docile", "Relaxed", "Impish", "Lax",
    "Timid", "Hasty", "Serious", "Jolly", "Naive", "Modest", "Mild", "Quiet", "Bashful", "Rash",
    "Calm", "Gentle", "Sassy", "Careful", "Quirky",
];

/// A Ruby to LeafGreen Pokémon's Nature ("" before them).
pub fn nature_of(m: &Mon) -> &'static str {
    if m.gen == 3 { NATURES[(m.pid % 25) as usize] } else { "" }
}

/// Red's types to its own type numbers (PokeAPI numbers in).
fn gen1_type(t: u8) -> u8 {
    match t {
        1 => 0x00,
        2 => 0x01,
        3 => 0x02,
        4 => 0x03,
        5 => 0x04,
        6 => 0x05,
        7 => 0x07,
        8 => 0x08,
        10 => 0x14,
        11 => 0x15,
        12 => 0x16,
        13 => 0x17,
        14 => 0x18,
        15 => 0x19,
        16 => 0x1A,
        _ => 0x00,
    }
}

// --- reading -------------------------------------------------------------------------------------

impl Stored {
    pub fn nickname_bytes(&self) -> Option<&[u8]> {
        match self.game.gen() {
            1 => self.data.get(GEN1_LEN + NAME_LEN..GEN1_LEN + 2 * NAME_LEN),
            2 => self.data.get(GEN2_LEN + NAME_LEN..GEN2_LEN + 2 * NAME_LEN),
            _ => None,
        }
    }

    /// The Pokémon in it, or None when its data does not add up (a "Bad Egg").
    pub fn mon(&self) -> Option<Mon> {
        match self.game.gen() {
            1 => decode_gen1(self),
            2 => decode_gen2(self),
            _ => decode_gen3(self),
        }
    }
}

fn pp_fields(b: &[u8]) -> [u8; 4] {
    [b[0] >> 6, b[1] >> 6, b[2] >> 6, b[3] >> 6]
}

fn decode_gen1(s: &Stored) -> Option<Mon> {
    let d = &s.data;
    if d.len() < GEN1_LEN + 2 * NAME_LEN {
        return None;
    }
    let species = gen1_dex(d[0]);
    if species == 0 {
        return None;
    }
    let nick = &d[GEN1_LEN + NAME_LEN..GEN1_LEN + 2 * NAME_LEN];
    let default = text::gb_encode(&text::default_name(species, 1), NAME_LEN).0;
    let exp = u32::from(d[14]) << 16 | u32::from(d[15]) << 8 | u32::from(d[16]);
    Some(Mon {
        gen: 1,
        species,
        nickname: text::gb_decode(nick),
        nicknamed: nick != default.as_slice(),
        ot_name: text::gb_decode(&d[GEN1_LEN..GEN1_LEN + NAME_LEN]),
        ot_id: be16(d, 12),
        exp,
        level: level_for(species, exp),
        moves: [d[8], d[9], d[10], d[11]].map(u16::from),
        pp_ups: pp_fields(&d[29..33]),
        dvs: [d[27] >> 4, d[27] & 15, d[28] >> 4, d[28] & 15],
        stat_exp: [be16(d, 17), be16(d, 19), be16(d, 21), be16(d, 23), be16(d, 25)],
        language: 2,
        ..Mon::default()
    })
}

fn decode_gen2(s: &Stored) -> Option<Mon> {
    let d = &s.data;
    if d.len() < GEN2_LEN + 2 * NAME_LEN || d[0] == 0 || u16::from(d[0]) > 251 {
        return None;
    }
    let species = u16::from(d[0]);
    let nick = &d[GEN2_LEN + NAME_LEN..GEN2_LEN + 2 * NAME_LEN];
    let default = text::gb_encode(&text::default_name(species, 2), NAME_LEN).0;
    let exp = u32::from(d[8]) << 16 | u32::from(d[9]) << 8 | u32::from(d[10]);
    Some(Mon {
        gen: 2,
        species,
        nickname: text::gb_decode(nick),
        nicknamed: !s.egg && nick != default.as_slice(),
        ot_name: text::gb_decode(&d[GEN2_LEN..GEN2_LEN + NAME_LEN]),
        ot_id: be16(d, 6),
        ot_female: s.game == Game::Crystal && d[30] & 0x80 != 0,
        exp,
        level: level_for(species, exp),
        moves: [d[2], d[3], d[4], d[5]].map(u16::from),
        pp_ups: pp_fields(&d[23..27]),
        item: item_from_game(2, u16::from(d[1])),
        friendship: d[27],
        pokerus: d[28],
        egg: s.egg,
        dvs: [d[21] >> 4, d[21] & 15, d[22] >> 4, d[22] & 15],
        stat_exp: [be16(d, 11), be16(d, 13), be16(d, 15), be16(d, 17), be16(d, 19)],
        language: 2,
        ..Mon::default()
    })
}

/// Where each of the four 12-byte parts (Growth, Attacks, EVs, Misc) sits, by personality % 24.
fn gen3_order(pid: u32) -> [usize; 4] {
    const ORDERS: [&[u8; 4]; 24] = [
        b"GAEM", b"GAME", b"GEAM", b"GEMA", b"GMAE", b"GMEA", b"AGEM", b"AGME", b"AEGM", b"AEMG",
        b"AMGE", b"AMEG", b"EGAM", b"EGMA", b"EAGM", b"EAMG", b"EMGA", b"EMAG", b"MGAE", b"MGEA",
        b"MAGE", b"MAEG", b"MEGA", b"MEAG",
    ];
    let o = ORDERS[(pid % 24) as usize];
    let at = |c: u8| o.iter().position(|x| *x == c).unwrap_or(0);
    [at(b'G'), at(b'A'), at(b'E'), at(b'M')]
}

/// The 48 bytes of parts, unscrambled into Growth, Attacks, EVs, Misc order and decrypted;
/// None when the checksum does not match.
pub fn gen3_parts(d: &[u8]) -> Option<[u8; 48]> {
    if d.len() < GEN3_LEN {
        return None;
    }
    let pid = le32(d, 0);
    let key = pid ^ le32(d, 4);
    let mut plain = [0u8; 48];
    for i in (0..48).step_by(4) {
        put_le32(&mut plain, i, le32(d, 32 + i) ^ key);
    }
    let sum = (0..48).step_by(2).fold(0u16, |a, i| a.wrapping_add(le16(&plain, i)));
    if sum != le16(d, 28) {
        return None;
    }
    let order = gen3_order(pid);
    let mut out = [0u8; 48];
    for (part, pos) in order.iter().enumerate() {
        out[part * 12..part * 12 + 12].copy_from_slice(&plain[pos * 12..pos * 12 + 12]);
    }
    Some(out)
}

/// Scrambles, encrypts and checksums `parts` (Growth, Attacks, EVs, Misc) into `d`.
pub fn gen3_seal(d: &mut [u8], parts: &[u8; 48]) {
    let pid = le32(d, 0);
    let key = pid ^ le32(d, 4);
    let order = gen3_order(pid);
    let mut plain = [0u8; 48];
    for (part, pos) in order.iter().enumerate() {
        plain[pos * 12..pos * 12 + 12].copy_from_slice(&parts[part * 12..part * 12 + 12]);
    }
    let sum = (0..48).step_by(2).fold(0u16, |a, i| a.wrapping_add(le16(&plain, i)));
    put_le16(d, 28, sum);
    for i in (0..48).step_by(4) {
        put_le32(d, 32 + i, le32(&plain, i) ^ key);
    }
}

fn decode_gen3(s: &Stored) -> Option<Mon> {
    let d = &s.data;
    let p = gen3_parts(d)?;
    let species = gen3_dex(le16(&p, 0));
    if species == 0 {
        return None;
    }
    let ivw = le32(&p, 36 + 4);
    let origins = le16(&p, 36 + 2);
    let exp = le32(&p, 4);
    let nick = text::gba_decode(&d[8..18]);
    let egg = ivw & 1 << 30 != 0;
    Some(Mon {
        gen: 3,
        species,
        nicknamed: !egg && nick != text::default_name(species, 3),
        nickname: nick,
        ot_name: text::gba_decode(&d[20..27]),
        ot_id: le16(d, 4),
        ot_secret: le16(d, 6),
        ot_female: origins & 0x8000 != 0,
        exp,
        level: level_for(species, exp),
        moves: [le16(&p, 12), le16(&p, 14), le16(&p, 16), le16(&p, 18)],
        pp_ups: [p[8] & 3, p[8] >> 2 & 3, p[8] >> 4 & 3, p[8] >> 6 & 3],
        item: item_from_game(3, le16(&p, 2)),
        friendship: p[9],
        pokerus: p[36],
        egg,
        pid: le32(d, 0),
        ivs: [0, 5, 10, 15, 20, 25].map(|sh| (ivw >> sh & 31) as u8),
        evs: [p[24], p[25], p[26], p[27], p[28], p[29]],
        language: d[18],
        origin: Game::from_origin_code(origins >> 7 & 15),
        ribbons: (le32(&p, 44) & 0x7FFF_FFFF).count_ones(),
        ..Mon::default()
    })
}

// --- moving between generations ------------------------------------------------------------------

/// What the move will do to it, or why it cannot go.
pub type Converted = Result<(Stored, Vec<String>), String>;

/// `s`, made into a Pokémon of `target`'s generation. `language` is the target game's (Ruby to
/// LeafGreen write it in each Pokémon).
pub fn convert(s: &Stored, target: Game, language: u8) -> Converted {
    if s.game.gen() == target.gen() {
        let mut out = s.clone();
        out.game = target;
        return Ok((out, vec![format!("Nothing about it changes: {} and {} are the same generation.", s.game.name(), target.name())]));
    }
    let m = s.mon().ok_or("This one's data is damaged (a Bad Egg), so it can't move.")?;
    let name = if m.nicknamed { m.nickname.clone() } else { species_name(m.species).to_owned() };
    if m.egg {
        return Err("Eggs can only move between games of the same generation. Hatch it first.".into());
    }
    if m.species > target.last_species() {
        return Err(format!("{} isn't in {}: that generation's games only have the first {} Pokémon.",
                           species_name(m.species), target.name(), target.last_species()));
    }
    let mut changes = Vec::new();
    let mut moves: Vec<(u16, u8)> = Vec::new();
    for (i, mv) in m.moves.iter().enumerate() {
        if *mv == 0 {
            continue;
        }
        if *mv > target.last_move() {
            changes.push(format!("It forgets {}: {} doesn't have that move.", move_name(*mv), target.name()));
        } else {
            moves.push((*mv, m.pp_ups[i]));
        }
    }
    if moves.is_empty() {
        return Err(format!("None of {name}'s moves exist in {}, and a Pokémon can't have no moves. Teach it an older move first.", target.name()));
    }
    let item = match target.gen() {
        1 => None,
        g => item_to_game(g, m.item),
    };
    if m.item != 0 && item.is_none() {
        changes.push(format!("Its held {} is left behind and lost: {} can't hold it. Take it off first to keep it.",
                             item_name(m.item), target.name()));
    }
    let exp = m.exp.min(exp_cap(m.species));
    let level = level_for(m.species, exp);
    let mut out = Mon { moves: [0; 4], pp_ups: [0; 4], item: item.map_or(0, |_| m.item), exp, level, ..m.clone() };
    for (i, (mv, ups)) in moves.iter().enumerate() {
        out.moves[i] = *mv;
        out.pp_ups[i] = *ups;
    }
    if m.gen < 3 && target.gen() < 3 {
        // Red <-> Gold: the Time Capsule's way. The strengths and training carry over.
        if target.gen() == 2 {
            out.friendship = SPECIES[usize::from(m.species)].base_friendship;
            changes.push("It starts with the friendship a newly caught one has.".into());
        }
    } else if target.gen() == 3 {
        to_gen3(&mut out, &m, &mut changes);
    } else {
        from_gen3(&mut out, &m, target, &mut changes);
    }
    if m.gen == 2 && target.gen() == 1 && m.pokerus != 0 {
        changes.push("Pokérus doesn't exist in Red, Blue and Yellow, so it's gone.".into());
    }
    let (stored, exact) = encode(&out, target, language);
    if !exact {
        changes.push("A letter in its name or its trainer's name doesn't exist in that game and becomes \"?\".".into());
    }
    if m.level != level {
        changes.push(format!("Its level is worked out again from its experience: level {level}."));
    }
    Ok((stored, changes))
}

/// A tiny repeatable random number source, seeded from the Pokémon, so the same Pokémon always
/// gets the same personality.
struct Xorshift(u32);

impl Xorshift {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
}

fn to_gen3(out: &mut Mon, m: &Mon, changes: &mut Vec<String>) {
    let d = m.dvs;
    let hp = (d[0] & 1) << 3 | (d[1] & 1) << 2 | (d[2] & 1) << 1 | (d[3] & 1);
    out.ivs = [hp, d[0], d[1], d[2], d[3], d[3]].map(|v| v * 2 + 1);
    out.evs = [0; 6];
    out.ot_secret = 0;
    out.gen = 3;
    let female = gender_of(m) == "♀";
    let want_shiny = shiny(m);
    let nature = (m.exp % 25) as u8;
    let letter = (m.species == 201).then(|| unown_gen2(m.dvs));
    let mut rng = Xorshift(m.exp ^ u32::from(m.ot_id) << 16 ^ u32::from(m.species) ^ 0x9E37_79B9 | 1);
    let fits = |pid: u32| {
        (pid % 25) as u8 == nature
            && gen3_female_below(m.species).is_none_or(|t| (pid & 0xFF < t) == female)
            && letter.is_none_or(|l| unown_gen3(pid) == l)
    };
    let mut pid = 0;
    'search: for _ in 0..2_000_000 {
        let hi = rng.next() & 0xFFFF;
        if want_shiny {
            for k in 0..8 {
                let lo = u32::from(m.ot_id) ^ hi ^ k;
                let p = hi << 16 | lo;
                if fits(p) {
                    pid = p;
                    break 'search;
                }
            }
        } else {
            let p = hi << 16 | (rng.next() & 0xFFFF);
            if !gen3_shiny(p, m.ot_id, 0) && fits(p) {
                pid = p;
                break 'search;
            }
        }
    }
    out.pid = pid;
    out.friendship = if m.gen == 1 { SPECIES[usize::from(m.species)].base_friendship } else { m.friendship };
    changes.push(format!("It gets a Nature ({}) and an Ability, which Red to Crystal don't have.", NATURES[usize::from(nature)]));
    changes.push("Its strengths (DVs) become IVs at about the same level; its training (stat experience) starts again from zero.".into());
    if want_shiny {
        changes.push("It's shiny and stays shiny.".into());
    }
    if m.gen == 1 {
        changes.push("It starts with the friendship a newly caught one has.".into());
    }
    changes.push("Its summary will say it was met in a trade.".into());
}

fn from_gen3(out: &mut Mon, m: &Mon, target: Game, changes: &mut Vec<String>) {
    let want = [m.ivs[1] / 2, m.ivs[2] / 2, m.ivs[3] / 2, m.ivs[4].max(m.ivs[5]) / 2];
    let want_shiny = shiny(m);
    let female = gender_of(m) == "♀";
    let letter = (m.species == 201).then(|| unown_gen3(m.pid)).filter(|l| *l < 26);
    let female_max = gen2_female_max(m.species);
    // The DVs nearest its IVs that keep it shiny (or not), its gender and its Unown letter.
    let mut best: Option<([u8; 4], u32, bool)> = None;
    for code in 0u32..65536 {
        let d = [(code >> 12) as u8 & 15, (code >> 8) as u8 & 15, (code >> 4) as u8 & 15, code as u8 & 15];
        if gen2_shiny(d) != want_shiny || letter.is_some_and(|l| unown_gen2(d) != l) {
            continue;
        }
        let gender_ok = female_max.is_none_or(|t| (d[0] <= t) == female);
        let dist: u32 = d.iter().zip(want).map(|(a, b)| u32::from(a.abs_diff(b))).sum();
        let better = match best {
            None => true,
            Some((_, bd, bg)) => (gender_ok && !bg) || (gender_ok == bg && dist < bd),
        };
        if better {
            best = Some((d, dist, gender_ok));
        }
    }
    let (dvs, _, gender_ok) = best.unwrap_or((want, 0, true));
    out.dvs = dvs;
    out.stat_exp = [0; 5];
    out.gen = target.gen();
    changes.push(format!("It loses its Nature ({}), its Ability{}, its effort points and its contest condition: {} doesn't have them.",
                         NATURES[(m.pid % 25) as usize],
                         if m.ribbons > 0 { format!(" and its {} ribbon{}", m.ribbons, if m.ribbons == 1 { "" } else { "s" }) } else { String::new() },
                         target.name()));
    changes.push("Its IVs become strengths (DVs) at about the same level.".into());
    if want_shiny {
        changes.push("It's shiny and stays shiny.".into());
    }
    if !gender_ok {
        changes.push("It can't keep its gender there and still be shiny, so its gender changes.".into());
    }
    if target.gen() == 1 && m.friendship != 0 {
        changes.push("Red, Blue and Yellow don't keep friendship, so it starts again if it comes back.".into());
    }
}

/// `m` as `game` stores it. Says whether every letter of its names could be written.
pub fn encode(m: &Mon, game: Game, language: u8) -> (Stored, bool) {
    // Its level is always the one its experience gives.
    let m = &Mon { level: level_for(m.species, m.exp), ..m.clone() };
    match game.gen() {
        1 | 2 => encode_gb(m, game),
        _ => encode_gen3(m, game, language),
    }
}

fn encode_gb(m: &Mon, game: Game) -> (Stored, bool) {
    let gen = game.gen();
    let len = if gen == 1 { GEN1_LEN } else { GEN2_LEN };
    let mut d = vec![0u8; len + 2 * NAME_LEN];
    let sp = &SPECIES[usize::from(m.species)];
    let mut pp = [0u8; 4];
    for i in 0..4 {
        if m.moves[i] != 0 {
            pp[i] = m.pp_ups[i] << 6 | full_pp(m.moves[i], gen, m.pp_ups[i]);
        }
    }
    let exp = m.exp.to_be_bytes();
    let dv = [m.dvs[0] << 4 | m.dvs[1], m.dvs[2] << 4 | m.dvs[3]];
    if gen == 1 {
        d[0] = sp.gen1_index;
        // The box part keeps the current health: full.
        put_be16(&mut d, 1, stats(&Mon { gen: 1, ..m.clone() })[0]);
        d[3] = m.level;
        d[5] = gen1_type(sp.gen1_types[0]);
        d[6] = gen1_type(sp.gen1_types[1]);
        d[7] = sp.catch_rate;
        for i in 0..4 {
            d[8 + i] = m.moves[i] as u8;
        }
        put_be16(&mut d, 12, m.ot_id);
        d[14..17].copy_from_slice(&exp[1..]);
        for i in 0..5 {
            put_be16(&mut d, 17 + 2 * i, m.stat_exp[i]);
        }
        d[27..29].copy_from_slice(&dv);
        d[29..33].copy_from_slice(&pp);
    } else {
        d[0] = m.species as u8;
        d[1] = item_to_game(2, m.item).unwrap_or(0) as u8;
        for i in 0..4 {
            d[2 + i] = m.moves[i] as u8;
        }
        put_be16(&mut d, 6, m.ot_id);
        d[8..11].copy_from_slice(&exp[1..]);
        for i in 0..5 {
            put_be16(&mut d, 11 + 2 * i, m.stat_exp[i]);
        }
        d[21..23].copy_from_slice(&dv);
        d[23..27].copy_from_slice(&pp);
        d[27] = m.friendship;
        d[28] = m.pokerus;
        if game == Game::Crystal && m.ot_female {
            d[30] = 0x80;
        }
        d[31] = m.level;
    }
    let (ot, a) = text::gb_encode(&m.ot_name, NAME_LEN);
    let nick = if m.nicknamed { m.nickname.clone() } else { text::default_name(m.species, gen) };
    let (nk, b) = text::gb_encode(&nick, NAME_LEN);
    d[len..len + NAME_LEN].copy_from_slice(&ot);
    d[len + NAME_LEN..].copy_from_slice(&nk);
    (Stored { game, egg: false, data: d }, a && b)
}

fn encode_gen3(m: &Mon, game: Game, language: u8) -> (Stored, bool) {
    let mut d = vec![0u8; GEN3_LEN];
    put_le32(&mut d, 0, m.pid);
    put_le16(&mut d, 4, m.ot_id);
    put_le16(&mut d, 6, m.ot_secret);
    let nick = if m.nicknamed { m.nickname.clone() } else { text::default_name(m.species, 3) };
    let (nk, a) = text::gba_encode(&nick, 10);
    d[8..18].copy_from_slice(&nk);
    d[18] = language;
    d[19] = 0x02;
    let (ot, b) = text::gba_encode(&m.ot_name, 7);
    d[20..27].copy_from_slice(&ot);
    let mut p = [0u8; 48];
    put_le16(&mut p, 0, SPECIES[usize::from(m.species)].gen3_index);
    put_le16(&mut p, 2, item_to_game(3, m.item).unwrap_or(0));
    put_le32(&mut p, 4, m.exp);
    p[8] = m.pp_ups[0] | m.pp_ups[1] << 2 | m.pp_ups[2] << 4 | m.pp_ups[3] << 6;
    p[9] = m.friendship;
    for i in 0..4 {
        put_le16(&mut p, 12 + 2 * i, m.moves[i]);
        p[20 + i] = if m.moves[i] == 0 { 0 } else { full_pp(m.moves[i], 3, m.pp_ups[i]) };
    }
    p[24..30].copy_from_slice(&m.evs);
    p[36] = m.pokerus;
    p[37] = 0xFE; // met in a trade
    let origins = u16::from(m.level.min(100)) | game.origin_code() << 7 | 4 << 11 | u16::from(m.ot_female) << 15;
    put_le16(&mut p, 38, origins);
    let mut ivw = 0u32;
    for (i, iv) in m.ivs.iter().enumerate() {
        ivw |= u32::from(*iv & 31) << (5 * i);
    }
    ivw |= (m.pid & 1) << 31;
    put_le32(&mut p, 40, ivw);
    gen3_seal(&mut d, &p);
    (Stored { game, egg: false, data: d }, a && b)
}

// --- stats, and a Pokémon as a team keeps it ---------------------------------------------------

/// Its stats as its game works them out: HP, Attack, Defense, Speed, Sp. Atk, Sp. Def (Red to
/// Yellow's Special is both of the last two).
pub fn stats(m: &Mon) -> [u16; 6] {
    let b = BASE_STATS.get(usize::from(m.species)).copied().unwrap_or([0; 7]);
    let l = u32::from(m.level.max(1));
    if m.gen == 3 {
        let n = (m.pid % 25) as usize;
        let calc = |i: usize, base: u8| (2 * u32::from(base) + u32::from(m.ivs[i]) + u32::from(m.evs[i]) / 4) * l / 100;
        let hp = if m.species == 292 { 1 } else { calc(0, b[0]) + l + 10 };
        let mut out = [hp as u16, 0, 0, 0, 0, 0];
        // Natures raise one of Attack, Defense, Speed, Sp. Atk, Sp. Def by a tenth and lower one.
        for i in 1..6 {
            let mut v = calc(i, b[i]) + 5;
            if n / 5 != n % 5 {
                if n / 5 == i - 1 {
                    v = v * 110 / 100;
                } else if n % 5 == i - 1 {
                    v = v * 90 / 100;
                }
            }
            out[i] = v as u16;
        }
        return out;
    }
    let d = m.dvs;
    let hp_dv = (d[0] & 1) << 3 | (d[1] & 1) << 2 | (d[2] & 1) << 1 | (d[3] & 1);
    // The games' square root of stat experience stops at 255.
    let se = |i: usize| (f64::from(m.stat_exp[i]).sqrt().ceil() as u32).min(255) / 4;
    let calc = |base: u8, dv: u8, i: usize| ((u32::from(base) + u32::from(dv)) * 2 + se(i)) * l / 100;
    let (spa, spd) = if m.gen == 1 { (b[6], b[6]) } else { (b[4], b[5]) };
    [
        (calc(b[0], hp_dv, 0) + l + 10) as u16,
        (calc(b[1], d[0], 1) + 5) as u16,
        (calc(b[2], d[1], 2) + 5) as u16,
        (calc(b[3], d[2], 3) + 5) as u16,
        (calc(spa, d[3], 4) + 5) as u16,
        (calc(spd, d[3], 4) + 5) as u16,
    ]
}

/// The bytes a team keeps after the box part: level and stats, health full, no status.
pub fn party_tail(s: &Stored) -> Option<Vec<u8>> {
    let m = s.mon()?;
    let st = stats(&m);
    let be = |v: u16| v.to_be_bytes();
    Some(match s.game.gen() {
        1 => {
            let mut t = vec![m.level];
            for v in [st[0], st[1], st[2], st[3], st[4]] {
                t.extend_from_slice(&be(v));
            }
            t
        }
        2 => {
            let mut t = vec![0, 0];
            t.extend_from_slice(&be(if m.egg { 0 } else { st[0] }));
            for v in st {
                t.extend_from_slice(&be(v));
            }
            t
        }
        _ => {
            let mut t = vec![0, 0, 0, 0, m.level, 0xFF];
            t.extend_from_slice(&st[0].to_le_bytes());
            for v in st {
                t.extend_from_slice(&v.to_le_bytes());
            }
            t
        }
    })
}

// --- trade evolutions ----------------------------------------------------------------------------

/// A real trade evolves some Pokémon (Kadabra, and those holding the right item). Applied to the
/// Pokémon as it arrives; returns the line for the preview.
pub fn trade_evolve(s: &mut Stored) -> Option<String> {
    let m = s.mon()?;
    if m.egg {
        return None;
    }
    let gen = s.game.gen();
    if gen > 1 && m.item == super::data::EVERSTONE {
        return Some(format!("{} would evolve, but its Everstone stops it.", species_name(m.species)));
    }
    let &(_, to, need) = super::data::TRADE_EVOLUTIONS.iter().find(|(from, to, item)| {
        *from == m.species && *to <= s.game.last_species()
            && (*item == 0 || (gen > 1 && m.item == *item && item_to_game(gen, *item).is_some()))
    })?;
    let rename = !m.nicknamed;
    let d = &mut s.data;
    match gen {
        1 => {
            let sp = &SPECIES[usize::from(to)];
            d[0] = sp.gen1_index;
            d[5] = gen1_type(sp.gen1_types[0]);
            d[6] = gen1_type(sp.gen1_types[1]);
            if rename {
                d[GEN1_LEN + NAME_LEN..].copy_from_slice(&text::gb_encode(&text::default_name(to, 1), NAME_LEN).0);
            }
        }
        2 => {
            d[0] = to as u8;
            if need != 0 {
                d[1] = 0;
            }
            if rename {
                d[GEN2_LEN + NAME_LEN..].copy_from_slice(&text::gb_encode(&text::default_name(to, 2), NAME_LEN).0);
            }
        }
        _ => {
            let mut p = gen3_parts(d)?;
            put_le16(&mut p, 0, SPECIES[usize::from(to)].gen3_index);
            if need != 0 {
                put_le16(&mut p, 2, 0);
            }
            gen3_seal(d, &p);
            if rename {
                d[8..18].copy_from_slice(&text::gba_encode(&text::default_name(to, 3), 10).0);
            }
        }
    }
    Some(format!("It evolves into {}, as in a real trade{}.", species_name(to),
                 if need != 0 { format!(" (its {} is used up)", item_name(need)) } else { String::new() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pikachu_gen1() -> Stored {
        let m = Mon {
            gen: 1, species: 25, ot_name: "ASH".into(), ot_id: 12345, exp: 1000, level: 10,
            moves: [84, 45, 0, 0], dvs: [10, 10, 10, 10], stat_exp: [500, 0, 0, 0, 0], ..Mon::default()
        };
        encode(&m, Game::Red, 2).0
    }

    #[test]
    fn red_to_crystal_and_back_keeps_everything_that_exists_in_both() {
        let red = pikachu_gen1();
        let back = red.mon().unwrap();
        assert_eq!((back.species, back.level, back.ot_id, back.nicknamed), (25, 10, 12345, false));
        assert_eq!(back.moves, [84, 45, 0, 0]);
        let (gold, changes) = convert(&red, Game::Crystal, 2).unwrap();
        let g = gold.mon().unwrap();
        assert_eq!((g.species, g.exp, g.dvs, g.stat_exp[0]), (25, 1000, [10, 10, 10, 10], 500));
        assert!(shiny(&g), "these DVs are shiny in Gold");
        assert!(changes.iter().any(|c| c.contains("friendship")));
        let (red2, _) = convert(&gold, Game::Blue, 2).unwrap();
        let r = red2.mon().unwrap();
        assert_eq!((r.species, r.exp, r.dvs, r.moves), (25, 1000, [10, 10, 10, 10], [84, 45, 0, 0]));
        // Max HP of a level 10 Pikachu (base 35; the HP DV is made of the others' last bits, all
        // even here, so 0; stat exp 500 adds ceil(sqrt(500)) / 4 = 5): the formula.
        assert_eq!(be16(&red2.data, 1), ((35 + 0) * 2 + 5) * 10 / 100 + 10 + 10);
    }

    #[test]
    fn to_ruby_it_gets_a_personality_that_keeps_shiny_gender_and_its_nature_rule() {
        let red = pikachu_gen1();
        let (crystal, _) = convert(&red, Game::Crystal, 2).unwrap();
        let (ruby, changes) = convert(&crystal, Game::Ruby, 2).unwrap();
        let m = ruby.mon().expect("the checksum adds up");
        assert_eq!(m.species, 25);
        assert!(shiny(&m), "still shiny");
        assert_eq!(gender_of(&m), gender_of(&crystal.mon().unwrap()));
        assert_eq!(m.pid % 25, 1000 % 25, "nature from its experience");
        assert_eq!(m.ivs[1], 21, "Attack DV 10 is IV 21");
        assert!(changes.iter().any(|c| c.contains("Nature")));
        // And back: still shiny, same gender, DVs near the IVs.
        let (gold, _) = convert(&ruby, Game::Gold, 2).unwrap();
        let g = gold.mon().unwrap();
        assert!(shiny(&g));
        assert_eq!(gender_of(&g), gender_of(&m));
    }

    #[test]
    fn what_cannot_go_is_refused_with_a_reason() {
        let treecko = Mon { gen: 3, species: 252, exp: 100, moves: [1, 0, 0, 0], ot_name: "MAY".into(), pid: 7, ..Mon::default() };
        let (ruby, _) = encode(&treecko, Game::Ruby, 2);
        assert!(convert(&ruby, Game::Gold, 2).unwrap_err().contains("first 251"));
        let pika = Mon { gen: 3, species: 25, exp: 100, moves: [344, 0, 0, 0], ot_name: "MAY".into(), pid: 9, ..Mon::default() };
        let (ruby, _) = encode(&pika, Game::Emerald, 2);
        assert!(convert(&ruby, Game::Red, 2).unwrap_err().contains("no moves"));
        let pika = Mon { moves: [344, 84, 0, 0], ..pika };
        let (ruby, _) = encode(&pika, Game::Emerald, 2);
        let (red, changes) = convert(&ruby, Game::Red, 2).unwrap();
        assert_eq!(red.mon().unwrap().moves, [84, 0, 0, 0]);
        assert!(changes.iter().any(|c| c.contains("Volt Tackle")));
    }

    #[test]
    fn stats_come_out_as_the_games_work_them() {
        // A level 100 Mewtwo with every DV 15 and full stat experience in Red: 415 HP and 406
        // Special (the game's square root stops at 255, so full stat experience adds 63).
        let m = Mon { gen: 1, species: 150, level: 100, dvs: [15; 4], stat_exp: [65535; 5], ..Mon::default() };
        let s = stats(&m);
        assert_eq!((s[0], s[4]), (415, 406));
        // Ruby, level 50 Pikachu, all IVs 31 and no EVs, Hardy (neutral): HP 110, Speed 110.
        let p = Mon { gen: 3, species: 25, level: 50, ivs: [31; 6], pid: 0, ..Mon::default() };
        assert_eq!(stats(&p), [110, 75, 50, 110, 70, 60]);
        // Modest (pid 15): Sp. Atk up a tenth, Attack down.
        let p = Mon { pid: 15, ..p };
        assert_eq!((stats(&p)[1], stats(&p)[4]), (67, 77));
    }

    #[test]
    fn a_traded_kadabra_evolves_and_an_everstone_stops_it() {
        let k = Mon { gen: 2, species: 64, exp: 5000, moves: [93, 0, 0, 0], ot_name: "GOLD".into(), ..Mon::default() };
        let (mut s, _) = encode(&k, Game::Gold, 2);
        assert!(trade_evolve(&mut s).unwrap().contains("Alakazam"));
        assert_eq!(s.mon().unwrap().species, 65);
        assert_eq!(s.mon().unwrap().nickname, "ALAKAZAM");
        let held = Mon { item: super::super::data::EVERSTONE, ..k.clone() };
        let (mut s, _) = encode(&held, Game::Gold, 2);
        assert!(trade_evolve(&mut s).unwrap().contains("Everstone"));
        assert_eq!(s.mon().unwrap().species, 64);
        let onix = Mon { species: 95, item: 210, ..k };
        let (mut s, _) = encode(&onix, Game::Emerald, 2);
        trade_evolve(&mut s).unwrap();
        let m = s.mon().unwrap();
        assert_eq!((m.species, m.item), (208, 0), "Steelix, the Metal Coat used up");
    }
}
