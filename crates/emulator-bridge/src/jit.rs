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
    /// A debugger attached, and this phone needs its code region prepared first (iOS 26 + TXM).
    /// One call away from [`JitState::On`]: see [`prepare_now`].
    NeedsPreparing,
    /// The region protocol was tried on this phone and did not work. Cores stay on interpreters.
    PrepareFailed,
    /// Usable.
    On,
}

impl JitState {
    /// A short phrase for the technical read-out, the activity log and the feedback details.
    ///
    /// Words rather than codes, because the owner reads this line on the phone and nothing in the
    /// app branches on it. "New-phone protection" is TXM: the thing that makes an iPhone 13 or
    /// newer on iOS 26 need its code memory blessed by the debugger first.
    pub fn code(self) -> &'static str {
        match self {
            JitState::NotThisPlatform => "not an iPhone build",
            JitState::Unreadable => "could not check this copy",
            JitState::CannotBeEnabled => "off, this copy was not signed for JIT",
            JitState::NotEnabled => "off, no JIT app has attached",
            JitState::NeedsPreparing => "attached, needs setting up (new-phone protection)",
            JitState::PrepareFailed => "attached, but setting it up failed",
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
            (JitState::NeedsPreparing, _) => {
                "Nearly. JIT is attached and this iPhone needs one more step, which Continuum \
                 does by itself when you turn JIT on from the button below."
            }
            (JitState::PrepareFailed, _) => {
                "Off. JIT is attached but setting it up on this iPhone didn't work. Everything \
                 still works without it, just slower on the heavy systems. Please tell me, and \
                 say which iPhone and iOS version you're on."
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
            region_state(),
        )
    }
    #[cfg(not(all(target_os = "ios", target_arch = "aarch64")))]
    {
        JitState::NotThisPlatform
    }
}

/// The decision itself, apart from the system calls, so it can be tested.
///
/// `region` is what the prepared-region protocol has done so far: see [`RegionState`]. It only
/// matters on a TXM phone, where executable memory exists solely inside that region.
pub fn classify(get_task_allow: bool, debugged: bool, txm: bool, region: RegionState) -> JitState {
    match (get_task_allow, debugged, txm) {
        (false, false, _) => JitState::CannotBeEnabled,
        (_, false, _) => JitState::NotEnabled,
        (_, true, true) => match region {
            RegionState::Ready => JitState::On,
            RegionState::Failed => JitState::PrepareFailed,
            RegionState::NotPrepared => JitState::NeedsPreparing,
        },
        // Debugged without get-task-allow cannot normally happen; if it does, the kernel already
        // allows executable memory, which is all that matters.
        (_, true, false) => JitState::On,
    }
}

/// How far the iOS 26 prepared-region protocol has got. Mirrors `continuum_jit26_state` in
/// `jit26.c`, which owns the actual region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionState {
    NotPrepared,
    Ready,
    Failed,
}

// ---------------------------------------------------------------- the iOS 26 region protocol
//
// `jit26.c` holds the region and the breakpoint calls, and its header explains the protocol. This
// side decides WHEN it is safe to run, which is the part that matters: a breakpoint with no
// script attached kills the app.

extern "C" {
    fn continuum_jit26_prepare(size: usize) -> bool;
    fn continuum_jit26_state() -> i32;
    fn continuum_jit26_set_live(live: bool);
    fn continuum_jit26_region_size() -> usize;
    fn continuum_jit26_used() -> usize;
    /// Not called from Rust. Referenced once in [`keep_region_symbol`] so the linker cannot drop
    /// it from the app: the cores find it by name at run time.
    fn continuum_jit_region(
        owner: *const core::ffi::c_char,
        size: usize,
        out_rx: *mut *mut core::ffi::c_void,
        out_rw: *mut *mut core::ffi::c_void,
    ) -> bool;
}

/// How big a region to ask for, reserved in one piece because nothing can be prepared after the
/// debugger lets go.
///
/// Address space, not memory: pages cost nothing until a core writes to them. Sized for a whole
/// session rather than one game, because a slice is kept for each core that asks (so leaving a
/// game and coming back does not eat another slice): the PlayStation wants 16 MB, the N64 32 MB,
/// the Dreamcast about 12 MB, the PSP 16 MB, and the 3DS 32 MB per recompiler instance after the
/// Continuum patch that brings it down from 128 MB.
const REGION_SIZE: usize = 512 * 1024 * 1024;

