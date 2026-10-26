//! The Pokémon Box: Pokémon taken out of one game's save, kept in a box of the app's own, and put
//! into another game's save, between Red, Blue, Yellow, Gold, Silver, Crystal, Ruby, Sapphire,
//! Emerald, FireRed and LeafGreen; and a trade between two of them in one step.
//!
//! Safety, after G1R Deluxe's (bryanthaboi/gen1recomp) way of doing it:
//! - every change is written whole into a journal first (`pending.journal` in the Box's folder),
//!   then into the files, then the journal is removed. A change cut off halfway is finished from
//!   the journal the next time the Box opens, so a Pokémon is never lost or doubled;
//! - before a game's save is changed, it is copied beside itself as `<save>.before-box`;
//! - a move between generations is shown first, with everything about the Pokémon that changes.
//!
//! The games' own saves are only ever changed while the game is closed (the app makes sure).

pub mod data;
pub mod items;
pub mod mon;
pub mod save;
pub mod text;

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use mon::{Game, Stored};

pub const STORAGE_BOXES: usize = 50;
pub const STORAGE_SLOTS: usize = 30;

const BOX_FILE: &str = "box.bin";
const JOURNAL: &str = "pending.journal";
const BOX_MAGIC: &[u8; 8] = b"CPKBOX1\n";
const JOURNAL_MAGIC: &[u8; 8] = b"CPKJRN1\n";

/// One game: which it is, its language, its save file and the save's bytes.
pub struct GameSave {
    pub game: Game,
    pub language: u8,
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

/// The game a cartridge file is, if the Box knows it.
pub fn identify_rom(rom: &Path) -> Option<(Game, u8)> {
    save::identify(&save::rom_head(rom)?)
}

pub fn open_game(rom: &Path, save_path: &Path) -> Result<GameSave, String> {
    let (game, language) = identify_rom(rom).ok_or("The Pokémon Box doesn't know this game.")?;
    let mut bytes = fs::read(save_path).map_err(|_| {
        format!("Pokémon {} has no save yet. Play it and save in the game first.", game.name())
    })?;
    save::check(game, &mut bytes)?;
    Ok(GameSave { game, language, path: save_path.to_path_buf(), bytes })
}

/// The app's own box: 50 boxes of 30, its item locker, the gift progress of each game's trainer,
/// and the originals kept of Pokémon moved to another generation inside it (Migration).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Storage {
    pub boxes: Vec<Vec<Option<Stored>>>,
    /// Item locker: PokeAPI item id to how many (at most `items::LOCKER_MAX` of each).
    pub items: BTreeMap<u16, u32>,
    /// Gifts: per trainer (`gifts::key`), the most of their Ruby-to-Emerald Pokémon ever kept
    /// here at once, and how many gift eggs they have had.
    pub gifts: BTreeMap<String, (u32, u8)>,
    /// Migration: by the Pokémon's bytes now (`archive_key`), the forms it had before.
    pub archives: BTreeMap<u64, Vec<Stored>>,
}

const BOX_MAGIC_2: &[u8; 8] = b"CPKBOX2\n";

/// Which archive a Box Pokémon has: its bytes, so it is found again wherever it is moved to.
pub fn archive_key(s: &Stored) -> u64 {
    let mut b = vec![s.game.code(), u8::from(s.egg)];
    b.extend_from_slice(&s.data);
    fnv(&b)
}

impl Storage {
    pub fn empty() -> Storage {
        Storage {
            boxes: vec![vec![None; STORAGE_SLOTS]; STORAGE_BOXES],
            items: BTreeMap::new(),
            gifts: BTreeMap::new(),
            archives: BTreeMap::new(),
        }
    }

    pub fn load(dir: &Path) -> Result<Storage, String> {
        let path = dir.join(BOX_FILE);
        let Ok(bytes) = fs::read(&path) else { return Ok(Storage::empty()) };
        Storage::from_bytes(&bytes).ok_or_else(|| "The Pokémon Box's file is damaged. It has been left as it is.".into())
    }

