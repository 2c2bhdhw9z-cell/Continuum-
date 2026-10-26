//! cBoy's core options (libretro SET_CORE_OPTIONS_V2, categories per
//! .kiro/notes/docs/CORE_OPTIONS_STANDARD.md), and the output-side effects they drive (palettes, colour,
//! blending, Super Game Boy colour and border, filters, volume). Output effects work on the
//! finished frame and samples, so they apply live and never touch save states.

use std::ffi::{c_char, c_void, CStr};

use crate::gb::Model;
use crate::gba::gpio::Hardware;
use crate::gba::save::SaveKind;

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
pub const ENV_SET_VARIABLES: u32 = 16;
pub const ENV_GET_VARIABLE_UPDATE: u32 = 17;
pub const ENV_SET_CORE_OPTIONS_DISPLAY: u32 = 55;
pub const ENV_SET_CORE_OPTIONS_V2: u32 = 67;
pub const ENV_SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK: u32 = 69;

/// `struct retro_core_option_display`: one key, and whether the settings page shows it.
#[repr(C)]
pub struct OptDisplay {
    pub key: *const c_char,
    pub visible: bool,
}

/// `struct retro_core_options_update_display_callback`.
#[repr(C)]
pub struct UpdateDisplayCallback {
    pub callback: Option<unsafe extern "C" fn() -> bool>,
}

const NULL_VALUE: OptValue = OptValue { value: std::ptr::null(), label: std::ptr::null() };

