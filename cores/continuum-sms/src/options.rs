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
    (c"input", c"Controls", c"The pad, and the gun, paddle and trackball."),
    (c"link", c"Link", c"Two Game Gears joined by the cable."),
];

const OFF_ON: Values = &[(c"off", c"Off"), (c"on", c"On")];
const PCT: Values = &[(c"100", c"100%"), (c"75", c"75%"), (c"50", c"50%"), (c"25", c"25%"), (c"0", c"Off")];

const DEVICES: Values = &[(c"pad", c"Pad"), (c"phaser", c"Light Phaser"), (c"paddle", c"Paddle"), (c"sports", c"Sports Pad")];
const OFF_ON_DEFAULT_ON: Values = &[(c"on", c"On"), (c"off", c"Off")];
const OFFSETS: Values = &[
    (c"-10", c"-10"), (c"-8", c"-8"), (c"-6", c"-6"), (c"-4", c"-4"), (c"-2", c"-2"), (c"0", c"0"),
    (c"2", c"+2"), (c"4", c"+4"), (c"6", c"+6"), (c"8", c"+8"), (c"10", c"+10"),
];
const SPEEDS: Values = &[
    (c"1", c"1"), (c"2", c"2"), (c"3", c"3"), (c"4", c"4"), (c"5", c"5"), (c"6", c"6"), (c"7", c"7"),
    (c"8", c"8"), (c"9", c"9"), (c"10", c"10"),
];

/// An equaliser band's level, percent.
const EQ: Values = &[(c"150", c"150%"), (c"125", c"125%"), (c"100", c"100%"), (c"75", c"75%"), (c"50", c"50%"), (c"25", c"25%"), (c"0", c"Off")];

