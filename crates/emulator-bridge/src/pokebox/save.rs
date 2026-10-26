//! The PC boxes inside each game's save file, read and written the way the game itself does,
//! checksums and all. From Bulbapedia's "Save data structure (Generation I)", "(Generation II)"
//! and "(Generation III)" (English and European cartridges).
//!
//! Red to Crystal keep the box being used in the main part of the save and the others in two
//! storage banks; the box being used is the real one (its bank copy is stale until the player
//! changes box). Ruby to LeafGreen keep two whole saves and use the newer; the newer one is
//! changed in place and every section's checksum is worked out again.

use std::io::Read;
use std::path::Path;

use super::mon::{Game, Stored, GEN1_LEN, GEN2_LEN, GEN3_LEN, NAME_LEN};
use super::text;

/// The game a cartridge is, and the language its Pokémon are written in (Ruby to LeafGreen).
pub fn identify(rom: &[u8]) -> Option<(Game, u8)> {
    if rom.len() >= 0xB0 && rom.len() > 0x8000 && &rom[0xB2..0xB3] == [0x96] {
        let code = &rom[0xAC..0xB0];
        let game = match &code[..3] {
            b"AXV" => Game::Ruby,
            b"AXP" => Game::Sapphire,
            b"BPE" => Game::Emerald,
            b"BPR" => Game::FireRed,
            b"BPG" => Game::LeafGreen,
            _ => return None,
        };
        let language = match code[3] {
            b'E' | b'P' => 2,
            b'F' => 3,
            b'I' => 4,
            b'D' => 5,
            b'S' => 7,
            _ => return None,
        };
        return Some((game, language));
    }
    if rom.len() < 0x150 || rom[0x14A] != 0x01 {
        return None;
    }
    let title = &rom[0x134..0x144];
    let starts = |t: &[u8]| title.starts_with(t);
    let game = if starts(b"POKEMON RED") {
        Game::Red
    } else if starts(b"POKEMON BLUE") {
        Game::Blue
    } else if starts(b"POKEMON YELLOW") {
        Game::Yellow
    } else if starts(b"POKEMON_GLD") {
        Game::Gold
    } else if starts(b"POKEMON_SLV") {
        Game::Silver
    } else if starts(b"PM_CRYSTAL") {
        Game::Crystal
    } else {
        return None;
    };
    if game.gen() == 2 {
        // The English cartridges (US "E", Europe "P"): the others are laid out differently.
        if !matches!(rom[0x142], b'E' | b'P') {
            return None;
        }
    }
    Some((game, 2))
}

/// The start of a game file, enough for `identify`; a .zip is opened and its game read.
pub fn rom_head(path: &Path) -> Option<Vec<u8>> {
    let mut file = std::fs::File::open(path).ok()?;
    let lower = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let mut head = Vec::new();
    if lower == "zip" {
        let mut zip = zip::ZipArchive::new(file).ok()?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).ok()?;
            let name = entry.name().to_ascii_lowercase();
            if [".gb", ".gbc", ".gba", ".sgb", ".cgb"].iter().any(|e| name.ends_with(e)) {
                entry.by_ref().take(0x10000).read_to_end(&mut head).ok()?;
                return Some(head);
            }
        }
        return None;
    }
    file.by_ref().take(0x10000).read_to_end(&mut head).ok()?;
    Some(head)
}

/// One PC box as the game has it. Red to Crystal: a list of up to 20, no gaps. Ruby to
/// LeafGreen: 30 places, empty ones None.
#[derive(Clone, Debug)]
pub struct PcBox {
    pub name: String,
    pub capacity: usize,
    pub slots: Vec<Option<Stored>>,
}

#[derive(Clone, Debug)]
pub struct Pc {
    pub trainer: String,
    pub trainer_id: u16,
    pub current_box: usize,
    pub boxes: Vec<PcBox>,
}

// --- Red, Blue, Yellow ------------------------------------------------------------------------------

const G1_NAME: usize = 0x2598;
const G1_OWNED: usize = 0x25A3;
const G1_SEEN: usize = 0x25B6;
const G1_ID: usize = 0x2605;
const G1_BOX_NUM: usize = 0x284C;
const G1_CURRENT: usize = 0x30C0;
const G1_CHECKSUM: usize = 0x3523;
const G1_BOX_SIZE: usize = 0x462;

// --- Gold, Silver, Crystal --------------------------------------------------------------------------

struct G2 {
    box_num: usize,
    box_names: usize,
    owned: usize,
    seen: usize,
    current: usize,
    checksum: usize,
    end: usize,
}

