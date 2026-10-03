//! Button remapping: profiles per system (and optionally per game), switchable mid-game.
//!
//! Owned here rather than in Swift so an Android host gets identical behaviour by calling the
//! same functions; the iOS app is only the editor.
//!
//! A [`RemapTable`] says, for every retro button a layer produces, what the game should see
//! instead: another button, nothing, or one of the app's own [`InputAction`]s (show the keyboard,
//! shake, cycle profile, close the DS lid, and so on). Each input layer has its own table, so the
//! physical controller and the on-screen pad can be mapped differently, which is the usual reason
//! to remap at all: a controller's shoulder buttons for a PlayStation game's L2/R2, and the
//! on-screen pad left as it is drawn.
//!
//! A [`RemapProfile`] is a named pair of tables plus the stick options, for one system, or for one
//! game of that system. Several can exist per system; one is active, and cycling moves to the
//! next ("triggerPro" in Manic EMU's terms). A game's own profile wins over the system's.
//!
//! [`InputConfig`] holds every profile, the active choice per system, and the controller TYPE the
//! user chose per port per system, and serialises to a small line-based text the host stores.

use super::Button;

/// Every target a button can be remapped to that is not a game button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum InputAction {
    /// Show or hide the on-screen keyboard. Host-side.
    ToggleKeyboard = 0,
    /// A shake: an accelerometer burst, or the Pokemon Mini's shake button.
    Shake = 1,
    /// Move to the next remap profile for this system.
    CycleProfile = 2,
    /// PlayStation: switch the pad between digital and analog (DualShock).
    ToggleAnalog = 3,
    /// Nintendo DS: close or open the lid.
    DsLid = 4,
    /// Nintendo DS: blow into the microphone, for as long as it is held.
    Blow = 5,
    /// Nintendo 3DS: the HOME button.
    Home = 6,
    /// Open the app's in-game menu. Host-side.
    Menu = 7,
}

impl InputAction {
    pub const ALL: [InputAction; 8] = [
        InputAction::ToggleKeyboard,
        InputAction::Shake,
        InputAction::CycleProfile,
        InputAction::ToggleAnalog,
        InputAction::DsLid,
        InputAction::Blow,
        InputAction::Home,
        InputAction::Menu,
    ];

    pub const fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => Self::ToggleKeyboard,
            1 => Self::Shake,
            2 => Self::CycleProfile,
            3 => Self::ToggleAnalog,
            4 => Self::DsLid,
            5 => Self::Blow,
            6 => Self::Home,
            7 => Self::Menu,
            _ => return None,
        })
    }

    /// The stable name the host receives and the text format stores.
    pub const fn key(self) -> &'static str {
        match self {
            Self::ToggleKeyboard => "keyboard",
            Self::Shake => "shake",
            Self::CycleProfile => "cycleProfile",
            Self::ToggleAnalog => "toggleAnalog",
            Self::DsLid => "dsLid",
            Self::Blow => "blow",
            Self::Home => "home",
            Self::Menu => "menu",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::ToggleKeyboard => "Show keyboard",
            Self::Shake => "Shake",
            Self::CycleProfile => "Next profile",
            Self::ToggleAnalog => "Analog on/off",
            Self::DsLid => "Close DS lid",
            Self::Blow => "Blow into mic",
            Self::Home => "3DS HOME",
            Self::Menu => "Game menu",
        }
    }

    /// Whether the app (not the engine) carries this one out.
    pub const fn is_host_side(self) -> bool {
        matches!(self, Self::ToggleKeyboard | Self::Menu)
    }

    /// Held actions act for as long as the button is down; the rest fire once per press.
    pub const fn is_held(self) -> bool {
        matches!(self, Self::Blow)
    }
}

/// What one button turns into. Encoded as a `u8` across the FFI and in the stored text:
/// `0..=15` a retro button, `0x80 + n` an [`InputAction`], [`Target::NONE_CODE`] nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    None,
    Button(Button),
    Action(InputAction),
}

impl Target {
    pub const NONE_CODE: u8 = 0xff;
    pub const ACTION_BASE: u8 = 0x80;

    pub fn code(self) -> u8 {
        match self {
            Target::None => Self::NONE_CODE,
            Target::Button(b) => b as u8,
            Target::Action(a) => Self::ACTION_BASE + a as u8,
        }
    }

