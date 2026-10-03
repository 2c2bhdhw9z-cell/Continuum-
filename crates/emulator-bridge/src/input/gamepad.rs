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

use super::keyboard::{self, KeyEvent, KeyboardState};
use super::remap::{InputAction, InputConfig, RemapTable};
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

/// Core frames a turbo button stays down, and then up, unless the host asks for another rate.
///
/// Four on and four off is 7.5 presses a second at 60 Hz, which is the "fast but countable"
/// rate most frontends ship as their default. Faster than about 2 frames each way and many games
/// stop seeing the release at all, because they sample input once every other frame.
pub const DEFAULT_TURBO_HALF_PERIOD: u32 = 4;

/// The slowest rate a host may ask for: half a second each way at 60 Hz.
const MAX_TURBO_HALF_PERIOD: u32 = 30;

/// What the input side knows about the running game, and the user's input configuration.
///
/// Lives on the bridge rather than in the session so the configuration survives between games,
/// and is reset (apart from `config`) when a game starts. See `bridge/input_glue.rs`.
#[derive(Debug, Default)]
pub struct InputSessionState {
    /// Every remap profile and controller-type choice. Persisted by the host as text.
    pub config: InputConfig,
    /// Shared system id of the running game, empty when none.
    pub system: String,
    /// Content id of the running game, empty when none.
    pub game: String,
    /// Name of the profile in force.
    pub profile: String,
    /// The device id each port was last switched to, `None` for the core's default (the joypad).
    pub port_devices: [Option<u32>; MAX_PORTS],
    /// The DS lid, closed by the action or the API.
    pub lid_closed: bool,
    /// "Blow" asked for through the API (a held on-screen button), as opposed to a remapped one.
    pub blow_requested: bool,
    /// Host-side actions (show keyboard, menu) waiting for the host to collect.
    pub host_actions: Vec<InputAction>,
    /// The last thing an action did, for the host's status line.
    pub last_action_line: String,
}

#[derive(Debug)]
pub struct GamepadBridge {
    /// One independent input layer per source; merged on snapshot.
    sources: [InputState; SOURCE_COUNT],
    connections: [Option<Connection>; MAX_PORTS],
    /// Buttons held as TURBO, per port, as a `Button` bitfield. Not a source layer: a turbo
    /// button is not pressed, it is pulsed, so it is added to each core frame's snapshot by
    /// [`Self::turbo_step`] rather than merged in [`Self::snapshot`].
    turbo: [u32; MAX_PORTS],
    /// Core frames per half cycle (down for this many, then up for this many).
    turbo_half_period: u32,
    /// Core frames since turbo was last picked up from nothing. Counted in core frames rather
    /// than display ticks so the rate is the same at 60 and 120 Hz and under fast forward.
    turbo_clock: u64,
    /// The remap table in force for each source. The keyboard layer is never remapped: its
    /// buttons are a keyboard pretending to be a pad, which is a mapping already.
    remap: [RemapTable; SOURCE_COUNT],
    /// Actions each source/port was holding at its last poll, for edge detection.
    held_actions: [[u32; MAX_PORTS]; SOURCE_COUNT],
    /// Actions pressed since the engine last collected them, in order.
    pending_actions: Vec<InputAction>,
    /// Buttons held on a port by the app rather than a hand: the DS lid (L3 on melonDS) and the
    /// microphone noise (L2). Added to every core frame after remapping, so a profile cannot
    /// move them.
    latched: [u32; MAX_PORTS],
    /// Buttons pressed by the app for a few core frames: the 3DS HOME button, the Pokemon Mini
    /// shake. `pulse_left` counts the frames down.
    pulse: [u32; MAX_PORTS],
    pulse_left: [u32; MAX_PORTS],
    /// See [`InputSessionState`].
    pub session: InputSessionState,
}