const G2_GS: G2 = G2 { box_num: 0x2724, box_names: 0x2727, owned: 0x2A4C, seen: 0x2A6C, current: 0x2D6C, checksum: 0x2D69, end: 0x2D68 };
const G2_C: G2 = G2 { box_num: 0x2700, box_names: 0x2703, owned: 0x2A27, seen: 0x2A47, current: 0x2D10, checksum: 0x2D0D, end: 0x2B82 };
const G2_BOX_SIZE: usize = 0x450;
/// Gold and Silver's second copy, in five pieces: (first, last) of the main copy, then where
/// the piece starts in the second.
const GS_SECOND: [(usize, usize, usize); 5] = [
    (0x2009, 0x222E, 0x15C7),
    (0x222F, 0x23D8, 0x3D96),
    (0x23D9, 0x2855, 0x0C6B),
    (0x2856, 0x2889, 0x7E39),
    (0x288A, 0x2D68, 0x10E8),
];

fn g2(game: Game) -> &'static G2 {
    if game == Game::Crystal { &G2_C } else { &G2_GS }
}

/// A Pokémon list of Red to Crystal: its count, species list, Pokémon, trainers' names, names.
/// A PC box holds 20, the team 6; `len` is the size of one Pokémon there, of which the first
/// `keep` bytes are what a box keeps (a team's Pokémon have their worked-out stats after them).
fn read_list_of(save: &[u8], at: usize, len: usize, cap: usize, keep: usize, game: Game) -> Option<Vec<Stored>> {
    let count = usize::from(*save.get(at)?);
    if count > cap || save.get(at + 1 + count) != Some(&0xFF) {
        return None;
    }
    let data = at + cap + 2;
    let ots = data + cap * len;
    let names = ots + cap * NAME_LEN;
    let mut out = Vec::new();
    for i in 0..count {
        let mut d = save.get(data + i * len..data + i * len + keep)?.to_vec();
        d.extend_from_slice(save.get(ots + i * NAME_LEN..ots + (i + 1) * NAME_LEN)?);
        d.extend_from_slice(save.get(names + i * NAME_LEN..names + (i + 1) * NAME_LEN)?);
        out.push(Stored { game, egg: game.gen() == 2 && save[at + 1 + i] == 0xFD, data: d });
    }
    Some(out)
}

fn read_list(save: &[u8], at: usize, len: usize, game: Game) -> Option<Vec<Stored>> {
    read_list_of(save, at, len, 20, len, game)
}

fn write_list(save: &mut [u8], at: usize, len: usize, list: &[Stored]) {
    let size = 22 + 20 * (len + 2 * NAME_LEN);
    save[at..at + size].fill(0);
    save[at] = list.len() as u8;
    for (i, s) in list.iter().enumerate() {
        save[at + 1 + i] = if s.egg { 0xFD } else { s.data[0] };
        let data = at + 22 + i * len;
        save[data..data + len].copy_from_slice(&s.data[..len]);
        let ot = at + 22 + 20 * len + i * NAME_LEN;
        save[ot..ot + NAME_LEN].copy_from_slice(&s.data[len..len + NAME_LEN]);
        let nm = at + 22 + 20 * (len + NAME_LEN) + i * NAME_LEN;
        save[nm..nm + NAME_LEN].copy_from_slice(&s.data[len + NAME_LEN..len + 2 * NAME_LEN]);
    }
    save[at + 1 + list.len()] = 0xFF;
}

fn sum8(b: &[u8]) -> u8 {
    b.iter().fold(0u8, |a, x| a.wrapping_add(*x))
}

fn sum16(b: &[u8]) -> u16 {
    b.iter().fold(0u16, |a, x| a.wrapping_add(u16::from(*x)))
}

fn g1_bank_box(b: usize) -> usize {
    0x4000 + (b / 6) * 0x2000 + (b % 6) * G1_BOX_SIZE
}

fn g1_seal(save: &mut [u8]) {
    save[G1_CHECKSUM] = !sum8(&save[G1_NAME..G1_CHECKSUM]);
    for bank in [0x4000usize, 0x6000] {
        save[bank + 0x1A4C] = !sum8(&save[bank..bank + 0x1A4C]);
        for i in 0..6 {
            let at = bank + i * G1_BOX_SIZE;
            save[bank + 0x1A4D + i] = !sum8(&save[at..at + G1_BOX_SIZE]);
        }
    }
}

fn g2_seal(save: &mut [u8], game: Game) {
    let l = g2(game);
    let sum = sum16(&save[0x2009..=l.end]);
    save[l.checksum..l.checksum + 2].copy_from_slice(&sum.to_le_bytes());
    if game == Game::Crystal {
        let primary = save[0x2009..=l.end].to_vec();
        save[0x1209..0x1209 + primary.len()].copy_from_slice(&primary);
        save[0x1F0D..0x1F0F].copy_from_slice(&sum.to_le_bytes());
    } else {
        for (a, b, to) in GS_SECOND {
            let piece = save[a..=b].to_vec();
            save[to..to + piece.len()].copy_from_slice(&piece);
        }
        save[0x7E6D..0x7E6F].copy_from_slice(&sum.to_le_bytes());
    }
}

