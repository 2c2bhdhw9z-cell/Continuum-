//! The phone's own hardware handed to a core: microphone, camera, and Amiibo files.
//!
//! Self-contained on purpose. `cores/native_core.rs` touches this module in exactly four places:
//! [`try_environment`] at the top of `on_environment`, [`reset_for_load`] before a core is
//! initialised, [`before_retro_run`] before each `retro_run`, and [`before_unload`] before
//! `retro_unload_game`. Everything else, the libretro structs, the callbacks and the state they
//! share with Swift, lives here, and none of it needs `libloading`, so the whole module is tested
//! by the plain host `cargo test`.

use std::ffi::{c_uint, c_void};

pub mod amiibo;
pub mod camera;
pub mod mic;

/// What Swift should have running right now. Polled from the display link; every field is an
/// atomic read, no lock is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeripheralRequests {
    /// A core holds a microphone and the user allows it: run the input tap, use `.playAndRecord`.
    pub microphone_wanted: bool,
    /// The rate the core reads at, for the HUD (Rust resamples to it).
    pub microphone_rate: u32,
    /// A core started the camera and the user allows it: run the capture session.
    pub camera_wanted: bool,
    pub camera_width: u32,
    pub camera_height: u32,
}

pub fn requests() -> PeripheralRequests {
    let (camera_width, camera_height) = camera::hub().requested_size();
    PeripheralRequests {
        microphone_wanted: mic::hub().wants_capture(),
        microphone_rate: mic::hub().rate(),
        camera_wanted: camera::hub().wants_capture(),
        camera_width,
        camera_height,
    }
}

/// The two HUD sentences, microphone then camera, joined.
pub fn status_line() -> String {
    format!(
        "{}; {}",
        mic::hub().status_line(),
        camera::hub().status_line()
    )
}

/// Answers the environment commands this module owns, or `None` for every other command.
///
/// # Safety
///
/// `data` is whatever the core passed to the environment callback for `cmd`.
pub unsafe fn try_environment(cmd: c_uint, data: *mut c_void) -> Option<bool> {
    match cmd {
        mic::ENV_GET_MICROPHONE_INTERFACE => Some(unsafe { mic::answer_interface(data) }),
        camera::ENV_GET_CAMERA_INTERFACE => Some(unsafe { camera::answer_interface(data) }),
        _ => None,
    }
}

/// Before `retro_set_environment` for a new core: nothing the last core held carries over.
pub fn reset_for_load() {
    mic::hub().reset_core_side();
    camera::hub().reset_core_side();
}

/// On the core's thread, immediately before `retro_run`.
pub fn before_retro_run() {
    camera::before_retro_run();
}

/// On the core's thread, before `retro_unload_game`.
pub fn before_unload() {
    camera::before_unload();
    mic::hub().reset_core_side();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_commands_are_not_claimed() {
        // GET_CAN_DUPE (3) and SET_HW_RENDER (14) belong to native_core.rs.
        assert_eq!(unsafe { try_environment(3, std::ptr::null_mut()) }, None);
        assert_eq!(unsafe { try_environment(14, std::ptr::null_mut()) }, None);
        // Ours, with null data, are claimed and refused.
        assert_eq!(
            unsafe { try_environment(mic::ENV_GET_MICROPHONE_INTERFACE, std::ptr::null_mut()) },
            Some(false)
        );
        assert_eq!(
            unsafe { try_environment(camera::ENV_GET_CAMERA_INTERFACE, std::ptr::null_mut()) },
            Some(false)
        );
    }
}
