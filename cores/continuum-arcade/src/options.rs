//! The cabinet's DIP switches as core options (libretro SET_CORE_OPTIONS_V2, with the plain
//! SET_VARIABLES list for older frontends), keyed `carcade_<board>_<switch>`, with plain labels and
//! the factory settings as defaults. Every board's switches are declared up front, grouped by
//! board; once a game loads, the ones its cabinet doesn't have are hidden.
//!
//! A switch the game reads all the time takes effect at once; most games read lives and the like
//! when a game starts, as on the real cabinet.

use std::ffi::{c_char, c_void, CStr};
use std::sync::Mutex;

use crate::board::Switches;
use crate::sets::{Dips, SetSpec};
use crate::set_list::FAMILIES;

#[repr(C)]
struct OptValue {
    value: *const c_char,
    label: *const c_char,
}

#[repr(C)]
struct OptCategory {
    key: *const c_char,
    desc: *const c_char,
    info: *const c_char,
}

#[repr(C)]
struct OptDefinition {
    key: *const c_char,
    desc: *const c_char,
    desc_categorized: *const c_char,
    info: *const c_char,
    info_categorized: *const c_char,
    category_key: *const c_char,
    values: [OptValue; 128],
    default_value: *const c_char,
}

#[repr(C)]
struct OptionsV2 {
    categories: *const OptCategory,
    definitions: *const OptDefinition,
}

#[repr(C)]
struct Variable {
    key: *const c_char,
    value: *const c_char,
}

#[repr(C)]
struct OptDisplay {
    key: *const c_char,
    visible: bool,
}

#[repr(C)]
struct UpdateDisplayCallback {
    callback: Option<unsafe extern "C" fn() -> bool>,
}

pub type EnvFn = unsafe extern "C" fn(u32, *mut c_void) -> bool;

const ENV_GET_VARIABLE: u32 = 15;
const ENV_SET_VARIABLES: u32 = 16;
const ENV_GET_VARIABLE_UPDATE: u32 = 17;
const ENV_SET_CORE_OPTIONS_DISPLAY: u32 = 55;
const ENV_SET_CORE_OPTIONS_V2: u32 = 67;
const ENV_SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK: u32 = 69;

/// One switch (or a few that work together, like the two coinage switches) and its settings.
pub struct Switch {
    pub key: &'static CStr,
    pub label: &'static CStr,
    pub info: &'static CStr,
    pub category: &'static CStr,
    /// Which of the board's input ports it's in (`Switches::ports`) and which bits.
    pub port: usize,
    pub mask: u8,
    /// (value, label, the bits it sets).
    pub values: &'static [(&'static CStr, &'static CStr, u8)],
    pub default: &'static CStr,
}

/// A family of sets with the same switches, generated from MAME's switch lists (`set_list.rs`):
/// its settings group, keyed by its first set.
pub struct SwitchFamily {
    pub key: &'static CStr,
    pub title: &'static CStr,
    pub switches: &'static [Switch],
}

/// The settings groups: the Pac-Man board's, then one per generated family.
fn categories() -> Vec<(&'static CStr, &'static CStr, &'static CStr)> {
    let mut c = vec![(
        c"pacman",
        c"Pac-Man games",
        c"The switches inside the cabinet of Pac-Man, Ms. Pac-Man and the games on their board.",
    )];
    for f in FAMILIES {
        c.push((f.key, f.title, c"The switches inside the cabinet."));
    }
    c
}

// The Pac-Man board's switches (DSW1 is port 2; the cabinet switch is IN1's top bit).
const PAC_COINS: Switch = Switch {
    key: c"carcade_pacman_coinage",
    label: c"Coins",
    info: c"",
    category: c"pacman",
    port: 2,
    mask: 0x03,
    values: &[
        (c"1c1g", c"1 coin, 1 game", 0x01),
        (c"2c1g", c"2 coins, 1 game", 0x03),
        (c"1c2g", c"1 coin, 2 games", 0x02),
        (c"free", c"Free play", 0x00),
    ],
    default: c"1c1g",
};
const PAC_LIVES: Switch = Switch {
    key: c"carcade_pacman_lives",
    label: c"Lives",
    info: c"",
    category: c"pacman",
    port: 2,
    mask: 0x0C,
    values: &[(c"1", c"1", 0x00), (c"2", c"2", 0x04), (c"3", c"3", 0x08), (c"5", c"5", 0x0C)],
    default: c"3",
};
const PAC_BONUS: Switch = Switch {
    key: c"carcade_pacman_bonus",
    label: c"Extra life",
    info: c"",
    category: c"pacman",
    port: 2,
    mask: 0x30,
    values: &[
        (c"10000", c"At 10,000 points", 0x00),
        (c"15000", c"At 15,000 points", 0x10),
        (c"20000", c"At 20,000 points", 0x20),
        (c"none", c"None", 0x30),
    ],
    default: c"10000",
};
const PAC_DIFFICULTY: Switch = Switch {
    key: c"carcade_pacman_difficulty",
    label: c"Difficulty",
    info: c"",
    category: c"pacman",
    port: 2,
    mask: 0x40,
    values: &[(c"normal", c"Normal", 0x40), (c"hard", c"Hard", 0x00)],
    default: c"normal",
};
const PAC_GHOSTS: Switch = Switch {
    key: c"carcade_pacman_ghosts",
    label: c"Ghost names",
    info: c"Pac-Man's ghosts have a second set of names.",
    category: c"pacman",
    port: 2,
    mask: 0x80,
    values: &[(c"usual", c"Usual", 0x80), (c"alternate", c"Alternate", 0x00)],
    default: c"usual",
};
const PAC_CABINET: Switch = Switch {
    key: c"carcade_pacman_cabinet",
    label: c"Cabinet",
    info: c"A cocktail table turns the picture round on player 2's turn.",
    category: c"pacman",
    port: 1,
    mask: 0x80,
    values: &[(c"upright", c"Upright", 0x80), (c"cocktail", c"Cocktail table", 0x00)],
    default: c"upright",
};

