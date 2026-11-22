//! The Settings cards: what each row is, how it reads its value and how it changes it.

use crate::config::{Action, Settings, ACCENT_PRESETS, BACKGROUND_PRESETS, OWN_SWIRL_PRESETS, PAD_BUTTONS, REMOTE_KEYS, SINGLE_COLOURS};
use crate::retro::{OptionCategory, OptionDef};
use std::collections::HashMap;

#[derive(Clone)]
pub enum Row {
    Toggle { key: String, label: String },
    Choice { key: String, label: String, values: Vec<(String, String)>, swatch: bool },
    Slider { key: String, label: String, min: f64, max: f64, step: f64 },
    Action { key: String, label: String, hint: String, danger: bool },
    Heading(String),
    Note(String),
}

pub struct Card {
    pub key: &'static str,
    pub title: &'static str,
    pub subtitle: String,
    pub rows: Vec<Row>,
}

fn t(key: &str, label: &str) -> Row {
    Row::Toggle { key: key.into(), label: label.into() }
}
fn c(key: &str, label: &str, values: &[(&str, &str)]) -> Row {
    Row::Choice { key: key.into(), label: label.into(), values: values.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(), swatch: false }
}
fn sw(key: &str, label: &str, values: Vec<(String, String)>) -> Row {
    Row::Choice { key: key.into(), label: label.into(), values, swatch: true }
}
fn sl(key: &str, label: &str, min: f64, max: f64, step: f64) -> Row {
    Row::Slider { key: key.into(), label: label.into(), min, max, step }
}
fn a(key: &str, label: &str, hint: &str, danger: bool) -> Row {
    Row::Action { key: key.into(), label: label.into(), hint: hint.into(), danger }
}

const ACCENT_NAMES: &[&str] = &["Gold", "Blue", "Sunset", "Purple and pink", "Silver", "Teal", "Red", "Green"];
const BG_NAMES: &[&str] = &["Charcoal", "Black", "Midnight", "Plum", "Forest", "Graphite"];
const COLOUR_NAMES: &[&str] = &["Gold", "White", "Pink", "Blue", "Purple", "Teal", "Orange", "Green"];
const OWN_NAMES: &[&str] = &["Purple, pink, green", "Pink, peach, violet", "Sky, violet, pink", "Fire", "Ocean", "Candy"];

fn named(list: &[&str], names: &[&str]) -> Vec<(String, String)> {
    list.iter().zip(names.iter()).map(|(v, n)| (v.to_string(), n.to_string())).collect()
}

/// TV wording for the core's own option text, which was written for a phone.
pub fn tv_words(s: &str) -> String {
    s.replace("phone's", "TV's").replace("Phone's", "TV's").replace("on one phone", "on one TV").replace("phone", "TV").replace("Phone", "TV")
}

