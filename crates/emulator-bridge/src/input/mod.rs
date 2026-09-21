//! Input state, owned by Rust rather than the UI.
//!
//! The UI's only job is to forward raw events — a key, a gamepad poll, a touch. All
//! mapping, latching, port assignment and per-frame snapshotting lives here, so
//! Phase 2's Swift UI inherits identical behaviour by calling the same functions
//! rather than reimplementing them.
//!
//! - [`GamepadBridge`] is the live state: connections, buttons, analog axes.
//! - [`InputSnapshot`] is one frame's frozen copy, and the thing a core queries.
//!
//! The numeric button ids are `RETRO_DEVICE_ID_JOYPAD_*` values, so
//! [`InputSnapshot::libretro_state`] can answer a core's `input_state` callback
//! with no translation layer in between.

mod gamepad;

pub use gamepad::{GamepadBridge, PadKind, PadSource};

/// Supported local players. Four covers every Phase 1 system (PS1 multitap aside).
pub const MAX_PORTS: usize = 4;

/// Digital buttons, ordered to match `RETRO_DEVICE_ID_JOYPAD_*`.
///
/// The numeric values are part of the FFI contract: the UI sends these integers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Button {
    B = 0,
    Y = 1,
    Select = 2,
    Start = 3,
    Up = 4,
    Down = 5,
    Left = 6,
    Right = 7,
    A = 8,
    X = 9,
    L = 10,
    R = 11,
    L2 = 12,
    R2 = 13,
    L3 = 14,
    R3 = 15,
}

impl Button {
    pub const COUNT: u32 = 16;

    /// The buttons every supported system has, in the order a UI should show them.
    /// Analog sticks and L2/R2/L3/R3 exist for later systems but are not part of the
    /// baseline retro pad.
    pub const STANDARD: [Button; 12] = [
        Button::Up,
        Button::Down,
        Button::Left,
        Button::Right,
        Button::A,
        Button::B,
        Button::X,
        Button::Y,
        Button::L,
        Button::R,
        Button::Start,
        Button::Select,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Button::B => "B",
            Button::Y => "Y",
            Button::Select => "Select",
            Button::Start => "Start",
            Button::Up => "Up",
            Button::Down => "Down",
            Button::Left => "Left",
            Button::Right => "Right",
            Button::A => "A",
            Button::X => "X",
            Button::L => "L",
            Button::R => "R",
            Button::L2 => "L2",
            Button::R2 => "R2",
            Button::L3 => "L3",
            Button::R3 => "R3",
        }
    }

    pub const fn from_u32(v: u32) -> Option<Self> {
        use Button::*;
        Some(match v {
            0 => B,
            1 => Y,
            2 => Select,
            3 => Start,
            4 => Up,
            5 => Down,
            6 => Left,
            7 => Right,
            8 => A,
            9 => X,
            10 => L,
            11 => R,
            12 => L2,
            13 => R2,
            14 => L3,
            15 => R3,
            _ => return None,
        })
    }
}

/// Analog sticks: `(left_x, left_y, right_x, right_y)`, each in `-1.0..=1.0`.
pub const AXIS_COUNT: usize = 4;

#[derive(Debug, Clone, Copy, Default)]
pub struct PortState {
    /// Bitfield indexed by [`Button`].
    pub buttons: u32,
    pub axes: [f32; AXIS_COUNT],
    /// Where a finger or stylus is, as a fraction of the framebuffer in `0.0..=1.0`, with the
    /// origin at the TOP LEFT.
    ///
    /// Stored as a fraction rather than in libretro's own units because the caller does not know
    /// the framebuffer's size and should not have to: a front end knows where a finger landed
    /// inside the picture it drew, which is a proportion. The conversion to libretro's
    /// `-32767..=32767` happens once, in [`InputSnapshot::input_state`], where the convention
    /// belongs.
    pub pointer: [f32; 2],
    /// Whether the pointer is down. Separate from the coordinates on purpose: a core reads
    /// position and pressed state as two different queries, and a release has to keep the last
    /// position rather than snapping it to a corner, or the final frame of a drag jumps.
    pub pointer_pressed: bool,
}

