//! Force feedback a core asked for, held until the host plays it.
//!
//! libretro's rumble is a SET call from inside the core: `RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE`
//! (libretro.h:1147) hands the core a `struct retro_rumble_interface { retro_set_rumble_state_t
//! set_rumble_state; }` (libretro.h:5649), and the core calls
//! `set_rumble_state(port, effect, strength)` whenever a motor should change, where `effect` is
//! `enum retro_rumble_effect` (`RETRO_RUMBLE_STRONG = 0`, `RETRO_RUMBLE_WEAK = 1`,
//! libretro.h:5621) and `strength` is `0..=0xffff`.
//!
//! The engine only REMEMBERS the latest strength per port and motor. Playing it is the host's
//! job, because the motor is a phone's Taptic Engine on iOS and a vibrator service on Android,
//! and neither belongs in this crate. The host polls [`Rumble::reading`] once per displayed frame
//! and changes its own haptics only when the value moved.
//!
//! A process global rather than a field on the bridge, for the same reason as the pixel-format
//! negotiation in `native_core.rs`: the callback fires from inside `retro_run`, which runs while
//! the bridge is already borrowed, so it cannot reach the bridge. A `Mutex` rather than atomics,
//! because a whole port is read as one pair and two atomics could be observed half updated. The
//! lock is never held across anything but a copy, so the core thread and the display link cannot
//! stall each other on it.

use super::MAX_PORTS;
use std::sync::Mutex;

/// `RETRO_RUMBLE_STRONG`, libretro.h:5623.
pub const RETRO_RUMBLE_STRONG: u32 = 0;
/// `RETRO_RUMBLE_WEAK`, libretro.h:5624.
pub const RETRO_RUMBLE_WEAK: u32 = 1;

/// One port's two motors, each `0..=0xffff` exactly as the core sent it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RumbleMotors {
    pub strong: u16,
    pub weak: u16,
}

impl RumbleMotors {
    /// The strong motor as a `0.0..=1.0` fraction.
    pub fn strong_fraction(self) -> f32 {
        f32::from(self.strong) / f32::from(u16::MAX)
    }

    /// The weak motor as a `0.0..=1.0` fraction.
    pub fn weak_fraction(self) -> f32 {
        f32::from(self.weak) / f32::from(u16::MAX)
    }

    pub fn is_idle(self) -> bool {
        self.strong == 0 && self.weak == 0
    }
}

#[derive(Debug)]
struct Inner {
    enabled: bool,
    ports: [RumbleMotors; MAX_PORTS],
    /// Bumped on every change that is actually a change, so a host can tell "still rumbling at
    /// the same strength" from "a new pulse at the same strength" without comparing floats.
    generation: u64,
}

/// The rumble table. One instance is the process global [`RUMBLE`]; tests build their own.
#[derive(Debug)]
pub struct Rumble {
    inner: Mutex<Inner>,
}

impl Default for Rumble {
    fn default() -> Self {
        Self::new()
    }
}

