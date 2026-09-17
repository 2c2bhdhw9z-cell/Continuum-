//! Input state, owned by Rust rather than the UI.
//!
//! The UI's only job is to translate a keyboard/gamepad/touch event into
//! `set_button(port, button, pressed)`. Mapping, latching and per-frame snapshots
//! live here so Phase 2's Swift UI inherits identical behaviour for free.

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

impl InputSnapshot {
    pub fn port(&self, port: usize) -> PortState {
        self.ports.get(port).copied().unwrap_or_default()
    }

    /// Convenience for the eventual libretro `input_state_t` callback.
    pub fn button(&self, port: usize, button: Button) -> bool {
        self.port(port).is_pressed(button)
    }
}
