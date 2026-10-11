//! cBoy's core options (libretro SET_CORE_OPTIONS_V2), and the output-side effects they drive.
//! Everything here works on the finished frame and the mixed samples, so it applies live and
//! never touches emulation, save states or test hashes.

use std::ffi::{c_char, c_void, CStr};

#[repr(C)]
pub struct OptValue {
    pub value: *const c_char,
    pub label: *const c_char,
}

#[repr(C)]
pub struct OptCategory {
    pub key: *const c_char,
    pub desc: *const c_char,
    pub info: *const c_char,
}

#[repr(C)]
pub struct OptDefinition {
    pub key: *const c_char,
    pub desc: *const c_char,
    pub desc_categorized: *const c_char,
    pub info: *const c_char,
    pub info_categorized: *const c_char,
    pub category_key: *const c_char,
    pub values: [OptValue; 128],
    pub default_value: *const c_char,
}

#[repr(C)]
pub struct OptionsV2 {
    pub categories: *const OptCategory,
    pub definitions: *const OptDefinition,
}

#[repr(C)]
pub struct Variable {
    pub key: *const c_char,
    pub value: *const c_char,
}

pub const ENV_GET_VARIABLE: u32 = 15;
pub const ENV_GET_VARIABLE_UPDATE: u32 = 17;
pub const ENV_SET_CORE_OPTIONS_V2: u32 = 67;

const NULL_VALUE: OptValue = OptValue { value: std::ptr::null(), label: std::ptr::null() };

/// (key, description, info, [(value, label)], default)
type Spec = (&'static CStr, &'static CStr, &'static CStr, &'static [(&'static CStr, &'static CStr)], &'static CStr);

const SPECS: &[Spec] = &[
    (c"cboy_dmg_palette", c"Game Boy palette",
     c"Colours for original Game Boy games (not Color or Advance ones).",
     &[(c"grayscale", c"Grayscale"), (c"green", c"Original green (DMG)"), (c"pocket", c"Game Boy Pocket"),
       (c"light", c"Game Boy Light"), (c"kiosk", c"Kiosk"), (c"sepia", c"Sepia")],
     c"grayscale"),
    (c"cboy_gbc_color_correction", c"Game Boy Color colour correction",
     c"Mutes the colours the way the Color's own screen showed them.",
     &[(c"off", c"Off"), (c"on", c"On")], c"off"),
    (c"cboy_gba_color_correction", c"Game Boy Advance colour correction",
     c"Off shows raw colours; On mimics the Advance's darker, washed-out LCD.",
     &[(c"off", c"Off"), (c"on", c"On (Advance LCD)")], c"off"),
    (c"cboy_frame_blend", c"LCD ghosting (frame blending)",
     c"Blends each frame with the last, like the slow LCD. Makes flicker effects look solid.",
     &[(c"off", c"Off"), (c"on", c"On")], c"off"),
    (c"cboy_volume", c"Volume",
     c"Overall sound volume.",
     &[(c"100", c"100%"), (c"75", c"75%"), (c"50", c"50%"), (c"25", c"25%"), (c"0", c"Muted")], c"100"),
    (c"cboy_rtc", c"Cartridge clock (RTC)",
     c"Real time keeps a cartridge clock (Pokemon Gold, Silver, Crystal) running while the game is closed. Game time only counts while playing.",
     &[(c"real", c"Real time"), (c"game", c"Game time only")], c"real"),
];

/// Declares the options to the frontend. Called from retro_set_environment.
pub unsafe fn declare(env: unsafe extern "C" fn(u32, *mut c_void) -> bool) {
    // Built once and leaked: the frontend may keep the pointers.
    let defs: &'static mut Vec<OptDefinition> = Box::leak(Box::new(Vec::new()));
    for (key, desc, info, values, default) in SPECS {
        let mut vals = [NULL_VALUE; 128];
        for (i, (v, l)) in values.iter().enumerate() {
            vals[i] = OptValue { value: v.as_ptr(), label: l.as_ptr() };
        }
        defs.push(OptDefinition {
            key: key.as_ptr(), desc: desc.as_ptr(), desc_categorized: std::ptr::null(),
            info: info.as_ptr(), info_categorized: std::ptr::null(), category_key: std::ptr::null(),
            values: vals, default_value: default.as_ptr(),
        });
    }
    defs.push(OptDefinition {
        key: std::ptr::null(), desc: std::ptr::null(), desc_categorized: std::ptr::null(),
        info: std::ptr::null(), info_categorized: std::ptr::null(), category_key: std::ptr::null(),
        values: [NULL_VALUE; 128], default_value: std::ptr::null(),
    });
    let cats: &'static [OptCategory; 1] = Box::leak(Box::new([OptCategory {
        key: std::ptr::null(), desc: std::ptr::null(), info: std::ptr::null(),
    }]));
    let mut opts = OptionsV2 { categories: cats.as_ptr(), definitions: defs.as_ptr() };
    env(ENV_SET_CORE_OPTIONS_V2, &mut opts as *mut OptionsV2 as *mut c_void);
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub dmg_palette: [u32; 4],
    pub gbc_correct: bool,
    pub gba_correct: bool,
    pub blend: bool,
    pub volume: u32,
    pub real_time_rtc: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { dmg_palette: palette("grayscale"), gbc_correct: false, gba_correct: false,
                   blend: false, volume: 100, real_time_rtc: true }
    }
}

pub fn palette(name: &str) -> [u32; 4] {
    match name {
        "green" => [0x9BBC0F, 0x8BAC0F, 0x306230, 0x0F380F],
        "pocket" => [0xC4CFA1, 0x8B956D, 0x4D533C, 0x1F1F1F],
        "light" => [0x00B581, 0x009A71, 0x00694A, 0x004F3B],
        "kiosk" => [0xFFE78C, 0xDEB55A, 0x9C7B39, 0x4A3918],
        "sepia" => [0xFFF6D3, 0xF9A875, 0xEB6B6F, 0x7C3F58],
        _ => crate::ppu::SHADES,
    }
}

