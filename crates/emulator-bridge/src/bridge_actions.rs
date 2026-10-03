//! The in-game actions that are not plain settings: speed presets and slow motion, the Atari 2600
//! console switches, disc swaps (including the Famicom Disk System's side flip), rotation, the
//! post-process look, and the TV's own fit and layout.
//!
//! A child module of `bridge` so it can reach the bridge's private fields, the way `netplay_glue`
//! does. Everything here is engine behaviour so Android gets it for free; Swift only names actions.

use std::collections::VecDeque;

use super::EmulatorBridge;
use crate::cores::options;
use crate::gfx::screen_layout::DualLayout;
use crate::gfx::{PostSettings, Renderer, ScaleMode};
use crate::input::{Button, InputSnapshot};

/// The fast-forward cycle: 1x, 2x, 3x, 4x. 4x is the pacer's real ceiling on a 60 Hz screen
/// (`MAX_CATCH_UP_STEPS`), so nothing above it is offered.
pub const FAST_FORWARD_STEPS: [f64; 4] = [1.0, 2.0, 3.0, 4.0];

/// The slow-motion cycle after "off".
pub const SLOW_MOTION_STEPS: [f64; 2] = [0.5, 0.25];

/// One button press the engine makes on the user's behalf: held for `hold` core frames, then
/// released for `gap` frames before the next one starts, so edge-triggered cores see each press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pulse {
    pub port: usize,
    pub mask: u32,
    pub hold: u32,
    pub gap: u32,
}

impl Pulse {
    pub fn button(port: usize, button: Button) -> Self {
        Self {
            port,
            mask: 1 << button as u32,
            hold: 3,
            gap: 3,
        }
    }
}

/// What the actions remember. Survives sessions where that is what a user expects (the speed
/// preset, the look, the TV choices) and is reset per game where it is not (switches, rotation).
#[derive(Debug, Clone)]
pub struct ActionState {
    base_speed: f64,
    hold_speed: Option<f64>,
    slow_motion: Option<f64>,
    pulses: VecDeque<Pulse>,
    manual_rotation: u32,
    post: PostSettings,
    tv_scale: Option<ScaleMode>,
    tv_layout: Option<DualLayout>,
    /// The running game's file name, for the FDS check.
    content_name: String,
    /// Atari 2600 switches as the engine last set them. Stella's own defaults are colour and B/B.
    atari_color: bool,
    atari_left_a: bool,
    atari_right_a: bool,
}

impl Default for ActionState {
    fn default() -> Self {
        Self {
            base_speed: 1.0,
            hold_speed: None,
            slow_motion: None,
            pulses: VecDeque::new(),
            manual_rotation: 0,
            post: PostSettings::default(),
            tv_scale: None,
            tv_layout: None,
            content_name: String::new(),
            atari_color: true,
            atari_left_a: false,
            atari_right_a: false,
        }
    }
}

impl ActionState {
    /// The speed in force: a held button wins, then slow motion, then the cycle's preset.
    pub fn effective_speed(&self) -> f64 {
        self.hold_speed.or(self.slow_motion).unwrap_or(self.base_speed)
    }

    /// A new game: the per-game state goes, the preferences stay.
    pub fn new_session(&mut self, content_name: &str) {
        self.hold_speed = None;
        self.pulses.clear();
        self.manual_rotation = 0;
        self.content_name = content_name.to_owned();
        self.atari_color = true;
        self.atari_left_a = false;
        self.atari_right_a = false;
    }

    /// Applies at most one queued press to this core frame.
    pub fn pulse_step(&mut self, snapshot: &mut InputSnapshot) {
        loop {
            let Some(front) = self.pulses.front_mut() else {
                return;
            };
            if front.hold > 0 {
                if let Some(port) = snapshot.ports.get_mut(front.port) {
                    port.buttons |= front.mask;
                }
                front.hold -= 1;
                return;
            }
            if front.gap > 0 {
                front.gap -= 1;
                return;
            }
            self.pulses.pop_front();
        }
    }

    pub fn pulses_pending(&self) -> usize {
        self.pulses.len()
    }

