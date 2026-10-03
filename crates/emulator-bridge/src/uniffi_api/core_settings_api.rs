//! The Swift-facing half of core settings, post-process looks, palettes, speeds, discs, rotation and
//! the TV's own fit and layout.
//!
//! A child of `uniffi_api` so it can use the engine's private `lock()`. Nothing in here is logic:
//! every function converts a type and delegates to `cores::options` or `bridge_actions`, which is
//! where Android will find the same behaviour. Every exported method is unconditional, because
//! `#[uniffi::export]` ignores `#[cfg]` on individual methods.

use super::{ContinuumEngine, EngineError, ScaleModeOption, ScreenLayoutOption};
use crate::cores::options;
use crate::gfx::{PostEffect, PostSettings};

/// One value a core option can take.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CoreOptionChoice {
    pub value: String,
    pub label: String,
}

/// One group of options, from a v2 table. Options with an empty category belong to none.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CoreOptionCategoryRecord {
    pub key: String,
    pub label: String,
    pub info: String,
}

/// One option as the settings screen shows it.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CoreOptionEntry {
    pub key: String,
    pub label: String,
    pub info: String,
    /// The category key, empty for none.
    pub category: String,
    pub values: Vec<CoreOptionChoice>,
    pub current: String,
    pub default_value: String,
    /// The core asked for it to be hidden right now (SET_CORE_OPTIONS_DISPLAY).
    pub visible: bool,
    /// The current value is this game's own, not the core-wide one.
    pub game_override: bool,
    /// Changed, and the core has not picked it up: the game needs a restart to see it.
    pub needs_restart: bool,
}

/// A post-process look. Mirrors [`crate::gfx::PostEffect`] for the reason `ScaleModeOption`
/// mirrors its gfx counterpart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PostEffectOption {
    None,
    Smooth,
    SharpBilinear,
    Scanlines,
    Crt,
    LcdGrid,
    DotMatrix,
}

impl From<PostEffectOption> for PostEffect {
    fn from(e: PostEffectOption) -> Self {
        match e {
            PostEffectOption::None => Self::None,
            PostEffectOption::Smooth => Self::Smooth,
            PostEffectOption::SharpBilinear => Self::SharpBilinear,
            PostEffectOption::Scanlines => Self::Scanlines,
            PostEffectOption::Crt => Self::Crt,
            PostEffectOption::LcdGrid => Self::LcdGrid,
            PostEffectOption::DotMatrix => Self::DotMatrix,
        }
    }
}

impl From<PostEffect> for PostEffectOption {
    fn from(e: PostEffect) -> Self {
        match e {
            PostEffect::None => Self::None,
            PostEffect::Smooth => Self::Smooth,
            PostEffect::SharpBilinear => Self::SharpBilinear,
            PostEffect::Scanlines => Self::Scanlines,
            PostEffect::Crt => Self::Crt,
            PostEffect::LcdGrid => Self::LcdGrid,
            PostEffect::DotMatrix => Self::DotMatrix,
        }
    }
}

/// The look and its strengths. Values outside the ranges are clamped by the engine.
#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct PostEffectSettings {
    pub effect: PostEffectOption,
    /// 0.2..=2.0, 1.0 unchanged.
    pub brightness: f32,
    /// 0..=0.3.
    pub curvature: f32,
    /// 0..=1: scanline darkness, or the grid's.
    pub line_strength: f32,
    /// 0..=1.
    pub mask_strength: f32,
    /// 0..=1.
    pub vignette: f32,
}

impl From<PostSettings> for PostEffectSettings {
    fn from(p: PostSettings) -> Self {
        Self {
            effect: p.effect.into(),
            brightness: p.brightness,
            curvature: p.curvature,
            line_strength: p.line_strength,
            mask_strength: p.mask_strength,
            vignette: p.vignette,
        }
    }
}

