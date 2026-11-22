//! Every setting the player can change, with the New look's defaults, saved as JSON in the
//! player's own folder (outside the app's folder, so an update never loses it).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// What a remote key or controller button does while a game is playing.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    None,
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    Start,
    Select,
    L,
    R,
    TurboA,
    TurboB,
    Menu,
    FastForward,
    SaveState,
    LoadState,
}

impl Action {
    pub const ALL: [Action; 17] = [
        Action::None,
        Action::Up,
        Action::Down,
        Action::Left,
        Action::Right,
        Action::A,
        Action::B,
        Action::Start,
        Action::Select,
        Action::L,
        Action::R,
        Action::TurboA,
        Action::TurboB,
        Action::Menu,
        Action::FastForward,
        Action::SaveState,
        Action::LoadState,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Action::None => "Nothing",
            Action::Up => "Up",
            Action::Down => "Down",
            Action::Left => "Left",
            Action::Right => "Right",
            Action::A => "A",
            Action::B => "B",
            Action::Start => "Start",
            Action::Select => "Select",
            Action::L => "L",
            Action::R => "R",
            Action::TurboA => "Turbo A",
            Action::TurboB => "Turbo B",
            Action::Menu => "Menu",
            Action::FastForward => "Fast forward (hold)",
            Action::SaveState => "Quick save",
            Action::LoadState => "Quick load",
        }
    }
}

/// The remote's keys a player can map.
pub const REMOTE_KEYS: &[(&str, &str)] = &[
    ("ok", "OK"),
    ("back", "Back"),
    ("red", "Red"),
    ("green", "Green"),
    ("yellow", "Yellow"),
    ("blue", "Blue"),
    ("1", "1"),
    ("2", "2"),
    ("3", "3"),
    ("4", "4"),
    ("5", "5"),
    ("6", "6"),
    ("7", "7"),
    ("8", "8"),
    ("9", "9"),
    ("0", "0"),
    ("play", "Play"),
    ("pause", "Pause"),
    ("ff", "Fast-forward key"),
    ("rew", "Rewind key"),
    ("chup", "Channel up"),
    ("chdown", "Channel down"),
];

/// The controller's buttons a player can map (SDL game controller names).
pub const PAD_BUTTONS: &[(u8, &str)] = &[
    (0, "A (bottom)"),
    (1, "B (right)"),
    (2, "X (left)"),
    (3, "Y (top)"),
    (4, "View"),
    (5, "Xbox button"),
    (6, "Menu"),
    (7, "Left stick click"),
    (8, "Right stick click"),
    (9, "LB"),
    (10, "RB"),
    (11, "D-pad up"),
    (12, "D-pad down"),
    (13, "D-pad left"),
    (14, "D-pad right"),
    (15, "Share"),
    (100, "LT"),
    (101, "RT"),
];

fn default_remote() -> HashMap<String, Action> {
    let mut m = HashMap::new();
    m.insert("ok".into(), Action::A);
    m.insert("back".into(), Action::Menu);
    m.insert("red".into(), Action::B);
    m.insert("green".into(), Action::A);
    m.insert("yellow".into(), Action::Select);
    m.insert("blue".into(), Action::Start);
    m.insert("1".into(), Action::L);
    m.insert("3".into(), Action::R);
    m.insert("2".into(), Action::Select);
    m.insert("5".into(), Action::Start);
    m.insert("4".into(), Action::B);
    m.insert("6".into(), Action::A);
    m.insert("7".into(), Action::SaveState);
    m.insert("9".into(), Action::LoadState);
    m.insert("0".into(), Action::FastForward);
    m.insert("play".into(), Action::Start);
    m.insert("pause".into(), Action::Menu);
    m.insert("ff".into(), Action::FastForward);
    m
}

