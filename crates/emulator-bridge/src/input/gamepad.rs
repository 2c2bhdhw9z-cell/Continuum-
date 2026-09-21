//! `GamepadBridge` — every controller, from every source, in one place.
//!
//! Four input sources have to end up as the same thing: a physical gamepad polled
//! through the HTML5 Gamepad API, a keyboard pretending to be a pad, an on-screen
//! touch pad, and (Phase 2) iOS `GameController`. The translation from each of those
//! to a retro pad lives here, in Rust, rather than in the front end — including the
//! W3C "standard gamepad" button layout, which would otherwise have to be
//! reimplemented identically in Swift and inevitably drift.
//!
//! What the front end sends is deliberately dumb:
//!
//! ```text
//!   keyboard      →  set_button(port, PadSource::Keyboard, Button::A, true)
//!   touch overlay →  set_button(port, PadSource::Touch, Button::A, true)
//!   gamepad poll  →  apply_standard_gamepad(port, &buttons, &axes)
//! ```
//!
//! ## Why input is tracked per source
//!
//! The Gamepad API has no events, so a connected pad is polled every frame and each
//! poll reports the *complete* state of that pad — including "D-pad not pressed".
//! With one shared button field, that poll would clear a D-pad press the keyboard or
//! the touch overlay was holding, 60 times a second: the keyboard would appear to
//! stop working whenever a controller was plugged in.
//!
//! So each source keeps its own state and [`GamepadBridge::snapshot`] merges them:
//! buttons are OR-ed, and for axes the largest magnitude wins. A full poll can then
//! overwrite its own source without touching anyone else's.

use super::{Button, InputSnapshot, InputState, PortState, AXIS_COUNT, MAX_PORTS};

/// Below this magnitude a stick is treated as centred. Cheap drift rejection, and the
/// threshold at which an analog stick starts synthesising D-pad presses.
const AXIS_DEADZONE: f32 = 0.35;

/// Where a button press came from. Each source owns an independent state layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum PadSource {
    Gamepad = 0,
    Keyboard = 1,
    Touch = 2,
}

const SOURCE_COUNT: usize = 3;

impl PadSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            PadSource::Gamepad => "gamepad",
            PadSource::Keyboard => "keyboard",
            PadSource::Touch => "touch",
        }
    }

    pub fn from_str_or_keyboard(name: &str) -> Self {
        match name {
            "gamepad" => PadSource::Gamepad,
            "touch" => PadSource::Touch,
            _ => PadSource::Keyboard,
        }
    }
}

/// What kind of device is registered on a port. Reported to the UI so a player can
/// tell whether their controller was actually picked up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadKind {
    /// Physical controller reporting the W3C standard layout.
    StandardGamepad,
    /// Physical controller with an unrecognised layout; buttons pass through by index.
    UnmappedGamepad,
    Keyboard,
    Touch,
}

impl PadKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            PadKind::StandardGamepad => "gamepad",
            PadKind::UnmappedGamepad => "gamepad-unmapped",
            PadKind::Keyboard => "keyboard",
            PadKind::Touch => "touch",
        }
    }
}

#[derive(Debug, Clone)]
struct Connection {
    kind: PadKind,
    /// Device id as reported by the platform, for display.
    label: String,
}

/// The W3C standard gamepad layout, mapped to a retro pad.
///
/// Index 0 is the bottom face button (Cross on a DualSense, A on an Xbox pad) and maps
/// to retro **B**, not retro A. That is not a mistake: retro's A/B follow the Nintendo
/// arrangement where A sits to the right of B, so the bottom button is B. Getting this
/// backwards makes every core feel like its buttons are swapped.
const STANDARD_GAMEPAD_MAP: [(usize, Button); 16] = [
    (0, Button::B),
    (1, Button::A),
    (2, Button::Y),
    (3, Button::X),
    (4, Button::L),
    (5, Button::R),
    (6, Button::L2),
    (7, Button::R2),
    (8, Button::Select),
    (9, Button::Start),
    (10, Button::L3),
    (11, Button::R3),
    (12, Button::Up),
    (13, Button::Down),
    (14, Button::Left),
    (15, Button::Right),
];

#[derive(Debug, Default)]
pub struct GamepadBridge {
    /// One independent input layer per source; merged on snapshot.
    sources: [InputState; SOURCE_COUNT],
    connections: [Option<Connection>; MAX_PORTS],
}

impl GamepadBridge {
    pub fn new() -> Self {
        Self::default()
    }

    // ------------------------------------------------------------- connections

    /// Registers a device on `port`. Re-registering replaces the label.
    pub fn connect(&mut self, port: usize, kind: PadKind, label: impl Into<String>) -> bool {
        if port >= MAX_PORTS {
            return false;
        }
        let label = label.into();
        log::info!("port {port}: connected {} ({label})", kind.as_str());
        self.connections[port] = Some(Connection { kind, label });
        true
    }

