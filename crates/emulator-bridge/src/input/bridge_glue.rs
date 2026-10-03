//! The engine half of the input features that need the running core: controller type per port,
//! remap profiles in force, and the app's own input actions (shake, analog toggle, DS lid, blow,
//! 3DS HOME, profile cycling).
//!
//! A child module of `bridge` (see the `#[path]` there) so it can reach the session and the
//! gamepads without widening either. Everything here is behaviour rather than plumbing, so an
//! Android host gets it by calling the same methods.
//!
//! How each console-specific action reaches its core, from the cores' own sources:
//! - DS lid: libretro melonDS reads `RETRO_DEVICE_ID_JOYPAD_L3` as "Close lid" and calls
//!   `NDS::SetLidClosed` while it is held (src/libretro/input.cpp). So the lid is a held L3.
//! - DS blow: the same core reads `JOYPAD_L2` as "Make microphone noise" and feeds a recorded blow
//!   (or white noise) into the mic for every frame it is held (libretro.cpp, `holding_noise_btn`).
//!   It does not use the libretro microphone interface at all, so this is the only way to blow.
//! - 3DS HOME: Azahar's libretro frontend binds Citra's Home button to `JOYPAD_L3`
//!   (citra_libretro.cpp, "Home/Swap screens"), so HOME is a short L3 press.
//! - Pokemon Mini shake: PokeMini's libretro core labels `JOYPAD_L` "Shake" (libretro/libretro.c).
//! - Everything else that reads an accelerometer gets a burst through `sensors`.

use super::EmulatorBridge;
use crate::input::remap::{InputAction, RemapProfile};
use crate::input::sensors::SENSORS;
use crate::input::{Button, PadSource, MAX_PORTS, RETRO_DEVICE_JOYPAD};

/// Core frames the 3DS HOME press lasts. Citra polls its buttons once a frame; a tenth of a
/// second is a firm tap that every HOME menu check sees.
const HOME_PRESS_FRAMES: u32 = 6;
/// Core frames the Pokemon Mini shake button is held.
const POKEMINI_SHAKE_FRAMES: u32 = 12;

fn bit(button: Button) -> u32 {
    1 << button as u32
}

impl EmulatorBridge {
    fn running_core_id(&self) -> Option<&str> {
        self.session.as_ref().map(|s| s.core_id.as_str())
    }

    fn input_system(&self) -> &str {
        &self.gamepads.session.system
    }

    fn is_ds(&self) -> bool {
        self.input_system() == "ds" || self.running_core_id().is_some_and(|c| c.contains("melonds"))
    }

    fn is_3ds(&self) -> bool {
        self.input_system() == "n3ds"
            || self
                .running_core_id()
                .is_some_and(|c| c.contains("azahar") || c.contains("citra"))
    }

    fn is_pokemini(&self) -> bool {
        self.input_system() == "pokemini"
            || self.running_core_id().is_some_and(|c| c.contains("pokemini"))
    }

    /// Tells the input side which game just started. Applies the remap profile in force and the
    /// controller types the user chose for this system. Returns a plain line for the status bar.
    ///
    /// Call once per successful launch, after the core has loaded (it declares its controller
    /// types while loading).
    pub fn input_session_started(&mut self, system: &str, game: &str) -> String {
        {
            let state = &mut self.gamepads.session;
            state.system = system.to_string();
            state.game = game.to_string();
            state.port_devices = [None; MAX_PORTS];
            state.lid_closed = false;
            state.blow_requested = false;
            state.host_actions.clear();
            state.last_action_line.clear();
        }
        for port in 0..MAX_PORTS {
            self.gamepads.set_latched(port, u32::MAX, false);
        }
        let _ = self.gamepads.take_actions();
        let profile_line = self.apply_active_profile();
        let mut lines = vec![profile_line];
        let choices = self.gamepads.session.config.devices_for(system);
        for (port, name) in choices {
            lines.push(self.apply_controller_type(port, &name));
        }
        lines.join("; ")
    }