type Values = &'static [(&'static CStr, &'static CStr)];
/// (category, key, label, info, values, default)
type Spec = (&'static CStr, &'static CStr, &'static CStr, &'static CStr, Values, &'static CStr);

const CATEGORIES: &[(&CStr, &CStr, &CStr)] = &[
    (c"system", c"System", c"Which console, start-up files, the cartridge's clock and save chip."),
    (c"video", c"Picture", c"Palettes, screen colour, LCD ghosting and the Super Game Boy border."),
    (c"audio", c"Sound", c"Volume, channel mix and filters."),
    (c"input", c"Controls", c"Turbo buttons, motion and light sensors, rumble."),
    (c"link", c"Link", c"The link cable, and two Game Boys on one phone."),
    (c"performance", c"Performance", c"Speed and button delay."),
    (c"debug", c"Debug", c"Switch picture layers off one at a time."),
];

const PCT: Values = &[(c"100", c"100%"), (c"75", c"75%"), (c"50", c"50%"), (c"25", c"25%"), (c"0", c"Off")];
const ON_OFF: Values = &[(c"on", c"On"), (c"off", c"Off")];
const OFF_ON: Values = &[(c"off", c"Off"), (c"on", c"On")];

const SPECS: &[Spec] = &[
    // System
    (c"system", c"cboy_gb_model", c"Console for Game Boy and Color games",
     c"Auto picks the right console for each game; changes apply at the next start.",
     &[(c"auto", c"Auto"), (c"auto_sgb", c"Auto (Super Game Boy for SGB games)"), (c"dmg", c"Game Boy"), (c"dmg0", c"Game Boy (first revision)"), (c"mgb", c"Game Boy Pocket"), (c"sgb", c"Super Game Boy"),
       (c"sgb2", c"Super Game Boy 2"), (c"sgb_pal", c"Super Game Boy (PAL)"), (c"cgb", c"Game Boy Color"), (c"agb", c"Game Boy Advance (Color games)")],
     c"auto"),
    (c"system", c"cboy_cgb_revision", c"Which Game Boy Color",
     c"Only a handful of games can tell them apart; changes apply at the next start.",
     &[(c"e", c"The usual one"), (c"0", c"The first Japanese one")],
     c"e"),
    (c"system", c"cboy_hybrid_model", c"Console for games that run on both",
     c"Only used when the console above is on Auto; changes apply at the next start.",
     &[(c"cgb", c"Game Boy Color"), (c"dmg", c"Game Boy"), (c"sgb", c"Super Game Boy"), (c"agb", c"Game Boy Advance")],
     c"cgb"),
    (c"system", c"cboy_newer_console", c"Let games see a newer console",
     c"Some games add extras on a Pocket or an Advance; only used with the console on Auto, from the next start.",
     &[(c"off", c"Off"), (c"pocket", c"Pocket and Super Game Boy 2"), (c"advance", c"Advance (Color games)"), (c"both", c"Both")],
     c"off"),
    (c"system", c"cboy_sgb_speed", c"Super Game Boy speed",
     c"A real one runs a little fast, sound and all, unlike the Super Game Boy 2; changes apply at the next start.",
     &[(c"real", c"Real (a little fast)"), (c"normal", c"Same as a Game Boy")], c"real"),
    (c"system", c"cboy_gbp_reset", c"Game Boy Player reset",
     c"With the console on Advance, a reset fades out and pauses for a moment the way a Game Boy Player does.",
     OFF_ON, c"off"),
    (c"system", c"cboy_gba_model", c"Advance console",
     c"Only Game Boy Player is different, letting games made for it shake the phone; changes apply at the next start.",
     &[(c"gba", c"Game Boy Advance"), (c"sp", c"Game Boy Advance SP"), (c"gbp", c"Game Boy Player")], c"gba"),
    (c"system", c"cboy_gba_clock", c"Advance cartridge clock",
     c"In Advance games with a clock, like Pokemon Ruby, Game time only moves while you play.",
     &[(c"phone", c"Phone clock"), (c"game", c"Game time only")], c"phone"),
    (c"system", c"cboy_vba_compat", c"Old fan-made game fixes",
     c"On fixes fan-made Advance games built on an old emulator; other games play the same either way.",
     ON_OFF, c"on"),
    (c"system", c"cboy_force_color", c"Run Game Boy games as Color",
     c"Only for fan-made colour hacks, as normal Game Boy games may look wrong; changes apply at the next start.",
     OFF_ON, c"off"),
    (c"system", c"cboy_boot_rom", c"Start from a boot ROM file",
     c"Plays the real start-up logo from your own boot file, added with Install a BIOS in Settings.",
     OFF_ON, c"off"),
    (c"system", c"cboy_gba_bios", c"Game Boy Advance BIOS",
     c"Built-in needs no file, and the other two use your own gba_bios.bin, added with Install a BIOS in Settings.",
     &[(c"builtin", c"Built-in"), (c"file", c"From file"), (c"file_skip", c"From file, no logo")], c"builtin"),
    (c"system", c"cboy_gba_save_type", c"Game Boy Advance save chip",
     c"Leave it on Auto unless a game won't save; changes apply at the next start.",
     &[(c"auto", c"Auto"), (c"sram", c"SRAM 32 KB"), (c"flash64", c"Flash 64 KB"), (c"flash128", c"Flash 128 KB"),
       (c"eeprom512", c"EEPROM 512 B"), (c"eeprom8k", c"EEPROM 8 KB"), (c"none", c"None")], c"auto"),
    (c"system", c"cboy_gba_hardware", c"Game Boy Advance cartridge extras",
     c"Auto knows which games have a clock, sensor or rumble built in; changes apply at the next start.",
     &[(c"auto", c"Auto"), (c"none", c"None"), (c"rtc", c"Clock"), (c"solar", c"Solar sensor"),
       (c"gyro", c"Gyro sensor"), (c"tilt", c"Tilt sensor"), (c"rumble", c"Rumble")], c"auto"),
    (c"system", c"cboy_gbp", c"Game Boy Player rumble",
     c"Lets Advance games made for the Game Boy Player shake the phone, from the next start; leave it off for link-cable games.",
     OFF_ON, c"off"),
    (c"system", c"cboy_rtc", c"Cartridge clock",
     c"In Game Boy and Color games with a clock, like Pokemon Gold, Real time keeps ticking while the game is closed.",
     &[(c"real", c"Real time"), (c"game", c"Game time only"), (c"phone", c"Phone clock")], c"real"),
    (c"system", c"cboy_rtc_offset", c"Clock offset",
     c"Shifts the time a game gets from the phone, for Advance clocks and Phone clock above.",
     &[(c"0", c"None"), (c"1", c"+1 hour"), (c"2", c"+2 hours"), (c"3", c"+3 hours"), (c"6", c"+6 hours"),
       (c"12", c"+12 hours"), (c"24", c"+1 day"), (c"-1", c"-1 hour"), (c"-2", c"-2 hours"), (c"-3", c"-3 hours"),
       (c"-6", c"-6 hours"), (c"-12", c"-12 hours"), (c"-24", c"-1 day")], c"0"),
    // Picture
    (c"video", c"cboy_dmg_palette", c"Game Boy palette",
     c"For Custom, write four colours, lightest first, in a palette.txt and add it with Install a BIOS in Settings.",
     &[(c"grayscale", c"Grayscale"), (c"green", c"Original green"), (c"pocket", c"Pocket"),
       (c"auto", c"Automatic (Pocket colours on a Pocket)"), (c"light", c"Light (backlit)"),
       (c"kiosk", c"Kiosk"), (c"sepia", c"Sepia"), (c"ice", c"Ice"), (c"custom", c"Custom")], c"grayscale"),
    (c"video", c"cboy_dmg_colorize", c"Colour for black-and-white games",
     c"Automatic and the ones with buttons need the console on Auto or a Color, and change at the next start.",
     &[(c"off", c"Off (like a Game Boy)"), (c"gbc", c"Automatic (like a Game Boy Color)"),
       (c"gbc_up", c"Brown (Up)"), (c"gbc_up_a", c"Red (Up + A)"), (c"gbc_up_b", c"Dark brown (Up + B)"),
       (c"gbc_left", c"Blue (Left)"), (c"gbc_left_a", c"Dark blue (Left + A)"), (c"gbc_left_b", c"Grey (Left + B)"),
       (c"gbc_down", c"Pastel (Down)"), (c"gbc_down_a", c"Orange (Down + A)"), (c"gbc_down_b", c"Yellow (Down + B)"),
       (c"gbc_right", c"Green (Right)"), (c"gbc_right_a", c"Dark green (Right + A)"), (c"gbc_right_b", c"Reversed (Right + B)"),
       (c"auto", c"cBoy's pick"), (c"meadow", c"Meadow"), (c"ocean", c"Ocean"), (c"ember", c"Ember"),
       (c"dusk", c"Dusk"), (c"candy", c"Candy"), (c"slate", c"Slate")], c"off"),
    (c"video", c"cboy_gbc_color", c"Game Boy Color screen colour",
     c"Screen looks like a real Color's screen, and Soft is a lighter version.",
     &[(c"off", c"Off"), (c"screen", c"Screen"), (c"soft", c"Soft")], c"off"),
    (c"video", c"cboy_gbc_light", c"Game Boy Color light position",
     c"With a Color screen colour on, light from above looks brighter and washed out, and from below darker.",
     &[(c"central", c"Central"), (c"above", c"Above the screen"), (c"below", c"Below the screen")], c"central"),
    (c"video", c"cboy_gba_color", c"Game Boy Advance screen colour",
     c"Advance is the original unlit screen, SP the lit one, and TV a Game Boy Player on a television.",
     &[(c"off", c"Off"), (c"gba", c"Advance"), (c"sp", c"Advance SP"), (c"tv", c"TV")], c"off"),
    (c"video", c"cboy_blend", c"LCD ghosting",
     c"Mix makes flickering things look solid, and Ghosting leaves trails like the slow old screens.",
     &[(c"off", c"Off"), (c"mix", c"Mix"), (c"light", c"Ghosting, light"), (c"heavy", c"Ghosting, heavy")], c"off"),
    (c"video", c"cboy_sgb_border", c"Super Game Boy border",
     c"Needs a Super Game Boy console, except Always, which frames every Game Boy game; takes effect when the game restarts.",
     &[(c"on", c"Super Game Boy games"), (c"off", c"Off"), (c"always", c"Always")], c"on"),
    (c"video", c"cboy_renderer", c"Picture drawing",
     c"Exact is for rare games that change the picture halfway along a line, but isn't finished yet and may look wrong.",
     &[(c"standard", c"Standard"), (c"exact", c"Exact (experimental)")], c"standard"),
    (c"video", c"cboy_color_temp", c"Colour temperature", c"",
     &[(c"neutral", c"Neutral"), (c"warm", c"Warm"), (c"cool", c"Cool")], c"neutral"),
    (c"video", c"cboy_contrast", c"Contrast", c"",
     &[(c"normal", c"Normal"), (c"boost", c"Higher"), (c"reduce", c"Lower")], c"normal"),
    (c"video", c"cboy_dark_filter", c"Darken", c"",
     &[(c"0", c"Off"), (c"10", c"10%"), (c"20", c"20%"), (c"30", c"30%"), (c"40", c"40%"), (c"50", c"50%")], c"0"),
    (c"video", c"cboy_sprite_limit", c"Sprite flicker",
     c"Off stops busy scenes flickering by drawing everything, which a real Game Boy couldn't.",
     ON_OFF, c"on"),
    (c"video", c"cboy_gba_sprite_limit", c"Advance sprite flicker",
     c"On drops things from very busy scenes like a real Advance did, and Off draws everything.",
     OFF_ON, c"off"),
    // Sound
    (c"audio", c"cboy_volume", c"Volume", c"", PCT, c"100"),
    (c"audio", c"cboy_vol_sq1", c"Pulse channel 1", c"", PCT, c"100"),
    (c"audio", c"cboy_vol_sq2", c"Pulse channel 2", c"", PCT, c"100"),
    (c"audio", c"cboy_vol_wave", c"Wave channel", c"", PCT, c"100"),
    (c"audio", c"cboy_vol_noise", c"Noise channel", c"", PCT, c"100"),
    (c"audio", c"cboy_vol_fifo_a", c"Advance recorded sound A",
     c"Carries an Advance game's drums, voices and most of its music.", PCT, c"100"),
    (c"audio", c"cboy_vol_fifo_b", c"Advance recorded sound B", c"", PCT, c"100"),
    (c"audio", c"cboy_lowpass", c"Treble cut",
     c"Softens harsh high notes, like a small speaker.",
     &[(c"off", c"Off"), (c"light", c"Light"), (c"medium", c"Medium"), (c"strong", c"Strong")], c"off"),
    (c"audio", c"cboy_highpass", c"Sound centring",
     c"Gentle keeps more deep bass, and Off leaves the sound raw, hum and all.",
     &[(c"on", c"Like the console"), (c"gentle", c"Gentle"), (c"off", c"Off")], c"on"),
    (c"audio", c"cboy_audio_quality", c"Sound quality",
     c"High sounds smoother, and Fast is easier on the phone.",
     &[(c"fast", c"Fast"), (c"high", c"High")], c"fast"),
    (c"audio", c"cboy_gba_music", c"Advance music quality",
     c"High plays most Advance games' music at full quality, without the hiss and crunch.",
     &[(c"console", c"Console (as it was)"), (c"high", c"High")], c"console"),
    (c"audio", c"cboy_gba_smooth", c"Smooth Advance samples",
     c"Makes Advance sound less gritty but a little duller.",
     OFF_ON, c"off"),
    (c"audio", c"cboy_interference", c"LCD buzz",
     c"Adds the faint buzz a real Game Boy's screen puts into its sound.",
     &[(c"0", c"Off"), (c"25", c"Faint"), (c"50", c"Medium"), (c"100", c"Loud")], c"0"),
    (c"audio", c"cboy_sample_rate", c"Output sample rate",
     c"Lower rates are easier on the phone; takes effect when the game restarts.",
     &[(c"48000", c"48000 Hz"), (c"44100", c"44100 Hz"), (c"32768", c"32768 Hz")], c"48000"),
    // Controls
    (c"input", c"cboy_turbo", c"Turbo buttons",
     c"X and Y become rapid-fire A and B, at this many presses a second.",
     &[(c"off", c"Off"), (c"30", c"30"), (c"20", c"20"), (c"15", c"15"), (c"10", c"10"), (c"7", c"7.5")], c"off"),
    (c"input", c"cboy_rumble", c"Rumble", c"How hard the phone or controller shakes when a game rumbles.",
     &[(c"100", c"100%"), (c"75", c"75%"), (c"50", c"50%"), (c"25", c"25%"), (c"off", c"Off")], c"100"),
    (c"input", c"cboy_motion_sensitivity", c"Motion sensitivity",
     c"How strongly tilt and turn games react to moving the phone or stick.",
     &[(c"100", c"Normal"), (c"50", c"Half"), (c"75", c"Lower"), (c"150", c"Higher"), (c"200", c"Double")], c"100"),
    (c"input", c"cboy_opposing", c"Allow opposite directions",
     c"Some games glitch when left and right are held together, which a real pad can't do.",
     OFF_ON, c"off"),
    (c"input", c"cboy_stick_dpad", c"Left stick as D-pad", c"",
     OFF_ON, c"off"),
    (c"input", c"cboy_stick_deadzone", c"Stick dead zone",
     c"How far the stick has to move before it counts.",
     &[(c"15", c"15%"), (c"0", c"0%"), (c"5", c"5%"), (c"10", c"10%"), (c"20", c"20%"), (c"25", c"25%"), (c"30", c"30%")], c"15"),
    (c"input", c"cboy_swap_stick", c"Use the right stick",
     c"The right stick takes over from the left in these settings.",
     OFF_ON, c"off"),
    (c"input", c"cboy_tilt_source", c"Tilt input",
     c"For tilt games like Kirby Tilt 'n' Tumble and Yoshi Topsy-Turvy.",
     &[(c"phone", c"Phone motion"), (c"stick", c"Left stick"), (c"off", c"Off")], c"phone"),
    (c"input", c"cboy_gyro_source", c"Turn input",
     c"For WarioWare: Twisted!",
     &[(c"phone", c"Phone rotation"), (c"stick", c"Left stick"), (c"off", c"Off")], c"phone"),
    (c"input", c"cboy_solar", c"Sunlight",
     c"How much sun Boktai sees; Light sensor uses the phone's, or half sun when there's none to read.",
     &[(c"50", c"50%"), (c"phone", c"Light sensor"), (c"0", c"Dark"), (c"25", c"25%"), (c"75", c"75%"), (c"100", c"Full sun")], c"50"),
    // Link
    (c"link", c"cboy_link", c"Link cable",
     c"Loopback is for test programs; to link two phones, choose Link cable to a second phone in a game's ... menu.",
     &[(c"off", c"Unplugged"), (c"loopback", c"Loopback")], c"off"),
    (c"link", c"cboy_gba_link_port", c"Advance link port",
     c"Pokemon FireRed, LeafGreen and Emerald talk to the other phone through a Wireless Adapter here.",
     &[(c"auto", c"Auto (a Wireless Adapter for games that look for one)"), (c"cable", c"Link cable only")], c"auto"),
    (c"link", c"cboy_dual", c"Two Game Boys on this phone",
     c"Two consoles play this game, joined by a link cable, each with its own save; changes apply at the next start.",
     OFF_ON, c"off"),
    (c"link", c"cboy_dual_layout", c"Two Game Boys: where the screens go",
     c"Changes apply at the next start.",
     &[(c"stacked", c"One above the other"), (c"side", c"Side by side")], c"stacked"),
    (c"link", c"cboy_dual_show", c"Two Game Boys: screens shown",
     c"Between both screens and one, changes apply at the next start; between the two single screens, straight away.",
     &[(c"both", c"Both"), (c"1", c"The first console's only"), (c"2", c"The second console's only")], c"both"),
    (c"link", c"cboy_dual_swap", c"Two Game Boys: swap the screens",
     c"Puts the second console's screen first.",
     OFF_ON, c"off"),
    (c"link", c"cboy_dual_audio", c"Two Game Boys: sound",
     c"Which console you hear.",
     &[(c"1", c"The first console"), (c"2", c"The second console"), (c"both", c"Both together")], c"1"),
    (c"link", c"cboy_dual_controls", c"Two Game Boys: your controls play",
     c"The on-screen buttons and the first controller play this console; a second controller plays the other one.",
     &[(c"1", c"The first console"), (c"2", c"The second console")], c"1"),
    // Performance
    (c"performance", c"cboy_frameskip", c"Frame skip",
     c"Draws fewer frames to save work, while the game keeps full speed.",
     &[(c"off", c"Off"), (c"auto", c"Auto (when the phone falls behind)"), (c"1", c"Every other frame"), (c"2", c"Two in three"), (c"3", c"Three in four")], c"off"),
    (c"performance", c"cboy_frameskip_threshold", c"Auto frame skip threshold",
     c"With Auto frame skip on, a lower number skips frames sooner.",
     &[(c"85", c"85%"), (c"50", c"50%"), (c"60", c"60%"), (c"70", c"70%"), (c"80", c"80%"), (c"90", c"90%"), (c"95", c"95%")], c"85"),
    (c"performance", c"cboy_frameskip_interval", c"Auto frame skip limit",
     c"With Auto frame skip on, the most frames skipped in a row.",
     &[(c"1", c"1"), (c"2", c"2"), (c"3", c"3"), (c"4", c"4"), (c"6", c"6"), (c"9", c"9")], c"1"),
    (c"performance", c"cboy_run_ahead", c"Run-ahead",
     c"Cuts a game's built-in button delay, but costs the phone two or three times the work.",
     &[(c"0", c"Off"), (c"1", c"1 frame"), (c"2", c"2 frames")], c"0"),
    (c"performance", c"cboy_idle_skip", c"Skip idle loops (Advance)",
     c"Saves battery in Advance games, though a few may run at the wrong speed.",
     OFF_ON, c"off"),
    (c"performance", c"cboy_gb_idle_skip", c"Skip waiting loops (Game Boy and Color)",
     c"Saves a little battery in games that wait in a loop, without changing anything they do.",
     OFF_ON, c"off"),
    // Debug. Every option in this category is hidden from the settings page (see `hide_debug`).
    (c"debug", c"cboy_layer_bg", c"Background", c"For Game Boy and Color games.", ON_OFF, c"on"),
    (c"debug", c"cboy_layer_window", c"Window", c"For Game Boy and Color games.", ON_OFF, c"on"),
    (c"debug", c"cboy_layer_obj", c"Sprites", c"For Game Boy, Color and Advance games.", ON_OFF, c"on"),
    (c"debug", c"cboy_layer_bg0", c"Advance background 0", c"", ON_OFF, c"on"),
    (c"debug", c"cboy_layer_bg1", c"Advance background 1", c"", ON_OFF, c"on"),
    (c"debug", c"cboy_layer_bg2", c"Advance background 2", c"", ON_OFF, c"on"),
    (c"debug", c"cboy_layer_bg3", c"Advance background 3", c"", ON_OFF, c"on"),
    (c"debug", c"cboy_layer_win0", c"Advance window 0", c"Off draws as if window 0 were switched off.", ON_OFF, c"on"),
    (c"debug", c"cboy_layer_win1", c"Advance window 1", c"Off draws as if window 1 were switched off.", ON_OFF, c"on"),
    (c"debug", c"cboy_layer_objwin", c"Advance object window", c"Off draws as if the object window were switched off.", ON_OFF, c"on"),
];

/// Every key, for tests and for the fallback table.
pub fn keys() -> impl Iterator<Item = &'static CStr> {
    SPECS.iter().map(|s| s.1)
}

