//! Is MoltenVK actually in this process, and does it answer?
//!
//! **Step 3 of the sequence in `docs/SET_HW_RENDER_DESIGN.md` §13, and the riskiest unknown left in
//! the project.** Everything commercially interesting sits on top of it: Dreamcast, PSP, the 3DS and
//! a fast N64 all render through OpenGL or Vulkan, and none of them can be reached while this app
//! can only display pixels a core rasterised in software. The design's answer is MoltenVK in
//! process, sharing the one `MTLDevice` the engine already owns, with a `VkImage` backed by an
//! `MTLTexture` so the handoff costs no copy.
//!
//! This module does not do any of that yet. It answers the question that has to be answered first
//! and cannot be answered from a build machine: **does the framework load on the device at all, and
//! does it report a working Vulkan?** A triangle is worth nothing until that is true, and if it is
//! false then the four graphics steps after this one are unbuildable and it is better to know now.
//!
//! # Why this does so little on purpose
//!
//! `dlopen` and two function calls. No instance, no device, no structs.
//!
//! The two calls were chosen because they are the only useful ones in the whole API that need **no
//! struct definitions at all**: `vkEnumerateInstanceVersion` takes a single `u32` out-parameter, and
//! `vkEnumerateInstanceExtensionProperties` can be called with a null array purely to get a count.
//! Hand-declaring `VkInstanceCreateInfo` and its chain would be a large surface of layout guesses
//! whose failure mode is memory corruption, for no more information than these two give: if
//! MoltenVK loads and reports a version and a plausible extension count, it initialised its Metal
//! backend and the path is open.
//!
//! # The lesson this module is built around
//!
//! **Nothing here runs at startup that can take the app down.** An earlier probe in this project
//! wrote machine code into a page and called it, on the launch path, which iOS terminates a process
//! for; the app could not be opened at all and every on-device test was blocked for several builds
//! without anyone knowing why. So this is `dlopen` plus two reads, every failure is a returned
//! string, and the framework is deliberately NOT linked: a missing or broken MoltenVK has to be a
//! line on the diagnostics panel rather than an app that will not launch.

use core::ffi::{c_char, c_int, c_void};
use std::ffi::{CStr, CString};

// dlopen and friends are public POSIX API on every target this builds for, so unlike `csops` in
// `jit_probe` these are safe to declare for the linker rather than resolve through `dlsym`.
extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *mut c_char;
}

/// Resolve every symbol now rather than lazily, so a framework that is present but unusable fails
/// here with a message instead of at the first call.
const RTLD_NOW: c_int = 0x2;
/// Keep the symbols out of the global namespace. This library is only ever reached through the
/// handle returned below, and a Vulkan loader publishing `vk*` globally could shadow something.
const RTLD_LOCAL: c_int = 0x4;

/// Where MoltenVK lives inside the app bundle, relative to the Frameworks directory.
///
/// A framework rather than a bare dylib because that is the only shape MoltenVK publishes a DYNAMIC
/// iOS arm64 build in: `MoltenVK-ios.tar` carries `static/…/libMoltenVK.a` and
/// `dynamic/MoltenVK.xcframework/ios-arm64/MoltenVK.framework`, and only the second can be
/// `dlopen`ed.
const FRAMEWORK_SUFFIX: &str = "MoltenVK.framework/MoltenVK";