    /// Drops a device and releases what it was holding, so a controller unplugged
    /// mid-jump does not leave the character running forever.
    pub fn disconnect(&mut self, port: usize) {
        if port >= MAX_PORTS {
            return;
        }
        if self.connections[port].take().is_some() {
            log::info!("port {port}: disconnected");
        }
        // Only the gamepad layer: a keyboard player on the same port keeps playing.
        self.sources[PadSource::Gamepad as usize].ports[port] = PortState::default();
    }

    pub fn is_connected(&self, port: usize) -> bool {
        self.connections.get(port).is_some_and(Option::is_some)
    }

    pub fn connected_count(&self) -> usize {
        self.connections.iter().filter(|c| c.is_some()).count()
    }

    pub fn pad_kind(&self, port: usize) -> Option<PadKind> {
        self.connections.get(port)?.as_ref().map(|c| c.kind)
    }

    pub fn pad_label(&self, port: usize) -> Option<&str> {
        self.connections
            .get(port)?
            .as_ref()
            .map(|c| c.label.as_str())
    }

    /// First free port, for auto-assigning a newly connected controller.
    pub fn first_free_port(&self) -> Option<usize> {
        self.connections.iter().position(Option::is_none)
    }

    // ----------------------------------------------------------------- buttons

    pub fn set_button(&mut self, port: usize, source: PadSource, button: Button, pressed: bool) {
        self.sources[source as usize].set_button(port, button, pressed);
    }

    /// Moves one layer's pointer. See [`crate::input::InputState::set_pointer`] for the units and
    /// [`Self::snapshot`] for how a pointer merges, which is not how buttons merge.
    pub fn set_pointer(&mut self, port: usize, source: PadSource, x: f32, y: f32, pressed: bool) {
        self.sources[source as usize].set_pointer(port, x, y, pressed);
    }

    pub fn set_axis(&mut self, port: usize, source: PadSource, axis: usize, value: f32) {
        self.sources[source as usize].set_axis(port, axis, value);
    }

    /// Releases every button on every port, across all sources.
    pub fn release_all(&mut self) {
        for source in &mut self.sources {
            source.release_all();
        }
    }

    /// Releases one source, e.g. when the touch overlay is hidden.
    pub fn release_source(&mut self, source: PadSource) {
        self.sources[source as usize].release_all();
    }

    /// Freezes the merged state of every source for one frame.
    pub fn snapshot(&self) -> InputSnapshot {
        let mut ports = [PortState::default(); MAX_PORTS];
        for (port, merged) in ports.iter_mut().enumerate() {
            for source in &self.sources {
                let layer = source.ports[port];
                merged.buttons |= layer.buttons;
                for axis in 0..AXIS_COUNT {
                    // Largest magnitude wins, so a stick at rest cannot cancel a
                    // deflection reported by another source.
                    if layer.axes[axis].abs() > merged.axes[axis].abs() {
                        merged.axes[axis] = layer.axes[axis];
                    }
                }
                // A PRESSED POINTER WINS, and an unpressed one never overwrites a pressed one.
                // Buttons can be OR-ed and axes can take the largest, but a position cannot be
                // combined with another position: two fingers in different places have no
                // meaningful average. So the rule is that whichever layer is actually holding
                // the pointer supplies both the coordinates and the pressed flag, and a layer
                // resting at (0,0) cannot drag a live touch to the corner. In practice only the
                // touch layer ever sets this, which is why the simple rule is sufficient.
                if layer.pointer_pressed && !merged.pointer_pressed {
                    merged.pointer = layer.pointer;
                    merged.pointer_pressed = true;
                } else if !merged.pointer_pressed {
                    // Nothing held anywhere yet: keep the last position reported, so a release
                    // leaves the stylus where it was rather than snapping it to a corner.
                    merged.pointer = layer.pointer;
                }
            }
        }
        InputSnapshot { ports }
    }

    /// Applies one poll of a W3C standard gamepad.
    ///
    /// `buttons` is `pad.buttons.map(b => b.pressed)` and `axes` is `pad.axes`,
    /// exactly as `navigator.getGamepads()` reports them. Missing entries are treated
    /// as unpressed/centred, so a pad with fewer controls needs no special handling.
    ///
    /// Replaces the gamepad layer wholesale — which is correct, because a poll is a
    /// complete statement about that device — and leaves the keyboard and touch layers
    /// untouched. Called every frame from the engine tick, so it allocates nothing.
    pub fn apply_standard_gamepad(&mut self, port: usize, buttons: &[bool], axes: &[f32]) {
        self.apply_standard_gamepad_from(port, PadSource::Gamepad, buttons, axes);
    }

