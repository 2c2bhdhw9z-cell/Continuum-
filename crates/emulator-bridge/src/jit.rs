//! JIT: can this copy of the app run recompilers right now, and what the engine does about it.
//!
//! ## The rule (owner, 7 October 2026)
//!
//! Everything must keep working without JIT, and anyone who CAN turn JIT on must get it, used
//! automatically, by every core that has a recompiler. The owner signs with a certificate that
//! cannot carry `get-task-allow`, so their phone always takes the no-JIT path; testers who install
//! with a development profile and attach a JIT enabler (StikDebug and friends) take the fast one.
//!
//! ## How JIT happens on an iPhone
//!
//! An app may only make memory executable while it is being debugged. A JIT enabler attaches a
//! debugger to the app and lets go again; from then on the kernel's `CS_DEBUGGED` flag stays set
//! for the life of the process and executable pages are allowed. That needs `get-task-allow` in
//! the signature (development profiles only), and nothing else.
//!
//! **Except on iOS 26 phones that have TXM** (the Trusted Execution Monitor: A15 and newer
//! iPhones). There the flag alone is not enough: every executable region has to be prepared
//! through the debugger before it is used, with a breakpoint protocol the app's own JIT allocator
//! has to speak (StikJIT's INTEGRATION.md, "universal" script). None of the cores this app ships
//! speaks it, so on those phones the engine reports JIT as attached but not usable and keeps every
//! core on its interpreter. Treating it as usable would crash the first time a core made a page
//! executable. A breakpoint is NEVER executed here: one without the right script attached kills
//! the process.
//!
//! ## What "using JIT" means per core
//!
//! - PPSSPP and Azahar decide at runtime: they ask `RETRO_ENVIRONMENT_GET_JIT_CAPABLE`, which the
//!   host answers from [`capable_answer`]. Without JIT the answer is no and they stay exactly as
//!   they were.
//! - PCSX ReARMed, parallel-n64 and flycast choose at COMPILE time, so each has a second build
//!   with its recompiler on, `<core>_jit_libretro_ios.dylib`. [`library_for`] picks it when JIT is
//!   usable and the file is in the bundle; otherwise the ordinary build loads, unchanged.
//! - [`core_runs_jit_build`] tells the option rules which build is loaded, so a recompiler option
//!   is only ever answered "on" to a build that has one.

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Where this copy of the app stands on JIT right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JitState {
    /// Not an iPhone build (the test machine). Never usable.
    NotThisPlatform,
    /// The signing flags could not be read.
    Unreadable,
    /// Signed without `get-task-allow`: no debugger can attach, so JIT can never be on.
    CannotBeEnabled,
    /// `get-task-allow` is there; nothing has attached yet.
    NotEnabled,
    /// A debugger attached, but this phone needs the iOS 26 region protocol (TXM).
    NeedsNewerMethod,
    /// Usable.
    On,
}

impl JitState {
    /// A short code for logs and feedback details.
    pub fn code(self) -> &'static str {
        match self {
            JitState::NotThisPlatform => "not-ios",
            JitState::Unreadable => "unreadable",
            JitState::CannotBeEnabled => "no-get-task-allow",
            JitState::NotEnabled => "not-enabled",
            JitState::NeedsNewerMethod => "txm",
            JitState::On => "on",
        }
    }

    /// One line a tester reads in Settings. Casual and short on purpose (owner rule: every line a
    /// tester sees must sound like a person wrote it).
    pub fn sentence(self, allowed: bool) -> &'static str {
        match (self, allowed) {
            (JitState::On, true) => "On. Games that can use it run faster.",
            (JitState::On, false) => "Turned off below. Games use the regular mode.",
            (JitState::NotEnabled, _) => {
                "Off. Turn it on with StikDebug (or whatever JIT app you use), then come back. \
                 Everything works without it, just slower on the heavy systems."
            }
            (JitState::CannotBeEnabled, _) => {
                "Off. The way this copy was signed doesn't allow JIT. That's fine, everything \
                 still works, just slower on the heavy systems."
            }
            (JitState::NeedsNewerMethod, _) => {
                "Off. JIT is attached, but on iOS 26 this iPhone needs a newer kind of JIT \
                 support that Continuum doesn't have yet. Everything still works without it."
            }
            (JitState::Unreadable, _) | (JitState::NotThisPlatform, _) => {
                "Off. Couldn't check this copy, so games use the regular mode."
            }
        }
    }
}

