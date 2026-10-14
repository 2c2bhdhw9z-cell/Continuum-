//! Emulator updates, as players meet them: which old save states still load on a newer core,
//! and the one-line heads-up the first time a game opens on an emulator that changed.
//!
//! Both come from the owner's rules (`.kiro/steering/owner-rules.md`):
//!
//! - **"Emulator updates"**: we take each newer core only after confirming in-game saves and old
//!   save states still load. Where the check proved a core's save-state format unchanged, the
//!   pair of version strings is recorded in [`CORE_UPDATES`], and [`state_version_loads`] lets a
//!   state made by the old build load on the new one. Every other version difference stays
//!   refused, as before.
//! - **"Players get a heads-up"**: once per emulator version, never on a first install, one short
//!   casual line ([`take_heads_up`]). The heads-up is keyed on the pin (the commit for a core
//!   built from source, the zip sha256 for a downloaded one), never on the core's version string:
//!   Beetle Saturn's string did not change across its update.
//!
//! [`CORE_BUILDS`] is what this app version ships. A test fails when it disagrees with
//! `scripts/build-core.sh ios-pins` or `scripts/fetch-buildbot-cores.sh pins`, so moving a pin
//! without updating this file cannot pass `cargo test`.
//!
//! Rewind and online play don't use this on purpose: rewind only replays the running session's
//! own states, and online play needs the same build on both phones.
//!
//! File: `core-builds.txt` in the feedback folder, beside `session.txt` and `activity.txt`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

/// One core this app version ships, at its pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreBuild {
    /// The core id the app uses (a JIT build shares its base core's id).
    pub core_id: &'static str,
    /// The pin: a commit for a core built from source, the zip sha256 for a downloaded one.
    pub build: &'static str,
    /// What a player calls the console, for the heads-up.
    pub console: &'static str,
    /// The emulator's name, for the heads-up.
    pub emulator: &'static str,
}

const fn core(core_id: &'static str, build: &'static str, console: &'static str, emulator: &'static str) -> CoreBuild {
    CoreBuild { core_id, build, console, emulator }
}

