//! cAtari's settings, declared to the app the libretro way (v2, with categories), and what each
//! one does. The list it covers is .kiro/notes/docs/A7800_OPTIONS_INVENTORY.md. Every setting has a
//! sensible default: what a real console does.

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

pub type Env = unsafe extern "C" fn(u32, *mut c_void) -> bool;
type Values = &'static [(&'static CStr, &'static CStr)];
/// (category, key, label, info, values, default)
type Spec = (&'static CStr, &'static CStr, &'static CStr, &'static CStr, Values, &'static CStr);

const CATEGORIES: &[(&CStr, &CStr, &CStr)] = &[
    (c"system", c"System", c"The TV system, the console's start-up screen and its timing."),
    (c"video", c"Picture", c"Colours, the whole picture and flicker."),
    (c"audio", c"Sound", c"Volume, the two sound chips and the filter."),
    (c"input", c"Controls", c"The controllers, the light gun and the difficulty switches."),
];

const OFF_ON: Values = &[(c"off", c"Off"), (c"on", c"On")];
const ON_OFF: Values = &[(c"on", c"On"), (c"off", c"Off")];
const PCT: Values = &[(c"100", c"100%"), (c"75", c"75%"), (c"50", c"50%"), (c"25", c"25%"), (c"0", c"Off")];
const AB: Values = &[(c"a", c"A (advanced)"), (c"b", c"B (beginner)")];
const OFFSETS: Values = &[
    (c"-10", c"-10"), (c"-8", c"-8"), (c"-6", c"-6"), (c"-4", c"-4"), (c"-2", c"-2"), (c"0", c"0"),
    (c"2", c"+2"), (c"4", c"+4"), (c"6", c"+6"), (c"8", c"+8"), (c"10", c"+10"),
];
const FILTER_LEVELS: Values = &[
    (c"5", c"5%"), (c"10", c"10%"), (c"15", c"15%"), (c"20", c"20%"), (c"25", c"25%"), (c"30", c"30%"),
    (c"35", c"35%"), (c"40", c"40%"), (c"45", c"45%"), (c"50", c"50%"), (c"55", c"55%"), (c"60", c"60%"),
    (c"65", c"65%"), (c"70", c"70%"), (c"75", c"75%"), (c"80", c"80%"), (c"85", c"85%"), (c"90", c"90%"),
    (c"95", c"95%"),
];

const SPECS: &[Spec] = &[
    (c"system", c"catari_region", c"TV system",
     c"Auto goes by the game; European games want PAL. Changes apply at the next start.",
     &[(c"auto", c"Auto"), (c"ntsc", c"NTSC (60 Hz)"), (c"pal", c"PAL (50 Hz)")], c"auto"),
    (c"system", c"catari_bios", c"Console start-up screen",
     c"Plays the Atari logo first if you have the console's BIOS file. Changes apply at the next start.",
     OFF_ON, c"off"),
    (c"system", c"catari_slow_io", c"Slow chip access",
     c"The console slows down to talk to its sound and controller chips; light gun games need it.",
     &[(c"auto", c"Auto (light gun games)"), (c"on", c"Always, like the console"), (c"off", c"Never")], c"auto"),
    (c"video", c"catari_palette", c"Colours",
     c"Consoles differed a little in colour, and warmed up as they ran.",
     &[(c"atari", c"Atari"), (c"cool", c"Cool"), (c"warm", c"Warm"), (c"hot", c"Hot"), (c"pal", c"PAL console"),
       (c"grey", c"Black and white")], c"atari"),
    (c"video", c"catari_saturation", c"Colour strength", c"",
     &[(c"100", c"100%"), (c"125", c"125%"), (c"150", c"150%"), (c"75", c"75%"), (c"50", c"50%")], c"100"),
    (c"video", c"catari_brightness", c"Brightness", c"",
     &[(c"100", c"100%"), (c"110", c"110%"), (c"120", c"120%"), (c"90", c"90%"), (c"80", c"80%")], c"100"),
    (c"video", c"catari_overscan", c"Whole picture",
     c"Shows the lines a TV hid at the top and bottom. Changes apply at the next start.",
     OFF_ON, c"off"),
    (c"video", c"catari_blend", c"Smooth flicker",
     c"Many games flash things every other frame; this blends two frames to steady them.",
     &[(c"off", c"Off"), (c"light", c"Light"), (c"on", c"Half and half")], c"off"),
    (c"audio", c"catari_volume", c"Volume", c"", PCT, c"100"),
    (c"audio", c"catari_tia_volume", c"Console sound (TIA)", c"", PCT, c"100"),
    (c"audio", c"catari_pokey_volume", c"Cartridge sound (POKEY)",
     c"The extra sound chip in some cartridges, like Ballblazer and Commando.", PCT, c"100"),
    (c"audio", c"catari_lowpass", c"Soften the sound",
     c"Takes the harsh edge off the TIA's tones.", OFF_ON, c"off"),
    (c"audio", c"catari_lowpass_level", c"Softening strength", c"Higher takes away more of the high notes.",
     FILTER_LEVELS, c"60"),
    (c"audio", c"catari_sound_rate", c"Sound sample rate", c"Changes apply at the next start.",
     &[(c"48000", c"48 kHz"), (c"44100", c"44.1 kHz"), (c"32000", c"32 kHz"), (c"22050", c"22 kHz")], c"48000"),
    (c"input", c"catari_port1", c"Controller 1",
     c"Auto picks the light gun or paddles for the games made for them. Changes apply at the next start.",
     &[(c"auto", c"Auto"), (c"joystick", c"Joystick"), (c"lightgun", c"Light gun"), (c"paddles", c"Paddles")], c"auto"),
    (c"input", c"catari_port2", c"Controller 2", c"Changes apply at the next start.",
     &[(c"auto", c"Auto"), (c"joystick", c"Joystick"), (c"paddles", c"Paddles")], c"auto"),
    (c"input", c"catari_dual_stick", c"Dual-stick games",
     c"Puts player 2's stick on your right stick, for Robotron: 2084 and the like.", OFF_ON, c"off"),
    (c"input", c"catari_left_difficulty", c"Left difficulty switch", c"", AB, c"a"),
    (c"input", c"catari_right_difficulty", c"Right difficulty switch", c"", AB, c"a"),
    (c"input", c"catari_difficulty_buttons", c"Difficulty buttons",
     c"Flip makes each press flip the switch; Hold sets it to B only while held.",
     &[(c"flip", c"Flip"), (c"hold", c"Hold")], c"flip"),
    (c"input", c"catari_opposing", c"Allow opposite directions",
     c"Up with Down, Left with Right; a real stick can't, and some games glitch with it.", OFF_ON, c"off"),
    (c"input", c"catari_crosshair", c"Light gun crosshair", c"Shows where the gun points.", ON_OFF, c"on"),
    (c"input", c"catari_crosshair_colour", c"Crosshair colour", c"",
     &[(c"white", c"White"), (c"black", c"Black"), (c"red", c"Red"), (c"green", c"Green"), (c"blue", c"Blue"),
       (c"yellow", c"Yellow"), (c"magenta", c"Magenta"), (c"cyan", c"Cyan")], c"white"),
    (c"input", c"catari_gun_x", c"Light gun aim, sideways",
     c"Moves where shots land, if a game hits a little to one side.", OFFSETS, c"0"),
    (c"input", c"catari_gun_y", c"Light gun aim, up and down", c"", OFFSETS, c"0"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Port {
    Auto,
    Joystick,
    LightGun,
    Paddles,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlowIo {
    Auto,
    On,
    Off,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub region: Option<bool>,
    pub bios: bool,
    pub slow_io: SlowIo,
    pub palette: crate::palette::Preset,
    pub saturation: u8,
    pub brightness: u8,
    pub overscan: bool,
    pub blend: u8,
    pub volume: u8,
    pub tia_volume: u8,
    pub pokey_volume: u8,
    pub lowpass: bool,
    pub lowpass_level: u8,
    pub sound_rate: u32,
    pub ports: [Port; 2],
    pub dual_stick: bool,
    pub difficulty_a: [bool; 2],
    pub difficulty_hold: bool,
    pub opposing: bool,
    pub crosshair: bool,
    pub crosshair_colour: u32,
    pub gun_offset: (i32, i32),
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            region: None,
            bios: false,
            slow_io: SlowIo::Auto,
            palette: crate::palette::Preset::Atari,
            saturation: 100,
            brightness: 100,
            overscan: false,
            blend: 0,
            volume: 100,
            tia_volume: 100,
            pokey_volume: 100,
            lowpass: false,
            lowpass_level: 60,
            sound_rate: 48_000,
            ports: [Port::Auto; 2],
            dual_stick: false,
            difficulty_a: [true; 2],
            difficulty_hold: false,
            opposing: false,
            crosshair: true,
            crosshair_colour: 0xFFFFFF,
            gun_offset: (0, 0),
        }
    }
}

/// Settings from a lookup of option values (a missing one keeps its default).
pub fn parse(get: impl Fn(&CStr) -> Option<String>) -> Settings {
    use crate::palette::Preset;
    let mut s = Settings::default();
    let on = |k: &CStr, d: bool| get(k).map_or(d, |v| v == "on");
    let pct = |k: &CStr| get(k).and_then(|v| v.parse::<u8>().ok()).unwrap_or(100).min(100);
    s.region = match get(c"catari_region").as_deref() {
        Some("ntsc") => Some(false),
        Some("pal") => Some(true),
        _ => None,
    };
    s.bios = on(c"catari_bios", false);
    s.slow_io = match get(c"catari_slow_io").as_deref() {
        Some("on") => SlowIo::On,
        Some("off") => SlowIo::Off,
        _ => SlowIo::Auto,
    };
    s.palette = match get(c"catari_palette").as_deref() {
        Some("cool") => Preset::Cool,
        Some("warm") => Preset::Warm,
        Some("hot") => Preset::Hot,
        Some("pal") => Preset::Pal,
        Some("grey") => Preset::Grey,
        _ => Preset::Atari,
    };
    s.saturation = get(c"catari_saturation").and_then(|v| v.parse().ok()).unwrap_or(100).clamp(50, 150);
    s.brightness = get(c"catari_brightness").and_then(|v| v.parse().ok()).unwrap_or(100).clamp(80, 120);
    s.overscan = on(c"catari_overscan", false);
    s.blend = match get(c"catari_blend").as_deref() {
        Some("light") => 25,
        Some("on") => 50,
        _ => 0,
    };
    s.volume = pct(c"catari_volume");
    s.tia_volume = pct(c"catari_tia_volume");
    s.pokey_volume = pct(c"catari_pokey_volume");
    s.lowpass = on(c"catari_lowpass", false);
    s.lowpass_level = get(c"catari_lowpass_level").and_then(|v| v.parse().ok()).unwrap_or(60).clamp(5, 95);
    s.sound_rate = match get(c"catari_sound_rate").as_deref() {
        Some("44100") => 44_100,
        Some("32000") => 32_000,
        Some("22050") => 22_050,
        _ => 48_000,
    };
    let port = |k: &CStr| match get(k).as_deref() {
        Some("joystick") => Port::Joystick,
        Some("lightgun") => Port::LightGun,
        Some("paddles") => Port::Paddles,
        _ => Port::Auto,
    };
    s.ports = [port(c"catari_port1"), port(c"catari_port2")];
    if s.ports[1] == Port::LightGun {
        s.ports[1] = Port::Auto;
    }
    s.dual_stick = on(c"catari_dual_stick", false);
    s.difficulty_a = [
        get(c"catari_left_difficulty").is_none_or(|v| v != "b"),
        get(c"catari_right_difficulty").is_none_or(|v| v != "b"),
    ];
    s.difficulty_hold = get(c"catari_difficulty_buttons").is_some_and(|v| v == "hold");
    s.opposing = on(c"catari_opposing", false);
    s.crosshair = on(c"catari_crosshair", true);
    s.crosshair_colour = match get(c"catari_crosshair_colour").as_deref() {
        Some("black") => 0x000000,
        Some("red") => 0xFF0000,
        Some("green") => 0x00FF00,
        Some("blue") => 0x0000FF,
        Some("yellow") => 0xFFFF00,
        Some("magenta") => 0xFF00FF,
        Some("cyan") => 0x00FFFF,
        _ => 0xFFFFFF,
    };
    let offset = |k: &CStr| get(k).and_then(|v| v.parse::<i32>().ok()).unwrap_or(0).clamp(-10, 10);
    s.gun_offset = (offset(c"catari_gun_x"), offset(c"catari_gun_y"));
    s
}

/// Declares the settings to the app, once, from retro_set_environment.
///
/// # Safety
/// `env` must be the frontend's environment callback.
pub unsafe fn declare(env: Env) {
    // Built once and leaked: the app may keep the pointers.
    let defs: &'static mut Vec<OptDefinition> = Box::leak(Box::default());
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
    let cats: &'static mut Vec<OptCategory> = Box::leak(Box::default());
    for (k, d, i) in CATEGORIES {
        cats.push(OptCategory { key: k.as_ptr(), desc: d.as_ptr(), info: i.as_ptr() });
    }
    cats.push(OptCategory { key: std::ptr::null(), desc: std::ptr::null(), info: std::ptr::null() });
    let mut opts = OptionsV2 { categories: cats.as_ptr(), definitions: defs.as_ptr() };
    if env(ENV_SET_CORE_OPTIONS_V2, &mut opts as *mut OptionsV2 as *mut c_void) {
        return;
    }
    // Older frontends: "Label; default|other|...".
    let texts: &'static mut Vec<std::ffi::CString> = Box::leak(Box::default());
    let vars: &'static mut Vec<Variable> = Box::leak(Box::default());
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

/// # Safety
/// `env` must be the frontend's environment callback.
pub unsafe fn read(env: Env) -> Settings {
    parse(|k| get(env, k))
}

/// # Safety
/// `env` must be the frontend's environment callback.
pub unsafe fn changed(env: Env) -> bool {
    let mut updated = false;
    env(ENV_GET_VARIABLE_UPDATE, &mut updated as *mut bool as *mut c_void) && updated
}

/// Every key cAtari declares (for the inventory check).
pub fn keys() -> Vec<&'static str> {
    SPECS.iter().map(|s| s.1.to_str().unwrap()).collect()
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
