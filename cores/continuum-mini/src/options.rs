//! cMini settings. The first thirteen match PokeMini's current controls; BIOS and infrared are
//! hardware-backed additions.

use std::ffi::{c_char, c_void, CStr};
#[repr(C)]
struct Value {
    value: *const c_char,
    label: *const c_char,
}
#[repr(C)]
struct Category {
    key: *const c_char,
    desc: *const c_char,
    info: *const c_char,
}
#[repr(C)]
struct Definition {
    key: *const c_char,
    desc: *const c_char,
    desc_categorized: *const c_char,
    info: *const c_char,
    info_categorized: *const c_char,
    category_key: *const c_char,
    values: [Value; 128],
    default_value: *const c_char,
}
#[repr(C)]
struct Options {
    categories: *const Category,
    definitions: *const Definition,
}
#[repr(C)]
struct Variable {
    key: *const c_char,
    value: *const c_char,
}
pub type Env = unsafe extern "C" fn(u32, *mut c_void) -> bool;
const SET_V2: u32 = 67;
const GET: u32 = 15;
const UPDATED: u32 = 17;
type Values = &'static [(&'static CStr, &'static CStr)];
type Spec = (
    &'static CStr,
    &'static CStr,
    &'static CStr,
    &'static CStr,
    Values,
    &'static CStr,
);
const CATS: &[(&CStr, &CStr, &CStr)] = &[
    (
        c"system",
        c"System",
        c"How the console starts and connects.",
    ),
    (c"picture", c"Picture", c"The Pokémon Mini LCD."),
    (c"sound", c"Sound", c"The piezo speaker."),
    (c"controls", c"Controls", c"Shake, rumble and turbo."),
];
const ONOFF: Values = &[(c"on", c"On"), (c"off", c"Off")];
const OFFON: Values = &[(c"off", c"Off"), (c"on", c"On")];
const SPECS: &[Spec] = &[
    (
        c"picture",
        c"cmini_video_scale",
        c"Render scale",
        c"Keeps the dot pattern sharp before the app sizes the picture.",
        &[
            (c"1", c"1×"),
            (c"2", c"2×"),
            (c"3", c"3×"),
            (c"4", c"4×"),
            (c"5", c"5×"),
            (c"6", c"6×"),
        ],
        c"1",
    ),
    (
        c"picture",
        c"cmini_60hz",
        c"60 Hz display",
        c"Drops every sixth picture for a 60 Hz display.",
        OFFON,
        c"off",
    ),
    (
        c"picture",
        c"cmini_lcd_filter",
        c"LCD filter",
        c"Dot matrix recreates the screen's visible cells.",
        &[
            (c"dot", c"Dot matrix"),
            (c"scan", c"Scanlines"),
            (c"none", c"None"),
        ],
        c"dot",
    ),
    (
        c"picture",
        c"cmini_lcd_mode",
        c"LCD shades",
        c"Analog keeps the real screen's fading pixels.",
        &[
            (c"analog", c"Analog"),
            (c"three", c"3 shades"),
            (c"two", c"2 shades"),
        ],
        c"analog",
    ),
    (
        c"picture",
        c"cmini_contrast",
        c"Contrast",
        c"",
        &[
            (c"0", c"0"),
            (c"16", c"16"),
            (c"32", c"32"),
            (c"48", c"48"),
            (c"64", c"64"),
            (c"80", c"80"),
            (c"96", c"96"),
        ],
        c"64",
    ),
    (
        c"picture",
        c"cmini_brightness",
        c"Brightness",
        c"",
        &[
            (c"-80", c"-80"),
            (c"-40", c"-40"),
            (c"-20", c"-20"),
            (c"0", c"0"),
            (c"20", c"+20"),
            (c"40", c"+40"),
            (c"80", c"+80"),
        ],
        c"0",
    ),
    (
        c"picture",
        c"cmini_palette",
        c"LCD colour",
        c"Default follows the original yellow-green screen.",
        &[
            (c"default", c"Default"),
            (c"mono", c"Black and white"),
            (c"green", c"Green"),
            (c"blue", c"Blue"),
            (c"red", c"Red"),
            (c"sepia", c"Sepia"),
            (c"pink", c"Pink"),
        ],
        c"default",
    ),
    (
        c"sound",
        c"cmini_piezo",
        c"Piezo speaker",
        c"Recreates the small speaker's response.",
        ONOFF,
        c"on",
    ),
    (
        c"sound",
        c"cmini_lowpass",
        c"Soften sound",
        c"Takes away high-frequency harshness.",
        OFFON,
        c"off",
    ),
    (
        c"sound",
        c"cmini_lowpass_level",
        c"Softening strength",
        c"",
        &[
            (c"20", c"20%"),
            (c"40", c"40%"),
            (c"60", c"60%"),
            (c"80", c"80%"),
            (c"95", c"95%"),
        ],
        c"60",
    ),
    (
        c"controls",
        c"cmini_shake",
        c"Screen shake",
        c"How far the picture moves when the game runs the motor.",
        &[
            (c"0", c"Off"),
            (c"1", c"Low"),
            (c"2", c"Medium"),
            (c"3", c"Full"),
        ],
        c"3",
    ),
    (
        c"controls",
        c"cmini_rumble",
        c"Rumble strength",
        c"",
        &[
            (c"0", c"Off"),
            (c"3", c"30%"),
            (c"5", c"50%"),
            (c"7", c"70%"),
            (c"10", c"100%"),
        ],
        c"10",
    ),
    (
        c"controls",
        c"cmini_turbo",
        c"Turbo A period",
        c"Frames between repeated A presses.",
        &[
            (c"4", c"4"),
            (c"8", c"8"),
            (c"12", c"12"),
            (c"18", c"18"),
            (c"24", c"24"),
            (c"36", c"36"),
            (c"60", c"60"),
            (c"120", c"120"),
        ],
        c"18",
    ),
    (
        c"system",
        c"cmini_bios",
        c"Real console start-up",
        c"Uses your bios.min. Off starts the game directly.",
        OFFON,
        c"off",
    ),
    (
        c"system",
        c"cmini_infrared",
        c"Infrared link",
        c"Carries the console's infrared bits over Continuum's two-phone link.",
        OFFON,
        c"off",
    ),
];
#[derive(Clone, Debug)]
pub struct Settings {
    pub scale: u8,
    pub hz60: bool,
    pub filter: u8,
    pub shades: u8,
    pub contrast: i16,
    pub brightness: i16,
    pub palette: u8,
    pub piezo: bool,
    pub lowpass: u8,
    pub shake: u8,
    pub rumble: u8,
    pub turbo: u8,
    pub real_bios: bool,
    pub infrared: bool,
    pub sample_rate: u32,
    pub volume: u8,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            scale: 1,
            hz60: false,
            filter: 1,
            shades: 3,
            contrast: 64,
            brightness: 0,
            palette: 0,
            piezo: true,
            lowpass: 0,
            shake: 3,
            rumble: 10,
            turbo: 18,
            real_bios: false,
            infrared: false,
            sample_rate: 44_100,
            volume: 100,
        }
    }
}
pub unsafe fn declare(env: Env) {
    let mut cats: Vec<Category> = CATS
        .iter()
        .map(|(a, b, c)| Category {
            key: a.as_ptr(),
            desc: b.as_ptr(),
            info: c.as_ptr(),
        })
        .collect();
    cats.push(Category {
        key: std::ptr::null(),
        desc: std::ptr::null(),
        info: std::ptr::null(),
    });
    let mut defs = Vec::new();
    for (cat, key, label, info, vals, default) in SPECS {
        let mut values: [Value; 128] = std::array::from_fn(|_| Value {
            value: std::ptr::null(),
            label: std::ptr::null(),
        });
        for (i, (v, l)) in vals.iter().enumerate() {
            values[i] = Value {
                value: v.as_ptr(),
                label: l.as_ptr(),
            }
        }
        defs.push(Definition {
            key: key.as_ptr(),
            desc: label.as_ptr(),
            desc_categorized: std::ptr::null(),
            info: info.as_ptr(),
            info_categorized: std::ptr::null(),
            category_key: cat.as_ptr(),
            values,
            default_value: default.as_ptr(),
        })
    }
    defs.push(Definition {
        key: std::ptr::null(),
        desc: std::ptr::null(),
        desc_categorized: std::ptr::null(),
        info: std::ptr::null(),
        info_categorized: std::ptr::null(),
        category_key: std::ptr::null(),
        values: std::array::from_fn(|_| Value {
            value: std::ptr::null(),
            label: std::ptr::null(),
        }),
        default_value: std::ptr::null(),
    });
    let o = Options {
        categories: cats.as_ptr(),
        definitions: defs.as_ptr(),
    };
    env(SET_V2, &o as *const _ as *mut c_void);
    std::mem::forget(cats);
    std::mem::forget(defs)
}
unsafe fn get(env: Env, key: &CStr) -> Option<String> {
    let mut v = Variable {
        key: key.as_ptr(),
        value: std::ptr::null(),
    };
    if env(GET, &mut v as *mut _ as *mut c_void) && !v.value.is_null() {
        Some(CStr::from_ptr(v.value).to_string_lossy().into_owned())
    } else {
        None
    }
}
pub unsafe fn read(env: Env) -> Settings {
    let mut s = Settings::default();
    let n = |k: &CStr, d: i16| get(env, k).and_then(|v| v.parse().ok()).unwrap_or(d);
    let on = |k: &CStr, d: bool| get(env, k).map_or(d, |v| v == "on");
    s.scale = n(c"cmini_video_scale", 1).clamp(1, 6) as u8;
    s.hz60 = on(c"cmini_60hz", false);
    s.filter = match get(env, c"cmini_lcd_filter").as_deref() {
        Some("none") => 0,
        Some("scan") => 2,
        _ => 1,
    };
    s.shades = match get(env, c"cmini_lcd_mode").as_deref() {
        Some("two") => 2,
        Some("three") => 3,
        _ => 4,
    };
    s.contrast = n(c"cmini_contrast", 64);
    s.brightness = n(c"cmini_brightness", 0);
    s.palette = match get(env, c"cmini_palette").as_deref() {
        Some("mono") => 1,
        Some("green") => 2,
        Some("blue") => 3,
        Some("red") => 4,
        Some("sepia") => 5,
        Some("pink") => 6,
        _ => 0,
    };
    s.piezo = on(c"cmini_piezo", true);
    s.lowpass = if on(c"cmini_lowpass", false) {
        n(c"cmini_lowpass_level", 60) as u8
    } else {
        0
    };
    s.shake = n(c"cmini_shake", 3) as u8;
    s.rumble = n(c"cmini_rumble", 10) as u8;
    s.turbo = n(c"cmini_turbo", 18) as u8;
    s.real_bios = on(c"cmini_bios", false);
    s.infrared = on(c"cmini_infrared", false);
    s
}
pub unsafe fn changed(env: Env) -> bool {
    let mut b = false;
    env(UPDATED, &mut b as *mut _ as *mut c_void);
    b
}
