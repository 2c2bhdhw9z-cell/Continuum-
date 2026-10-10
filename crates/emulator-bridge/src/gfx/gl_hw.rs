//! OpenGL picture path next to the Vulkan door. Step 7 of
//! `docs/SET_HW_RENDER_DESIGN.md`, the slice that can be proven without ANGLE.
//!
//! `SET_HW_RENDER` for `RETRO_HW_CONTEXT_OPENGL` and the OpenGL ES variants used to be
//! refused here. That refusal was this app's choice: the phone can host both, and only
//! MoltenVK was wired. This module accepts those requests.
//!
//! What a core gets:
//!
//! - `get_current_framebuffer` — an FBO name, once a context exists.
//! - `get_proc_address` — `dlsym` of OpenGL ES on iOS. Null on a host with no GL.
//! - After `video_refresh(RETRO_HW_FRAME_BUFFER_VALID)`, a read of that color buffer as
//!   tightly packed top-left RGBA8. The existing compositor uploads those bytes. It is
//!   the same picture path software cores already use, not a second renderer.
//!
//! What this is not: ANGLE, `EGL_ANGLE_metal_texture_client_buffer`, or a zero-copy
//! `MTLTexture`. iOS has no desktop GL, so an `OPENGL` / `OPENGL_CORE` request is still
//! answered with an OpenGL ES context. `GET_PREFERRED_HW_RENDER` stays Vulkan, so a core
//! that can do both (Azahar, Beetle) keeps the MoltenVK door.
//!
//! Host tests have no EAGL. They install a CPU color buffer in GL's bottom-left order
//! and check the RGBA the upload would receive, including the vertical flip
//! `bottom_left_origin` asks for.

use std::ffi::{c_char, c_void};
use std::sync::Mutex;

use crate::gfx::hw::HwContextType;
use crate::gfx::vulkan_hw::RetroHwRenderCallback;

/// Starting FBO size, used when the core asks for GL before it has reported
/// `max_width` / `max_height`. Grown later by [`resize_for_core`].
const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 960;
const MAX_EDGE: u32 = 4096;

static STATE: Mutex<GlState> = Mutex::new(GlState::new());

