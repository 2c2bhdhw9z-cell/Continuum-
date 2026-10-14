//! The sets cArcade knows, and how a zip is recognised as one of them (docs/ARCADE_CORE.md, "How a
//! game is recognised").
//!
//! Arcade games are zips named after the game (pacman.zip) holding the board's ROM chips. Each chip
//! is found by its CRC32 and size from the zip's own directory, never by its file name, which makes
//! merged, split and non-merged sets the same problem: a non-merged zip holds every chip itself, a
//! split clone's zip holds only the chips that differ from its parent (the rest come from the
//! parent's zip, found next to it in the same folder), and a merged parent's zip holds its clones'
//! chips too. The list itself is `set_list.rs`, written by scripts/make-set-list.py.

use std::path::{Path, PathBuf};

use crate::pacman::{self, Variant};
use crate::zip::Zip;

/// Which board a set runs on, and how its program ROMs sit on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Board {
    Pacman(Variant),
}

/// Which DIP switches a set has (`options.rs` turns them into settings).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dips {
    /// Puck Man, Pac-Man, Pac-Man Plus and most of their clones.
    Pacman,
    /// Ms. Pac-Man and its bootlegs: Pac-Man's switches without the ghost names.
    MsPacman,
    /// Come Come: its own coinage and lives.
    ComeCome,
    /// Newpuc2: its own bonus-life scores.
    Newpuc2,
    /// Pacu-Man: its own coinage.
    Pacuman,
    /// The Spanish Puck Man: its Hard setting breaks the game, so it isn't offered.
    PacmanSp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Good,
    /// MAME marks the dump as probably bad; it's still the chip every copy of the set has.
    BadDump,
}

/// One ROM chip of a set.
#[derive(Debug)]
pub struct Rom {
    pub name: &'static str,
    pub size: u32,
    pub crc: u32,
    /// Its name in the parent's set, when the parent has the same chip.
    pub merge: Option<&'static str>,
    pub status: Status,
    /// The board reads it. Chips nothing reads (Pac-Man's timing PROM, a PAL's dump) needn't be
    /// in the zip.
    pub needed: bool,
}

/// One step of laying a set's chips out in a region, as the board's address decoding sees them.
#[derive(Debug)]
pub enum Op {
    /// `len` bytes of chip `rom`, from `from` in the file, to `at` in the region.
    Load { rom: usize, at: usize, from: usize, len: usize },
    /// A patch over a known bad byte of a dump.
    Fill { at: usize, len: usize, value: u8 },
    /// Bytes of an earlier region (a big EPROM a bootleg's board splits up).
    Copy { region: usize, from: usize, at: usize, len: usize },
}

#[derive(Debug)]
pub struct Region {
    pub tag: &'static str,
    pub size: usize,
    pub erase: u8,
    pub ops: &'static [Op],
}

/// A set: what MAME calls it, its title, and its chips.
#[derive(Debug)]
pub struct SetSpec {
    pub name: &'static str,
    pub title: &'static str,
    pub year: &'static str,
    pub maker: &'static str,
    pub parent: Option<&'static str>,
    /// The set whose zip holds the chips this one shares (its parent).
    pub romof: Option<&'static str>,
    pub board: Board,
    pub dips: Dips,
    /// MAME's factory setting of the set's switches: (port, the bits its switches cover, their
    /// setting). `options.rs`'s defaults are held to it.
    pub factory: &'static [(&'static str, u8, u8)],
    pub roms: &'static [Rom],
    pub regions: &'static [Region],
}

pub use crate::set_list::SETS;

impl SetSpec {
    /// The title without what MAME says in brackets: "Pac-Man (Midway)" is "Pac-Man".
    pub fn short_title(&self) -> &'static str {
        self.title.split(" (").next().unwrap_or(self.title).trim()
    }
}

pub fn find(name: &str) -> Option<&'static SetSpec> {
    let name = name.to_ascii_lowercase();
    SETS.iter().find(|s| s.name == name)
}

/// The games named in the "can't play this one yet" line, per board.
fn headline(board: Board) -> &'static str {
    match board {
        Board::Pacman(_) => "Pac-Man, Ms. Pac-Man",
    }
}

/// What a player reads when cArcade doesn't know a game, built from the boards it plays.
pub fn plays_now() -> String {
    let mut games: Vec<&str> = Vec::new();
    let mut boards = 0;
    for s in SETS {
        let h = headline(s.board);
        if !games.contains(&h) {
            games.push(h);
            boards += 1;
        }
    }
    format!(
        "Our arcade emulator can\u{2019}t play this one yet. Right now it plays {} and games on the same board{}.",
        games.join(", "),
        if boards > 1 { "s" } else { "" }
    )
}