    pub fn push_to(&self, renderer: &mut Renderer, core_rotation: u32) {
        renderer.set_post(self.post);
        renderer.set_rotation((core_rotation + self.manual_rotation) % 4);
        renderer.set_tv_overrides(self.tv_scale, self.tv_layout);
    }

    fn is_fds(&self) -> bool {
        self.content_name.to_ascii_lowercase().ends_with(".fds")
    }
}

/// The option that holds a system's palette, in the order tried. The first one the running core
/// declared is used. Read from each core's own source: mGBA `mgba_gb_colors` (Game Boy palettes,
/// filled in at run time from its preset list), `mgba_color_correction` for Game Boy Color colour,
/// `gambatte_gb_internal_palette` if Gambatte is ever the core, `vb_color_mode` in Beetle VB, and
/// `fceumm_palette` for the NES as a bonus.
pub fn palette_keys(system: &str) -> &'static [&'static str] {
    match system {
        "gb" => &["mgba_gb_colors", "gambatte_gb_internal_palette", "sameboy_dmg_palette"],
        "gbc" => &["mgba_color_correction", "gambatte_gbc_color_correction", "mgba_gb_colors"],
        "vb" => &["vb_color_mode"],
        "nes" | "fds" => &["fceumm_palette", "nestopia_palette"],
        "pokemini" => &["pokemini_palette"],
        _ => &[],
    }
}

/// The internal-resolution option per core, read from each core's source: PPSSPP
/// `ppsspp_internal_resolution`, Azahar `citra_resolution_factor`, parallel_n64
/// `parallel-n64-upscaling` (and the older `-screensize`), Beetle PSX HW
/// `beetle_psx_hw_internal_resolution`, melonDS `melonds_opengl_resolution` (only declared by an
/// OpenGL build, so absent from this app's), and a few others for cores other workers add.
pub const RESOLUTION_KEYS: [&str; 10] = [
    "ppsspp_internal_resolution",
    "citra_resolution_factor",
    "parallel-n64-upscaling",
    "parallel-n64-screensize",
    "beetle_psx_hw_internal_resolution",
    "beetle_psx_internal_resolution",
    "melonds_opengl_resolution",
    "flycast_internal_resolution",
    "yabasanshiro_resolution_mode",
    "mupen64plus-43screensize",
];

/// The first candidate the core actually declared.
fn first_declared(core_id: &str, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find(|key| options::definition(core_id, key).is_some())
        .map(|key| (*key).to_owned())
}

/// The resolution key for a core, if it has one.
pub fn resolution_key(core_id: &str) -> Option<String> {
    first_declared(core_id, &RESOLUTION_KEYS)
}

/// The palette key for a core on a system, if it has one.
pub fn palette_key(core_id: &str, system: &str) -> Option<String> {
    first_declared(core_id, palette_keys(system))
}

/// The next value in a cycle of presets, wrapping. A value not in the list starts it again.
pub fn next_in(steps: &[f64], now: f64) -> f64 {
    let index = steps.iter().position(|s| (s - now).abs() < 1e-6);
    match index {
        Some(i) => steps[(i + 1) % steps.len()],
        None => steps[0],
    }
}

pub fn speed_label(speed: f64) -> String {
    if (speed - speed.round()).abs() < 1e-6 {
        format!("{}x", speed.round() as u32)
    } else {
        format!("{speed}x")
    }
}

impl EmulatorBridge {
    fn apply_action_speed(&mut self) -> f64 {
        let wanted = self.actions.effective_speed();
        self.set_speed(wanted);
        self.speed()
    }

    /// The cycle action: 1x, 2x, 3x, 4x, back to 1x. Clears slow motion.
    pub fn cycle_fast_forward(&mut self) -> String {
        if self.netplay_is_live() {
            return "fast forward is off during online play".into();
        }
        self.actions.slow_motion = None;
        self.actions.base_speed = next_in(&FAST_FORWARD_STEPS, self.actions.base_speed);
        let speed = self.apply_action_speed();
        format!("speed {}", speed_label(speed))
    }

