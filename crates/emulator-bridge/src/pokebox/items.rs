//! Items: each game's bag, read and changed the way the game keeps it, and the Box's item locker.
//! After G1R Deluxe's "Items" tool: put items from one game's bag in the locker and take them out
//! into another game's bag, give a Box Pokémon an item to hold or take it off, and change the
//! ball a Ruby-to-Emerald Pokémon sits in.
//!
//! Key items, TMs and HMs, and mail stay in their game. Bags: Bulbapedia's save data structure
//! pages (Red to Crystal) and pret's decompilations' `SaveBlock1` (Ruby to LeafGreen: each pocket
//! a list of item and quantity, the quantity scrambled with the save's security key in Emerald,
//! FireRed and LeafGreen).

use super::mon::{self, Game};
use super::save;

/// The most of one item the locker keeps.
pub const LOCKER_MAX: u32 = 999;
/// The most of one item in one bag slot.
const SLOT_MAX: u16 = 99;

/// One item in a bag: PokeAPI's number, how many, and which pocket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BagItem {
    pub item: u16,
    pub count: u16,
    pub pocket: &'static str,
}

/// Whether an item can go in the locker: not a key item, TM or HM, or mail.
pub fn depositable(item: u16) -> bool {
    item != 0 && !matches!(mon::item_pocket(item), 4 | 6 | 8)
}

/// A pocket: where it is, how many slots, its name, and which items (by PokeAPI pocket) it holds.
struct Pocket {
    at: usize,
    cap: usize,
    name: &'static str,
    holds: &'static [u8],
}

const ALL: &[u8] = &[1, 2, 3, 5, 7];
const ITEMS: &[u8] = &[1, 2, 5, 7];
const GEN3_ITEMS: &[u8] = &[1, 2, 7];
const BALLS: &[u8] = &[3];
const BERRIES: &[u8] = &[5];

fn pockets(game: Game) -> Vec<Pocket> {
    let p = |at, cap, name, holds| Pocket { at, cap, name, holds };
    match game {
        Game::Red | Game::Blue | Game::Yellow => vec![p(0x25C9, 20, "Bag", ALL)],
        Game::Gold | Game::Silver => vec![p(0x241F, 20, "Items", ITEMS), p(0x2464, 12, "Balls", BALLS)],
        Game::Crystal => vec![p(0x2420, 20, "Items", ITEMS), p(0x2465, 12, "Balls", BALLS)],
        Game::Ruby | Game::Sapphire => vec![p(0x560, 20, "Items", GEN3_ITEMS), p(0x600, 16, "Balls", BALLS), p(0x740, 46, "Berries", BERRIES)],
        Game::Emerald => vec![p(0x560, 30, "Items", GEN3_ITEMS), p(0x650, 16, "Balls", BALLS), p(0x790, 46, "Berries", BERRIES)],
        Game::FireRed | Game::LeafGreen => vec![p(0x310, 42, "Items", GEN3_ITEMS), p(0x430, 13, "Balls", BALLS), p(0x54C, 43, "Berries", BERRIES)],
    }
}

/// Where a SaveBlock1 offset of Ruby to LeafGreen is in the save file (sections 1-4, 3968 bytes
/// of it each).
pub fn block1(save: &[u8], offset: usize) -> Result<usize, String> {
    let at = save::g3_current(save)?;
    let section = 1 + offset / 3968;
    if section > 4 {
        return Err("That part of the save is out of reach.".into());
    }
    Ok(at[section] + offset % 3968)
}

/// The security key's low half, which Emerald, FireRed and LeafGreen mix into bag quantities.
pub fn key16(game: Game, save: &[u8]) -> Result<u16, String> {
    let at = save::g3_current(save)?;
    Ok(match game {
        Game::Emerald => u16::from_le_bytes([save[at[0] + 0xAC], save[at[0] + 0xAD]]),
        Game::FireRed | Game::LeafGreen => u16::from_le_bytes([save[at[0] + 0xF20], save[at[0] + 0xF21]]),
        _ => 0,
    })
}