pub fn cards(s: &Settings, core: &(Vec<OptionDef>, Vec<OptionCategory>), hidden: &HashMap<String, bool>, core_version: &str, in_game: bool) -> Vec<Card> {
    let mut out = Vec::new();

    // ---- Look ----
    let mut look = vec![
        sw("accent", "Accent", named(ACCENT_PRESETS, ACCENT_NAMES)),
        c("background_style", "Background", &[("swirl", "Swirl"), ("glow", "Glow"), ("plain", "Plain")]),
    ];
    if s.background_style == "swirl" {
        look.push(c("swirl_source", "Swirl colours", &[("accent", "Accent"), ("rainbow", "Rainbow"), ("own", "My own")]));
        if s.swirl_source == "own" {
            look.push(sw("swirl_own", "My colours", named(OWN_SWIRL_PRESETS, OWN_NAMES)));
        }
        look.push(sl("swirl_strength", "Strength", 0.2, 1.0, 0.05));
        look.push(sl("swirl_speed", "Speed", 0.25, 3.0, 0.25));
    }
    if s.background_style != "plain" {
        look.push(t("background_moves", "Moving"));
    }
    look.push(sw("background", "Background colour", named(BACKGROUND_PRESETS, BG_NAMES)));
    out.push(Card { key: "look", title: "Look", subtitle: "Accent, background and swirl".into(), rows: look });

    // ---- Covers ----
    let mut covers = vec![c("glow_mode", "Glow round each cover", &[("console", "Console colours"), ("accent", "Accent"), ("off", "Off"), ("custom", "Custom")])];
    if s.glow_mode == "custom" {
        covers.push(sw("glow", "Glow colour", named(SINGLE_COLOURS, COLOUR_NAMES)));
    }
    covers.push(sl("cover_corner", "Corner roundness", 0.0, 1.0, 0.05));
    covers.push(c("card_size", "Cover size", &[("s", "Small"), ("m", "Medium"), ("l", "Large")]));
    covers.push(t("hide_titles", "Hide titles"));
    out.push(Card { key: "covers", title: "Covers", subtitle: "Glow, corners and size".into(), rows: covers });

    // ---- Light and motion ----
    let mut motion = vec![c("ring_mode", "Ring next to CONTINUUM", &[("rainbow", "Rainbow"), ("accent", "Accent"), ("custom", "Custom")])];
    if s.ring_mode == "custom" {
        motion.push(sw("ring", "Ring colour", named(SINGLE_COLOURS, COLOUR_NAMES)));
    }
    motion.push(t("ring_spins", "Spin"));
    motion.push(t("button_light", "Light round buttons"));
    if s.button_light {
        motion.push(sl("light_speed", "Light speed", 0.3, 3.0, 0.1));
    }
    motion.push(t("hero_glow", "Glow behind the cover"));
    motion.push(t("box_floats", "Cover floats"));
    motion.push(t("focus_pulse", "Picked tile glows"));
    motion.push(t("focus_grow", "Picked tile grows"));
    motion.push(t("page_fades", "Pages slide in"));
    if !is_default_look(s) {
        motion.push(a("reset_look", "Back to gold on charcoal", "", false));
    }
    out.push(Card { key: "motion", title: "Light and motion", subtitle: "The ring, the light round buttons, glows".into(), rows: motion });

    // ---- Picture ----
    out.push(Card {
        key: "picture",
        title: "Picture",
        subtitle: "Screen size, pixels and LCD grid".into(),
        rows: vec![
            c("scale", "Screen size", &[("fit", "Fill the height"), ("integer", "Pixel-perfect"), ("stretch", "Stretch")]),
            c("sharp", "Pixels", &[("true", "Sharp"), ("false", "Smooth")]),
            c("lcd_grid", "LCD grid", &[("0", "Off"), ("0.35", "Light"), ("0.6", "Medium"), ("0.9", "Strong")]),
            c("game_backdrop", "Behind the game", &[("black", "Black"), ("dark", "Background colour"), ("swirl", "Swirl")]),
            t("show_fps", "Show the speed"),
        ],
    });

    // ---- Sound ----
    out.push(Card {
        key: "sound",
        title: "Sound",
        subtitle: "Volume and delay".into(),
        rows: vec![
            sl("volume", "Volume", 0.0, 100.0, 5.0),
            c("audio_latency_ms", "Sound delay", &[("48", "Low"), ("80", "Normal"), ("120", "High (if it crackles)")]),
        ],
    });

    // ---- Controls ----
    let actions: Vec<(String, String)> = Action::ALL.iter().map(|x| (format!("{x:?}"), x.label().to_string())).collect();
    let mut controls = vec![Row::Heading("Remote, in a game".into())];
    controls.push(Row::Note("The arrows are the D-pad. Back opens the menu.".into()));
    for (k, label) in REMOTE_KEYS {
        if *k == "back" {
            continue;
        }
        controls.push(Row::Choice { key: format!("remote:{k}"), label: label.to_string(), values: actions.clone(), swatch: false });
    }
    controls.push(Row::Heading("Controller, in a game".into()));
    for (b, label) in PAD_BUTTONS {
        controls.push(Row::Choice { key: format!("pad:{b}"), label: label.to_string(), values: actions.clone(), swatch: false });
    }
    controls.push(Row::Heading("More".into()));
    controls.push(c("fast_speed", "Fast forward speed", &[("2", "2x"), ("3", "3x"), ("4", "4x"), ("5", "5x")]));
    controls.push(c("turbo_rate", "Turbo speed", &[("3", "Fast"), ("6", "Normal"), ("10", "Slow")]));
    controls.push(t("stick_moves", "Left stick moves too"));
    controls.push(t("rumble", "Rumble"));
    controls.push(t("menu_combo", "View + Menu together opens the menu"));
    controls.push(a("reset_controls", "Put the buttons back", "", false));
    out.push(Card { key: "controls", title: "Controls", subtitle: "Remote and controller buttons".into(), rows: controls });

    // ---- Saves ----
    out.push(Card {
        key: "saves",
        title: "Saves",
        subtitle: "Picking up where you left off".into(),
        rows: vec![
            t("auto_resume", "Pick up where you left off"),
            t("auto_save", "Save when you leave a game"),
            c("state_slots", "Save slots", &[("3", "3"), ("6", "6"), ("9", "9"), ("12", "12")]),
            Row::Note("In-game saves are kept by themselves, always.".into()),
        ],
    });

    // ---- cBoy ----
    let (defs, cats) = core;
    if !defs.is_empty() {
        let mut rows = Vec::new();
        for cat in cats.iter().filter(|c| c.key != "debug") {
            let visible: Vec<&OptionDef> = defs
                .iter()
                .filter(|d| d.category == cat.key && !hidden.get(&d.key).copied().unwrap_or(false))
                .filter(|d| !d.values.is_empty())
                .collect();
            if visible.is_empty() {
                continue;
            }
            rows.push(Row::Heading(tv_words(&cat.label)));
            for d in visible {
                let values = d.values.iter().map(|(v, l)| (v.clone(), tv_words(l))).collect();
                rows.push(Row::Choice { key: format!("core:{}", d.key), label: tv_words(&d.label), values, swatch: false });
            }
        }
        for d in defs.iter().filter(|d| d.category.is_empty() && !d.values.is_empty()) {
            rows.push(Row::Choice { key: format!("core:{}", d.key), label: tv_words(&d.label), values: d.values.clone(), swatch: false });
        }
        if in_game {
            rows.push(Row::Note("Some of these start working the next time the game starts.".into()));
        }
        rows.push(a("reset_core", "Put cBoy's settings back", "", false));
        out.push(Card { key: "cboy", title: "Game Boy (cBoy)", subtitle: format!("Continuum's own emulator, {core_version}"), rows });
    }

    if !in_game {
        // ---- Library ----
        out.push(Card {
            key: "library",
            title: "Library",
            subtitle: "Layout, order and finding games".into(),
            rows: vec![
                c("library_layout", "Layout", &[("console", "By console"), ("grid", "Grid")]),
                c("library_sort", "Order", &[("name", "Name"), ("added", "Recently added"), ("played", "Last played")]),
                t("show_home_shelves", "Shelves for each console on Home"),
                a("rescan", "Look for new games", "", false),
                a("add_games", "Send games from your phone", "", false),
            ],
        });
        out.push(Card {
            key: "about",
            title: "About",
            subtitle: "Continuum for TV".into(),
            rows: vec![
                Row::Note(format!("Continuum for TV {}. Game Boy, Color and Advance run on cBoy, Continuum's own emulator.", env!("CARGO_PKG_VERSION"))),
                Row::Note("Made with SDL2 (zlib licence), fontdue and qrcodegen (MIT), zip and sevenz-rust2 (MIT or Apache 2.0).".into()),
                Row::Note("Questions or problems: ContinuumDev2c2b@gmail.com".into()),
                a("quit_app", "Leave Continuum", "", false),
            ],
        });
    }
    out
}