/// The user's "Use JIT when it's available" switch. On unless they turn it off.
static ALLOWED: AtomicBool = AtomicBool::new(true);
/// What [`capable_answer`] says, frozen at each core load so a core asking twice hears the same.
static CAPABLE_AT_LOAD: AtomicBool = AtomicBool::new(false);
/// Core ids currently loaded from their `_jit_` build.
static JIT_BUILDS: Mutex<Option<HashSet<String>>> = Mutex::new(None);

pub fn set_allowed(allowed: bool) {
    ALLOWED.store(allowed, Ordering::SeqCst);
}

pub fn allowed() -> bool {
    ALLOWED.load(Ordering::SeqCst)
}

/// Read live: JIT can be switched on while the app is open, but never off again.
pub fn state() -> JitState {
    #[cfg(all(target_os = "ios", target_arch = "aarch64"))]
    {
        use crate::jit_probe::ios_aarch64::{signing_status, CS_DEBUGGED, CS_GET_TASK_ALLOW};
        let Some(status) = signing_status() else {
            return JitState::Unreadable;
        };
        let os_major = device::os_major().unwrap_or(0);
        let machine = device::machine().unwrap_or_default();
        classify(
            status & CS_GET_TASK_ALLOW != 0,
            status & CS_DEBUGGED != 0,
            device_has_txm(&machine, os_major),
        )
    }
    #[cfg(not(all(target_os = "ios", target_arch = "aarch64")))]
    {
        JitState::NotThisPlatform
    }
}

/// The decision itself, apart from the system calls, so it can be tested.
pub fn classify(get_task_allow: bool, debugged: bool, txm: bool) -> JitState {
    match (get_task_allow, debugged, txm) {
        (false, false, _) => JitState::CannotBeEnabled,
        (_, false, _) => JitState::NotEnabled,
        (_, true, true) => JitState::NeedsNewerMethod,
        // Debugged without get-task-allow cannot normally happen; if it does, the kernel already
        // allows executable memory, which is all that matters.
        (_, true, false) => JitState::On,
    }
}

/// Whether this phone needs the iOS 26 region protocol, whatever its JIT state.
pub fn this_device_has_txm() -> bool {
    #[cfg(all(target_os = "ios", target_arch = "aarch64"))]
    {
        device_has_txm(&device::machine().unwrap_or_default(), device::os_major().unwrap_or(0))
    }
    #[cfg(not(all(target_os = "ios", target_arch = "aarch64")))]
    {
        false
    }
}

/// Whether the cores may use JIT right now: it is on, and the user has not switched it off.
pub fn usable() -> bool {
    allowed() && state() == JitState::On
}

/// Whether an iPhone or iPad needs the iOS 26 region protocol, from its model identifier
/// (`iPhone14,2`) and the iOS major version.
///
/// The table is StikDebug's (`ProcessInfo+TXM.swift`), as DolphiniOS uses it: on iOS 26 the
/// iPhone14,2 (A15) and newer and the iPad14,5 and newer have it; from iOS 27 every device except
/// the iPad8,11 and iPad8,12 does; before iOS 26 it never matters. An identifier that cannot be
/// read on iOS 26 or later counts as TXM: guessing "no" would crash the first recompiler.
pub fn device_has_txm(machine: &str, os_major: u32) -> bool {
    if os_major < 26 {
        return false;
    }
    if os_major >= 27 {
        return machine != "iPad8,11" && machine != "iPad8,12";
    }
    let parse = |prefix: &str| -> Option<(u32, u32)> {
        let rest = machine.strip_prefix(prefix)?;
        let (major, minor) = rest.split_once(',')?;
        Some((major.parse().ok()?, minor.parse().ok()?))
    };
    if let Some(model) = parse("iPhone") {
        return model >= (14, 2);
    }
    if let Some(model) = parse("iPad") {
        return model >= (14, 5);
    }
    true
}