impl PortState {
    pub fn is_pressed(&self, button: Button) -> bool {
        self.buttons & (1 << button as u32) != 0
    }
}

/// Live input state, mutated by UI events between ticks.
#[derive(Debug, Clone, Copy, Default)]
pub struct InputState {
    ports: [PortState; MAX_PORTS],
}

impl InputState {
    pub fn set_button(&mut self, port: usize, button: Button, pressed: bool) {
        if let Some(p) = self.ports.get_mut(port) {
            let mask = 1 << button as u32;
            if pressed {
                p.buttons |= mask;
            } else {
                p.buttons &= !mask;
            }
        }
    }

    pub fn set_axis(&mut self, port: usize, axis: usize, value: f32) {
        if let Some(p) = self.ports.get_mut(port) {
            if axis < AXIS_COUNT {
                p.axes[axis] = value.clamp(-1.0, 1.0);
            }
        }
    }

    /// Moves the pointer, as a fraction of the framebuffer with the origin top left.
    ///
    /// The coordinates are kept even when `pressed` is false, which is the whole reason this takes
    /// both at once. A core reads position and pressed state as separate queries, and zeroing the
    /// position on release would make the last frame of every drag jump to the top-left corner: on
    /// a DS that is a stylus flicking to the corner of the touch screen at the end of every
    /// stroke, which games read as a real input.
    pub fn set_pointer(&mut self, port: usize, x: f32, y: f32, pressed: bool) {
        if let Some(p) = self.ports.get_mut(port) {
            // Clamped rather than rejected. A finger can legitimately slide a pixel outside the
            // picture mid-drag, and the useful reading of that is the edge rather than nothing.
            p.pointer = [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)];
            p.pointer_pressed = pressed;
        }
    }

    /// Releases everything. Called on blur/visibility loss so a held key cannot
    /// stick down while the tab is in the background.
    pub fn release_all(&mut self) {
        self.ports = [PortState::default(); MAX_PORTS];
    }

    /// Freezes the current state for one core step. Taking a snapshot keeps input
    /// coherent across the catch-up steps of a single tick.
    pub fn snapshot(&self) -> InputSnapshot {
        InputSnapshot { ports: self.ports }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct InputSnapshot {
    pub ports: [PortState; MAX_PORTS],
}

/// `RETRO_DEVICE_JOYPAD`.
pub const RETRO_DEVICE_JOYPAD: u32 = 1;
/// `RETRO_DEVICE_ANALOG`.
pub const RETRO_DEVICE_ANALOG: u32 = 5;
/// `RETRO_DEVICE_INDEX_ANALOG_LEFT` / `_RIGHT`.
pub const RETRO_ANALOG_LEFT: u32 = 0;
pub const RETRO_ANALOG_RIGHT: u32 = 1;

/// `RETRO_DEVICE_POINTER`. The Nintendo DS touch screen arrives through this.
pub const RETRO_DEVICE_POINTER: u32 = 6;
/// `RETRO_DEVICE_ID_POINTER_X` / `_Y` / `_PRESSED` / `_COUNT`.
pub const RETRO_POINTER_X: u32 = 0;
pub const RETRO_POINTER_Y: u32 = 1;
pub const RETRO_POINTER_PRESSED: u32 = 2;
pub const RETRO_POINTER_COUNT: u32 = 3;

impl InputSnapshot {
    pub fn port(&self, port: usize) -> PortState {
        self.ports.get(port).copied().unwrap_or_default()
    }

    pub fn button(&self, port: usize, button: Button) -> bool {
        self.port(port).is_pressed(button)
    }

    /// A `0.0..=1.0` fraction from the top left, as libretro's centre-origin pointer axis.
    ///
    /// Saturating rather than wrapping. `as i16` on a float outside the type's range is
    /// implementation-defined in the direction that matters here, and a wrapped coordinate is a
    /// stylus that teleports to the opposite edge, so the clamp is applied before the cast rather
    /// than trusted to it.
    fn to_pointer_axis(fraction: f32) -> i16 {
        let centred = (fraction.clamp(0.0, 1.0) - 0.5) * 2.0;
        (centred * 32767.0).round().clamp(-32767.0, 32767.0) as i16
    }

    /// Answers a core's `retro_input_state_t` callback.
    ///
    /// This is the actual implementation behind the callback, which is why the
    /// button ids in this module are libretro's: a core asks for
    /// `(port, RETRO_DEVICE_JOYPAD, 0, 4)` and gets Up, with nothing translating in
    /// between. Unknown devices return 0, which libretro defines as "not present".
    pub fn libretro_state(&self, port: u32, device: u32, index: u32, id: u32) -> i16 {
        let port = port as usize;
        match device {
            RETRO_DEVICE_JOYPAD => match Button::from_u32(id) {
                Some(button) if self.button(port, button) => 1,
                _ => 0,
            },
            RETRO_DEVICE_ANALOG => {
                // Analog axes are reported in -32768..=32767.
                let axis = match (index, id) {
                    (RETRO_ANALOG_LEFT, 0) => 0,
                    (RETRO_ANALOG_LEFT, 1) => 1,
                    (RETRO_ANALOG_RIGHT, 0) => 2,
                    (RETRO_ANALOG_RIGHT, 1) => 3,
                    _ => return 0,
                };
                let value = self.port(port).axes[axis];
                (value.clamp(-1.0, 1.0) * 32767.0) as i16
            }
            RETRO_DEVICE_POINTER => {
                // ONE POINTER, so anything past index 0 is absent rather than clamped. A core
                // that supports multi-touch asks for index 1 and must be told there is nothing
                // there; answering with the first finger's position again would read as two
                // fingers in the same place.
                if index != 0 {
                    return 0;
                }
                let state = self.port(port);
                match id {
                    // libretro's pointer space is -32767..=32767 across the WHOLE framebuffer,
                    // with the origin in the CENTRE, while this side stores a 0..1 fraction from
                    // the top left. Hence the doubling and the shift: 0.5 maps to 0.
                    RETRO_POINTER_X => Self::to_pointer_axis(state.pointer[0]),
                    RETRO_POINTER_Y => Self::to_pointer_axis(state.pointer[1]),
                    RETRO_POINTER_PRESSED => i16::from(state.pointer_pressed),
                    // Cores use this to discover how many touches there are before asking for
                    // any. Reporting 0 while nothing is held is what stops a core treating a
                    // resting stylus position as a live touch.
                    RETRO_POINTER_COUNT => i16::from(state.pointer_pressed),
                    _ => 0,
                }
            }
            _ => 0,
        }
    }
}


#[cfg(test)]
mod pointer_tests {
    use super::{
        InputState, RETRO_DEVICE_POINTER, RETRO_POINTER_COUNT, RETRO_POINTER_PRESSED,
        RETRO_POINTER_X, RETRO_POINTER_Y,
    };

    /// A pointer query against a snapshot built from one `set_pointer` call.
    fn query(x: f32, y: f32, pressed: bool, id: u32) -> i16 {
        let mut state = InputState::default();
        state.set_pointer(0, x, y, pressed);
        state
            .snapshot()
            .libretro_state(0, RETRO_DEVICE_POINTER, 0, id)
    }

    #[test]
    fn the_centre_of_the_framebuffer_is_the_origin() {
        // libretro's pointer space is centre-origin, and this side stores a top-left fraction.
        // Half way across has to come out as zero, or every touch is offset by half a screen.
        assert_eq!(query(0.5, 0.5, true, RETRO_POINTER_X), 0);
        assert_eq!(query(0.5, 0.5, true, RETRO_POINTER_Y), 0);
    }

    #[test]
    fn the_corners_reach_the_full_range() {
        assert_eq!(query(0.0, 0.0, true, RETRO_POINTER_X), -32767);
        assert_eq!(query(0.0, 0.0, true, RETRO_POINTER_Y), -32767);
        assert_eq!(query(1.0, 1.0, true, RETRO_POINTER_X), 32767);
        assert_eq!(query(1.0, 1.0, true, RETRO_POINTER_Y), 32767);
    }

    #[test]
    fn the_top_of_a_ds_touch_screen_is_halfway_down_the_framebuffer() {
        // THE DS CASE, WRITTEN OUT BECAUSE IT IS THE ONE THAT WILL BE GOT WRONG. melonDS emits
        // both screens stacked in one 256x384 framebuffer, so the touch screen is the LOWER HALF
        // and its top edge is y = 0.5, which is the vertical centre of the framebuffer and
        // therefore 0 in libretro's space. A host that passed a fraction of the touch screen
        // rather than of the framebuffer would have every tap land in the upper screen's half.
        assert_eq!(query(0.5, 0.5, true, RETRO_POINTER_Y), 0);
        // The very bottom of the touch screen is the bottom of the framebuffer.
        assert_eq!(query(0.5, 1.0, true, RETRO_POINTER_Y), 32767);
        // Halfway down the touch screen is three quarters down the framebuffer.
        let three_quarters = query(0.5, 0.75, true, RETRO_POINTER_Y);
        assert!((three_quarters - 16383).abs() <= 2, "got {three_quarters}");
    }

    #[test]
    fn out_of_range_saturates_rather_than_wrapping() {
        // A finger can slide a little outside the picture mid-drag. The useful reading is the
        // edge; a wrapped coordinate would teleport the stylus to the opposite side, and `as i16`
        // on an out-of-range float is not something to rely on.
        assert_eq!(query(-5.0, -5.0, true, RETRO_POINTER_X), -32767);
        assert_eq!(query(9.0, 9.0, true, RETRO_POINTER_Y), 32767);
    }

    #[test]
    fn releasing_keeps_the_position_but_clears_pressed() {
        let mut state = InputState::default();
        state.set_pointer(0, 0.25, 0.75, true);
        state.set_pointer(0, 0.25, 0.75, false);
        let snapshot = state.snapshot();

        assert_eq!(snapshot.libretro_state(0, RETRO_DEVICE_POINTER, 0, RETRO_POINTER_PRESSED), 0);
        assert_eq!(snapshot.libretro_state(0, RETRO_DEVICE_POINTER, 0, RETRO_POINTER_COUNT), 0);
        // The position survives. Zeroing it would put a jump to the corner at the end of every
        // stroke, which a game reads as a deliberate input.
        assert_eq!(
            snapshot.libretro_state(0, RETRO_DEVICE_POINTER, 0, RETRO_POINTER_Y),
            super::InputSnapshot::to_pointer_axis(0.75)
        );
    }

    #[test]
    fn a_second_finger_is_reported_as_absent() {
        // One pointer. A core that supports multi-touch asks for index 1 and must be told there is
        // nothing there; repeating the first finger's position would read as two touches at once.
        let mut state = InputState::default();
        state.set_pointer(0, 0.5, 0.9, true);
        let snapshot = state.snapshot();
        assert_eq!(snapshot.libretro_state(0, RETRO_DEVICE_POINTER, 1, RETRO_POINTER_PRESSED), 0);
        assert_eq!(snapshot.libretro_state(0, RETRO_DEVICE_POINTER, 1, RETRO_POINTER_X), 0);
    }

    #[test]
    fn count_is_zero_until_something_is_held() {
        // Cores read COUNT before asking for a position. Reporting a resting stylus as one touch
        // would make every DS game think the screen was being held from the moment it booted.
        let mut state = InputState::default();
        state.set_pointer(0, 0.5, 0.9, false);
        assert_eq!(
            state.snapshot().libretro_state(0, RETRO_DEVICE_POINTER, 0, RETRO_POINTER_COUNT),
            0
        );
    }

    #[test]
    fn a_pointer_does_not_disturb_the_buttons() {
        // The pointer shares PortState with the button bitfield, so this is cheap insurance
        // against a future field reorder quietly aliasing the two.
        let mut state = InputState::default();
        state.set_button(0, super::Button::A, true);
        state.set_pointer(0, 0.9, 0.9, true);
        let snapshot = state.snapshot();
        assert!(snapshot.button(0, super::Button::A));
        assert_eq!(snapshot.libretro_state(0, RETRO_DEVICE_POINTER, 0, RETRO_POINTER_PRESSED), 1);
    }
}