/// Why a zip can't be played, in the words the player reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Not a set cArcade knows.
    Unknown,
    /// A split clone whose parent zip isn't next to it.
    NeedsParent { parent: String },
    /// A known set with a chip missing or damaged.
    MissingPiece { title: String, set: String },
    /// Not a zip at all, or a damaged one.
    NotAZip,
}

impl Refusal {
    pub fn line(&self) -> String {
        match self {
            Refusal::Unknown => plays_now(),
            Refusal::NeedsParent { parent } => format!("This one needs {parent}.zip next to it."),
            Refusal::MissingPiece { title, set } => format!(
                "This copy of {title} is missing a piece. A complete copy of {set}.zip fixes it."
            ),
            Refusal::NotAZip => {
                "This file isn\u{2019}t a working arcade zip. Another copy of the game usually fixes it."
                    .into()
            }
        }
    }
}

/// A zip and where it came from.
pub struct Archive {
    pub zip: Zip,
    /// Its file name without `.zip`, lower case: the set it's named after.
    pub stem: String,
}

/// Where a chip's bytes are: which archive (0 is the game's own zip, then its parents) and which
/// entry in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Place {
    archive: usize,
    entry: usize,
}

/// A set recognised in the archives, with where each chip it needs is.
pub struct Found {
    pub spec: &'static SetSpec,
    places: Vec<Option<Place>>,
    /// Chips found by name and size whose contents differ from the set's: a hack or homebrew on
    /// that board, which plays anyway (MAME runs it too, after a warning).
    pub differing: Vec<&'static str>,
    archives: Vec<Archive>,
}

/// Looks up the zip of a set next to the game: `<name>.zip` in the same folder, any letter case.
pub fn sibling(path: &Path, name: &str) -> Option<PathBuf> {
    let dir = path.parent()?;
    let want = format!("{name}.zip");
    let exact = dir.join(&want);
    if exact.is_file() {
        return Some(exact);
    }
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case(&want))
        })
}

/// Where each needed chip of `spec` is, by CRC32 and size: in the game's zip first, then in its
/// parents' zips. `None` for each chip that isn't anywhere.
fn by_crc(spec: &SetSpec, archives: &[Archive]) -> Vec<Option<Place>> {
    spec.roms
        .iter()
        .map(|rom| {
            archives.iter().enumerate().find_map(|(a, ar)| {
                ar.zip
                    .entries()
                    .iter()
                    .position(|e| e.crc == rom.crc && e.size == u64::from(rom.size))
                    .map(|entry| Place { archive: a, entry })
            })
        })
        .collect()
}

fn complete(spec: &SetSpec, places: &[Option<Place>]) -> bool {
    spec.roms.iter().zip(places).all(|(r, p)| !r.needed || p.is_some())
}

/// Recognises the game. `stem` is the zip's file name without `.zip`; `parent_zip` finds the zip of
/// a set next to the game, if it's there.
pub fn identify(
    game: Archive,
    parent_zip: &dyn Fn(&str) -> Option<Archive>,
) -> Result<Found, Refusal> {
    // The parents a set's chips may come from: its romof chain, as far as zips are there.
    let with_parents = |spec: &SetSpec, game: Archive| -> Vec<Archive> {
        let mut archives = vec![game];
        let mut next = spec.romof;
        while let Some(name) = next {
            if archives.iter().any(|a| a.stem == name) {
                break;
            }
            match parent_zip(name) {
                Some(a) => archives.push(a),
                None => break,
            }
            next = find(name).and_then(|p| p.romof);
        }
        archives
    };
    let stem = game.stem.clone();
    let named = find(&stem);

    // 1. The set the zip is named after, with every chip there by CRC.
    if let Some(spec) = named {
        let archives = with_parents(spec, game);
        let places = by_crc(spec, &archives);
        if complete(spec, &places) {
            return Ok(Found { spec, places, differing: Vec::new(), archives });
        }
        // Keep the game's own zip for the steps below.
        let game = archives.into_iter().next().expect("the game's own zip");
        return identify_unnamed(game, Some(spec), &with_parents);
    }
    identify_unnamed(game, None, &with_parents)
}

