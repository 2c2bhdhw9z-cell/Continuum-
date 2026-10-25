//! The libretro calls with the graphics chip, driven the way the app's engine drives them
//! (crates/emulator-bridge/src/gfx/vulkan_hw.rs): the core asks for Vulkan during
//! retro_load_game, the frontend calls context_reset inside that request, the core fetches the
//! interface, starts the GPU drawer after its picture check, and pictures still come back as
//! ordinary frames. The device is this machine's (SwiftShader here); without Vulkan the test says
//! so and passes. Its own file: libretro's state is per process.

#[path = "../src/gx/gpu/test_vulkan.rs"]
mod test_vulkan;

use ash::vk::Handle;
use continuum_gc::libretro::*;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

#[repr(C)]
struct HwRenderCallback {
    context_type: u32,
    context_reset: Option<unsafe extern "C" fn()>,
    get_current_framebuffer: *const c_void,
    get_proc_address: *const c_void,
    depth: bool,
    stencil: bool,
    bottom_left_origin: bool,
    version_major: u32,
    version_minor: u32,
    cache_context: bool,
    context_destroy: Option<unsafe extern "C" fn()>,
    debug_context: bool,
}

#[repr(C)]
struct Interface {
    interface_type: u32,
    interface_version: u32,
    handle: *mut c_void,
    instance: *mut c_void,
    gpu: *mut c_void,
    device: *mut c_void,
    get_device_proc_addr: *const c_void,
    get_instance_proc_addr: *const c_void,
    queue: *mut c_void,
    queue_index: u32,
    set_image: *const c_void,
    get_sync_index: *const c_void,
    get_sync_index_mask: *const c_void,
    set_command_buffers: *const c_void,
    wait_sync_index: *const c_void,
    lock_queue: Option<unsafe extern "C" fn(*mut c_void)>,
    unlock_queue: Option<unsafe extern "C" fn(*mut c_void)>,
    set_signal_semaphore: *const c_void,
}

struct Shared(Interface);
// SAFETY: plain handles, only read.
unsafe impl Send for Shared {}

static IFACE: Mutex<Option<Box<Shared>>> = Mutex::new(None);
static HW_ASKED: AtomicBool = AtomicBool::new(false);
static RESETS: AtomicU32 = AtomicU32::new(0);
static LOCKS: AtomicU32 = AtomicU32::new(0);
static UNLOCKS: AtomicU32 = AtomicU32::new(0);
static DRAWING_CPU: AtomicBool = AtomicBool::new(false);
static FRAMES: AtomicU32 = AtomicU32::new(0);
static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

unsafe extern "C" fn lock(_h: *mut c_void) {
    LOCKS.fetch_add(1, Ordering::SeqCst);
}
unsafe extern "C" fn unlock(_h: *mut c_void) {
    UNLOCKS.fetch_add(1, Ordering::SeqCst);
}

/// The core's log calls are printf-style with one string; on this machine (x86-64) a plain
/// three-argument function receives them the same way.
unsafe extern "C" fn log(_level: u32, _fmt: *const c_char, text: *const c_char) {
    LOG.lock().unwrap().push(CStr::from_ptr(text).to_string_lossy().into_owned());
}

unsafe extern "C" fn env(cmd: u32, data: *mut c_void) -> bool {
    match cmd {
        10 => true,
        // GET_LOG_INTERFACE.
        27 => {
            *(data as *mut *const c_void) = log as *const c_void;
            true
        }
        // GET_VARIABLE: the Drawing setting.
        15 => {
            let var = data as *mut [*const c_char; 2];
            if CStr::from_ptr((*var)[0]).to_str() == Ok("cgc_drawing") {
                (*var)[1] = if DRAWING_CPU.load(Ordering::SeqCst) { c"cpu".as_ptr() } else { c"gpu".as_ptr() };
                return true;
            }
            false
        }
        // SET_HW_RENDER: Vulkan accepted, context_reset called inside the request (the app does
        // that when its device is ready, which it always is by the time a game starts).
        14 => {
            let cb = &mut *(data as *mut HwRenderCallback);
            assert_eq!(cb.context_type, 6, "Vulkan");
            HW_ASKED.store(true, Ordering::SeqCst);
            if IFACE.lock().unwrap().is_none() {
                return false;
            }
            RESETS.fetch_add(1, Ordering::SeqCst);
            (cb.context_reset.expect("a context_reset"))();
            true
        }
        // GET_HW_RENDER_INTERFACE.
        0x10029 => match IFACE.lock().unwrap().as_ref() {
            Some(i) => {
                *(data as *mut *const Interface) = &i.0;
                true
            }
            None => false,
        },
        _ => false,
    }
}
unsafe extern "C" fn video(d: *const c_void, w: u32, _h: u32, _p: usize) {
    assert!(d as usize != usize::MAX, "pictures are ordinary frames, never the hardware frame marker");
    assert!(d.is_null() || w == 640);
    FRAMES.fetch_add(1, Ordering::SeqCst);
}
unsafe extern "C" fn audio(_d: *const i16, n: usize) -> usize {
    n
}
unsafe extern "C" fn poll() {}
unsafe extern "C" fn input(_p: u32, _d: u32, _i: u32, _id: u32) -> i16 {
    0
}