/// Every pinned core in this app version (cBoy is ours and has no pin).
pub const CORE_BUILDS: &[CoreBuild] = &[
    // Built from source (scripts/build-core.sh ios-pins).
    core("fceumm", "ac0d00a7fc01166f84c7bac01d1084ebfd2b865e", "NES", "FCEUmm"),
    core("mesen2", "4c4e069f16a8ad6ed79e046046c56d51558c7daf", "NES", "Mesen"),
    core("mgba", "7a12d6d4b9acb14c0ae62c9166b6a2f3d08007f6", "Game Boy", "mGBA"),
    core("bsnes", "05439f96121d2b9d7ad7a5fc1f29d7eebdcc8c43", "SNES", "bsnes"),
    core("gearsystem", "4a801d223ce687e68165d750a0fdda4bd8bc0a7e", "Master System and Game Gear", "Gearsystem"),
    core("blastem", "ab743653ed110eeb7b1fe64d00cc5f9279c2bfec", "Mega Drive", "BlastEm"),
    core("pcsx_rearmed", "c8816799b50388e61cfe237fe2cdbb7d8175f20a", "PlayStation", "PCSX ReARMed"),
    core("mednafen_psx_hw", "5ec9909f2654fb2041315a13fac0b704c5065c0e", "PlayStation", "Beetle PSX"),
    core("melonds", "66b5d2634cd0a79030562811e6e05f5532f800ba", "DS", "melonDS"),
    core("mednafen_pce_fast", "3f946f277aef3aa99a95551618bbcd1dd2bda0d9", "TurboGrafx-16", "Beetle PCE Fast"),
    core("stella2023", "ba52c43b9eda950eb0c0eec69cda9b17dee8c39b", "Atari 2600", "Stella"),
    core("parallel_n64", "a8280501a71c1bcd4c85c5b8bfe75cfa62cd6127", "N64", "ParaLLEl N64"),
    core("azahar", "bfa768727c60ad5bcb9397ad1471f8586605adf8", "3DS", "Azahar"),
    core("ppsspp", "dd6bd1a53d38773c1d07985fcb7d2f56fcaa872c", "PSP", "PPSSPP"),
    core("flycast", "f03faffd2f89cb2c7c73ff08dce51a774a61ee27", "Dreamcast", "Flycast"),
    core("continuum_symbian", "661031f9d8ccd37612797e32a52b2d4c6aadfcdc", "Symbian", "EKA2L1"),
    // Downloaded (scripts/fetch-buildbot-cores.sh pins, the zip sha256).
    core("mednafen_wswan", "9aa05d0207e283731e6d0bde7aa6830de4269b0465b53f47ceb2e99d0ff6e5a5", "WonderSwan", "Beetle WonderSwan"),
    core("mednafen_ngp", "1ed8617c9e45a572998d093d5b389f3ee16eeb9e019c6dbe2f9b9beaa59ff7ae", "Neo Geo Pocket", "Beetle NeoPop"),
    core("mednafen_pce", "6e078cf6fe615bf171b8c2401681457f233e26793a5eb42d736159d55483815b", "PC Engine CD", "Beetle PCE"),
    core("mednafen_supergrafx", "3e51d2d9131c34393e563cbfbb6b438af8f5eeb63c00b10899a64b27038c8408", "SuperGrafx", "Beetle SuperGrafx"),
    core("puae", "6f8dc0190d92af9398a29c4647147d3d300ba42620763d6ba53178f96e009889", "Amiga", "PUAE"),
    core("vice_x64sc", "04c07e0f795dfe3daafbbe42a5b81a37949cb2cdf83f6b641ee456bb49473fe7", "Commodore 64", "VICE"),
    core("dosbox_pure", "d12b0ad50a93713462b4060375b28258be940ab2692d66a33be4f72850761ad6", "DOS", "DOSBox Pure"),
    core("prboom", "65b4f8ab9f21011a19b023bdde585946d908120c1a98c81480fec1e5b6997be7", "DOOM", "PrBoom"),
    core("virtualjaguar", "06861e89c131c88a277954fba521d0e71afa59f1295429bacbb33abb9790ba44", "Jaguar", "Virtual Jaguar"),
    core("handy", "402ba7a02edf5ba345464de4c0e332faefe86c4d4cb73ed44f6a11962b50846b", "Lynx", "Handy"),
    core("prosystem", "c175f25ee35c79deaf5ccf8b2995dcb6c3db2890d98213a15f1de1c9b2500a9b", "Atari 7800", "ProSystem"),
    core("a5200", "9c134a3b51496fbc7b43b2572e57e48ee884f92e0709c17e1c57d46ec3f605dd", "Atari 5200", "a5200"),
    core("pokemini", "3964e0b8effcbc036e7af63adb31788b83759b5a4d78d5b3d8ce9ffc7a70ac02", "Pok\u{e9}mon Mini", "PokeMini"),
    core("mednafen_vb", "1bd318f06c0d1f794d6712a37c27f58479a37297838ea1e6d150d73283941ce4", "Virtual Boy", "Beetle VB"),
    core("yabause", "5a3e5ce7691781b0107bfc4364acab242a64e931a74b95769bde4d73b85f3c8e", "Saturn", "Yabause"),
    core("mednafen_saturn", "cb67ba4be4a5d6662d67daf89e34f282274764054dacb16c24f02de2c950a582", "Saturn", "Beetle Saturn"),
];