    fn from_bytes(b: &[u8]) -> Option<Storage> {
        let mut s = Storage::empty();
        if let Some(body) = b.strip_prefix(BOX_MAGIC.as_slice()) {
            // The first layout: Pokémon only.
            let mut at = 0;
            while at < body.len() {
                let h = body.get(at..at + 6)?;
                let (bx, slot, game, egg) = (usize::from(h[0]), usize::from(h[1]), Game::from_code(h[2])?, h[3] != 0);
                let n = usize::from(u16::from_le_bytes([h[4], h[5]]));
                let data = body.get(at + 6..at + 6 + n)?.to_vec();
                *s.boxes.get_mut(bx)?.get_mut(slot)? = Some(Stored { game, egg, data });
                at += 6 + n;
            }
            return Some(s);
        }
        let body = b.strip_prefix(BOX_MAGIC_2.as_slice())?;
        let u16_at = |at: usize| body.get(at..at + 2).map(|x| u16::from_le_bytes([x[0], x[1]]));
        let u32_at = |at: usize| body.get(at..at + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]));
        let mut at = 0;
        while at < body.len() {
            match body[at] {
                b'P' => {
                    let h = body.get(at + 1..at + 7)?;
                    let (bx, slot, game, egg) = (usize::from(h[0]), usize::from(h[1]), Game::from_code(h[2])?, h[3] != 0);
                    let n = usize::from(u16::from_le_bytes([h[4], h[5]]));
                    let data = body.get(at + 7..at + 7 + n)?.to_vec();
                    *s.boxes.get_mut(bx)?.get_mut(slot)? = Some(Stored { game, egg, data });
                    at += 7 + n;
                }
                b'I' => {
                    s.items.insert(u16_at(at + 1)?, u32_at(at + 3)?);
                    at += 7;
                }
                b'G' => {
                    let n = usize::from(u16_at(at + 1)?);
                    let key = String::from_utf8(body.get(at + 3..at + 3 + n)?.to_vec()).ok()?;
                    let peak = u32_at(at + 3 + n)?;
                    let claimed = *body.get(at + 7 + n)?;
                    s.gifts.insert(key, (peak, claimed));
                    at += 8 + n;
                }
                b'A' => {
                    let key = u64::from_le_bytes(body.get(at + 1..at + 9)?.try_into().ok()?);
                    let (game, egg) = (Game::from_code(*body.get(at + 9)?)?, *body.get(at + 10)? != 0);
                    let n = usize::from(u16_at(at + 11)?);
                    let data = body.get(at + 13..at + 13 + n)?.to_vec();
                    s.archives.entry(key).or_default().push(Stored { game, egg, data });
                    at += 13 + n;
                }
                _ => return None,
            }
        }
        Some(s)
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut out = BOX_MAGIC_2.to_vec();
        let mut kept = std::collections::BTreeSet::new();
        for (b, slots) in self.boxes.iter().enumerate() {
            for (i, s) in slots.iter().enumerate() {
                if let Some(s) = s {
                    out.extend_from_slice(&[b'P', b as u8, i as u8, s.game.code(), u8::from(s.egg)]);
                    out.extend_from_slice(&(s.data.len() as u16).to_le_bytes());
                    out.extend_from_slice(&s.data);
                    kept.insert(archive_key(s));
                }
            }
        }
        for (id, n) in &self.items {
            if *n > 0 {
                out.push(b'I');
                out.extend_from_slice(&id.to_le_bytes());
                out.extend_from_slice(&n.to_le_bytes());
            }
        }
        for (key, (peak, claimed)) in &self.gifts {
            out.push(b'G');
            out.extend_from_slice(&(key.len() as u16).to_le_bytes());
            out.extend_from_slice(key.as_bytes());
            out.extend_from_slice(&peak.to_le_bytes());
            out.push(*claimed);
        }
        // Only the originals of Pokémon still here: one that left the Box leaves its archive.
        for (key, list) in self.archives.iter().filter(|(k, _)| kept.contains(k)) {
            for a in list {
                out.push(b'A');
                out.extend_from_slice(&key.to_le_bytes());
                out.extend_from_slice(&[a.game.code(), u8::from(a.egg)]);
                out.extend_from_slice(&(a.data.len() as u16).to_le_bytes());
                out.extend_from_slice(&a.data);
            }
        }
        out
    }

    fn take(&mut self, b: usize, slot: usize) -> Result<Stored, String> {
        self.boxes.get_mut(b).and_then(|x| x.get_mut(slot)).and_then(Option::take)
            .ok_or_else(|| "That Pokémon isn't there any more. Close this and open it again.".into())
    }

    fn put(&mut self, b: usize, s: Stored) -> Result<usize, String> {
        let slots = self.boxes.get_mut(b).ok_or("There's no such box.")?;
        let i = slots.iter().position(Option::is_none).ok_or("That box is full. Pick another box.")?;
        slots[i] = Some(s);
        Ok(i)
    }
}

// --- the journal ---------------------------------------------------------------------------------

fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xCBF2_9CE4_8422_2325u64, |h, x| (h ^ u64::from(*x)).wrapping_mul(0x0000_0100_0000_01B3))
}

fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("box-writing");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

/// Paths in the journal are kept from the folder above the Box's (the app's own files), so a
/// change cut off before an app update, which moves the app's folder on iOS, is still finished.
fn journal_bytes(root: &Path, writes: &[(PathBuf, Vec<u8>)]) -> Vec<u8> {
    let mut out = JOURNAL_MAGIC.to_vec();
    out.extend_from_slice(&(writes.len() as u32).to_le_bytes());
    for (p, b) in writes {
        let p = match p.strip_prefix(root) {
            Ok(rel) => format!("rel:{}", rel.to_string_lossy()),
            Err(_) => p.to_string_lossy().into_owned(),
        };
        out.extend_from_slice(&(p.len() as u32).to_le_bytes());
        out.extend_from_slice(p.as_bytes());
        out.extend_from_slice(&(b.len() as u32).to_le_bytes());
        out.extend_from_slice(b);
    }
    let h = fnv(&out);
    out.extend_from_slice(&h.to_le_bytes());
    out
}