fn program() -> std::path::PathBuf {
    let words: Vec<u32> = vec![0x3C60_CC00, 0x6063_2000, 0x3880_0F06, 0xB083_0000, 0x3880_2850, 0xB083_0048, 0x3880_0005, 0xB083_0002, 0x4800_0000];
    let mut dol = vec![0u8; 0x100];
    dol[0..4].copy_from_slice(&0x100u32.to_be_bytes());
    dol[0x48..0x4C].copy_from_slice(&0x8000_3100u32.to_be_bytes());
    dol[0x90..0x94].copy_from_slice(&((words.len() * 4) as u32).to_be_bytes());
    dol[0xE0..0xE4].copy_from_slice(&0x8000_3100u32.to_be_bytes());
    dol.extend(words.iter().flat_map(|w| w.to_be_bytes()));
    let path = std::env::temp_dir().join(format!("continuum_gc_gpu_frontend_{}.dol", std::process::id()));
    std::fs::write(&path, dol).unwrap();
    path
}

fn logged(what: &str) -> bool {
    LOG.lock().unwrap().iter().any(|l| l.contains(what))
}

#[test]
fn the_graphics_chip_through_the_libretro_calls() {
    let Some(vk) = test_vulkan::vulkan() else {
        eprintln!("no Vulkan on this machine: the GPU drawer through libretro not tested");
        return;
    };
    let h = vk.handles;
    *IFACE.lock().unwrap() = Some(Box::new(Shared(Interface {
        interface_type: 0,
        interface_version: 5,
        handle: 0x1234 as *mut c_void,
        instance: h.instance.as_raw() as *mut c_void,
        gpu: h.physical.as_raw() as *mut c_void,
        device: h.device.as_raw() as *mut c_void,
        get_device_proc_addr: std::ptr::null(),
        get_instance_proc_addr: h.get_instance_proc_addr as *const c_void,
        queue: h.queue.as_raw() as *mut c_void,
        queue_index: h.queue_family,
        set_image: std::ptr::null(),
        get_sync_index: std::ptr::null(),
        get_sync_index_mask: std::ptr::null(),
        set_command_buffers: std::ptr::null(),
        wait_sync_index: std::ptr::null(),
        lock_queue: Some(lock),
        unlock_queue: Some(unlock),
        set_signal_semaphore: std::ptr::null(),
    })));
    let path = program();
    let cpath = CString::new(path.to_str().unwrap()).unwrap();
    retro_set_environment(env);
    retro_set_video_refresh(video);
    retro_set_audio_sample_batch(audio);
    retro_set_input_poll(poll);
    retro_set_input_state(input);
    retro_init();
    let info = RetroGameInfo { path: cpath.as_ptr(), data: std::ptr::null(), size: 0, meta: std::ptr::null() };
    assert!(unsafe { retro_load_game(&info) });
    assert!(HW_ASKED.load(Ordering::SeqCst), "the core asks for the graphics chip");
    for l in LOG.lock().unwrap().iter() {
        eprintln!("log: {l}");
    }
    assert!(logged("drawing on the graphics chip"), "the GPU drawer started after its picture check");
    assert!(LOCKS.load(Ordering::SeqCst) > 0 && LOCKS.load(Ordering::SeqCst) == UNLOCKS.load(Ordering::SeqCst), "submits hold the app's queue lock");
    for _ in 0..3 {
        retro_run();
    }
    assert_eq!(FRAMES.load(Ordering::SeqCst), 3);
    let size = retro_serialize_size();
    let mut buf = vec![0u8; size];
    assert!(unsafe { retro_serialize(buf.as_mut_ptr() as *mut c_void, size) });
    assert!(unsafe { retro_unserialize(buf.as_ptr() as *const c_void, size) });
    retro_run();
    assert!(!logged("stopped drawing"), "the graphics chip kept drawing");
    retro_unload_game();

    // The Drawing setting on the processor: no graphics chip asked for.
    LOG.lock().unwrap().clear();
    HW_ASKED.store(false, Ordering::SeqCst);
    DRAWING_CPU.store(true, Ordering::SeqCst);
    assert!(unsafe { retro_load_game(&info) });
    assert!(!HW_ASKED.load(Ordering::SeqCst));
    assert!(logged("drawing on the processor (the Drawing setting)"));
    retro_run();
    retro_unload_game();

    // An app without a graphics chip for the core: the processor draws.
    LOG.lock().unwrap().clear();
    DRAWING_CPU.store(false, Ordering::SeqCst);
    *IFACE.lock().unwrap() = None;
    assert!(unsafe { retro_load_game(&info) });
    assert!(logged("drawing on the processor: the app has no graphics chip"));
    retro_run();
    retro_unload_game();
    retro_deinit();
    drop(vk);
    let _ = std::fs::remove_file(path);
}