impl Default for GamepadBridge {
    fn default() -> Self {
        Self {
            sources: Default::default(),
            connections: Default::default(),
            turbo: [0; MAX_PORTS],
            turbo_half_period: DEFAULT_TURBO_HALF_PERIOD,
            turbo_clock: 0,
            remap: [RemapTable::default(); SOURCE_COUNT],
            held_actions: [[0; MAX_PORTS]; SOURCE_COUNT],
            pending_actions: Vec::new(),
            latched: [0; MAX_PORTS],
            pulse: [0; MAX_PORTS],
            pulse_left: [0; MAX_PORTS],
            session: InputSessionState::default(),
        }
    }
}

impl GamepadBridge {
    pub fn new() -> Self {
        Self::default()
    }

    // ------------------------------------------------------------------- turbo

    /// Which buttons on `port` are being held as turbo, in the same W3C standard order as
    /// [`Self::apply_standard_gamepad_from`], so the host builds this array with the same table
    /// it already builds its pad array with.
    ///
    /// Replaces that port's turbo set wholesale, like a poll. When turbo goes from nothing held
    /// to something held, the clock restarts so the first frame after the press is a DOWN frame:
    /// a turbo button that sometimes waited four frames before doing anything would feel laggy.
    pub fn apply_turbo_standard(&mut self, port: usize, buttons: &[bool]) {
        if port >= MAX_PORTS {
            return;
        }
        let mut mask = 0u32;
        for (index, button) in STANDARD_GAMEPAD_MAP {
            if buttons.get(index).copied().unwrap_or(false) {
                mask |= 1 << button as u32;
            }
        }
        let was_idle = self.turbo.iter().all(|m| *m == 0);
        self.turbo[port] = mask;
        if was_idle && mask != 0 {
            self.turbo_clock = 0;
        }
    }

    /// Frames down (and then up) per turbo cycle. Clamped to `1..=30`.
    pub fn set_turbo_half_period(&mut self, frames: u32) {
        self.turbo_half_period = frames.clamp(1, MAX_TURBO_HALF_PERIOD);
    }

    pub fn turbo_half_period(&self) -> u32 {
        self.turbo_half_period
    }

    /// The input for ONE core frame: `base` plus whichever turbo buttons are in their down phase.
    ///
    /// Advances the turbo clock, so it must be called exactly once per `run_frame`. The tick
    /// still snapshots once and shares that snapshot across its catch-up steps; this only adds
    /// the pulse on top, which is the one part of input that is meant to change between steps.
    pub fn turbo_step(&mut self, base: &InputSnapshot) -> InputSnapshot {
        let mut out = *base;
        // The app's own holds and pulses, per core frame like turbo, so a pulse lasts the same
        // number of emulated frames at any display rate and under fast forward.
        for port in 0..MAX_PORTS {
            out.ports[port].buttons |= self.latched[port];
            if self.pulse_left[port] > 0 {
                out.ports[port].buttons |= self.pulse[port];
                self.pulse_left[port] -= 1;
                if self.pulse_left[port] == 0 {
                    self.pulse[port] = 0;
                }
            }
        }
        if self.turbo.iter().any(|m| *m != 0) {
            let half = u64::from(self.turbo_half_period.max(1));
            let down = (self.turbo_clock / half) % 2 == 0;
            if down {
                for (port, mask) in self.turbo.iter().enumerate() {
                    out.ports[port].buttons |= *mask;
                }
            }
            self.turbo_clock = self.turbo_clock.wrapping_add(1);
        }
        out
    }

    // ------------------------------------------------------- holds and pulses

    /// Holds (or lets go of) buttons on a port on the app's behalf. See `latched`.
    pub fn set_latched(&mut self, port: usize, buttons: u32, held: bool) {
        if let Some(slot) = self.latched.get_mut(port) {
            if held {
                *slot |= buttons;
            } else {
                *slot &= !buttons;
            }
        }
    }

    pub fn latched(&self, port: usize) -> u32 {
        self.latched.get(port).copied().unwrap_or(0)
    }