    /// As [`Self::apply_standard_gamepad`], but says which layer the poll belongs to.
    ///
    /// This distinction is the whole reason the on-screen pad and a real controller can be
    /// used at the same time, or even in the same moment on the same button. Sources are
    /// independent layers merged at snapshot time, so a poll that lands on the wrong one does
    /// not merge, it *replaces*: an overlay reporting "nothing held" sixty times a second into
    /// the same layer a physical pad writes to would cancel that pad out entirely, and the
    /// symptom would be a controller that works only while no finger is near the screen.
    pub fn apply_standard_gamepad_from(
        &mut self,
        port: usize,
        source: PadSource,
        buttons: &[bool],
        axes: &[f32],
    ) {
        if port >= MAX_PORTS {
            return;
        }

        // The pointer is carried across rather than reset, and this is load-bearing rather
        // than tidy. A poll replaces its layer wholesale, which is what makes the overlay and
        // a real controller independent, but a gamepad poll has nothing to say about a stylus:
        // they share the touch layer, since a thumb on the glass and a finger on the DS touch
        // screen are the same input device. Resetting it here would mean the stylus survived
        // only for as long as the tick happened to push the buttons before the pointer, and
        // swapping those two lines in MetalCanvas would silently erase every stroke.
        let mut state = PortState {
            pointer: self.sources[source as usize].ports[port].pointer,
            pointer_pressed: self.sources[source as usize].ports[port].pointer_pressed,
            ..PortState::default()
        };
        for (index, button) in STANDARD_GAMEPAD_MAP {
            if buttons.get(index).copied().unwrap_or(false) {
                state.buttons |= 1 << button as u32;
            }
        }

        // Analog axes pass through for cores that read them...
        for axis in 0..AXIS_COUNT {
            state.axes[axis] = axes.get(axis).copied().unwrap_or(0.0).clamp(-1.0, 1.0);
        }

        // ...and the left stick additionally drives the D-pad, because most retro
        // cores only read the D-pad while a player on a modern controller reaches for
        // the stick first.
        let x = state.axes[0];
        let y = state.axes[1];
        if x <= -AXIS_DEADZONE {
            state.buttons |= 1 << Button::Left as u32;
        }
        if x >= AXIS_DEADZONE {
            state.buttons |= 1 << Button::Right as u32;
        }
        if y <= -AXIS_DEADZONE {
            state.buttons |= 1 << Button::Up as u32;
        }
        if y >= AXIS_DEADZONE {
            state.buttons |= 1 << Button::Down as u32;
        }

        self.sources[source as usize].ports[port] = state;
    }

    /// Pass-through for a controller whose layout is not the standard one: button
    /// indices map to retro ids unchanged. Better than mapping them wrongly.
    pub fn apply_raw_gamepad(&mut self, port: usize, buttons: &[bool]) {
        if port >= MAX_PORTS {
            return;
        }
        // Carried across for the reason `apply_standard_gamepad_from` explains, even though
        // this path is only ever a physical controller and so has no stylus of its own today.
        // Leaving one of the two polls destructive would be a trap for whoever wires a pointer
        // to a second layer later.
        let mut state = PortState {
            pointer: self.sources[PadSource::Gamepad as usize].ports[port].pointer,
            pointer_pressed: self.sources[PadSource::Gamepad as usize].ports[port]
                .pointer_pressed,
            ..PortState::default()
        };
        for (index, pressed) in buttons.iter().enumerate() {
            if *pressed {
                if let Some(button) = Button::from_u32(index as u32) {
                    state.buttons |= 1 << button as u32;
                }
            }
        }
        self.sources[PadSource::Gamepad as usize].ports[port] = state;
    }
}

#[cfg(test)]
mod tests {
    use super::super::{RETRO_DEVICE_ANALOG, RETRO_DEVICE_JOYPAD};
    use super::*;

    #[test]
    fn standard_layout_puts_bottom_button_on_retro_b() {
        let mut pads = GamepadBridge::new();
        let mut buttons = [false; 16];
        buttons[0] = true; // bottom face button
        pads.apply_standard_gamepad(0, &buttons, &[]);
        let snapshot = pads.snapshot();
        assert!(snapshot.button(0, Button::B), "index 0 must be retro B");
        assert!(!snapshot.button(0, Button::A));
    }

    #[test]
    fn dpad_indices_map_through() {
        let mut pads = GamepadBridge::new();
        let mut buttons = [false; 16];
        buttons[12] = true; // Up
        buttons[15] = true; // Right
        pads.apply_standard_gamepad(0, &buttons, &[]);
        let snapshot = pads.snapshot();
        assert!(snapshot.button(0, Button::Up));
        assert!(snapshot.button(0, Button::Right));
        assert!(!snapshot.button(0, Button::Down));
    }