/// All of the Pac-Man board's switches.
const PAC_ALL: &[&Switch] = &[&PAC_COINS, &PAC_LIVES, &PAC_BONUS, &PAC_DIFFICULTY, &PAC_GHOSTS, &PAC_CABINET];

/// Every switch of every board, in the order the settings list them.
pub fn all_switches() -> Vec<&'static Switch> {
    let mut v: Vec<&'static Switch> = PAC_ALL.to_vec();
    for f in FAMILIES {
        v.extend(f.switches.iter());
    }
    v
}

/// A cabinet's switches: the ones it offers, and the bits of the ones it doesn't, which stay at
/// the factory setting (a game whose switch means something else there, like Come Come's coinage).
/// `base` is the ports with nothing pressed and every offered switch at 0.
pub struct Family {
    pub switches: Vec<&'static Switch>,
    pub base: [u8; 4],
}

/// The Pac-Man board's ports at rest: IN0 and IN1 read 1 for every control not pressed, and for
/// the rack test and service switches, which stay off; DSW2 reads nothing.
const PAC_REST: [u8; 4] = [0xFF, 0x7F, 0x00, 0x00];

/// The switches a set's cabinet offers, and its ports with nothing pressed and those switches at 0.
pub fn family_of(set: &SetSpec) -> Family {
    match set.dips {
        Dips::Table(i) => {
            let switches: Vec<&'static Switch> = FAMILIES[i].switches.iter().collect();
            let mut base = set.rest;
            for s in &switches {
                base[s.port] &= !s.mask;
            }
            Family { switches, base }
        }
        d => family(d),
    }
}

/// The Pac-Man board's families (their ports at rest are the same for every set).
pub fn family(dips: Dips) -> Family {
    let with_fixed = |switches: &'static [&'static Switch], dsw1: u8| {
        let mut base = PAC_REST;
        base[2] = dsw1;
        Family { switches: switches.to_vec(), base }
    };
    match dips {
        Dips::Pacman => with_fixed(PAC_ALL, 0x00),
        // The ghost names' switch isn't used: it reads 1.
        Dips::MsPacman => {
            with_fixed(&[&PAC_COINS, &PAC_LIVES, &PAC_BONUS, &PAC_DIFFICULTY, &PAC_CABINET], 0x80)
        }
        // Coinage, lives and an unknown switch of its own, at the factory setting.
        Dips::ComeCome => with_fixed(&[&PAC_BONUS, &PAC_DIFFICULTY, &PAC_GHOSTS, &PAC_CABINET], 0x0C),
        // Its own bonus scores, at the factory setting (the lowest).
        Dips::Newpuc2 => {
            with_fixed(&[&PAC_COINS, &PAC_LIVES, &PAC_DIFFICULTY, &PAC_GHOSTS, &PAC_CABINET], 0x00)
        }
        // Its own coinage, at the factory setting.
        Dips::Pacuman => {
            with_fixed(&[&PAC_LIVES, &PAC_BONUS, &PAC_DIFFICULTY, &PAC_GHOSTS, &PAC_CABINET], 0x00)
        }
        // Its Hard setting breaks the game, so difficulty stays Normal.
        Dips::PacmanSp => {
            with_fixed(&[&PAC_COINS, &PAC_LIVES, &PAC_BONUS, &PAC_GHOSTS, &PAC_CABINET], 0x40)
        }
        Dips::Table(i) => Family { switches: FAMILIES[i].switches.iter().collect(), base: [0; 4] },
    }
}

