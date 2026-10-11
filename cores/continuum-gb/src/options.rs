//! cBoy's core options (libretro SET_CORE_OPTIONS_V2, categories per
//! docs/CORE_OPTIONS_STANDARD.md), and the output-side effects they drive (palettes, colour,
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
pub const ENV_SET_CORE_OPTIONS_V2: u32 = 67;

const NULL_VALUE: OptValue = OptValue { value: std::ptr::null(), label: std::ptr::null() };

type Values = &'static [(&'static CStr, &'static CStr)];
/// (category, key, label, info, values, default)
type Spec = (&'static CStr, &'static CStr, &'static CStr, &'static CStr, Values, &'static CStr);

const CATEGORIES: &[(&CStr, &CStr, &CStr)] = &[
    (c"system", c"System", c"Which console, start-up files, the cartridge's clock and save chip."),
    (c"video", c"Picture", c"Palettes, screen colour, LCD ghosting and the Super Game Boy border."),
    (c"audio", c"Sound", c"Volume, channel mix and filters."),
    (c"input", c"Controls", c"Turbo buttons, motion and light sensors, rumble."),
    (c"link", c"Link", c"The link cable."),
    (c"performance", c"Performance", c"Speed and latency."),
    (c"debug", c"Debug", c"Switch picture layers off one at a time."),
];

const PCT: Values = &[(c"100", c"100%"), (c"75", c"75%"), (c"50", c"50%"), (c"25", c"25%"), (c"0", c"Off")];
const ON_OFF: Values = &[(c"on", c"On"), (c"off", c"Off")];
const OFF_ON: Values = &[(c"off", c"Off"), (c"on", c"On")];

const SPECS: &[Spec] = &[
    // System
    (c"system", c"cboy_gb_model", c"Console for Game Boy and Color games",
     c"Auto runs Color cartridges on a Game Boy Color and the rest on a Game Boy. Takes effect on the next start.",
     &[(c"auto", c"Auto"), (c"auto_sgb", c"Auto (Super Game Boy for SGB games)"), (c"dmg", c"Game Boy"), (c"dmg0", c"Game Boy (first revision)"), (c"mgb", c"Game Boy Pocket"), (c"sgb", c"Super Game Boy"),
       (c"sgb2", c"Super Game Boy 2"), (c"sgb_pal", c"Super Game Boy (PAL)"), (c"cgb", c"Game Boy Color"), (c"agb", c"Game Boy Advance (Color games)")],
     c"auto"),
    (c"system", c"cboy_cgb_revision", c"Game Boy Color revision",
     c"Which Game Boy Color hardware revision to model. Revision 0 (the first Japanese units) leaves the timer at a different point after start-up; later revisions match each other. Next start.",
     &[(c"e", c"CPU CGB E (most common)"), (c"d", c"CPU CGB D"), (c"c", c"CPU CGB C"), (c"b", c"CPU CGB B"), (c"a", c"CPU CGB A"), (c"0", c"CPU CGB (revision 0)")],
     c"e"),
    (c"system", c"cboy_force_color", c"Run Game Boy games as Color",
     c"Runs Game Boy-only cartridges in Color mode, for colourisation hacks that write Color palettes. Ordinary Game Boy games may look wrong. Next start.",
     OFF_ON, c"off"),
    (c"system", c"cboy_boot_rom", c"Start from a boot ROM file",
     c"Plays the console's own start-up logo from a file you supply: <system folder>/cboy/dmg_boot.bin, mgb_boot.bin, sgb_boot.bin, sgb2_boot.bin or cgb_boot.bin. Without the file, cBoy plays its own short startup, then the game. Next start.",
     OFF_ON, c"off"),
    (c"system", c"cboy_gba_bios", c"Game Boy Advance BIOS",
     c"Built-in needs no file. From file uses <system folder>/cboy/gba_bios.bin (or gba_bios.bin) and plays the start-up logo; From file, no logo uses it but starts the game directly. Next start.",
     &[(c"builtin", c"Built-in"), (c"file", c"From file"), (c"file_skip", c"From file, no logo")], c"builtin"),
    (c"system", c"cboy_gba_save_type", c"Game Boy Advance save chip",
     c"Auto finds it from the game. Pick one if a game does not save. Next start.",
     &[(c"auto", c"Auto"), (c"sram", c"SRAM 32 KB"), (c"flash64", c"Flash 64 KB"), (c"flash128", c"Flash 128 KB"),
       (c"eeprom512", c"EEPROM 512 B"), (c"eeprom8k", c"EEPROM 8 KB"), (c"none", c"None")], c"auto"),
    (c"system", c"cboy_gba_hardware", c"Game Boy Advance cartridge extras",
     c"Clock, solar sensor, motion sensors or rumble built into some cartridges. Auto knows the games that have them. Next start.",
     &[(c"auto", c"Auto"), (c"none", c"None"), (c"rtc", c"Clock"), (c"solar", c"Solar sensor"),
       (c"gyro", c"Gyro sensor"), (c"tilt", c"Tilt sensor"), (c"rumble", c"Rumble")], c"auto"),
    (c"system", c"cboy_gbp", c"Game Boy Player rumble",
     c"Answers the Game Boy Player handshake so Advance games that rumble through the Player can turn the phone's motor on. Leave off for link-cable games. Next start.",
     OFF_ON, c"off"),
    (c"system", c"cboy_rtc", c"Cartridge clock",
     c"Real time keeps the clock running while the game is closed. Game time counts only while playing. Phone clock sets it to the phone's time at each start. The Advance's clock always follows the phone.",
     &[(c"real", c"Real time"), (c"game", c"Game time only"), (c"phone", c"Phone clock")], c"real"),
    (c"system", c"cboy_rtc_offset", c"Clock offset",
     c"Moves the phone-clock time by whole hours (Advance clocks, and the Phone clock setting).",
     &[(c"0", c"None"), (c"1", c"+1 hour"), (c"2", c"+2 hours"), (c"3", c"+3 hours"), (c"6", c"+6 hours"),
       (c"12", c"+12 hours"), (c"24", c"+1 day"), (c"-1", c"-1 hour"), (c"-2", c"-2 hours"), (c"-3", c"-3 hours"),
       (c"-6", c"-6 hours"), (c"-12", c"-12 hours"), (c"-24", c"-1 day")], c"0"),
    // Picture
    (c"video", c"cboy_dmg_palette", c"Game Boy palette",
     c"The four shades of original Game Boy games. Custom reads <system folder>/cboy/palette.txt: four colours as hex, lightest first.",
     &[(c"grayscale", c"Grayscale"), (c"green", c"Original green"), (c"pocket", c"Pocket"), (c"light", c"Light (backlit)"),
       (c"kiosk", c"Kiosk"), (c"sepia", c"Sepia"), (c"ice", c"Ice"), (c"custom", c"Custom")], c"grayscale"),
    (c"video", c"cboy_dmg_colorize", c"Colour Game Boy games",
     c"Gives original Game Boy games separate colours for background and sprites. Auto picks a scheme from the game's title. Overrides the palette above.",
     &[(c"off", c"Off"), (c"auto", c"Auto"), (c"meadow", c"Meadow"), (c"ocean", c"Ocean"), (c"ember", c"Ember"),
       (c"dusk", c"Dusk"), (c"candy", c"Candy"), (c"slate", c"Slate")], c"off"),
    (c"video", c"cboy_gbc_color", c"Game Boy Color screen colour",
     c"Off shows the colours as stored. Screen mimics the Color's own LCD; Soft is a lighter version of it.",
     &[(c"off", c"Off"), (c"screen", c"Screen"), (c"soft", c"Soft")], c"off"),
    (c"video", c"cboy_gbc_light", c"Game Boy Color light position",
     c"With a Game Boy Color screen colour on: where the light falls on the reflective screen. Above is brighter and washed out, below darker.",
     &[(c"central", c"Central"), (c"above", c"Above the screen"), (c"below", c"Below the screen")], c"central"),
    (c"video", c"cboy_gba_color", c"Game Boy Advance screen colour",
     c"Off shows the colours as stored. Advance mimics the original unlit screen; SP the frontlit SP; TV a Game Boy Player on a television.",
     &[(c"off", c"Off"), (c"gba", c"Advance"), (c"sp", c"Advance SP"), (c"tv", c"TV")], c"off"),
    (c"video", c"cboy_blend", c"LCD ghosting",
     c"Mix blends each frame with the last, which makes flicker effects solid. Ghosting leaves trails like the slow original screens.",
     &[(c"off", c"Off"), (c"mix", c"Mix"), (c"light", c"Ghosting, light"), (c"heavy", c"Ghosting, heavy")], c"off"),
    (c"video", c"cboy_sgb_border", c"Super Game Boy border",
     c"Shows the frame a Super Game Boy game draws around the picture (with the Super Game Boy console). Always also puts Game Boy games on any other console inside a plain 256x224 frame.",
     &[(c"on", c"Super Game Boy games"), (c"off", c"Off"), (c"always", c"Always")], c"on"),
    (c"video", c"cboy_color_temp", c"Colour temperature",
     c"Tints the whole picture warmer or cooler, like viewing the screen under different light.",
     &[(c"neutral", c"Neutral"), (c"warm", c"Warm"), (c"cool", c"Cool")], c"neutral"),
    (c"video", c"cboy_contrast", c"Contrast",
     c"Raises or lowers the contrast of the finished picture.",
     &[(c"normal", c"Normal"), (c"boost", c"Higher"), (c"reduce", c"Lower")], c"normal"),
    (c"video", c"cboy_dark_filter", c"Darken",
     c"Dims bright colours, easier on the eyes in the dark.",
     &[(c"0", c"Off"), (c"10", c"10%"), (c"20", c"20%"), (c"30", c"30%"), (c"40", c"40%"), (c"50", c"50%")], c"0"),
    // Sound
    (c"audio", c"cboy_volume", c"Volume", c"Overall volume.", PCT, c"100"),
    (c"audio", c"cboy_vol_sq1", c"Pulse channel 1", c"Volume of the first pulse (square) channel.", PCT, c"100"),
    (c"audio", c"cboy_vol_sq2", c"Pulse channel 2", c"Volume of the second pulse channel.", PCT, c"100"),
    (c"audio", c"cboy_vol_wave", c"Wave channel", c"Volume of the wave channel.", PCT, c"100"),
    (c"audio", c"cboy_vol_noise", c"Noise channel", c"Volume of the noise channel.", PCT, c"100"),
    (c"audio", c"cboy_vol_fifo_a", c"Advance sample channel A", c"Volume of the Advance's first sampled (DirectSound) channel.", PCT, c"100"),
    (c"audio", c"cboy_vol_fifo_b", c"Advance sample channel B", c"Volume of the Advance's second sampled channel.", PCT, c"100"),
    (c"audio", c"cboy_lowpass", c"Treble cut",
     c"Softens harsh high tones, like a small speaker. Off is the console's full output.",
     &[(c"off", c"Off"), (c"light", c"Light"), (c"medium", c"Medium"), (c"strong", c"Strong")], c"off"),
    (c"audio", c"cboy_highpass", c"Output capacitor",
     c"Centres the waveform the way the console's output capacitor does. Gentle keeps more bass; Off passes the raw signal, offset and all.",
     &[(c"on", c"Like the console"), (c"gentle", c"Gentle"), (c"off", c"Off")], c"on"),
    (c"audio", c"cboy_audio_quality", c"Sound quality",
     c"High averages the sound between output samples (less harshness and aliasing); Fast takes one point per sample and costs less work.",
     &[(c"fast", c"Fast"), (c"high", c"High")], c"fast"),
    (c"audio", c"cboy_gba_smooth", c"Smooth Advance samples",
     c"Ramps the Advance's sampled sound between samples, including the music most games mix in software. Less gritty, slightly duller.",
     OFF_ON, c"off"),
    (c"audio", c"cboy_interference", c"LCD buzz",
     c"Adds the faint buzz the screen's circuitry puts into a real Game Boy's sound.",
     &[(c"0", c"Off"), (c"25", c"Faint"), (c"50", c"Medium"), (c"100", c"Loud")], c"0"),
    (c"audio", c"cboy_sample_rate", c"Output sample rate",
     c"Lower rates cost less work. Next start.",
     &[(c"48000", c"48000 Hz"), (c"44100", c"44100 Hz"), (c"32768", c"32768 Hz")], c"48000"),
    // Controls
    (c"input", c"cboy_turbo", c"Turbo buttons",
     c"X and Y become rapid-fire A and B. The number is presses per second.",
     &[(c"off", c"Off"), (c"30", c"30"), (c"20", c"20"), (c"15", c"15"), (c"10", c"10"), (c"7", c"7.5")], c"off"),
    (c"input", c"cboy_rumble", c"Rumble", c"Passes a cartridge's rumble motor to the phone or controller, at this strength.",
     &[(c"100", c"100%"), (c"75", c"75%"), (c"50", c"50%"), (c"25", c"25%"), (c"off", c"Off")], c"100"),
    (c"input", c"cboy_motion_sensitivity", c"Motion sensitivity",
     c"How far the tilt and turn inputs move for a given motion of the phone or stick.",
     &[(c"100", c"Normal"), (c"50", c"Half"), (c"75", c"Lower"), (c"150", c"Higher"), (c"200", c"Double")], c"100"),
    (c"input", c"cboy_opposing", c"Allow opposite directions",
     c"Lets left and right (or up and down) be held together, which a real pad cannot do and some games misbehave on.",
     OFF_ON, c"off"),
    (c"input", c"cboy_stick_dpad", c"Left stick as D-pad",
     c"The analog stick also works as the D-pad.",
     OFF_ON, c"off"),
    (c"input", c"cboy_stick_deadzone", c"Stick dead zone",
     c"How far the analog stick must move before it counts, for the stick as D-pad, tilt and turn.",
     &[(c"15", c"15%"), (c"0", c"0%"), (c"5", c"5%"), (c"10", c"10%"), (c"20", c"20%"), (c"25", c"25%"), (c"30", c"30%")], c"15"),
    (c"input", c"cboy_swap_stick", c"Use the right stick",
     c"The right analog stick does what the left stick does in these settings.",
     OFF_ON, c"off"),
    (c"input", c"cboy_tilt_source", c"Tilt input",
     c"For tilt cartridges (Kirby Tilt 'n' Tumble, Yoshi Topsy-Turvy): the phone's motion or the left stick.",
     &[(c"phone", c"Phone motion"), (c"stick", c"Left stick"), (c"off", c"Off")], c"phone"),
    (c"input", c"cboy_gyro_source", c"Turn input",
     c"For the gyro cartridge (WarioWare: Twisted!): the phone's rotation or the left stick.",
     &[(c"phone", c"Phone rotation"), (c"stick", c"Left stick"), (c"off", c"Off")], c"phone"),
    (c"input", c"cboy_solar", c"Sunlight",
     c"For solar cartridges (Boktai): the phone's light sensor if it has one, or a fixed level.",
     &[(c"50", c"50%"), (c"phone", c"Light sensor"), (c"0", c"Dark"), (c"25", c"25%"), (c"75", c"75%"), (c"100", c"Full sun")], c"50"),
    // Link
    (c"link", c"cboy_link", c"Link cable",
     c"Unplugged is a console with nothing attached. Loopback connects the cable to itself, which some test programs use. Linking two phones is not available yet.",
     &[(c"off", c"Unplugged"), (c"loopback", c"Loopback")], c"off"),
    // Performance
    (c"performance", c"cboy_frameskip", c"Frame skip",
     c"Skips drawing some frames to save work; the game still runs at full speed.",
     &[(c"off", c"Off"), (c"auto", c"Auto (when the phone falls behind)"), (c"1", c"Every other frame"), (c"2", c"Two in three"), (c"3", c"Three in four")], c"off"),
    (c"performance", c"cboy_frameskip_threshold", c"Auto frame skip threshold",
     c"With auto frame skip: how much of a frame's time emulating may take before frames are skipped.",
     &[(c"85", c"85%"), (c"50", c"50%"), (c"60", c"60%"), (c"70", c"70%"), (c"80", c"80%"), (c"90", c"90%"), (c"95", c"95%")], c"85"),
    (c"performance", c"cboy_frameskip_interval", c"Auto frame skip limit",
     c"With auto frame skip: the most frames skipped in a row.",
     &[(c"1", c"1"), (c"2", c"2"), (c"3", c"3"), (c"4", c"4"), (c"6", c"6"), (c"9", c"9")], c"1"),
    (c"performance", c"cboy_run_ahead", c"Run-ahead",
     c"Shows the picture one or two frames ahead, cutting the input delay a game has built in. Costs two or three times the work.",
     &[(c"0", c"Off"), (c"1", c"1 frame"), (c"2", c"2 frames")], c"0"),
    (c"performance", c"cboy_idle_skip", c"Skip idle loops (Advance)",
     c"Fast-forwards through loops where an Advance game only waits. Saves battery; a few games may run at the wrong pace.",
     OFF_ON, c"off"),
    // Debug
    (c"debug", c"cboy_layer_bg", c"Background", c"Game Boy and Color background layer.", ON_OFF, c"on"),
    (c"debug", c"cboy_layer_window", c"Window", c"Game Boy and Color window layer.", ON_OFF, c"on"),
    (c"debug", c"cboy_layer_obj", c"Sprites", c"Game Boy, Color and Advance sprites.", ON_OFF, c"on"),
    (c"debug", c"cboy_layer_bg0", c"Advance BG0", c"Advance background 0.", ON_OFF, c"on"),
    (c"debug", c"cboy_layer_bg1", c"Advance BG1", c"Advance background 1.", ON_OFF, c"on"),
    (c"debug", c"cboy_layer_bg2", c"Advance BG2", c"Advance background 2.", ON_OFF, c"on"),
    (c"debug", c"cboy_layer_bg3", c"Advance BG3", c"Advance background 3.", ON_OFF, c"on"),
    (c"debug", c"cboy_layer_win0", c"Advance window 0", c"Off draws as if window 0 were disabled.", ON_OFF, c"on"),
    (c"debug", c"cboy_layer_win1", c"Advance window 1", c"Off draws as if window 1 were disabled.", ON_OFF, c"on"),
    (c"debug", c"cboy_layer_objwin", c"Advance object window", c"Off draws as if the object window were disabled.", ON_OFF, c"on"),
    (c"debug", c"cboy_gba_sprite_limit", c"Advance object time limit",
     c"Drops objects that a real Advance has no time to draw on a busy line. Off draws every object.",
     OFF_ON, c"off"),
    (c"debug", c"cboy_sprite_limit", c"Ten-sprite limit",
     c"The Game Boy shows at most ten sprites on a line, which makes some games flicker. Off lifts the limit.",
     ON_OFF, c"on"),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Phone,
    Stick,
    Off,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub model: Model,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtcMode {
    Real,
    Game,
    Phone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colorize {
    Off,
    Auto,
    Scheme(usize),
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            model: Model::Auto, boot_rom: false, gba_bios_file: false, gba_save: None, gba_hardware: None,
            rtc_mode: RtcMode::Real, rtc_offset_hours: 0, dmg_palette: crate::ppu::SHADES, custom_palette: false,
            colorize: Colorize::Off, gbc_color: 0, gba_color: 0, blend: 0, sgb_border: true, volume: 100,
            chan_vol: [100; 4], fifo_vol: [100; 2], lowpass: 0, highpass: true, gba_smooth: false,
            sample_rate: 48_000, turbo_period: 0, rumble: true, tilt: Source::Phone, gyro: Source::Phone,
            solar: Some(128), link_loopback: false, frameskip: 0, run_ahead: 0, idle_skip: false,
            gb_layers: 7, gba_layers: 0xFF, gba_bios_skip_logo: false, border_always: false, color_temp: 0,
            contrast: 0, dark: 0, highpass_hz: 28.0, audio_hq: false, rumble_strength: 100, motion_scale: 1.0,
            opposing: false, stick_dpad: false, sprite_limit: true, sgb_pal: false, auto_sgb: false, cgb_rev0: false, stick_deadzone: 0.15, swap_stick: false,
            frameskip_threshold: 85, frameskip_interval: 1, gba_obj_limit: false, force_color: false, gbp: false, gbc_light: 0, frameskip_auto: false, interference: 0,
        }
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
    s.cgb_rev0 = get(c"cboy_cgb_revision").is_some_and(|v| v == "0");
    s.stick_deadzone = get(c"cboy_stick_deadzone").and_then(|v| v.parse::<f32>().ok()).unwrap_or(15.0).clamp(0.0, 50.0) / 100.0;
    s.swap_stick = on(c"cboy_swap_stick", false);
    s.frameskip_threshold = get(c"cboy_frameskip_threshold").and_then(|v| v.parse().ok()).unwrap_or(85).clamp(30, 100);
    s.frameskip_interval = get(c"cboy_frameskip_interval").and_then(|v| v.parse().ok()).unwrap_or(1).clamp(1, 9);
    s.gba_obj_limit = on(c"cboy_gba_sprite_limit", false);
    s.force_color = on(c"cboy_force_color", false);
    s.gbp = on(c"cboy_gbp", false);
    s.gbc_light = match get(c"cboy_gbc_light").as_deref() { Some("above") => 1, Some("below") => -1, _ => 0 };
    s.sgb_pal = get(c"cboy_gb_model").is_some_and(|v| v == "sgb_pal");
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
        _ => Colorize::Off,
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
                    Colorize::Off => None,
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

    #[test]
    fn every_option_has_a_unique_cboy_key_a_category_and_its_default_among_its_values() {
        let mut seen = std::collections::HashSet::new();
        for (cat, key, _, info, values, default) in SPECS {
            let k = key.to_str().unwrap();
            assert!(k.starts_with("cboy_"), "{k}");
            assert!(seen.insert(k), "duplicate {k}");
            assert!(CATEGORIES.iter().any(|c| c.0 == *cat), "{k}");
            assert!(values.iter().any(|v| v.0 == *default), "{k}");
            assert!(!info.to_bytes().is_empty());
        }
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
            ("cboy_highpass", "off"), ("cboy_gba_smooth", "on"), ("cboy_sample_rate", "32768"), ("cboy_turbo", "10"),
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
        assert_eq!(settings(&[("cboy_gb_model", "dmg0")]).model, Model::Dmg0);
        assert_eq!((u.model, u.sgb_pal, u.frameskip_auto, u.frameskip, u.interference), (Model::Sgb, true, true, 0, 50));
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
