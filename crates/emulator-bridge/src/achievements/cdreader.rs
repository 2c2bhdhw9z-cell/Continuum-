//! Hands rcheevos a disc reader that opens CHD and CSO itself (see [`super::disc`]) and passes
//! every other path to rcheevos' own reader, the arrangement RetroArch uses for CHD
//! (`rc_hash_reset_cdreader_hooks` in its `cheevos.c`).
//!
//! Installed once, process-wide, with `rc_hash_init_custom_cdreader` (rc_hash.h line 96), which
//! every hash iterator copies when it is set up (`rc_hash_reset_iterator_disc`). RetroArch swaps the
//! iterator's read callbacks per handle by casting away a `const`; here every handle is a Rust box
//! saying whose it is instead, so the one table serves both kinds.

use std::ffi::{c_char, c_void, CStr};
use std::sync::OnceLock;

use super::disc::{self, Track};

type OpenTrack = unsafe extern "C" fn(*const c_char, u32) -> *mut c_void;
type ReadSector = unsafe extern "C" fn(*mut c_void, u32, *mut c_void, usize) -> usize;
type CloseTrack = unsafe extern "C" fn(*mut c_void);
type FirstTrackSector = unsafe extern "C" fn(*mut c_void) -> u32;
type OpenTrackIterator = unsafe extern "C" fn(*const c_char, u32, *const c_void) -> *mut c_void;

/// `struct rc_hash_cdreader`, rc_hash.h lines 83 to 90. Field order is the layout; `RC_CCONV` is
/// empty off Windows.
#[repr(C)]
#[derive(Clone, Copy)]
struct RcHashCdreader {
    open_track: Option<OpenTrack>,
    read_sector: Option<ReadSector>,
    close_track: Option<CloseTrack>,
    first_track_sector: Option<FirstTrackSector>,
    open_track_iterator: Option<OpenTrackIterator>,
}

extern "C" {
    fn rc_hash_get_default_cdreader(reader: *mut RcHashCdreader);
    fn rc_hash_init_custom_cdreader(reader: *mut RcHashCdreader);
}

/// rcheevos' own reader, kept for every path that is not ours.
static DEFAULT: OnceLock<RcHashCdreader> = OnceLock::new();

enum Handle {
    Theirs(*mut c_void),
    Ours(Track),
}

/// Installs the reader. Safe to call more than once; only the first call does anything.
pub fn install() {
    DEFAULT.get_or_init(|| {
        let mut default = RcHashCdreader {
            open_track: None,
            read_sector: None,
            close_track: None,
            first_track_sector: None,
            open_track_iterator: None,
        };
        unsafe { rc_hash_get_default_cdreader(&mut default) };
        let mut ours = RcHashCdreader {
            open_track: None,
            read_sector: Some(read_sector),
            close_track: Some(close_track),
            first_track_sector: Some(first_track_sector),
            open_track_iterator: Some(open_track_iterator),
        };
        unsafe { rc_hash_init_custom_cdreader(&mut ours) };
        default
    });
}

fn boxed(handle: Handle) -> *mut c_void {
    Box::into_raw(Box::new(handle)).cast()
}

// A panic must not cross into C, so each entry point catches one and answers "nothing".

unsafe extern "C" fn open_track_iterator(
    path: *const c_char,
    track: u32,
    iterator: *const c_void,
) -> *mut c_void {
    if path.is_null() {
        return std::ptr::null_mut();
    }
    let text = unsafe { CStr::from_ptr(path) }
        .to_string_lossy()
        .into_owned();
    if disc::handles(&text) {
        return std::panic::catch_unwind(|| disc::open(&text, track))
            .ok()
            .flatten()
            .map_or(std::ptr::null_mut(), |t| boxed(Handle::Ours(t)));
    }
    let Some(open) = DEFAULT.get().and_then(|d| d.open_track_iterator) else {
        return std::ptr::null_mut();
    };
    let theirs = unsafe { open(path, track, iterator) };
    if theirs.is_null() {
        std::ptr::null_mut()
    } else {
        boxed(Handle::Theirs(theirs))
    }
}

unsafe extern "C" fn read_sector(
    handle: *mut c_void,
    sector: u32,
    buffer: *mut c_void,
    requested: usize,
) -> usize {
    if handle.is_null() || buffer.is_null() {
        return 0;
    }
    match unsafe { &mut *(handle as *mut Handle) } {
        Handle::Theirs(inner) => match DEFAULT.get().and_then(|d| d.read_sector) {
            Some(read) => unsafe { read(*inner, sector, buffer, requested) },
            None => 0,
        },
        Handle::Ours(track) => {
            let out = unsafe { std::slice::from_raw_parts_mut(buffer as *mut u8, requested) };
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                track.read_sector(sector, out)
            }))
            .unwrap_or(0)
        }
    }
}

unsafe extern "C" fn first_track_sector(handle: *mut c_void) -> u32 {
    if handle.is_null() {
        return 0;
    }
    match unsafe { &*(handle as *const Handle) } {
        Handle::Theirs(inner) => match DEFAULT.get().and_then(|d| d.first_track_sector) {
            Some(first) => unsafe { first(*inner) },
            None => 0,
        },
        Handle::Ours(track) => track.first_sector(),
    }
}

unsafe extern "C" fn close_track(handle: *mut c_void) {
    if handle.is_null() {
        return;
    }
    let handle = unsafe { Box::from_raw(handle as *mut Handle) };
    if let Handle::Theirs(inner) = *handle {
        if let Some(close) = DEFAULT.get().and_then(|d| d.close_track) {
            unsafe { close(inner) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cso_opens_reads_and_closes_through_the_c_entry_points() {
        install();
        install();
        let dir = crate::import::testdir::TestDir::new("rc-reader");
        let mut iso = vec![0u8; 2048 * 20];
        iso[16 * 2048..16 * 2048 + 6].copy_from_slice(b"\x01CD001");
        iso[18 * 2048] = 0x5A;
        assert!(
            !disc::handles("g.iso"),
            "plain ISOs stay with rcheevos' own reader"
        );
        // A CSO through the C-shaped entry points.
        let cso = disc::tests_support::cso_of(&iso, 2048, 1);
        let cso_path = std::ffi::CString::new(dir.write("g.cso", &cso).to_str().unwrap()).unwrap();
        unsafe {
            let handle = open_track_iterator(cso_path.as_ptr(), 1, std::ptr::null());
            assert!(!handle.is_null());
            assert_eq!(first_track_sector(handle), 0);
            let mut byte = [0u8; 1];
            assert_eq!(read_sector(handle, 18, byte.as_mut_ptr().cast(), 1), 1);
            assert_eq!(byte[0], 0x5A);
            close_track(handle);
            assert!(open_track_iterator(std::ptr::null(), 1, std::ptr::null()).is_null());
            let missing = std::ffi::CString::new("/no/such.chd").unwrap();
            assert!(open_track_iterator(missing.as_ptr(), 1, std::ptr::null()).is_null());
        }
    }
}