/// Declares the options to the frontend. Called from retro_set_environment.
pub unsafe fn declare(env: unsafe extern "C" fn(u32, *mut c_void) -> bool) {
    // Built once and leaked: the frontend may keep the pointers.
    let defs: &'static mut Vec<OptDefinition> = Box::leak(Box::new(Vec::new()));
    for (cat, key, desc, info, values, default) in SPECS {
        let mut vals = [NULL_VALUE; 128];
        for (i, (v, l)) in values.iter().enumerate() {
            vals[i] = OptValue { value: v.as_ptr(), label: l.as_ptr() };
        }
        defs.push(OptDefinition {
            key: key.as_ptr(), desc: desc.as_ptr(), desc_categorized: desc.as_ptr(),
            info: info.as_ptr(), info_categorized: info.as_ptr(), category_key: cat.as_ptr(),
            values: vals, default_value: default.as_ptr(),
        });
    }
    defs.push(OptDefinition {
        key: std::ptr::null(), desc: std::ptr::null(), desc_categorized: std::ptr::null(),
        info: std::ptr::null(), info_categorized: std::ptr::null(), category_key: std::ptr::null(),
        values: [NULL_VALUE; 128], default_value: std::ptr::null(),
    });
    let cats: &'static mut Vec<OptCategory> = Box::leak(Box::new(Vec::new()));
    for (k, d, i) in CATEGORIES {
        cats.push(OptCategory { key: k.as_ptr(), desc: d.as_ptr(), info: i.as_ptr() });
    }
    cats.push(OptCategory { key: std::ptr::null(), desc: std::ptr::null(), info: std::ptr::null() });
    let mut opts = OptionsV2 { categories: cats.as_ptr(), definitions: defs.as_ptr() };
    if env(ENV_SET_CORE_OPTIONS_V2, &mut opts as *mut OptionsV2 as *mut c_void) {
        // After the table, never before: declaring it clears what the frontend was hiding.
        if let Ok(mut slot) = ENV.lock() {
            *slot = Some(env);
        }
        hide_debug(env);
        let mut cb = UpdateDisplayCallback { callback: Some(refresh_display) };
        env(ENV_SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK, &mut cb as *mut UpdateDisplayCallback as *mut c_void);
        return;
    }
    // Older frontends: "Label; default|other|...".
    let texts: &'static mut Vec<std::ffi::CString> = Box::leak(Box::new(Vec::new()));
    let vars: &'static mut Vec<Variable> = Box::leak(Box::new(Vec::new()));
    for (_, key, desc, _, values, default) in SPECS {
        let mut list = vec![default.to_string_lossy().into_owned()];
        for (v, _) in values.iter() {
            if v != default {
                list.push(v.to_string_lossy().into_owned());
            }
        }
        let t = std::ffi::CString::new(format!("{}; {}", desc.to_string_lossy(), list.join("|"))).unwrap_or_default();
        texts.push(t);
        vars.push(Variable { key: key.as_ptr(), value: texts.last().map_or(std::ptr::null(), |t| t.as_ptr()) });
    }
    vars.push(Variable { key: std::ptr::null(), value: std::ptr::null() });
    env(ENV_SET_VARIABLES, vars.as_mut_ptr() as *mut c_void);
}

/// The frontend's environment call, kept so the display callback can answer it.
static ENV: std::sync::Mutex<Option<unsafe extern "C" fn(u32, *mut c_void) -> bool>> =
    std::sync::Mutex::new(None);

/// Keeps the layer switches out of the settings page. They are for finding a drawing problem, and
/// a player who turns one off just gets a game that looks broken, so they are not shown with the
/// ordinary settings; the app has its own switch for seeing what a core is hiding.
unsafe fn hide_debug(env: unsafe extern "C" fn(u32, *mut c_void) -> bool) {
    for (cat, key, ..) in SPECS {
        if *cat == c"debug" {
            let mut d = OptDisplay { key: key.as_ptr(), visible: false };
            env(ENV_SET_CORE_OPTIONS_DISPLAY, &mut d as *mut OptDisplay as *mut c_void);
        }
    }
}

/// Asked by the frontend, before it lists the settings, which ones to show right now.
unsafe extern "C" fn refresh_display() -> bool {
    let Some(env) = ENV.lock().ok().and_then(|e| *e) else { return false };
    hide_debug(env);
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Phone,
    Stick,
    Off,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub model: Model,
    /// The console a cartridge that runs on both a Game Boy and a Color gets, when the model is Auto.
    pub hybrid_model: Model,
    /// Which Advance an Advance game is told it is in.
    pub gba_model: GbaModel,
    /// Count the Advance cartridge clock from play time instead of the phone's clock.
    pub gba_game_clock: bool,
    /// Keep the Advance BIOS's unpacking inside the size the data asks for (old fan-made games).
    pub vba_compat: bool,
    pub boot_rom: bool,
    pub gba_bios_file: bool,
    pub gba_save: Option<(SaveKind, u8)>,
    pub gba_hardware: Option<Hardware>,
    pub rtc_mode: RtcMode,
    pub rtc_offset_hours: i64,
    pub dmg_palette: [u32; 4],
    pub custom_palette: bool,
    pub colorize: Colorize,
    pub gbc_color: u8,
    pub gba_color: u8,
    pub blend: u8,
    pub sgb_border: bool,
    pub volume: u32,
    pub chan_vol: [u8; 4],
    pub fifo_vol: [u8; 2],
    pub lowpass: u8,
    pub highpass: bool,
    pub gba_smooth: bool,
    /// Play the music an Advance game mixes in software again at the output rate, instead of
    /// taking the game's own low-quality mix ("Advance music quality").
    pub gba_music_hq: bool,
    pub sample_rate: u32,
    /// Turbo period in frames (0 off).
    pub turbo_period: u32,
    pub rumble: bool,
    pub tilt: Source,
    pub gyro: Source,
    /// None: the phone's light sensor; Some(level 0-255).
    pub solar: Option<u8>,
    pub link_loopback: bool,
    pub frameskip: u32,
    pub run_ahead: u32,
    pub idle_skip: bool,
    /// Skip the Game Boy's own waiting loops (exact: see `gb::WaitLoop`).
    pub gb_idle_skip: bool,
    /// Let games see a Pocket (and a Super Game Boy 2) instead of a Game Boy (and a Super Game
    /// Boy), with the console on Auto.
    pub newer_pocket: bool,
    /// Let Color games see an Advance, with the console on Auto.
    pub newer_advance: bool,
    /// The Pocket's colours when the console is a Pocket, Grayscale otherwise.
    pub palette_auto: bool,
    /// A Super Game Boy (not the 2) runs from the SNES clock, a little fast.
    pub sgb_fast: bool,
    /// A reset on the Advance console fades out and pauses like a Game Boy Player.
    pub gbp_reset: bool,
    pub gb_layers: u8,
    pub gba_layers: u8,
    pub gba_bios_skip_logo: bool,
    /// Border for every Game Boy picture, not just Super Game Boy ones.
    pub border_always: bool,
    pub color_temp: u8,
    pub contrast: u8,
    pub dark: u8,
    /// High-pass cutoff in Hz (0 off).
    pub highpass_hz: f32,
    pub audio_hq: bool,
    /// 0-100.
    pub rumble_strength: u8,
    pub motion_scale: f32,
    pub opposing: bool,
    pub stick_dpad: bool,
    pub sprite_limit: bool,
    pub sgb_pal: bool,
    /// Auto, but Super Game Boy for cartridges that support it.
    pub auto_sgb: bool,
    /// Model Game Boy Color revision 0.
    pub cgb_rev0: bool,
    /// The Color revision that draws the picture: revision 0 draws as a C, "the usual one" is an E.
    pub cgb_rev: crate::ppu::CgbRev,
    pub stick_deadzone: f32,
    pub swap_stick: bool,
    pub frameskip_threshold: u32,
    pub frameskip_interval: u32,
    pub gba_obj_limit: bool,
    pub force_color: bool,
    /// Answer the Game Boy Player so its rumble bit reaches the phone.
    pub gbp: bool,
    /// Game Boy Color light: -1 below, 0 central, 1 above.
    pub gbc_light: i8,
    pub frameskip_auto: bool,
    pub interference: u8,
    /// "Picture drawing": the exact (experimental) pixel-by-pixel drawer instead of the standard one.
    pub exact_renderer: bool,
    /// A game that looks for an Advance Wireless Adapter finds one (`gba/rfu.rs`).
    pub gba_wireless: bool,
    /// Two Game Boys on this phone, joined by a cable (`dual.rs`).
    pub dual: bool,
    /// Their screens side by side instead of one above the other.
    pub dual_side_by_side: bool,
    pub dual_show: Which,
    /// The second console's screen first.
    pub dual_swap: bool,
    pub dual_audio: Which,
    /// The on-screen buttons and the first controller play the second console.
    pub dual_controls_second: bool,
}

/// Of two Game Boys on one phone: the first, the second, or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    First,
    Second,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtcMode {
    Real,
    Game,
    Phone,
}

/// The Advance a game is told it is running in. The Advance and the SP are the same machine to
/// the game; the Game Boy Player answers its own handshake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GbaModel {
    Gba,
    Sp,
    Gbp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colorize {
    Off,
    /// cBoy's own scheme for the title (`auto_scheme`), drawn over the shades.
    Auto,
    /// One of cBoy's own `SCHEMES`.
    Scheme(usize),
    /// A Game Boy Color's colours: the ones its start-up code picks for the game, or one of the
    /// twelve a player picks with the buttons (`compat::PICKS`). These play a black-and-white
    /// game on a Color, which draws it in them itself.
    Gbc(Option<usize>),
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            model: Model::Auto, hybrid_model: Model::Cgb, gba_model: GbaModel::Gba, gba_game_clock: false,
            vba_compat: true, boot_rom: false, gba_bios_file: false, gba_save: None, gba_hardware: None,
            rtc_mode: RtcMode::Real, rtc_offset_hours: 0, dmg_palette: crate::ppu::SHADES, custom_palette: false,
            colorize: Colorize::Off, gbc_color: 0, gba_color: 0, blend: 0, sgb_border: true, volume: 100,
            chan_vol: [100; 4], fifo_vol: [100; 2], lowpass: 0, highpass: true, gba_smooth: false,
            gba_music_hq: false,
            sample_rate: 48_000, turbo_period: 0, rumble: true, tilt: Source::Phone, gyro: Source::Phone,
            solar: Some(128), link_loopback: false, frameskip: 0, run_ahead: 0, idle_skip: false, gb_idle_skip: false,
            newer_pocket: false, newer_advance: false, palette_auto: false, sgb_fast: true, gbp_reset: false,
            gb_layers: 7, gba_layers: 0xFF, gba_bios_skip_logo: false, border_always: false, color_temp: 0,
            contrast: 0, dark: 0, highpass_hz: 28.0, audio_hq: false, rumble_strength: 100, motion_scale: 1.0,
            opposing: false, stick_dpad: false, sprite_limit: true, sgb_pal: false, auto_sgb: false, cgb_rev0: false, cgb_rev: crate::ppu::CgbRev::E, stick_deadzone: 0.15, swap_stick: false,
            frameskip_threshold: 85, frameskip_interval: 1, gba_obj_limit: false, force_color: false, gbp: false, gbc_light: 0, frameskip_auto: false, interference: 0,
            exact_renderer: false,
            gba_wireless: true,
            dual: false, dual_side_by_side: false, dual_show: Which::Both, dual_swap: false, dual_audio: Which::First,
            dual_controls_second: false,
        }
    }
}