fn parse_journal(root: &Path, b: &[u8]) -> Option<Vec<(PathBuf, Vec<u8>)>> {
    if b.len() < 20 || !b.starts_with(JOURNAL_MAGIC) {
        return None;
    }
    let (body, h) = b.split_at(b.len() - 8);
    if fnv(body) != u64::from_le_bytes(h.try_into().ok()?) {
        return None;
    }
    let u32_at = |at: usize| body.get(at..at + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]) as usize);
    let n = u32_at(8)?;
    let mut at = 12;
    let mut out = Vec::new();
    for _ in 0..n {
        let pl = u32_at(at)?;
        let p = String::from_utf8(body.get(at + 4..at + 4 + pl)?.to_vec()).ok()?;
        at += 4 + pl;
        let bl = u32_at(at)?;
        let path = match p.strip_prefix("rel:") {
            Some(rel) => root.join(rel),
            None => PathBuf::from(p),
        };
        out.push((path, body.get(at + 4..at + 4 + bl)?.to_vec()));
        at += 4 + bl;
    }
    Some(out)
}

fn root(dir: &Path) -> &Path {
    dir.parent().unwrap_or(dir)
}

/// Finishes a change that was cut off. Returns a line for the player when there was one.
pub fn recover(dir: &Path) -> Option<String> {
    let path = dir.join(JOURNAL);
    let bytes = fs::read(&path).ok()?;
    let line = match parse_journal(root(dir), &bytes) {
        Some(writes) => {
            for (p, b) in &writes {
                if let Err(e) = write_synced(p, b) {
                    return Some(format!("A change the Pokémon Box was making couldn't be finished yet: {e}. It will try again."));
                }
            }
            "The last change in the Pokémon Box was cut off, and it has now been finished.".to_owned()
        }
        // Not written to the end: the files were never touched.
        None => "The last change in the Pokémon Box was cut off before it began, so nothing changed.".to_owned(),
    };
    let _ = fs::remove_file(&path);
    Some(line)
}

/// Writes every file of one change, all or nothing (see the module notes).
fn commit(dir: &Path, writes: &[(PathBuf, Vec<u8>)]) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("The Pokémon Box's folder can't be made: {e}"))?;
    for (p, _) in writes {
        if p.file_name().is_some_and(|n| n != BOX_FILE) && p.exists() {
            let mut name = p.file_name().unwrap_or_default().to_os_string();
            name.push(".before-box");
            fs::copy(p, p.with_file_name(name)).map_err(|e| format!("The save couldn't be backed up first, so nothing changed: {e}"))?;
        }
    }
    let journal = dir.join(JOURNAL);
    write_synced(&journal, &journal_bytes(root(dir), writes)).map_err(|e| format!("Nothing changed: {e}"))?;
    for (p, b) in writes {
        write_synced(p, b).map_err(|e| format!("The change was cut off ({e}); it will be finished next time the Box opens."))?;
    }
    let _ = fs::remove_file(&journal);
    Ok(())
}

// --- what the player does -------------------------------------------------------------------------

/// What a change will do, shown before it is made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub allowed: bool,
    pub title: String,
    pub changes: Vec<String>,
}

impl Plan {
    fn refused(title: String, why: String) -> Plan {
        Plan { allowed: false, title, changes: vec![why] }
    }
}

/// A change worked out but not yet written.
struct Worked {
    plan: Plan,
    writes: Vec<(PathBuf, Vec<u8>)>,
}

fn display_name(s: &Stored) -> String {
    match s.mon() {
        Some(m) if m.egg => "The Egg".into(),
        Some(m) if m.nicknamed => format!("{} (a {})", m.nickname, mon::species_name(m.species)),
        Some(m) => mon::species_name(m.species).to_owned(),
        None => "This one".into(),
    }
}

fn traded_note(s: &Stored, g: &GameSave) -> Option<String> {
    let m = s.mon()?;
    let pc = save::read_pc(g.game, &g.bytes).ok()?;
    (m.ot_id != pc.trainer_id || m.ot_name != pc.trainer).then(|| {
        format!("In {} it counts as traded (its trainer isn't {}): it gets extra experience, and a high-level one may not obey until there are enough badges.",
                g.game.name(), pc.trainer)
    })
}

fn storage_path(dir: &Path) -> PathBuf {
    dir.join(BOX_FILE)
}

fn work_to_storage(dir: &Path, rom: &Path, save_path: &Path, b: usize, slot: usize, to_box: usize) -> Result<Worked, String> {
    let mut g = open_game(rom, save_path)?;
    let mut st = Storage::load(dir)?;
    let s = save::take(g.game, &mut g.bytes, b, slot)?;
    let name = display_name(&s);
    st.put(to_box, s)?;
    let plan = Plan {
        allowed: true,
        title: format!("{name} goes from {} into the Pokémon Box, box {}.", g.game.name(), to_box + 1),
        changes: vec!["It stays exactly as it is, and can go back to that game or into another later.".into()],
    };
    Ok(Worked { plan, writes: vec![(g.path, g.bytes), (storage_path(dir), st.to_bytes())] })
}

fn work_to_game(dir: &Path, rom: &Path, save_path: &Path, b: usize, slot: usize, to_box: usize) -> Result<Worked, String> {
    let mut g = open_game(rom, save_path)?;
    let mut st = Storage::load(dir)?;
    let s = st.take(b, slot)?;
    let name = display_name(&s);
    let title = format!("{name} goes from the Pokémon Box into {}, box {}.", g.game.name(), to_box + 1);
    let (into, mut changes) = match mon::convert(&s, g.game, g.language) {
        Ok(x) => x,
        Err(why) => return Ok(Worked { plan: Plan::refused(title, why), writes: Vec::new() }),
    };
    save::put(g.game, &mut g.bytes, to_box, &into)?;
    if let Some(m) = into.mon().filter(|m| !m.egg) {
        save::register(g.game, &mut g.bytes, m.species)?;
    }
    changes.extend(traded_note(&into, &g));
    Ok(Worked { plan: Plan { allowed: true, title, changes }, writes: vec![(g.path, g.bytes), (storage_path(dir), st.to_bytes())] })
}

