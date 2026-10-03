//! The keyboard: `RETRO_DEVICE_KEYBOARD` state and `RETRO_ENVIRONMENT_SET_KEYBOARD_CALLBACK`.
//!
//! DOS, the C64 and the Amiga are computers, and a computer game asks for a key by name: "press
//! F1", "type your name", "Y/N". A core learns about keys in two ways and the big computer cores
//! use both, so both are served:
//!
//! - It POLLS: `input_state(port, RETRO_DEVICE_KEYBOARD, 0, RETROK_x)` answers whether that key
//!   is down. That is answered from [`KeyboardState`], which lives in every input layer and is
//!   merged (OR-ed) into the frame's snapshot exactly like the pad buttons, so a hardware keyboard
//!   and the on-screen keyboard can be used together without one cancelling the other.
//! - It is CALLED BACK: a core that registered a `retro_keyboard_event_t` gets every press and
//!   release, with the character it typed. That call is made on the core's own thread, inside the
//!   frame, immediately before `retro_run` ([`deliver_pending`]). It is NEVER made from a UIKit
//!   handler: the key handler only queues the event, so the core is never re-entered from
//!   another thread or outside a frame.
//!
//! The keycodes are libretro's `enum retro_key` (libretro.h:529-701), which are what both the
//! host and the core speak, so nothing is translated between them.

use std::collections::VecDeque;
use std::ffi::{c_uint, c_void};
use std::sync::Mutex;

/// `RETRO_DEVICE_KEYBOARD` (libretro.h:219).
pub const RETRO_DEVICE_KEYBOARD: u32 = 3;
/// `RETRO_ENVIRONMENT_SET_KEYBOARD_CALLBACK` (libretro.h:900).
pub const ENV_SET_KEYBOARD_CALLBACK: c_uint = 12;

/// `RETROK_LAST` (libretro.h:698): one past `RETROK_LAUNCH_APP2 = 341`.
pub const RETROK_LAST: u32 = 342;

// The keys this module treats specially. Values from `enum retro_key` (libretro.h:529-701).
pub const RETROK_UNKNOWN: u32 = 0;
pub const RETROK_BACKSPACE: u32 = 8;
pub const RETROK_TAB: u32 = 9;
pub const RETROK_RETURN: u32 = 13;
pub const RETROK_ESCAPE: u32 = 27;
pub const RETROK_SPACE: u32 = 32;
pub const RETROK_DELETE: u32 = 127;
pub const RETROK_NUMLOCK: u32 = 300;
pub const RETROK_CAPSLOCK: u32 = 301;
pub const RETROK_SCROLLOCK: u32 = 302;
pub const RETROK_RSHIFT: u32 = 303;
pub const RETROK_LSHIFT: u32 = 304;
pub const RETROK_RCTRL: u32 = 305;
pub const RETROK_LCTRL: u32 = 306;
pub const RETROK_RALT: u32 = 307;
pub const RETROK_LALT: u32 = 308;
pub const RETROK_RMETA: u32 = 309;
pub const RETROK_LMETA: u32 = 310;
pub const RETROK_LSUPER: u32 = 311;
pub const RETROK_RSUPER: u32 = 312;

// `enum retro_mod` (libretro.h:703-717).
pub const RETROKMOD_SHIFT: u16 = 0x01;
pub const RETROKMOD_CTRL: u16 = 0x02;
pub const RETROKMOD_ALT: u16 = 0x04;
pub const RETROKMOD_META: u16 = 0x08;
pub const RETROKMOD_NUMLOCK: u16 = 0x10;
pub const RETROKMOD_CAPSLOCK: u16 = 0x20;
pub const RETROKMOD_SCROLLOCK: u16 = 0x40;

const WORDS: usize = (RETROK_LAST as usize).div_ceil(64);

/// Which keys are down, one bit per `retro_key`. `Copy`, so it rides in an `InputSnapshot`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyboardState {
    bits: [u64; WORDS],
}

impl KeyboardState {
    /// Sets one key. Keycodes outside `enum retro_key` (and `RETROK_UNKNOWN`) are ignored, so a
    /// host that sends a key libretro has no name for cannot set a stray bit.
    pub fn set(&mut self, key: u32, down: bool) {
        if key == RETROK_UNKNOWN || key >= RETROK_LAST {
            return;
        }
        let (word, bit) = ((key / 64) as usize, key % 64);
        if down {
            self.bits[word] |= 1 << bit;
        } else {
            self.bits[word] &= !(1 << bit);
        }
    }

    pub fn is_down(&self, key: u32) -> bool {
        if key >= RETROK_LAST {
            return false;
        }
        self.bits[(key / 64) as usize] & (1 << (key % 64)) != 0
    }

    pub fn any_down(&self) -> bool {
        self.bits.iter().any(|w| *w != 0)
    }

    /// The union of two layers.
    pub fn merge(&mut self, other: &KeyboardState) {
        for (mine, theirs) in self.bits.iter_mut().zip(other.bits.iter()) {
            *mine |= *theirs;
        }
    }