const SPECS: &[Spec] = &[
    (c"system", c"cgear_console", c"Console",
     c"Auto picks it from the game's file and header; changes apply at the next start.",
     &[(c"auto", c"Auto"), (c"sms", c"Master System"), (c"gg", c"Game Gear"), (c"sg", c"SG-1000")], c"auto"),
    (c"system", c"cgear_region", c"Region",
     c"Some games change their text or music on a Japanese console; changes apply at the next start.",
     &[(c"auto", c"Auto"), (c"export", c"Export (America, Europe)"), (c"japan", c"Japan")], c"auto"),
    (c"system", c"cgear_refresh", c"Refresh rate",
     c"European games want 50 Hz; Auto goes by the file's name; changes apply at the next start.",
     &[(c"60", c"60 Hz"), (c"50", c"50 Hz"), (c"auto", c"Auto")], c"60"),
    (c"system", c"cgear_mapper", c"Cartridge wiring",
     c"Auto works for nearly every game; changes apply at the next start.",
     &[(c"auto", c"Auto"), (c"sega", c"Sega"), (c"codemasters", c"Codemasters"), (c"korean", c"Korean"),
       (c"msx", c"Korean MSX"), (c"nemesis", c"Nemesis"), (c"janggun", c"Janggun")], c"auto"),
    (c"video", c"cgear_sprite_limit", c"Eight sprites a line",
     c"Off draws every sprite, so nothing flickers, but a few games use the limit on purpose.",
     &[(c"on", c"On"), (c"off", c"Off")], c"on"),
    (c"video", c"cgear_hide_left", c"Hide the left column",
     c"Many Master System games leave an empty strip at the left; changes apply at the next start.",
     OFF_ON, c"off"),
    (c"video", c"cgear_tms_colours", c"Colours in SG-1000 modes",
     c"A Master System shows SG-1000 games in its own darker colours; Auto matches the console being played.",
     &[(c"auto", c"Auto"), (c"sms", c"Master System's"), (c"sg", c"SG-1000's")], c"auto"),
    (c"video", c"cgear_borders", c"Borders",
     c"The coloured edge a TV shows round the picture; changes apply at the next start.",
     &[(c"off", c"Off"), (c"tb", c"Above and below"), (c"full", c"All round")], c"off"),
    (c"video", c"cgear_3d", c"3-D glasses games",
     c"SegaScope 3-D games flash two pictures; showing one eye's stops the flicker.",
     &[(c"both", c"Both eyes"), (c"left", c"Left eye"), (c"right", c"Right eye")], c"both"),
    (c"video", c"cgear_gg_ghosting", c"Game Gear screen ghosting",
     c"Blends each picture with the last, as the Game Gear's slow screen did.",
     &[(c"off", c"Off"), (c"weak", c"Weak"), (c"on", c"Normal")], c"off"),
    (c"video", c"cgear_gg_full", c"Game Gear whole picture",
     c"Shows everything the console draws, beyond its small screen's edges; changes apply at the next start.",
     OFF_ON, c"off"),
    (c"audio", c"cgear_fm", c"FM sound chip",
     c"Richer music in many games; Auto has it on a Japanese Master System only, like the real ones.",
     &[(c"auto", c"Auto"), (c"off", c"Off"), (c"on", c"On")], c"auto"),
    (c"audio", c"cgear_volume", c"Volume", c"", PCT, c"100"),
    (c"audio", c"cgear_fm_volume", c"FM volume", c"", PCT, c"100"),
    (c"audio", c"cgear_vol_tone1", c"Tone 1", c"", PCT, c"100"),
    (c"audio", c"cgear_vol_tone2", c"Tone 2", c"", PCT, c"100"),
    (c"audio", c"cgear_vol_tone3", c"Tone 3", c"", PCT, c"100"),
    (c"audio", c"cgear_vol_noise", c"Noise", c"", PCT, c"100"),
    (c"audio", c"cgear_vol_fm1", c"FM channel 1", c"", PCT, c"100"),
    (c"audio", c"cgear_vol_fm2", c"FM channel 2", c"", PCT, c"100"),
    (c"audio", c"cgear_vol_fm3", c"FM channel 3", c"", PCT, c"100"),
    (c"audio", c"cgear_vol_fm4", c"FM channel 4", c"", PCT, c"100"),
    (c"audio", c"cgear_vol_fm5", c"FM channel 5", c"", PCT, c"100"),
    (c"audio", c"cgear_vol_fm6", c"FM channel 6", c"", PCT, c"100"),
    (c"audio", c"cgear_vol_fm7", c"FM channel 7", c"In drum mode, channels 7 to 9 play the drums.", PCT, c"100"),
    (c"audio", c"cgear_vol_fm8", c"FM channel 8", c"", PCT, c"100"),
    (c"audio", c"cgear_vol_fm9", c"FM channel 9", c"", PCT, c"100"),
    (c"audio", c"cgear_lowpass", c"Soften the sound",
     c"Takes the edge off the high notes, like a TV's speaker.",
     &[(c"off", c"Off"), (c"light", c"Light"), (c"medium", c"Medium"), (c"strong", c"Strong")], c"off"),
    (c"audio", c"cgear_eq_low", c"Bass", c"Below about 880 Hz.", EQ, c"100"),
    (c"audio", c"cgear_eq_mid", c"Middle", c"", EQ, c"100"),
    (c"audio", c"cgear_eq_high", c"Treble", c"Above about 5 kHz.", EQ, c"100"),
    (c"audio", c"cgear_mono", c"Mono sound",
     c"Game Gear games are in stereo; this mixes them into one.",
     OFF_ON, c"off"),
    (c"system", c"cgear_cpu_speed", c"CPU speed",
     c"Faster than the real console takes away slowdown in busy scenes; a few games then run too fast.",
     &[(c"100", c"Real (100%)"), (c"150", c"150%"), (c"200", c"200%"), (c"300", c"300%")], c"100"),
    (c"input", c"cgear_port1", c"Controller port 1",
     c"Light Phaser games, the paddle and the Sports Pad trackball; the stick or D-pad works them.",
     DEVICES, c"pad"),
    (c"input", c"cgear_port2", c"Controller port 2", c"", DEVICES, c"pad"),
    (c"input", c"cgear_crosshair", c"Light Phaser crosshair", c"Shows where the gun points.", OFF_ON_DEFAULT_ON, c"on"),
    (c"input", c"cgear_crosshair_shape", c"Crosshair shape", c"",
     &[(c"cross", c"Cross"), (c"square", c"Square")], c"cross"),
    (c"input", c"cgear_crosshair_colour", c"Crosshair colour", c"",
     &[(c"white", c"White"), (c"black", c"Black"), (c"red", c"Red"), (c"green", c"Green"), (c"blue", c"Blue"),
       (c"yellow", c"Yellow"), (c"magenta", c"Magenta"), (c"cyan", c"Cyan")], c"white"),
    (c"input", c"cgear_phaser_x", c"Light Phaser aim, sideways",
     c"Moves where shots land, if a game hits a little to one side.", OFFSETS, c"0"),
    (c"input", c"cgear_phaser_y", c"Light Phaser aim, up and down", c"", OFFSETS, c"0"),
    (c"input", c"cgear_device_speed", c"Paddle and trackball speed",
     c"How fast the stick turns the paddle, rolls the trackball or moves the gun.", SPEEDS, c"5"),
    (c"link", c"cgear_gg_link", c"Two Game Gears on this phone",
     c"Two consoles play this Game Gear game, joined by the Gear-to-Gear cable; changes apply at the next start.",
     OFF_ON, c"off"),
    (c"link", c"cgear_gg_link_layout", c"Two Game Gears: where the screens go",
     c"Changes apply at the next start.", &[(c"side", c"Side by side"), (c"stacked", c"One above the other")], c"side"),
    (c"link", c"cgear_gg_link_show", c"Two Game Gears: screens shown",
     c"Between both screens and one, changes apply at the next start.",
     &[(c"both", c"Both"), (c"1", c"The first console's only"), (c"2", c"The second console's only")], c"both"),
    (c"link", c"cgear_gg_link_swap", c"Two Game Gears: swap the screens", c"Puts the second console's screen first.", OFF_ON, c"off"),
    (c"link", c"cgear_gg_link_audio", c"Two Game Gears: sound", c"Which console you hear.",
     &[(c"1", c"The first console"), (c"2", c"The second console"), (c"both", c"Both together")], c"1"),
    (c"link", c"cgear_gg_link_controls", c"Two Game Gears: your controls play",
     c"The on-screen buttons and the first controller play this console; a second controller plays the other.",
     &[(c"1", c"The first console"), (c"2", c"The second console")], c"1"),
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
    /// Region and refresh rate picked from the game (its header's region, its file's name).
    pub region_auto: bool,
    pub refresh_auto: bool,
    pub pal: bool,
    pub mapper: Option<crate::bus::Mapper>,
    pub sprite_limit: bool,
    /// Cut the Master System's leftmost eight pixels off the picture.
    pub hide_left: bool,
    /// How much of the last picture each one keeps, percent (0 off, 25 weak, 50 normal).
    pub gg_ghosting: u8,
    /// A Game Gear shows the whole 256-pixel picture, not its 160 x 144 window.
    pub gg_full: bool,
    /// Each FM channel's volume, percent.
    pub fm_chan_vol: [u8; 9],
    /// A low-pass filter's corner, Hz (0: none).
    pub lowpass: u16,
    pub volume: u8,
    pub chan_vol: [u8; 4],
    pub mono: bool,
    pub opposing: bool,
    /// The TMS9918 modes' colours: None for Auto (the console's own), or the Master System's.
    pub tms_colours_sms: Option<bool>,
    /// 3-D glasses games: None shows both eyes' pictures, Some(true) only the left's.
    pub glasses_eye: Option<bool>,
    /// What is plugged into each controller port.
    pub ports: [crate::peripherals::Device; 2],
    pub crosshair: bool,
    pub crosshair_square: bool,
    pub crosshair_colour: u32,
    /// Moves the Light Phaser's aim, pixels.
    pub phaser_offset: (i32, i32),
    pub device_speed: u8,
    /// Two Game Gears on one phone (link.rs): on, side by side, which screens (0 both, 1 the
    /// first, 2 the second), swapped, which is heard (0 the first, 1 the second, 2 both), and
    /// whether the player's controls play the second.
    pub gg_link: bool,
    pub gg_link_side: bool,
    pub gg_link_show: u8,
    pub gg_link_swap: bool,
    pub gg_link_audio: u8,
    pub gg_link_controls_second: bool,
    /// The equaliser: bass, middle and treble, percent.
    pub eq: [u8; 3],
    /// The TV border: 0 none, 1 above and below, 2 all round.
    pub borders: u8,
    /// The CPU's speed, percent.
    pub cpu_percent: u32,
    /// The FM chip: None for Auto (a Japanese Master System), or forced off or on.
    pub fm: Option<bool>,
    pub fm_volume: u8,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            console: Console::Auto, japan: false, region_auto: true, refresh_auto: false, pal: false, mapper: None, sprite_limit: true, hide_left: false,
            gg_ghosting: 0, gg_full: false, fm_chan_vol: [100; 9], lowpass: 0, volume: 100, chan_vol: [100; 4], mono: false, opposing: false, fm: None, fm_volume: 100, cpu_percent: 100, tms_colours_sms: None, borders: 0, glasses_eye: None, eq: [100; 3], ports: [crate::peripherals::Device::Pad; 2], crosshair: true, crosshair_square: false, crosshair_colour: 0xFFFFFF, phaser_offset: (0, 0), device_speed: 5, gg_link: false, gg_link_side: true, gg_link_show: 0, gg_link_swap: false, gg_link_audio: 0, gg_link_controls_second: false,
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
    s.region_auto = get(c"cgear_region").is_none_or(|v| v == "auto");
    s.refresh_auto = get(c"cgear_refresh").is_some_and(|v| v == "auto");
    s.pal = get(c"cgear_refresh").is_some_and(|v| v == "50");
    s.mapper = match get(c"cgear_mapper").as_deref() {
        Some("sega") => Some(Mapper::Sega),
        Some("codemasters") => Some(Mapper::Codemasters),
        Some("korean") => Some(Mapper::Korean),
        Some("msx") => Some(Mapper::Msx),
        Some("nemesis") => Some(Mapper::Nemesis),
        Some("janggun") => Some(Mapper::Janggun),
        _ => None,
    };
    s.sprite_limit = on(c"cgear_sprite_limit", true);
    s.hide_left = on(c"cgear_hide_left", false);
    s.gg_ghosting = match get(c"cgear_gg_ghosting").as_deref() {
        Some("weak") => 25,
        Some("on") => 50,
        _ => 0,
    };
    s.gg_full = on(c"cgear_gg_full", false);
    let fm_keys = [c"cgear_vol_fm1", c"cgear_vol_fm2", c"cgear_vol_fm3", c"cgear_vol_fm4", c"cgear_vol_fm5", c"cgear_vol_fm6", c"cgear_vol_fm7", c"cgear_vol_fm8", c"cgear_vol_fm9"];
    s.fm_chan_vol = fm_keys.map(|k| pct(k));
    s.lowpass = match get(c"cgear_lowpass").as_deref() {
        Some("light") => 8000,
        Some("medium") => 5000,
        Some("strong") => 3000,
        _ => 0,
    };
    s.volume = pct(c"cgear_volume");
    s.chan_vol = [pct(c"cgear_vol_tone1"), pct(c"cgear_vol_tone2"), pct(c"cgear_vol_tone3"), pct(c"cgear_vol_noise")];
    s.mono = on(c"cgear_mono", false);
    s.opposing = on(c"cgear_opposing", false);
    s.fm = match get(c"cgear_fm").as_deref() {
        Some("off") => Some(false),
        Some("on") => Some(true),
        _ => None,
    };
    s.fm_volume = pct(c"cgear_fm_volume");
    s.tms_colours_sms = match get(c"cgear_tms_colours").as_deref() {
        Some("sms") => Some(true),
        Some("sg") => Some(false),
        _ => None,
    };
    s.glasses_eye = match get(c"cgear_3d").as_deref() {
        Some("left") => Some(true),
        Some("right") => Some(false),
        _ => None,
    };
    let device = |k: &CStr| {
        use crate::peripherals::Device;
        match get(k).as_deref() {
            Some("phaser") => Device::Phaser,
            Some("paddle") => Device::Paddle,
            Some("sports") => Device::SportsPad,
            _ => Device::Pad,
        }
    };
    s.ports = [device(c"cgear_port1"), device(c"cgear_port2")];
    s.crosshair = on(c"cgear_crosshair", true);
    s.crosshair_square = get(c"cgear_crosshair_shape").is_some_and(|v| v == "square");
    s.crosshair_colour = match get(c"cgear_crosshair_colour").as_deref() {
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
    s.phaser_offset = (offset(c"cgear_phaser_x"), offset(c"cgear_phaser_y"));
    s.device_speed = get(c"cgear_device_speed").and_then(|v| v.parse::<u8>().ok()).unwrap_or(5).clamp(1, 10);
    s.gg_link = on(c"cgear_gg_link", false);
    s.gg_link_side = get(c"cgear_gg_link_layout").is_none_or(|v| v != "stacked");
    s.gg_link_show = match get(c"cgear_gg_link_show").as_deref() {
        Some("1") => 1,
        Some("2") => 2,
        _ => 0,
    };
    s.gg_link_swap = on(c"cgear_gg_link_swap", false);
    s.gg_link_audio = match get(c"cgear_gg_link_audio").as_deref() {
        Some("2") => 1,
        Some("both") => 2,
        _ => 0,
    };
    s.gg_link_controls_second = get(c"cgear_gg_link_controls").is_some_and(|v| v == "2");
    let band = |k: &CStr| get(k).and_then(|v| v.parse::<u8>().ok()).unwrap_or(100).min(150);
    s.eq = [band(c"cgear_eq_low"), band(c"cgear_eq_mid"), band(c"cgear_eq_high")];
    s.borders = match get(c"cgear_borders").as_deref() {
        Some("tb") => 1,
        Some("full") => 2,
        _ => 0,
    };
    s.cpu_percent = get(c"cgear_cpu_speed").and_then(|v| v.parse::<u32>().ok()).unwrap_or(100).clamp(100, 300);
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