fn work_party_to_storage(dir: &Path, rom: &Path, save_path: &Path, slot: usize, to_box: usize) -> Result<Worked, String> {
    let mut g = open_game(rom, save_path)?;
    let mut st = Storage::load(dir)?;
    let s = save::take_party(g.game, &mut g.bytes, slot)?;
    let name = display_name(&s);
    st.put(to_box, s)?;
    let plan = Plan {
        allowed: true,
        title: format!("{name} leaves the team in {} for the Pokémon Box, box {}.", g.game.name(), to_box + 1),
        changes: vec!["It keeps everything it has. Its health is restored when it next joins a team, as when it is taken out of a PC.".into()],
    };
    Ok(Worked { plan, writes: vec![(g.path, g.bytes), (storage_path(dir), st.to_bytes())] })
}

fn work_storage_to_party(dir: &Path, rom: &Path, save_path: &Path, b: usize, slot: usize) -> Result<Worked, String> {
    let mut g = open_game(rom, save_path)?;
    let mut st = Storage::load(dir)?;
    let s = st.take(b, slot)?;
    let name = display_name(&s);
    let title = format!("{name} joins the team in {}.", g.game.name());
    let (into, mut changes) = match mon::convert(&s, g.game, g.language) {
        Ok(x) => x,
        Err(why) => return Ok(Worked { plan: Plan::refused(title, why), writes: Vec::new() }),
    };
    changes.retain(|l| !l.starts_with("Nothing about it changes"));
    save::put_party(g.game, &mut g.bytes, &into)?;
    if let Some(m) = into.mon().filter(|m| !m.egg) {
        save::register(g.game, &mut g.bytes, m.species)?;
    }
    changes.push("Its stats are worked out for its level and its health is full, as when it is taken out of a PC.".into());
    changes.extend(traded_note(&into, &g));
    Ok(Worked { plan: Plan { allowed: true, title, changes }, writes: vec![(g.path, g.bytes), (storage_path(dir), st.to_bytes())] })
}

/// Moves a Pokémon to one place in the Box: swapped with whatever is there.
fn work_storage_place(dir: &Path, b: usize, slot: usize, to_box: usize, to_slot: usize) -> Result<Worked, String> {
    let mut st = Storage::load(dir)?;
    let s = st.take(b, slot)?;
    let name = display_name(&s);
    let there = st.boxes.get_mut(to_box).and_then(|x| x.get_mut(to_slot)).ok_or("There's no such place.")?;
    let other = there.replace(s);
    if let Some(o) = other {
        st.boxes[b][slot] = Some(o);
    }
    Ok(Worked {
        plan: Plan { allowed: true, title: format!("{name} moves to box {}, place {}.", to_box + 1, to_slot + 1), changes: Vec::new() },
        writes: vec![(storage_path(dir), st.to_bytes())],
    })
}

/// How a Box box can be put in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Number,
    Level,
    Name,
}

