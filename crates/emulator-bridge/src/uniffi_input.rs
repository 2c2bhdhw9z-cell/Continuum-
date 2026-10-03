//! The Swift-facing half of the input features: keyboard, motion, controller type per port,
//! remap profiles, and the console actions (shake, analog, DS lid, blow, 3DS HOME).
//!
//! A child module of `uniffi_api` (see the `#[path]` there) so it can use `ContinuumEngine::lock`,
//! in its own `#[uniffi::export]` block so it merges cleanly beside other work.
//!
//! THE MOTION SAMPLE METHODS TAKE NO ENGINE LOCK. CoreMotion calls `push_motion` from its own
//! queue up to 100 times a second while the display link holds the lock for each whole tick; see
//! `input::sensors`. Everything else here is a tap or a key press, on the main thread.

use super::{ContinuumEngine, InputSource};
use crate::input::remap::{all_targets, RemapProfile, RemapTable, Target};
use crate::input::sensors::{ScreenOrientation, SENSORS};
use crate::input::{keyboard, Button, MAX_PORTS};

/// One remap profile, as the editor sees it.
///
/// Each map has 16 entries indexed by the retro button a layer produced (B, Y, Select, Start, Up,
/// Down, Left, Right, A, X, L, R, L2, R2, L3, R3; labels from `remap_button_labels`), and each
/// entry is a target code from `remap_targets`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct RemapProfileRecord {
    pub name: String,
    pub system: String,
    /// Empty for every game of the system.
    pub game: String,
    pub gamepad_map: Vec<u32>,
    pub gamepad_swap_ab: bool,
    pub gamepad_stick_to_dpad: bool,
    pub gamepad_deadzone: f32,
    pub touch_map: Vec<u32>,
    pub touch_swap_ab: bool,
    pub touch_stick_to_dpad: bool,
    pub touch_deadzone: f32,
}

/// One thing a button can be mapped to.
#[derive(Debug, Clone, uniffi::Record)]
pub struct RemapTargetRecord {
    pub code: u32,
    pub label: String,
}

/// What a port can be set to, and what it is set to.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ControllerPortRecord {
    pub port: u32,
    /// The device types the running core declared for this port, by name. Empty when none.
    pub choices: Vec<String>,
    /// What the port is set to right now.
    pub current: String,
}

/// Host-side actions pressed on remapped buttons since the last call.
#[derive(Debug, Clone, uniffi::Record)]
pub struct HostInputActions {
    /// Stable names: "keyboard", "menu".
    pub actions: Vec<String>,
    /// The line the last engine-side action wrote (shake, lid, profile...), empty for none.
    pub line: String,
}

fn table_from(map: &[u32], swap_ab: bool, stick_to_dpad: bool, deadzone: f32) -> RemapTable {
    let mut table = RemapTable {
        swap_ab,
        stick_to_dpad,
        deadzone,
        ..RemapTable::default()
    };
    for (slot, code) in table.map.iter_mut().zip(map) {
        *slot = Target::from_code(u8::try_from(*code).unwrap_or(Target::NONE_CODE)).code();
    }
    table.clamp();
    table
}

fn record_from(profile: &RemapProfile) -> RemapProfileRecord {
    RemapProfileRecord {
        name: profile.name.clone(),
        system: profile.system.clone(),
        game: profile.game.clone(),
        gamepad_map: profile.gamepad.map.iter().map(|c| u32::from(*c)).collect(),
        gamepad_swap_ab: profile.gamepad.swap_ab,
        gamepad_stick_to_dpad: profile.gamepad.stick_to_dpad,
        gamepad_deadzone: profile.gamepad.deadzone,
        touch_map: profile.touch.map.iter().map(|c| u32::from(*c)).collect(),
        touch_swap_ab: profile.touch.swap_ab,
        touch_stick_to_dpad: profile.touch.stick_to_dpad,
        touch_deadzone: profile.touch.deadzone,
    }
}