/// The builds in build 181 (b88308f, identical at 4574969), the last install before this file
/// existed. An install that has session files but no `core-builds.txt` is compared against it.
/// Frozen forever: never edit it when a pin moves.
pub const BUILD_181: &[(&str, &str)] = &[
    ("fceumm", "7a542dab1e87679921962a9f056186eca425c0c2"),
    ("mesen2", "4c4e069f16a8ad6ed79e046046c56d51558c7daf"),
    ("mgba", "7a12d6d4b9acb14c0ae62c9166b6a2f3d08007f6"),
    ("bsnes", "05439f96121d2b9d7ad7a5fc1f29d7eebdcc8c43"),
    ("gearsystem", "02b9d9e792559f3e871472548ca1fd72d9cc3e2a"),
    ("blastem", "ab743653ed110eeb7b1fe64d00cc5f9279c2bfec"),
    ("pcsx_rearmed", "c8816799b50388e61cfe237fe2cdbb7d8175f20a"),
    ("mednafen_psx_hw", "5ec9909f2654fb2041315a13fac0b704c5065c0e"),
    ("melonds", "66b5d2634cd0a79030562811e6e05f5532f800ba"),
    ("mednafen_pce_fast", "3f946f277aef3aa99a95551618bbcd1dd2bda0d9"),
    ("stella2023", "ba52c43b9eda950eb0c0eec69cda9b17dee8c39b"),
    ("parallel_n64", "0bd516ee793bb87b57e9eef19993b37ac0098d74"),
    ("azahar", "065c9222ae7e8ce5535aa2672c8565e82c0a6d54"),
    ("ppsspp", "53fae900997fd12ac0ef8f8d82d6f25390acbc29"),
    ("flycast", "59ed35a7ea7c1940d4c8ac221a662d0e6d6dc9ea"),
    ("continuum_symbian", "661031f9d8ccd37612797e32a52b2d4c6aadfcdc"),
    ("a5200", "9c134a3b51496fbc7b43b2572e57e48ee884f92e0709c17e1c57d46ec3f605dd"),
    ("dosbox_pure", "01200d7c974308a79f29de978685f276fadd510980f49324d6dc5d503854538a"),
    ("handy", "402ba7a02edf5ba345464de4c0e332faefe86c4d4cb73ed44f6a11962b50846b"),
    ("mednafen_ngp", "1ed8617c9e45a572998d093d5b389f3ee16eeb9e019c6dbe2f9b9beaa59ff7ae"),
    ("mednafen_pce", "6e078cf6fe615bf171b8c2401681457f233e26793a5eb42d736159d55483815b"),
    ("mednafen_saturn", "cbb2f723ccdd99230e28c493d35b0d728112f79d48f07081ad6852dc9340e16c"),
    ("mednafen_supergrafx", "e4fd854cf959fc7d9d6775a875fbf0736627a0bead2a4653afeb76766bf45881"),
    ("mednafen_vb", "1bd318f06c0d1f794d6712a37c27f58479a37297838ea1e6d150d73283941ce4"),
    ("mednafen_wswan", "9aa05d0207e283731e6d0bde7aa6830de4269b0465b53f47ceb2e99d0ff6e5a5"),
    ("pokemini", "3964e0b8effcbc036e7af63adb31788b83759b5a4d78d5b3d8ce9ffc7a70ac02"),
    ("prboom", "65b4f8ab9f21011a19b023bdde585946d908120c1a98c81480fec1e5b6997be7"),
    ("prosystem", "c175f25ee35c79deaf5ccf8b2995dcb6c3db2890d98213a15f1de1c9b2500a9b"),
    ("puae", "a90d7f7382da10eb7cb5def98676bb21d21ccad8780be7a0fb72ff033e5cad66"),
    ("vice_x64sc", "04c07e0f795dfe3daafbbe42a5b81a37949cb2cdf83f6b641ee456bb49473fe7"),
    ("virtualjaguar", "06861e89c131c88a277954fba521d0e71afa59f1295429bacbb33abb9790ba44"),
    ("yabause", "5a3e5ce7691781b0107bfc4364acab242a64e931a74b95769bde4d73b85f3c8e"),
];

// ------------------------------------------------------------------------ old save states

/// How a core's version string (`retro_system_info.library_version`, recorded with every state)
/// is matched to one side of an update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionMatch {
    /// A core built from source: git writes the commit into the string at build time
    /// (`(SVN) 7a542da`, `1.0 0bd516e`, `v3.5.0-1-g02b9d9e-dirty`, plain `53fae90`), and how many
    /// characters it uses isn't knowable off the build machine. Matches when the string holds a
    /// 7-40 character hex token (one leading `g` stripped) that starts the full pin.
    Commit(&'static str),
    /// A downloaded core: the exact string read from the shipped dylib.
    Exact(&'static str),
}

impl VersionMatch {
    pub fn matches(&self, version: &str) -> bool {
        match self {
            VersionMatch::Exact(want) => version == *want,
            VersionMatch::Commit(pin) => {
                let pin = pin.to_ascii_lowercase();
                version.split(|c: char| !c.is_ascii_alphanumeric()).any(|token| {
                    let token = match token.strip_prefix('g') {
                        Some(rest) if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_hexdigit()) => rest,
                        _ => token,
                    };
                    (7..=40).contains(&token.len())
                        && token.chars().all(|c| c.is_ascii_hexdigit())
                        && pin.starts_with(&token.to_ascii_lowercase())
                })
            }
        }
    }
}

/// Whether states made by the old build load on the new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OldStates {
    /// The update check proved the state format unchanged: a state whose version matches
    /// `from_version` loads on a core whose version matches `to_version`.
    Load { from_version: VersionMatch, to_version: VersionMatch },
    /// Refused, as any other version difference is.
    Refused,
}

/// One pin move this app version made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreUpdate {
    pub core_id: &'static str,
    pub from_build: &'static str,
    pub to_build: &'static str,
    pub old_states: OldStates,
}

const fn commits(core_id: &'static str, from: &'static str, to: &'static str) -> CoreUpdate {
    CoreUpdate {
        core_id,
        from_build: from,
        to_build: to,
        old_states: OldStates::Load { from_version: VersionMatch::Commit(from), to_version: VersionMatch::Commit(to) },
    }
}