    /// Presses buttons on a port for `frames` core frames.
    pub fn pulse(&mut self, port: usize, buttons: u32, frames: u32) {
        if port < MAX_PORTS {
            self.pulse[port] |= buttons;
            self.pulse_left[port] = self.pulse_left[port].max(frames);
        }
    }

    // ----------------------------------------------------------------- remapping

    /// Puts a remap table in force for one source. The keyboard layer ignores this.
    pub fn set_remap(&mut self, source: PadSource, table: RemapTable) {
        if source != PadSource::Keyboard {
            self.remap[source as usize] = table;
        }
    }

    pub fn remap(&self, source: PadSource) -> RemapTable {
        self.remap[source as usize]
    }

    /// The actions pressed since the last call, in order, each once per press.
    pub fn take_actions(&mut self) -> Vec<InputAction> {
        std::mem::take(&mut self.pending_actions)
    }

    /// Whether any source on any port is holding `action` right now.
    pub fn action_held(&self, action: InputAction) -> bool {
        let bit = 1u32 << action as u8;
        self.held_actions
            .iter()
            .any(|ports| ports.iter().any(|mask| mask & bit != 0))
    }

    // ------------------------------------------------------------------ keyboard

    /// Every layer's keyboard, merged.
    pub fn merged_keys(&self) -> KeyboardState {
        let mut keys = KeyboardState::default();
        for source in &self.sources {
            keys.merge(source.keys());
        }
        keys
    }

    /// One key, from one layer: a hardware keyboard on `Keyboard`, the on-screen one on `Touch`.
    ///
    /// The polled state changes here, and the core's keyboard callback, if it registered one, is
    /// told on its next frame. It is told only when the MERGED state changes, so the same key
    /// held on both keyboards is one press and one release, not two. `character` is the UTF-32
    /// text the key typed, 0 for none; it is sent with the press only.
    pub fn set_key(&mut self, source: PadSource, keycode: u32, down: bool, character: u32) {
        let before = self.merged_keys();
        self.sources[source as usize].set_key(keycode, down);
        let after = self.merged_keys();
        if before.is_down(keycode) == after.is_down(keycode) {
            return;
        }
        keyboard::queue(KeyEvent {
            down,
            keycode,
            character: if down { character } else { 0 },
            modifiers: after.held_modifiers(),
        });
    }