impl Settings {
    /// Which console runs this cartridge: the chosen model, or for Auto the one the cartridge
    /// asks for. A cartridge that runs on both a Game Boy and a Color (header $143 = $80) gets
    /// the "runs on both" setting; a Color-only one ($C0) always gets a Color.
    pub fn console_for(&self, cgb_flag: u8, sgb_cart: bool) -> Model {
        let colour = cgb_flag & 0x80 != 0;
        let both = cgb_flag & 0xC0 == 0x80;
        match self.model {
            Model::Auto => {
                let m = if both {
                    self.hybrid_model
                } else if colour {
                    Model::Cgb
                } else if self.auto_sgb && sgb_cart {
                    Model::Sgb
                } else if self.gbc_colours() {
                    // A black-and-white game the player wants in a Game Boy Color's colours goes
                    // on the console that has them.
                    Model::Cgb
                } else {
                    Model::Dmg
                };
                // A newer console, for the games that look for one and add extras on it.
                match m {
                    Model::Dmg if self.newer_pocket => Model::Mgb,
                    Model::Sgb if self.newer_pocket => Model::Sgb2,
                    Model::Cgb if self.newer_advance => Model::Agb,
                    m => m,
                }
            }
            m => m,
        }
    }

    /// The player wants black-and-white games in a Game Boy Color's colours.
    pub fn gbc_colours(&self) -> bool {
        matches!(self.colorize, Colorize::Gbc(_))
    }

    /// Answer the Game Boy Player handshake: the Player model, or the rumble option on its own.
    pub fn player_handshake(&self) -> bool {
        self.gbp || self.gba_model == GbaModel::Gbp
    }
}

/// The sunlight a cartridge with a sun sensor in it sees: the level the player set, or the
/// phone's own light reading turned into 0-255 (full sun saturates well before daylight levels).
///
/// `lux` is None when there is no light reading to be had, which is every iPhone today: the app
/// turns the request down, and a sensor that always reads nothing left Boktai in the pitch dark
/// and unplayable. With nothing to read, half sun it is.
pub fn sunlight(level: Option<u8>, lux: Option<f32>) -> u8 {
    match (level, lux) {
        (Some(level), _) => level,
        (None, Some(lux)) => (lux / 4000.0 * 255.0).clamp(0.0, 255.0) as u8,
        (None, None) => 128,
    }
}

pub fn palette(name: &str) -> [u32; 4] {
    match name {
        "green" => [0x9BBC0F, 0x8BAC0F, 0x306230, 0x0F380F],
        "pocket" => [0xC4CFA1, 0x8B956D, 0x4D533C, 0x1F1F1F],
        "light" => [0x00B581, 0x009A71, 0x00694A, 0x004F3B],
        "kiosk" => [0xFFE78C, 0xDEB55A, 0x9C7B39, 0x4A3918],
        "sepia" => [0xFFF6D3, 0xF9A875, 0xEB6B6F, 0x7C3F58],
        "ice" => [0xE8F8FF, 0x9FD3F0, 0x4F7FB8, 0x1A2850],
        _ => crate::ppu::SHADES,
    }
}

/// The option values for a Game Boy Color's twelve button schemes, in the order of
/// `compat::PICKS`: Up, Up + A, Up + B, Left, Left + A, Left + B, Down, Down + A, Down + B, Right,
/// Right + A, Right + B.
const GBC_PICKS: [&str; 12] = ["gbc_up", "gbc_up_a", "gbc_up_b", "gbc_left", "gbc_left_a", "gbc_left_b",
    "gbc_down", "gbc_down_a", "gbc_down_b", "gbc_right", "gbc_right_a", "gbc_right_b"];

/// Colour schemes for Game Boy games: (background, sprite palette 0, sprite palette 1).
pub const SCHEMES: [[[u32; 4]; 3]; 6] = [
    // meadow
    [[0xF8F8D8, 0x98D068, 0x487838, 0x182810], [0xFFFFFF, 0xF8A060, 0xA83818, 0x301008], [0xFFFFFF, 0x80B0F8, 0x3050A8, 0x101838]],
    // ocean
    [[0xE8F8F8, 0x78C8E8, 0x2868A8, 0x081838], [0xFFFFFF, 0xF8D878, 0xC07820, 0x402008], [0xFFFFFF, 0xF89898, 0xB83040, 0x401018]],
    // ember
    [[0xFFF0D8, 0xF8B068, 0xB85020, 0x401008], [0xFFFFFF, 0xF8E890, 0x988030, 0x302808], [0xFFFFFF, 0xA8D8F8, 0x4878B8, 0x102040]],
    // dusk
    [[0xF0E8F8, 0xB8A0D8, 0x685098, 0x201838], [0xFFFFFF, 0xF8C098, 0xC06848, 0x402018], [0xFFFFFF, 0x98E0B8, 0x388860, 0x082818]],
    // candy
    [[0xFFF0F8, 0xF8A8D0, 0xC04888, 0x401028], [0xFFFFFF, 0xA8E8F8, 0x3898C0, 0x083040], [0xFFFFFF, 0xF8F0A0, 0xB8A838, 0x383008]],
    // slate
    [[0xF0F0F0, 0xB0B8C0, 0x606870, 0x181C20], [0xFFFFFF, 0xF8C870, 0xB06820, 0x382008], [0xFFFFFF, 0x90D8F8, 0x3878B0, 0x0C2038]],
];

/// The scheme Auto picks for a title: a stable hash of the header title.
pub fn auto_scheme(title: &str) -> usize {
    let h = title.bytes().fold(2166136261u32, |h, b| (h ^ u32::from(b)).wrapping_mul(16777619));
    (h % SCHEMES.len() as u32) as usize
}

/// Reads a custom palette file: four hex colours, lightest first, separated by anything.
pub fn read_custom_palette(text: &str) -> Option<[u32; 4]> {
    let mut out = [0u32; 4];
    let mut n = 0;
    for word in text.split(|c: char| !c.is_ascii_hexdigit() && c != '#' && c != 'x') {
        let w = word.trim_start_matches('#').trim_start_matches("0x");
        if w.len() == 6 {
            if let Ok(v) = u32::from_str_radix(w, 16) {
                if n < 4 {
                    out[n] = v;
                    n += 1;
                }
            }
        }
    }
    (n == 4).then_some(out)
}

unsafe fn get(env: unsafe extern "C" fn(u32, *mut c_void) -> bool, key: &CStr) -> Option<String> {
    let mut var = Variable { key: key.as_ptr(), value: std::ptr::null() };
    if env(ENV_GET_VARIABLE, &mut var as *mut Variable as *mut c_void) && !var.value.is_null() {
        Some(CStr::from_ptr(var.value).to_string_lossy().into_owned())
    } else {
        None
    }
}