/// A pocket's slots: (number in the game, quantity).
fn read_pocket(game: Game, save: &[u8], p: &Pocket) -> Result<Vec<(u16, u16)>, String> {
    if game.gen() < 3 {
        let n = usize::from(save[p.at]).min(p.cap);
        return Ok((0..n).map(|i| (u16::from(save[p.at + 1 + 2 * i]), u16::from(save[p.at + 2 + 2 * i]))).collect());
    }
    let base = block1(save, p.at)?;
    let key = key16(game, save)?;
    Ok((0..p.cap)
        .map(|i| {
            let a = base + 4 * i;
            (u16::from_le_bytes([save[a], save[a + 1]]), u16::from_le_bytes([save[a + 2], save[a + 3]]) ^ key)
        })
        .filter(|(id, n)| *id != 0 && *n != 0)
        .collect())
}

fn write_pocket(game: Game, save: &mut [u8], p: &Pocket, slots: &[(u16, u16)]) -> Result<(), String> {
    if slots.len() > p.cap {
        return Err(format!("The {} pocket is full.", p.name));
    }
    if game.gen() < 3 {
        save[p.at..p.at + 2 + 2 * p.cap].fill(0);
        save[p.at] = slots.len() as u8;
        for (i, (id, n)) in slots.iter().enumerate() {
            save[p.at + 1 + 2 * i] = *id as u8;
            save[p.at + 2 + 2 * i] = *n as u8;
        }
        save[p.at + 1 + 2 * slots.len()] = 0xFF;
        return Ok(());
    }
    let base = block1(save, p.at)?;
    let key = key16(game, save)?;
    for i in 0..p.cap {
        let (id, n) = slots.get(i).copied().unwrap_or((0, 0));
        let a = base + 4 * i;
        save[a..a + 2].copy_from_slice(&id.to_le_bytes());
        // An empty slot's quantity is 0 before the key, as the game leaves it.
        let q = if id == 0 { key } else { n ^ key };
        save[a + 2..a + 4].copy_from_slice(&q.to_le_bytes());
    }
    Ok(())
}

/// The items in a game's bag that can go in the locker, by pocket.
pub fn read_bag(game: Game, save: &[u8]) -> Result<Vec<BagItem>, String> {
    let gen = game.gen();
    let mut out = Vec::new();
    for p in pockets(game) {
        for (id, n) in read_pocket(game, save, &p)? {
            let item = mon::item_from_game(gen, id);
            if depositable(item) {
                out.push(BagItem { item, count: n, pocket: p.name });
            }
        }
    }
    Ok(out)
}

fn pocket_for(game: Game, item: u16) -> Option<Pocket> {
    let pk = mon::item_pocket(item);
    pockets(game).into_iter().find(|p| p.holds.contains(&pk))
}

fn game_index(game: Game, item: u16) -> Result<u16, String> {
    mon::items_of(game.gen()).iter().find(|x| x.1 == item).map(|x| x.0)
        .ok_or_else(|| format!("Pokémon {} has no {}.", game.name(), mon::item_name(item)))
}

/// Takes `n` of an item out of the bag (the save's sums are worked out again).
pub fn bag_remove(game: Game, save: &mut [u8], item: u16, n: u16) -> Result<(), String> {
    let idx = game_index(game, item)?;
    let at = block_of(game, save)?;
    let p = pocket_for(game, item).ok_or("That item isn't kept in the bag.")?;
    let mut slots = read_pocket(game, save, &p)?;
    let have: u32 = slots.iter().filter(|(id, _)| *id == idx).map(|(_, c)| u32::from(*c)).sum();
    if have < u32::from(n) {
        return Err(format!("The bag only has {have} {}.", mon::item_name(item)));
    }
    let mut left = n;
    for s in slots.iter_mut().rev().filter(|(id, _)| *id == idx) {
        let take = left.min(s.1);
        s.1 -= take;
        left -= take;
    }
    slots.retain(|(_, c)| *c > 0);
    write_pocket(game, save, &p, &slots)?;
    finish(game, save, at)
}

/// Puts `n` of an item in the bag: onto its stack, then new slots.
pub fn bag_add(game: Game, save: &mut [u8], item: u16, n: u16) -> Result<(), String> {
    let idx = game_index(game, item)?;
    let at = block_of(game, save)?;
    let p = pocket_for(game, item).ok_or("That item can't go in this game's bag.")?;
    let mut slots = read_pocket(game, save, &p)?;
    let mut left = n;
    for s in slots.iter_mut().filter(|(id, _)| *id == idx) {
        let put = left.min(SLOT_MAX - s.1.min(SLOT_MAX));
        s.1 += put;
        left -= put;
    }
    while left > 0 {
        let put = left.min(SLOT_MAX);
        slots.push((idx, put));
        left -= put;
    }
    if slots.len() > p.cap {
        return Err(format!("The {} pocket in {} has no room for that many.", p.name, game.name()));
    }
    write_pocket(game, save, &p, &slots)?;
    finish(game, save, at)
}

