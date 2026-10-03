//! The camera: `RETRO_ENVIRONMENT_GET_CAMERA_INTERFACE`, raw framebuffer path only.
//!
//! ## The threading rule
//!
//! libretro.h:1209 says the frontend delivers camera frames "via a user-defined callback that runs
//! in the same thread as `retro_run()`". So frames arrive here from AVCaptureSession's own queue
//! (Swift calls `ContinuumEngine::push_camera_frame`), are scaled and parked in a one-frame slot,
//! and are handed to the core's `frame_raw_framebuffer` by [`before_retro_run`], which
//! `native_core.rs` calls on the core's thread immediately before `retro_run`. The camera thread
//! never calls into the core.
//!
//! ## What is served
//!
//! Only `RETRO_CAMERA_BUFFER_RAW_FRAMEBUFFER`: XRGB8888 pixels in memory. The OpenGL texture path
//! needs a GL context shared with the core, which no Vulkan core here has, so a core that asks
//! ONLY for textures is refused (the environment call returns false and the core carries on with
//! no camera, which libretro requires it to handle).
//!
//! ## Scaling
//!
//! The core names the size it wants (`width`/`height`, a hint, zero meaning "you decide"). The
//! phone's camera delivers whatever its preset gives. Scaling happens here, in Rust, so an Android
//! build gets the same picture: aspect FILL with a centre crop, so the game never sees letterbox
//! bars it would mistake for part of the scene, and nearest sampling, which is plenty for a
//! 640x480 target and costs one read per output pixel.
//!
//! ## Azahar
//!
//! Azahar's libretro frontend (`src/citra_libretro/` in azahar-emu/azahar, read at commit
//! 86a9f92) never sends `GET_CAMERA_INTERFACE`: its camera factory is the blank camera, so today
//! no core in this app asks for this. The frontend half is complete and tested so that a core
//! which does ask gets a working camera without any change here.

use std::ffi::{c_uint, c_void};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// libretro.h:1225 `RETRO_ENVIRONMENT_GET_CAMERA_INTERFACE (26 | RETRO_ENVIRONMENT_EXPERIMENTAL)`.
pub const ENV_GET_CAMERA_INTERFACE: c_uint = 26 | 0x10000;

/// libretro.h:5335 `RETRO_CAMERA_BUFFER_OPENGL_TEXTURE = 0`.
pub const CAMERA_BUFFER_OPENGL_TEXTURE: u64 = 0;
/// libretro.h:5342 `RETRO_CAMERA_BUFFER_RAW_FRAMEBUFFER` (= 1).
pub const CAMERA_BUFFER_RAW_FRAMEBUFFER: u64 = 1;

/// What a core gets when it leaves `width`/`height` at zero: VGA, the 3DS camera's own size.
pub const DEFAULT_CAMERA_WIDTH: u32 = 640;
pub const DEFAULT_CAMERA_HEIGHT: u32 = 480;

/// Refused above this, so a core naming a silly size cannot make every frame a huge allocation.
pub const MAX_CAMERA_DIMENSION: u32 = 4096;

pub type RetroCameraStart = unsafe extern "C" fn() -> bool;
pub type RetroCameraStop = unsafe extern "C" fn();
pub type RetroCameraLifetime = unsafe extern "C" fn();
pub type RetroCameraFrameRaw = unsafe extern "C" fn(*const u32, c_uint, c_uint, usize);
pub type RetroCameraFrameGl = unsafe extern "C" fn(c_uint, c_uint, *const f32);

/// libretro.h:5434 `struct retro_camera_callback`, field for field and in order.
#[repr(C)]
pub struct RetroCameraCallback {
    /// Set by the core: bitmask of `1 << retro_camera_buffer`.
    pub caps: u64,
    /// Set by the core: desired size, zero for "frontend decides".
    pub width: c_uint,
    pub height: c_uint,
    /// Set by the frontend.
    pub start: Option<RetroCameraStart>,
    /// Set by the frontend.
    pub stop: Option<RetroCameraStop>,
    /// Set by the core.
    pub frame_raw_framebuffer: Option<RetroCameraFrameRaw>,
    /// Set by the core.
    pub frame_opengl_texture: Option<RetroCameraFrameGl>,
    /// Set by the core.
    pub initialized: Option<RetroCameraLifetime>,
    /// Set by the core.
    pub deinitialized: Option<RetroCameraLifetime>,
}