fn default_pad() -> HashMap<u8, Action> {
    // By position, like the real consoles: the right button is A, the bottom one is B.
    let mut m = HashMap::new();
    m.insert(1, Action::A);
    m.insert(0, Action::B);
    m.insert(3, Action::TurboA);
    m.insert(2, Action::TurboB);
    m.insert(4, Action::Select);
    m.insert(6, Action::Start);
    m.insert(5, Action::Menu);
    m.insert(9, Action::L);
    m.insert(10, Action::R);
    m.insert(11, Action::Up);
    m.insert(12, Action::Down);
    m.insert(13, Action::Left);
    m.insert(14, Action::Right);
    m.insert(101, Action::FastForward);
    m.insert(8, Action::Menu);
    m
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    // ---- Look (the New look's settings, same names and defaults as the phone) ----
    pub accent: String,
    pub background: String,
    pub background_style: String, // swirl, glow, plain
    pub background_moves: bool,
    pub swirl_speed: f64,
    pub swirl_strength: f64,
    pub swirl_source: String, // accent, rainbow, own
    pub swirl_own: String,
    pub button_light: bool,
    pub light_speed: f64,
    pub ring_mode: String, // rainbow, accent, custom
    pub ring: String,
    pub ring_spins: bool,
    pub glow_mode: String, // console, accent, off, custom
    pub glow: String,
    pub hero_glow: bool,
    pub box_floats: bool,
    pub cover_corner: f64,
    pub hide_titles: bool,
    pub focus_pulse: bool,
    pub focus_grow: bool,
    pub page_fades: bool,

    // ---- Picture ----
    pub scale: String, // fit, integer, stretch
    pub sharp: bool,
    pub lcd_grid: f64,
    pub game_backdrop: String, // black, swirl, dark
    pub show_fps: bool,

    // ---- Sound ----
    pub volume: u32,
    pub audio_latency_ms: u32,

    // ---- Controls ----
    pub remote: HashMap<String, Action>,
    pub pad: HashMap<u8, Action>,
    pub fast_speed: u32,
    pub turbo_rate: u32,
    pub stick_moves: bool,
    pub rumble: bool,
    pub menu_combo: bool,

    // ---- Saves ----
    pub auto_save: bool,
    pub auto_resume: bool,
    pub state_slots: u32,

    // ---- Library ----
    pub library_layout: String, // console, grid
    pub library_sort: String,   // name, added, played
    pub card_size: String,      // s, m, l
    pub show_home_shelves: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            accent: "FFD66B-F2B233".into(),
            background: "131316".into(),
            background_style: "swirl".into(),
            background_moves: true,
            swirl_speed: 1.0,
            swirl_strength: 0.7,
            swirl_source: "accent".into(),
            swirl_own: "AA00FF-FF0088-00CC66".into(),
            button_light: true,
            light_speed: 1.0,
            ring_mode: "rainbow".into(),
            ring: "FAC442".into(),
            ring_spins: true,
            glow_mode: "console".into(),
            glow: "FAC442".into(),
            hero_glow: true,
            box_floats: true,
            cover_corner: 0.48,
            hide_titles: false,
            focus_pulse: true,
            focus_grow: true,
            page_fades: true,
            scale: "fit".into(),
            sharp: true,
            lcd_grid: 0.0,
            game_backdrop: "black".into(),
            show_fps: false,
            volume: 100,
            audio_latency_ms: 80,
            remote: default_remote(),
            pad: default_pad(),
            fast_speed: 3,
            turbo_rate: 6,
            stick_moves: true,
            rumble: true,
            menu_combo: true,
            auto_save: true,
            auto_resume: true,
            state_slots: 9,
            library_layout: "console".into(),
            library_sort: "name".into(),
            card_size: "m".into(),
            show_home_shelves: true,
        }
    }
}

impl Settings {
    pub fn load(path: &str) -> Settings {
        let mut s: Settings = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        // Any key missing from an older file gets its default mapping.
        for (k, v) in default_remote() {
            s.remote.entry(k).or_insert(v);
        }
        s
    }