impl Switch {
    fn bits(&self, value: Option<&str>) -> u8 {
        let pick = |v: &str| self.values.iter().find(|(id, ..)| id.to_str() == Ok(v));
        let (_, _, bits) = value
            .and_then(pick)
            .or_else(|| pick(self.default.to_str().unwrap_or_default()))
            .expect("a switch's default is one of its values");
        *bits & self.mask
    }
}

/// The ports with the switches set as `value` says (a key's chosen value, or None for the default).
pub fn switches(set: &SetSpec, value: impl Fn(&CStr) -> Option<String>) -> Switches {
    let f = family_of(set);
    let mut ports = f.base;
    for s in f.switches {
        ports[s.port] |= s.bits(value(s.key).as_deref());
    }
    Switches { ports }
}

const NULL_VALUE: OptValue = OptValue { value: std::ptr::null(), label: std::ptr::null() };

/// The frontend's environment call and the loaded game's switches, so the display callback can
/// hide the switches the game's cabinet doesn't have.
static ENV: Mutex<Option<EnvFn>> = Mutex::new(None);
static LOADED: Mutex<Option<Dips>> = Mutex::new(None);

/// Declares every switch. Called from retro_set_environment.
///
/// # Safety
/// `env` must be the frontend's environment callback.
pub unsafe fn declare(env: EnvFn) {
    // Built once and leaked: the frontend may keep the pointers.
    let defs: &'static mut Vec<OptDefinition> = Box::leak(Box::default());
    let all = all_switches();
    for s in &all {
        let mut values = [NULL_VALUE; 128];
        for (i, (v, l, _)) in s.values.iter().enumerate() {
            values[i] = OptValue { value: v.as_ptr(), label: l.as_ptr() };
        }
        defs.push(OptDefinition {
            key: s.key.as_ptr(),
            desc: s.label.as_ptr(),
            desc_categorized: s.label.as_ptr(),
            info: s.info.as_ptr(),
            info_categorized: s.info.as_ptr(),
            category_key: s.category.as_ptr(),
            values,
            default_value: s.default.as_ptr(),
        });
    }
    defs.push(OptDefinition {
        key: std::ptr::null(),
        desc: std::ptr::null(),
        desc_categorized: std::ptr::null(),
        info: std::ptr::null(),
        info_categorized: std::ptr::null(),
        category_key: std::ptr::null(),
        values: [NULL_VALUE; 128],
        default_value: std::ptr::null(),
    });
    let cats: &'static mut Vec<OptCategory> = Box::leak(Box::default());
    for (k, d, i) in categories() {
        cats.push(OptCategory { key: k.as_ptr(), desc: d.as_ptr(), info: i.as_ptr() });
    }
    cats.push(OptCategory { key: std::ptr::null(), desc: std::ptr::null(), info: std::ptr::null() });
    let mut opts = OptionsV2 { categories: cats.as_ptr(), definitions: defs.as_ptr() };
    if env(ENV_SET_CORE_OPTIONS_V2, &mut opts as *mut OptionsV2 as *mut c_void) {
        if let Ok(mut slot) = ENV.lock() {
            *slot = Some(env);
        }
        let mut cb = UpdateDisplayCallback { callback: Some(refresh_display) };
        env(
            ENV_SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK,
            &mut cb as *mut UpdateDisplayCallback as *mut c_void,
        );
        return;
    }
    // Older frontends: "Label; default|other|...".
    let texts: &'static mut Vec<std::ffi::CString> = Box::leak(Box::default());
    let vars: &'static mut Vec<Variable> = Box::leak(Box::default());
    for s in &all {
        let mut list = vec![s.default.to_string_lossy().into_owned()];
        for (v, ..) in s.values {
            if *v != s.default {
                list.push(v.to_string_lossy().into_owned());
            }
        }
        let text = format!("{}; {}", s.label.to_string_lossy(), list.join("|"));
        texts.push(std::ffi::CString::new(text).unwrap_or_default());
        let value = texts.last().map_or(std::ptr::null(), |t| t.as_ptr());
        vars.push(Variable { key: s.key.as_ptr(), value });
    }
    vars.push(Variable { key: std::ptr::null(), value: std::ptr::null() });
    env(ENV_SET_VARIABLES, vars.as_mut_ptr() as *mut c_void);
}

/// Shows the loaded game's switches and hides the rest (all of them show while no game is loaded).
unsafe fn show_for(env: EnvFn, dips: Option<Dips>) {
    let offered = dips.map(|d| family(d).switches);
    for s in all_switches() {
        let visible = offered.as_ref().is_none_or(|o| o.iter().any(|x| x.key == s.key));
        let mut d = OptDisplay { key: s.key.as_ptr(), visible };
        env(ENV_SET_CORE_OPTIONS_DISPLAY, &mut d as *mut OptDisplay as *mut c_void);
    }
}