    /// Tells the core about every key that a release just let go of.
    fn queue_key_releases(&self, before: &KeyboardState) {
        let after = self.merged_keys();
        for key in before.held_keys() {
            if !after.is_down(key) {
                keyboard::queue(KeyEvent {
                    down: false,
                    keycode: key,
                    character: 0,
                    modifiers: after.held_modifiers(),
                });
            }
        }
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
        // A remapped "blow" held on the pad that left must not stay held.
        self.held_actions[PadSource::Gamepad as usize][port] = 0;
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

    /// Adds relative mouse motion to one layer. See [`crate::input::MouseState`].
    pub fn add_mouse_motion(&mut self, port: usize, source: PadSource, dx: f32, dy: f32) {
        self.sources[source as usize].add_mouse_motion(port, dx, dy);
    }

    pub fn set_mouse_buttons(
        &mut self,
        port: usize,
        source: PadSource,
        left: bool,
        right: bool,
        middle: bool,
    ) {
        self.sources[source as usize].set_mouse_buttons(port, left, right, middle);
    }

    pub fn add_mouse_wheel(&mut self, port: usize, source: PadSource, vertical: i32, horizontal: i32) {
        self.sources[source as usize].add_mouse_wheel(port, vertical, horizontal);
    }

    /// A core frame read the mouse: every layer drops the motion it delivered.
    pub fn end_mouse_frame(&mut self) {
        for source in &mut self.sources {
            source.end_mouse_frame();
        }
    }

    /// Releases every button on every port, across all sources.
    pub fn release_all(&mut self) {
        let keys = self.merged_keys();
        for source in &mut self.sources {
            source.release_all();
        }
        self.turbo = [0; MAX_PORTS];
        self.held_actions = [[0; MAX_PORTS]; SOURCE_COUNT];
        self.queue_key_releases(&keys);
    }

    /// Releases one source, e.g. when the touch overlay is hidden.
    ///
    /// Releasing the touch layer also drops turbo, because only the on-screen pad holds turbo
    /// buttons, and an overlay taken away mid-hold would otherwise leave a button firing forever.
    pub fn release_source(&mut self, source: PadSource) {
        let keys = self.merged_keys();
        self.sources[source as usize].release_all();
        self.held_actions[source as usize] = [0; MAX_PORTS];
        self.queue_key_releases(&keys);
        if source == PadSource::Touch {
            self.turbo = [0; MAX_PORTS];
        }
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
                // Mouse: whole units of motion add across layers (each layer keeps its own
                // remainder), buttons OR, wheel steps add.
                let (dx, dy) = layer.mouse.delivered();
                merged.mouse.dx += dx as f32;
                merged.mouse.dy += dy as f32;
                merged.mouse.buttons |= layer.mouse.buttons;
                merged.mouse.wheel = merged.mouse.wheel.saturating_add(layer.mouse.wheel);
                merged.mouse.hwheel = merged.mouse.hwheel.saturating_add(layer.mouse.hwheel);
            }
        }
        InputSnapshot {
            ports,
            keys: self.merged_keys(),
        }
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
            // The mouse shares the touch layer for the same reason the stylus does.
            mouse: self.sources[source as usize].ports[port].mouse,
            ..PortState::default()
        };
        let mut raw = 0u32;
        for (index, button) in STANDARD_GAMEPAD_MAP {
            if buttons.get(index).copied().unwrap_or(false) {
                raw |= 1 << button as u32;
            }
        }

        // The profile in force for this source: buttons to buttons, to nothing, or to actions.
        let table = self.remap[source as usize];
        let mapped = table.apply(raw);
        state.buttons = mapped.buttons;
        self.note_actions(source, port, mapped.actions);

        // Analog axes pass through for cores that read them, through the profile's deadzone...
        for axis in 0..AXIS_COUNT {
            state.axes[axis] = axes.get(axis).copied().unwrap_or(0.0).clamp(-1.0, 1.0);
        }
        let (lx, ly) = table.shape_stick(state.axes[0], state.axes[1]);
        let (rx, ry) = table.shape_stick(state.axes[2], state.axes[3]);
        state.axes = [lx, ly, rx, ry];

        // ...and the left stick additionally drives the D-pad, because most retro
        // cores only read the D-pad while a player on a modern controller reaches for
        // the stick first. A profile can turn that off. The threshold is the larger of the
        // usual one and the profile's deadzone, measured on the raw stick.
        if table.stick_to_dpad {
            let threshold = AXIS_DEADZONE.max(table.deadzone);
            let x = axes.first().copied().unwrap_or(0.0).clamp(-1.0, 1.0);
            let y = axes.get(1).copied().unwrap_or(0.0).clamp(-1.0, 1.0);
            if x <= -threshold {
                state.buttons |= 1 << Button::Left as u32;
            }
            if x >= threshold {
                state.buttons |= 1 << Button::Right as u32;
            }
            if y <= -threshold {
                state.buttons |= 1 << Button::Up as u32;
            }
            if y >= threshold {
                state.buttons |= 1 << Button::Down as u32;
            }
        }

        self.sources[source as usize].ports[port] = state;
    }

    /// Records which actions a source/port now holds, queuing each newly pressed one.
    fn note_actions(&mut self, source: PadSource, port: usize, actions: u32) {
        let previous = self.held_actions[source as usize][port];
        let pressed = actions & !previous;
        if pressed != 0 {
            for action in InputAction::ALL {
                if pressed & (1 << action as u8) != 0 && self.pending_actions.len() < 64 {
                    self.pending_actions.push(action);
                }
            }
        }
        self.held_actions[source as usize][port] = actions;
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
            mouse: self.sources[PadSource::Gamepad as usize].ports[port].mouse,
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

    /// W3C index 1 is retro A (see `STANDARD_GAMEPAD_MAP`).
    fn turbo_a() -> Vec<bool> {
        let mut buttons = vec![false; 16];
        buttons[1] = true;
        buttons
    }

    fn a_pattern(pads: &mut GamepadBridge, frames: usize) -> Vec<bool> {
        let base = pads.snapshot();
        (0..frames)
            .map(|_| pads.turbo_step(&base).button(0, Button::A))
            .collect()
    }

    #[test]
    fn turbo_pulses_at_the_half_period() {
        let mut pads = GamepadBridge::new();
        pads.set_turbo_half_period(2);
        pads.apply_turbo_standard(0, &turbo_a());
        assert_eq!(
            a_pattern(&mut pads, 8),
            vec![true, true, false, false, true, true, false, false]
        );
    }

    #[test]
    fn turbo_starts_on_a_down_frame() {
        let mut pads = GamepadBridge::new();
        pads.set_turbo_half_period(3);
        pads.apply_turbo_standard(0, &turbo_a());
        let _ = a_pattern(&mut pads, 4); // part way into an up phase
        pads.apply_turbo_standard(0, &[]);
        pads.apply_turbo_standard(0, &turbo_a());
        assert!(
            a_pattern(&mut pads, 1)[0],
            "a fresh press must fire at once"
        );
    }

    #[test]
    fn turbo_does_not_touch_ordinary_buttons_or_other_ports() {
        let mut pads = GamepadBridge::new();
        pads.set_button(0, PadSource::Touch, Button::B, true);
        pads.apply_turbo_standard(0, &turbo_a());
        let base = pads.snapshot();
        assert!(
            !base.button(0, Button::A),
            "turbo is not part of the merged layers"
        );
        for _ in 0..12 {
            let step = pads.turbo_step(&base);
            assert!(step.button(0, Button::B), "a held button stays held");
            assert!(!step.button(1, Button::A));
        }
    }

    #[test]
    fn idle_turbo_leaves_the_snapshot_alone() {
        let mut pads = GamepadBridge::new();
        pads.set_button(0, PadSource::Touch, Button::Start, true);
        let base = pads.snapshot();
        let step = pads.turbo_step(&base);
        assert_eq!(step.ports[0].buttons, base.ports[0].buttons);
    }

    #[test]
    fn releasing_touch_or_everything_drops_turbo() {
        let mut pads = GamepadBridge::new();
        pads.apply_turbo_standard(0, &turbo_a());
        pads.release_source(PadSource::Touch);
        assert!(a_pattern(&mut pads, 8).iter().all(|down| !down));
        pads.apply_turbo_standard(0, &turbo_a());
        pads.release_source(PadSource::Gamepad);
        assert!(
            a_pattern(&mut pads, 1)[0],
            "a controller unplug is not the overlay"
        );
        pads.release_all();
        assert!(a_pattern(&mut pads, 8).iter().all(|down| !down));
    }

    #[test]
    fn turbo_rate_is_clamped() {
        let mut pads = GamepadBridge::new();
        assert_eq!(pads.turbo_half_period(), DEFAULT_TURBO_HALF_PERIOD);
        pads.set_turbo_half_period(0);
        assert_eq!(pads.turbo_half_period(), 1);
        pads.set_turbo_half_period(1000);
        assert_eq!(pads.turbo_half_period(), MAX_TURBO_HALF_PERIOD);
    }

    #[test]
    fn turbo_ignores_an_out_of_range_port() {
        let mut pads = GamepadBridge::new();
        pads.apply_turbo_standard(MAX_PORTS, &turbo_a());
        assert!(a_pattern(&mut pads, 4).iter().all(|down| !down));
    }
}