/// Whether the app asked a JIT enabler for the universal script in this session.
///
/// THE GUARD ON RUNNING THE BREAKPOINT AT ALL. The protocol is a `brk` instruction that only the
/// universal script answers; with an ordinary debugger attached, or no script, it terminates the
/// process. The app cannot ask the system which script is attached, so it relies on having asked
/// for it itself (the StikDebug button builds the URL with `script-name=universal.js`), or on the
/// user pressing a button that says what it needs.
static ENABLER_ASKED: AtomicBool = AtomicBool::new(false);

/// Called just before opening a JIT enabler with the universal script requested.
pub fn set_enabler_asked() {
    ENABLER_ASKED.store(true, Ordering::SeqCst);
}

pub fn enabler_asked() -> bool {
    ENABLER_ASKED.load(Ordering::SeqCst)
}

pub fn region_state() -> RegionState {
    // SAFETY: a read of one `int` with no arguments.
    match unsafe { continuum_jit26_state() } {
        1 => RegionState::Ready,
        2 => RegionState::Failed,
        _ => RegionState::NotPrepared,
    }
}

/// Makes sure the symbol the cores look up survives the link.
///
/// The engine is a static library inside the app's executable, and nothing in the app calls
/// `continuum_jit_region`: the cores do, by name, at run time. Taking its address here is what
/// keeps it in the symbol table for `dlsym` to find.
pub fn keep_region_symbol() -> usize {
    continuum_jit_region as *const () as usize
}

/// Runs the protocol: reserve the region, have the debugger prepare it, keep a writable view.
///
/// **Only call this when the universal script is attached.** The two callers are
/// [`prepare_if_asked`], which requires that the app asked for it itself, and the explicit
/// "set JIT up now" control. Does nothing unless the state is exactly [`JitState::NeedsPreparing`],
/// so it cannot run on a phone that does not need it, cannot run before a debugger has attached,
/// and cannot run twice.
pub fn prepare_now() -> JitState {
    if state() != JitState::NeedsPreparing {
        return state();
    }
    log::info!("JIT: preparing a {} MB code region (iOS 26 protocol)", REGION_SIZE / (1024 * 1024));
    // SAFETY: the guard above establishes the one condition the call has: a debugger is attached
    // on a phone that needs this, so the script is listening for the breakpoint.
    let ok = unsafe { continuum_jit26_prepare(REGION_SIZE) };
    let after = state();
    log::info!("JIT: region prepared: {ok}; state is now {}", after.code());
    after
}