    #[test]
    fn left_stick_synthesises_dpad() {
        let mut pads = GamepadBridge::new();
        pads.apply_standard_gamepad(0, &[], &[-0.9, 0.0]);
        assert!(pads.snapshot().button(0, Button::Left));

        pads.apply_standard_gamepad(0, &[], &[0.0, 0.0]);
        assert!(!pads.snapshot().button(0, Button::Left));
    }

    #[test]
    fn gamepad_poll_does_not_clear_keyboard_press() {
        // The regression this whole per-source design exists for: an idle controller
        // polled every frame must not cancel the keyboard.
        let mut pads = GamepadBridge::new();
        pads.set_button(0, PadSource::Keyboard, Button::Left, true);
        for _ in 0..10 {
            pads.apply_standard_gamepad(0, &[false; 16], &[0.0, 0.0]);
        }
        assert!(
            pads.snapshot().button(0, Button::Left),
            "an idle gamepad poll must not clear another source's press"
        );
    }

    #[test]
    fn touch_and_gamepad_combine() {
        let mut pads = GamepadBridge::new();
        pads.set_button(0, PadSource::Touch, Button::A, true);
        let mut buttons = [false; 16];
        buttons[12] = true; // Up on the physical pad
        pads.apply_standard_gamepad(0, &buttons, &[]);
        let snapshot = pads.snapshot();
        assert!(snapshot.button(0, Button::A) && snapshot.button(0, Button::Up));
    }

    #[test]
    fn releasing_one_source_leaves_the_others() {
        let mut pads = GamepadBridge::new();
        pads.set_button(0, PadSource::Keyboard, Button::Start, true);
        pads.set_button(0, PadSource::Touch, Button::Select, true);
        pads.release_source(PadSource::Touch);
        let snapshot = pads.snapshot();
        assert!(snapshot.button(0, Button::Start));
        assert!(!snapshot.button(0, Button::Select));
    }

    #[test]
    fn deadzone_rejects_drift() {
        let mut pads = GamepadBridge::new();
        pads.apply_standard_gamepad(0, &[], &[0.2, -0.2]);
        let snapshot = pads.snapshot();
        assert!(!snapshot.button(0, Button::Right));
        assert!(!snapshot.button(0, Button::Up));
    }

    #[test]
    fn analog_axes_reach_libretro_range() {
        let mut pads = GamepadBridge::new();
        pads.apply_standard_gamepad(0, &[], &[1.0, -1.0]);
        let snapshot = pads.snapshot();
        assert_eq!(snapshot.libretro_state(0, RETRO_DEVICE_ANALOG, 0, 0), 32767);
        assert_eq!(
            snapshot.libretro_state(0, RETRO_DEVICE_ANALOG, 0, 1),
            -32767
        );
    }

    #[test]
    fn disconnect_releases_the_gamepad_layer_only() {
        let mut pads = GamepadBridge::new();
        pads.connect(0, PadKind::StandardGamepad, "Test Pad");
        let mut buttons = [false; 16];
        buttons[9] = true; // Start
        pads.apply_standard_gamepad(0, &buttons, &[]);
        pads.set_button(0, PadSource::Keyboard, Button::A, true);
        assert!(pads.snapshot().button(0, Button::Start));

        pads.disconnect(0);
        let snapshot = pads.snapshot();
        assert!(
            !snapshot.button(0, Button::Start),
            "pad buttons must release"
        );
        assert!(snapshot.button(0, Button::A), "keyboard must survive");
        assert!(!pads.is_connected(0));
    }

    #[test]
    fn ports_are_independent() {
        let mut pads = GamepadBridge::new();
        pads.set_button(0, PadSource::Keyboard, Button::A, true);
        pads.set_button(1, PadSource::Keyboard, Button::B, true);
        let snapshot = pads.snapshot();
        assert!(snapshot.button(0, Button::A) && !snapshot.button(0, Button::B));
        assert!(snapshot.button(1, Button::B) && !snapshot.button(1, Button::A));
    }

    #[test]
    fn libretro_joypad_query_matches_pressed_state() {
        let mut pads = GamepadBridge::new();
        pads.set_button(0, PadSource::Keyboard, Button::Start, true);
        let snapshot = pads.snapshot();
        assert_eq!(
            snapshot.libretro_state(0, RETRO_DEVICE_JOYPAD, 0, Button::Start as u32),
            1
        );
        assert_eq!(
            snapshot.libretro_state(0, RETRO_DEVICE_JOYPAD, 0, Button::A as u32),
            0
        );
        // An unknown device type is "not connected", not a panic.
        assert_eq!(snapshot.libretro_state(0, 99, 0, 0), 0);
    }

    #[test]
    fn first_free_port_skips_connected_ones() {
        let mut pads = GamepadBridge::new();
        assert_eq!(pads.first_free_port(), Some(0));
        pads.connect(0, PadKind::StandardGamepad, "one");
        assert_eq!(pads.first_free_port(), Some(1));
    }
}