/// What the core registered.
#[derive(Clone, Copy)]
struct Registration {
    raw: RetroCameraFrameRaw,
    initialized: Option<RetroCameraLifetime>,
    deinitialized: Option<RetroCameraLifetime>,
    initialized_called: bool,
}

/// One scaled frame, ready for the core.
#[derive(Debug, Default)]
struct Frame {
    pixels: Vec<u32>,
    width: u32,
    height: u32,
}

/// Everything shared between the core thread, the camera thread and Swift's poll.
pub struct CameraHub {
    registration: Mutex<Option<Registration>>,
    /// The Settings switch "Allow camera in games", ANDed in Swift with the iOS authorisation.
    allowed: AtomicBool,
    /// The core called `start` and has not called `stop`.
    started: AtomicBool,
    /// The requested size, mirrored out of `registration` so the camera thread and Swift's poll
    /// read two atomics rather than taking the registration lock.
    width: AtomicU32,
    height: AtomicU32,
    /// The newest frame not yet given to the core.
    pending: Mutex<Option<Frame>>,
    /// Producer-only scratch, so scaling happens outside `pending`'s lock.
    scratch: Mutex<Vec<u32>>,
    /// A buffer the consumer finished with, handed back so steady state does not allocate.
    recycled: Mutex<Option<Vec<u32>>>,
    received: AtomicU64,
    delivered: AtomicU64,
    refused_gl_only: AtomicBool,
    interface_requests: AtomicU32,
}

impl CameraHub {
    pub fn new() -> Self {
        Self {
            registration: Mutex::new(None),
            allowed: AtomicBool::new(false),
            started: AtomicBool::new(false),
            width: AtomicU32::new(DEFAULT_CAMERA_WIDTH),
            height: AtomicU32::new(DEFAULT_CAMERA_HEIGHT),
            pending: Mutex::new(None),
            scratch: Mutex::new(Vec::new()),
            recycled: Mutex::new(None),
            received: AtomicU64::new(0),
            delivered: AtomicU64::new(0),
            refused_gl_only: AtomicBool::new(false),
            interface_requests: AtomicU32::new(0),
        }
    }

    pub fn set_allowed(&self, allowed: bool) {
        self.allowed.store(allowed, Ordering::Release);
        if !allowed {
            *lock(&self.pending) = None;
        }
    }

    pub fn allowed(&self) -> bool {
        self.allowed.load(Ordering::Acquire)
    }

    pub fn registered(&self) -> bool {
        lock(&self.registration).is_some()
    }

    pub fn started(&self) -> bool {
        self.started.load(Ordering::Acquire)
    }

    /// Whether Swift should have the capture session running.
    pub fn wants_capture(&self) -> bool {
        self.started() && self.allowed() && self.registered()
    }

    pub fn requested_size(&self) -> (u32, u32) {
        (
            self.width.load(Ordering::Acquire),
            self.height.load(Ordering::Acquire),
        )
    }

    pub fn interface_requests(&self) -> u32 {
        self.interface_requests.load(Ordering::Relaxed)
    }

    pub fn frames_delivered(&self) -> u64 {
        self.delivered.load(Ordering::Relaxed)
    }

    /// Forgets the core. Called before a core loads, and after `deinitialized` on unload.
    pub fn reset_core_side(&self) {
        *lock(&self.registration) = None;
        self.started.store(false, Ordering::Release);
        self.width.store(DEFAULT_CAMERA_WIDTH, Ordering::Release);
        self.height.store(DEFAULT_CAMERA_HEIGHT, Ordering::Release);
        *lock(&self.pending) = None;
        self.received.store(0, Ordering::Relaxed);
        self.delivered.store(0, Ordering::Relaxed);
        self.refused_gl_only.store(false, Ordering::Relaxed);
        self.interface_requests.store(0, Ordering::Relaxed);
    }