fn lock_state() -> std::sync::MutexGuard<'static, GlState> {
    match STATE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub fn reset() {
    // `context_destroy` runs while the ES context is still current, which is what
    // libretro requires. The bookkeeping is cleared after that, not before.
    let destroy = {
        let state = lock_state();
        if state.reset_sent {
            state.context_destroy
        } else {
            None
        }
    };
    #[cfg(target_os = "ios")]
    ios::make_current();
    if let Some(context_destroy) = destroy {
        unsafe { context_destroy() };
    }
    #[cfg(target_os = "ios")]
    ios::destroy();
    *lock_state() = GlState::new();
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlHwStatus {
    pub accepted: bool,
    pub context_type: Option<HwContextType>,
    /// A real EAGL FBO, or the host test stand-in, is in place.
    pub context_ready: bool,
    pub framebuffer: u32,
    pub bottom_left_origin: bool,
    pub frames_read: u64,
}

pub fn status() -> GlHwStatus {
    let state = lock_state();
    GlHwStatus {
        accepted: state.accepted,
        context_type: state.context_type,
        context_ready: state.context_ready,
        framebuffer: state.framebuffer,
        bottom_left_origin: state.bottom_left_origin,
        frames_read: state.frames_read,
    }
}

/// Vulkan just took `SET_HW_RENDER`. Stop treating later hardware frames as GL.
///
/// Does not tear the ES context down: the next [`reset`] (a new core load) does that.
/// Called so a Vulkan core cannot have its `set_image` skipped because an earlier GL
/// request was still marked accepted.
pub fn note_vulkan_took_over() {
    let mut state = lock_state();
    state.accepted = false;
}

struct CpuTarget {
    width: u32,
    height: u32,
    /// Tight RGBA8. Row 0 is the bottom row, matching `glReadPixels`.
    rgba_gl_order: Vec<u8>,
}

struct GlState {
    accepted: bool,
    context_type: Option<HwContextType>,
    bottom_left_origin: bool,
    depth: bool,
    stencil: bool,
    version_major: u32,
    version_minor: u32,
    context_reset: Option<unsafe extern "C" fn()>,
    context_destroy: Option<unsafe extern "C" fn()>,
    reset_sent: bool,
    context_ready: bool,
    framebuffer: u32,
    fbo_width: u32,
    fbo_height: u32,
    cpu: Option<CpuTarget>,
    frames_read: u64,
}

impl GlState {
    const fn new() -> Self {
        Self {
            accepted: false,
            context_type: None,
            bottom_left_origin: false,
            depth: false,
            stencil: false,
            version_major: 0,
            version_minor: 0,
            context_reset: None,
            context_destroy: None,
            reset_sent: false,
            context_ready: false,
            framebuffer: 0,
            fbo_width: 0,
            fbo_height: 0,
            cpu: None,
            frames_read: 0,
        }
    }
}

/// Accepts an OpenGL or OpenGL ES `SET_HW_RENDER`. Fills the two frontend callbacks.
///
/// Returns false for anything that is not a GL context type. The caller still refuses
/// Direct3D and unknown values.
pub fn accept(callback: &mut RetroHwRenderCallback) -> bool {
    let Some(context_type) = HwContextType::from_libretro(callback.context_type) else {
        return false;
    };
    if !context_type.is_gl() {
        return false;
    }
    callback.get_current_framebuffer = Some(frontend_get_current_framebuffer);
    callback.get_proc_address = Some(frontend_get_proc_address);
    {
        let mut state = lock_state();
        state.accepted = true;
        state.context_type = Some(context_type);
        state.bottom_left_origin = callback.bottom_left_origin;
        state.depth = callback.depth;
        state.stencil = callback.stencil;
        state.version_major = callback.version_major;
        state.version_minor = callback.version_minor;
        state.context_reset = callback.context_reset;
        state.context_destroy = callback.context_destroy;
        state.reset_sent = false;
        state.frames_read = 0;
    }
    log::info!(
        "SET_HW_RENDER accepted: {:?} {}.{} bottom_left_origin={} (OpenGL ES readback, not ANGLE)",
        context_type,
        callback.version_major,
        callback.version_minor,
        callback.bottom_left_origin
    );
    if ensure_context(DEFAULT_WIDTH, DEFAULT_HEIGHT) {
        fire_context_reset(false);
    } else {
        log::info!("SET_HW_RENDER: OpenGL request accepted; no GL context on this host yet");
    }
    true
}

/// Grows the FBO up to the core's max geometry, once `retro_load_game` has reported it.
///
/// A second `context_reset` is sent when the size actually changes after the first one,
/// because the core may have allocated against the placeholder target.
pub fn resize_for_core(max_width: u32, max_height: u32) {
    if max_width == 0 || max_height == 0 {
        return;
    }
    if !status().accepted {
        return;
    }
    let width = max_width.min(MAX_EDGE);
    let height = max_height.min(MAX_EDGE);
    let (current_w, current_h, ready) = {
        let state = lock_state();
        (state.fbo_width, state.fbo_height, state.context_ready)
    };
    if ready && width <= current_w && height <= current_h {
        return;
    }
    if ensure_context(width, height) {
        fire_context_reset(true);
    }
}

/// Makes the ES context current before `retro_run`, when there is one.
pub fn prepare_frame() {
    // Written as one `if` rather than an early return, because off iOS the call below is
    // compiled out and clippy then reads the return as needless.
    if status().accepted {
        #[cfg(target_os = "ios")]
        ios::make_current();
    }
}

/// Reads the GL color target and returns top-left RGBA8 for the compositor upload.
///
/// `None` when no context was created, or the size is empty. That leaves the previous
/// picture up rather than inventing pixels.
pub struct PresentedRgba {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn read_presented_rgba(width: u32, height: u32) -> Option<PresentedRgba> {
    if width == 0 || height == 0 {
        return None;
    }
    let (bottom_left, cpu, framebuffer, fbo_w, fbo_h) = {
        let state = lock_state();
        if !state.accepted || !state.context_ready {
            return None;
        }
        (
            state.bottom_left_origin,
            state.cpu.as_ref().map(|cpu| CpuTarget {
                width: cpu.width,
                height: cpu.height,
                rgba_gl_order: cpu.rgba_gl_order.clone(),
            }),
            state.framebuffer,
            state.fbo_width,
            state.fbo_height,
        )
    };
    let read_w = width.min(fbo_w.max(1));
    let read_h = height.min(fbo_h.max(1));
    let rgba = if let Some(cpu) = cpu {
        let read_w = width.min(cpu.width);
        let read_h = height.min(cpu.height);
        (
            read_w,
            read_h,
            orient_gl_rgba(
                &cpu.rgba_gl_order,
                cpu.width,
                cpu.height,
                read_w,
                read_h,
                bottom_left,
            ),
        )
    } else {
        #[cfg(target_os = "ios")]
        {
            let pixels = ios::read_rgba(framebuffer, read_w, read_h, bottom_left)?;
            (read_w, read_h, pixels)
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = (framebuffer, read_w, read_h, bottom_left);
            return None;
        }
    };
    {
        let mut state = lock_state();
        let first = state.frames_read == 0;
        state.frames_read = state.frames_read.saturating_add(1);
        if first {
            log::info!(
                "OpenGL frame read back {}x{} into the compositor (RGBA8 upload, not ANGLE)",
                rgba.0,
                rgba.1
            );
        }
    }
    Some(PresentedRgba {
        width: rgba.0,
        height: rgba.1,
        rgba: rgba.2,
    })
}

/// Host tests only: stand in for the color attachment `glReadPixels` would return.
///
/// `rgba_gl_order` is tight RGBA8 with row 0 at the bottom, the same order as
/// `glReadPixels`. The framebuffer name becomes `1`, which is non-zero so a core that
/// treats `0` as "no target" will bind it. It is not a real GL name on this host.
#[cfg(test)]
pub fn install_cpu_color_target_for_tests(width: u32, height: u32, rgba_gl_order: Vec<u8>) {
    assert_eq!(
        rgba_gl_order.len(),
        width as usize * height as usize * 4,
        "CPU color target must be tight RGBA8"
    );
    {
        let mut state = lock_state();
        state.cpu = Some(CpuTarget {
            width,
            height,
            rgba_gl_order,
        });
        state.framebuffer = 1;
        state.fbo_width = width;
        state.fbo_height = height;
        state.context_ready = true;
    }
    fire_context_reset(false);
}

fn ensure_context(width: u32, height: u32) -> bool {
    let width = width.clamp(1, MAX_EDGE);
    let height = height.clamp(1, MAX_EDGE);
    // A test (or a previous ensure) already published a target of at least this size.
    {
        let state = lock_state();
        if state.context_ready && state.cpu.is_some() {
            return true;
        }
        if state.context_ready && state.fbo_width >= width && state.fbo_height >= height {
            return true;
        }
    }
    #[cfg(target_os = "ios")]
    {
        let (kind, major, depth, stencil) = {
            let state = lock_state();
            (
                state.context_type.unwrap_or(HwContextType::OpenGlEs3),
                state.version_major,
                state.depth,
                state.stencil,
            )
        };
        match ios::ensure(kind, major, depth, stencil, width, height) {
            Some(framebuffer) => {
                let mut state = lock_state();
                state.framebuffer = framebuffer;
                state.fbo_width = width;
                state.fbo_height = height;
                state.context_ready = true;
                state.cpu = None;
                true
            }
            None => false,
        }
    }
    #[cfg(not(target_os = "ios"))]
    {
        let _ = (width, height);
        lock_state().context_ready
    }
}

fn fire_context_reset(again: bool) {
    let reset = {
        let mut state = lock_state();
        if !state.context_ready {
            return;
        }
        if state.reset_sent && !again {
            return;
        }
        state.reset_sent = true;
        state.context_reset
    };
    if let Some(context_reset) = reset {
        unsafe { context_reset() };
    }
}

/// Crops the bottom-left `width`×`height` of a GL-ordered buffer and, when the core
/// asked for it, flips so row 0 is the top of the picture the compositor expects.
fn orient_gl_rgba(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    width: u32,
    height: u32,
    bottom_left_origin: bool,
) -> Vec<u8> {
    let width = width.min(src_width);
    let height = height.min(src_height);
    let mut out = vec![0u8; width as usize * height as usize * 4];
    let src_row_bytes = src_width as usize * 4;
    let dst_row_bytes = width as usize * 4;
    for y in 0..height as usize {
        // glReadPixels row 0 is the bottom `height` rows of the FBO. The visible
        // image is those rows, not the top of a taller target.
        let src_y = if bottom_left_origin {
            height as usize - 1 - y
        } else {
            y
        };
        let src_off = src_y * src_row_bytes;
        let dst_off = y * dst_row_bytes;
        out[dst_off..dst_off + dst_row_bytes]
            .copy_from_slice(&src[src_off..src_off + dst_row_bytes]);
    }
    out
}

unsafe extern "C" fn frontend_get_current_framebuffer() -> usize {
    lock_state().framebuffer as usize
}

unsafe extern "C" fn frontend_get_proc_address(symbol: *const c_char) -> *const c_void {
    if symbol.is_null() {
        return std::ptr::null();
    }
    #[cfg(target_os = "ios")]
    {
        return ios::proc_address(symbol);
    }
    #[cfg(not(target_os = "ios"))]
    {
        let _ = symbol;
        std::ptr::null()
    }
}

#[cfg(target_os = "ios")]
mod ios {
    use std::ffi::{c_char, c_void};

    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    use crate::gfx::hw::HwContextType;

    const GL_TEXTURE_2D: u32 = 0x0DE1;
    const GL_RGBA: u32 = 0x1908;
    const GL_UNSIGNED_BYTE: u32 = 0x1401;
    const GL_TEXTURE_MIN_FILTER: u32 = 0x2801;
    const GL_TEXTURE_MAG_FILTER: u32 = 0x2800;
    const GL_LINEAR: i32 = 0x2601;
    const GL_FRAMEBUFFER: u32 = 0x8D40;
    const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
    const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
    const GL_RENDERBUFFER: u32 = 0x8D41;
    const GL_DEPTH_STENCIL_ATTACHMENT: u32 = 0x821A;
    const GL_DEPTH_ATTACHMENT: u32 = 0x8D00;
    const GL_DEPTH24_STENCIL8: u32 = 0x88F0;
    const GL_DEPTH_COMPONENT16: u32 = 0x81A5;

    const API_GLES2: usize = 2;
    const API_GLES3: usize = 3;

    /// `RTLD_DEFAULT`: look up a symbol the process already linked.
    const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

    struct Live {
        context: *mut AnyObject,
        framebuffer: u32,
        texture: u32,
        depth: u32,
        api: usize,
    }

    // The EAGL context pointer is only touched from the emulation thread, same as the
    // rest of the libretro callbacks. The mutex in the parent module covers the flags.
    static LIVE: std::sync::Mutex<Option<Live>> = std::sync::Mutex::new(None);

    unsafe impl Send for Live {}

    fn live_lock() -> std::sync::MutexGuard<'static, Option<Live>> {
        match LIVE.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    #[link(name = "OpenGLES", kind = "framework")]
    unsafe extern "C" {
        fn glGenFramebuffers(n: i32, framebuffers: *mut u32);
        fn glBindFramebuffer(target: u32, framebuffer: u32);
        fn glDeleteFramebuffers(n: i32, framebuffers: *const u32);
        fn glGenTextures(n: i32, textures: *mut u32);
        fn glBindTexture(target: u32, texture: u32);
        fn glDeleteTextures(n: i32, textures: *const u32);
        fn glTexImage2D(
            target: u32,
            level: i32,
            internalformat: i32,
            width: i32,
            height: i32,
            border: i32,
            format: u32,
            type_: u32,
            pixels: *const c_void,
        );
        fn glTexParameteri(target: u32, pname: u32, param: i32);
        fn glFramebufferTexture2D(
            target: u32,
            attachment: u32,
            textarget: u32,
            texture: u32,
            level: i32,
        );
        fn glGenRenderbuffers(n: i32, renderbuffers: *mut u32);
        fn glBindRenderbuffer(target: u32, renderbuffer: u32);
        fn glDeleteRenderbuffers(n: i32, renderbuffers: *const u32);
        fn glRenderbufferStorage(target: u32, internalformat: u32, width: i32, height: i32);
        fn glFramebufferRenderbuffer(
            target: u32,
            attachment: u32,
            renderbuffertarget: u32,
            renderbuffer: u32,
        );
        fn glCheckFramebufferStatus(target: u32) -> u32;
        fn glReadPixels(
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            format: u32,
            type_: u32,
            pixels: *mut c_void,
        );
        fn glFinish();
    }

    unsafe extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    fn class() -> Option<&'static AnyClass> {
        AnyClass::get(c"EAGLContext")
    }

    fn make_context(api: usize) -> *mut AnyObject {
        let Some(class) = class() else {
            log::info!("OpenGL: EAGLContext is not in this process");
            return std::ptr::null_mut();
        };
        unsafe {
            let allocated: *mut AnyObject = msg_send![class, alloc];
            if allocated.is_null() {
                return std::ptr::null_mut();
            }
            let context: *mut AnyObject = msg_send![allocated, initWithAPI: api];
            context
        }
    }

    pub(super) fn make_current() -> bool {
        let guard = live_lock();
        let Some(live) = guard.as_ref() else {
            return false;
        };
        let Some(class) = class() else {
            return false;
        };
        let context = live.context;
        unsafe {
            let ok: i8 = msg_send![class, setCurrentContext: context];
            ok != 0
        }
    }

    pub(super) fn ensure(
        kind: HwContextType,
        version_major: u32,
        depth: bool,
        stencil: bool,
        width: u32,
        height: u32,
    ) -> Option<u32> {
        let want_gles3 = match kind {
            HwContextType::OpenGlEs2 => false,
            HwContextType::OpenGlEsVersion => version_major >= 3,
            _ => true,
        };
        let mut guard = live_lock();
        if guard.is_none() {
            let api = if want_gles3 { API_GLES3 } else { API_GLES2 };
            let mut context = make_context(api);
            let mut used = api;
            if context.is_null() && api == API_GLES3 {
                context = make_context(API_GLES2);
                used = API_GLES2;
            }
            if context.is_null() {
                log::info!("OpenGL: EAGLContext initWithAPI failed");
                return None;
            }
            *guard = Some(Live {
                context,
                framebuffer: 0,
                texture: 0,
                depth: 0,
                api: used,
            });
        }
        let live = guard.as_mut().unwrap();
        let class = class()?;
        let context = live.context;
        let current_ok = unsafe {
            let ok: i8 = msg_send![class, setCurrentContext: context];
            ok != 0
        };
        if !current_ok {
            log::info!("OpenGL: EAGLContext setCurrentContext failed");
            return None;
        }

        unsafe {
            if live.texture == 0 {
                glGenTextures(1, &mut live.texture);
            }
            glBindTexture(GL_TEXTURE_2D, live.texture);
            glTexImage2D(
                GL_TEXTURE_2D,
                0,
                GL_RGBA as i32,
                width as i32,
                height as i32,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                std::ptr::null(),
            );
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);

            if live.framebuffer == 0 {
                glGenFramebuffers(1, &mut live.framebuffer);
            }
            glBindFramebuffer(GL_FRAMEBUFFER, live.framebuffer);
            glFramebufferTexture2D(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                live.texture,
                0,
            );

            if depth || stencil {
                if live.depth == 0 {
                    glGenRenderbuffers(1, &mut live.depth);
                }
                glBindRenderbuffer(GL_RENDERBUFFER, live.depth);
                let (internal, attachment) = if live.api == API_GLES3 {
                    (GL_DEPTH24_STENCIL8, GL_DEPTH_STENCIL_ATTACHMENT)
                } else {
                    (GL_DEPTH_COMPONENT16, GL_DEPTH_ATTACHMENT)
                };
                glRenderbufferStorage(GL_RENDERBUFFER, internal, width as i32, height as i32);
                glFramebufferRenderbuffer(GL_FRAMEBUFFER, attachment, GL_RENDERBUFFER, live.depth);
            }

            let status = glCheckFramebufferStatus(GL_FRAMEBUFFER);
            if status != GL_FRAMEBUFFER_COMPLETE {
                log::info!("OpenGL: FBO incomplete (status {status:#x})");
                return None;
            }
            log::info!(
                "OpenGL: EAGL ES{} FBO {} at {width}x{height}",
                if live.api == API_GLES3 { 3 } else { 2 },
                live.framebuffer
            );
            Some(live.framebuffer)
        }
    }

    pub(super) fn read_rgba(
        framebuffer: u32,
        width: u32,
        height: u32,
        bottom_left_origin: bool,
    ) -> Option<Vec<u8>> {
        if width == 0 || height == 0 || framebuffer == 0 {
            return None;
        }
        if !make_current() {
            return None;
        }
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        unsafe {
            glBindFramebuffer(GL_FRAMEBUFFER, framebuffer);
            glFinish();
            glReadPixels(
                0,
                0,
                width as i32,
                height as i32,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                pixels.as_mut_ptr().cast(),
            );
        }
        if bottom_left_origin {
            flip_rows(&mut pixels, width, height);
        }
        Some(pixels)
    }

    pub(super) fn proc_address(symbol: *const c_char) -> *const c_void {
        unsafe { dlsym(RTLD_DEFAULT, symbol) as *const c_void }
    }

    pub(super) fn destroy() {
        let mut guard = live_lock();
        let Some(live) = guard.take() else {
            return;
        };
        if let Some(class) = class() {
            unsafe {
                let _: i8 = msg_send![class, setCurrentContext: live.context];
                if live.framebuffer != 0 {
                    glDeleteFramebuffers(1, &live.framebuffer);
                }
                if live.texture != 0 {
                    glDeleteTextures(1, &live.texture);
                }
                if live.depth != 0 {
                    glDeleteRenderbuffers(1, &live.depth);
                }
                let _: i8 =
                    msg_send![class, setCurrentContext: std::ptr::null_mut::<*mut AnyObject>()];
                let _: () = msg_send![live.context, release];
            }
        }
    }

    fn flip_rows(pixels: &mut [u8], width: u32, height: u32) {
        let row = width as usize * 4;
        let height = height as usize;
        if row == 0 || height < 2 {
            return;
        }
        let mut tmp = vec![0u8; row];
        for y in 0..height / 2 {
            let top = y * row;
            let bottom = (height - 1 - y) * row;
            tmp.copy_from_slice(&pixels[top..top + row]);
            pixels.copy_within(bottom..bottom + row, top);
            pixels[bottom..bottom + row].copy_from_slice(&tmp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{FrameView, PixelFormat};
    use crate::gfx::vulkan_hw::{self, ENV_GET_PREFERRED_HW_RENDER, ENV_SET_HW_RENDER};
    use std::sync::atomic::{AtomicU32, Ordering};

    fn blank_callback(context_type: HwContextType) -> RetroHwRenderCallback {
        RetroHwRenderCallback {
            context_type: context_type as u32,
            context_reset: None,
            get_current_framebuffer: None,
            get_proc_address: None,
            depth: false,
            stencil: false,
            bottom_left_origin: false,
            version_major: 3,
            version_minor: 0,
            cache_context: false,
            context_destroy: None,
            debug_context: false,
        }
    }

    #[test]
    fn set_hw_render_accepts_gl_family_and_still_prefers_vulkan() {
        let _guard = vulkan_hw::test_guard();
        reset();
        vulkan_hw::reset();
        for kind in [
            HwContextType::OpenGl,
            HwContextType::OpenGlEs2,
            HwContextType::OpenGlCore,
            HwContextType::OpenGlEs3,
            HwContextType::OpenGlEsVersion,
        ] {
            reset();
            let mut callback = blank_callback(kind);
            let ok = unsafe {
                vulkan_hw::try_environment(
                    ENV_SET_HW_RENDER,
                    &mut callback as *mut _ as *mut c_void,
                )
                .unwrap()
            };
            assert!(ok, "{kind:?} must be accepted");
            assert!(callback.get_current_framebuffer.is_some());
            assert!(callback.get_proc_address.is_some());
            assert_eq!(status().context_type, Some(kind));
            assert!(status().accepted);
            // Preferred stays Vulkan so dual-backend cores keep MoltenVK.
            assert!(!vulkan_hw::status().set_hw_render_accepted);
            let mut preferred: u32 = 0;
            assert!(unsafe {
                vulkan_hw::try_environment(
                    ENV_GET_PREFERRED_HW_RENDER,
                    &mut preferred as *mut u32 as *mut c_void,
                )
                .unwrap()
            });
            assert_eq!(preferred, HwContextType::Vulkan as u32);
            // No symbol table on the host. The callback is real; the lookup is empty.
            let missing = std::ffi::CString::new("glDefinitelyNotARealSymbol").unwrap();
            assert!(unsafe { callback.get_proc_address.unwrap()(missing.as_ptr()) }.is_null());
        }
        reset();
        vulkan_hw::reset();
    }

    #[test]
    fn direct3d_is_still_refused() {
        let _guard = vulkan_hw::test_guard();
        reset();
        vulkan_hw::reset();
        for bad in [7u32, 9u32, 11u32] {
            let mut callback = blank_callback(HwContextType::OpenGlEs3);
            callback.context_type = bad;
            let ok = unsafe {
                vulkan_hw::try_environment(
                    ENV_SET_HW_RENDER,
                    &mut callback as *mut _ as *mut c_void,
                )
                .unwrap()
            };
            assert!(!ok, "context_type {bad} must be refused");
            assert!(callback.get_current_framebuffer.is_none());
        }
        assert!(!status().accepted);
        assert!(!vulkan_hw::status().set_hw_render_accepted);
        reset();
    }

    #[test]
    fn gl_readback_is_the_rgba_the_compositor_uploads() {
        let _guard = vulkan_hw::test_guard();
        reset();
        vulkan_hw::reset();
        static RESETS: AtomicU32 = AtomicU32::new(0);
        unsafe extern "C" fn on_reset() {
            RESETS.fetch_add(1, Ordering::SeqCst);
        }
        RESETS.store(0, Ordering::SeqCst);

        let mut callback = blank_callback(HwContextType::OpenGlEs3);
        callback.bottom_left_origin = true;
        callback.context_reset = Some(on_reset);
        assert!(unsafe {
            vulkan_hw::try_environment(ENV_SET_HW_RENDER, &mut callback as *mut _ as *mut c_void)
                .unwrap()
        });
        // No EAGL on this host, so the core is not told the context is current yet.
        assert_eq!(RESETS.load(Ordering::SeqCst), 0);
        assert_eq!(unsafe { callback.get_current_framebuffer.unwrap()() }, 0);

        // 2×2, GL order (row 0 = bottom). Bottom is green, top is red.
        // R G B A
        let gl_order = vec![
            0, 255, 0, 255, // bottom-left green
            0, 255, 0, 255, // bottom-right green
            255, 0, 0, 255, // top-left red
            255, 0, 0, 255, // top-right red
        ];
        install_cpu_color_target_for_tests(2, 2, gl_order);
        assert_eq!(RESETS.load(Ordering::SeqCst), 1);
        assert_eq!(unsafe { callback.get_current_framebuffer.unwrap()() }, 1);

        let frame = read_presented_rgba(2, 2).expect("color target");
        let rgba = &frame.rgba;
        // Top-left of the picture the compositor samples is red.
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(&rgba[4..8], &[255, 0, 0, 255]);
        assert_eq!(&rgba[8..12], &[0, 255, 0, 255]);
        assert_eq!((frame.width, frame.height), (2, 2));

        let view = FrameView {
            data: rgba,
            width: frame.width,
            height: frame.height,
            stride_bytes: 8,
            format: PixelFormat::Rgba8888,
        };
        let mut scratch = Vec::new();
        let uploaded = super::super::convert::to_rgba8(&view, &mut scratch);
        assert_eq!(uploaded, rgba.as_slice());
        assert_eq!(status().frames_read, 1);

        // A later Vulkan request is still the Vulkan door, and it wins the picture.
        reset();
        vulkan_hw::reset();
        let mut vulkan = blank_callback(HwContextType::Vulkan);
        assert!(unsafe {
            vulkan_hw::try_environment(ENV_SET_HW_RENDER, &mut vulkan as *mut _ as *mut c_void)
                .unwrap()
        });
        assert!(vulkan_hw::status().set_hw_render_accepted);
        assert!(!status().accepted);
        assert_eq!(
            vulkan_hw::status().context_type,
            Some(HwContextType::Vulkan)
        );
        reset();
        vulkan_hw::reset();
    }

    #[test]
    fn top_left_cores_are_not_flipped() {
        let _guard = vulkan_hw::test_guard();
        reset();
        let mut callback = blank_callback(HwContextType::OpenGl);
        callback.bottom_left_origin = false;
        assert!(unsafe {
            vulkan_hw::try_environment(ENV_SET_HW_RENDER, &mut callback as *mut _ as *mut c_void)
                .unwrap()
        });
        let gl_order = vec![
            1, 2, 3, 255, // row 0 stays row 0 when the core already rendered top-left
            4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255,
        ];
        install_cpu_color_target_for_tests(2, 2, gl_order.clone());
        let rgba = read_presented_rgba(2, 2).unwrap();
        assert_eq!(rgba.rgba, gl_order);
        reset();
    }

    #[test]
    fn unknown_symbol_name_is_not_read() {
        // Null symbol must not crash. The CStr path is the other branch.
        assert!(unsafe { frontend_get_proc_address(std::ptr::null()) }.is_null());
        let name = c"glClear";
        // On the host this is null. On iOS it may resolve; either is a real answer.
        let _ = unsafe { frontend_get_proc_address(name.as_ptr()) };
    }
}