fn f(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A row's current value, as the string its choices use.
pub fn get(s: &Settings, core_vals: &HashMap<String, String>, core_defaults: &HashMap<String, String>, key: &str) -> String {
    if let Some(k) = key.strip_prefix("core:") {
        return core_vals.get(k).or_else(|| core_defaults.get(k)).cloned().unwrap_or_default();
    }
    if let Some(k) = key.strip_prefix("remote:") {
        return format!("{:?}", s.remote.get(k).copied().unwrap_or(Action::None));
    }
    if let Some(k) = key.strip_prefix("pad:") {
        let b: u8 = k.parse().unwrap_or(255);
        return format!("{:?}", s.pad.get(&b).copied().unwrap_or(Action::None));
    }
    match key {
        "accent" => s.accent.clone(),
        "background" => s.background.clone(),
        "background_style" => s.background_style.clone(),
        "background_moves" => s.background_moves.to_string(),
        "swirl_speed" => f(s.swirl_speed),
        "swirl_strength" => f(s.swirl_strength),
        "swirl_source" => s.swirl_source.clone(),
        "swirl_own" => s.swirl_own.clone(),
        "button_light" => s.button_light.to_string(),
        "light_speed" => f(s.light_speed),
        "ring_mode" => s.ring_mode.clone(),
        "ring" => s.ring.clone(),
        "ring_spins" => s.ring_spins.to_string(),
        "glow_mode" => s.glow_mode.clone(),
        "glow" => s.glow.clone(),
        "hero_glow" => s.hero_glow.to_string(),
        "box_floats" => s.box_floats.to_string(),
        "cover_corner" => f(s.cover_corner),
        "hide_titles" => s.hide_titles.to_string(),
        "focus_pulse" => s.focus_pulse.to_string(),
        "focus_grow" => s.focus_grow.to_string(),
        "page_fades" => s.page_fades.to_string(),
        "scale" => s.scale.clone(),
        "sharp" => s.sharp.to_string(),
        "lcd_grid" => f(s.lcd_grid),
        "game_backdrop" => s.game_backdrop.clone(),
        "show_fps" => s.show_fps.to_string(),
        "volume" => s.volume.to_string(),
        "audio_latency_ms" => s.audio_latency_ms.to_string(),
        "fast_speed" => s.fast_speed.to_string(),
        "turbo_rate" => s.turbo_rate.to_string(),
        "stick_moves" => s.stick_moves.to_string(),
        "rumble" => s.rumble.to_string(),
        "menu_combo" => s.menu_combo.to_string(),
        "auto_save" => s.auto_save.to_string(),
        "auto_resume" => s.auto_resume.to_string(),
        "state_slots" => s.state_slots.to_string(),
        "library_layout" => s.library_layout.clone(),
        "library_sort" => s.library_sort.clone(),
        "card_size" => s.card_size.clone(),
        "show_home_shelves" => s.show_home_shelves.to_string(),
        _ => String::new(),
    }
}

fn action_of(v: &str) -> Action {
    Action::ALL.iter().copied().find(|a| format!("{a:?}") == v).unwrap_or(Action::None)
}

/// Change a setting. Returns true when it was one of cBoy's (the caller passes it to the core).
pub fn set(s: &mut Settings, core_vals: &mut HashMap<String, String>, key: &str, v: &str) -> bool {
    if let Some(k) = key.strip_prefix("core:") {
        core_vals.insert(k.to_string(), v.to_string());
        return true;
    }
    if let Some(k) = key.strip_prefix("remote:") {
        s.remote.insert(k.to_string(), action_of(v));
        return false;
    }
    if let Some(k) = key.strip_prefix("pad:") {
        if let Ok(b) = k.parse::<u8>() {
            s.pad.insert(b, action_of(v));
        }
        return false;
    }
    let b = v == "true";
    let num = v.parse::<f64>().unwrap_or(0.0);
    match key {
        "accent" => s.accent = v.into(),
        "background" => s.background = v.into(),
        "background_style" => s.background_style = v.into(),
        "background_moves" => s.background_moves = b,
        "swirl_speed" => s.swirl_speed = num,
        "swirl_strength" => s.swirl_strength = num,
        "swirl_source" => s.swirl_source = v.into(),
        "swirl_own" => s.swirl_own = v.into(),
        "button_light" => s.button_light = b,
        "light_speed" => s.light_speed = num,
        "ring_mode" => s.ring_mode = v.into(),
        "ring" => s.ring = v.into(),
        "ring_spins" => s.ring_spins = b,
        "glow_mode" => s.glow_mode = v.into(),
        "glow" => s.glow = v.into(),
        "hero_glow" => s.hero_glow = b,
        "box_floats" => s.box_floats = b,
        "cover_corner" => s.cover_corner = num,
        "hide_titles" => s.hide_titles = b,
        "focus_pulse" => s.focus_pulse = b,
        "focus_grow" => s.focus_grow = b,
        "page_fades" => s.page_fades = b,
        "scale" => s.scale = v.into(),
        "sharp" => s.sharp = b,
        "lcd_grid" => s.lcd_grid = num,
        "game_backdrop" => s.game_backdrop = v.into(),
        "show_fps" => s.show_fps = b,
        "volume" => s.volume = num as u32,
        "audio_latency_ms" => s.audio_latency_ms = num as u32,
        "fast_speed" => s.fast_speed = num as u32,
        "turbo_rate" => s.turbo_rate = num as u32,
        "stick_moves" => s.stick_moves = b,
        "rumble" => s.rumble = b,
        "menu_combo" => s.menu_combo = b,
        "auto_save" => s.auto_save = b,
        "auto_resume" => s.auto_resume = b,
        "state_slots" => s.state_slots = num as u32,
        "library_layout" => s.library_layout = v.into(),
        "library_sort" => s.library_sort = v.into(),
        "card_size" => s.card_size = v.into(),
        "show_home_shelves" => s.show_home_shelves = b,
        _ => {}
    }
    false
}

/// How a slider shows its value.
pub fn slider_text(key: &str, v: f64) -> String {
    match key {
        "swirl_speed" | "light_speed" => {
            if (v - 1.0).abs() < 0.001 {
                "Normal".into()
            } else {
                format!("{}x", f(v))
            }
        }
        "volume" => format!("{}%", v.round()),
        _ => format!("{}%", (v * 100.0).round()),
    }
}

pub fn is_default_look(s: &Settings) -> bool {
    let d = Settings::default();
    s.accent == d.accent
        && s.background == d.background
        && s.background_style == d.background_style
        && s.background_moves == d.background_moves
        && (s.swirl_speed - d.swirl_speed).abs() < 1e-9
        && (s.swirl_strength - d.swirl_strength).abs() < 1e-9
        && s.swirl_source == d.swirl_source
        && s.button_light == d.button_light
        && (s.light_speed - d.light_speed).abs() < 1e-9
        && s.ring_mode == d.ring_mode
        && s.ring_spins == d.ring_spins
        && s.glow_mode == d.glow_mode
        && s.hero_glow == d.hero_glow
        && s.box_floats == d.box_floats
        && (s.cover_corner - d.cover_corner).abs() < 1e-9
        && s.hide_titles == d.hide_titles
        && s.focus_pulse == d.focus_pulse
        && s.focus_grow == d.focus_grow
        && s.page_fades == d.page_fades
}