    pub fn save(&self, path: &str) {
        if let Ok(t) = serde_json::to_string_pretty(self) {
            crate::files::write_atomic(path, t.as_bytes());
        }
    }

    pub fn reset_look(&mut self) {
        let d = Settings::default();
        self.accent = d.accent;
        self.background = d.background;
        self.background_style = d.background_style;
        self.background_moves = d.background_moves;
        self.swirl_speed = d.swirl_speed;
        self.swirl_strength = d.swirl_strength;
        self.swirl_source = d.swirl_source;
        self.swirl_own = d.swirl_own;
        self.button_light = d.button_light;
        self.light_speed = d.light_speed;
        self.ring_mode = d.ring_mode;
        self.ring = d.ring;
        self.ring_spins = d.ring_spins;
        self.glow_mode = d.glow_mode;
        self.glow = d.glow;
        self.hero_glow = d.hero_glow;
        self.box_floats = d.box_floats;
        self.cover_corner = d.cover_corner;
        self.hide_titles = d.hide_titles;
        self.focus_pulse = d.focus_pulse;
        self.focus_grow = d.focus_grow;
        self.page_fades = d.page_fades;
    }

    pub fn reset_controls(&mut self) {
        self.remote = default_remote();
        self.pad = default_pad();
    }

    pub fn accent_stops(&self) -> Vec<String> {
        stops(&self.accent)
    }

    /// The swirl's colours, worked out the same way as the phone (NewLookColors.swirlRGB).
    pub fn swirl_colours(&self) -> Vec<[f64; 3]> {
        use crate::swirl::*;
        let hexes: Vec<String> = match self.swirl_source.as_str() {
            "rainbow" => {
                let start = hsb(hex_rgb(&self.accent_stops()[0])).0;
                (0..4).map(|i| rgb_hex(from_hsb((start + i as f64 / 4.0).rem_euclid(1.0), 0.72, 0.95))).collect()
            }
            "own" => stops(&self.swirl_own),
            _ => {
                let st = self.accent_stops();
                let first = st[0].clone();
                let second = if st.len() > 1 { st[1].clone() } else { shifted_hue(&first, -0.06) };
                let third = shifted_hue(&first, if st.len() > 1 { -0.08 } else { -0.12 });
                vec![first, second, third]
            }
        };
        hexes.iter().map(|h| hex_rgb(h)).collect()
    }
}

pub fn stops(hex: &str) -> Vec<String> {
    let v: Vec<String> = hex.split('-').filter(|s| s.len() >= 6).map(|s| s[..6].to_uppercase()).collect();
    if v.is_empty() {
        vec!["FFD66B".into()]
    } else {
        v
    }
}

pub const ACCENT_PRESETS: &[&str] = &[
    "FFD66B-F2B233",
    "5AA9FF-2F5BFF",
    "FF5E9C-FF9F43",
    "A66BFF-FF5FB0",
    "6B6B73-D9D9DE",
    "3EE6C1-1E8BE0",
    "FF6A3D-D7263D",
    "9BE15D-00A86B",
];

pub const BACKGROUND_PRESETS: &[&str] = &["131316", "000000", "0E1424", "1A1022", "0F1A14", "1C1C1E"];

/// Swirl colour sets to pick from for "My own" (the TV has no colour wheel).
pub const OWN_SWIRL_PRESETS: &[&str] = &[
    "AA00FF-FF0088-00CC66",
    "FF5E9C-FFB86B-7A5CFF",
    "00C2FF-7A5CFF-FF4FD8",
    "FF6A3D-FFD66B-D7263D",
    "3EE6C1-1E8BE0-9BE15D",
    "FF4FD8-FFD66B-3EE6C1",
];

pub const SINGLE_COLOURS: &[&str] =
    &["FAC442", "FFFFFF", "FF5E9C", "5AA9FF", "A66BFF", "3EE6C1", "FF6A3D", "9BE15D"];