fn profile_from(record: &RemapProfileRecord) -> RemapProfile {
    RemapProfile {
        name: record.name.clone(),
        system: record.system.clone(),
        game: record.game.clone(),
        gamepad: table_from(
            &record.gamepad_map,
            record.gamepad_swap_ab,
            record.gamepad_stick_to_dpad,
            record.gamepad_deadzone,
        ),
        touch: table_from(
            &record.touch_map,
            record.touch_swap_ab,
            record.touch_stick_to_dpad,
            record.touch_deadzone,
        ),
    }
}

#[uniffi::export]
impl ContinuumEngine {
    // ------------------------------------------------------------------ keyboard

    /// One key from a keyboard: `.keyboard` for a hardware keyboard, `.touch` for the on-screen
    /// one. `keycode` is libretro's `retro_key` (RETROK_*); `character` the UTF-32 it typed, 0 for
    /// none. Polled by the core from the next frame, and delivered to its keyboard callback on
    /// the core's thread before that frame runs.
    pub fn key_event(&self, source: InputSource, keycode: u32, down: bool, character: u32) {
        self.lock().key_event(source.into(), keycode, down, character);
    }

    /// A plain line saying whether the running core listens to the keyboard.
    pub fn keyboard_status(&self) -> String {
        if keyboard::has_callback() {
            format!(
                "keyboard: the core listens for keys ({} delivered)",
                keyboard::delivered_count()
            )
        } else {
            "keyboard: keys are available to the core when it asks for them".into()
        }
    }

    // -------------------------------------------------------------------- motion

    /// One CoreMotion sample in libretro's convention: acceleration in g with a phone lying flat
    /// reading (0, 0, +1) (so NEGATE CoreMotion's acceleration), rotation in radians per second
    /// (CoreMotion's rotationRate as is), device axes. NEVER TAKES THE ENGINE LOCK.
    #[allow(clippy::too_many_arguments)]
    pub fn push_motion(&self, ax: f32, ay: f32, az: f32, gx: f32, gy: f32, gz: f32) {
        SENSORS.push([ax, ay, az], [gx, gy, gz]);
    }

    /// Whether CoreMotion should be running: a core switched a sensor on and motion is allowed.
    /// Atomic reads only; poll it every frame.
    pub fn motion_wanted(&self) -> bool {
        SENSORS.wanted()
    }

    pub fn set_motion_enabled(&self, enabled: bool) {
        SENSORS.set_enabled(enabled);
    }

    pub fn motion_enabled(&self) -> bool {
        SENSORS.is_enabled()
    }

    pub fn set_motion_inverted(&self, invert_x: bool, invert_y: bool) {
        SENSORS.set_inverted(invert_x, invert_y);
    }

    /// 0 portrait, 1 landscape with the phone's top on the left (`.landscapeRight`), 2 landscape
    /// with the top on the right (`.landscapeLeft`), 3 upside down.
    pub fn set_motion_orientation(&self, orientation: u32) {
        SENSORS.set_orientation(ScreenOrientation::from_u32(orientation));
    }

    /// Takes the way the phone is held now as level.
    pub fn calibrate_motion(&self) -> String {
        if SENSORS.calibrate() {
            "motion: calibrated, the way you hold the phone now is level".into()
        } else {
            "motion: not calibrated, no motion reading has arrived yet (start a tilt game first)"
                .into()
        }
    }

    /// Flat on a table is level again.
    pub fn reset_motion_calibration(&self) {
        SENSORS.reset_calibration();
    }

    pub fn motion_status(&self) -> String {
        SENSORS.status_line()
    }

    // ----------------------------------------------------------- console actions

    /// Shakes the console: the Pokemon Mini's shake, or a short accelerometer burst.
    pub fn shake(&self) -> String {
        self.lock().shake()
    }

    /// PlayStation: flips player 1 between the digital pad and the DualShock (analog).
    pub fn toggle_analog_mode(&self) -> String {
        self.lock().toggle_analog_mode()
    }

    pub fn is_analog_mode(&self) -> bool {
        self.lock().is_analog_mode()
    }

    /// Nintendo DS: closes the lid, or opens it.
    pub fn toggle_ds_lid(&self) -> String {
        self.lock().toggle_ds_lid()
    }