fn work_sort(dir: &Path, b: usize, by: Order) -> Result<Worked, String> {
    let mut st = Storage::load(dir)?;
    let slots = st.boxes.get_mut(b).ok_or("There's no such box.")?;
    let mut all: Vec<Stored> = slots.iter_mut().filter_map(Option::take).collect();
    let key = |s: &Stored| s.mon().map(|m| (m.species, m.level, if m.nicknamed { m.nickname.clone() } else { mon::species_name(m.species).to_owned() }));
    all.sort_by(|a, c| {
        let (ka, kc) = (key(a), key(c));
        match (ka, kc) {
            (Some(x), Some(y)) => match by {
                Order::Number => (x.0, std::cmp::Reverse(x.1)).cmp(&(y.0, std::cmp::Reverse(y.1))),
                Order::Level => (std::cmp::Reverse(x.1), x.0).cmp(&(std::cmp::Reverse(y.1), y.0)),
                Order::Name => x.2.cmp(&y.2),
            },
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
    for (i, s) in all.into_iter().enumerate() {
        slots[i] = Some(s);
    }
    let how = match by {
        Order::Number => "by Pokédex number",
        Order::Level => "by level, highest first",
        Order::Name => "by name",
    };
    Ok(Worked {
        plan: Plan { allowed: true, title: format!("Box {} is put in order {how}.", b + 1), changes: Vec::new() },
        writes: vec![(storage_path(dir), st.to_bytes())],
    })
}

fn work_storage_move(dir: &Path, b: usize, slot: usize, to_box: usize) -> Result<Worked, String> {
    let mut st = Storage::load(dir)?;
    let s = st.take(b, slot)?;
    let name = display_name(&s);
    st.put(to_box, s)?;
    Ok(Worked {
        plan: Plan { allowed: true, title: format!("{name} moves to box {}.", to_box + 1), changes: Vec::new() },
        writes: vec![(storage_path(dir), st.to_bytes())],
    })
}

/// One side of a trade: a game and the place in its PC.
pub struct Side<'a> {
    pub rom: &'a Path,
    pub save: &'a Path,
    pub b: usize,
    pub slot: usize,
}

fn work_trade(a: &Side, b: &Side) -> Result<Worked, String> {
    if a.save == b.save {
        return Err("Pick two different games to trade between.".into());
    }
    let mut ga = open_game(a.rom, a.save)?;
    let mut gb = open_game(b.rom, b.save)?;
    let sa = save::take(ga.game, &mut ga.bytes, a.b, a.slot)?;
    let sb = save::take(gb.game, &mut gb.bytes, b.b, b.slot)?;
    let title = format!("{} ({}) for {} ({}).", display_name(&sa), ga.game.name(), display_name(&sb), gb.game.name());
    let mut changes = Vec::new();
    let mut side = |s: &Stored, from: &GameSave, to: &GameSave| -> Result<Stored, String> {
        let (mut into, lines) = mon::convert(s, to.game, to.language)
            .map_err(|why| format!("{} can't go to {}: {why}", display_name(s), to.game.name()))?;
        let head = format!("{} going to {}:", display_name(s), to.game.name());
        let mut lines: Vec<String> = lines.into_iter().filter(|l| !l.starts_with("Nothing about it changes")).collect();
        lines.extend(mon::trade_evolve(&mut into));
        lines.extend(traded_note(&into, to));
        let _ = from;
        if lines.is_empty() {
            lines.push("Nothing about it changes.".into());
        }
        changes.push(head);
        changes.extend(lines.into_iter().map(|l| format!("• {l}")));
        Ok(into)
    };
    let a_to_b = match side(&sa, &ga, &gb) {
        Ok(x) => x,
        Err(why) => return Ok(Worked { plan: Plan::refused(title, why), writes: Vec::new() }),
    };
    let b_to_a = match side(&sb, &gb, &ga) {
        Ok(x) => x,
        Err(why) => return Ok(Worked { plan: Plan::refused(title, why), writes: Vec::new() }),
    };
    save::put(gb.game, &mut gb.bytes, b.b, &a_to_b)?;
    save::put(ga.game, &mut ga.bytes, a.b, &b_to_a)?;
    for (g, s) in [(&mut ga, &b_to_a), (&mut gb, &a_to_b)] {
        if let Some(m) = s.mon().filter(|m| !m.egg) {
            save::register(g.game, &mut g.bytes, m.species)?;
        }
    }
    Ok(Worked { plan: Plan { allowed: true, title, changes }, writes: vec![(ga.path, ga.bytes), (gb.path, gb.bytes)] })
}

/// The kinds of change, for `plan` and `apply`.
pub enum Change<'a> {
    ToStorage { rom: &'a Path, save: &'a Path, b: usize, slot: usize, to_box: usize },
    ToGame { rom: &'a Path, save: &'a Path, b: usize, slot: usize, to_box: usize },
    InStorage { b: usize, slot: usize, to_box: usize },
    /// One place in the Box to another (swapped with what is there).
    InStorageTo { b: usize, slot: usize, to_box: usize, to_slot: usize },
    /// A Pokémon from a game's team into the Box.
    PartyToStorage { rom: &'a Path, save: &'a Path, slot: usize, to_box: usize },
    /// A Pokémon from the Box into a game's team.
    StorageToParty { rom: &'a Path, save: &'a Path, b: usize, slot: usize },
    /// A Box box put in order.
    Sort { b: usize, by: Order },
    Trade { a: Side<'a>, b: Side<'a> },
}

fn work(dir: &Path, c: &Change) -> Result<Worked, String> {
    if let Some(line) = recover(dir) {
        log::info!("pokebox: {line}");
    }
    match c {
        Change::ToStorage { rom, save, b, slot, to_box } => work_to_storage(dir, rom, save, *b, *slot, *to_box),
        Change::ToGame { rom, save, b, slot, to_box } => work_to_game(dir, rom, save, *b, *slot, *to_box),
        Change::InStorage { b, slot, to_box } => work_storage_move(dir, *b, *slot, *to_box),
        Change::InStorageTo { b, slot, to_box, to_slot } => work_storage_place(dir, *b, *slot, *to_box, *to_slot),
        Change::PartyToStorage { rom, save, slot, to_box } => work_party_to_storage(dir, rom, save, *slot, *to_box),
        Change::Sort { b, by } => work_sort(dir, *b, *by),
        Change::StorageToParty { rom, save, b, slot } => work_storage_to_party(dir, rom, save, *b, *slot),
        Change::Trade { a, b } => work_trade(a, b),
    }
}

// --- Pokémon files ----------------------------------------------------------------------------------

/// A Box Pokémon as the file other Pokémon tools (PKHeX among them) read: .pk1 (69 bytes) or .pk2
/// (73), a one-Pokémon list in the team's form; .pk3, the 100 bytes of a team Pokémon. Returns the
/// file's name and its bytes.
pub fn export(dir: &Path, b: usize, slot: usize) -> Result<(String, Vec<u8>), String> {
    let st = Storage::load(dir)?;
    let s = st.boxes.get(b).and_then(|x| x.get(slot)).and_then(Option::as_ref)
        .ok_or("That Pokémon isn't there any more. Close this and open it again.")?;
    let m = s.mon().ok_or("This one's data is damaged (a Bad Egg), so it can't be saved as a file.")?;
    let tail = mon::party_tail(s).ok_or("This one's data is damaged.")?;
    let name = if m.egg { "Egg".to_owned() } else if m.nicknamed { m.nickname.clone() } else { mon::species_name(m.species).to_owned() };
    let clean: String = name.chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect();
    let gen = s.game.gen();
    let bytes = match gen {
        1 | 2 => {
            let keep = if gen == 1 { mon::GEN1_LEN } else { mon::GEN2_LEN };
            let mut out = vec![1, if s.egg { 0xFD } else { s.data[0] }, 0xFF];
            let mut party = s.data[..keep].to_vec();
            party.extend_from_slice(&tail);
            out.extend_from_slice(&party);
            out.extend_from_slice(&s.data[keep..]);
            out
        }
        _ => {
            let mut out = s.data.clone();
            out.extend_from_slice(&tail);
            out
        }
    };
    Ok((format!("{:03} {}.pk{gen}", m.species, clean.trim()), bytes))
}

/// The game a Pokémon file's Pokémon is kept as: its generation's, and for Ruby to LeafGreen the
/// game it was caught in when that is one of them.
fn file_game(bytes: &[u8]) -> Option<(Game, Vec<u8>, bool)> {
    match bytes.len() {
        69 if bytes[0] == 1 && bytes[2] == 0xFF => {
            let mut d = bytes[3..3 + mon::GEN1_LEN].to_vec();
            d.extend_from_slice(&bytes[3 + 44..]);
            Some((Game::Red, d, false))
        }
        73 if bytes[0] == 1 && bytes[2] == 0xFF => {
            let mut d = bytes[3..3 + mon::GEN2_LEN].to_vec();
            d.extend_from_slice(&bytes[3 + 48..]);
            let crystal = d[29] != 0 || d[30] != 0;
            Some((if crystal { Game::Crystal } else { Game::Gold }, d, bytes[1] == 0xFD))
        }
        80 | 100 => {
            let d = bytes[..mon::GEN3_LEN].to_vec();
            mon::gen3_parts(&d)?;
            let game = Game::ALL.iter().copied().find(|g| g.gen() == 3 && {
                let s = Stored { game: *g, egg: false, data: d.clone() };
                s.mon().is_some_and(|m| m.origin == Some(*g))
            });
            Some((game.unwrap_or(Game::Emerald), d, false))
        }
        _ => None,
    }
}

/// A Pokémon file into the Box, box `to_box`.
pub fn import(dir: &Path, bytes: &[u8], to_box: usize) -> Result<Plan, String> {
    if let Some(line) = recover(dir) {
        log::info!("pokebox: {line}");
    }
    let (game, data, egg) = file_game(bytes)
        .ok_or("That isn't a Pokémon file the Box can read (.pk1, .pk2 or .pk3 from Red to Emerald).")?;
    let s = Stored { game, egg, data };
    if s.mon().is_none() {
        return Err("That file's Pokémon doesn't add up (its data is damaged), so it was left out.".into());
    }
    let name = display_name(&s);
    let mut st = Storage::load(dir)?;
    let at = st.put(to_box, s)?;
    commit(dir, &[(storage_path(dir), st.to_bytes())])?;
    Ok(Plan { allowed: true, title: format!("{name} is in the Pokémon Box, box {}, place {}.", to_box + 1, at + 1), changes: Vec::new() })
}

/// What the change would do, without doing it.
pub fn plan(dir: &Path, c: &Change) -> Plan {
    match work(dir, c) {
        Ok(w) => w.plan,
        Err(why) => Plan::refused("This can't be done.".into(), why),
    }
}

/// Makes the change. Returns its plan (title and lines) when done.
pub fn apply(dir: &Path, c: &Change) -> Result<Plan, String> {
    let w = work(dir, c)?;
    if !w.plan.allowed {
        return Err(w.plan.changes.join(" "));
    }
    commit(dir, &w.writes)?;
    Ok(w.plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mon::{encode, Mon};

    struct Place {
        dir: PathBuf,
    }

    impl Place {
        fn new(tag: &str) -> Place {
            let dir = std::env::temp_dir().join(format!("pokebox-test-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Place { dir }
        }

        /// A cartridge file the Box recognises as `game`, and a fresh save for it.
        fn game(&self, game: Game) -> (PathBuf, PathBuf) {
            let rom = self.dir.join(format!("{}.rom", game.name()));
            let mut r = vec![0u8; 0x10000];
            match game.gen() {
                3 => {
                    let code: &[u8; 4] = match game {
                        Game::Ruby => b"AXVE",
                        Game::Sapphire => b"AXPE",
                        Game::Emerald => b"BPEE",
                        Game::FireRed => b"BPRE",
                        _ => b"BPGE",
                    };
                    r[0xAC..0xB0].copy_from_slice(code);
                    r[0xB2] = 0x96;
                }
                _ => {
                    let t: &[u8] = match game {
                        Game::Red => b"POKEMON RED",
                        Game::Blue => b"POKEMON BLUE",
                        Game::Yellow => b"POKEMON YELLOW",
                        Game::Gold => b"POKEMON_GLDAAUE",
                        Game::Silver => b"POKEMON_SLVAAXE",
                        _ => b"PM_CRYSTAL\0BYTE",
                    };
                    r[0x134..0x134 + t.len()].copy_from_slice(t);
                    r[0x14A] = 1;
                }
            }
            fs::write(&rom, r).unwrap();
            let sav = self.dir.join(format!("{}.srm", game.name()));
            fs::write(&sav, save::tests::blank(game)).unwrap();
            (rom, sav)
        }

        fn add(&self, sav: &Path, game: Game, m: &Mon) {
            let mut b = fs::read(sav).unwrap();
            save::put(game, &mut b, 0, &encode(m, game, 2).0).unwrap();
            fs::write(sav, b).unwrap();
        }
    }

    impl Drop for Place {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn charmander() -> Mon {
        Mon { gen: 1, species: 4, ot_name: "ASH".into(), ot_id: 4321, exp: 2000, moves: [10, 45, 0, 0], dvs: [9, 8, 7, 6], ..Mon::default() }
    }

    #[test]
    fn red_to_the_box_to_firered_shows_the_changes_then_makes_them() {
        let p = Place::new("move");
        let (red, red_sav) = p.game(Game::Red);
        let (fr, fr_sav) = p.game(Game::FireRed);
        p.add(&red_sav, Game::Red, &charmander());
        let out = Change::ToStorage { rom: &red, save: &red_sav, b: 0, slot: 0, to_box: 2 };
        let before = fs::read(&red_sav).unwrap();
        assert!(plan(&p.dir, &out).allowed);
        assert_eq!(fs::read(&red_sav).unwrap(), before, "a plan changes nothing");
        apply(&p.dir, &out).unwrap();
        assert!(red_sav.with_file_name("Red.srm.before-box").exists());
        let st = Storage::load(&p.dir).unwrap();
        assert_eq!(st.boxes[2][0].as_ref().unwrap().mon().unwrap().species, 4);
        let red_pc = save::read_pc(Game::Red, &fs::read(&red_sav).unwrap()).unwrap();
        assert!(red_pc.boxes[0].slots.is_empty());

        let into = Change::ToGame { rom: &fr, save: &fr_sav, b: 2, slot: 0, to_box: 0 };
        let pl = plan(&p.dir, &into);
        assert!(pl.allowed && pl.changes.iter().any(|c| c.contains("Nature")), "{pl:?}");
        apply(&p.dir, &into).unwrap();
        let mut fr_bytes = fs::read(&fr_sav).unwrap();
        save::check(Game::FireRed, &mut fr_bytes).unwrap();
        let pc = save::read_pc(Game::FireRed, &fr_bytes).unwrap();
        let m = pc.boxes[0].slots[0].as_ref().unwrap().mon().unwrap();
        assert_eq!((m.species, m.ot_name.as_str(), m.exp), (4, "ASH", 2000));
        assert!(Storage::load(&p.dir).unwrap().boxes[2][0].is_none());
    }

    #[test]
    fn a_trade_between_gold_and_ruby_evolves_kadabra_and_swaps_both() {
        let p = Place::new("trade");
        let (gold, gold_sav) = p.game(Game::Gold);
        let (ruby, ruby_sav) = p.game(Game::Ruby);
        p.add(&gold_sav, Game::Gold, &Mon { gen: 2, species: 64, ot_name: "GOLD".into(), ot_id: 4321, exp: 9000, moves: [93, 0, 0, 0], ..Mon::default() });
        p.add(&ruby_sav, Game::Ruby, &Mon { gen: 3, species: 252, ot_name: "MAY".into(), ot_id: 4321, exp: 100, moves: [1, 0, 0, 0], pid: 42, ..Mon::default() });
        let t = Change::Trade {
            a: Side { rom: &gold, save: &gold_sav, b: 0, slot: 0 },
            b: Side { rom: &ruby, save: &ruby_sav, b: 0, slot: 0 },
        };
        let pl = plan(&p.dir, &t);
        assert!(!pl.allowed, "Treecko can't go to Gold");
        assert!(pl.changes[0].contains("first 251"));
        // Now a Pokémon Gold has.
        let mut b = fs::read(&ruby_sav).unwrap();
        save::take(Game::Ruby, &mut b, 0, 0).unwrap();
        fs::write(&ruby_sav, &b).unwrap();
        p.add(&ruby_sav, Game::Ruby, &Mon { gen: 3, species: 25, ot_name: "MAY".into(), ot_id: 9, exp: 100, moves: [84, 0, 0, 0], pid: 42, ..Mon::default() });
        let pl = apply(&p.dir, &t).unwrap();
        assert!(pl.changes.iter().any(|c| c.contains("Alakazam")), "{pl:?}");
        let mut rb = fs::read(&ruby_sav).unwrap();
        save::check(Game::Ruby, &mut rb).unwrap();
        let in_ruby = save::read_pc(Game::Ruby, &rb).unwrap().boxes[0].slots.iter().flatten().next().unwrap().mon().unwrap();
        assert_eq!(in_ruby.species, 65);
        let in_gold = save::read_pc(Game::Gold, &fs::read(&gold_sav).unwrap()).unwrap().boxes[0].slots[0].clone().unwrap().mon().unwrap();
        assert_eq!(in_gold.species, 25);
    }

    #[test]
    fn a_change_cut_off_after_its_journal_is_finished_next_time_and_a_torn_journal_changes_nothing() {
        let p = Place::new("journal");
        let a = p.dir.join("a.srm");
        let b = p.dir.join("b.srm");
        fs::write(&a, b"old a").unwrap();
        fs::write(&b, b"old b").unwrap();
        let writes = vec![(a.clone(), b"new a".to_vec()), (b.clone(), b"new b".to_vec())];
        let j = journal_bytes(root(&p.dir), &writes);
        fs::write(p.dir.join(JOURNAL), &j).unwrap();
        fs::write(&a, b"new a").unwrap(); // the power went after the first file
        assert!(recover(&p.dir).unwrap().contains("finished"));
        assert_eq!(fs::read(&b).unwrap(), b"new b");
        assert!(!p.dir.join(JOURNAL).exists());
        fs::write(p.dir.join(JOURNAL), &j[..j.len() - 3]).unwrap();
        assert!(recover(&p.dir).unwrap().contains("nothing changed"));
        assert!(recover(&p.dir).is_none());
    }

    #[test]
    fn a_box_is_put_in_order_and_a_pokemon_swaps_places() {
        let p = Place::new("sort");
        let mut st = Storage::empty();
        for (i, (sp, exp)) in [(25u16, 100u32), (1, 9000), (150, 50)].into_iter().enumerate() {
            st.boxes[0][i * 3] = Some(encode(&Mon { gen: 1, species: sp, exp, moves: [1, 0, 0, 0], ot_name: "A".into(), ..Mon::default() }, Game::Red, 2).0);
        }
        fs::write(p.dir.join(BOX_FILE), st.to_bytes()).unwrap();
        apply(&p.dir, &Change::Sort { b: 0, by: Order::Number }).unwrap();
        let order = |st: &Storage| st.boxes[0].iter().take(3).map(|s| s.as_ref().map(|x| x.mon().unwrap().species)).collect::<Vec<_>>();
        assert_eq!(order(&Storage::load(&p.dir).unwrap()), [Some(1), Some(25), Some(150)]);
        apply(&p.dir, &Change::Sort { b: 0, by: Order::Level }).unwrap();
        assert_eq!(order(&Storage::load(&p.dir).unwrap())[0], Some(1), "level 30-odd Bulbasaur first");
        apply(&p.dir, &Change::InStorageTo { b: 0, slot: 0, to_box: 0, to_slot: 2 }).unwrap();
        let st = Storage::load(&p.dir).unwrap();
        assert_eq!(st.boxes[0][2].as_ref().unwrap().mon().unwrap().species, 1, "swapped");
        assert!(st.boxes[0][0].is_some());
    }

    #[test]
    fn a_box_pokemon_joins_a_team_in_another_generation() {
        let p = Place::new("team");
        let (fr, fr_sav) = p.game(Game::FireRed);
        let mut st = Storage::empty();
        st.boxes[1][4] = Some(encode(&charmander(), Game::Red, 2).0);
        fs::write(p.dir.join(BOX_FILE), st.to_bytes()).unwrap();
        let pl = apply(&p.dir, &Change::StorageToParty { rom: &fr, save: &fr_sav, b: 1, slot: 4 }).unwrap();
        assert!(pl.changes.iter().any(|c| c.contains("stats")), "{pl:?}");
        let mut b = fs::read(&fr_sav).unwrap();
        save::check(Game::FireRed, &mut b).unwrap();
        let team = save::read_party(Game::FireRed, &b).unwrap();
        assert_eq!(team.last().unwrap().mon().unwrap().species, 4);
    }

    #[test]
    fn a_pokemon_goes_out_as_a_file_and_comes_back_the_same() {
        let p = Place::new("files");
        let mut st = Storage::empty();
        st.boxes[0][0] = Some(encode(&charmander(), Game::Yellow, 2).0);
        st.boxes[0][1] = Some(encode(&Mon { gen: 2, species: 152, exp: 400, moves: [33, 0, 0, 0], ot_name: "GOLD".into(), ..Mon::default() }, Game::Gold, 2).0);
        st.boxes[0][2] = Some(encode(&Mon { gen: 3, species: 258, exp: 300, moves: [33, 0, 0, 0], ot_name: "MAY".into(), pid: 77, ..Mon::default() }, Game::Ruby, 2).0);
        fs::write(p.dir.join(BOX_FILE), st.to_bytes()).unwrap();
        for (slot, len, ext) in [(0, 69, "pk1"), (1, 73, "pk2"), (2, 100, "pk3")] {
            let (name, bytes) = export(&p.dir, 0, slot).unwrap();
            assert!(name.ends_with(ext) && bytes.len() == len, "{name} {}", bytes.len());
            import(&p.dir, &bytes, 3).unwrap();
            let back = Storage::load(&p.dir).unwrap().boxes[3][slot].clone().unwrap();
            assert_eq!(back.mon().unwrap().species, st.boxes[0][slot].as_ref().unwrap().mon().unwrap().species);
            assert_eq!(back.data, st.boxes[0][slot].as_ref().unwrap().data, "{ext}: byte for byte");
        }
        assert!(import(&p.dir, b"not a pokemon", 3).is_err());
    }

    #[test]
    fn the_box_file_keeps_every_pokemon_where_it_was() {
        let mut st = Storage::empty();
        st.boxes[49][29] = Some(encode(&charmander(), Game::Yellow, 2).0);
        st.boxes[0][3] = Some(encode(&Mon { gen: 3, species: 300, exp: 9, moves: [1, 0, 0, 0], pid: 5, ..Mon::default() }, Game::Emerald, 2).0);
        assert_eq!(Storage::from_bytes(&st.to_bytes()).unwrap(), st);
    }
}