/// `fceumm_libretro_ios.dylib` -> `fceumm_jit_libretro_ios.dylib`. `None` for a name that is not
/// a core dylib, or is already a JIT build.
pub fn jit_library_name(library: &str) -> Option<String> {
    const TAIL: &str = "_libretro_ios.dylib";
    let stem = library.strip_suffix(TAIL)?;
    if stem.is_empty() || stem.ends_with("_jit") {
        return None;
    }
    Some(format!("{stem}_jit{TAIL}"))
}

/// Whether a dylib path is one of the `_jit_` builds.
pub fn is_jit_library(path: &str) -> bool {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with("_jit_libretro_ios.dylib"))
}

/// The dylib to load for a core: its JIT build when JIT is usable and that file is in
/// `frameworks_dir`, else the ordinary one.
pub fn library_for(frameworks_dir: &str, library: &str) -> String {
    pick_library(library, usable(), |name| Path::new(frameworks_dir).join(name).is_file())
}

/// [`library_for`] without the system calls.
pub fn pick_library(library: &str, usable: bool, exists: impl Fn(&str) -> bool) -> String {
    if usable {
        if let Some(jit) = jit_library_name(library) {
            if exists(&jit) {
                return jit;
            }
        }
    }
    library.to_owned()
}

/// Called just before a core is loaded, with the dylib it is loaded from. Freezes the answer to
/// `GET_JIT_CAPABLE` and records which build the option rules are talking to.
pub fn note_core_load(core_id: &str, library_path: &str) {
    CAPABLE_AT_LOAD.store(usable(), Ordering::SeqCst);
    let mut guard = JIT_BUILDS.lock().unwrap_or_else(|p| p.into_inner());
    let set = guard.get_or_insert_with(HashSet::new);
    if is_jit_library(library_path) {
        set.insert(core_id.to_owned());
    } else {
        set.remove(core_id);
    }
    log::info!(
        "JIT {} (allowed: {}); {core_id} loads {}",
        state().code(),
        allowed(),
        if is_jit_library(library_path) { "its JIT build" } else { "its regular build" }
    );
}

/// What the host answers to `RETRO_ENVIRONMENT_GET_JIT_CAPABLE`.
pub fn capable_answer() -> bool {
    CAPABLE_AT_LOAD.load(Ordering::SeqCst)
}

/// Whether `core_id` is running its `_jit_` build.
pub fn core_runs_jit_build(core_id: &str) -> bool {
    let guard = JIT_BUILDS.lock().unwrap_or_else(|p| p.into_inner());
    guard.as_ref().is_some_and(|set| set.contains(core_id))
}

/// The technical line for the (i) panel and the feedback details.
pub fn technical_line() -> String {
    let state = state();
    let device = {
        #[cfg(all(target_os = "ios", target_arch = "aarch64"))]
        {
            format!(
                ", {} on iOS {}",
                device::machine().unwrap_or_else(|| "unknown".into()),
                device::os_major().map(|v| v.to_string()).unwrap_or_else(|| "?".into())
            )
        }
        #[cfg(not(all(target_os = "ios", target_arch = "aarch64")))]
        {
            String::new()
        }
    };
    format!(
        "JIT: {}{}{device}",
        state.code(),
        if allowed() { "" } else { ", switched off in Settings" }
    )
}

#[cfg(all(target_os = "ios", target_arch = "aarch64"))]
mod device {
    use core::ffi::{c_char, c_void, CStr};

    extern "C" {
        fn sysctlbyname(
            name: *const c_char,
            oldp: *mut c_void,
            oldlenp: *mut usize,
            newp: *mut c_void,
            newlen: usize,
        ) -> i32;
    }