const fn exact(
    core_id: &'static str,
    from: &'static str,
    to: &'static str,
    from_version: &'static str,
    to_version: &'static str,
) -> CoreUpdate {
    CoreUpdate {
        core_id,
        from_build: from,
        to_build: to,
        old_states: OldStates::Load { from_version: VersionMatch::Exact(from_version), to_version: VersionMatch::Exact(to_version) },
    }
}

/// Every move to [`CORE_BUILDS`] from a pin an earlier install can have run, each saying whether
/// old save states load. Only direct pairs count ([`state_version_loads`]), so when a pin moves,
/// that core's entries are checked again and pointed at the new pin, and one is added from the pin
/// it moved off. Build 181's pins ([`BUILD_181`]) always stay a "from": an install without
/// `core-builds.txt` is compared against them, and a heads-up still waiting since then keeps them.
/// 12 October 2026: the first moves from build 181. 14 October 2026
/// (`docs/reports/core-updates-2026-10-14.md`): the cores that moved again, from the pins builds
/// 182 to 185 shipped.
pub const CORE_UPDATES: &[CoreUpdate] = &[
    // Built from source.
    commits("fceumm", "7a542dab1e87679921962a9f056186eca425c0c2", "ac0d00a7fc01166f84c7bac01d1084ebfd2b865e"),
    // Save-state format 111 at all three pins: states saved on build 181 and on builds 182 to 185
    // were loaded on the new pin on a computer, and the game carried on frame for frame.
    commits("gearsystem", "02b9d9e792559f3e871472548ca1fd72d9cc3e2a", "4a801d223ce687e68165d750a0fdda4bd8bc0a7e"),
    commits("gearsystem", "692494c8d3e322faeea204fd54f582f00d77f03a", "4a801d223ce687e68165d750a0fdda4bd8bc0a7e"),
    // Covers parallel_n64_jit too: same core id and version string.
    commits("parallel_n64", "0bd516ee793bb87b57e9eef19993b37ac0098d74", "a8280501a71c1bcd4c85c5b8bfe75cfa62cd6127"),
    // Azahar refuses states from another revision itself.
    CoreUpdate {
        core_id: "azahar",
        from_build: "065c9222ae7e8ce5535aa2672c8565e82c0a6d54",
        to_build: "bfa768727c60ad5bcb9397ad1471f8586605adf8",
        old_states: OldStates::Refused,
    },
    // Versioned sections, no minimum raised.
    commits("ppsspp", "53fae900997fd12ac0ef8f8d82d6f25390acbc29", "dd6bd1a53d38773c1d07985fcb7d2f56fcaa872c"),
    // Covers flycast_jit too.
    commits("flycast", "59ed35a7ea7c1940d4c8ac221a662d0e6d6dc9ea", "f03faffd2f89cb2c7c73ff08dce51a774a61ee27"),
    // Downloaded: the version strings were read from the shipped dylibs with `strings`.
    exact(
        "mednafen_supergrafx",
        "e4fd854cf959fc7d9d6775a875fbf0736627a0bead2a4653afeb76766bf45881",
        "3e51d2d9131c34393e563cbfbb6b438af8f5eeb63c00b10899a64b27038c8408",
        "v1.29.0 3c6fcd3",
        "v1.29.0 9c11dcd",
    ),
    exact(
        "puae",
        "a90d7f7382da10eb7cb5def98676bb21d21ccad8780be7a0fb72ff033e5cad66",
        "6f8dc0190d92af9398a29c4647147d3d300ba42620763d6ba53178f96e009889",
        "5.3.1 6536174",
        "5.3.1 7387937",
    ),
    exact(
        "dosbox_pure",
        "01200d7c974308a79f29de978685f276fadd510980f49324d6dc5d503854538a",
        "d12b0ad50a93713462b4060375b28258be940ab2692d66a33be4f72850761ad6",
        "1.0-preview6",
        "1.0-preview7",
    ),
    exact(
        "mednafen_saturn",
        "cbb2f723ccdd99230e28c493d35b0d728112f79d48f07081ad6852dc9340e16c",
        "cb67ba4be4a5d6662d67daf89e34f282274764054dacb16c24f02de2c950a582",
        "v1.32.1",
        "v1.32.1",
    ),
];

