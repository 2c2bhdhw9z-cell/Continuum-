//! Disc and disk swapping: `SET_DISK_CONTROL_INTERFACE` (libretro.h:924),
//! `SET_DISK_CONTROL_EXT_INTERFACE` (libretro.h:2038) and `GET_DISK_CONTROL_INTERFACE_VERSION`
//! (libretro.h:2020).
//!
//! A core hands over a table of function pointers during `retro_init` or `retro_load_game`. They are
//! stored here, tagged with the core they came from, and only ever called from under the engine
//! lock, so a swap can never overlap `retro_run`. Cleared when that core's dylib is released.
//!
//! An `.m3u` playlist reaches the core as a path and the core expands it into images itself; that is
//! the whole of the `.m3u` support a frontend needs, plus this swap. The Famicom Disk System in
//! FCEUmm does not use this interface at all: it flips the side on a press of L and ejects on R
//! (fceumm libretro.c, `FCEU_FDSSelect` / `FCEU_FDSInsert`), so the engine handles it as button
//! pulses in `bridge.rs` instead.

use std::ffi::{c_char, c_uint, c_void, CStr};
use std::sync::Mutex;

pub type SetEjectState = unsafe extern "C" fn(bool) -> bool;
pub type GetEjectState = unsafe extern "C" fn() -> bool;
pub type GetImageIndex = unsafe extern "C" fn() -> c_uint;
pub type SetImageIndex = unsafe extern "C" fn(c_uint) -> bool;
pub type GetNumImages = unsafe extern "C" fn() -> c_uint;
pub type ReplaceImageIndex = unsafe extern "C" fn(c_uint, *const c_void) -> bool;
pub type AddImageIndex = unsafe extern "C" fn() -> bool;
pub type SetInitialImage = unsafe extern "C" fn(c_uint, *const c_char) -> bool;
pub type GetImageString = unsafe extern "C" fn(c_uint, *mut c_char, usize) -> bool;

/// `struct retro_disk_control_callback` (libretro.h:6148).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RetroDiskControlCallback {
    pub set_eject_state: Option<SetEjectState>,
    pub get_eject_state: Option<GetEjectState>,
    pub get_image_index: Option<GetImageIndex>,
    pub set_image_index: Option<SetImageIndex>,
    pub get_num_images: Option<GetNumImages>,
    pub replace_image_index: Option<ReplaceImageIndex>,
    pub add_image_index: Option<AddImageIndex>,
}

/// `struct retro_disk_control_ext_callback` (libretro.h:6179): the same seven, then three more.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RetroDiskControlExtCallback {
    pub set_eject_state: Option<SetEjectState>,
    pub get_eject_state: Option<GetEjectState>,
    pub get_image_index: Option<GetImageIndex>,
    pub set_image_index: Option<SetImageIndex>,
    pub get_num_images: Option<GetNumImages>,
    pub replace_image_index: Option<ReplaceImageIndex>,
    pub add_image_index: Option<AddImageIndex>,
    pub set_initial_image: Option<SetInitialImage>,
    pub get_image_path: Option<GetImageString>,
    pub get_image_label: Option<GetImageString>,
}

/// The version this host reports: 1, so cores register the EXT table with labels.
pub const INTERFACE_VERSION: c_uint = 1;

#[derive(Clone, Copy)]
struct Registered {
    callbacks: RetroDiskControlExtCallback,
    extended: bool,
}

struct Slot {
    core_id: String,
    registered: Option<Registered>,
}

static SLOT: Mutex<Slot> = Mutex::new(Slot {
    core_id: String::new(),
    registered: None,
});

fn with_slot<R>(f: impl FnOnce(&mut Slot) -> R) -> R {
    let mut guard = match SLOT.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    f(&mut guard)
}

/// A new core is about to be opened: whatever the last one registered is void.
pub fn reset_for_core(core_id: &str) {
    with_slot(|s| {
        s.core_id = core_id.to_owned();
        s.registered = None;
    });
}

/// The core's dylib is being released.
pub fn forget_core(core_id: &str) {
    with_slot(|s| {
        if s.core_id == core_id {
            s.registered = None;
        }
    });
}

/// # Safety
/// `data` is null or a valid `retro_disk_control_callback`.
pub unsafe fn register_basic(data: *const RetroDiskControlCallback) {
    let registered = if data.is_null() {
        None
    } else {
        let b = unsafe { *data };
        Some(Registered {
            callbacks: RetroDiskControlExtCallback {
                set_eject_state: b.set_eject_state,
                get_eject_state: b.get_eject_state,
                get_image_index: b.get_image_index,
                set_image_index: b.set_image_index,
                get_num_images: b.get_num_images,
                replace_image_index: b.replace_image_index,
                add_image_index: b.add_image_index,
                set_initial_image: None,
                get_image_path: None,
                get_image_label: None,
            },
            extended: false,
        })
    };
    with_slot(|s| s.registered = registered);
}