    pub fn from_code(code: u8) -> Self {
        if code < Button::COUNT as u8 {
            return Button::from_u32(u32::from(code)).map_or(Target::None, Target::Button);
        }
        if code >= Self::ACTION_BASE {
            if let Some(action) = InputAction::from_u8(code - Self::ACTION_BASE) {
                return Target::Action(action);
            }
        }
        Target::None
    }

    pub fn label(self) -> &'static str {
        match self {
            Target::None => "Nothing",
            Target::Button(b) => b.label(),
            Target::Action(a) => a.label(),
        }
    }
}

/// Every target, in the order an editor should list them.
pub fn all_targets() -> Vec<Target> {
    let mut out: Vec<Target> = (0..Button::COUNT)
        .filter_map(Button::from_u32)
        .map(Target::Button)
        .collect();
    out.extend(InputAction::ALL.iter().copied().map(Target::Action));
    out.push(Target::None);
    out
}

/// The default analog deadzone. Zero: the raw stick, as before remapping existed.
pub const DEFAULT_DEADZONE: f32 = 0.0;

/// One layer's mapping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemapTable {
    /// Indexed by the retro button the layer produced. Target codes, see [`Target`].
    pub map: [u8; Button::COUNT as usize],
    /// Exchange A and B after mapping (Manic's "swap A/B"). Separate from the map so it can be
    /// flipped in one tap without disturbing a careful mapping.
    pub swap_ab: bool,
    /// The left stick also presses the D-pad.
    pub stick_to_dpad: bool,
    /// Below this the sticks read as centred, and the rest of the travel is rescaled so full
    /// deflection still reaches 1.0. `0.0..=0.9`.
    pub deadzone: f32,
}

impl Default for RemapTable {
    fn default() -> Self {
        let mut map = [0u8; Button::COUNT as usize];
        for (i, slot) in map.iter_mut().enumerate() {
            *slot = i as u8;
        }
        Self {
            map,
            swap_ab: false,
            stick_to_dpad: true,
            deadzone: DEFAULT_DEADZONE,
        }
    }
}

/// The result of passing one layer's buttons through a table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mapped {
    /// Retro buttons, as a `Button` bitfield.
    pub buttons: u32,
    /// Actions held, as a bitfield indexed by `InputAction as u8`.
    pub actions: u32,
}

impl RemapTable {
    pub fn is_identity(&self) -> bool {
        *self == RemapTable::default()
    }

    pub fn set(&mut self, from: Button, to: Target) {
        self.map[from as usize] = to.code();
    }

    pub fn target(&self, from: Button) -> Target {
        Target::from_code(self.map[from as usize])
    }

    pub fn clamp(&mut self) {
        self.deadzone = if self.deadzone.is_finite() {
            self.deadzone.clamp(0.0, 0.9)
        } else {
            0.0
        };
    }

    /// Maps a button bitfield. Allocation-free; called on every poll.
    pub fn apply(&self, buttons: u32) -> Mapped {
        let mut out = Mapped::default();
        if buttons == 0 {
            return out;
        }
        for (from, code) in self.map.iter().enumerate() {
            if buttons & (1 << from) == 0 {
                continue;
            }
            match Target::from_code(*code) {
                Target::None => {}
                Target::Button(b) => out.buttons |= 1 << b as u32,
                Target::Action(a) => out.actions |= 1 << a as u8,
            }
        }
        if self.swap_ab {
            let a = 1 << Button::A as u32;
            let b = 1 << Button::B as u32;
            let had_a = out.buttons & a != 0;
            let had_b = out.buttons & b != 0;
            out.buttons &= !(a | b);
            if had_a {
                out.buttons |= b;
            }
            if had_b {
                out.buttons |= a;
            }
        }
        out
    }

    /// Applies the deadzone to one stick, radially, so a diagonal is not clipped into a cross.
    pub fn shape_stick(&self, x: f32, y: f32) -> (f32, f32) {
        let dz = self.deadzone;
        if dz <= 0.0 {
            return (x, y);
        }
        let magnitude = (x * x + y * y).sqrt();
        if magnitude <= dz {
            return (0.0, 0.0);
        }
        let scaled = ((magnitude - dz) / (1.0 - dz)).min(1.0);
        (x / magnitude * scaled, y / magnitude * scaled)
    }
}