    fn apply_active_profile(&mut self) -> String {
        let state = &self.gamepads.session;
        let profile = state.config.active_for(&state.system, &state.game);
        let line = format!("controls: profile \"{}\"", profile.name);
        self.gamepads.session.profile = profile.name.clone();
        self.gamepads.set_remap(PadSource::Gamepad, profile.gamepad);
        self.gamepads.set_remap(PadSource::Touch, profile.touch);
        line
    }

    // ---------------------------------------------------------------- profiles

    /// The profiles for a system, built-in Default first.
    pub fn input_profiles(&self, system: &str) -> Vec<RemapProfile> {
        self.gamepads.session.config.profiles_for(system)
    }

    /// The profile in force for a system and game (game may be empty).
    pub fn active_input_profile(&self, system: &str, game: &str) -> RemapProfile {
        self.gamepads.session.config.active_for(system, game)
    }

    /// Saves a profile and, if it is the one in force for the running game, applies it at once.
    pub fn save_input_profile(&mut self, profile: RemapProfile) -> String {
        let name = profile.name.clone();
        let system = profile.system.clone();
        match self.gamepads.session.config.save(profile) {
            Ok(()) => {
                if system == self.gamepads.session.system {
                    self.apply_active_profile();
                }
                format!("controls: saved profile \"{}\" for {system}", name.trim())
            }
            Err(reason) => format!("controls: not saved, {reason}"),
        }
    }

    pub fn delete_input_profile(&mut self, system: &str, game: &str, name: &str) -> String {
        if self.gamepads.session.config.delete(system, game, name) {
            if system == self.gamepads.session.system {
                self.apply_active_profile();
            }
            format!("controls: deleted profile \"{name}\"")
        } else {
            format!("controls: there is no saved profile \"{name}\" to delete")
        }
    }

    /// Makes a profile the one in force, for the game when `game` is non-empty.
    pub fn select_input_profile(&mut self, system: &str, game: &str, name: &str) -> String {
        if !self.gamepads.session.config.select(system, game, name) {
            return format!("controls: no profile \"{name}\" for {system}");
        }
        if system == self.gamepads.session.system {
            self.apply_active_profile()
        } else {
            format!("controls: \"{name}\" will be used for {system}")
        }
    }

    /// Moves to the next profile for the running game (Manic's triggerPro).
    pub fn cycle_input_profile(&mut self) -> String {
        let system = self.gamepads.session.system.clone();
        if system.is_empty() {
            return "controls: no game is running, so there is no profile to switch".into();
        }
        let game = self.gamepads.session.game.clone();
        let count = self
            .gamepads
            .session
            .config
            .profiles_for(&system)
            .iter()
            .filter(|p| p.game.is_empty() || p.game == game)
            .count();
        let next = self.gamepads.session.config.cycle(&system, &game);
        self.apply_active_profile();
        if count < 2 {
            format!(
                "controls: \"{}\" is the only profile for this system; make another in Controllers",
                next.name
            )
        } else {
            format!("controls: switched to profile \"{}\"", next.name)
        }
    }

    /// The remembered device type for a port of a system.
    pub fn input_config_device(&self, system: &str, port: u32) -> Option<String> {
        self.gamepads
            .session
            .config
            .device_for(system, port)
            .map(str::to_string)
    }

    pub fn export_input_config(&self) -> String {
        self.gamepads.session.config.to_text()
    }

    /// Replaces the whole configuration with a stored one. Returns how many lines were skipped.
    pub fn import_input_config(&mut self, text: &str) -> usize {
        let (config, skipped) = crate::input::remap::InputConfig::from_text(text);
        self.gamepads.session.config = config;
        if !self.gamepads.session.system.is_empty() {
            self.apply_active_profile();
        }
        skipped
    }

    // --------------------------------------------------------- controller type

