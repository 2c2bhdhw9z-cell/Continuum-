//! cGear's settings, declared to the app the libretro way (v2, with categories), and what each
//! one does. The list to reach is .kiro/notes/docs/SMS_OPTIONS_INVENTORY.md; this is the first
//! part of it. Every setting has a sensible default: what a real console does.

use std::ffi::{c_char, c_void, CStr};

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

const ENV_GET_VARIABLE: u32 = 15;
const ENV_SET_VARIABLES: u32 = 16;
const ENV_GET_VARIABLE_UPDATE: u32 = 17;
const ENV_SET_CORE_OPTIONS_V2: u32 = 67;

type Env = unsafe extern "C" fn(u32, *mut c_void) -> bool;
type Values = &'static [(&'static CStr, &'static CStr)];
/// (category, key, label, info, values, default)
type Spec = (&'static CStr, &'static CStr, &'static CStr, &'static CStr, Values, &'static CStr);

const CATEGORIES: &[(&CStr, &CStr, &CStr)] = &[
    (c"system", c"System", c"Which console, its region and how the cartridge is wired."),
    (c"video", c"Picture", c"Sprites, the left column and the Game Gear's screen."),
    (c"audio", c"Sound", c"Volume and channel mix."),
    (c"input", c"Controls", c"The pad."),
];

const OFF_ON: Values = &[(c"off", c"Off"), (c"on", c"On")];
const PCT: Values = &[(c"100", c"100%"), (c"75", c"75%"), (c"50", c"50%"), (c"25", c"25%"), (c"0", c"Off")];

const SPECS: &[Spec] = &[
    (c"system", c"cgear_console", c"Console",
     c"Auto picks it from the game's file and header; changes apply at the next start.",
     &[(c"auto", c"Auto"), (c"sms", c"Master System"), (c"gg", c"Game Gear"), (c"sg", c"SG-1000")], c"auto"),
    (c"system", c"cgear_region", c"Region",
     c"Some games change their text or music on a Japanese console; changes apply at the next start.",
     &[(c"export", c"Export (America, Europe)"), (c"japan", c"Japan")], c"export"),
    (c"system", c"cgear_refresh", c"Refresh rate",
     c"European games want 50 Hz; changes apply at the next start.",
     &[(c"60", c"60 Hz"), (c"50", c"50 Hz")], c"60"),
    (c"system", c"cgear_mapper", c"Cartridge wiring",
     c"Auto works for nearly every game; changes apply at the next start.",
     &[(c"auto", c"Auto"), (c"sega", c"Sega"), (c"codemasters", c"Codemasters"), (c"korean", c"Korean")], c"auto"),
    (c"video", c"cgear_sprite_limit", c"Eight sprites a line",
     c"Off draws every sprite, so nothing flickers, but a few games use the limit on purpose.",
     &[(c"on", c"On"), (c"off", c"Off")], c"on"),
    (c"video", c"cgear_hide_left", c"Hide the left column",
     c"Many Master System games leave an empty strip at the left; changes apply at the next start.",
     OFF_ON, c"off"),
    (c"video", c"cgear_gg_ghosting", c"Game Gear screen ghosting",
     c"Blends each picture with the last, as the Game Gear's slow screen did.",
     OFF_ON, c"off"),
    (c"audio", c"cgear_volume", c"Volume", c"", PCT, c"100"),
    (c"audio", c"cgear_vol_tone1", c"Tone 1", c"", PCT, c"100"),
    (c"audio", c"cgear_vol_tone2", c"Tone 2", c"", PCT, c"100"),
    (c"audio", c"cgear_vol_tone3", c"Tone 3", c"", PCT, c"100"),
    (c"audio", c"cgear_vol_noise", c"Noise", c"", PCT, c"100"),
    (c"audio", c"cgear_mono", c"Mono sound",
     c"Game Gear games are in stereo; this mixes them into one.",
     OFF_ON, c"off"),
    (c"input", c"cgear_opposing", c"Allow opposite directions",
     c"Up with Down, Left with Right; the pad itself can't, and some games glitch with it.",
     OFF_ON, c"off"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Console {
    Auto,
    Sms,
    GameGear,
    Sg1000,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub console: Console,
    pub japan: bool,
    pub pal: bool,
    pub mapper: Option<crate::bus::Mapper>,
    pub sprite_limit: bool,
    /// Cut the Master System's leftmost eight pixels off the picture.
    pub hide_left: bool,
    pub gg_ghosting: bool,
    pub volume: u8,
    pub chan_vol: [u8; 4],
    pub mono: bool,
    pub opposing: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            console: Console::Auto, japan: false, pal: false, mapper: None, sprite_limit: true, hide_left: false,
            gg_ghosting: false, volume: 100, chan_vol: [100; 4], mono: false, opposing: false,
        }
    }
}

/// Settings from a lookup of option values (a missing one keeps its default).
pub fn parse(get: impl Fn(&CStr) -> Option<String>) -> Settings {
    use crate::bus::Mapper;
    let mut s = Settings::default();
    let on = |k: &CStr, d: bool| get(k).map_or(d, |v| v == "on");
    let pct = |k: &CStr| get(k).and_then(|v| v.parse::<u8>().ok()).unwrap_or(100).min(100);
    s.console = match get(c"cgear_console").as_deref() {
        Some("sms") => Console::Sms,
        Some("gg") => Console::GameGear,
        Some("sg") => Console::Sg1000,
        _ => Console::Auto,
    };
    s.japan = get(c"cgear_region").is_some_and(|v| v == "japan");
    s.pal = get(c"cgear_refresh").is_some_and(|v| v == "50");
    s.mapper = match get(c"cgear_mapper").as_deref() {
        Some("sega") => Some(Mapper::Sega),
        Some("codemasters") => Some(Mapper::Codemasters),
        Some("korean") => Some(Mapper::Korean),
        _ => None,
    };
    s.sprite_limit = on(c"cgear_sprite_limit", true);
    s.hide_left = on(c"cgear_hide_left", false);
    s.gg_ghosting = on(c"cgear_gg_ghosting", false);
    s.volume = pct(c"cgear_volume");
    s.chan_vol = [pct(c"cgear_vol_tone1"), pct(c"cgear_vol_tone2"), pct(c"cgear_vol_tone3"), pct(c"cgear_vol_noise")];
    s.mono = on(c"cgear_mono", false);
    s.opposing = on(c"cgear_opposing", false);
    s
}

/// Declares the settings to the app, once, from retro_set_environment.
pub unsafe fn declare(env: Env) {
    // Built once and leaked: the app may keep the pointers.
    let defs: &'static mut Vec<OptDefinition> = Box::leak(Box::new(Vec::new()));
    for (cat, key, desc, info, values, default) in SPECS {
        let mut vals: [OptValue; 128] = std::array::from_fn(|_| OptValue { value: std::ptr::null(), label: std::ptr::null() });
        for (i, (v, l)) in values.iter().enumerate() {
            vals[i] = OptValue { value: v.as_ptr(), label: l.as_ptr() };
        }
        defs.push(OptDefinition {
            key: key.as_ptr(), desc: desc.as_ptr(), desc_categorized: desc.as_ptr(), info: info.as_ptr(),
            info_categorized: info.as_ptr(), category_key: cat.as_ptr(), values: vals, default_value: default.as_ptr(),
        });
    }
    defs.push(OptDefinition {
        key: std::ptr::null(), desc: std::ptr::null(), desc_categorized: std::ptr::null(), info: std::ptr::null(),
        info_categorized: std::ptr::null(), category_key: std::ptr::null(),
        values: std::array::from_fn(|_| OptValue { value: std::ptr::null(), label: std::ptr::null() }), default_value: std::ptr::null(),
    });
    let cats: &'static mut Vec<OptCategory> = Box::leak(Box::new(Vec::new()));
    for (k, d, i) in CATEGORIES {
        cats.push(OptCategory { key: k.as_ptr(), desc: d.as_ptr(), info: i.as_ptr() });
    }
    cats.push(OptCategory { key: std::ptr::null(), desc: std::ptr::null(), info: std::ptr::null() });
    let mut opts = OptionsV2 { categories: cats.as_ptr(), definitions: defs.as_ptr() };
    if env(ENV_SET_CORE_OPTIONS_V2, &mut opts as *mut OptionsV2 as *mut c_void) {
        return;
    }
    // Older frontends: "Label; default|other|...".
    let texts: &'static mut Vec<std::ffi::CString> = Box::leak(Box::new(Vec::new()));
    let vars: &'static mut Vec<Variable> = Box::leak(Box::new(Vec::new()));
    for (_, key, desc, _, values, default) in SPECS {
        let mut list = vec![default.to_string_lossy().into_owned()];
        list.extend(values.iter().filter(|(v, _)| v != default).map(|(v, _)| v.to_string_lossy().into_owned()));
        texts.push(std::ffi::CString::new(format!("{}; {}", desc.to_string_lossy(), list.join("|"))).unwrap_or_default());
        vars.push(Variable { key: key.as_ptr(), value: texts.last().map_or(std::ptr::null(), |t| t.as_ptr()) });
    }
    vars.push(Variable { key: std::ptr::null(), value: std::ptr::null() });
    env(ENV_SET_VARIABLES, vars.as_mut_ptr() as *mut c_void);
}

unsafe fn get(env: Env, key: &CStr) -> Option<String> {
    let mut var = Variable { key: key.as_ptr(), value: std::ptr::null() };
    if env(ENV_GET_VARIABLE, &mut var as *mut Variable as *mut c_void) && !var.value.is_null() {
        Some(CStr::from_ptr(var.value).to_string_lossy().into_owned())
    } else {
        None
    }
}

pub unsafe fn read(env: Env) -> Settings {
    parse(|k| get(env, k))
}

pub unsafe fn changed(env: Env) -> bool {
    let mut updated = false;
    env(ENV_GET_VARIABLE_UPDATE, &mut updated as *mut bool as *mut c_void) && updated
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(pairs: &[(&str, &str)]) -> Settings {
        parse(|k| pairs.iter().find(|(key, _)| k.to_str() == Ok(*key)).map(|(_, v)| v.to_string()))
    }

    #[test]
    fn nothing_set_is_a_real_console() {
        assert_eq!(settings(&[]), Settings::default());
    }

    #[test]
    fn every_value_offered_is_one_the_core_reads() {
        for (_, key, _, _, values, default) in SPECS {
            assert!(values.iter().any(|(v, _)| v == default), "{key:?}: its default is one of its values");
            for (v, _) in values.iter() {
                let s = settings(&[(key.to_str().unwrap(), v.to_str().unwrap())]);
                let back = settings(&[(key.to_str().unwrap(), default.to_str().unwrap())]);
                if v != default {
                    assert_ne!(s, back, "{key:?} = {v:?} changes nothing");
                }
            }
        }
    }

    #[test]
    fn each_line_under_a_setting_is_one_short_sentence() {
        for (_, key, _, info, _, _) in SPECS {
            let t = info.to_str().unwrap();
            assert!(t.is_empty() || (t.ends_with('.') && t.split_whitespace().count() <= 22), "{key:?}: {t}");
        }
    }
}
