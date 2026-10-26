//! Gifts: the gift eggs of Pokémon Box Ruby & Sapphire, as G1R Deluxe hands them out. A trainer
//! who keeps Pokémon from their own Ruby-to-Emerald game in the Box earns an egg at 0, 100, 500
//! and 1499 of them: Swablu (with False Swipe), Zigzagoon (Extreme Speed), Skitty (Pay Day) and
//! Pichu (Surf), from AZUSA, in that order. The eggs go into the Box; from there, into that game.

use super::data::{EXPERIENCE, SPECIES};
use super::mon::{self, Game, Mon, Stored};
use super::save;
use super::Storage;

/// (species, how many of the trainer's Pokémon must be in the Box, moves).
pub const AWARDS: [(u16, u32, [u16; 4]); 4] = [
    (333, 0, [64, 45, 206, 0]),
    (263, 100, [33, 45, 39, 245]),
    (300, 500, [45, 33, 39, 6]),
    (172, 1499, [84, 204, 57, 0]),
];

/// A Ruby-to-LeafGreen trainer: their full ID (with the hidden half) and name.
pub fn trainer(game: Game, save: &[u8]) -> Result<(u32, String), String> {
    if game.gen() != 3 {
        return Err("The Box's gift eggs are for Ruby, Sapphire, Emerald, FireRed and LeafGreen.".into());
    }
    let at = save::g3_current(save)?;
    let s0 = at[0];
    let id = u32::from_le_bytes([save[s0 + 0x0A], save[s0 + 0x0B], save[s0 + 0x0C], save[s0 + 0x0D]]);
    Ok((id, super::text::gba_decode(&save[s0..s0 + 7])))
}

pub fn key(game: Game, id: u32, name: &str) -> String {
    format!("{}|{id}|{name}", game.name())
}

/// How many of this trainer's Pokémon are in the Box now.
pub fn kept(st: &Storage, id: u32) -> u32 {
    st.boxes.iter().flatten().flatten()
        .filter(|s| s.game.gen() == 3)
        .filter_map(Stored::mon)
        .filter(|m| (u32::from(m.ot_secret) << 16 | u32::from(m.ot_id)) == id)
        .count() as u32
}

/// Where a trainer stands: Pokémon kept now, the most ever, eggs had, and the next egg (its
/// species and how many it needs), if any is left.
pub struct Progress {
    pub now: u32,
    pub peak: u32,
    pub claimed: u8,
    pub next: Option<(u16, u32)>,
}

pub fn progress(st: &Storage, game: Game, save: &[u8]) -> Result<Progress, String> {
    let (id, name) = trainer(game, save)?;
    let now = kept(st, id);
    let (peak, claimed) = st.gifts.get(&key(game, id, &name)).copied().unwrap_or((0, 0));
    let next = AWARDS.get(usize::from(claimed)).map(|(sp, n, _)| (*sp, *n));
    Ok(Progress { now, peak: peak.max(now), claimed, next })
}

/// A tiny repeatable random source (xorshift), for the egg's personality and strengths.
fn random(seed: &mut u32) -> u32 {
    let mut x = *seed | 1;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *seed = x;
    x
}

/// The next gift egg for this trainer, into the Box. Records the gift so it is given once.
pub fn claim(st: &mut Storage, game: Game, save: &[u8], seed: u32) -> Result<(Stored, String), String> {
    let (id, name) = trainer(game, save)?;
    let p = progress(st, game, save)?;
    let Some((species, need)) = p.next else {
        return Err("This trainer has had all four of the Box's gift eggs.".into());
    };
    if p.peak < need {
        return Err(format!("The next gift egg comes when {need} of this trainer's Pokémon are in the Box at once (now {}).", p.now));
    }
    let (_, _, moves) = AWARDS[usize::from(p.claimed)];
    let sp = &SPECIES[usize::from(species)];
    let mut seed = seed ^ id;
    let pid = random(&mut seed);
    let ivs = [0; 6].map(|_| (random(&mut seed) % 32) as u8);
    let m = Mon {
        gen: 3,
        species,
        egg: true,
        ot_name: "AZUSA".into(),
        ot_female: true,
        exp: EXPERIENCE[usize::from(sp.growth).min(6)][5],
        level: 5,
        moves,
        friendship: sp.hatch,
        pid,
        ivs,
        language: 1,
        ..Mon::default()
    };
    let (egg, _) = mon::encode(&m, game, 1);
    st.gifts.insert(key(game, id, &name), (p.peak, p.claimed + 1));
    Ok((egg, format!("A gift egg from AZUSA: it will hatch into {}.", mon::species_name(species))))
}

/// Keeps the most of a trainer's Pokémon ever in the Box, after a change (called with each save
/// the Box touches).
pub fn note_peak(st: &mut Storage, game: Game, save: &[u8]) {
    let Ok((id, name)) = trainer(game, save) else { return };
    let now = kept(st, id);
    let e = st.gifts.entry(key(game, id, &name)).or_insert((0, 0));
    e.0 = e.0.max(now);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pokebox::save::tests::blank;

    #[test]
    fn the_first_egg_is_there_at_once_and_the_next_needs_a_hundred() {
        let save = blank(Game::Ruby);
        let mut st = Storage::empty();
        let p = progress(&st, Game::Ruby, &save).unwrap();
        assert_eq!(p.next, Some((333, 0)));
        let (egg, line) = claim(&mut st, Game::Ruby, &save, 7).unwrap();
        assert!(line.contains("Swablu"));
        let m = egg.mon().expect("the egg checks out");
        assert!(m.egg && m.species == 333 && m.moves[2] == 206);
        assert_eq!(m.ot_name, "AZUSA");
        assert!(claim(&mut st, Game::Ruby, &save, 7).unwrap_err().contains("100"));
        assert!(trainer(Game::Red, &blank(Game::Red)).is_err());
    }
}