/// A named mapping for a system, or for one game of it.
#[derive(Debug, Clone, PartialEq)]
pub struct RemapProfile {
    pub name: String,
    /// One of the shared system ids (`ps1`, `gba`, ...).
    pub system: String,
    /// Empty for a system-wide profile; otherwise the content id it belongs to.
    pub game: String,
    /// Physical controllers.
    pub gamepad: RemapTable,
    /// The on-screen pad and skin buttons.
    pub touch: RemapTable,
}

impl RemapProfile {
    pub fn new(name: &str, system: &str) -> Self {
        Self {
            name: name.to_string(),
            system: system.to_string(),
            game: String::new(),
            gamepad: RemapTable::default(),
            touch: RemapTable::default(),
        }
    }

    /// The built-in "Default" profile for a system: the plain mapping, plus the one action a
    /// system cannot be played without. The computers get the keyboard on L3, because a DOS,
    /// C64 or Amiga game that says "press any key" needs one reachable from a controller.
    pub fn default_for(system: &str) -> Self {
        let mut profile = Self::new("Default", system);
        if matches!(system, "dos" | "c64" | "amiga") {
            profile
                .gamepad
                .set(Button::L3, Target::Action(InputAction::ToggleKeyboard));
        }
        profile
    }
}

/// Every profile and controller-type choice the user made. The host persists [`Self::to_text`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InputConfig {
    profiles: Vec<RemapProfile>,
    /// `(system, game, profile name)`. `game` empty for the system-wide choice.
    active: Vec<(String, String, String)>,
    /// `(system, port, device name)`.
    devices: Vec<(String, u32, String)>,
}

/// Bumped if the text format ever changes incompatibly.
const FORMAT_HEADER: &str = "continuum-input 1";