    /// The device a port is set to: the one switched to this session, else the joypad.
    pub fn port_device(&self, port: u32) -> u32 {
        self.gamepads
            .session
            .port_devices
            .get(port as usize)
            .copied()
            .flatten()
            .unwrap_or(RETRO_DEVICE_JOYPAD)
    }

    /// The name of the device a port is set to, from the core's own table.
    pub fn port_device_name(&self, port: u32) -> String {
        let device = self.port_device(port);
        self.controller_types(port)
            .into_iter()
            .find(|(_, id)| *id == device)
            .map(|(name, _)| name)
            .unwrap_or_else(|| {
                if device == RETRO_DEVICE_JOYPAD {
                    "the core's default pad".to_string()
                } else {
                    format!("device {device}")
                }
            })
    }

    /// Switches a port, recording what it was switched to. The one place that does, so the
    /// analog toggle and mouse mode agree about what is plugged in.
    pub fn switch_port_device(&mut self, port: u32, device: u32) -> Result<(), crate::BridgeError> {
        self.set_controller_port_device(port, device)?;
        if let Some(slot) = self.gamepads.session.port_devices.get_mut(port as usize) {
            *slot = Some(device);
        }
        Ok(())
    }

    fn apply_controller_type(&mut self, port: u32, name: &str) -> String {
        let player = port + 1;
        let types = self.controller_types(port);
        let Some((found, device)) = types
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .cloned()
        else {
            return format!(
                "controls: this core does not offer \"{name}\" for player {player}, so it keeps its default"
            );
        };
        match self.switch_port_device(port, device) {
            Ok(()) => format!("controls: player {player} is \"{found}\""),
            Err(error) => format!("controls: could not make player {player} \"{found}\": {error}"),
        }
    }

    /// Chooses a controller type for a port of a system, remembers it, and applies it now if that
    /// system is running. An empty name forgets the choice (the core's default at next launch).
    pub fn set_controller_type(&mut self, system: &str, port: u32, name: &str) -> String {
        if port as usize >= MAX_PORTS {
            return format!("controls: there is no player {}", port + 1);
        }
        self.gamepads.session.config.set_device(system, port, name);
        if name.trim().is_empty() {
            return format!(
                "controls: player {} goes back to the core's default from the next launch",
                port + 1
            );
        }
        if system == self.gamepads.session.system && self.session.is_some() {
            self.apply_controller_type(port, name.trim())
        } else {
            format!(
                "controls: player {} will be \"{}\" next time a {system} game starts",
                port + 1,
                name.trim()
            )
        }
    }

    /// The analog device a PlayStation core offers on a port: DualShock first, then any analog.
    fn analog_device(&self, port: u32) -> Option<(String, u32)> {
        let types = self.controller_types(port);
        let find = |needle: &str| {
            types
                .iter()
                .find(|(n, _)| n.to_ascii_lowercase().contains(needle))
                .cloned()
        };
        find("dualshock").or_else(|| find("analog"))
    }

    pub fn is_analog_mode(&self) -> bool {
        let device = self.port_device(0);
        device != RETRO_DEVICE_JOYPAD
            && self
                .controller_types(0)
                .iter()
                .any(|(n, id)| {
                    *id == device && {
                        let n = n.to_ascii_lowercase();
                        n.contains("dualshock") || n.contains("analog")
                    }
                })
    }

    /// Flips player 1 between the digital pad and the analog one (PlayStation DualShock).
    pub fn toggle_analog_mode(&mut self) -> String {
        if self.session.is_none() {
            return "analog: no game is running".into();
        }
        let line = if self.is_analog_mode() {
            let (name, device) = crate::cores::pick_joypad_device(&self.controller_types(0));
            match self.switch_port_device(0, device) {
                Ok(()) => format!("analog off: player 1 is \"{name}\""),
                Err(error) => format!("analog: could not switch to \"{name}\": {error}"),
            }
        } else {
            match self.analog_device(0) {
                None => "analog: this core offers no analog pad for player 1".to_string(),
                Some((name, device)) => match self.switch_port_device(0, device) {
                    Ok(()) => format!("analog on: player 1 is \"{name}\""),
                    Err(error) => format!("analog: could not switch to \"{name}\": {error}"),
                },
            }
        };
        self.gamepads.session.last_action_line = line.clone();
        line
    }