impl Rumble {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                enabled: true,
                ports: [RumbleMotors { strong: 0, weak: 0 }; MAX_PORTS],
                generation: 0,
            }),
        }
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, Inner> {
        match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// The core's `set_rumble_state`. Returns what libretro's return value means: whether the
    /// request was honoured.
    ///
    /// Refused (false, nothing stored) when rumble is switched off, for a port past
    /// [`MAX_PORTS`], and for an effect number libretro does not define. A core is required to
    /// cope with false, and answering true while discarding the value would be a claim this host
    /// does not keep.
    pub fn set(&self, port: u32, effect: u32, strength: u16) -> bool {
        let mut inner = self.guard();
        if !inner.enabled {
            return false;
        }
        let Some(motors) = inner.ports.get_mut(port as usize) else {
            return false;
        };
        let slot = match effect {
            RETRO_RUMBLE_STRONG => &mut motors.strong,
            RETRO_RUMBLE_WEAK => &mut motors.weak,
            _ => return false,
        };
        if *slot != strength {
            *slot = strength;
            inner.generation = inner.generation.wrapping_add(1);
        }
        true
    }

    /// The latest strengths for one port. Idle for a port past [`MAX_PORTS`].
    pub fn reading(&self, port: u32) -> RumbleMotors {
        self.guard()
            .ports
            .get(port as usize)
            .copied()
            .unwrap_or_default()
    }

    /// Bumped whenever any motor on any port changed.
    pub fn generation(&self) -> u64 {
        self.guard().generation
    }

    /// Stops every motor. Called when a session starts, stops or resets, because a core that was
    /// torn down mid-rumble will never send the zero that ends it, and the phone would buzz on
    /// through the library.
    pub fn clear(&self) {
        let mut inner = self.guard();
        if inner.ports.iter().any(|m| !m.is_idle()) {
            inner.ports = [RumbleMotors::default(); MAX_PORTS];
            inner.generation = inner.generation.wrapping_add(1);
        }
    }

    /// The user's Rumble setting. Switching it off also stops whatever is playing, so turning
    /// the toggle off mid-rumble is felt at once rather than at the core's next change.
    pub fn set_enabled(&self, enabled: bool) {
        let mut inner = self.guard();
        inner.enabled = enabled;
        if !enabled && inner.ports.iter().any(|m| !m.is_idle()) {
            inner.ports = [RumbleMotors::default(); MAX_PORTS];
            inner.generation = inner.generation.wrapping_add(1);
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.guard().enabled
    }
}

/// The table the native core's callback writes and the host reads.
pub static RUMBLE: Rumble = Rumble::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_idle_and_enabled() {
        let rumble = Rumble::new();
        assert!(rumble.is_enabled());
        for port in 0..MAX_PORTS as u32 {
            assert!(rumble.reading(port).is_idle());
        }
        assert_eq!(rumble.generation(), 0);
    }

    #[test]
    fn strong_and_weak_are_independent_motors() {
        let rumble = Rumble::new();
        assert!(rumble.set(0, RETRO_RUMBLE_STRONG, 0xffff));
        assert!(rumble.set(0, RETRO_RUMBLE_WEAK, 0x8000));
        let reading = rumble.reading(0);
        assert_eq!(reading.strong, 0xffff);
        assert_eq!(reading.weak, 0x8000);
        // Setting one does not disturb the other, which is libretro's own wording.
        assert!(rumble.set(0, RETRO_RUMBLE_STRONG, 0));
        assert_eq!(
            rumble.reading(0),
            RumbleMotors {
                strong: 0,
                weak: 0x8000
            }
        );
    }

    #[test]
    fn ports_are_independent() {
        let rumble = Rumble::new();
        assert!(rumble.set(1, RETRO_RUMBLE_STRONG, 100));
        assert!(rumble.reading(0).is_idle());
        assert_eq!(rumble.reading(1).strong, 100);
    }

    #[test]
    fn fractions_span_zero_to_one() {
        let full = RumbleMotors {
            strong: u16::MAX,
            weak: 0,
        };
        assert!((full.strong_fraction() - 1.0).abs() < f32::EPSILON);
        assert_eq!(full.weak_fraction(), 0.0);
    }

    #[test]
    fn out_of_range_port_and_unknown_effect_are_refused() {
        let rumble = Rumble::new();
        assert!(!rumble.set(MAX_PORTS as u32, RETRO_RUMBLE_STRONG, 1));
        assert!(!rumble.set(0, 2, 1));
        assert!(rumble.reading(MAX_PORTS as u32).is_idle());
        assert!(rumble.reading(0).is_idle());
        assert_eq!(rumble.generation(), 0);
    }

    #[test]
    fn disabled_refuses_and_stops_what_was_playing() {
        let rumble = Rumble::new();
        assert!(rumble.set(0, RETRO_RUMBLE_WEAK, 500));
        rumble.set_enabled(false);
        assert!(!rumble.is_enabled());
        assert!(
            rumble.reading(0).is_idle(),
            "switching off must stop the motor"
        );
        assert!(!rumble.set(0, RETRO_RUMBLE_WEAK, 500));
        assert!(rumble.reading(0).is_idle());
        rumble.set_enabled(true);
        assert!(rumble.set(0, RETRO_RUMBLE_WEAK, 500));
        assert_eq!(rumble.reading(0).weak, 500);
    }

    #[test]
    fn generation_moves_only_on_a_real_change() {
        let rumble = Rumble::new();
        rumble.set(0, RETRO_RUMBLE_STRONG, 10);
        let after_first = rumble.generation();
        rumble.set(0, RETRO_RUMBLE_STRONG, 10);
        assert_eq!(rumble.generation(), after_first, "a repeat is not a change");
        rumble.set(0, RETRO_RUMBLE_STRONG, 11);
        assert!(rumble.generation() > after_first);
    }

    #[test]
    fn clear_stops_every_port() {
        let rumble = Rumble::new();
        rumble.set(0, RETRO_RUMBLE_STRONG, 1);
        rumble.set(3, RETRO_RUMBLE_WEAK, 2);
        let before = rumble.generation();
        rumble.clear();
        for port in 0..MAX_PORTS as u32 {
            assert!(rumble.reading(port).is_idle());
        }
        assert!(rumble.generation() > before);
        // Clearing an idle table is not a change.
        let idle = rumble.generation();
        rumble.clear();
        assert_eq!(rumble.generation(), idle);
    }
}