impl InputConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// The profiles for a system, the built-in Default first even if the user never saved one.
    pub fn profiles_for(&self, system: &str) -> Vec<RemapProfile> {
        let mut out: Vec<RemapProfile> = self
            .profiles
            .iter()
            .filter(|p| p.system == system)
            .cloned()
            .collect();
        if !out.iter().any(|p| p.name == "Default" && p.game.is_empty()) {
            out.insert(0, RemapProfile::default_for(system));
        }
        out
    }

    /// Adds or replaces (by system, game and name). Names are trimmed; an empty name is refused.
    pub fn save(&mut self, mut profile: RemapProfile) -> Result<(), String> {
        profile.name = sanitize(&profile.name);
        profile.system = sanitize(&profile.system);
        profile.game = sanitize(&profile.game);
        if profile.name.is_empty() {
            return Err("a profile needs a name".into());
        }
        if profile.system.is_empty() {
            return Err("a profile needs a system".into());
        }
        profile.gamepad.clamp();
        profile.touch.clamp();
        if let Some(existing) = self.profiles.iter_mut().find(|p| {
            p.system == profile.system && p.game == profile.game && p.name == profile.name
        }) {
            *existing = profile;
        } else {
            self.profiles.push(profile);
        }
        Ok(())
    }

    /// Removes a profile. The built-in Default cannot be removed, only reset by saving over it.
    pub fn delete(&mut self, system: &str, game: &str, name: &str) -> bool {
        let before = self.profiles.len();
        self.profiles
            .retain(|p| !(p.system == system && p.game == game && p.name == name));
        self.active
            .retain(|(s, g, n)| !(s == system && g == game && n == name));
        self.profiles.len() != before
    }

    /// Makes a profile the active one. `game` empty for the system-wide choice.
    pub fn select(&mut self, system: &str, game: &str, name: &str) -> bool {
        let exists = self
            .profiles_for(system)
            .iter()
            .any(|p| p.name == name && (p.game.is_empty() || p.game == game));
        if !exists {
            return false;
        }
        self.active.retain(|(s, g, _)| !(s == system && g == game));
        self.active
            .push((system.to_string(), game.to_string(), name.to_string()));
        true
    }

    /// The profile that applies: the game's own active choice, else a profile saved for this game,
    /// else the system's active choice, else Default.
    pub fn active_for(&self, system: &str, game: &str) -> RemapProfile {
        let all = self.profiles_for(system);
        let find = |name: &str, want_game: &str| {
            all.iter()
                .find(|p| p.name == name && (p.game == want_game || p.game.is_empty()))
                .cloned()
        };
        if !game.is_empty() {
            if let Some((_, _, name)) = self
                .active
                .iter()
                .find(|(s, g, _)| s == system && g == game)
            {
                if let Some(p) = find(name, game) {
                    return p;
                }
            }
            if let Some(p) = all.iter().find(|p| p.game == game) {
                return p.clone();
            }
        }
        if let Some((_, _, name)) = self
            .active
            .iter()
            .find(|(s, g, _)| s == system && g.is_empty())
        {
            if let Some(p) = find(name, "") {
                return p;
            }
        }
        all.into_iter()
            .next()
            .unwrap_or_else(|| RemapProfile::default_for(system))
    }

    /// Moves to the next profile usable for this game (system-wide ones and this game's own),
    /// wrapping. Remembered for the game when one is running, else for the system.
    pub fn cycle(&mut self, system: &str, game: &str) -> RemapProfile {
        let usable: Vec<RemapProfile> = self
            .profiles_for(system)
            .into_iter()
            .filter(|p| p.game.is_empty() || p.game == game)
            .collect();
        let current = self.active_for(system, game);
        let index = usable
            .iter()
            .position(|p| p.name == current.name && p.game == current.game)
            .unwrap_or(0);
        let next = usable[(index + 1) % usable.len()].clone();
        let scope = if game.is_empty() { "" } else { game };
        self.active.retain(|(s, g, _)| !(s == system && g == scope));
        self.active
            .push((system.to_string(), scope.to_string(), next.name.clone()));
        next
    }

    /// The device name chosen for a port of a system, if any.
    pub fn device_for(&self, system: &str, port: u32) -> Option<&str> {
        self.devices
            .iter()
            .find(|(s, p, _)| s == system && *p == port)
            .map(|(_, _, name)| name.as_str())
    }

    pub fn set_device(&mut self, system: &str, port: u32, name: &str) {
        self.devices.retain(|(s, p, _)| !(s == system && *p == port));
        let name = sanitize(name);
        if !name.is_empty() {
            self.devices.push((sanitize(system), port, name));
        }
    }

    pub fn devices_for(&self, system: &str) -> Vec<(u32, String)> {
        let mut out: Vec<(u32, String)> = self
            .devices
            .iter()
            .filter(|(s, _, _)| s == system)
            .map(|(_, p, n)| (*p, n.clone()))
            .collect();
        out.sort();
        out
    }

    /// The stored form. Tab-separated lines; names are sanitised so a tab or newline in one cannot
    /// break the format.
    pub fn to_text(&self) -> String {
        let mut out = String::from(FORMAT_HEADER);
        out.push('\n');
        for p in &self.profiles {
            out.push_str(&format!(
                "profile\t{}\t{}\t{}\t{}\t{}\n",
                p.system,
                p.game,
                p.name,
                table_text(&p.gamepad),
                table_text(&p.touch)
            ));
        }
        for (s, g, n) in &self.active {
            out.push_str(&format!("active\t{s}\t{g}\t{n}\n"));
        }
        for (s, port, n) in &self.devices {
            out.push_str(&format!("device\t{s}\t{port}\t{n}\n"));
        }
        out
    }

    /// Parses [`Self::to_text`]. Lines it cannot read are skipped and counted, never fatal: a
    /// config written by a newer build should lose what it cannot read, not everything.
    pub fn from_text(text: &str) -> (Self, usize) {
        let mut config = Self::new();
        let mut skipped = 0;
        for line in text.lines() {
            if line.is_empty() || line == FORMAT_HEADER {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            let ok = match fields.as_slice() {
                ["profile", system, game, name, gamepad, touch] => {
                    match (parse_table(gamepad), parse_table(touch)) {
                        (Some(gamepad), Some(touch)) => config
                            .save(RemapProfile {
                                name: name.to_string(),
                                system: system.to_string(),
                                game: game.to_string(),
                                gamepad,
                                touch,
                            })
                            .is_ok(),
                        _ => false,
                    }
                }
                ["active", system, game, name] => {
                    config
                        .active
                        .push((system.to_string(), game.to_string(), name.to_string()));
                    true
                }
                ["device", system, port, name] => match port.parse::<u32>() {
                    Ok(port) => {
                        config.set_device(system, port, name);
                        true
                    }
                    Err(_) => false,
                },
                _ => false,
            };
            if !ok {
                skipped += 1;
            }
        }
        (config, skipped)
    }
}

