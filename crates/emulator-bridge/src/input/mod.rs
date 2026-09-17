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

impl InputSnapshot {
    pub fn port(&self, port: usize) -> PortState {
        self.ports.get(port).copied().unwrap_or_default()
    }

    pub fn button(&self, port: usize, button: Button) -> bool {
        self.port(port).is_pressed(button)
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
            _ => 0,
        }
    }
}