    /// The hold buttons (2x, 3x, 4x, or any multiplier). Releasing returns to the preset.
    pub fn set_hold_speed(&mut self, multiplier: f64, held: bool) -> String {
        if self.netplay_is_live() {
            return "fast forward is off during online play".into();
        }
        self.actions.hold_speed = if held && multiplier.is_finite() && multiplier > 0.0 {
            Some(multiplier.clamp(0.25, 4.0))
        } else {
            None
        };
        let speed = self.apply_action_speed();
        if held {
            format!("fast forward {}", speed_label(speed))
        } else {
            format!("speed back to {}", speed_label(speed))
        }
    }

    /// Off, 0.5x, 0.25x, off. The pacer runs below 1x and audio is resampled at the same ratio,
    /// the way fast forward is, so the sound slows and drops in pitch with the picture.
    pub fn toggle_slow_motion(&mut self) -> String {
        if self.netplay_is_live() {
            return "slow motion is off during online play".into();
        }
        self.actions.slow_motion = match self.actions.slow_motion {
            None => Some(SLOW_MOTION_STEPS[0]),
            Some(now) => {
                let i = SLOW_MOTION_STEPS.iter().position(|s| (s - now).abs() < 1e-6);
                match i {
                    Some(i) if i + 1 < SLOW_MOTION_STEPS.len() => Some(SLOW_MOTION_STEPS[i + 1]),
                    _ => None,
                }
            }
        };
        let speed = self.apply_action_speed();
        match self.actions.slow_motion {
            Some(_) => format!("slow motion {}", speed_label(speed)),
            None => format!("slow motion off, speed {}", speed_label(speed)),
        }
    }

    pub fn slow_motion(&self) -> Option<f64> {
        self.actions.slow_motion
    }

    pub fn speed_preset(&self) -> f64 {
        self.actions.base_speed
    }

    // ------------------------------------------------------------------ look