    /// CAMERA THREAD. Takes one BGRA frame (`kCVPixelFormatType_32BGRA`, which in memory is
    /// exactly libretro's XRGB8888 read as a little-endian `u32`, apart from the alpha byte),
    /// scales it to the size the core asked for and parks it for the next frame.
    ///
    /// Returns false, having changed nothing, when the core is not running the camera or the
    /// buffer is too short for the geometry it claims.
    pub fn push_bgra(
        &self,
        bgra: &[u8],
        width: u32,
        height: u32,
        stride: u32,
        mirror: bool,
    ) -> bool {
        if !self.wants_capture() {
            return false;
        }
        let (target_width, target_height) = self.requested_size();
        let mut scratch = lock(&self.scratch);
        if !scale_bgra_to_xrgb(
            bgra,
            width,
            height,
            stride as usize,
            &mut scratch,
            target_width,
            target_height,
            mirror,
        ) {
            return false;
        }
        // The spare buffer becomes the next scratch; the scaled one goes to the slot.
        let next = lock(&self.recycled).take().unwrap_or_default();
        let pixels = std::mem::replace(&mut *scratch, next);
        drop(scratch);
        let replaced = lock(&self.pending).replace(Frame {
            pixels,
            width: target_width,
            height: target_height,
        });
        if let Some(old) = replaced {
            // A frame the core never saw; its buffer is reused rather than freed.
            *lock(&self.recycled) = Some(old.pixels);
        }
        self.received.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// The HUD sentence. Never empty.
    pub fn status_line(&self) -> String {
        if self.refused_gl_only.load(Ordering::Relaxed) {
            return "camera: the core asked for OpenGL camera textures only, which this app \
                    cannot provide, so it has no camera"
                .into();
        }
        if self.interface_requests() == 0 {
            return "camera: this core did not ask for a camera".into();
        }
        let (width, height) = self.requested_size();
        if !self.started() {
            return format!("camera: offered to the core at {width}x{height}, not started");
        }
        if !self.allowed() {
            return "camera: the game started the camera, but Allow camera in games is off or \
                    iOS camera access was refused"
                .into();
        }
        format!(
            "camera: running at {width}x{height}, {} frame(s) captured, {} given to the core",
            self.received.load(Ordering::Relaxed),
            self.delivered.load(Ordering::Relaxed)
        )
    }
}

impl Default for CameraHub {
    fn default() -> Self {
        Self::new()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

static HUB: OnceLock<CameraHub> = OnceLock::new();

pub fn hub() -> &'static CameraHub {
    HUB.get_or_init(CameraHub::new)
}

/// Scales a BGRA buffer into `out` as XRGB8888 `u32`s, aspect fill with a centre crop.
///
/// Returns false, leaving `out` untouched, for any geometry that does not add up: zero sizes, a
/// stride narrower than a row, or a buffer shorter than `stride * height`.
#[allow(clippy::too_many_arguments)]
pub fn scale_bgra_to_xrgb(
    bgra: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    out: &mut Vec<u32>,
    target_width: u32,
    target_height: u32,
    mirror: bool,
) -> bool {
    if width == 0 || height == 0 || target_width == 0 || target_height == 0 {
        return false;
    }
    let row_bytes = width as usize * 4;
    if stride < row_bytes {
        return false;
    }
    let needed = stride * (height as usize - 1) + row_bytes;
    if bgra.len() < needed {
        return false;
    }

    // Aspect fill: the largest centred source rect with the target's aspect.
    let (src_w, src_h) = (width as f64, height as f64);
    let (dst_w, dst_h) = (target_width as f64, target_height as f64);
    let (crop_w, crop_h) = if src_w / src_h > dst_w / dst_h {
        (src_h * dst_w / dst_h, src_h)
    } else {
        (src_w, src_w * dst_h / dst_w)
    };
    let origin_x = (src_w - crop_w) / 2.0;
    let origin_y = (src_h - crop_h) / 2.0;
    let step_x = crop_w / dst_w;
    let step_y = crop_h / dst_h;

    out.clear();
    out.reserve(target_width as usize * target_height as usize);
    for row in 0..target_height {
        let sy = (origin_y + (row as f64 + 0.5) * step_y).floor() as usize;
        let sy = sy.min(height as usize - 1);
        let base = sy * stride;
        for column in 0..target_width {
            let column = if mirror {
                target_width - 1 - column
            } else {
                column
            };
            let sx = (origin_x + (column as f64 + 0.5) * step_x).floor() as usize;
            let sx = sx.min(width as usize - 1);
            let at = base + sx * 4;
            let (b, g, r) = (bgra[at] as u32, bgra[at + 1] as u32, bgra[at + 2] as u32);
            out.push((r << 16) | (g << 8) | b);
        }
    }
    true
}

// ------------------------------------------------------------------ the callbacks

unsafe extern "C" fn camera_start() -> bool {
    let hub = hub();
    if !hub.registered() {
        return false;
    }
    hub.started.store(true, Ordering::Release);
    let allowed = hub.allowed();
    log::info!(
        "camera: the core started the camera ({})",
        if allowed {
            "allowed"
        } else {
            "not allowed, so no frames will come"
        }
    );
    // libretro.h:5359: false when "the frontend doesn't have permission". The core is told the
    // truth, and the switch being turned on later still starts frames, because `started` is kept.
    allowed
}

unsafe extern "C" fn camera_stop() {
    let hub = hub();
    hub.started.store(false, Ordering::Release);
    *lock(&hub.pending) = None;
    log::info!("camera: the core stopped the camera");
}

/// Answers `GET_CAMERA_INTERFACE`.
///
/// # Safety
///
/// `data` must be null or point at a writable `retro_camera_callback` whose core-set fields are
/// valid function pointers or null.
pub unsafe fn answer_interface(data: *mut c_void) -> bool {
    if data.is_null() {
        return false;
    }
    let hub = hub();
    hub.interface_requests.fetch_add(1, Ordering::Relaxed);
    let callback = unsafe { &mut *(data as *mut RetroCameraCallback) };
    let wants_raw = callback.caps & (1 << CAMERA_BUFFER_RAW_FRAMEBUFFER) != 0;
    let Some(raw) = callback.frame_raw_framebuffer.filter(|_| wants_raw) else {
        hub.refused_gl_only.store(true, Ordering::Relaxed);
        log::warn!(
            "camera: refused, the core asked for caps {:#x} without a raw framebuffer callback",
            callback.caps
        );
        return false;
    };
    let width = match callback.width {
        0 => DEFAULT_CAMERA_WIDTH,
        value => value.min(MAX_CAMERA_DIMENSION),
    };
    let height = match callback.height {
        0 => DEFAULT_CAMERA_HEIGHT,
        value => value.min(MAX_CAMERA_DIMENSION),
    };
    *lock(&hub.registration) = Some(Registration {
        raw,
        initialized: callback.initialized,
        deinitialized: callback.deinitialized,
        initialized_called: false,
    });
    hub.width.store(width, Ordering::Release);
    hub.height.store(height, Ordering::Release);
    hub.started.store(false, Ordering::Release);
    callback.start = Some(camera_start);
    callback.stop = Some(camera_stop);
    log::info!("camera: raw framebuffer camera offered to the core at {width}x{height}");
    true
}

/// CORE THREAD, immediately before `retro_run`.
///
/// Calls the core's `initialized` once after it registered (RetroArch does this when its camera
/// driver comes up, which is after load), then hands over the newest captured frame if the
/// camera is started and one is waiting. No lock is held while the core runs its callback.
pub fn before_retro_run() {
    let hub = hub();
    let registration = {
        let mut guard = lock(&hub.registration);
        let Some(registration) = guard.as_mut() else {
            return;
        };
        let first = !registration.initialized_called;
        registration.initialized_called = true;
        (*registration, first)
    };
    let (registration, first) = registration;
    if first {
        if let Some(initialized) = registration.initialized {
            unsafe { initialized() };
        }
    }
    if !hub.started() || !hub.allowed() {
        return;
    }
    let Some(frame) = lock(&hub.pending).take() else {
        return;
    };
    if frame.pixels.len() == frame.width as usize * frame.height as usize && frame.width > 0 {
        unsafe {
            (registration.raw)(
                frame.pixels.as_ptr(),
                frame.width,
                frame.height,
                frame.width as usize * 4,
            )
        };
        hub.delivered.fetch_add(1, Ordering::Relaxed);
    }
    *lock(&hub.recycled) = Some(frame.pixels);
}

/// CORE THREAD, before `retro_unload_game`: the core's `deinitialized`, then forget it.
pub fn before_unload() {
    let hub = hub();
    let registration = *lock(&hub.registration);
    if let Some(registration) = registration {
        if registration.initialized_called {
            if let Some(deinitialized) = registration.deinitialized {
                unsafe { deinitialized() };
            }
        }
    }
    hub.reset_core_side();
}

/// Serialises every test, in any module, that touches the global hub.
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn struct_layout_matches_libretro_h() {
        let pointer = std::mem::size_of::<usize>();
        assert_eq!(
            std::mem::size_of::<RetroCameraCallback>(),
            8 + 4 + 4 + 6 * pointer
        );
        assert_eq!(ENV_GET_CAMERA_INTERFACE, 0x1001A);
    }