unsafe fn get(env: unsafe extern "C" fn(u32, *mut c_void) -> bool, key: &CStr) -> Option<String> {
    let mut var = Variable { key: key.as_ptr(), value: std::ptr::null() };
    if env(ENV_GET_VARIABLE, &mut var as *mut Variable as *mut c_void) && !var.value.is_null() {
        Some(CStr::from_ptr(var.value).to_string_lossy().into_owned())
    } else {
        None
    }
}

/// Reads every option (missing ones keep their default).
pub unsafe fn read(env: unsafe extern "C" fn(u32, *mut c_void) -> bool) -> Settings {
    let mut s = Settings::default();
    if let Some(v) = get(env, c"cboy_dmg_palette") { s.dmg_palette = palette(&v); }
    if let Some(v) = get(env, c"cboy_gbc_color_correction") { s.gbc_correct = v == "on"; }
    if let Some(v) = get(env, c"cboy_gba_color_correction") { s.gba_correct = v == "on"; }
    if let Some(v) = get(env, c"cboy_frame_blend") { s.blend = v == "on"; }
    if let Some(v) = get(env, c"cboy_volume") { s.volume = v.parse().unwrap_or(100).min(100); }
    if let Some(v) = get(env, c"cboy_rtc") { s.real_time_rtc = v != "game"; }
    s
}

pub unsafe fn changed(env: unsafe extern "C" fn(u32, *mut c_void) -> bool) -> bool {
    let mut updated = false;
    env(ENV_GET_VARIABLE_UPDATE, &mut updated as *mut bool as *mut c_void) && updated
}

/// Colour correction: a mix of the channels and a gamma lift, the usual approximation of the
/// GBC (`strong` false) and GBA (`strong` true) screens.
fn correct(c: u32, strong: bool) -> u32 {
    let (r, g, b) = (((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0);
    let gamma = if strong { 1.25 } else { 1.1 };
    let (r, g, b) = (r.powf(gamma), g.powf(gamma), b.powf(gamma));
    let (nr, ng, nb) = if strong {
        (0.80 * r + 0.275 * g - 0.075 * b, 0.135 * r + 0.64 * g + 0.225 * b, 0.195 * r + 0.155 * g + 0.65 * b)
    } else {
        (0.82 * r + 0.24 * g - 0.06 * b, 0.10 * r + 0.70 * g + 0.20 * b, 0.12 * r + 0.18 * g + 0.70 * b)
    };
    let ch = |v: f32| (v.clamp(0.0, 1.0).powf(1.0 / gamma) * 255.0).round() as u32;
    ch(nr) << 16 | ch(ng) << 8 | ch(nb)
}

/// Applies the picture options to a finished frame, in place. `prev` holds the last output for
/// blending and is updated.
pub fn apply_video(frame: &mut [u32], prev: &mut Vec<u32>, s: &Settings, machine: Machine) {
    match machine {
        Machine::Dmg if s.dmg_palette != crate::ppu::SHADES => {
            for p in frame.iter_mut() {
                if let Some(i) = crate::ppu::SHADES.iter().position(|&x| x == *p & 0xFFFFFF) {
                    *p = s.dmg_palette[i];
                }
            }
        }
        Machine::Cgb if s.gbc_correct => frame.iter_mut().for_each(|p| *p = correct(*p, false)),
        Machine::Gba if s.gba_correct => frame.iter_mut().for_each(|p| *p = correct(*p, true)),
        _ => {}
    }
    if s.blend {
        if prev.len() == frame.len() {
            for (p, q) in frame.iter_mut().zip(prev.iter()) {
                let avg = |a: u32, b: u32, sh: u32| (((a >> sh) & 255) + ((b >> sh) & 255)) / 2 << sh;
                let cur = *p;
                *p = avg(cur, *q, 16) | avg(cur, *q, 8) | avg(cur, *q, 0);
                // Ghosting follows the shown picture, so keep the blended value.
            }
        }
        prev.clear();
        prev.extend_from_slice(frame);
    } else {
        prev.clear();
    }
}

pub fn apply_volume(samples: &mut [i16], s: &Settings) {
    if s.volume < 100 {
        for x in samples.iter_mut() {
            *x = (i32::from(*x) * s.volume as i32 / 100) as i16;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Machine {
    Dmg,
    Cgb,
    Gba,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dmg_palette_maps_the_four_shades_and_blend_averages() {
        let s = Settings { dmg_palette: palette("green"), ..Settings::default() };
        let mut f = crate::ppu::SHADES.to_vec();
        let mut prev = Vec::new();
        apply_video(&mut f, &mut prev, &s, Machine::Dmg);
        assert_eq!(f, palette("green").to_vec());
        let s = Settings { blend: true, ..Settings::default() };
        let mut a = vec![0x00FF_FFFFu32];
        apply_video(&mut a, &mut prev, &s, Machine::Cgb);
        let mut b = vec![0u32];
        apply_video(&mut b, &mut prev, &s, Machine::Cgb);
        assert_eq!(b[0], 0x007F_7F7F);
    }

    #[test]
    fn correction_keeps_black_and_white_and_volume_scales() {
        assert_eq!(correct(0, true), 0);
        let w = correct(0xFFFFFF, false);
        assert!((w >> 16) & 255 >= 0xF0 && w & 255 >= 0xF0, "{w:06X}");
        let mut x = [1000i16, -1000];
        apply_volume(&mut x, &Settings { volume: 50, ..Settings::default() });
        assert_eq!(x, [500, -500]);
    }
}