fn identify_unnamed(
    game: Archive,
    named: Option<&'static SetSpec>,
    with_parents: &dyn Fn(&SetSpec, Archive) -> Vec<Archive>,
) -> Result<Found, Refusal> {
    // 2. Any set whose chips are all there by CRC (a renamed zip). The one with the most chips in
    // the zip itself wins, so a clone beats its parent; a merged parent's zip plays the parent.
    let mut best: Option<(&'static SetSpec, usize)> = None;
    let own = Archive { zip: game.zip, stem: game.stem };
    let lone = [own];
    for spec in SETS {
        let places = by_crc(spec, &lone);
        let in_zip = places.iter().filter(|p| p.is_some()).count();
        if in_zip == 0 {
            continue;
        }
        let whole = complete(spec, &places);
        if whole && best.is_none_or(|(_, n)| in_zip > n) {
            best = Some((spec, in_zip));
        }
    }
    let [own] = lone;
    if let Some((spec, _)) = best {
        let archives = vec![own];
        let places = by_crc(spec, &archives);
        return Ok(Found { spec, places, differing: Vec::new(), archives });
    }
    // Split clones under another name: their own chips here, the rest in their parent's zip.
    let mut best_split: Option<(&'static SetSpec, usize)> = None;
    for spec in SETS.iter().filter(|s| s.romof.is_some()) {
        let in_zip = by_crc(spec, std::slice::from_ref(&own)).iter().filter(|p| p.is_some()).count();
        if in_zip > 0 && best_split.is_none_or(|(_, n)| in_zip > n) {
            best_split = Some((spec, in_zip));
        }
    }
    if named.is_none() {
        if let Some((spec, _)) = best_split {
            let archives = with_parents(spec, own);
            let places = by_crc(spec, &archives);
            if complete(spec, &places) {
                return Ok(Found { spec, places, differing: Vec::new(), archives });
            }
            // Its own chips are here and its parent's aren't.
            if archives.len() == 1 {
                return Err(Refusal::NeedsParent { parent: spec.romof.unwrap_or_default().into() });
            }
            return Err(Refusal::MissingPiece { title: spec.short_title().into(), set: spec.name.into() });
        }
        return Err(Refusal::Unknown);
    }

    // 3. Named after a known set, with every chip there by name and size even though some differ
    // by CRC: a hack or homebrew on that board, which plays.
    let spec = named.expect("checked above");
    let archives = with_parents(spec, own);
    let crc_places = by_crc(spec, &archives);
    let mut places = Vec::with_capacity(spec.roms.len());
    let mut differing = Vec::new();
    let mut all_there = true;
    for (rom, by_crc) in spec.roms.iter().zip(&crc_places) {
        if let Some(p) = by_crc {
            places.push(Some(*p));
            continue;
        }
        let by_name = archives.iter().enumerate().find_map(|(a, ar)| {
            ar.zip
                .entries()
                .iter()
                .position(|e| {
                    let base = e.base_name();
                    e.size == u64::from(rom.size)
                        && (base == rom.name.to_ascii_lowercase()
                            || rom.merge.is_some_and(|m| a > 0 && base == m.to_ascii_lowercase()))
                })
                .map(|entry| Place { archive: a, entry })
        });
        if by_name.is_some() && rom.needed {
            differing.push(rom.name);
        }
        if by_name.is_none() && rom.needed {
            all_there = false;
        }
        places.push(by_name);
    }
    if all_there {
        return Ok(Found { spec, places, differing, archives });
    }
    // 4. Not playable: say what would fix it.
    let missing_from_parent = spec.romof.is_some()
        && archives.len() == 1
        && spec.roms.iter().zip(&places).any(|(r, p)| r.needed && p.is_none() && r.merge.is_some());
    if missing_from_parent {
        return Err(Refusal::NeedsParent { parent: spec.romof.unwrap_or_default().into() });
    }
    Err(Refusal::MissingPiece { title: spec.short_title().into(), set: spec.name.into() })
}

impl Found {
    /// The chips that weren't found (only ones the board doesn't read can be missing).
    pub fn absent(&self) -> Vec<&'static str> {
        self.spec
            .roms
            .iter()
            .zip(&self.places)
            .filter(|(_, p)| p.is_none())
            .map(|(r, _)| r.name)
            .collect()
    }

    /// Lays the set's chips out in its regions, as MAME's ROM loading does. Fails when a chip's
    /// bytes don't unpack (a damaged zip), with the reason for the log.
    pub fn regions(&self) -> Result<Vec<Vec<u8>>, String> {
        let mut chips: Vec<Option<Vec<u8>>> = vec![None; self.spec.roms.len()];
        let mut out: Vec<Vec<u8>> = Vec::new();
        for region in self.spec.regions {
            let mut r = vec![region.erase; region.size];
            for op in region.ops {
                match *op {
                    Op::Load { rom, at, from, len } => {
                        let Some(place) = self.places[rom] else { continue };
                        if chips[rom].is_none() {
                            chips[rom] = Some(self.archives[place.archive].zip.read(place.entry)?);
                        }
                        let bytes = chips[rom].as_ref().expect("just read");
                        let src = bytes
                            .get(from..from + len)
                            .ok_or_else(|| format!("{} is too short", self.spec.roms[rom].name))?;
                        r.get_mut(at..at + len)
                            .ok_or_else(|| format!("{} doesn't fit its region", self.spec.roms[rom].name))?
                            .copy_from_slice(src);
                    }
                    Op::Fill { at, len, value } => {
                        if let Some(dst) = r.get_mut(at..at + len) {
                            dst.fill(value);
                        }
                    }
                    Op::Copy { region: src, from, at, len } => {
                        let bytes = out
                            .get(src)
                            .and_then(|s| s.get(from..from + len))
                            .ok_or("a copy from outside its region")?
                            .to_vec();
                        r.get_mut(at..at + len)
                            .ok_or("a copy that doesn't fit its region")?
                            .copy_from_slice(&bytes);
                    }
                }
            }
            out.push(r);
        }
        Ok(out)
    }

    /// The region called `tag`, or an empty one.
    fn region<'a>(&self, regions: &'a [Vec<u8>], tag: &str) -> &'a [u8] {
        self.spec
            .regions
            .iter()
            .position(|r| r.tag == tag)
            .map_or(&[][..], |i| &regions[i][..])
    }

    /// The Pac-Man board's ROMs, from the regions as MAME's sets lay them out: maincpu, gfx1
    /// (tiles then sprites), proms (the palette, then the colour lookup at 20h) and namco (the
    /// sound PROM).
    pub fn pacman_roms(&self, regions: &[Vec<u8>]) -> pacman::Roms {
        let get = |tag: &str, from: usize, len: usize| -> Vec<u8> {
            let r = self.region(regions, tag);
            let mut v = r.get(from..).unwrap_or(&[]).to_vec();
            v.resize(len, 0);
            v
        };
        pacman::Roms {
            program: get("maincpu", 0, 0x10000),
            gfx: get("gfx1", 0, 0x2000),
            palette: get("proms", 0, 0x20),
            lookup: get("proms", 0x20, 0x100),
            wave: get("namco", 0, 0x100),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zip::{crc32, write_stored};

    fn archive(stem: &str, files: &[(&str, Vec<u8>)]) -> Archive {
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (*n, d.as_slice())).collect();
        Archive { zip: Zip::parse(write_stored(&refs)).unwrap(), stem: stem.into() }
    }

    /// Bytes with a given CRC32 can't be made up, so these tests use a fake list: the real sets'
    /// CRCs are of commercial ROMs, which are never fetched. The fake chips' CRCs are computed.
    fn chip(seed: u8, size: usize) -> Vec<u8> {
        (0..size).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
    }

    #[test]
    fn every_set_is_well_formed() {
        let mut names = std::collections::HashSet::new();
        for s in SETS {
            assert!(names.insert(s.name), "{} twice", s.name);
            assert!(!s.title.is_empty() && !s.roms.is_empty(), "{}", s.name);
            if let Some(p) = s.romof {
                assert!(find(p).is_some(), "{}'s parent {p} isn't in the list", s.name);
            }
            // Every load fits its region and its chip.
            for r in s.regions {
                for op in r.ops {
                    if let Op::Load { rom, at, from, len } = *op {
                        assert!(at + len <= r.size, "{} {}", s.name, r.tag);
                        assert!(from + len <= s.roms[rom].size as usize, "{} {}", s.name, s.roms[rom].name);
                    }
                }
            }
            // The board's regions are all there.
            let tags: Vec<&str> = s.regions.iter().map(|r| r.tag).collect();
            for t in ["maincpu", "gfx1", "proms", "namco"] {
                assert!(tags.contains(&t), "{} has no {t}", s.name);
            }
        }
        assert!(find("pacman").is_some() && find("MsPacman").is_some());
        assert_eq!(find("pacman").unwrap().short_title(), "Pac-Man");
    }

    #[test]
    fn the_known_boards_are_named_to_the_player() {
        assert_eq!(
            Refusal::Unknown.line(),
            "Our arcade emulator can\u{2019}t play this one yet. Right now it plays Pac-Man, Ms. Pac-Man and games on the same board."
        );
        assert_eq!(
            Refusal::NeedsParent { parent: "mspacman".into() }.line(),
            "This one needs mspacman.zip next to it."
        );
        assert_eq!(
            Refusal::MissingPiece { title: "Pac-Man".into(), set: "pacman".into() }.line(),
            "This copy of Pac-Man is missing a piece. A complete copy of pacman.zip fixes it."
        );
    }

    /// Our own files under the names of the `pacman` set (a homebrew on the board): it plays by name
    /// and size, and the chips that differ are listed.
    #[test]
    fn a_zip_named_after_a_set_with_its_files_plays_as_that_board() {
        let spec = find("pacman").unwrap();
        let files: Vec<(&str, Vec<u8>)> =
            spec.roms.iter().enumerate().map(|(i, r)| (r.name, chip(i as u8, r.size as usize))).collect();
        let found = identify(archive("pacman", &files), &|_| None).ok().unwrap();
        assert_eq!(found.spec.name, "pacman");
        assert_eq!(found.differing.len(), spec.roms.iter().filter(|r| r.needed).count());
        let regions = found.regions().unwrap();
        let roms = found.pacman_roms(&regions);
        assert_eq!(&roms.program[..0x1000], &files[0].1[..]);
        assert_eq!(&roms.gfx[0x1000..0x2000], &files[5].1[..]);
        assert_eq!(roms.lookup, files[7].1);
        // A file missing: it says which zip fixes it. Pac-Man's PROMs come from puckman.zip in a
        // split set, so without them it asks for that.
        let missing: Vec<(&str, Vec<u8>)> = files.iter().filter(|(n, _)| *n != "pacman.6f").cloned().collect();
        assert_eq!(
            identify(archive("pacman", &missing), &|_| None).err(),
            Some(Refusal::MissingPiece { title: "Pac-Man".into(), set: "pacman".into() })
        );
        let split: Vec<(&str, Vec<u8>)> = files.iter().filter(|(n, _)| n.starts_with("pacman.")).cloned().collect();
        assert_eq!(
            identify(archive("pacman", &split), &|_| None).err(),
            Some(Refusal::NeedsParent { parent: "puckman".into() })
        );
        // With puckman.zip next to it, the PROMs come from there under puckman's names.
        let puck = find("puckman").unwrap();
        let parent: Vec<(&str, Vec<u8>)> = puck
            .roms
            .iter()
            .map(|r| {
                let mine = spec.roms.iter().position(|m| m.merge == Some(r.name));
                (r.name, mine.map_or_else(|| chip(99, r.size as usize), |i| files[i].1.clone()))
            })
            .collect();
        let found = identify(archive("pacman", &split), &|n| (n == "puckman").then(|| archive("puckman", &parent)))
            .ok()
            .unwrap();
        let regions = found.regions().unwrap();
        assert_eq!(found.pacman_roms(&regions).palette, files[6].1);
    }

    #[test]
    fn junk_and_unknown_games_are_refused() {
        let found = identify(archive("sf2", &[("sf2_30a.bin", chip(1, 0x20000))]), &|_| None);
        assert_eq!(found.err(), Some(Refusal::Unknown));
        let found = identify(archive("whatever", &[]), &|_| None);
        assert_eq!(found.err(), Some(Refusal::Unknown));
    }

    /// The CRC rules, on the real list: the real chips' CRCs can't be met by bytes we make, so this
    /// checks the bookkeeping (a zip whose directory claims the real CRCs) without unpacking.
    #[test]
    fn chips_are_found_by_crc_whatever_their_names() {
        // A directory claiming Ms. Pac-Man's chips under other names, in a renamed zip.
        let spec = find("mspacman").unwrap();
        let mut zip = write_stored(&spec.roms.iter().map(|r| (r.name, &[][..])).collect::<Vec<_>>());
        // Patch each directory entry's CRC and sizes to the real chip's.
        let mut at = 0;
        for r in spec.roms {
            let pos = (at..zip.len() - 4).find(|&i| zip[i..i + 4] == 0x0201_4B50u32.to_le_bytes()).unwrap();
            zip[pos + 16..pos + 20].copy_from_slice(&r.crc.to_le_bytes());
            zip[pos + 24..pos + 28].copy_from_slice(&r.size.to_le_bytes());
            at = pos + 4;
        }
        let z = Zip::parse(zip).unwrap();
        let found = identify(Archive { zip: z, stem: "my ms pac".into() }, &|_| None).ok().unwrap();
        assert_eq!(found.spec.name, "mspacman", "a renamed zip is still Ms. Pac-Man");
        assert!(found.differing.is_empty());
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