fn sanitize(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ").trim().to_string()
}

/// `map(16 hex bytes) flags deadzone` as one space-free token: `0001...0f:s1d1:0.15`.
fn table_text(t: &RemapTable) -> String {
    let map: String = t.map.iter().map(|c| format!("{c:02x}")).collect();
    format!(
        "{map}:s{}d{}:{}",
        u8::from(t.swap_ab),
        u8::from(t.stick_to_dpad),
        t.deadzone
    )
}

fn parse_table(s: &str) -> Option<RemapTable> {
    let mut parts = s.split(':');
    let map_hex = parts.next()?;
    let flags = parts.next()?;
    let deadzone: f32 = parts.next()?.parse().ok()?;
    if map_hex.len() != Button::COUNT as usize * 2 {
        return None;
    }
    let mut map = [0u8; Button::COUNT as usize];
    for (i, slot) in map.iter_mut().enumerate() {
        *slot = u8::from_str_radix(map_hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    let flags = flags.as_bytes();
    if flags.len() != 4 || flags[0] != b's' || flags[2] != b'd' {
        return None;
    }
    let mut table = RemapTable {
        map,
        swap_ab: flags[1] == b'1',
        stick_to_dpad: flags[3] == b'1',
        deadzone,
    };
    table.clamp();
    Some(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bit(b: Button) -> u32 {
        1 << b as u32
    }

    #[test]
    fn the_default_table_changes_nothing() {
        let t = RemapTable::default();
        assert!(t.is_identity());
        let all = 0xffff;
        assert_eq!(t.apply(all), Mapped { buttons: all, actions: 0 });
    }

    #[test]
    fn a_button_can_become_another_nothing_or_an_action() {
        let mut t = RemapTable::default();
        t.set(Button::L, Target::Button(Button::L2));
        t.set(Button::Select, Target::None);
        t.set(Button::R3, Target::Action(InputAction::Shake));
        let out = t.apply(bit(Button::L) | bit(Button::Select) | bit(Button::R3));
        assert_eq!(out.buttons, bit(Button::L2));
        assert_eq!(out.actions, 1 << InputAction::Shake as u8);
    }

    #[test]
    fn two_buttons_can_land_on_one() {
        let mut t = RemapTable::default();
        t.set(Button::X, Target::Button(Button::A));
        assert_eq!(t.apply(bit(Button::X)).buttons, bit(Button::A));
        assert_eq!(t.apply(bit(Button::A)).buttons, bit(Button::A));
    }

    #[test]
    fn swap_ab_swaps_after_mapping() {
        let t = RemapTable {
            swap_ab: true,
            ..RemapTable::default()
        };
        assert_eq!(t.apply(bit(Button::A)).buttons, bit(Button::B));
        assert_eq!(t.apply(bit(Button::B)).buttons, bit(Button::A));
        assert_eq!(
            t.apply(bit(Button::A) | bit(Button::B)).buttons,
            bit(Button::A) | bit(Button::B)
        );
        assert_eq!(t.apply(bit(Button::Y)).buttons, bit(Button::Y));
    }

    #[test]
    fn the_deadzone_is_radial_and_full_travel_still_reaches_one() {
        let t = RemapTable {
            deadzone: 0.2,
            ..RemapTable::default()
        };
        assert_eq!(t.shape_stick(0.1, 0.1), (0.0, 0.0));
        let (x, _) = t.shape_stick(1.0, 0.0);
        assert!((x - 1.0).abs() < 1e-6);
        let (x, y) = t.shape_stick(0.6, 0.0);
        assert!((x - 0.5).abs() < 1e-6 && y == 0.0);
    }

    #[test]
    fn target_codes_round_trip() {
        for t in all_targets() {
            assert_eq!(Target::from_code(t.code()), t);
        }
        assert_eq!(Target::from_code(0x40), Target::None);
        assert_eq!(Target::from_code(0x80 + 99), Target::None);
    }

    #[test]
    fn every_system_has_a_default_and_computers_get_the_keyboard() {
        let config = InputConfig::new();
        let ps1 = config.profiles_for("ps1");
        assert_eq!(ps1.len(), 1);
        assert!(ps1[0].gamepad.is_identity());
        let dos = config.active_for("dos", "");
        assert_eq!(
            dos.gamepad.target(Button::L3),
            Target::Action(InputAction::ToggleKeyboard)
        );
    }

    #[test]
    fn a_game_profile_beats_the_system_one() {
        let mut config = InputConfig::new();
        let mut sys = RemapProfile::new("Shoulders", "ps1");
        sys.gamepad.set(Button::L, Target::Button(Button::L2));
        config.save(sys).unwrap();
        assert!(config.select("ps1", "", "Shoulders"));
        let mut game = RemapProfile::new("Racing", "ps1");
        game.game = "Ridge Racer".into();
        config.save(game).unwrap();
        assert_eq!(config.active_for("ps1", "Ridge Racer").name, "Racing");
        assert_eq!(config.active_for("ps1", "Other").name, "Shoulders");
        assert_eq!(config.active_for("ps1", "").name, "Shoulders");
    }

    #[test]
    fn cycling_walks_every_usable_profile_and_wraps() {
        let mut config = InputConfig::new();
        config.save(RemapProfile::new("One", "gba")).unwrap();
        config.save(RemapProfile::new("Two", "gba")).unwrap();
        let mut other = RemapProfile::new("Elsewhere", "gba");
        other.game = "Some Other Game".into();
        config.save(other).unwrap();
        let names: Vec<String> = (0..4).map(|_| config.cycle("gba", "Game").name).collect();
        assert_eq!(names, vec!["One", "Two", "Default", "One"]);
        // Remembered for the game, not the system.
        assert_eq!(config.active_for("gba", "Game").name, "One");
        assert_eq!(config.active_for("gba", "").name, "Default");
    }

    #[test]
    fn empty_names_are_refused_and_tabs_cannot_break_the_text() {
        let mut config = InputConfig::new();
        assert!(config.save(RemapProfile::new("  ", "nes")).is_err());
        config.save(RemapProfile::new("Tab\tName\n", "nes")).unwrap();
        assert_eq!(config.profiles_for("nes")[1].name, "Tab Name");
    }

    #[test]
    fn the_text_round_trips_and_bad_lines_are_skipped_not_fatal() {
        let mut config = InputConfig::new();
        let mut p = RemapProfile::new("Pro", "snes");
        p.gamepad.set(Button::A, Target::Button(Button::B));
        p.gamepad.swap_ab = true;
        p.touch.stick_to_dpad = false;
        p.touch.deadzone = 0.25;
        config.save(p).unwrap();
        config.select("snes", "", "Pro");
        config.set_device("ps1", 0, "DualShock");
        config.set_device("snes", 1, "Multitap");
        let text = config.to_text();
        let (back, skipped) = InputConfig::from_text(&format!("{text}garbage line\nprofile\tx\n"));
        assert_eq!(skipped, 2);
        assert_eq!(back, config);
        assert_eq!(back.device_for("ps1", 0), Some("DualShock"));
        assert_eq!(back.active_for("snes", "").gamepad.target(Button::A), Target::Button(Button::B));
    }

    #[test]
    fn deleting_forgets_the_choice_too() {
        let mut config = InputConfig::new();
        config.save(RemapProfile::new("Gone", "nes")).unwrap();
        config.select("nes", "", "Gone");
        assert!(config.delete("nes", "", "Gone"));
        assert_eq!(config.active_for("nes", "").name, "Default");
        assert!(!config.select("nes", "", "Gone"));
    }

    #[test]
    fn a_device_choice_is_per_system_and_port_and_can_be_cleared() {
        let mut config = InputConfig::new();
        config.set_device("ps1", 0, "DualShock");
        config.set_device("ps1", 0, "standard");
        assert_eq!(config.device_for("ps1", 0), Some("standard"));
        assert_eq!(config.device_for("ps1", 1), None);
        config.set_device("ps1", 0, "");
        assert_eq!(config.device_for("ps1", 0), None);
    }
}