/// The second copy of Gold to Crystal's main data, as the main copy's bytes.
fn g2_second(save: &[u8], game: Game) -> (Vec<u8>, u16) {
    let l = g2(game);
    if game == Game::Crystal {
        let n = l.end - 0x2009 + 1;
        (save[0x1209..0x1209 + n].to_vec(), u16::from_le_bytes([save[0x1F0D], save[0x1F0E]]))
    } else {
        let mut v = Vec::new();
        for (a, b, to) in GS_SECOND {
            v.extend_from_slice(&save[to..to + (b - a + 1)]);
        }
        (v, u16::from_le_bytes([save[0x7E6D], save[0x7E6E]]))
    }
}

// --- Ruby, Sapphire, Emerald, FireRed, LeafGreen ----------------------------------------------------

const SECTION: usize = 0x1000;
const SIGNATURE: u32 = 0x0801_2025;

fn section_size(id: u16) -> usize {
    match id {
        0 => 3884,
        4 => 3848,
        13 => 2000,
        _ => 3968,
    }
}

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn section_sum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    for w in data.chunks(4) {
        let mut x = [0u8; 4];
        x[..w.len()].copy_from_slice(w);
        sum = sum.wrapping_add(u32::from_le_bytes(x));
    }
    ((sum >> 16) as u16).wrapping_add(sum as u16)
}

/// Where each section (by id) of a save block is, if the whole block checks out.
fn g3_block(save: &[u8], block: usize) -> Option<([usize; 14], u32)> {
    let mut at = [usize::MAX; 14];
    for i in 0..14 {
        let s = block + i * SECTION;
        let id = le16(save, s + 0xFF4);
        if id > 13 || at[usize::from(id)] != usize::MAX || le32(save, s + 0xFF8) != SIGNATURE {
            return None;
        }
        if section_sum(&save[s..s + section_size(id)]) != le16(save, s + 0xFF6) {
            return None;
        }
        at[usize::from(id)] = s;
    }
    Some((at, le32(save, block + 13 * SECTION + 0xFFC)))
}

/// The newer of the two saves that checks out.
fn g3_current(save: &[u8]) -> Result<[usize; 14], String> {
    if save.len() < 0x1C000 {
        return Err("This doesn't look like a save from that game (it's too small).".into());
    }
    match (g3_block(save, 0), g3_block(save, 0xE000)) {
        (Some((a, ia)), Some((b, ib))) => Ok(if ia > ib { a } else { b }),
        (Some((a, _)), None) => Ok(a),
        (None, Some((b, _))) => Ok(b),
        (None, None) => Err("The game's save doesn't check out, so it's left alone. Save once in the game and try again.".into()),
    }
}

fn g3_pc(save: &[u8], at: &[usize; 14]) -> Vec<u8> {
    let mut pc = Vec::with_capacity(33744);
    for id in 5..=13 {
        pc.extend_from_slice(&save[at[id]..at[id] + section_size(id as u16)]);
    }
    pc
}

fn g3_put_pc(save: &mut [u8], at: &[usize; 14], pc: &[u8]) {
    let mut off = 0;
    for id in 5..=13 {
        let n = section_size(id as u16);
        save[at[id]..at[id] + n].copy_from_slice(&pc[off..off + n]);
        off += n;
    }
}

fn g3_seal(save: &mut [u8], at: &[usize; 14]) {
    for (id, s) in at.iter().enumerate() {
        let sum = section_sum(&save[*s..*s + section_size(id as u16)]);
        save[s + 0xFF6..s + 0xFF8].copy_from_slice(&sum.to_le_bytes());
    }
}

fn g3_cell(b: usize, slot: usize) -> usize {
    4 + (b * 30 + slot) * GEN3_LEN
}

fn g3_empty(d: &[u8]) -> bool {
    d.iter().all(|x| *x == 0) || super::mon::gen3_parts(d).is_some_and(|p| le16(&p, 0) == 0)
}

// --- the same calls for every game ------------------------------------------------------------------

/// Checks the save and, for Gold to Crystal, repairs the main copy from the second when only
/// the second checks out, as the game itself would.
pub fn check(game: Game, save: &mut [u8]) -> Result<(), String> {
    let damaged = "The game's save doesn't check out, so it's left alone. Save once in the game and try again.";
    match game.gen() {
        1 => {
            if save.len() < 0x8000 {
                return Err("This doesn't look like a save from that game (it's too small).".into());
            }
            if save[G1_CHECKSUM] != !sum8(&save[G1_NAME..G1_CHECKSUM]) {
                return Err(damaged.into());
            }
            Ok(())
        }
        2 => {
            if save.len() < 0x8000 {
                return Err("This doesn't look like a save from that game (it's too small).".into());
            }
            let l = g2(game);
            let main_ok = sum16(&save[0x2009..=l.end]) == u16::from_le_bytes([save[l.checksum], save[l.checksum + 1]]);
            if main_ok {
                return Ok(());
            }
            let (second, sum) = g2_second(save, game);
            if sum16(&second) != sum {
                return Err(damaged.into());
            }
            save[0x2009..0x2009 + second.len()].copy_from_slice(&second);
            save[l.checksum..l.checksum + 2].copy_from_slice(&sum.to_le_bytes());
            Ok(())
        }
        _ => g3_current(save).map(|_| ()),
    }
}