/// One line for the diagnostics panel: whether Vulkan is reachable, and what it said.
///
/// `frameworks_dir` is the bundle's Frameworks directory, passed in because only the host knows it.
/// The full path is used rather than a bare library name on purpose: a bare name leaves the answer
/// to dyld's search order, which differs between a simulator and a device and is exactly the kind
/// of difference that produces "works for me" on one and silence on the other.
pub fn describe(frameworks_dir: &str) -> String {
    let path = format!("{}/{}", frameworks_dir.trim_end_matches('/'), FRAMEWORK_SUFFIX);
    let Ok(c_path) = CString::new(path.clone()) else {
        return "Vulkan: the Frameworks path contains a NUL byte, which is not a path".to_string();
    };

    // SAFETY: a NUL-terminated path, and the handle is only used with `dlsym` below. Deliberately
    // never closed: the loader is process-wide state that wgpu's Vulkan backend will want to find
    // already resident later, and closing it could unload a library another part of the process is
    // mid-way through using.
    let handle = unsafe { dlopen(c_path.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
    if handle.is_null() {
        // SAFETY: `dlerror` returns a NUL-terminated string owned by the loader, or null.
        let reason = unsafe {
            let raw = dlerror();
            if raw.is_null() {
                "no reason given".to_string()
            } else {
                CStr::from_ptr(raw).to_string_lossy().into_owned()
            }
        };
        return format!(
            "Vulkan: NOT AVAILABLE, MoltenVK did not load from {path} ({reason}). Hardware \
             rendered cores are impossible on this build, so Dreamcast, PSP and 3DS are out of \
             reach until it does"
        );
    }

    let version = unsafe { instance_version(handle) };
    let extensions = unsafe { instance_extension_count(handle) };

    match (version, extensions) {
        (Some(version), Some(count)) => format!(
            "Vulkan: MoltenVK loaded and answered. API {}, {} instance extension(s). The zero-copy \
             path to Metal is open",
            format_api_version(version),
            count
        ),
        (Some(version), None) => format!(
            "Vulkan: MoltenVK loaded and reports API {}, but would not list its instance \
             extensions, which a working loader always can",
            format_api_version(version)
        ),
        (None, _) => format!(
            "Vulkan: MoltenVK loaded from {path} but does not export \
             vkEnumerateInstanceVersion, so it is not a usable Vulkan 1.1 or later loader"
        ),
    }
}

/// `vkEnumerateInstanceVersion`, which needs no structs: one `u32` out-parameter.
///
/// # Safety
///
/// `handle` must be a live `dlopen` handle.
unsafe fn instance_version(handle: *mut c_void) -> Option<u32> {
    type EnumerateInstanceVersion = unsafe extern "C" fn(*mut u32) -> i32;
    let symbol = unsafe { dlsym(handle, c"vkEnumerateInstanceVersion".as_ptr()) };
    if symbol.is_null() {
        return None;
    }
    let enumerate: EnumerateInstanceVersion = unsafe { core::mem::transmute(symbol) };
    let mut version: u32 = 0;
    // VK_SUCCESS is 0. Any other result means the loader refused, which is worth reporting as
    // absent rather than as a version of zero.
    if unsafe { enumerate(&mut version) } == 0 {
        Some(version)
    } else {
        None
    }
}

/// `vkEnumerateInstanceExtensionProperties` called for the COUNT ONLY.
///
/// Passing a null array is how Vulkan is asked how many there are, and it is the reason this can be
/// called without declaring `VkExtensionProperties`. A plausible non-zero count is evidence the
/// loader initialised its driver rather than merely being a library that opened.
///
/// # Safety
///
/// `handle` must be a live `dlopen` handle.
unsafe fn instance_extension_count(handle: *mut c_void) -> Option<u32> {
    type EnumerateExtensions =
        unsafe extern "C" fn(*const c_char, *mut u32, *mut c_void) -> i32;
    let symbol = unsafe { dlsym(handle, c"vkEnumerateInstanceExtensionProperties".as_ptr()) };
    if symbol.is_null() {
        return None;
    }
    let enumerate: EnumerateExtensions = unsafe { core::mem::transmute(symbol) };
    let mut count: u32 = 0;
    // Null layer name means "the implementation's own extensions", null array means "just count".
    if unsafe { enumerate(core::ptr::null(), &mut count, core::ptr::null_mut()) } == 0 {
        Some(count)
    } else {
        None
    }
}

/// Vulkan packs its version into one word: 7 bits of major, 10 of minor, 12 of patch.
///
/// The top three bits are a variant field added in 1.3 headers and are masked off rather than
/// shifted into the major number, which is what makes a 1.x loader read as 1 and not as 128.
fn format_api_version(version: u32) -> String {
    let major = (version >> 22) & 0x7F;
    let minor = (version >> 12) & 0x3FF;
    let patch = version & 0xFFF;
    format!("{major}.{minor}.{patch}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_path_with_no_framework_reports_that_rather_than_pretending() {
        // The host has no MoltenVK, which is the point: the failure has to arrive as a readable
        // line rather than as a panic or a crash, because on a phone this string is the only
        // diagnostic there is.
        let line = super::describe("/definitely/not/a/real/frameworks/dir");
        assert!(line.starts_with("Vulkan: NOT AVAILABLE"), "unexpected line: {line}");
        assert!(line.contains("MoltenVK did not load"));
    }

    #[test]
    fn a_nul_byte_in_the_path_is_refused_without_panicking() {
        let line = super::describe("/frameworks\0/nope");
        assert!(line.contains("NUL byte"), "unexpected line: {line}");
    }

    #[test]
    fn version_words_decode_the_way_vulkan_packs_them() {
        // VK_MAKE_API_VERSION(0, 1, 2, 198) is what MoltenVK 1.2 era loaders report.
        let packed = (1u32 << 22) | (2u32 << 12) | 198;
        assert_eq!(super::format_api_version(packed), "1.2.198");
        // A variant in the top bits must not leak into the major number.
        let with_variant = packed | (1u32 << 29);
        assert_eq!(super::format_api_version(with_variant), "1.2.198");
    }
}