/// # Safety
/// `data` is null or a valid `retro_disk_control_ext_callback`.
pub unsafe fn register_ext(data: *const RetroDiskControlExtCallback) {
    let registered = if data.is_null() {
        None
    } else {
        Some(Registered {
            callbacks: unsafe { *data },
            extended: true,
        })
    };
    with_slot(|s| s.registered = registered);
}

fn registered_for(core_id: &str) -> Option<Registered> {
    with_slot(|s| (s.core_id == core_id).then_some(s.registered).flatten())
}

/// Whether the core registered a disk interface.
pub fn available(core_id: &str) -> bool {
    registered_for(core_id).is_some()
}

/// What the disc menu shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiskStatus {
    pub count: u32,
    pub index: u32,
    pub ejected: bool,
    pub labels: Vec<String>,
    /// The core registered the labelled (EXT) interface.
    pub extended: bool,
}

fn read_string(f: Option<GetImageString>, index: u32) -> Option<String> {
    let f = f?;
    let mut buffer = [0 as c_char; 512];
    if !unsafe { f(index, buffer.as_mut_ptr(), buffer.len()) } {
        return None;
    }
    buffer[buffer.len() - 1] = 0;
    let text = unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_string_lossy().into_owned();
    (!text.is_empty()).then_some(text)
}

/// A label for one image: the core's label, else the file name from its path, else "Disc N".
pub fn label_from(label: Option<String>, path: Option<String>, index: u32) -> String {
    if let Some(label) = label {
        return label;
    }
    if let Some(path) = path {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_owned();
        if !name.is_empty() {
            return name;
        }
    }
    format!("Disc {}", index + 1)
}

/// The current state. Call only under the engine lock.
pub fn status(core_id: &str) -> Option<DiskStatus> {
    let r = registered_for(core_id)?;
    let c = r.callbacks;
    let count = c.get_num_images.map_or(0, |f| unsafe { f() });
    let index = c.get_image_index.map_or(0, |f| unsafe { f() });
    let ejected = c.get_eject_state.is_some_and(|f| unsafe { f() });
    let labels = (0..count.min(64))
        .map(|i| label_from(read_string(c.get_image_label, i), read_string(c.get_image_path, i), i))
        .collect();
    Some(DiskStatus {
        count,
        index,
        ejected,
        labels,
        extended: r.extended,
    })
}

/// Opens the tray, selects `index`, closes it: the sequence libretro.h:6012 asks for.
pub fn insert(core_id: &str, index: u32) -> Result<String, String> {
    let r = registered_for(core_id).ok_or_else(|| "this game has no discs to swap".to_owned())?;
    let c = r.callbacks;
    let (Some(set_eject), Some(set_index)) = (c.set_eject_state, c.set_image_index) else {
        return Err("the core did not give a way to change discs".to_owned());
    };
    let count = c.get_num_images.map_or(0, |f| unsafe { f() });
    if count == 0 {
        return Err("the core reports no discs".to_owned());
    }
    if index >= count {
        return Err(format!("there is no disc {} (the game has {count})", index + 1));
    }
    let was_ejected = c.get_eject_state.is_some_and(|f| unsafe { f() });
    if !was_ejected && !unsafe { set_eject(true) } {
        return Err("the core would not open the disc tray".to_owned());
    }
    if !unsafe { set_index(index) } {
        // Close the tray again on the old disc rather than leaving it open.
        unsafe { set_eject(false) };
        return Err(format!("the core refused disc {}", index + 1));
    }
    if !unsafe { set_eject(false) } {
        return Err("the core would not close the disc tray".to_owned());
    }
    let label = status(core_id)
        .and_then(|s| s.labels.get(index as usize).cloned())
        .unwrap_or_else(|| format!("Disc {}", index + 1));
    Ok(format!("inserted {label} ({} of {count})", index + 1))
}

/// The next disc, wrapping after the last.
pub fn swap_next(core_id: &str) -> Result<String, String> {
    let s = status(core_id).ok_or_else(|| "this game has no discs to swap".to_owned())?;
    if s.count < 2 {
        return Err(format!("this game has {} disc, nothing to swap to", s.count));
    }
    insert(core_id, (s.index + 1) % s.count)
}