pub fn read_pc(game: Game, save: &[u8]) -> Result<Pc, String> {
    match game.gen() {
        1 => {
            let current = usize::from(save[G1_BOX_NUM] & 0x7F).min(11);
            let ready = save[G1_BOX_NUM] & 0x80 != 0;
            let boxes = (0..12)
                .map(|b| {
                    let list = if b == current {
                        read_list(save, G1_CURRENT, GEN1_LEN, game)
                    } else if ready {
                        read_list(save, g1_bank_box(b), GEN1_LEN, game)
                    } else {
                        None
                    };
                    PcBox { name: format!("BOX {}", b + 1), capacity: 20, slots: list.unwrap_or_default().into_iter().map(Some).collect() }
                })
                .collect();
            Ok(Pc {
                trainer: text::gb_decode(&save[G1_NAME..G1_NAME + NAME_LEN]),
                trainer_id: u16::from_be_bytes([save[G1_ID], save[G1_ID + 1]]),
                current_box: current,
                boxes,
            })
        }
        2 => {
            let l = g2(game);
            let current = usize::from(save[l.box_num] & 0x0F).min(13);
            let boxes = (0..14)
                .map(|b| {
                    let at = if b == current { l.current } else { g2_bank_box(b) };
                    let list = read_list(save, at, GEN2_LEN, game).unwrap_or_default();
                    let n = &save[l.box_names + b * 9..l.box_names + b * 9 + 9];
                    let name = text::gb_decode(n);
                    PcBox { name: if name.is_empty() { format!("BOX{}", b + 1) } else { name }, capacity: 20, slots: list.into_iter().map(Some).collect() }
                })
                .collect();
            Ok(Pc {
                trainer: text::gb_decode(&save[0x200B..0x200B + NAME_LEN]),
                trainer_id: u16::from_be_bytes([save[0x2009], save[0x200A]]),
                current_box: current,
                boxes,
            })
        }
        _ => {
            let at = g3_current(save)?;
            let pc = g3_pc(save, &at);
            let boxes = (0..14)
                .map(|b| {
                    let slots = (0..30)
                        .map(|i| {
                            let d = &pc[g3_cell(b, i)..g3_cell(b, i) + GEN3_LEN];
                            (!g3_empty(d)).then(|| Stored { game, egg: false, data: d.to_vec() })
                        })
                        .collect();
                    let name = text::gba_decode(&pc[0x8344 + b * 9..0x8344 + b * 9 + 9]);
                    PcBox { name: if name.is_empty() { format!("BOX {}", b + 1) } else { name }, capacity: 30, slots }
                })
                .collect();
            let s0 = at[0];
            Ok(Pc {
                trainer: text::gba_decode(&save[s0..s0 + 7]),
                trainer_id: le16(save, s0 + 0x0A),
                current_box: (le32(&pc, 0) as usize).min(13),
                boxes,
            })
        }
    }
}

fn g2_bank_box(b: usize) -> usize {
    if b < 7 { 0x4000 + b * G2_BOX_SIZE } else { 0x6000 + (b - 7) * G2_BOX_SIZE }
}

/// Where a box's list is, for writing. Red to Yellow's banks are made ready first if the game
/// never has (it empties them the first time the player changes box, and would wipe anything
/// put there).
fn gb_box_at(game: Game, save: &mut [u8], b: usize) -> usize {
    if game.gen() == 1 {
        let current = usize::from(save[G1_BOX_NUM] & 0x7F).min(11);
        if b == current {
            return G1_CURRENT;
        }
        if save[G1_BOX_NUM] & 0x80 == 0 {
            for i in 0..12 {
                write_list(save, g1_bank_box(i), GEN1_LEN, &[]);
            }
            save[G1_BOX_NUM] |= 0x80;
        }
        g1_bank_box(b)
    } else {
        let l = g2(game);
        if b == usize::from(save[l.box_num] & 0x0F) { l.current } else { g2_bank_box(b) }
    }
}