impl From<PostEffectSettings> for PostSettings {
    fn from(p: PostEffectSettings) -> Self {
        Self {
            effect: p.effect.into(),
            brightness: p.brightness,
            curvature: p.curvature,
            line_strength: p.line_strength,
            mask_strength: p.mask_strength,
            vignette: p.vignette,
        }
    }
}

/// A look and its plain name, for a picker.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PostEffectInfo {
    pub effect: PostEffectOption,
    pub label: String,
}

/// The running game's discs.
#[derive(Debug, Clone, uniffi::Record)]
pub struct DiscStatusRecord {
    pub count: u32,
    pub index: u32,
    pub ejected: bool,
    pub labels: Vec<String>,
    /// A Famicom Disk System game, whose side flips with buttons and has no list.
    pub fds: bool,
}

fn option_error(reason: String) -> EngineError {
    EngineError::CoreOption { reason }
}

#[uniffi::export]
impl ContinuumEngine {
    // ---------------------------------------------------------------- core settings

    /// Where per-core and per-game `.opt` files live. Call once at start, before any game.
    pub fn set_core_options_directory(&self, path: String) {
        let _engine = self.lock();
        options::set_root(if path.is_empty() { None } else { Some(path.into()) });
    }

    /// Whether the core's options are known, live or from a previous run.
    pub fn core_options_known(&self, core_id: String) -> bool {
        options::has_table(&core_id)
    }

    pub fn core_option_categories(&self, core_id: String) -> Vec<CoreOptionCategoryRecord> {
        options::categories(&core_id)
            .into_iter()
            .map(|c| CoreOptionCategoryRecord { key: c.key, label: c.label, info: c.info })
            .collect()
    }

    /// Every option, in the core's order. For the running core, the core is first asked to
    /// refresh which options it shows (its update-display callback), under the engine lock.
    pub fn core_option_entries(&self, core_id: String) -> Vec<CoreOptionEntry> {
        {
            let mut engine = self.lock();
            engine.refresh_option_visibility(&core_id);
        }
        options::list(&core_id)
            .into_iter()
            .map(|v| CoreOptionEntry {
                key: v.key,
                label: v.label,
                info: v.info,
                category: v.category,
                values: v
                    .values
                    .into_iter()
                    .map(|c| CoreOptionChoice { value: c.value, label: c.label })
                    .collect(),
                current: v.current,
                default_value: v.default,
                visible: v.visible,
                game_override: v.game_override,
                needs_restart: v.needs_restart,
            })
            .collect()
    }

    /// Stores a choice, core-wide or for the running game, and tells a running core. The returned
    /// line is for the status bar.
    pub fn set_core_option_value(
        &self,
        core_id: String,
        key: String,
        value: String,
        for_game: bool,
    ) -> Result<String, EngineError> {
        let _engine = self.lock();
        options::set(&core_id, &key, &value, for_game).map_err(option_error)
    }

    pub fn reset_core_options(&self, core_id: String) -> Result<String, EngineError> {
        let _engine = self.lock();
        options::reset_core(&core_id).map_err(option_error)
    }

    pub fn reset_game_options(&self, core_id: String) -> Result<String, EngineError> {
        let _engine = self.lock();
        options::reset_game(&core_id).map_err(option_error)
    }

    /// The game whose own choices are live for this core, if one is running.
    pub fn core_options_game(&self, core_id: String) -> Option<String> {
        options::active_game(&core_id)
    }

    // ---------------------------------------------------------------- speeds

    /// 1x, 2x, 3x, 4x, back to 1x. Returns the status line.
    pub fn cycle_fast_forward(&self) -> String {
        self.lock().cycle_fast_forward()
    }

    /// A held fast-forward button at `multiplier`; releasing returns to the preset.
    pub fn set_hold_speed(&self, multiplier: f64, held: bool) -> String {
        self.lock().set_hold_speed(multiplier, held)
    }

    /// Off, 0.5x, 0.25x, off.
    pub fn toggle_slow_motion(&self) -> String {
        self.lock().toggle_slow_motion()
    }