    pub fn set_post_settings(&mut self, post: PostSettings) {
        self.actions.post = post.sanitized();
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.set_post(self.actions.post);
        }
    }

    pub fn post_settings(&self) -> PostSettings {
        self.actions.post
    }

    // ------------------------------------------------------------------ rotation

    /// The user's rotate action: one more quarter turn counter-clockwise, on top of the core's own.
    pub fn rotate_screen(&mut self) -> String {
        if self.has_dual_screens() {
            return "rotation is for one-screen systems; use the layout for the DS and 3DS".into();
        }
        self.actions.manual_rotation = (self.actions.manual_rotation + 1) % 4;
        self.push_actions_to_renderer();
        let total = self.screen_rotation();
        format!("picture turned to {} degrees", total * 90)
    }

    /// Total quarter turns in force: the core's request plus the user's.
    pub fn screen_rotation(&self) -> u32 {
        let core = self.session.as_ref().map_or(0, |s| s.core.rotation());
        (core + self.actions.manual_rotation) % 4
    }

    // ------------------------------------------------------------------ TV

    pub fn set_tv_scale_mode(&mut self, mode: Option<ScaleMode>) {
        self.actions.tv_scale = mode;
        self.push_actions_to_renderer();
    }

    pub fn tv_scale_mode(&self) -> Option<ScaleMode> {
        self.actions.tv_scale
    }

    pub fn set_tv_layout(&mut self, layout: Option<DualLayout>) {
        self.actions.tv_layout = layout;
        self.push_actions_to_renderer();
    }

    pub fn tv_layout(&self) -> Option<DualLayout> {
        self.actions.tv_layout
    }

    /// Re-applies everything here to the renderer. Called on attach, on launch and every tick (the
    /// core may call SET_ROTATION mid-game; the writes are plain field stores).
    pub(super) fn push_actions_to_renderer(&mut self) {
        let core_rotation = self.session.as_ref().map_or(0, |s| s.core.rotation());
        if let Some(renderer) = self.renderer.as_mut() {
            self.actions.push_to(renderer, core_rotation);
        }
    }

    // ------------------------------------------------------------------ core settings helpers

    fn session_core(&self) -> Option<String> {
        self.session.as_ref().map(|s| s.core_id.clone())
    }

    /// Asks the running core to refresh which options it shows, under the engine lock.
    pub fn refresh_option_visibility(&mut self, core_id: &str) -> bool {
        match self.session.as_mut() {
            Some(session) if session.core_id == core_id => session.core.refresh_option_visibility(),
            _ => false,
        }
    }

    /// The palette cycle for the running game on `system`.
    pub fn cycle_palette(&mut self, system: &str) -> String {
        let Some(core) = self.session_core() else {
            return "no game is running".into();
        };
        let Some(key) = palette_key(&core, system) else {
            return format!("{core} has no palette setting for this system");
        };
        match options::cycle(&core, &key, true) {
            Ok((_, label)) => format!("palette: {label}"),
            Err(err) => err,
        }
    }

    /// The internal-resolution cycle for the running core.
    pub fn cycle_resolution(&mut self) -> String {
        let Some(core) = self.session_core() else {
            return "no game is running".into();
        };
        let Some(key) = resolution_key(&core) else {
            return format!("{core} has no internal resolution setting");
        };
        let restart = options::definition(&core, &key).is_some_and(|d| d.says_restart());
        match options::cycle(&core, &key, true) {
            Ok((_, label)) if restart => format!("resolution: {label} (restart the game to see it)"),
            Ok((_, label)) => format!("resolution: {label}"),
            Err(err) => err,
        }
    }

    // ------------------------------------------------------------------ Atari 2600

    fn is_stella(&self) -> bool {
        self.session.as_ref().is_some_and(|s| s.core_id.starts_with("stella"))
    }

    /// Stella's TV type switch. stella2023 reads it as INPUT, not as an option: port 0 L3 sets
    /// colour and R3 sets black and white (libretro.cxx, `Event::ConsoleColor` /
    /// `ConsoleBlackWhite`), latched by `Switches::update`. So the engine presses the one wanted.
    pub fn toggle_tv_type(&mut self) -> String {
        if !self.is_stella() {
            return "the TV type switch is an Atari 2600 control".into();
        }
        let color = !self.actions.atari_color;
        self.actions.atari_color = color;
        let button = if color { Button::L3 } else { Button::R3 };
        self.actions.pulses.push_back(Pulse::button(0, button));
        if color { "TV type: colour".into() } else { "TV type: black and white".into() }
    }

    pub fn tv_type_is_color(&self) -> bool {
        self.actions.atari_color
    }

    /// Stella's difficulty switches: L sets the left one to A, L2 to B; R and R2 the right one
    /// (libretro.cxx, `Event::ConsoleLeftDiffA` and the rest, latched by `Switches::update`).
    pub fn toggle_difficulty(&mut self, left: bool) -> String {
        if !self.is_stella() {
            return "the difficulty switches are Atari 2600 controls".into();
        }
        let a = if left {
            self.actions.atari_left_a = !self.actions.atari_left_a;
            self.actions.atari_left_a
        } else {
            self.actions.atari_right_a = !self.actions.atari_right_a;
            self.actions.atari_right_a
        };
        let button = match (left, a) {
            (true, true) => Button::L,
            (true, false) => Button::L2,
            (false, true) => Button::R,
            (false, false) => Button::R2,
        };
        self.actions.pulses.push_back(Pulse::button(0, button));
        format!(
            "{} difficulty: {}",
            if left { "left" } else { "right" },
            if a { "A (hard)" } else { "B (easy)" }
        )
    }

    pub fn difficulty_is_a(&self, left: bool) -> bool {
        if left {
            self.actions.atari_left_a
        } else {
            self.actions.atari_right_a
        }
    }

    // ------------------------------------------------------------------ discs

    /// The running game's disc table, if its core has one.
    pub fn disk_status(&self) -> Option<crate::cores::disk::DiskStatus> {
        self.session.as_ref().and_then(|s| s.core.disk_status())
    }

    /// Whether the running game is on the Famicom Disk System through FCEUmm, which flips sides
    /// with buttons rather than through the disk interface.
    pub fn is_fds_session(&self) -> bool {
        self.session.as_ref().is_some_and(|s| s.core_id == "fceumm") && self.actions.is_fds()
    }

    /// Next disc. For the FDS: eject (R), next side (L), insert (R), because FCEUmm refuses to
    /// select a side with the disk in.
    pub fn swap_disc(&mut self) -> String {
        if self.session.is_none() {
            return "no game is running".into();
        }
        if self.is_fds_session() {
            for button in [Button::R, Button::L, Button::R] {
                self.actions.pulses.push_back(Pulse::button(0, button));
            }
            return "disk: ejected, turned to the next side, inserted".into();
        }
        let Some(status) = self.disk_status() else {
            return "this game's core has no disc swapping".into();
        };
        if status.count < 2 {
            return format!("this game has {} disc, nothing to swap to", status.count);
        }
        self.insert_disc((status.index + 1) % status.count)
    }

    /// A specific disc. For the FDS there is no list, so this ejects or inserts the disk.
    pub fn insert_disc(&mut self, index: u32) -> String {
        if self.is_fds_session() {
            self.actions.pulses.push_back(Pulse::button(0, Button::R));
            return "disk: ejected or inserted".into();
        }
        let Some(session) = self.session.as_mut() else {
            return "no game is running".into();
        };
        match session.core.disk_insert(index) {
            Ok(line) => line,
            Err(err) => err,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::PortState;

    #[test]
    fn the_fast_forward_cycle_wraps() {
        assert_eq!(next_in(&FAST_FORWARD_STEPS, 1.0), 2.0);
        assert_eq!(next_in(&FAST_FORWARD_STEPS, 4.0), 1.0);
        assert_eq!(next_in(&FAST_FORWARD_STEPS, 1.7), 1.0);
    }

    #[test]
    fn hold_beats_slow_motion_beats_the_preset() {
        let mut a = ActionState::default();
        assert_eq!(a.effective_speed(), 1.0);
        a.base_speed = 3.0;
        assert_eq!(a.effective_speed(), 3.0);
        a.slow_motion = Some(0.5);
        assert_eq!(a.effective_speed(), 0.5);
        a.hold_speed = Some(4.0);
        assert_eq!(a.effective_speed(), 4.0);
        a.new_session("x.nes");
        assert_eq!(a.effective_speed(), 0.5, "slow motion is a preference, the hold is not");
    }

    #[test]
    fn pulses_hold_then_release_then_the_next_one() {
        let mut a = ActionState::default();
        a.pulses.push_back(Pulse::button(0, Button::R));
        a.pulses.push_back(Pulse::button(0, Button::L));
        let mut pressed = Vec::new();
        for _ in 0..14 {
            let mut snap = InputSnapshot { ports: [PortState::default(); crate::input::MAX_PORTS] };
            a.pulse_step(&mut snap);
            pressed.push(snap.ports[0].buttons);
        }
        let r = 1 << Button::R as u32;
        let l = 1 << Button::L as u32;
        assert_eq!(&pressed[0..3], &[r, r, r]);
        assert_eq!(&pressed[3..6], &[0, 0, 0], "released long enough for an edge");
        assert_eq!(&pressed[6..9], &[l, l, l]);
        assert!(pressed[9..].iter().all(|b| *b == 0));
        assert_eq!(a.pulses_pending(), 0);
    }

    #[test]
    fn speed_labels_read_naturally() {
        assert_eq!(speed_label(2.0), "2x");
        assert_eq!(speed_label(0.25), "0.25x");
    }

    #[test]
    fn palette_and_resolution_candidates_come_from_the_core_tables() {
        let _g = options::test_guard();
        options::reset_for_tests();
        options::install("mgba", None, true);
        options::declare(crate::cores::options::OptionTable {
            version: 0,
            categories: vec![],
            defs: vec![options::parse_v0_value("mgba_gb_colors", "Palette; Grayscale|DMG Green")],
        });
        assert_eq!(palette_key("mgba", "gb").as_deref(), Some("mgba_gb_colors"));
        assert_eq!(palette_key("mgba", "gba"), None);
        assert_eq!(resolution_key("mgba"), None);
        options::install("ppsspp", None, true);
        options::declare(crate::cores::options::OptionTable {
            version: 0,
            categories: vec![],
            defs: vec![options::parse_v0_value("ppsspp_internal_resolution", "Res; 480x272|960x544")],
        });
        assert_eq!(resolution_key("ppsspp").as_deref(), Some("ppsspp_internal_resolution"));
        options::reset_for_tests();
    }
}