    // ------------------------------------------------------ console actions

    /// Shakes the console: the Pokemon Mini's shake button, or an accelerometer burst.
    pub fn shake(&mut self) -> String {
        if self.session.is_none() {
            return "shake: no game is running".into();
        }
        let line = if self.is_pokemini() {
            self.gamepads.pulse(0, bit(Button::L), POKEMINI_SHAKE_FRAMES);
            "shake: Pokemon Mini shaken".to_string()
        } else {
            SENSORS.shake();
            if SENSORS.wanted() || SENSORS.status_line().contains("tilt") {
                "shake: sent to the game's motion sensor".to_string()
            } else {
                "shake: sent, but this game has not asked for the motion sensor".to_string()
            }
        };
        self.gamepads.session.last_action_line = line.clone();
        line
    }

    /// Closes the DS lid, or opens it again.
    pub fn toggle_ds_lid(&mut self) -> String {
        if !self.is_ds() || self.session.is_none() {
            return "lid: only a Nintendo DS game has a lid".into();
        }
        let closed = !self.gamepads.session.lid_closed;
        self.gamepads.session.lid_closed = closed;
        self.gamepads.set_latched(0, bit(Button::L3), closed);
        let line = if closed {
            "lid: closed (the game may go to sleep; use the lid button again to open it)"
        } else {
            "lid: open"
        }
        .to_string();
        self.gamepads.session.last_action_line = line.clone();
        line
    }

    pub fn ds_lid_closed(&self) -> bool {
        self.gamepads.session.lid_closed
    }

    /// Blows into the DS microphone for as long as `held` is true.
    pub fn blow_into_mic(&mut self, held: bool) -> String {
        if !self.is_ds() || self.session.is_none() {
            return "blow: only a Nintendo DS game listens for blowing".into();
        }
        self.gamepads.session.blow_requested = held;
        self.apply_blow();
        if held {
            "blow: blowing into the microphone".into()
        } else {
            "blow: stopped".into()
        }
    }

    fn apply_blow(&mut self) {
        let held = self.gamepads.session.blow_requested || self.gamepads.action_held(InputAction::Blow);
        if self.is_ds() {
            self.gamepads.set_latched(0, bit(Button::L2), held);
        }
    }

    /// The 3DS HOME button: a short press.
    pub fn press_home_button(&mut self) -> String {
        if !self.is_3ds() || self.session.is_none() {
            return "HOME: only a Nintendo 3DS game has a HOME button".into();
        }
        self.gamepads.pulse(0, bit(Button::L3), HOME_PRESS_FRAMES);
        let line = "HOME: pressed".to_string();
        self.gamepads.session.last_action_line = line.clone();
        line
    }

    /// Carries out the actions remapped buttons pressed since the last tick. Called at the top of
    /// every tick; nothing to do costs one empty `Vec` swap and one flag check.
    pub fn process_input_actions(&mut self) {
        let actions = self.gamepads.take_actions();
        for action in actions {
            match action {
                InputAction::Shake => {
                    self.shake();
                }
                InputAction::CycleProfile => {
                    let line = self.cycle_input_profile();
                    self.gamepads.session.last_action_line = line;
                }
                InputAction::ToggleAnalog => {
                    self.toggle_analog_mode();
                }
                InputAction::DsLid => {
                    self.toggle_ds_lid();
                }
                InputAction::Home => {
                    self.press_home_button();
                }
                // A level, handled below.
                InputAction::Blow => {}
                InputAction::ToggleKeyboard | InputAction::Menu => {
                    if self.gamepads.session.host_actions.len() < 16 {
                        self.gamepads.session.host_actions.push(action);
                    }
                }
            }
        }
        let held = self.gamepads.session.blow_requested
            || self.gamepads.action_held(InputAction::Blow);
        let applied = self.gamepads.latched(0) & bit(Button::L2) != 0;
        if held != applied && self.session.is_some() {
            self.apply_blow();
        }
    }