/// A game loaded (or unloaded, None): its cabinet's switches are the ones to show.
///
/// # Safety
/// `env` must be the frontend's environment callback.
pub unsafe fn game_loaded(env: EnvFn, dips: Option<Dips>) {
    if let Ok(mut slot) = LOADED.lock() {
        *slot = dips;
    }
    show_for(env, dips);
}

/// Asked by the frontend, before it lists the settings, which ones to show right now.
/// The frontend calls it like an entry point, so it is guarded like one (`libretro::guarded`).
unsafe extern "C" fn refresh_display() -> bool {
    crate::libretro::guarded("refresh_display", false, || false, || unsafe {
        let Some(env) = ENV.lock().ok().and_then(|e| *e) else { return false };
        let dips = LOADED.lock().ok().and_then(|d| *d);
        show_for(env, dips);
        true
    })
}

unsafe fn get(env: EnvFn, key: &CStr) -> Option<String> {
    let mut var = Variable { key: key.as_ptr(), value: std::ptr::null() };
    if env(ENV_GET_VARIABLE, &mut var as *mut Variable as *mut c_void) && !var.value.is_null() {
        Some(CStr::from_ptr(var.value).to_string_lossy().into_owned())
    } else {
        None
    }
}

/// The switches the frontend's settings ask for.
///
/// # Safety
/// `env` must be the frontend's environment callback.
pub unsafe fn read(env: EnvFn, set: &SetSpec) -> Switches {
    switches(set, |k| get(env, k))
}

/// Whether a setting changed since the last look.
///
/// # Safety
/// `env` must be the frontend's environment callback.
pub unsafe fn changed(env: EnvFn) -> bool {
    let mut updated = false;
    env(ENV_GET_VARIABLE_UPDATE, &mut updated as *mut bool as *mut c_void) && updated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sets::SETS;

    /// Every set's switches at their defaults are MAME's factory settings, bit for bit, on every
    /// bit MAME's switches cover.
    #[test]
    fn defaults_are_the_factory_settings() {
        for set in SETS {
            let s = switches(set, |_| None);
            let generated = matches!(set.dips, Dips::Table(_));
            for &(port, mask, value) in set.factory {
                let i = match (port, generated) {
                    ("IN0", _) => 0,
                    ("IN1", _) => 1,
                    ("DSW1", false) | ("IN2", true) => 2,
                    ("IN3", true) | ("DSW0", true) => 3,
                    (other, _) => panic!("{}: a switch in {other}", set.name),
                };
                assert_eq!(s.ports[i] & mask, value, "{}: {port}", set.name);
            }
        }
    }

    #[test]
    fn a_setting_moves_only_its_own_bits() {
        let pick = |key: &'static CStr, v: &'static str| move |k: &CStr| (k == key).then(|| v.to_string());
        let pacman = crate::sets::find("pacman").unwrap();
        let base = switches(pacman, |_| None).ports;
        assert_eq!(base, [0xFF, 0xFF, 0xC9, 0x00]);
        assert_eq!(switches(pacman, pick(c"carcade_pacman_lives", "5")).ports[2], 0xCD);
        assert_eq!(switches(pacman, pick(c"carcade_pacman_coinage", "free")).ports[2], 0xC8);
        assert_eq!(switches(pacman, pick(c"carcade_pacman_bonus", "none")).ports[2], 0xF9);
        assert_eq!(switches(pacman, pick(c"carcade_pacman_cabinet", "cocktail")).ports[1], 0x7F);
        // A value that isn't one is the default.
        assert_eq!(switches(pacman, pick(c"carcade_pacman_lives", "9")).ports, base);
        // A switch the cabinet doesn't have stays at the factory setting.
        let comecome = SETS.iter().find(|s| s.dips == Dips::ComeCome).unwrap();
        assert_eq!(switches(comecome, pick(c"carcade_pacman_lives", "5")).ports[2], 0xCC);
    }

    #[test]
    fn every_switch_is_plain_and_whole() {
        let mut keys = std::collections::HashSet::new();
        for s in all_switches() {
            assert!(keys.insert(s.key), "{:?} twice", s.key);
            let key = s.key.to_str().unwrap();
            assert!(key.starts_with("carcade_"), "{key}");
            assert!(s.values.iter().any(|(v, ..)| *v == s.default), "{key}'s default");
            assert!(s.values.iter().all(|&(_, _, b)| b & !s.mask == 0), "{key}'s bits");
            assert!(categories().iter().any(|(c, ..)| *c == s.category), "{key}'s category");
            let label = s.label.to_str().unwrap();
            assert!(label.len() <= 20 && s.info.to_bytes().len() <= 80, "{key}: short words");
        }
    }
}