/// Which save block is in use, found before anything changes (after, its sums no longer match).
fn block_of(game: Game, save: &[u8]) -> Result<Option<[usize; 14]>, String> {
    Ok(if game.gen() == 3 { Some(save::g3_current(save)?) } else { None })
}

fn finish(game: Game, save: &mut [u8], at: Option<[usize; 14]>) -> Result<(), String> {
    save::seal(game, save, at.as_ref());
    Ok(())
}

// --- held items and balls --------------------------------------------------------------------------

/// What a Box Pokémon holds (PokeAPI number, 0 for nothing); None when it can't hold anything.
pub fn held(s: &mon::Stored) -> Option<u16> {
    let m = s.mon()?;
    (s.game.gen() > 1 && !m.egg).then_some(m.item)
}

/// Gives a Box Pokémon an item to hold (0 takes it off).
pub fn set_held(s: &mut mon::Stored, item: u16) -> Result<(), String> {
    let m = s.mon().ok_or("This one's data is damaged.")?;
    if s.game.gen() == 1 {
        return Err("Pokémon from Red, Blue and Yellow can't hold items.".into());
    }
    if m.egg {
        return Err("Eggs can't hold items.".into());
    }
    if mon::item_pocket(item) == 6 || mon::item_pocket(m.item) == 6 {
        return Err("Mail needs its message: give or take it in the game.".into());
    }
    let idx = if item == 0 { 0 } else { game_index(s.game, item)? };
    if s.game.gen() == 2 {
        s.data[1] = idx as u8;
        return Ok(());
    }
    let mut p = mon::gen3_parts(&s.data).ok_or("This one's data is damaged.")?;
    p[2..4].copy_from_slice(&idx.to_le_bytes());
    mon::gen3_seal(&mut s.data, &p);
    Ok(())
}

/// The ball a Ruby-to-Emerald Pokémon is in (1 Master Ball to 12 Premier Ball, the same numbers
/// as those items in those games).
pub fn ball(s: &mon::Stored) -> Option<u16> {
    if s.game.gen() != 3 {
        return None;
    }
    let p = mon::gen3_parts(&s.data)?;
    Some(u16::from_le_bytes([p[38], p[39]]) >> 11 & 15)
}

pub fn set_ball(s: &mut mon::Stored, ball_index: u16) -> Result<(), String> {
    if s.game.gen() != 3 {
        return Err("Only Pokémon from Ruby to LeafGreen remember their ball.".into());
    }
    if s.mon().is_some_and(|m| m.egg) {
        return Err("Eggs aren't in a ball yet.".into());
    }
    if !(1..=12).contains(&ball_index) {
        return Err("Choose a ball.".into());
    }
    let mut p = mon::gen3_parts(&s.data).ok_or("This one's data is damaged.")?;
    let o = u16::from_le_bytes([p[38], p[39]]) & !(15 << 11) | ball_index << 11;
    p[38..40].copy_from_slice(&o.to_le_bytes());
    mon::gen3_seal(&mut s.data, &p);
    Ok(())
}

/// A Ruby-to-LeafGreen game's flag, by its number in pret's flags.h (SaveBlock1's flag bytes are
/// at $1220 in Ruby and Sapphire, $1270 in Emerald, $EE0 in FireRed and LeafGreen).
fn flag_at(game: Game, save: &[u8], flag: u16) -> Result<(usize, u8), String> {
    let base = match game {
        Game::Ruby | Game::Sapphire => 0x1220,
        Game::Emerald => 0x1270,
        Game::FireRed | Game::LeafGreen => 0xEE0,
        _ => return Err("Only Ruby to LeafGreen have these.".into()),
    };
    Ok((block1(save, base + usize::from(flag / 8))?, 1 << (flag % 8)))
}

pub fn flag(game: Game, save: &[u8], f: u16) -> Result<bool, String> {
    let (at, bit) = flag_at(game, save, f)?;
    Ok(save[at] & bit != 0)
}

