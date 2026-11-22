//! The player's games: everything Game Boy-shaped in the games folder (where the TV's upload
//! page and Continuum's own transfer put them), plus when each was added and last played, and
//! favourites.

use crate::files::{self, Paths};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum System {
    Gb,
    Gbc,
    Gba,
}

impl System {
    pub const ALL: [System; 3] = [System::Gb, System::Gbc, System::Gba];
    pub fn name(self) -> &'static str {
        match self {
            System::Gb => "Game Boy",
            System::Gbc => "Game Boy Color",
            System::Gba => "Game Boy Advance",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            System::Gb => "GB",
            System::Gbc => "GBC",
            System::Gba => "GBA",
        }
    }
    /// The phone's console glow colours (NewLook.consoleColor).
    pub fn colour(self) -> [f32; 4] {
        match self {
            System::Gb => [0.62, 0.80, 0.35, 1.0],
            System::Gbc => [1.0, 0.70, 0.20, 1.0],
            System::Gba => [0.36, 0.45, 1.0, 1.0],
        }
    }
}

#[derive(Clone, Debug)]
pub struct Game {
    /// The game's own file name (inside an archive, the file in it), as the phone names saves.
    pub id: String,
    pub path: PathBuf,
    pub system: System,
    pub title: String,
    pub tags: String,
    pub added: u64,
    pub played: u64,
    pub favourite: bool,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct Known {
    id: String,
    system: Option<System>,
    size: u64,
    mtime: u64,
    added: u64,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Meta {
    files: HashMap<String, Known>,
    played: HashMap<String, u64>,
    favourites: Vec<String>,
}

pub struct Library {
    pub games: Vec<Game>,
    meta: Meta,
    path: String,
}

/// Which console a game is for, from its name and header.
pub fn detect(name: &str, head: &[u8]) -> Option<System> {
    match files::ext_of(name).as_str() {
        "gba" | "agb" => Some(System::Gba),
        "gbc" | "cgb" => Some(System::Gbc),
        "gb" | "dmg" | "sgb" => {
            // 0x143: 0x80 works on both, 0xC0 is Color only; both are Color games.
            if head.len() > 0x143 && (head[0x143] == 0x80 || head[0x143] == 0xC0) {
                Some(System::Gbc)
            } else {
                Some(System::Gb)
            }
        }
        _ => None,
    }
}

/// "Pokemon - Crystal Version (USA, Europe) (Rev 1).gbc" -> ("Pokemon - Crystal Version", "USA, Europe · Rev 1")
pub fn title_of(id: &str) -> (String, String) {
    let stem = Path::new(id).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| id.to_string());
    let stem = stem.replace('_', " ");
    let cut = stem.find(" (").into_iter().chain(stem.find(" [")).min().unwrap_or(stem.len());
    let title = stem[..cut].trim().to_string();
    let mut tags = Vec::new();
    let rest = &stem[cut..];
    let mut cur = String::new();
    let mut depth = 0;
    for c in rest.chars() {
        match c {
            '(' | '[' => {
                depth += 1;
                if depth == 1 {
                    cur.clear();
                    continue;
                }
            }
            ')' | ']' => {
                depth -= 1;
                if depth == 0 {
                    if !cur.trim().is_empty() {
                        tags.push(cur.trim().to_string());
                    }
                    continue;
                }
            }
            _ => {}
        }
        if depth > 0 {
            cur.push(c);
        }
    }
    (if title.is_empty() { stem.clone() } else { title }, tags.join(" · "))
}

impl Library {
    pub fn load(paths: &Paths) -> Library {
        let path = paths.library();
        let meta: Meta = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let mut l = Library { games: Vec::new(), meta, path };
        l.scan(paths);
        l
    }

    fn save(&self) {
        if let Ok(t) = serde_json::to_string(&self.meta) {
            files::write_atomic(&self.path, t.as_bytes());
        }
    }

    /// Look through the games folder again. Quick: a file already seen is not reopened.
    pub fn scan(&mut self, paths: &Paths) {
        let mut found = Vec::new();
        walk(&paths.games, 0, &mut found);
        let mut games = Vec::new();
        let mut changed = false;
        let mut seen = std::collections::HashSet::new();
        for p in found {
            let key = p.to_string_lossy().into_owned();
            let md = match std::fs::metadata(&p) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let (size, mtime) = (md.len(), files::mtime(&p));
            let known = match self.meta.files.get(&key) {
                Some(k) if k.size == size && k.mtime == mtime => k.clone(),
                _ => {
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let ext = files::ext_of(&name);
                    let (id, head) = if files::ARCHIVE_EXTS.contains(&ext.as_str()) {
                        match files::archive_inner(&p) {
                            Some(x) => x,
                            None => (name.clone(), Vec::new()),
                        }
                    } else {
                        (name.clone(), files::read_head(&p))
                    };
                    let added = self.meta.files.get(&key).map(|k| k.added).filter(|a| *a > 0).unwrap_or_else(files::now_secs);
                    let k = Known { system: detect(&id, &head), id, size, mtime, added };
                    self.meta.files.insert(key.clone(), k.clone());
                    changed = true;
                    k
                }
            };
            seen.insert(key);
            let system = match known.system {
                Some(s) => s,
                None => continue,
            };
            let (title, tags) = title_of(&known.id);
            games.push(Game {
                played: self.meta.played.get(&known.id).copied().unwrap_or(0),
                favourite: self.meta.favourites.contains(&known.id),
                id: known.id,
                path: p,
                system,
                title,
                tags,
                added: known.added,
                size,
            });
        }
        let before = self.meta.files.len();
        self.meta.files.retain(|k, _| seen.contains(k));
        if changed || before != self.meta.files.len() {
            self.save();
        }
        // The same game twice (a .gbc and a .zip of it): keep one.
        games.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()).then(a.id.cmp(&b.id)));
        games.dedup_by(|a, b| a.id == b.id);
        self.games = games;
    }

    pub fn mark_played(&mut self, id: &str) {
        let t = files::now_secs();
        self.meta.played.insert(id.to_string(), t);
        for g in self.games.iter_mut().filter(|g| g.id == id) {
            g.played = t;
        }
        self.save();
    }

    pub fn toggle_favourite(&mut self, id: &str) -> bool {
        let on = if let Some(i) = self.meta.favourites.iter().position(|f| f == id) {
            self.meta.favourites.remove(i);
            false
        } else {
            self.meta.favourites.push(id.to_string());
            true
        };
        for g in self.games.iter_mut().filter(|g| g.id == id) {
            g.favourite = on;
        }
        self.save();
        on
    }

    /// Take the game's file off the TV. Its saves stay.
    pub fn remove(&mut self, id: &str, paths: &Paths) {
        if let Some(g) = self.games.iter().find(|g| g.id == id) {
            let _ = std::fs::remove_file(&g.path);
        }
        self.scan(paths);
    }

    pub fn by_index(&self, i: usize) -> Option<&Game> {
        self.games.get(i)
    }

    pub fn find(&self, id: &str) -> Option<usize> {
        self.games.iter().position(|g| g.id == id)
    }
}

fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if depth > 3 {
        return;
    }
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.ends_with(".part") {
            continue;
        }
        if p.is_dir() {
            walk(&p, depth + 1, out);
        } else {
            let ext = files::ext_of(&name);
            if files::ROM_EXTS.contains(&ext.as_str()) || files::ARCHIVE_EXTS.contains(&ext.as_str()) {
                out.push(p);
            }
        }
    }
}