    #[test]
    fn scale_packs_xrgb_from_bgra() {
        // 1x1 BGRA (b=1, g=2, r=3, a=255) into 1x1.
        let mut out = Vec::new();
        assert!(scale_bgra_to_xrgb(&[1, 2, 3, 255], 1, 1, 4, &mut out, 1, 1, false));
        assert_eq!(out, vec![0x0003_0201]);
    }

    fn gradient(width: u32, height: u32, stride: usize) -> Vec<u8> {
        // Blue channel is the column, green the row, so a sample says where it came from.
        let mut data = vec![0u8; stride * height as usize];
        for y in 0..height as usize {
            for x in 0..width as usize {
                let at = y * stride + x * 4;
                data[at] = x as u8;
                data[at + 1] = y as u8;
                data[at + 2] = 0;
                data[at + 3] = 255;
            }
        }
        data
    }

    #[test]
    fn scale_crops_the_centre_of_a_wider_source() {
        // 8x2 source into 2x2: aspect fill keeps the middle 2x2 columns 3..5.
        let source = gradient(8, 2, 8 * 4);
        let mut out = Vec::new();
        assert!(scale_bgra_to_xrgb(&source, 8, 2, 32, &mut out, 2, 2, false));
        let columns: Vec<u32> = out.iter().map(|p| p & 0xFF).collect();
        let rows: Vec<u32> = out.iter().map(|p| (p >> 8) & 0xFF).collect();
        assert_eq!(columns, vec![3, 4, 3, 4]);
        assert_eq!(rows, vec![0, 0, 1, 1]);
    }