/// Works the sums out again. Ruby to LeafGreen: of the save block `at`, chosen before the change
/// (after it, that block's old sums no longer match, so it could not be found again).
fn seal(game: Game, save: &mut [u8], at: Option<&[usize; 14]>) {
    match (game.gen(), at) {
        (1, _) => g1_seal(save),
        (2, _) => g2_seal(save, game),
        (_, Some(at)) => g3_seal(save, at),
        _ => {}
    }
}

/// Takes the Pokémon out of box `b`, place `slot`.
pub fn take(game: Game, save: &mut [u8], b: usize, slot: usize) -> Result<Stored, String> {
    let gone = "That Pokémon isn't there any more. Close this and open it again.";
    let out = match game.gen() {
        1 | 2 => {
            let len = if game.gen() == 1 { GEN1_LEN } else { GEN2_LEN };
            let at = gb_box_at(game, save, b);
            let mut list = read_list(save, at, len, game).ok_or(gone)?;
            if slot >= list.len() {
                return Err(gone.into());
            }
            let s = list.remove(slot);
            write_list(save, at, len, &list);
            s
        }
        _ => {
            let at = g3_current(save)?;
            let mut pc = g3_pc(save, &at);
            let c = g3_cell(b, slot);
            let d = pc.get(c..c + GEN3_LEN).ok_or(gone)?.to_vec();
            if g3_empty(&d) {
                return Err(gone.into());
            }
            pc[c..c + GEN3_LEN].fill(0);
            g3_put_pc(save, &at, &pc);
            seal(game, save, Some(&at));
            return Ok(Stored { game, egg: false, data: d });
        }
    };
    seal(game, save, None);
    Ok(out)
}

/// Puts a Pokémon (already in this game's generation) into box `b`: at the end of the list, or
/// the first empty place. Returns the place.
pub fn put(game: Game, save: &mut [u8], b: usize, s: &Stored) -> Result<usize, String> {
    let full = "That box is full. Pick another box.";
    let slot = match game.gen() {
        1 | 2 => {
            let len = if game.gen() == 1 { GEN1_LEN } else { GEN2_LEN };
            let at = gb_box_at(game, save, b);
            let mut list = read_list(save, at, len, game).unwrap_or_default();
            if list.len() >= 20 {
                return Err(full.into());
            }
            list.push(s.clone());
            write_list(save, at, len, &list);
            list.len() - 1
        }
        _ => {
            let at = g3_current(save)?;
            let mut pc = g3_pc(save, &at);
            let slot = (0..30).find(|i| g3_empty(&pc[g3_cell(b, *i)..g3_cell(b, *i) + GEN3_LEN])).ok_or(full)?;
            pc[g3_cell(b, slot)..g3_cell(b, slot) + GEN3_LEN].copy_from_slice(&s.data);
            g3_put_pc(save, &at, &pc);
            seal(game, save, Some(&at));
            return Ok(slot);
        }
    };
    seal(game, save, None);
    Ok(slot)
}

// --- the team ------------------------------------------------------------------------------------

/// Where the team is and how it is kept: (offset in the save, or in section 1 for Ruby to
/// LeafGreen; bytes a Pokémon takes there; bytes of that a box keeps).
fn party_at(game: Game) -> (usize, usize, usize) {
    match game {
        Game::Red | Game::Blue | Game::Yellow => (0x2F2C, 44, GEN1_LEN),
        Game::Gold | Game::Silver => (0x288A, 48, GEN2_LEN),
        Game::Crystal => (0x2865, 48, GEN2_LEN),
        Game::FireRed | Game::LeafGreen => (0x0034, 100, GEN3_LEN),
        _ => (0x0234, 100, GEN3_LEN),
    }
}

/// The team, each Pokémon as a box would keep it (its stats are worked out again when it joins a
/// team, as the games do when one is taken out of the PC).
pub fn read_party(game: Game, save: &[u8]) -> Result<Vec<Stored>, String> {
    let (at, len, keep) = party_at(game);
    let damaged = || "The team in this save can't be read.".to_string();
    if game.gen() == 3 {
        let s1 = g3_current(save)?[1];
        let count = (le32(save, s1 + at) as usize).min(6);
        return Ok((0..count)
            .map(|i| {
                let d = &save[s1 + at + 4 + i * len..s1 + at + 4 + i * len + keep];
                Stored { game, egg: false, data: d.to_vec() }
            })
            .collect());
    }
    let mut list = read_list_of(save, at, len, 6, keep, game).ok_or_else(damaged)?;
    if game.gen() == 1 {
        // A team Pokémon's level is kept after its box part; the box part's own copy may be old.
        let data = at + 8;
        for (i, s) in list.iter_mut().enumerate() {
            s.data[3] = save[data + i * len + 33];
        }
    }
    Ok(list)
}