    /// Every key that is down here, in keycode order.
    pub fn held_keys(&self) -> Vec<u32> {
        (1..RETROK_LAST).filter(|k| self.is_down(*k)).collect()
    }

    /// The held modifiers as `RETROKMOD_*` bits. Lock keys are not levels and are added by the
    /// caller from its own toggles.
    pub fn held_modifiers(&self) -> u16 {
        let mut mods = 0;
        if self.is_down(RETROK_LSHIFT) || self.is_down(RETROK_RSHIFT) {
            mods |= RETROKMOD_SHIFT;
        }
        if self.is_down(RETROK_LCTRL) || self.is_down(RETROK_RCTRL) {
            mods |= RETROKMOD_CTRL;
        }
        if self.is_down(RETROK_LALT) || self.is_down(RETROK_RALT) {
            mods |= RETROKMOD_ALT;
        }
        if self.is_down(RETROK_LMETA)
            || self.is_down(RETROK_RMETA)
            || self.is_down(RETROK_LSUPER)
            || self.is_down(RETROK_RSUPER)
        {
            mods |= RETROKMOD_META;
        }
        mods
    }
}

/// One event for the core's keyboard callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub down: bool,
    pub keycode: u32,
    /// UTF-32, 0 when the key types nothing (and on every release).
    pub character: u32,
    pub modifiers: u16,
}

/// `retro_keyboard_event_t` (libretro.h:5929).
pub type RetroKeyboardEvent = unsafe extern "C" fn(bool, c_uint, u32, u16);

/// `struct retro_keyboard_callback` (libretro.h:5932). One field, so its order is the layout.
#[repr(C)]
pub struct RetroKeyboardCallback {
    pub callback: Option<RetroKeyboardEvent>,
}

/// Events waiting for the next frame. Bounded so a core that never runs (paused, or one that
/// registered a callback and then stalled) cannot grow it without limit; the oldest go first.
const MAX_PENDING: usize = 256;

struct Hub {
    callback: Option<RetroKeyboardEvent>,
    pending: VecDeque<KeyEvent>,
    /// The lock keys are toggles, so they are tracked here rather than read from a level.
    locks: u16,
    delivered: u64,
}

/// Process-global, for the reason every libretro callback target is: the core holds a bare C
/// function pointer with no user data, and registers it from inside `retro_load_game`.
static HUB: Mutex<Hub> = Mutex::new(Hub {
    callback: None,
    pending: VecDeque::new(),
    locks: 0,
    delivered: 0,
});

fn hub() -> std::sync::MutexGuard<'static, Hub> {
    match HUB.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// `SET_KEYBOARD_CALLBACK`: keeps the core's function. A null struct is refused; a null function
/// inside it is accepted and clears the callback, which is how a core says "stop".
///
/// # Safety
/// `data` is what the core passed for command 12: `const struct retro_keyboard_callback *`.
pub unsafe fn answer_set_keyboard_callback(data: *mut c_void) -> bool {
    if data.is_null() {
        return false;
    }
    let callback = unsafe { (*(data as *const RetroKeyboardCallback)).callback };
    let mut hub = hub();
    hub.callback = callback;
    hub.pending.clear();
    true
}

/// Whether the running core asked to be told about key events.
pub fn has_callback() -> bool {
    hub().callback.is_some()
}

/// Lock-key toggles as `RETROKMOD_*` bits.
pub fn lock_modifiers() -> u16 {
    hub().locks
}

/// Queues one event for the core. Updates the lock toggles on a lock key's press. Dropped when no
/// core has a callback, because nothing would ever drain it.
pub fn queue(event: KeyEvent) {
    let mut hub = hub();
    if event.down {
        let toggle = match event.keycode {
            RETROK_CAPSLOCK => RETROKMOD_CAPSLOCK,
            RETROK_NUMLOCK => RETROKMOD_NUMLOCK,
            RETROK_SCROLLOCK => RETROKMOD_SCROLLOCK,
            _ => 0,
        };
        hub.locks ^= toggle;
    }
    if hub.callback.is_none() {
        return;
    }
    if hub.pending.len() >= MAX_PENDING {
        hub.pending.pop_front();
    }
    let locks = hub.locks;
    hub.pending.push_back(KeyEvent {
        modifiers: event.modifiers | locks,
        ..event
    });
}

/// On the core's thread, immediately before `retro_run`: hands every queued event to the core's
/// callback, in order. The lock is released before the first call, so a core that queries input
/// or the environment from inside its callback cannot deadlock on it.
pub fn deliver_pending() -> usize {
    let (callback, events) = {
        let mut hub = hub();
        let Some(callback) = hub.callback else {
            hub.pending.clear();
            return 0;
        };
        if hub.pending.is_empty() {
            return 0;
        }
        let events: Vec<KeyEvent> = hub.pending.drain(..).collect();
        hub.delivered += events.len() as u64;
        (callback, events)
    };
    for event in &events {
        unsafe { callback(event.down, event.keycode, event.character, event.modifiers) };
    }
    events.len()
}

/// Events handed to the core since it was loaded, for the HUD.
pub fn delivered_count() -> u64 {
    hub().delivered
}