/// Sets a flag; the sums are worked out by the caller (`finish`).
pub fn set_flag(game: Game, save: &mut [u8], f: u16) -> Result<(), String> {
    let (at, bit) = flag_at(game, save, f)?;
    save[at] |= bit;
    Ok(())
}

/// The key items of a Ruby-to-LeafGreen bag (numbers in the game).
pub fn key_items(game: Game, save: &[u8]) -> Result<Vec<u16>, String> {
    let (at, cap) = match game {
        Game::Ruby | Game::Sapphire => (0x5B0, 20),
        Game::Emerald => (0x5D8, 30),
        Game::FireRed | Game::LeafGreen => (0x3B8, 30),
        _ => return Err("Only Ruby to LeafGreen have these.".into()),
    };
    let p = Pocket { at, cap, name: "Key Items", holds: &[8] };
    Ok(read_pocket(game, save, &p)?.into_iter().map(|(id, _)| id).collect())
}

/// Puts a key item (its number in the game) in the Key Items pocket, and the sums right.
pub fn add_key_item(game: Game, save: &mut [u8], id: u16) -> Result<(), String> {
    let (at, cap) = match game {
        Game::Ruby | Game::Sapphire => (0x5B0, 20),
        Game::Emerald => (0x5D8, 30),
        Game::FireRed | Game::LeafGreen => (0x3B8, 30),
        _ => return Err("Only Ruby to LeafGreen have these.".into()),
    };
    let p = Pocket { at, cap, name: "Key Items", holds: &[8] };
    let mut slots = read_pocket(game, save, &p)?;
    if !slots.iter().any(|(i, _)| *i == id) {
        slots.push((id, 1));
    }
    write_pocket(game, save, &p, &slots)
}

/// Works the sums out again for the block found before the changes (`block_of`).
pub fn seal_block(game: Game, save: &mut [u8], at: Option<[usize; 14]>) -> Result<(), String> {
    finish(game, save, at)
}

pub fn current_block(game: Game, save: &[u8]) -> Result<Option<[usize; 14]>, String> {
    block_of(game, save)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pokebox::save::tests::blank;

    #[test]
    fn each_game_bag_takes_items_in_and_out_with_its_sums_right() {
        let potion = 17; // PokeAPI's Potion
        let ultra = 2; // Ultra Ball
        for game in Game::ALL {
            let mut s = blank(game);
            bag_add(game, &mut s, potion, 5).unwrap();
            bag_add(game, &mut s, ultra, 120).unwrap();
            save::check(game, &mut s).unwrap_or_else(|e| panic!("{game:?}: {e}"));
            let bag = read_bag(game, &s).unwrap();
            let count = |item| bag.iter().filter(|b| b.item == item).map(|b| u32::from(b.count)).sum::<u32>();
            assert_eq!((count(potion), count(ultra)), (5, 120), "{game:?}: {bag:?}");
            bag_remove(game, &mut s, ultra, 110).unwrap();
            assert!(bag_remove(game, &mut s, potion, 9).is_err(), "{game:?}: not that many");
            let bag = read_bag(game, &s).unwrap();
            assert_eq!(bag.iter().filter(|b| b.item == ultra).map(|b| b.count).sum::<u16>(), 10);
            save::check(game, &mut s).unwrap();
        }
    }

    #[test]
    fn a_held_item_and_a_ball_change_and_the_pokemon_still_checks_out() {
        let m = mon::Mon { gen: 3, species: 25, exp: 100, moves: [84, 0, 0, 0], pid: 99, ot_name: "MAY".into(), ..mon::Mon::default() };
        let mut s = mon::encode(&m, Game::Emerald, 2).0;
        set_held(&mut s, 17).unwrap();
        assert_eq!(held(&s), Some(17));
        set_ball(&mut s, 1).unwrap();
        assert_eq!(ball(&s), Some(1));
        assert_eq!(s.mon().unwrap().species, 25);
        let red = mon::encode(&mon::Mon { gen: 1, ..m }, Game::Red, 2).0;
        let mut red2 = red.clone();
        assert!(set_held(&mut red2, 17).is_err());
        assert!(!depositable(mon::item_from_game(3, 275)), "the Eon Ticket stays in its game");
    }
}