/// Puts a Pokémon (already in this game's generation) at the end of the team, with its stats
/// worked out and its health full, as the game does when one is taken out of the PC.
pub fn put_party(game: Game, save: &mut [u8], s: &Stored) -> Result<usize, String> {
    let team = read_party(game, save)?;
    let n = team.len();
    if n >= 6 {
        return Err("That team is full (six). Take one out of it first.".into());
    }
    let tail = super::mon::party_tail(s).ok_or("This one's data is damaged, so it can't join a team.")?;
    let (at, len, keep) = party_at(game);
    let mut entry = s.data[..keep].to_vec();
    entry.extend_from_slice(&tail);
    entry.resize(len, 0);
    if game.gen() == 3 {
        let ids = g3_current(save)?;
        let base = ids[1] + at + 4 + n * len;
        save[base..base + len].copy_from_slice(&entry);
        save[ids[1] + at..ids[1] + at + 4].copy_from_slice(&((n + 1) as u32).to_le_bytes());
        seal(game, save, Some(&ids));
        return Ok(n);
    }
    if game.gen() == 1 {
        // Red's box part keeps the current health first: full.
        let hp = [entry[34], entry[35]];
        entry[1..3].copy_from_slice(&hp);
        entry[4] = 0;
    }
    let cap = 6;
    let data = at + cap + 2;
    let ots = data + cap * len;
    let names = ots + cap * NAME_LEN;
    save[at + 1 + n] = if s.egg { 0xFD } else { s.data[0] };
    save[at + 2 + n] = 0xFF;
    save[data + n * len..data + (n + 1) * len].copy_from_slice(&entry);
    save[ots + n * NAME_LEN..ots + (n + 1) * NAME_LEN].copy_from_slice(&s.data[keep..keep + NAME_LEN]);
    save[names + n * NAME_LEN..names + (n + 1) * NAME_LEN].copy_from_slice(&s.data[keep + NAME_LEN..keep + 2 * NAME_LEN]);
    save[at] = (n + 1) as u8;
    seal(game, save, None);
    Ok(n)
}

/// Takes the Pokémon in place `slot` of the team; the rest move up. Never the last one: a game
/// can't be played with an empty team.
pub fn take_party(game: Game, save: &mut [u8], slot: usize) -> Result<Stored, String> {
    let team = read_party(game, save)?;
    if team.len() <= 1 {
        return Err("That's the only Pokémon in the team, and a team can't be empty. Put another in the team first.".into());
    }
    let out = team.get(slot).cloned().ok_or("That Pokémon isn't in the team any more. Close this and open it again.")?;
    let (at, len, _) = party_at(game);
    let n = team.len();
    if game.gen() == 3 {
        let ids = g3_current(save)?;
        let base = ids[1] + at + 4;
        for i in slot..n - 1 {
            let next = save[base + (i + 1) * len..base + (i + 2) * len].to_vec();
            save[base + i * len..base + (i + 1) * len].copy_from_slice(&next);
        }
        save[base + (n - 1) * len..base + n * len].fill(0);
        save[ids[1] + at..ids[1] + at + 4].copy_from_slice(&((n - 1) as u32).to_le_bytes());
        seal(game, save, Some(&ids));
        return Ok(out);
    }
    let cap = 6;
    let species = at + 1;
    let data = at + cap + 2;
    let ots = data + cap * len;
    let names = ots + cap * NAME_LEN;
    for i in slot..n - 1 {
        save[species + i] = save[species + i + 1];
        for (base, size) in [(data, len), (ots, NAME_LEN), (names, NAME_LEN)] {
            let next = save[base + (i + 1) * size..base + (i + 2) * size].to_vec();
            save[base + i * size..base + (i + 1) * size].copy_from_slice(&next);
        }
    }
    for (base, size) in [(data, len), (ots, NAME_LEN), (names, NAME_LEN)] {
        save[base + (n - 1) * size..base + n * size].fill(0);
    }
    save[species + n - 1] = 0xFF;
    save[species + n] = 0;
    save[at] = (n - 1) as u8;
    seal(game, save, None);
    Ok(out)
}