    fn read_string(name: &CStr) -> Option<String> {
        let mut buffer = [0u8; 128];
        let mut len = buffer.len();
        // SAFETY: the buffer and its length are passed together, and nothing is written.
        let rc = unsafe {
            sysctlbyname(
                name.as_ptr(),
                buffer.as_mut_ptr() as *mut c_void,
                &mut len,
                core::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 || len == 0 {
            return None;
        }
        let text = CStr::from_bytes_until_nul(&buffer[..len.min(buffer.len())]).ok()?;
        Some(text.to_string_lossy().into_owned())
    }

    /// The model identifier, for example `iPhone14,2`.
    pub fn machine() -> Option<String> {
        read_string(c"hw.machine").filter(|m| !m.is_empty())
    }

    /// The iOS major version: 18, 26 and so on.
    pub fn os_major() -> Option<u32> {
        if let Some(version) = read_string(c"kern.osproductversion") {
            if let Some(major) = version.split('.').next().and_then(|m| m.parse().ok()) {
                return Some(major);
            }
        }
        // The Darwin version instead: iOS 18 is Darwin 24, and iOS 26 is Darwin 25.
        let darwin: u32 = read_string(c"kern.osrelease")?.split('.').next()?.parse().ok()?;
        Some(if darwin >= 25 { darwin + 1 } else { darwin.saturating_sub(6) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txm_follows_stikdebugs_table() {
        assert!(!device_has_txm("iPhone17,1", 18));
        assert!(!device_has_txm("iPhone11,2", 26));
        assert!(!device_has_txm("iPhone13,4", 26));
        assert!(device_has_txm("iPhone14,2", 26));
        assert!(device_has_txm("iPhone14,7", 26));
        assert!(device_has_txm("iPhone18,2", 26));
        assert!(!device_has_txm("iPad14,1", 26));
        assert!(device_has_txm("iPad14,5", 26));
        assert!(device_has_txm("iPad14,10", 26));
        assert!(device_has_txm("iPhone12,1", 27));
        assert!(!device_has_txm("iPad8,11", 27));
        // Unreadable on iOS 26: assume the strict case.
        assert!(device_has_txm("", 26));
    }

    #[test]
    fn the_state_follows_the_signing_flags() {
        assert_eq!(classify(false, false, false), JitState::CannotBeEnabled);
        assert_eq!(classify(true, false, false), JitState::NotEnabled);
        assert_eq!(classify(true, false, true), JitState::NotEnabled);
        assert_eq!(classify(true, true, false), JitState::On);
        assert_eq!(classify(true, true, true), JitState::NeedsNewerMethod);
    }

    #[test]
    fn jit_builds_are_named_and_picked() {
        assert_eq!(
            jit_library_name("pcsx_rearmed_libretro_ios.dylib").as_deref(),
            Some("pcsx_rearmed_jit_libretro_ios.dylib")
        );
        assert_eq!(jit_library_name("pcsx_rearmed_jit_libretro_ios.dylib"), None);
        assert_eq!(jit_library_name("libcontinuum_switch.dylib"), None);
        assert!(is_jit_library("/x/Frameworks/flycast_jit_libretro_ios.dylib"));
        assert!(!is_jit_library("/x/Frameworks/flycast_libretro_ios.dylib"));

        let present = |name: &str| name == "flycast_jit_libretro_ios.dylib";
        assert_eq!(
            pick_library("flycast_libretro_ios.dylib", true, present),
            "flycast_jit_libretro_ios.dylib"
        );
        // No JIT: always the regular build.
        assert_eq!(
            pick_library("flycast_libretro_ios.dylib", false, present),
            "flycast_libretro_ios.dylib"
        );
        // JIT but no JIT build in the bundle: the regular build.
        assert_eq!(
            pick_library("ppsspp_libretro_ios.dylib", true, present),
            "ppsspp_libretro_ios.dylib"
        );
    }

    #[test]
    fn on_this_machine_jit_is_never_usable() {
        assert_eq!(state(), JitState::NotThisPlatform);
        assert!(!usable());
        assert!(technical_line().starts_with("JIT: not-ios"));
    }
}