/// Settings from a lookup of option values (missing ones keep their default).
pub fn parse(get: impl Fn(&CStr) -> Option<String>, system_dir: Option<&str>) -> Settings {
    let mut s = Settings::default();
    let on = |k: &CStr, d: bool| get(k).map_or(d, |v| v == "on");
    let pct = |k: &CStr| get(k).and_then(|v| v.parse::<u8>().ok()).unwrap_or(100).min(100);
    if let Some(v) = get(c"cboy_gb_model") {
        s.model = match v.as_str() {
            "dmg" => Model::Dmg, "dmg0" => Model::Dmg0, "mgb" => Model::Mgb, "sgb" | "sgb_pal" => Model::Sgb, "sgb2" => Model::Sgb2,
            "cgb" => Model::Cgb, "agb" => Model::Agb, _ => Model::Auto,
        };
    }
    s.auto_sgb = get(c"cboy_gb_model").is_some_and(|v| v == "auto_sgb");
    s.hybrid_model = match get(c"cboy_hybrid_model").as_deref() {
        Some("dmg") => Model::Dmg, Some("sgb") => Model::Sgb, Some("agb") => Model::Agb, _ => Model::Cgb,
    };
    s.gba_model = match get(c"cboy_gba_model").as_deref() {
        Some("sp") => GbaModel::Sp, Some("gbp") => GbaModel::Gbp, _ => GbaModel::Gba,
    };
    s.gba_game_clock = get(c"cboy_gba_clock").is_some_and(|v| v == "game");
    s.vba_compat = on(c"cboy_vba_compat", true);
    s.cgb_rev0 = get(c"cboy_cgb_revision").is_some_and(|v| v == "0");
    s.cgb_rev = match get(c"cboy_cgb_revision").as_deref() {
        Some("0") => crate::ppu::CgbRev::C,
        _ => crate::ppu::CgbRev::E,
    };
    s.stick_deadzone = get(c"cboy_stick_deadzone").and_then(|v| v.parse::<f32>().ok()).unwrap_or(15.0).clamp(0.0, 50.0) / 100.0;
    s.swap_stick = on(c"cboy_swap_stick", false);
    s.frameskip_threshold = get(c"cboy_frameskip_threshold").and_then(|v| v.parse().ok()).unwrap_or(85).clamp(30, 100);
    s.frameskip_interval = get(c"cboy_frameskip_interval").and_then(|v| v.parse().ok()).unwrap_or(1).clamp(1, 9);
    s.gba_obj_limit = on(c"cboy_gba_sprite_limit", false);
    s.force_color = on(c"cboy_force_color", false);
    s.gbp = on(c"cboy_gbp", false);
    s.gbc_light = match get(c"cboy_gbc_light").as_deref() { Some("above") => 1, Some("below") => -1, _ => 0 };
    s.sgb_pal = get(c"cboy_gb_model").is_some_and(|v| v == "sgb_pal");
    let newer = get(c"cboy_newer_console");
    s.newer_pocket = newer.as_deref().is_some_and(|v| v == "pocket" || v == "both");
    s.newer_advance = newer.as_deref().is_some_and(|v| v == "advance" || v == "both");
    s.sgb_fast = get(c"cboy_sgb_speed").is_none_or(|v| v != "normal");
    s.gbp_reset = on(c"cboy_gbp_reset", false);
    s.boot_rom = on(c"cboy_boot_rom", false);
    s.gba_bios_file = get(c"cboy_gba_bios").is_some_and(|v| v == "file" || v == "file_skip");
    s.gba_bios_skip_logo = get(c"cboy_gba_bios").is_some_and(|v| v == "file_skip");
    s.gba_save = get(c"cboy_gba_save_type").and_then(|v| match v.as_str() {
        "sram" => Some((SaveKind::Sram, 0)), "flash64" => Some((SaveKind::Flash64, 0)),
        "flash128" => Some((SaveKind::Flash128, 0)), "eeprom512" => Some((SaveKind::Eeprom, 6)),
        "eeprom8k" => Some((SaveKind::Eeprom, 14)), "none" => Some((SaveKind::None, 0)), _ => None,
    });
    s.gba_hardware = get(c"cboy_gba_hardware").and_then(|v| match v.as_str() {
        "none" => Some(Hardware::None), "rtc" => Some(Hardware::Rtc), "solar" => Some(Hardware::Solar),
        "gyro" => Some(Hardware::Gyro), "tilt" => Some(Hardware::Tilt), "rumble" => Some(Hardware::Rumble), _ => None,
    });
    s.rtc_mode = match get(c"cboy_rtc").as_deref() { Some("game") => RtcMode::Game, Some("phone") => RtcMode::Phone, _ => RtcMode::Real };
    s.rtc_offset_hours = get(c"cboy_rtc_offset").and_then(|v| v.parse().ok()).unwrap_or(0);
    s.palette_auto = get(c"cboy_dmg_palette").is_some_and(|v| v == "auto");
    if let Some(v) = get(c"cboy_dmg_palette") {
        if v == "custom" {
            let file = system_dir.and_then(|d| std::fs::read_to_string(format!("{d}/cboy/palette.txt")).ok());
            if let Some(p) = file.as_deref().and_then(read_custom_palette) {
                s.dmg_palette = p;
                s.custom_palette = true;
            }
        } else {
            s.dmg_palette = palette(&v);
        }
    }
    s.colorize = match get(c"cboy_dmg_colorize").as_deref() {
        Some("auto") => Colorize::Auto,
        Some("meadow") => Colorize::Scheme(0), Some("ocean") => Colorize::Scheme(1), Some("ember") => Colorize::Scheme(2),
        Some("dusk") => Colorize::Scheme(3), Some("candy") => Colorize::Scheme(4), Some("slate") => Colorize::Scheme(5),
        Some("gbc") => Colorize::Gbc(None),
        // In the order of `compat::PICKS`.
        Some(v) => GBC_PICKS.iter().position(|k| *k == v).map_or(Colorize::Off, |i| Colorize::Gbc(Some(i))),
        None => Colorize::Off,
    };
    s.gbc_color = match get(c"cboy_gbc_color").as_deref() { Some("screen") => 1, Some("soft") => 2, _ => 0 };
    s.gba_color = match get(c"cboy_gba_color").as_deref() { Some("gba") => 1, Some("sp") => 2, Some("tv") => 3, _ => 0 };
    s.blend = match get(c"cboy_blend").as_deref() { Some("mix") => 1, Some("light") => 2, Some("heavy") => 3, _ => 0 };
    s.sgb_border = get(c"cboy_sgb_border").is_none_or(|v| v != "off");
    s.border_always = get(c"cboy_sgb_border").is_some_and(|v| v == "always");
    s.color_temp = match get(c"cboy_color_temp").as_deref() { Some("warm") => 1, Some("cool") => 2, _ => 0 };
    s.contrast = match get(c"cboy_contrast").as_deref() { Some("boost") => 1, Some("reduce") => 2, _ => 0 };
    s.dark = get(c"cboy_dark_filter").and_then(|v| v.parse().ok()).unwrap_or(0).min(50);
    s.volume = u32::from(pct(c"cboy_volume"));
    s.chan_vol = [pct(c"cboy_vol_sq1"), pct(c"cboy_vol_sq2"), pct(c"cboy_vol_wave"), pct(c"cboy_vol_noise")];
    s.fifo_vol = [pct(c"cboy_vol_fifo_a"), pct(c"cboy_vol_fifo_b")];
    s.lowpass = match get(c"cboy_lowpass").as_deref() { Some("light") => 1, Some("medium") => 2, Some("strong") => 3, _ => 0 };
    s.highpass_hz = match get(c"cboy_highpass").as_deref() { Some("off") => 0.0, Some("gentle") => 5.0, _ => 28.0 };
    s.highpass = s.highpass_hz > 0.0;
    s.audio_hq = get(c"cboy_audio_quality").is_some_and(|v| v == "high");
    s.gba_smooth = on(c"cboy_gba_smooth", false);
    s.gba_music_hq = get(c"cboy_gba_music").is_some_and(|v| v == "high");
    s.sample_rate = get(c"cboy_sample_rate").and_then(|v| v.parse().ok()).filter(|r| [48_000, 44_100, 32_768].contains(r)).unwrap_or(48_000);
    s.turbo_period = get(c"cboy_turbo").and_then(|v| v.parse::<u32>().ok()).map_or(0, |per_s| (60 / per_s.max(1)).max(2));
    s.rumble_strength = match get(c"cboy_rumble").as_deref() {
        Some("off") => 0,
        Some("on") | None => 100,
        Some(v) => v.parse::<u8>().unwrap_or(100).min(100),
    };
    s.rumble = s.rumble_strength > 0;
    s.motion_scale = get(c"cboy_motion_sensitivity").and_then(|v| v.parse::<f32>().ok()).unwrap_or(100.0).clamp(25.0, 400.0) / 100.0;
    s.opposing = on(c"cboy_opposing", false);
    s.stick_dpad = on(c"cboy_stick_dpad", false);
    s.sprite_limit = on(c"cboy_sprite_limit", true);
    let src = |k: &CStr| match get(k).as_deref() { Some("stick") => Source::Stick, Some("off") => Source::Off, _ => Source::Phone };
    s.tilt = src(c"cboy_tilt_source");
    s.gyro = src(c"cboy_gyro_source");
    s.solar = match get(c"cboy_solar").as_deref() {
        Some("phone") => None,
        Some(v) => Some((v.parse::<u32>().unwrap_or(50).min(100) * 255 / 100) as u8),
        None => Some(128),
    };
    s.link_loopback = get(c"cboy_link").is_some_and(|v| v == "loopback");
    s.frameskip = get(c"cboy_frameskip").and_then(|v| v.parse().ok()).unwrap_or(0).min(3);
    s.frameskip_auto = get(c"cboy_frameskip").is_some_and(|v| v == "auto");
    s.interference = get(c"cboy_interference").and_then(|v| v.parse().ok()).unwrap_or(0).min(100);
    s.run_ahead = get(c"cboy_run_ahead").and_then(|v| v.parse().ok()).unwrap_or(0).min(2);
    s.idle_skip = on(c"cboy_idle_skip", false);
    s.gb_idle_skip = on(c"cboy_gb_idle_skip", false);
    s.exact_renderer = get(c"cboy_renderer").is_some_and(|v| v == "exact");
    s.gba_wireless = !get(c"cboy_gba_link_port").is_some_and(|v| v == "cable");
    s.dual = on(c"cboy_dual", false);
    s.dual_side_by_side = get(c"cboy_dual_layout").is_some_and(|v| v == "side");
    let which = |k: &CStr, default: Which| match get(k).as_deref() {
        Some("1") => Which::First,
        Some("2") => Which::Second,
        Some("both") => Which::Both,
        _ => default,
    };
    s.dual_show = which(c"cboy_dual_show", Which::Both);
    s.dual_audio = which(c"cboy_dual_audio", Which::First);
    s.dual_swap = on(c"cboy_dual_swap", false);
    s.dual_controls_second = get(c"cboy_dual_controls").is_some_and(|v| v == "2");
    s.gb_layers = u8::from(on(c"cboy_layer_bg", true)) | u8::from(on(c"cboy_layer_window", true)) << 1
        | u8::from(on(c"cboy_layer_obj", true)) << 2;
    s.gba_layers = u8::from(on(c"cboy_layer_bg0", true)) | u8::from(on(c"cboy_layer_bg1", true)) << 1
        | u8::from(on(c"cboy_layer_bg2", true)) << 2 | u8::from(on(c"cboy_layer_bg3", true)) << 3
        | u8::from(on(c"cboy_layer_obj", true)) << 4 | u8::from(on(c"cboy_layer_win0", true)) << 5
        | u8::from(on(c"cboy_layer_win1", true)) << 6 | u8::from(on(c"cboy_layer_objwin", true)) << 7;
    s
}

/// Reads every option from the frontend.
pub unsafe fn read(env: unsafe extern "C" fn(u32, *mut c_void) -> bool, system_dir: Option<&str>) -> Settings {
    parse(|k| get(env, k), system_dir)
}

pub unsafe fn changed(env: unsafe extern "C" fn(u32, *mut c_void) -> bool) -> bool {
    let mut updated = false;
    env(ENV_GET_VARIABLE_UPDATE, &mut updated as *mut bool as *mut c_void) && updated
}