    /// The slow-motion speed, 0 when off.
    pub fn slow_motion_speed(&self) -> f64 {
        self.lock().slow_motion().unwrap_or(0.0)
    }

    /// The fast-forward cycle's current preset.
    pub fn speed_preset(&self) -> f64 {
        self.lock().speed_preset()
    }

    // ---------------------------------------------------------------- looks

    pub fn set_post_effect(&self, settings: PostEffectSettings) {
        self.lock().set_post_settings(settings.into());
    }

    pub fn post_effect(&self) -> PostEffectSettings {
        self.lock().post_settings().into()
    }

    /// The defaults for each strength, with `effect` set.
    pub fn post_effect_defaults(&self, effect: PostEffectOption) -> PostEffectSettings {
        PostEffectSettings::from(PostSettings { effect: effect.into(), ..PostSettings::default() })
    }

    pub fn post_effects(&self) -> Vec<PostEffectInfo> {
        PostEffect::ALL
            .iter()
            .map(|e| PostEffectInfo { effect: (*e).into(), label: e.label().to_owned() })
            .collect()
    }

    /// The look suggested for a system id.
    pub fn suggested_post_effect(&self, system: String) -> PostEffectOption {
        PostEffect::suggested_for(&system).into()
    }

    // ---------------------------------------------------------------- palettes and resolution

    pub fn cycle_palette(&self, system: String) -> String {
        self.lock().cycle_palette(&system)
    }

    pub fn cycle_resolution(&self) -> String {
        self.lock().cycle_resolution()
    }

    // ---------------------------------------------------------------- rotation

    /// One more quarter turn, counter-clockwise, on top of what the core asked for.
    pub fn rotate_screen(&self) -> String {
        self.lock().rotate_screen()
    }

    /// Quarter turns in force, core plus user, 0..=3.
    pub fn screen_rotation(&self) -> u32 {
        self.lock().screen_rotation()
    }

    // ---------------------------------------------------------------- TV

    /// The TV's own fit. `None` follows the phone.
    pub fn set_tv_scale_mode(&self, mode: Option<ScaleModeOption>) {
        self.lock().set_tv_scale_mode(mode.map(Into::into));
    }

    pub fn tv_scale_mode(&self) -> Option<ScaleModeOption> {
        self.lock().tv_scale_mode().map(Into::into)
    }

    /// The TV's own two-screen layout. `None` follows the phone.
    pub fn set_tv_layout(&self, layout: Option<ScreenLayoutOption>) {
        self.lock().set_tv_layout(layout.map(Into::into));
    }

    pub fn tv_layout(&self) -> Option<ScreenLayoutOption> {
        self.lock().tv_layout().map(Into::into)
    }

    // ---------------------------------------------------------------- Atari 2600

    pub fn toggle_tv_type(&self) -> String {
        self.lock().toggle_tv_type()
    }

    pub fn tv_type_is_color(&self) -> bool {
        self.lock().tv_type_is_color()
    }

    pub fn toggle_difficulty(&self, left: bool) -> String {
        self.lock().toggle_difficulty(left)
    }

    pub fn difficulty_is_a(&self, left: bool) -> bool {
        self.lock().difficulty_is_a(left)
    }

    // ---------------------------------------------------------------- discs

    /// The running game's discs, or `None` when its core has no disc control.
    pub fn disc_status(&self) -> Option<DiscStatusRecord> {
        let engine = self.lock();
        if engine.is_fds_session() {
            return Some(DiscStatusRecord {
                count: 0,
                index: 0,
                ejected: false,
                labels: Vec::new(),
                fds: true,
            });
        }
        engine.disk_status().map(|s| DiscStatusRecord {
            count: s.count,
            index: s.index,
            ejected: s.ejected,
            labels: s.labels,
            fds: false,
        })
    }

    pub fn swap_disc(&self) -> String {
        self.lock().swap_disc()
    }

    pub fn insert_disc(&self, index: u32) -> String {
        self.lock().insert_disc(index)
    }
}