#[cfg(test)]
pub static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    static INDEX: AtomicU32 = AtomicU32::new(0);
    static EJECTED: AtomicBool = AtomicBool::new(false);
    static SET_WHILE_CLOSED: AtomicBool = AtomicBool::new(false);

    unsafe extern "C" fn set_eject(e: bool) -> bool {
        EJECTED.store(e, Ordering::SeqCst);
        true
    }
    unsafe extern "C" fn get_eject() -> bool {
        EJECTED.load(Ordering::SeqCst)
    }
    unsafe extern "C" fn get_index() -> c_uint {
        INDEX.load(Ordering::SeqCst)
    }
    unsafe extern "C" fn set_index(i: c_uint) -> bool {
        if !EJECTED.load(Ordering::SeqCst) {
            SET_WHILE_CLOSED.store(true, Ordering::SeqCst);
        }
        if i >= 3 {
            return false;
        }
        INDEX.store(i, Ordering::SeqCst);
        true
    }
    unsafe extern "C" fn num() -> c_uint {
        3
    }
    unsafe extern "C" fn label(i: c_uint, s: *mut c_char, len: usize) -> bool {
        if i == 2 {
            return false;
        }
        let text = format!("Disc {} label\0", i + 1);
        let n = text.len().min(len);
        unsafe { std::ptr::copy_nonoverlapping(text.as_ptr().cast::<c_char>(), s, n) };
        true
    }
    unsafe extern "C" fn path(_i: c_uint, s: *mut c_char, len: usize) -> bool {
        let text = b"/games/Final Disc.chd\0";
        let n = text.len().min(len);
        unsafe { std::ptr::copy_nonoverlapping(text.as_ptr().cast::<c_char>(), s, n) };
        true
    }

    fn guard() -> std::sync::MutexGuard<'static, ()> {
        match TEST_LOCK.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }

    fn ext() -> RetroDiskControlExtCallback {
        RetroDiskControlExtCallback {
            set_eject_state: Some(set_eject),
            get_eject_state: Some(get_eject),
            get_image_index: Some(get_index),
            set_image_index: Some(set_index),
            get_num_images: Some(num),
            replace_image_index: None,
            add_image_index: None,
            set_initial_image: None,
            get_image_path: Some(path),
            get_image_label: Some(label),
        }
    }

    #[test]
    fn the_ext_struct_is_the_basic_one_plus_three_pointers() {
        let p = std::mem::size_of::<usize>();
        assert_eq!(std::mem::size_of::<RetroDiskControlCallback>(), 7 * p);
        assert_eq!(std::mem::size_of::<RetroDiskControlExtCallback>(), 10 * p);
    }

    #[test]
    fn swap_opens_selects_closes_and_wraps() {
        let _g = guard();
        INDEX.store(0, Ordering::SeqCst);
        EJECTED.store(false, Ordering::SeqCst);
        SET_WHILE_CLOSED.store(false, Ordering::SeqCst);
        reset_for_core("psx");
        let table = ext();
        unsafe { register_ext(&table) };
        let s = status("psx").unwrap();
        assert_eq!(s.count, 3);
        assert_eq!(s.labels, ["Disc 1 label", "Disc 2 label", "Final Disc.chd"]);
        assert!(s.extended);
        assert!(swap_next("psx").unwrap().contains("Disc 2 label"));
        assert!(swap_next("psx").unwrap().contains("Final Disc.chd"));
        assert!(swap_next("psx").unwrap().contains("1 of 3"));
        assert!(!EJECTED.load(Ordering::SeqCst), "tray closed afterwards");
        assert!(!SET_WHILE_CLOSED.load(Ordering::SeqCst), "never swapped with the tray shut");
        assert!(insert("psx", 7).is_err());
        assert!(status("other").is_none(), "only the core that registered it");
        forget_core("psx");
        assert!(status("psx").is_none());
    }

    #[test]
    fn the_basic_interface_has_numbered_labels() {
        let _g = guard();
        INDEX.store(0, Ordering::SeqCst);
        reset_for_core("pcsx");
        let basic = RetroDiskControlCallback {
            set_eject_state: Some(set_eject),
            get_eject_state: Some(get_eject),
            get_image_index: Some(get_index),
            set_image_index: Some(set_index),
            get_num_images: Some(num),
            replace_image_index: None,
            add_image_index: None,
        };
        unsafe { register_basic(&basic) };
        let s = status("pcsx").unwrap();
        assert_eq!(s.labels, ["Disc 1", "Disc 2", "Disc 3"]);
        assert!(!s.extended);
        unsafe { register_basic(std::ptr::null()) };
        assert!(!available("pcsx"), "NULL deregisters");
    }

    #[test]
    fn labels_fall_back_to_the_file_name() {
        assert_eq!(label_from(None, Some("C:\\x\\Disc B.cue".into()), 1), "Disc B.cue");
        assert_eq!(label_from(None, None, 0), "Disc 1");
        assert_eq!(label_from(Some("Side A".into()), None, 0), "Side A");
    }
}