fn split(c: u32) -> (f32, f32, f32) {
    (((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0)
}

fn join(r: f32, g: f32, b: f32) -> u32 {
    let ch = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    ch(r) << 16 | ch(g) << 8 | ch(b)
}

/// The screen-colour looks. Each is a gamma into linear light, a channel mix (the panels bleed
/// colours into each other), a brightness and a gamma back out. Our own approximations.
pub fn correct(c: u32, look: Look) -> u32 {
    correct_lit(c, look, 0)
}

/// `correct` with the Game Boy Color light position: light from above washes the reflective
/// panel out (brighter, flatter), from below leaves it darker.
pub fn correct_lit(c: u32, look: Look, light: i8) -> u32 {
    let (gin, m, mut bright, mut gout): (f32, [[f32; 3]; 3], f32, f32) = match look {
        Look::GbcScreen => (2.2, [[0.80, 0.18, 0.02], [0.10, 0.74, 0.16], [0.12, 0.20, 0.68]], 0.94, 2.0),
        Look::GbcSoft => (2.2, [[0.88, 0.10, 0.02], [0.06, 0.84, 0.10], [0.06, 0.12, 0.82]], 1.0, 2.1),
        Look::Gba => (3.2, [[0.82, 0.24, -0.06], [0.12, 0.66, 0.22], [0.18, 0.16, 0.66]], 0.92, 2.2),
        Look::GbaSp => (2.5, [[0.86, 0.16, -0.02], [0.08, 0.76, 0.16], [0.10, 0.12, 0.78]], 1.0, 2.2),
        Look::Tv => (2.2, [[0.95, 0.05, 0.0], [0.03, 0.94, 0.03], [0.02, 0.05, 0.93]], 1.0, 2.4),
    };
    if matches!(look, Look::GbcScreen | Look::GbcSoft) {
        bright *= 1.0 + 0.06 * f32::from(light);
        gout += 0.15 * f32::from(light);
    }
    let (r, g, b) = split(c);
    let (r, g, b) = (r.powf(gin), g.powf(gin), b.powf(gin));
    let mix = |row: [f32; 3]| ((row[0] * r + row[1] * g + row[2] * b) * bright).max(0.0).powf(1.0 / gout);
    join(mix(m[0]), mix(m[1]), mix(m[2]))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    GbcScreen,
    GbcSoft,
    Gba,
    GbaSp,
    Tv,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Machine {
    Dmg,
    Cgb,
    Gba,
}

/// Colour temperature, contrast and darkening on the finished picture (all machines).
pub fn tone(frame: &mut [u32], s: &Settings) {
    if s.color_temp == 0 && s.contrast == 0 && s.dark == 0 {
        return;
    }
    let (tr, tg, tb) = match s.color_temp { 1 => (1.0, 0.95, 0.84), 2 => (0.88, 0.96, 1.0), _ => (1.0, 1.0, 1.0) };
    let k = match s.contrast { 1 => 1.25, 2 => 0.8, _ => 1.0 };
    let dim = 1.0 - f32::from(s.dark) / 100.0;
    for p in frame.iter_mut() {
        let (r, g, b) = split(*p);
        let c = |v: f32, t: f32| (((v - 0.5) * k + 0.5) * t * dim).clamp(0.0, 1.0);
        *p = join(c(r, tr), c(g, tg), c(b, tb));
    }
}

/// Output-side state that lives across frames.
#[derive(Default)]
pub struct Post {
    prev: Vec<u32>,
    lp: [f32; 2],
    hp_in: [f32; 2],
    hp_out: [f32; 2],
}

impl Post {
    /// Picture options on a finished 160x144 (or 240x160) frame. For the DMG, `shade` and
    /// `source` say what each pixel is (see `Ppu::shade`).
    pub fn video(&mut self, frame: &mut [u32], shade: &[u8], source: &[u8], s: &Settings, machine: Machine, title: &str) {
        match machine {
            Machine::Dmg => {
                let scheme = match s.colorize {
                    // A Color draws its own colours, so a picture that gets here is not one: the
                    // player picked another console, or the game has not restarted on a Color yet.
                    Colorize::Off | Colorize::Gbc(_) => None,
                    Colorize::Auto => Some(SCHEMES[auto_scheme(title)]),
                    Colorize::Scheme(i) => Some(SCHEMES[i % SCHEMES.len()]),
                };
                if shade.len() == frame.len() {
                    for i in 0..frame.len() {
                        let sh = usize::from(shade[i] & 3);
                        frame[i] = match scheme {
                            Some(sc) => sc[usize::from(source[i].min(2))][sh],
                            None => s.dmg_palette[sh],
                        };
                    }
                }
            }
            Machine::Cgb => {
                let look = match s.gbc_color { 1 => Some(Look::GbcScreen), 2 => Some(Look::GbcSoft), _ => None };
                if let Some(l) = look {
                    let light = s.gbc_light;
                    frame.iter_mut().for_each(|p| *p = correct_lit(*p, l, light));
                }
            }
            Machine::Gba => {
                let look = match s.gba_color { 1 => Some(Look::Gba), 2 => Some(Look::GbaSp), 3 => Some(Look::Tv), _ => None };
                if let Some(l) = look {
                    frame.iter_mut().for_each(|p| *p = correct(*p, l));
                }
            }
        }
        tone(frame, s);
        self.blend(frame, s.blend);
    }

    /// LCD ghosting: 1 mixes with the last frame shown; 2 and 3 leave decaying trails.
    pub fn blend(&mut self, frame: &mut [u32], mode: u8) {
        if mode == 0 {
            self.prev.clear();
            return;
        }
        let drawn = if mode == 1 { Some(frame.to_vec()) } else { None };
        if self.prev.len() == frame.len() {
            let keep = match mode { 1 => 0.5, 2 => 0.4, _ => 0.65 };
            for (p, q) in frame.iter_mut().zip(self.prev.iter()) {
                let (a, b) = (split(*p), split(*q));
                *p = join(a.0 * (1.0 - keep) + b.0 * keep, a.1 * (1.0 - keep) + b.1 * keep, a.2 * (1.0 - keep) + b.2 * keep);
            }
        }
        // A plain mix blends with the frame as drawn; ghosting with what was shown (trails).
        self.prev = drawn.unwrap_or_else(|| frame.to_vec());
    }

    /// Volume and filters on interleaved stereo samples.
    pub fn audio(&mut self, samples: &mut [i16], s: &Settings, rate: u32) {
        let cut = match s.lowpass { 1 => Some(9_000.0f32), 2 => Some(5_000.0), 3 => Some(2_500.0), _ => None };
        let a_lp = cut.map(|f| {
            let dt = 1.0 / rate as f32;
            let rc = 1.0 / (2.0 * std::f32::consts::PI * f);
            dt / (rc + dt)
        });
        // The console's coupling capacitor sits near 28 Hz; Gentle uses 5 Hz.
        let hz = if s.highpass_hz > 0.0 { s.highpass_hz } else { 28.0 };
        let a_hp = 1.0 - (2.0 * std::f32::consts::PI * hz / rate as f32);
        let vol = s.volume as f32 / 100.0;
        for (i, x) in samples.iter_mut().enumerate() {
            let ch = i & 1;
            let mut v = f32::from(*x);
            if s.highpass {
                let out = a_hp * (self.hp_out[ch] + v - self.hp_in[ch]);
                self.hp_in[ch] = v;
                self.hp_out[ch] = out;
                v = out;
            }
            if let Some(a) = a_lp {
                self.lp[ch] += a * (v - self.lp[ch]);
                v = self.lp[ch];
            }
            *x = (v * vol).clamp(-32768.0, 32767.0) as i16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn settings(pairs: &[(&str, &str)]) -> Settings {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        parse(|k| m.get(k.to_str().unwrap()).cloned(), None)
    }

    /// Descriptions are checked by `every_description_is_one_short_sentence_or_none`.
    #[test]
    fn every_option_has_a_unique_cboy_key_a_category_and_its_default_among_its_values() {
        let mut seen = std::collections::HashSet::new();
        for (cat, key, _, _, values, default) in SPECS {
            let k = key.to_str().unwrap();
            assert!(k.starts_with("cboy_"), "{k}");
            assert!(seen.insert(k), "duplicate {k}");
            assert!(CATEGORIES.iter().any(|c| c.0 == *cat), "{k}");
            assert!(values.iter().any(|v| v.0 == *default), "{k}");
        }
    }

    /// Every description is one short sentence a player can act on, or none where the label and
    /// its choices say it all (the owner, 13 October 2026: every explanation short and casual, or
    /// none when the control says it all). The ones with none are named here, so a description
    /// cannot quietly go missing, and none may grow back past about two lines on a phone.
    #[test]
    fn every_description_is_one_short_sentence_or_none() {
        // "Contrast: Normal, Higher, Lower", "Volume: 100%", "Left stick as D-pad: On".
        let none = [
            "cboy_color_temp", "cboy_contrast", "cboy_dark_filter", "cboy_volume", "cboy_vol_sq1", "cboy_vol_sq2",
            "cboy_vol_wave", "cboy_vol_noise", "cboy_vol_fifo_b", "cboy_stick_dpad",
            "cboy_layer_bg0", "cboy_layer_bg1", "cboy_layer_bg2", "cboy_layer_bg3",
        ];
        for (_, key, _, info, _, _) in SPECS {
            let (k, text) = (key.to_str().unwrap(), info.to_str().unwrap());
            if none.contains(&k) {
                assert!(text.is_empty(), "{k}: the label says it all, so it has no description");
                continue;
            }
            assert!(!text.is_empty(), "{k} has lost its description");
            let words = text.split_whitespace().count();
            assert!(words <= 22 && text.len() <= 125, "{k}: {words} words, {} letters: \"{text}\"", text.len());
            // One sentence: it ends once, and nothing inside it starts another.
            assert!(text.ends_with('.') || text.ends_with('!'), "{k}: \"{text}\"");
            let inside = &text[..text.len() - 1];
            for stop in [". ", "! ", "? "] {
                for (i, _) in inside.match_indices(stop) {
                    let next = inside[i + stop.len()..].chars().next();
                    assert!(!next.is_some_and(char::is_uppercase), "{k}: two sentences: \"{text}\"");
                }
            }
        }
    }

    /// The app reads one word out of this text. An option whose label or description says
    /// "restart" is held at what the game started with until it starts again, is marked "restart
    /// needed" with a Restart the game now button, and loading a save state made under another
    /// value of it gets a warning (`OptionDef::says_restart` in
    /// crates/emulator-bridge/src/cores/options.rs, `isStateSensitive` in
    /// native/ios/SaveStates.swift). Exactly the two settings cBoy itself holds until the game is
    /// loaded again (`NextStart` in libretro.rs) say it. Any other that did would stop changing
    /// straight away, and none may say it in a way the app reads as "no restart".
    #[test]
    fn only_the_two_settings_held_for_a_restart_say_restart() {
        let not = ["no restart", "without restart", "without a restart", "not require a restart",
            "n't require a restart", "n't need a restart"];
        let mut says = Vec::new();
        for (_, key, desc, info, _, _) in SPECS {
            let text = format!("{} {}", desc.to_string_lossy(), info.to_string_lossy()).to_lowercase();
            for n in not {
                assert!(!text.contains(n), "{}: \"{n}\"", key.to_string_lossy());
            }
            if text.contains("restart") || text.contains("(reload") {
                says.push(key.to_str().unwrap());
            }
        }
        assert_eq!(says, ["cboy_sgb_border", "cboy_sample_rate"]);
    }

    /// The settings cBoy only picks up as a game starts say so, so nobody changes one mid-game
    /// and wonders why nothing happened. ("Start from a boot ROM file" and the Advance BIOS are
    /// about how a game starts, which says it already.)
    #[test]
    fn settings_that_wait_for_the_next_start_say_so() {
        for key in [c"cboy_gb_model", c"cboy_cgb_revision", c"cboy_hybrid_model", c"cboy_gba_model", c"cboy_force_color",
            c"cboy_gba_save_type", c"cboy_gba_hardware", c"cboy_gbp", c"cboy_dmg_colorize", c"cboy_newer_console",
            c"cboy_sgb_speed", c"cboy_dual", c"cboy_dual_layout", c"cboy_dual_show"] {
            let info = SPECS.iter().find(|s| s.1 == key).unwrap().3.to_str().unwrap();
            assert!(info.contains("next start"), "{key:?}: \"{info}\"");
        }
    }

    /// Everything a player reads in cBoy's settings is plain words. These are the ones the owner
    /// picked out of the page itself on 12 October 2026; nothing may bring them back.
    #[test]
    fn no_jargon_in_anything_a_player_reads() {
        let jargon = ["directsound", "capacitor", "aliasing", "256x224", "handshake", "fifo", "hex", "waveform"];
        // Words a player wouldn't know (the cBoy checklist, 13 October 2026), matched as the start
        // of a word, so "registers" counts and "while" does not.
        let words = ["m-cycle", "register", "hle", "dmg", "cgb", "agb", "latency", "scanline"];
        let check = |key: &str, text: String| {
            let text = text.to_lowercase();
            for word in jargon {
                assert!(!text.contains(word), "{key}: \"{word}\"");
            }
            for w in text.split(|c: char| !c.is_ascii_alphanumeric() && c != '-') {
                for word in words {
                    assert!(!w.starts_with(word), "{key}: \"{w}\"");
                }
            }
        };
        for (cat, key, desc, info, values, _) in SPECS {
            // The layer switches are not shown with the ordinary settings (see `hide_debug`).
            if *cat == c"debug" {
                continue;
            }
            let labels: Vec<String> = values.iter().map(|v| v.1.to_string_lossy().into_owned()).collect();
            check(key.to_str().unwrap(), format!("{} {} {}", desc.to_string_lossy(), info.to_string_lossy(), labels.join(" ")));
        }
        // And the line under each group's heading.
        for (key, name, info) in CATEGORIES {
            check(key.to_str().unwrap(), format!("{} {}", name.to_string_lossy(), info.to_string_lossy()));
        }
    }

    /// Two phones link from the game's ... menu since the app's half landed (commit 1dee835), so
    /// the Link cable setting says how, naming that menu's row exactly as the app writes it
    /// (`native/ios/PlayerScreen.swift`, "Link cable to a second phone"). If the row is renamed
    /// there, this text and this test change with it.
    #[test]
    fn the_link_cable_setting_says_how_to_link_two_phones() {
        let info = SPECS.iter().find(|s| s.1 == c"cboy_link").unwrap().3.to_str().unwrap();
        assert!(info.contains("... menu") && info.contains("Link cable to a second phone"), "{info}");
        assert!(!info.contains("not available"), "{info}");
    }

    /// What the core asked the frontend to hide, and whether it offered to be asked again.
    static HIDDEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    static ASKED_AGAIN: std::sync::Mutex<bool> = std::sync::Mutex::new(false);

    unsafe extern "C" fn declaring_env(cmd: u32, d: *mut c_void) -> bool {
        match cmd {
            ENV_SET_CORE_OPTIONS_V2 => true,
            ENV_SET_CORE_OPTIONS_DISPLAY => {
                let v = &*(d as *const OptDisplay);
                if !v.visible {
                    HIDDEN.lock().unwrap().push(CStr::from_ptr(v.key).to_string_lossy().into_owned());
                }
                true
            }
            ENV_SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK => {
                *ASKED_AGAIN.lock().unwrap() = (*(d as *const UpdateDisplayCallback)).callback.is_some();
                true
            }
            _ => false,
        }
    }

    /// The layer switches are declared, so anyone hunting a drawing problem can still reach them
    /// through the app's own "show what the core hid", but they are not in the settings page a
    /// player sees: turning "Sprites" off there just makes a game look broken.
    #[test]
    fn the_layer_switches_are_kept_out_of_the_settings_page() {
        HIDDEN.lock().unwrap().clear();
        unsafe { declare(declaring_env) };
        let hidden = HIDDEN.lock().unwrap().clone();
        let debug: Vec<String> = SPECS.iter().filter(|s| s.0 == c"debug").map(|s| s.1.to_string_lossy().into_owned()).collect();
        assert!(debug.len() >= 10, "{debug:?}");
        assert_eq!(hidden, debug, "every Debug option, and nothing else, is hidden");
        assert!(*ASKED_AGAIN.lock().unwrap(), "the frontend must be able to ask again before it lists them");
        // Settings a player has a use for stay in the page, including the two accuracy switches
        // that used to sit in the Debug group.
        for key in ["cboy_sprite_limit", "cboy_gba_sprite_limit", "cboy_dmg_palette", "cboy_solar"] {
            assert!(!hidden.iter().any(|h| h == key), "{key} must stay visible");
        }
    }

    #[test]
    fn sunlight_sits_at_half_when_there_is_no_light_reading_to_be_had() {
        // A level the player picked is used as it stands.
        assert_eq!(sunlight(Some(0), None), 0);
        assert_eq!(sunlight(Some(255), Some(4000.0)), 255);
        assert_eq!(settings(&[("cboy_solar", "0")]).solar, Some(0));
        // "Light sensor" with a reading: dark to full sun.
        assert_eq!(settings(&[("cboy_solar", "phone")]).solar, None);
        assert_eq!(sunlight(None, Some(0.0)), 0);
        assert_eq!(sunlight(None, Some(4000.0)), 255);
        assert_eq!(sunlight(None, Some(40_000.0)), 255, "daylight is not brighter than full sun");
        assert!((100..150).contains(&sunlight(None, Some(2000.0))));
        // "Light sensor" where there is no sensor to read, which is every iPhone: half sun, so
        // Boktai can be played instead of sitting in the dark.
        assert_eq!(sunlight(None, None), 128);
    }

    #[test]
    fn defaults_parse_to_hardware_behaviour() {
        assert_eq!(settings(&[]), Settings::default());
    }

    #[test]
    fn each_option_parses() {
        let s = settings(&[
            ("cboy_gb_model", "sgb2"), ("cboy_boot_rom", "on"), ("cboy_gba_bios", "file"),
            ("cboy_gba_save_type", "eeprom512"), ("cboy_gba_hardware", "solar"), ("cboy_rtc", "phone"),
            ("cboy_rtc_offset", "-6"), ("cboy_dmg_palette", "green"), ("cboy_dmg_colorize", "ocean"),
            ("cboy_gbc_color", "soft"), ("cboy_gba_color", "tv"), ("cboy_blend", "heavy"), ("cboy_sgb_border", "off"),
            ("cboy_volume", "50"), ("cboy_vol_sq1", "0"), ("cboy_vol_sq2", "25"), ("cboy_vol_wave", "75"),
            ("cboy_vol_noise", "50"), ("cboy_vol_fifo_a", "25"), ("cboy_vol_fifo_b", "0"), ("cboy_lowpass", "strong"),
            ("cboy_highpass", "off"), ("cboy_gba_smooth", "on"), ("cboy_gba_music", "high"), ("cboy_sample_rate", "32768"), ("cboy_turbo", "10"),
            ("cboy_rumble", "off"), ("cboy_tilt_source", "stick"), ("cboy_gyro_source", "off"), ("cboy_solar", "100"),
            ("cboy_link", "loopback"), ("cboy_frameskip", "2"), ("cboy_run_ahead", "1"), ("cboy_idle_skip", "on"),
            ("cboy_layer_bg", "off"), ("cboy_layer_window", "off"), ("cboy_layer_obj", "off"), ("cboy_layer_bg0", "off"),
            ("cboy_layer_bg1", "on"), ("cboy_layer_bg2", "off"), ("cboy_layer_bg3", "on"),
        ]);
        assert_eq!(s.model, Model::Sgb2);
        assert!(s.boot_rom && s.gba_bios_file);
        assert_eq!(s.gba_save, Some((SaveKind::Eeprom, 6)));
        assert_eq!(s.gba_hardware, Some(Hardware::Solar));
        assert_eq!((s.rtc_mode, s.rtc_offset_hours), (RtcMode::Phone, -6));
        assert_eq!(s.dmg_palette, palette("green"));
        assert_eq!(s.colorize, Colorize::Scheme(1));
        assert_eq!((s.gbc_color, s.gba_color, s.blend, s.sgb_border), (2, 3, 3, false));
        assert_eq!((s.volume, s.chan_vol, s.fifo_vol), (50, [0, 25, 75, 50], [25, 0]));
        assert_eq!((s.lowpass, s.highpass, s.gba_smooth, s.sample_rate), (3, false, true, 32_768));
        // Advance music quality: High has to be asked for; "Console (as it was)" is the default,
        // so nobody's sound changes by surprise.
        assert!(s.gba_music_hq && !Settings::default().gba_music_hq);
        assert!(!settings(&[("cboy_gba_music", "console")]).gba_music_hq);
        assert_eq!((s.turbo_period, s.rumble, s.tilt, s.gyro, s.solar), (6, false, Source::Stick, Source::Off, Some(255)));
        assert_eq!((s.link_loopback, s.frameskip, s.run_ahead, s.idle_skip), (true, 2, 1, true));
        assert_eq!((s.gb_layers, s.gba_layers), (0, 0b1110_1010));
        let t = settings(&[("cboy_gba_bios", "file_skip"), ("cboy_sgb_border", "always"), ("cboy_color_temp", "warm"),
            ("cboy_contrast", "reduce"), ("cboy_dark_filter", "30"), ("cboy_highpass", "gentle"), ("cboy_audio_quality", "high"),
            ("cboy_rumble", "50"), ("cboy_motion_sensitivity", "200"), ("cboy_opposing", "on"), ("cboy_stick_dpad", "on"),
            ("cboy_sprite_limit", "off"), ("cboy_layer_win0", "off"), ("cboy_layer_objwin", "off")]);
        assert!(t.gba_bios_file && t.gba_bios_skip_logo && t.border_always && t.sgb_border);
        assert_eq!((t.color_temp, t.contrast, t.dark, t.highpass_hz, t.audio_hq), (1, 2, 30, 5.0, true));
        assert_eq!((t.rumble_strength, t.motion_scale, t.opposing, t.stick_dpad, t.sprite_limit), (50, 2.0, true, true, false));
        assert_eq!(t.gba_layers, 0b0101_1111);
        let u = settings(&[("cboy_gb_model", "sgb_pal"), ("cboy_frameskip", "auto"), ("cboy_interference", "50")]);
        let n = settings(&[("cboy_stick_deadzone", "30"), ("cboy_swap_stick", "on"), ("cboy_frameskip_threshold", "60"),
            ("cboy_frameskip_interval", "3"), ("cboy_gba_sprite_limit", "on"), ("cboy_force_color", "on"), ("cboy_gbc_light", "above")]);
        assert_eq!((n.stick_deadzone, n.swap_stick, n.frameskip_threshold, n.frameskip_interval), (0.3, true, 60, 3));
        assert!(n.gba_obj_limit && n.force_color && n.gbc_light == 1);
        let w = 0x00C0C0C0;
        assert!(correct_lit(w, Look::GbcScreen, 1) > correct_lit(w, Look::GbcScreen, 0));
        assert!(correct_lit(w, Look::GbcScreen, -1) < correct_lit(w, Look::GbcScreen, 0));
        let r = settings(&[("cboy_gb_model", "auto_sgb"), ("cboy_cgb_revision", "0")]);
        assert!(r.auto_sgb && r.cgb_rev0 && r.model == Model::Auto);
        assert_eq!(r.cgb_rev, crate::ppu::CgbRev::C, "revision 0 draws as a C");
        assert_eq!(settings(&[]).cgb_rev, crate::ppu::CgbRev::E, "the usual one is an E");
        assert_eq!(settings(&[("cboy_gb_model", "dmg0")]).model, Model::Dmg0);
        let m = settings(&[("cboy_hybrid_model", "sgb"), ("cboy_gba_model", "gbp"), ("cboy_gba_clock", "game"),
            ("cboy_vba_compat", "off")]);
        assert_eq!((m.hybrid_model, m.gba_model), (Model::Sgb, GbaModel::Gbp));
        assert!(m.gba_game_clock && !m.vba_compat);
        assert_eq!((u.model, u.sgb_pal, u.frameskip_auto, u.frameskip, u.interference), (Model::Sgb, true, true, 0, 50));
    }

    #[test]
    fn the_console_a_cartridge_runs_on() {
        // Plain Game Boy, Color-only and both-consoles cartridges, on the default settings: the
        // same choices cBoy has always made.
        let d = Settings::default();
        assert_eq!(d.console_for(0x00, false), Model::Dmg);
        assert_eq!(d.console_for(0xC0, false), Model::Cgb);
        assert_eq!(d.console_for(0x80, false), Model::Cgb);
        // "Runs on both" only touches the both-consoles ones.
        for (v, want) in [("dmg", Model::Dmg), ("sgb", Model::Sgb), ("agb", Model::Agb), ("cgb", Model::Cgb)] {
            let s = settings(&[("cboy_hybrid_model", v)]);
            assert_eq!(s.console_for(0x80, false), want, "{v}");
            assert_eq!(s.console_for(0xC0, false), Model::Cgb, "{v} must not move a Color-only game");
            assert_eq!(s.console_for(0x00, false), Model::Dmg, "{v} must not move a Game Boy game");
        }
        // Auto (Super Game Boy) still catches a Super Game Boy cartridge, and a chosen console
        // still wins over both.
        let a = settings(&[("cboy_gb_model", "auto_sgb"), ("cboy_hybrid_model", "dmg")]);
        assert_eq!(a.console_for(0x00, true), Model::Sgb);
        assert_eq!(a.console_for(0x80, true), Model::Dmg);
        let p = settings(&[("cboy_gb_model", "mgb"), ("cboy_hybrid_model", "agb")]);
        assert_eq!(p.console_for(0x80, false), Model::Mgb);
    }

    /// "Colour for black-and-white games": Off is the default and changes nothing; Automatic and
    /// the twelve button schemes are a Game Boy Color's colours, which put a black-and-white game
    /// on a Color when the console is Auto and leave every other choice of console alone; cBoy's
    /// own looks are what they always were.
    /// "Let games see a newer console": with the console on Auto, a Game Boy game gets a Pocket,
    /// a Super Game Boy one a Super Game Boy 2, a Color one an Advance; a console the player
    /// picked is kept.
    #[test]
    fn a_newer_console_for_games_that_look_for_one() {
        let at = |pairs: &[(&str, &str)], flag: u8, sgb: bool| settings(pairs).console_for(flag, sgb);
        let pocket = [("cboy_newer_console", "pocket")];
        assert_eq!(at(&pocket, 0, false), Model::Mgb);
        assert_eq!(at(&[("cboy_newer_console", "pocket"), ("cboy_gb_model", "auto_sgb")], 0, true), Model::Sgb2);
        assert_eq!(at(&pocket, 0xC0, false), Model::Cgb, "a Color game is not moved by the Pocket choice");
        let advance = [("cboy_newer_console", "advance")];
        assert_eq!(at(&advance, 0xC0, false), Model::Agb);
        assert_eq!(at(&advance, 0x80, false), Model::Agb, "nor are games that run on both");
        assert_eq!(at(&advance, 0, false), Model::Dmg);
        let both = [("cboy_newer_console", "both")];
        assert_eq!((at(&both, 0, false), at(&both, 0xC0, false)), (Model::Mgb, Model::Agb));
        assert_eq!(at(&[("cboy_newer_console", "both"), ("cboy_gb_model", "dmg")], 0, false), Model::Dmg, "a picked console stays");
        assert_eq!(at(&[], 0, false), Model::Dmg, "off by default");
        let s = settings(&[("cboy_sgb_speed", "normal"), ("cboy_gbp_reset", "on"), ("cboy_dmg_palette", "auto"), ("cboy_gb_idle_skip", "on")]);
        assert!(!s.sgb_fast && s.gbp_reset && s.palette_auto && s.gb_idle_skip);
        let d = settings(&[]);
        assert!(d.sgb_fast && !d.gbp_reset && !d.palette_auto && !d.gb_idle_skip);
    }

    #[test]
    fn the_colour_choices_for_black_and_white_games() {
        assert_eq!(Settings::default().colorize, Colorize::Off);
        assert_eq!(settings(&[("cboy_dmg_colorize", "off")]), Settings::default());
        assert_eq!(settings(&[("cboy_dmg_colorize", "gbc")]).colorize, Colorize::Gbc(None));
        for (i, v) in GBC_PICKS.iter().enumerate() {
            assert_eq!(settings(&[("cboy_dmg_colorize", v)]).colorize, Colorize::Gbc(Some(i)), "{v}");
        }
        // Every value the settings page offers parses to what it says, and the twelve button
        // schemes are in `compat::PICKS`'s order: Up first, Right + B last.
        let values = &SPECS.iter().find(|s| s.1 == c"cboy_dmg_colorize").unwrap().4;
        assert_eq!(values.len(), 2 + 12 + 7);
        assert_eq!(values[2].1, c"Brown (Up)");
        assert_eq!(values[13].1, c"Reversed (Right + B)");
        for (v, _) in values.iter() {
            let v = v.to_str().unwrap();
            assert!(v == "off" || settings(&[("cboy_dmg_colorize", v)]).colorize != Colorize::Off, "{v} is not read");
        }
        assert_eq!(settings(&[("cboy_dmg_colorize", "auto")]).colorize, Colorize::Auto, "cBoy's own pick keeps its value");
        // The console: a black-and-white game goes on a Color for a Color's colours, but only on
        // Auto. A console the player picked, and Super Game Boy games under Auto (Super Game
        // Boy), stay where they are; so do colour games and cBoy's own looks.
        let gbc = settings(&[("cboy_dmg_colorize", "gbc_left")]);
        assert!(gbc.gbc_colours());
        assert_eq!(gbc.console_for(0x00, false), Model::Cgb);
        assert_eq!(gbc.console_for(0x80, false), Model::Cgb);
        assert_eq!(gbc.console_for(0xC0, false), Model::Cgb);
        assert_eq!(settings(&[("cboy_dmg_colorize", "gbc"), ("cboy_gb_model", "dmg")]).console_for(0x00, false), Model::Dmg);
        assert_eq!(settings(&[("cboy_dmg_colorize", "gbc"), ("cboy_gb_model", "agb")]).console_for(0x00, false), Model::Agb);
        assert_eq!(settings(&[("cboy_dmg_colorize", "gbc"), ("cboy_gb_model", "auto_sgb")]).console_for(0x00, true), Model::Sgb);
        assert_eq!(settings(&[("cboy_dmg_colorize", "gbc"), ("cboy_hybrid_model", "dmg")]).console_for(0x80, false), Model::Dmg);
        assert_eq!(settings(&[("cboy_dmg_colorize", "meadow")]).console_for(0x00, false), Model::Dmg);
        assert!(!settings(&[("cboy_dmg_colorize", "slate")]).gbc_colours());
        // A Color's colours never reach a Game Boy's picture: on one, they leave the Game Boy
        // palette in charge.
        let mut post = Post::default();
        let mut f = vec![0u32; 4];
        post.video(&mut f, &[0, 1, 2, 3], &[0, 1, 2, 0], &settings(&[("cboy_dmg_colorize", "gbc"), ("cboy_dmg_palette", "green")]), Machine::Dmg, "T");
        assert_eq!(f, palette("green").to_vec());
    }

    #[test]
    fn the_game_boy_player_handshake_follows_the_advance_console() {
        assert!(!Settings::default().player_handshake());
        assert!(settings(&[("cboy_gba_model", "gbp")]).player_handshake());
        assert!(!settings(&[("cboy_gba_model", "sp")]).player_handshake());
        assert!(!settings(&[("cboy_gba_model", "gba")]).player_handshake());
        // The older rumble option on its own still answers it.
        assert!(settings(&[("cboy_gbp", "on")]).player_handshake());
    }

    #[test]
    fn palettes_colorize_and_custom_file() {
        let mut post = Post::default();
        let s = settings(&[("cboy_dmg_palette", "green")]);
        let mut f = vec![0u32; 4];
        post.video(&mut f, &[0, 1, 2, 3], &[0; 4], &s, Machine::Dmg, "T");
        assert_eq!(f, palette("green").to_vec());
        let s = settings(&[("cboy_dmg_colorize", "ember")]);
        post.video(&mut f, &[3, 3, 3, 0], &[0, 1, 2, 0], &s, Machine::Dmg, "T");
        assert_eq!(f, vec![SCHEMES[2][0][3], SCHEMES[2][1][3], SCHEMES[2][2][3], SCHEMES[2][0][0]]);
        assert_eq!(auto_scheme("POKEMON YELLOW"), auto_scheme("POKEMON YELLOW"));
        assert_eq!(read_custom_palette("#ffffff, #aaaaaa\n555555 0x000000"), Some([0xFFFFFF, 0xAAAAAA, 0x555555, 0]));
        assert_eq!(read_custom_palette("ffffff"), None);
    }

    #[test]
    fn colour_looks_keep_black_and_change_mid_tones() {
        for l in [Look::GbcScreen, Look::GbcSoft, Look::Gba, Look::GbaSp, Look::Tv] {
            assert_eq!(correct(0, l), 0, "{l:?}");
            assert_ne!(correct(0x0080FF, l), 0x0080FF, "{l:?}");
        }
    }

    #[test]
    fn blending_mixes_and_ghosting_trails() {
        let mut p = Post::default();
        let mut a = vec![0x00FF_FFFFu32];
        p.blend(&mut a, 1);
        let mut b = vec![0u32];
        p.blend(&mut b, 1);
        assert_eq!(b[0], 0x0080_8080);
        let mut p = Post::default();
        p.blend(&mut vec![0x00FF_FFFF], 3);
        let mut c = vec![0u32];
        p.blend(&mut c, 3);
        let mut d = vec![0u32];
        p.blend(&mut d, 3);
        assert!(d[0] & 0xFF > 0 && d[0] & 0xFF < c[0] & 0xFF, "{:06X} {:06X}", c[0], d[0]);
    }

    #[test]
    fn tone_changes_the_picture() {
        let base = vec![0x00C04020u32, 0x00FFFFFF];
        for (k, v) in [("cboy_color_temp", "warm"), ("cboy_color_temp", "cool"), ("cboy_contrast", "boost"),
                       ("cboy_contrast", "reduce"), ("cboy_dark_filter", "50")] {
            let mut f = base.clone();
            tone(&mut f, &settings(&[(k, v)]));
            assert_ne!(f, base, "{k}={v}");
        }
        let mut f = base.clone();
        tone(&mut f, &settings(&[("cboy_dark_filter", "50")]));
        assert_eq!(f[1], 0x00808080);
    }

    #[test]
    fn audio_volume_filters() {
        let mut p = Post::default();
        let mut x = vec![1000i16; 2];
        p.audio(&mut x, &Settings { volume: 50, highpass: false, ..Settings::default() }, 48_000);
        assert_eq!(x, vec![500, 500]);
        // DC removal: a constant level decays toward zero.
        let mut p = Post::default();
        let mut dc = vec![8000i16; 48_000];
        p.audio(&mut dc, &Settings::default(), 48_000);
        assert!(dc[47_998].abs() < 100, "{}", dc[47_998]);
        // Treble cut: a full-scale alternating signal is reduced.
        let mut p = Post::default();
        let mut hf: Vec<i16> = (0..2000).map(|i| if (i / 2) % 2 == 0 { 10_000 } else { -10_000 }).collect();
        p.audio(&mut hf, &Settings { lowpass: 3, highpass: false, ..Settings::default() }, 48_000);
        assert!(hf[1998].abs() < 3000, "{}", hf[1998]);
    }
}

/// Every option key with every value it accepts (for tests and tooling).
pub fn all_values() -> Vec<(String, Vec<String>)> {
    SPECS.iter()
        .map(|s| (s.1.to_string_lossy().into_owned(), s.4.iter().map(|v| v.0.to_string_lossy().into_owned()).collect()))
        .collect()
}