    #[test]
    fn scale_crops_the_centre_of_a_taller_source() {
        // Portrait 4x8 into landscape 4x2: rows 3..5 survive.
        let source = gradient(4, 8, 16);
        let mut out = Vec::new();
        assert!(scale_bgra_to_xrgb(&source, 4, 8, 16, &mut out, 4, 2, false));
        let rows: Vec<u32> = out.iter().map(|p| (p >> 8) & 0xFF).collect();
        assert_eq!(rows, vec![3, 3, 3, 3, 4, 4, 4, 4]);
    }

    #[test]
    fn scale_honours_stride_and_mirror() {
        // Stride padded past the row; mirror reverses the columns.
        let source = gradient(4, 1, 32);
        let mut out = Vec::new();
        assert!(scale_bgra_to_xrgb(&source, 4, 1, 32, &mut out, 4, 1, true));
        let columns: Vec<u32> = out.iter().map(|p| p & 0xFF).collect();
        assert_eq!(columns, vec![3, 2, 1, 0]);
    }

    #[test]
    fn scale_refuses_bad_geometry() {
        let mut out = vec![42];
        assert!(!scale_bgra_to_xrgb(&[0; 15], 2, 2, 8, &mut out, 1, 1, false));
        assert!(!scale_bgra_to_xrgb(&[0; 64], 4, 4, 8, &mut out, 1, 1, false));
        assert!(!scale_bgra_to_xrgb(&[0; 64], 0, 4, 16, &mut out, 1, 1, false));
        assert!(!scale_bgra_to_xrgb(&[0; 64], 4, 4, 16, &mut out, 0, 1, false));
        assert_eq!(out, vec![42]);
    }

    // A fake core.
    static FRAMES: AtomicUsize = AtomicUsize::new(0);
    static LAST_SIZE: AtomicU64 = AtomicU64::new(0);
    static LAST_PIXEL: AtomicU32 = AtomicU32::new(0);
    static INITS: AtomicUsize = AtomicUsize::new(0);
    static DEINITS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn fake_frame(buffer: *const u32, width: c_uint, height: c_uint, pitch: usize) {
        FRAMES.fetch_add(1, Ordering::SeqCst);
        LAST_SIZE.store(
            ((width as u64) << 32) | height as u64 | ((pitch as u64) << 48),
            Ordering::SeqCst,
        );
        LAST_PIXEL.store(unsafe { *buffer }, Ordering::SeqCst);
    }
    unsafe extern "C" fn fake_init() {
        INITS.fetch_add(1, Ordering::SeqCst);
    }
    unsafe extern "C" fn fake_deinit() {
        DEINITS.fetch_add(1, Ordering::SeqCst);
    }

    fn locked() -> std::sync::MutexGuard<'static, ()> {
        let guard = lock(&TEST_LOCK);
        hub().reset_core_side();
        hub().set_allowed(false);
        FRAMES.store(0, Ordering::SeqCst);
        INITS.store(0, Ordering::SeqCst);
        DEINITS.store(0, Ordering::SeqCst);
        guard
    }