/// Whether a save state recorded with core version `saved` may load on core `core_id` running
/// version `running`. True when either side is missing or empty (the gate can't answer, so it
/// falls through to its next check, as before), when they are equal, or when [`CORE_UPDATES`]
/// records that exact old→new pair as proven. Direct pairs only: no chaining, never new→old.
pub fn state_version_loads(core_id: &str, saved: Option<&str>, running: Option<&str>) -> bool {
    let (Some(saved), Some(running)) = (saved, running) else { return true };
    if saved.is_empty() || running.is_empty() || saved == running {
        return true;
    }
    CORE_UPDATES.iter().any(|update| {
        update.core_id == core_id
            && matches!(update.old_states, OldStates::Load { from_version, to_version }
                if from_version.matches(saved) && to_version.matches(running))
    })
}

// ------------------------------------------------------------------------ the heads-up

const STATE_FILE: &str = "core-builds.txt";
const STATE_TEMP: &str = "core-builds.txt.tmp";
/// Written by every build since ~127, so their presence means an earlier install.
const EARLIER_INSTALL_MARKERS: &[&str] = &["session.txt", "activity.txt"];

static STATE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Default, PartialEq, Eq)]
struct State {
    /// Core id → the build this install last ran.
    seen: BTreeMap<String, String>,
    /// Core id → (from, to) of an update whose heads-up hasn't been shown yet.
    due: BTreeMap<String, (String, String)>,
}

impl State {
    fn parse(text: &str) -> State {
        let mut state = State::default();
        for line in text.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            match fields.as_slice() {
                ["seen", core, build] => {
                    state.seen.insert(core.to_string(), build.to_string());
                }
                ["due", core, from, to] => {
                    state.due.insert(core.to_string(), (from.to_string(), to.to_string()));
                }
                _ => {}
            }
        }
        state
    }

    fn render(&self) -> String {
        let mut text = String::from("# Continuum: the emulator builds this install has run, and heads-ups not shown yet.\n");
        for (core, build) in &self.seen {
            text.push_str(&format!("seen {core} {build}\n"));
        }
        for (core, (from, to)) in &self.due {
            text.push_str(&format!("due {core} {from} {to}\n"));
        }
        text
    }
}

fn read_state(dir: &Path) -> Option<State> {
    fs::read(dir.join(STATE_FILE)).ok().map(|bytes| State::parse(&String::from_utf8_lossy(&bytes)))
}

fn write_state(dir: &Path, state: &State) {
    let _ = fs::create_dir_all(dir);
    let temp = dir.join(STATE_TEMP);
    if fs::write(&temp, state.render()).is_ok() {
        let _ = fs::rename(&temp, dir.join(STATE_FILE));
    }
}

fn open_with(dir: &Path, current: &[(&str, &str)], baseline: &[(&str, &str)]) -> String {
    let _guard = STATE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut state = read_state(dir).unwrap_or_else(|| {
        let earlier = EARLIER_INSTALL_MARKERS.iter().any(|name| dir.join(name).exists());
        // An earlier install without this file ran build 181's cores; a first install has seen
        // exactly what it ships, so it gets no heads-up.
        let seen = if earlier { baseline } else { current };
        State {
            seen: seen.iter().map(|(c, b)| (c.to_string(), b.to_string())).collect(),
            due: BTreeMap::new(),
        }
    });

    let mut moved = Vec::new();
    for (core, build) in current {
        if let Some(previous) = state.seen.get(*core).filter(|seen| seen.as_str() != *build) {
            // Two moves before the line is shown keep the first "from", so the save-state
            // wording stays right.
            let from = state.due.get(*core).map_or_else(|| previous.clone(), |(from, _)| from.clone());
            state.due.insert(core.to_string(), (from, build.to_string()));
            moved.push(core.to_string());
        }
    }
    state.seen = current.iter().map(|(c, b)| (c.to_string(), b.to_string())).collect();
    state.due.retain(|core, (from, to)| current.iter().any(|(c, _)| c == core) && from != to);
    write_state(dir, &state);

    moved.retain(|core| state.due.contains_key(core));
    moved.sort();
    if moved.is_empty() {
        String::new()
    } else {
        format!("emulator updates since the last install: {}", moved.join(", "))
    }
}

/// Once per app start, before the session files in `dir` are rewritten: records the builds this
/// install runs and queues a heads-up for every core whose pin moved since the last install.
/// Returns "" or one line for the activity log naming the moved cores.
pub fn open(dir: &Path) -> String {
    let current: Vec<(&str, &str)> = CORE_BUILDS.iter().map(|c| (c.core_id, c.build)).collect();
    open_with(dir, &current, BUILD_181)
}