/// The automatic path, called whenever the app comes back to the front: if the app asked a JIT
/// enabler for the universal script and a debugger is now attached, finish the job.
pub fn prepare_if_asked() -> JitState {
    if enabler_asked() {
        return prepare_now();
    }
    state()
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

/// Whether this core may use JIT on this phone.
///
/// Everything can, except on a phone where code may only live inside the one region the debugger
/// blessed (iPhone 13 and newer on iOS 26). There, a core must be able to write its code through a
/// second address, and not all of them can:
///
/// - PCSX ReARMed, flycast, PPSSPP and Azahar can, and are patched to ask the host for the pair.
/// - parallel-n64 cannot yet: its jump trampolines are written through the same pointer they are
///   run from, so the N64 stays on its interpreter on those phones, exactly as it is today.
pub fn core_may_use_jit(core_id: &str, region_in_use: bool) -> bool {
    if !region_in_use {
        return true;
    }
    matches!(core_id, "pcsx_rearmed" | "flycast" | "ppsspp" | "azahar")
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

/// The dylib to load for a core: its JIT build when JIT is usable for that core and that file is
/// in `frameworks_dir`, else the ordinary one.
pub fn library_for(core_id: &str, frameworks_dir: &str, library: &str) -> String {
    let allowed = usable() && core_may_use_jit(core_id, region_state() == RegionState::Ready);
    pick_library(library, allowed, |name| Path::new(frameworks_dir).join(name).is_file())
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
    let capable = usable() && core_may_use_jit(core_id, region_state() == RegionState::Ready);
    CAPABLE_AT_LOAD.store(capable, Ordering::SeqCst);
    // The shim hands out no code memory unless this says JIT is really on, so a core that asks
    // anyway is refused here rather than failing somewhere inside itself.
    // SAFETY: setting one `bool`.
    unsafe { continuum_jit26_set_live(capable) };
    // Referenced so the linker keeps the symbol the cores look up by name.
    let _ = keep_region_symbol();
    let mut guard = JIT_BUILDS.lock().unwrap_or_else(|p| p.into_inner());
    let set = guard.get_or_insert_with(HashSet::new);
    if is_jit_library(library_path) {
        set.insert(core_id.to_owned());
    } else {
        set.remove(core_id);
    }
    log::info!(
        "JIT {} (allowed: {}); {core_id} loads {}; capable answer {capable}",
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
    let region = match region_state() {
        RegionState::NotPrepared => String::new(),
        RegionState::Ready => {
            // SAFETY: two reads of a `size_t`.
            let (size, used) = unsafe { (continuum_jit26_region_size(), continuum_jit26_used()) };
            format!(", region {} MB with {} MB handed out", size / (1024 * 1024), used / (1024 * 1024))
        }
        RegionState::Failed => ", region prepare failed".to_string(),
    };
    format!(
        "JIT: {}{}{device}{region}",
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
        let fresh = RegionState::NotPrepared;
        assert_eq!(classify(false, false, false, fresh), JitState::CannotBeEnabled);
        assert_eq!(classify(true, false, false, fresh), JitState::NotEnabled);
        assert_eq!(classify(true, false, true, fresh), JitState::NotEnabled);
        // No TXM: attaching is the whole job.
        assert_eq!(classify(true, true, false, fresh), JitState::On);
        assert_eq!(classify(true, true, false, RegionState::Failed), JitState::On);
        // TXM: the region decides.
        assert_eq!(classify(true, true, true, fresh), JitState::NeedsPreparing);
        assert_eq!(classify(true, true, true, RegionState::Ready), JitState::On);
        assert_eq!(classify(true, true, true, RegionState::Failed), JitState::PrepareFailed);
    }

    #[test]
    fn the_region_symbol_is_linked_in_and_does_nothing_here() {
        // The cores find this by name at run time, so it has to survive the link.
        assert_ne!(keep_region_symbol(), 0);
        // Nothing is prepared on the test machine, so a core asking is told to carry on as usual.
        assert_eq!(region_state(), RegionState::NotPrepared);
        let mut rx = core::ptr::null_mut();
        let mut rw = core::ptr::null_mut();
        // SAFETY: a NUL-terminated name and two real out-pointers.
        let given = unsafe { continuum_jit_region(c"test".as_ptr(), 4096, &mut rx, &mut rw) };
        assert!(!given);
    }

    #[test]
    fn preparing_does_nothing_unless_the_phone_needs_it() {
        // Not an iPhone here, so the breakpoint path is unreachable by construction.
        assert_eq!(prepare_now(), JitState::NotThisPlatform);
        assert_eq!(prepare_if_asked(), JitState::NotThisPlatform);
        set_enabler_asked();
        assert!(enabler_asked());
        assert_eq!(prepare_if_asked(), JitState::NotThisPlatform);
    }

    #[test]
    fn the_n64_jit_build_is_held_back_only_where_code_must_live_in_the_region() {
        // No blessed region (older phone, or iOS 18): every core may use JIT.
        for core in ["pcsx_rearmed", "parallel_n64", "flycast", "ppsspp", "azahar", "melonds"] {
            assert!(core_may_use_jit(core, false), "{core} without the region");
        }
        // With it, the N64 one is held back: it writes its trampolines where it runs them.
        assert!(!core_may_use_jit("parallel_n64", true));
        assert!(core_may_use_jit("pcsx_rearmed", true));
        assert!(core_may_use_jit("flycast", true));
        assert!(core_may_use_jit("ppsspp", true));
        assert!(core_may_use_jit("azahar", true));
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
        assert!(technical_line().starts_with("JIT: not an iPhone build"));
    }
}