    fn callback(caps: u64, width: u32, height: u32) -> RetroCameraCallback {
        RetroCameraCallback {
            caps,
            width,
            height,
            start: None,
            stop: None,
            frame_raw_framebuffer: Some(fake_frame),
            frame_opengl_texture: None,
            initialized: Some(fake_init),
            deinitialized: Some(fake_deinit),
        }
    }

    #[test]
    fn gl_only_request_is_refused() {
        let _guard = locked();
        let mut request = callback(1 << CAMERA_BUFFER_OPENGL_TEXTURE, 0, 0);
        assert!(!unsafe { answer_interface(&mut request as *mut _ as *mut c_void) });
        assert!(request.start.is_none());
        assert!(hub().status_line().contains("OpenGL"));
        assert!(!unsafe { answer_interface(std::ptr::null_mut()) });
    }

    #[test]
    fn raw_request_gets_start_stop_and_frames_on_the_core_thread() {
        let _guard = locked();
        let mut request = callback(
            (1 << CAMERA_BUFFER_RAW_FRAMEBUFFER) | (1 << CAMERA_BUFFER_OPENGL_TEXTURE),
            4,
            2,
        );
        assert!(unsafe { answer_interface(&mut request as *mut _ as *mut c_void) });
        let (start, stop) = (request.start.unwrap(), request.stop.unwrap());
        assert_eq!(hub().requested_size(), (4, 2));

        // `initialized` fires on the first frame, once.
        before_retro_run();
        before_retro_run();
        assert_eq!(INITS.load(Ordering::SeqCst), 1);

        // Not started: a pushed frame is refused and nothing reaches the core.
        hub().set_allowed(true);
        let source = gradient(8, 4, 32);
        assert!(!hub().push_bgra(&source, 8, 4, 32, false));

        // Started and allowed: the push is parked, NOT delivered, until the core's frame.
        assert!(unsafe { start() });
        assert!(hub().wants_capture());
        assert!(hub().push_bgra(&source, 8, 4, 32, false));
        assert_eq!(FRAMES.load(Ordering::SeqCst), 0);
        before_retro_run();
        assert_eq!(FRAMES.load(Ordering::SeqCst), 1);
        let size = LAST_SIZE.load(Ordering::SeqCst);
        assert_eq!((size >> 32) & 0xFFFF, 4);
        assert_eq!(size & 0xFFFF_FFFF, 2);
        assert_eq!(size >> 48, 16);
        // No new frame, no second delivery.
        before_retro_run();
        assert_eq!(FRAMES.load(Ordering::SeqCst), 1);
        // Two pushes before one frame: only the newest arrives.
        assert!(hub().push_bgra(&source, 8, 4, 32, false));
        assert!(hub().push_bgra(&source, 8, 4, 32, true));
        before_retro_run();
        assert_eq!(FRAMES.load(Ordering::SeqCst), 2);
        assert_eq!(hub().frames_delivered(), 2);

        // Stopped: pushes refused again.
        unsafe { stop() };
        assert!(!hub().wants_capture());
        assert!(!hub().push_bgra(&source, 8, 4, 32, false));

        before_unload();
        assert_eq!(DEINITS.load(Ordering::SeqCst), 1);
        assert!(!hub().registered());
    }

    #[test]
    fn start_reports_false_when_not_allowed_and_frames_are_withheld() {
        let _guard = locked();
        let mut request = callback(1 << CAMERA_BUFFER_RAW_FRAMEBUFFER, 0, 0);
        assert!(unsafe { answer_interface(&mut request as *mut _ as *mut c_void) });
        assert_eq!(
            hub().requested_size(),
            (DEFAULT_CAMERA_WIDTH, DEFAULT_CAMERA_HEIGHT)
        );
        assert!(!unsafe { (request.start.unwrap())() });
        assert!(hub().started());
        assert!(!hub().wants_capture());
        assert!(hub().status_line().contains("is off"));
    }

    #[test]
    fn deinit_is_not_called_for_a_camera_that_never_initialised() {
        let _guard = locked();
        let mut request = callback(1 << CAMERA_BUFFER_RAW_FRAMEBUFFER, 0, 0);
        assert!(unsafe { answer_interface(&mut request as *mut _ as *mut c_void) });
        before_unload();
        assert_eq!(DEINITS.load(Ordering::SeqCst), 0);
    }
}