/// A new core is being loaded: the last one's callback must never be called again.
pub fn reset_for_load() {
    let mut hub = hub();
    hub.callback = None;
    hub.pending.clear();
    hub.locks = 0;
    hub.delivered = 0;
}

/// Test serialisation: the hub is process-global, so tests that use it take this first.
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn guard() -> std::sync::MutexGuard<'static, ()> {
        match TEST_LOCK.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }

    #[test]
    fn keys_set_and_clear_and_unknown_is_ignored() {
        let mut keys = KeyboardState::default();
        keys.set(97, true); // RETROK_a
        keys.set(341, true); // RETROK_LAUNCH_APP2, the last real key
        keys.set(RETROK_LAST, true);
        keys.set(RETROK_UNKNOWN, true);
        assert!(keys.is_down(97) && keys.is_down(341));
        assert!(!keys.is_down(RETROK_LAST) && !keys.is_down(0));
        assert_eq!(keys.held_keys(), vec![97, 341]);
        keys.set(97, false);
        assert!(!keys.is_down(97));
    }

    #[test]
    fn modifiers_come_from_either_side() {
        let mut keys = KeyboardState::default();
        keys.set(RETROK_RSHIFT, true);
        keys.set(RETROK_LCTRL, true);
        assert_eq!(keys.held_modifiers(), RETROKMOD_SHIFT | RETROKMOD_CTRL);
    }

    static SEEN: Mutex<Vec<(bool, u32, u32, u16)>> = Mutex::new(Vec::new());
    static CALLS: AtomicU32 = AtomicU32::new(0);

    unsafe extern "C" fn record(down: bool, key: c_uint, character: u32, mods: u16) {
        CALLS.fetch_add(1, Ordering::SeqCst);
        SEEN.lock().unwrap().push((down, key, character, mods));
    }

    fn register() {
        let mut cb = RetroKeyboardCallback {
            callback: Some(record),
        };
        assert!(unsafe { answer_set_keyboard_callback(&mut cb as *mut _ as *mut c_void) });
    }

    #[test]
    fn events_wait_for_the_frame_and_arrive_in_order() {
        let _g = guard();
        reset_for_load();
        SEEN.lock().unwrap().clear();
        register();
        queue(KeyEvent { down: true, keycode: 97, character: 'a' as u32, modifiers: 0 });
        queue(KeyEvent { down: false, keycode: 97, character: 0, modifiers: 0 });
        // Nothing reaches the core until the frame delivers it.
        assert!(SEEN.lock().unwrap().is_empty());
        assert_eq!(deliver_pending(), 2);
        assert_eq!(
            *SEEN.lock().unwrap(),
            vec![(true, 97, 'a' as u32, 0), (false, 97, 0, 0)]
        );
        // Delivered once only.
        assert_eq!(deliver_pending(), 0);
        reset_for_load();
    }

    #[test]
    fn caps_lock_is_a_toggle_carried_on_later_events() {
        let _g = guard();
        reset_for_load();
        SEEN.lock().unwrap().clear();
        register();
        queue(KeyEvent { down: true, keycode: RETROK_CAPSLOCK, character: 0, modifiers: 0 });
        queue(KeyEvent { down: false, keycode: RETROK_CAPSLOCK, character: 0, modifiers: 0 });
        queue(KeyEvent { down: true, keycode: 97, character: 'A' as u32, modifiers: 0 });
        deliver_pending();
        let seen = SEEN.lock().unwrap().clone();
        assert_eq!(seen[2].3 & RETROKMOD_CAPSLOCK, RETROKMOD_CAPSLOCK);
        reset_for_load();
    }

    #[test]
    fn no_callback_means_nothing_queues_and_a_new_core_forgets_the_old_one() {
        let _g = guard();
        reset_for_load();
        queue(KeyEvent { down: true, keycode: 97, character: 0, modifiers: 0 });
        assert_eq!(deliver_pending(), 0);
        register();
        queue(KeyEvent { down: true, keycode: 98, character: 0, modifiers: 0 });
        reset_for_load();
        assert!(!has_callback());
        assert_eq!(deliver_pending(), 0, "the old core's queue must not reach a new core");
    }

    #[test]
    fn the_queue_is_bounded() {
        let _g = guard();
        reset_for_load();
        register();
        for _ in 0..(MAX_PENDING + 50) {
            queue(KeyEvent { down: true, keycode: 99, character: 0, modifiers: 0 });
        }
        let before = CALLS.load(Ordering::SeqCst);
        assert_eq!(deliver_pending(), MAX_PENDING);
        assert_eq!(CALLS.load(Ordering::SeqCst) - before, MAX_PENDING as u32);
        reset_for_load();
    }

    #[test]
    fn null_struct_is_refused() {
        assert!(!unsafe { answer_set_keyboard_callback(std::ptr::null_mut()) });
    }

    #[test]
    fn command_number_matches_libretro_h() {
        assert_eq!(ENV_SET_KEYBOARD_CALLBACK, 12);
        assert_eq!(RETRO_DEVICE_KEYBOARD, 3);
        assert_eq!(RETROK_LAST, 342);
    }
}