/// The heads-up for `core_id`, once: the line is removed as it is returned.
pub fn take_heads_up(dir: &Path, core_id: &str) -> Option<String> {
    let _guard = STATE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut state = read_state(dir)?;
    let (from, to) = state.due.remove(core_id)?;
    write_state(dir, &state);
    heads_up_line(core_id, &from, &to)
}

fn heads_up_line(core_id: &str, from: &str, to: &str) -> Option<String> {
    let label = CORE_BUILDS.iter().find(|c| c.core_id == core_id)?;
    let loads = CORE_UPDATES.iter().any(|u| {
        u.core_id == core_id && u.from_build == from && u.to_build == to && matches!(u.old_states, OldStates::Load { .. })
    });
    Some(wording(label.console, label.emulator, loads))
}

/// The owner's example sentence, or, when old save states can't load, the version that says so
/// (it drops "from the people who make it" to stay short).
fn wording(console: &str, emulator: &str, old_states_load: bool) -> String {
    if old_states_load {
        format!(
            "Heads up: the {console} emulator ({emulator}) just got an update from the people who make it. \
             If anything seems off, send us feedback."
        )
    } else {
        format!(
            "Heads up: the {console} emulator ({emulator}) just got an update. Older save states can\u{2019}t \
             load on it, but your in-game saves are fine. If anything seems off, send us feedback."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let n = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("continuum-core-updates-{name}-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn script_lines(script: &str, arg: &str) -> Vec<Vec<String>> {
        let out = Command::new("bash").arg(repo().join(script)).arg(arg).output().unwrap();
        assert!(out.status.success(), "{script} {arg} failed: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.split('\t').map(String::from).collect())
            .collect()
    }

    fn current() -> Vec<(&'static str, &'static str)> {
        CORE_BUILDS.iter().map(|c| (c.core_id, c.build)).collect()
    }

    fn dues(dir: &Path) -> BTreeMap<String, (String, String)> {
        read_state(dir).unwrap().due
    }

    const MOVED: [&str; 10] = [
        "azahar",
        "dosbox_pure",
        "fceumm",
        "flycast",
        "gearsystem",
        "mednafen_saturn",
        "mednafen_supergrafx",
        "parallel_n64",
        "ppsspp",
        "puae",
    ];

    // (a)
    #[test]
    fn core_builds_match_the_pinned_scripts() {
        let mut pins: BTreeMap<String, String> = BTreeMap::new();
        for fields in script_lines("scripts/build-core.sh", "ios-pins") {
            let (core, pin) = (fields[0].clone(), fields[2].clone());
            let base = core.strip_suffix("_jit").unwrap_or(&core).to_string();
            if let Some(existing) = pins.get(&base) {
                assert_eq!(existing, &pin, "{core} is not at its base core's pin");
            }
            pins.insert(base, pin);
        }
        for fields in script_lines("scripts/fetch-buildbot-cores.sh", "pins") {
            assert!(pins.insert(fields[0].clone(), fields[1].clone()).is_none(), "{} listed twice", fields[0]);
        }
        let ours: BTreeMap<String, String> =
            CORE_BUILDS.iter().map(|c| (c.core_id.to_string(), c.build.to_string())).collect();
        assert_eq!(ours.len(), CORE_BUILDS.len(), "a core is listed twice in CORE_BUILDS");
        assert_eq!(
            ours, pins,
            "CORE_BUILDS disagrees with the pins: update core_updates.rs (CORE_BUILDS and CORE_UPDATES)"
        );
        let baseline: BTreeMap<&str, &str> = BUILD_181.iter().copied().collect();
        assert_eq!(baseline.len(), 32);
    }

    // (b)
    #[test]
    fn every_moved_pin_has_an_update_entry() {
        let baseline: BTreeMap<&str, &str> = BUILD_181.iter().copied().collect();
        for c in CORE_BUILDS {
            if baseline.get(c.core_id) != Some(&c.build) {
                assert!(
                    CORE_UPDATES.iter().any(|u| u.core_id == c.core_id && u.to_build == c.build),
                    "{} moved without a CORE_UPDATES entry",
                    c.core_id
                );
            }
        }
        for u in CORE_UPDATES {
            assert!(CORE_BUILDS.iter().any(|c| c.core_id == u.core_id), "{} is not shipped", u.core_id);
            assert_ne!(u.from_build, u.to_build, "{}", u.core_id);
            if let OldStates::Load { from_version: VersionMatch::Commit(from), to_version } = u.old_states {
                assert_eq!(from, u.from_build, "{}", u.core_id);
                assert_eq!(to_version, VersionMatch::Commit(u.to_build), "{}", u.core_id);
            }
        }
        assert_eq!(CORE_UPDATES.len(), 11);
    }

    fn loads(core: &str, saved: &str, running: &str) -> bool {
        state_version_loads(core, Some(saved), Some(running))
    }

    // (c)
    #[test]
    fn proven_pairs_load() {
        assert!(loads("fceumm", "(SVN) 7a542da", "(SVN) ac0d00a"));
        assert!(loads("fceumm", "(SVN) 7a542dab1e", "(SVN) ac0d00a7fc"));
        assert!(loads("gearsystem", "02b9d9e-dirty", "4a801d2-dirty"));
        assert!(loads("gearsystem", "v3.5.0-1-g02b9d9e-dirty", "4a801d2"));
        assert!(loads("gearsystem", "692494c-dirty", "4a801d2-dirty"));
        // What host builds of the three pins reported (git describe with tags), 14 October 2026.
        assert!(loads("gearsystem", "3.9.22-4-g02b9d9e-dirty", "3.9.22-10-g4a801d2-dirty"));
        assert!(loads("gearsystem", "3.9.22-5-g692494c-dirty", "3.9.22-10-g4a801d2-dirty"));
        assert!(loads("parallel_n64", "1.0 0bd516e", "1.0 a828050"));
        assert!(loads("ppsspp", "53fae90", "v1.19.3-1501-gdd6bd1a5"));
        assert!(loads("flycast", "59ed35a7", "f03faffd"));
        assert!(loads("mednafen_supergrafx", "v1.29.0 3c6fcd3", "v1.29.0 9c11dcd"));
        assert!(loads("puae", "5.3.1 6536174", "5.3.1 7387937"));
        assert!(loads("dosbox_pure", "1.0-preview6", "1.0-preview7"));
        assert!(loads("mednafen_saturn", "v1.32.1", "v1.32.1"));
    }

    // (d)
    #[test]
    fn everything_else_stays_refused() {
        assert!(!loads("azahar", "065c922", "bfa7687"));
        assert!(!loads("azahar", "a", "b"));
        assert!(!loads("fceumm", "(SVN) ac0d00a", "(SVN) 7a542da"));
        assert!(!loads("gearsystem", "4a801d2-dirty", "692494c-dirty"));
        assert!(!loads("mesen2", "(SVN) 7a542da", "(SVN) ac0d00a"));
        assert!(!loads("fceumm", "(SVN) 1234567", "(SVN) ac0d00a"));
        assert!(!loads("fceumm", "(SVN)", "(SVN) ac0d00a"));
        assert!(!loads("fceumm", "(SVN) 7a542d", "(SVN) ac0d00a"));
        assert!(!loads("fceumm", "v0.0.0-0-g000000000", "(SVN) ac0d00a"));
        assert!(!loads("puae", "5.3.1 7387937", "5.3.1 6536174"));
        assert!(!loads("dosbox_pure", "1.0-preview5", "1.0-preview7"));
    }

    // (e)
    #[test]
    fn missing_or_equal_versions_fall_through() {
        assert!(state_version_loads("azahar", None, Some("x")));
        assert!(state_version_loads("azahar", Some("x"), None));
        assert!(state_version_loads("azahar", Some(""), Some("x")));
        assert!(state_version_loads("azahar", Some("x"), Some("")));
        assert!(state_version_loads("azahar", Some("x"), Some("x")));
    }

    // (f)
    #[test]
    fn a_first_install_gets_no_heads_up() {
        let dir = TempDir::new("first");
        assert_eq!(open(&dir.0), "");
        for c in CORE_BUILDS {
            assert_eq!(take_heads_up(&dir.0, c.core_id), None, "{}", c.core_id);
        }
        assert!(fs::read_to_string(dir.0.join(STATE_FILE)).unwrap().starts_with('#'));
    }

    #[test]
    fn an_update_from_build_181_gets_one_line_per_moved_core() {
        let dir = TempDir::new("update");
        fs::write(dir.0.join("session.txt"), "x").unwrap();
        assert_eq!(open(&dir.0), format!("emulator updates since the last install: {}", MOVED.join(", ")));
        let due: Vec<String> = dues(&dir.0).into_keys().collect();
        assert_eq!(due, MOVED);

        assert_eq!(
            take_heads_up(&dir.0, "ppsspp").unwrap(),
            "Heads up: the PSP emulator (PPSSPP) just got an update from the people who make it. \
             If anything seems off, send us feedback."
        );
        assert_eq!(take_heads_up(&dir.0, "ppsspp"), None);

        let azahar = take_heads_up(&dir.0, "azahar").unwrap();
        assert!(azahar.contains("Older save states can\u{2019}t load"), "{azahar}");
        assert!(azahar.contains("in-game saves are fine"), "{azahar}");

        // Reopening adds nothing, and an untaken line survives it.
        let before = dues(&dir.0);
        assert_eq!(open(&dir.0), "");
        assert_eq!(dues(&dir.0), before);
        assert!(before.contains_key("fceumm"));
        assert_eq!(take_heads_up(&dir.0, "nonexistent"), None);
    }

    #[test]
    fn an_update_from_builds_182_to_185_says_old_states_load() {
        let dir = TempDir::new("from-185");
        // core-builds.txt as builds 182 to 185 wrote it, for an install where only Gearsystem moved since.
        let mut text = String::from("# Continuum\n");
        for c in CORE_BUILDS {
            let build = if c.core_id == "gearsystem" { "692494c8d3e322faeea204fd54f582f00d77f03a" } else { c.build };
            text.push_str(&format!("seen {} {build}\n", c.core_id));
        }
        fs::write(dir.0.join(STATE_FILE), text).unwrap();
        assert_eq!(open(&dir.0), "emulator updates since the last install: gearsystem");
        assert_eq!(
            take_heads_up(&dir.0, "gearsystem").unwrap(),
            "Heads up: the Master System and Game Gear emulator (Gearsystem) just got an update from the people who make it. \
             If anything seems off, send us feedback."
        );
    }

    #[test]
    fn a_second_move_before_the_line_shows_keeps_the_first_from() {
        let dir = TempDir::new("twice");
        fs::write(dir.0.join("activity.txt"), "x").unwrap();
        open(&dir.0);
        let later: Vec<(&str, &str)> = current()
            .into_iter()
            .map(|(c, b)| if c == "fceumm" { (c, "1111111111111111111111111111111111111111") } else { (c, b) })
            .collect();
        assert_eq!(open_with(&dir.0, &later, BUILD_181), "emulator updates since the last install: fceumm");
        assert_eq!(
            dues(&dir.0)["fceumm"],
            ("7a542dab1e87679921962a9f056186eca425c0c2".into(), "1111111111111111111111111111111111111111".into())
        );
        let line = take_heads_up(&dir.0, "fceumm").unwrap();
        assert!(line.contains("Older save states can\u{2019}t load"), "{line}");
    }

    #[test]
    fn a_broken_file_does_not_panic() {
        let dir = TempDir::new("garbage");
        fs::write(dir.0.join(STATE_FILE), [0xff, 0xfe, b'\n', b's', b'e', b'e', b'n', b'\n', b'd', b'u', b'e', b' ', b'x']).unwrap();
        assert_eq!(open(&dir.0), "");
        assert_eq!(take_heads_up(&dir.0, "fceumm"), None);
        fs::write(dir.0.join(STATE_FILE), "seen fceumm\ndue fceumm a\ndue\n\u{0}\u{0}").unwrap();
        assert_eq!(open(&dir.0), "");
    }

    fn wrapped_lines(text: &str, width: usize) -> usize {
        let mut lines = 1;
        let mut used = 0;
        for word in text.split(' ') {
            let len = word.chars().count();
            if used > 0 && used + 1 + len > width {
                lines += 1;
                used = len;
            } else {
                used += if used > 0 { 1 + len } else { len };
            }
        }
        lines
    }

    // (g)
    #[test]
    fn every_heads_up_is_short_and_plain() {
        for c in CORE_BUILDS {
            // No exact CORE_UPDATES pair gives the refused wording.
            assert_eq!(heads_up_line(c.core_id, "a", "b").unwrap(), wording(c.console, c.emulator, false));
            for line in [wording(c.console, c.emulator, true), wording(c.console, c.emulator, false)] {
                // The player's fade line shows 4 lines; upright it fits ~53 characters a line.
                // Checked at 50 for a margin. (The longest, Gearsystem's refused line, is 193.)
                assert!(line.chars().count() <= 200, "{} chars: {line}", line.chars().count());
                assert!(wrapped_lines(&line, 50) <= 4, "more than 4 lines upright: {line}");
                assert!(line.starts_with("Heads up:"), "{line}");
                assert!(!line.contains('\''), "{line}");
                // No commit or checksum leaks into a line: no word made only of 7+ hex digits.
                assert!(
                    !line
                        .split(|ch: char| !ch.is_ascii_alphanumeric())
                        .any(|w| w.len() >= 7 && w.chars().all(|ch| ch.is_ascii_hexdigit())),
                    "{line}"
                );
            }
        }
    }
}