    /// Host-side actions pressed since the last call, by stable name ("keyboard", "menu"), plus
    /// the last line an engine-side action wrote, if any (empty otherwise).
    pub fn take_host_input_actions(&mut self) -> (Vec<String>, String) {
        let actions = std::mem::take(&mut self.gamepads.session.host_actions)
            .into_iter()
            .map(|a| a.key().to_string())
            .collect();
        let line = std::mem::take(&mut self.gamepads.session.last_action_line);
        (actions, line)
    }

    /// One key from a keyboard layer. See `GamepadBridge::set_key`.
    pub fn key_event(&mut self, source: PadSource, keycode: u32, down: bool, character: u32) {
        self.gamepads.set_key(source, keycode, down, character);
    }
}

#[cfg(test)]
mod tests {
    use crate::input::remap::{RemapProfile, Target};
    use crate::input::{Button, PadSource};
    use crate::EmulatorBridge;

    #[test]
    fn actions_need_a_game() {
        let mut bridge = EmulatorBridge::new();
        assert!(bridge.toggle_ds_lid().contains("only a Nintendo DS"));
        assert!(bridge.press_home_button().contains("only a Nintendo 3DS"));
        assert!(bridge.toggle_analog_mode().contains("no game"));
        assert!(bridge.shake().contains("no game"));
        assert!(bridge.cycle_input_profile().contains("no game"));
    }

    #[test]
    fn starting_a_game_applies_its_profile() {
        let mut bridge = EmulatorBridge::new();
        let mut p = RemapProfile::new("Swap", "snes");
        p.gamepad.set(Button::A, Target::Button(Button::Y));
        bridge.save_input_profile(p);
        bridge.select_input_profile("snes", "", "Swap");
        let line = bridge.input_session_started("snes", "Some Game");
        assert!(line.contains("\"Swap\""), "{line}");
        let mut pad = [false; 16];
        pad[1] = true; // W3C east = retro A
        bridge.apply_gamepad_from(0, PadSource::Gamepad, &pad, &[]);
        let snapshot = bridge.gamepads.snapshot();
        assert!(snapshot.button(0, Button::Y) && !snapshot.button(0, Button::A));
        // The on-screen pad keeps its own (default) table.
        bridge.apply_gamepad_from(0, PadSource::Touch, &pad, &[]);
        assert!(bridge.gamepads.snapshot().button(0, Button::A));
    }

    #[test]
    fn a_remapped_host_action_reaches_the_host_once_per_press() {
        let mut bridge = EmulatorBridge::new();
        bridge.input_session_started("dos", "Game");
        let mut pad = [false; 16];
        pad[10] = true; // W3C left stick click = retro L3, the keyboard on the DOS default
        for _ in 0..3 {
            bridge.apply_gamepad_from(0, PadSource::Gamepad, &pad, &[]);
            bridge.process_input_actions();
        }
        let (actions, _) = bridge.take_host_input_actions();
        assert_eq!(actions, vec!["keyboard".to_string()]);
        // The game does not see the L3 that was spent on the keyboard.
        assert!(!bridge.gamepads.snapshot().button(0, Button::L3));
    }

    #[test]
    fn profile_and_device_choices_survive_export_and_import() {
        let mut bridge = EmulatorBridge::new();
        bridge.save_input_profile(RemapProfile::new("Mine", "gba"));
        bridge.set_controller_type("ps1", 0, "dualshock");
        let text = bridge.export_input_config();
        let mut other = EmulatorBridge::new();
        assert_eq!(other.import_input_config(&text), 0);
        assert_eq!(other.input_profiles("gba").len(), 2);
        assert_eq!(other.export_input_config(), text);
    }
}