    pub fn ds_lid_closed(&self) -> bool {
        self.lock().ds_lid_closed()
    }

    /// Nintendo DS: blows into the microphone while `held` is true.
    pub fn blow_into_mic(&self, held: bool) -> String {
        self.lock().blow_into_mic(held)
    }

    /// Nintendo 3DS: one press of HOME.
    pub fn press_home_button(&self) -> String {
        self.lock().press_home_button()
    }

    // --------------------------------------------------------- controller type

    /// Every port's declared device types and current setting, for the Controllers screen.
    pub fn controller_ports(&self) -> Vec<ControllerPortRecord> {
        let bridge = self.lock();
        (0..MAX_PORTS as u32)
            .map(|port| ControllerPortRecord {
                port,
                choices: bridge
                    .controller_types(port)
                    .into_iter()
                    .map(|(name, _)| name)
                    .collect(),
                current: bridge.port_device_name(port),
            })
            .collect()
    }

    /// Chooses (and remembers) a device type for a port of a system. Empty forgets the choice.
    pub fn set_controller_type(&self, system: String, port: u32, name: String) -> String {
        self.lock().set_controller_type(&system, port, &name)
    }

    /// The remembered device type for a port of a system, empty when none.
    pub fn controller_type_choice(&self, system: String, port: u32) -> String {
        let bridge = self.lock();
        bridge
            .input_config_device(&system, port)
            .unwrap_or_default()
    }

    // ---------------------------------------------------------------- remapping

    /// What a button can become, in the order to list them.
    pub fn remap_targets(&self) -> Vec<RemapTargetRecord> {
        all_targets()
            .into_iter()
            .map(|t| RemapTargetRecord {
                code: u32::from(t.code()),
                label: t.label().to_string(),
            })
            .collect()
    }

    /// The 16 retro button names, in map order.
    pub fn remap_button_labels(&self) -> Vec<String> {
        (0..Button::COUNT)
            .filter_map(Button::from_u32)
            .map(|b| b.label().to_string())
            .collect()
    }

    pub fn input_profiles(&self, system: String) -> Vec<RemapProfileRecord> {
        self.lock()
            .input_profiles(&system)
            .iter()
            .map(record_from)
            .collect()
    }

    pub fn active_input_profile(&self, system: String, game: String) -> RemapProfileRecord {
        record_from(&self.lock().active_input_profile(&system, &game))
    }

    pub fn save_input_profile(&self, profile: RemapProfileRecord) -> String {
        self.lock().save_input_profile(profile_from(&profile))
    }

    pub fn delete_input_profile(&self, system: String, game: String, name: String) -> String {
        self.lock().delete_input_profile(&system, &game, &name)
    }

    pub fn select_input_profile(&self, system: String, game: String, name: String) -> String {
        self.lock().select_input_profile(&system, &game, &name)
    }

    /// Next profile for the running game (Manic's triggerPro).
    pub fn cycle_input_profile(&self) -> String {
        self.lock().cycle_input_profile()
    }

    /// Every profile and controller-type choice, as text for the host to store.
    pub fn export_input_config(&self) -> String {
        self.lock().export_input_config()
    }

    /// Restores [`Self::export_input_config`]'s text. Returns a plain line.
    pub fn import_input_config(&self, text: String) -> String {
        let skipped = self.lock().import_input_config(&text);
        if skipped == 0 {
            "controls: settings loaded".into()
        } else {
            format!("controls: settings loaded, {skipped} unreadable line(s) skipped")
        }
    }

    // ------------------------------------------------------------------ session

    /// Call after every successful launch: applies the profile and controller types for it.
    pub fn input_session_started(&self, system: String, game: String) -> String {
        self.lock().input_session_started(&system, &game)
    }

    /// Host-side actions pressed on remapped buttons, and the last action line. Cheap: poll it
    /// every frame.
    pub fn take_host_input_actions(&self) -> HostInputActions {
        let (actions, line) = self.lock().take_host_input_actions();
        HostInputActions { actions, line }
    }
}