/// Marks a species seen and caught in the game's Pokédex, as a trade does.
pub fn register(game: Game, save: &mut [u8], dex: u16) -> Result<(), String> {
    if dex == 0 || dex > game.last_species() {
        return Ok(());
    }
    let (byte, bit) = (usize::from(dex - 1) / 8, (dex - 1) % 8);
    let mut set = |at: usize| save[at + byte] |= 1 << bit;
    match game.gen() {
        1 => {
            set(G1_OWNED);
            set(G1_SEEN);
        }
        2 => {
            let l = g2(game);
            set(l.owned);
            set(l.seen);
        }
        _ => {
            let at = g3_current(save)?;
            let (b, c) = match game {
                Game::Ruby | Game::Sapphire => (0x938, 0xC0C),
                Game::Emerald => (0x988, 0xCA4),
                _ => (0x5F8, 0xB98),
            };
            for a in [at[0] + 0x28, at[0] + 0x5C, at[1] + b, at[4] + c] {
                save[a + byte] |= 1 << bit;
            }
            seal(game, save, Some(&at));
            return Ok(());
        }
    }
    seal(game, save, None);
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::pokebox::mon::{encode, Mon};

    /// A save as the game would leave it: trainer `ASH`, ID 4321, empty boxes, sums right.
    pub fn blank(game: Game) -> Vec<u8> {
        match game.gen() {
            1 => {
                let mut s = vec![0u8; 0x8000];
                s[G1_NAME..G1_NAME + NAME_LEN].copy_from_slice(&text::gb_encode("ASH", NAME_LEN).0);
                s[G1_ID..G1_ID + 2].copy_from_slice(&4321u16.to_be_bytes());
                write_list(&mut s, G1_CURRENT, GEN1_LEN, &[]);
                g1_seal(&mut s);
                s
            }
            2 => {
                let mut s = vec![0u8; 0x8000];
                let l = g2(game);
                s[0x2008] = 0x63;
                s[0x2009..0x200B].copy_from_slice(&4321u16.to_be_bytes());
                s[0x200B..0x200B + NAME_LEN].copy_from_slice(&text::gb_encode("GOLD", NAME_LEN).0);
                for b in 0..14 {
                    write_list(&mut s, g2_bank_box(b), GEN2_LEN, &[]);
                }
                write_list(&mut s, l.current, GEN2_LEN, &[]);
                g2_seal(&mut s, game);
                s
            }
            _ => {
                let mut s = vec![0xFFu8; 0x20000];
                for block in [0usize, 0xE000] {
                    for i in 0..14 {
                        let at = block + i * SECTION;
                        // Rotated, as the game leaves them: the order starts at section 3.
                        let id = ((i + 3) % 14) as u16;
                        s[at..at + SECTION].fill(0);
                        s[at + 0xFF4..at + 0xFF6].copy_from_slice(&id.to_le_bytes());
                        s[at + 0xFF8..at + 0xFFC].copy_from_slice(&SIGNATURE.to_le_bytes());
                        let index: u32 = if block == 0 { 7 } else { 6 };
                        s[at + 0xFFC..at + 0x1000].copy_from_slice(&index.to_le_bytes());
                        if id == 0 {
                            s[at..at + 7].copy_from_slice(&text::gba_encode("MAY", 7).0);
                            s[at + 0x0A..at + 0x0C].copy_from_slice(&4321u16.to_le_bytes());
                        }
                    }
                    for i in 0..14 {
                        let a = block + i * SECTION;
                        let id = le16(&s, a + 0xFF4);
                        let sum = section_sum(&s[a..a + section_size(id)]);
                        s[a + 0xFF6..a + 0xFF8].copy_from_slice(&sum.to_le_bytes());
                    }
                }
                s
            }
        }
    }

    fn mon(species: u16) -> Mon {
        Mon { gen: 1, species, ot_name: "ASH".into(), ot_id: 4321, exp: 500, moves: [33, 0, 0, 0], pid: 0x1234_5678, ..Mon::default() }
    }

    #[test]
    fn each_game_takes_a_pokemon_into_a_box_and_gives_it_back_with_its_sums_right() {
        for game in Game::ALL {
            let mut s = blank(game);
            check(game, &mut s).unwrap();
            let p = encode(&mon(25), game, 2).0;
            let other_box = if game.gen() == 3 { 13 } else { 5 };
            for b in [0, other_box] {
                let slot = put(game, &mut s, b, &p).unwrap();
                check(game, &mut s).unwrap_or_else(|e| panic!("{game:?}: {e}"));
                let pc = read_pc(game, &s).unwrap();
                assert_eq!(pc.boxes[b].slots[slot].as_ref().unwrap_or_else(|| panic!("{game:?} box {b} slot {slot}: {:?}", pc.boxes[b].slots.iter().map(|x| x.is_some()).collect::<Vec<_>>())).mon().map(|m| m.species), Some(25), "{game:?} box {b}");
                register(game, &mut s, 25).unwrap();
                check(game, &mut s).unwrap();
            }
            let back = take(game, &mut s, 0, 0).unwrap();
            assert_eq!(back.data, p.data, "{game:?}: byte for byte");
            check(game, &mut s).unwrap();
            assert!(read_pc(game, &s).unwrap().boxes[0].slots.iter().all(|x| x.is_none()), "{game:?}");
            assert_eq!(read_pc(game, &s).unwrap().trainer_id, 4321);
        }
    }

    /// Puts `team` in the game's team the way the game keeps it (box part, then zeros for the
    /// stats; Red's level copied after the box part).
    fn set_party(game: Game, s: &mut [u8], team: &[Stored]) {
        let (at, len, keep) = party_at(game);
        if game.gen() == 3 {
            let ids = g3_current(s).unwrap();
            let base = ids[1] + at;
            s[base..base + 4].copy_from_slice(&(team.len() as u32).to_le_bytes());
            for (i, m) in team.iter().enumerate() {
                s[base + 4 + i * len..base + 4 + i * len + keep].copy_from_slice(&m.data);
            }
            g3_seal(s, &ids);
            return;
        }
        s[at] = team.len() as u8;
        let data = at + 8;
        let ots = data + 6 * len;
        let names = ots + 6 * NAME_LEN;
        for (i, m) in team.iter().enumerate() {
            s[at + 1 + i] = m.data[0];
            s[data + i * len..data + i * len + keep].copy_from_slice(&m.data[..keep]);
            if game.gen() == 1 {
                s[data + i * len + 33] = m.data[3];
            }
            s[ots + i * NAME_LEN..ots + (i + 1) * NAME_LEN].copy_from_slice(&m.data[keep..keep + NAME_LEN]);
            s[names + i * NAME_LEN..names + (i + 1) * NAME_LEN].copy_from_slice(&m.data[keep + NAME_LEN..]);
        }
        s[at + 1 + team.len()] = 0xFF;
        seal(game, s, None);
    }

    #[test]
    fn a_team_pokemon_comes_out_and_the_rest_move_up_but_never_the_last() {
        for game in Game::ALL {
            let mut s = blank(game);
            let a = encode(&mon(25), game, 2).0;
            let b = encode(&mon(1), game, 2).0;
            set_party(game, &mut s, &[a.clone(), b.clone()]);
            check(game, &mut s).unwrap();
            assert_eq!(read_party(game, &s).unwrap().len(), 2, "{game:?}");
            let out = take_party(game, &mut s, 0).unwrap();
            assert_eq!(out.data, a.data, "{game:?}");
            check(game, &mut s).unwrap_or_else(|e| panic!("{game:?}: {e}"));
            let left = read_party(game, &s).unwrap();
            assert_eq!(left.len(), 1);
            assert_eq!(left[0].mon().unwrap().species, 1, "{game:?}: Bulbasaur moved up");
            assert!(take_party(game, &mut s, 0).is_err(), "{game:?}: the last stays");
            put_party(game, &mut s, &a).unwrap();
            check(game, &mut s).unwrap_or_else(|e| panic!("{game:?}: {e}"));
            let team = read_party(game, &s).unwrap();
            assert_eq!(team.len(), 2);
            assert_eq!(team[1].mon().unwrap().species, 25, "{game:?}: joined at the end");
        }
    }

    #[test]
    fn red_makes_its_storage_banks_ready_before_writing_to_them() {
        let mut s = blank(Game::Red);
        s[0x4000..0x8000].fill(0x55); // never used: what a new cartridge's battery memory holds
        g1_seal(&mut s);
        assert!(read_pc(Game::Red, &s).unwrap().boxes[3].slots.is_empty());
        put(Game::Red, &mut s, 3, &encode(&mon(1), Game::Red, 2).0).unwrap();
        assert_ne!(s[G1_BOX_NUM] & 0x80, 0);
        let pc = read_pc(Game::Red, &s).unwrap();
        assert_eq!(pc.boxes[3].slots.len(), 1);
        assert!(pc.boxes[4].slots.is_empty());
    }

    #[test]
    fn gold_with_only_its_second_copy_right_is_repaired_from_it() {
        let mut s = blank(Game::Gold);
        s[0x2100] ^= 0xFF;
        check(Game::Gold, &mut s).unwrap();
        assert_eq!(s[0x2100], 0);
        s[0x2100] ^= 0xFF;
        s[0x15C7 + (0x2100 - 0x2009)] ^= 0xFF;
        assert!(check(Game::Gold, &mut s).is_err(), "both copies wrong: left alone");
    }

    #[test]
    fn cartridges_are_told_apart_by_their_headers() {
        let mut gb = vec![0u8; 0x8000];
        gb[0x134..0x134 + 11].copy_from_slice(b"POKEMON RED");
        gb[0x14A] = 1;
        assert_eq!(identify(&gb), Some((Game::Red, 2)));
        gb[0x14A] = 0;
        assert_eq!(identify(&gb), None, "the Japanese one is laid out differently");
        let mut c = vec![0u8; 0x8000];
        c[0x134..0x134 + 15].copy_from_slice(b"PM_CRYSTAL\0BYTE");
        c[0x14A] = 1;
        assert_eq!(identify(&c), Some((Game::Crystal, 2)));
        let mut gba = vec![0u8; 0x10000];
        gba[0xAC..0xB0].copy_from_slice(b"BPRE");
        gba[0xB2] = 0x96;
        assert_eq!(identify(&gba), Some((Game::FireRed, 2)));
        gba[0xAF] = b'J';
        assert_eq!(identify(&gba), None);
    }
}
