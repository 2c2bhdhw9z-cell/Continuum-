//! Step 4 of `docs/SET_HW_RENDER_DESIGN.md` §13: frontend half of
//! `RETRO_ENVIRONMENT_SET_HW_RENDER` / `GET_HW_RENDER_INTERFACE` for **Vulkan**.
//!
//! Proof core: **Beetle PSX HW** (`mednafen_psx_hw`), not N64 and not Citra — a wrong
//! contract should fail on a PlayStation title whose software path (`pcsx_rearmed`)
//! already works on device.
//!
//! Accepts Vulkan. OpenGL and OpenGL ES are accepted by [`super::gl_hw`] instead of
//! being refused here. Preferred stays Vulkan. Fills frontend callbacks; stores
//! negotiation; serves `GET_HW_RENDER_INTERFACE` once a context is marked ready; records
//! `set_image` for the compositor. Live MoltenVK handles come from
//! [`crate::gfx::moltenvk_device`] after Metal attach (`prepare_vulkan_hw`); install runs
//! when `SET_HW_RENDER` is accepted or when prepare lands after accept. Beetle PSX HW is in
//! `IOS_CORES` / the IPA; Settings selects it for PlayStation launches.
//!
//! Constants verified against `.work/hdr/libretro/{libretro,libretro_vulkan}.h`.

use std::ffi::{c_char, c_void, CStr};
use std::sync::Mutex;

use crate::gfx::hw::HwContextType;

pub const ENV_SET_HW_RENDER: u32 = 14;
pub const ENV_GET_HW_RENDER_INTERFACE: u32 = 41 | ENV_EXPERIMENTAL;
pub const ENV_SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE: u32 = 43 | ENV_EXPERIMENTAL;
pub const ENV_GET_PREFERRED_HW_RENDER: u32 = 56;
const ENV_EXPERIMENTAL: u32 = 0x1_0000;

pub const HW_RENDER_INTERFACE_VULKAN: u32 = 0;
pub const HW_RENDER_INTERFACE_VULKAN_VERSION: u32 = 5;
pub const HW_RENDER_NEGOTIATION_INTERFACE_VULKAN_VERSION: u32 = 2;

static STATE: Mutex<VulkanHwState> = Mutex::new(VulkanHwState::new());
static QUEUE_LOCKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const HANDLE_TOKEN: *mut c_void = 1 as *mut c_void;

fn lock_state() -> std::sync::MutexGuard<'static, VulkanHwState> {
    match STATE.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

pub fn reset() {
    *lock_state() = VulkanHwState::new();
    QUEUE_LOCKED.store(false, std::sync::atomic::Ordering::Release);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VulkanHwStatus {
    pub set_hw_render_accepted: bool,
    pub context_type: Option<HwContextType>,
    pub negotiation_stored: bool,
    pub interface_ready: bool,
    pub set_image_count: u64,
    pub bottom_left_origin: bool,
}

pub fn status() -> VulkanHwStatus {
    let s = lock_state();
    VulkanHwStatus {
        set_hw_render_accepted: s.accepted,
        context_type: s.context_type,
        negotiation_stored: s.negotiation.is_some(),
        interface_ready: s.interface_ready,
        set_image_count: s.set_image_count,
        bottom_left_origin: s.bottom_left_origin,
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PendingVulkanFrame {
    pub image_view: u64,
    /// `VkImage` from `retro_vulkan_image::create_info.image` when the core filled it.
    pub image: u64,
    pub image_layout: u32,
    pub width: u32,
    pub height: u32,
    pub src_queue_family: u32,
}

pub fn take_pending_frame() -> Option<PendingVulkanFrame> {
    lock_state().pending.take()
}

/// Records the size from `video_refresh(RETRO_HW_FRAME_BUFFER_VALID, w, h, …)`.
///
/// `set_image` does not carry width/height; the refresh callback does. Called from the
/// native core when a hardware frame arrives so the compositor can adopt at the right size.
pub fn note_frame_size(width: u32, height: u32) {
    let mut state = lock_state();
    if let Some(pending) = state.pending.as_mut() {
        pending.width = width;
        pending.height = height;
    } else {
        state.pending = Some(PendingVulkanFrame {
            width,
            height,
            ..PendingVulkanFrame::default()
        });
    }
}

/// Consumes a pending `set_image` and adopts its `VkImage` into the compositor when possible.
///
/// Returns whether a texture was adopted. Soft failure (no pending / no device) is `Ok(false)`.
pub fn apply_pending_to_renderer(renderer: &mut crate::gfx::Renderer) -> Result<bool, String> {
    let Some(frame) = take_pending_frame() else {
        return Ok(false);
    };
    if frame.image == 0 {
        // Core called set_image without filling create_info.image — cannot export yet.
        return Ok(false);
    }
    let width = frame.width;
    let height = frame.height;
    if width == 0 || height == 0 {
        // Size not yet noted from video_refresh; put the frame back and wait.
        lock_state().pending = Some(frame);
        return Ok(false);
    }
    crate::gfx::moltenvk_device::adopt_pending_frame(renderer, frame.image, width, height)
}

/// `struct retro_hw_render_callback` — clang layout on this host: sizeof 64,
/// framebuffer @16, proc @24, depth @32, version_major @36, context_destroy @48.
#[repr(C)]
pub struct RetroHwRenderCallback {
    pub context_type: u32,
    pub context_reset: Option<unsafe extern "C" fn()>,
    pub get_current_framebuffer: Option<unsafe extern "C" fn() -> usize>,
    pub get_proc_address: Option<unsafe extern "C" fn(*const c_char) -> *const c_void>,
    pub depth: bool,
    pub stencil: bool,
    pub bottom_left_origin: bool,
    pub version_major: u32,
    pub version_minor: u32,
    pub cache_context: bool,
    pub context_destroy: Option<unsafe extern "C" fn()>,
    pub debug_context: bool,
}

#[cfg(test)]
#[repr(C)]
struct RetroHwRenderInterfaceHeader {
    interface_type: u32,
    interface_version: u32,
}

/// Field order from `libretro_vulkan.h` (SESSION_HANDOFF §15): `handle` third;
/// `get_device_proc_addr` before `get_instance_proc_addr`; `queue` then `queue_index`.
#[repr(C)]
struct RetroHwRenderInterfaceVulkan {
    interface_type: u32,
    interface_version: u32,
    handle: *mut c_void,
    instance: u64,
    gpu: u64,
    device: u64,
    get_device_proc_addr: Option<unsafe extern "C" fn(u64, *const c_char) -> *const c_void>,
    get_instance_proc_addr: Option<unsafe extern "C" fn(u64, *const c_char) -> *const c_void>,
    queue: u64,
    queue_index: u32,
    set_image:
        Option<unsafe extern "C" fn(*mut c_void, *const RetroVulkanImage, u32, *const u64, u32)>,
    get_sync_index: Option<unsafe extern "C" fn(*mut c_void) -> u32>,
    get_sync_index_mask: Option<unsafe extern "C" fn(*mut c_void) -> u32>,
    set_command_buffers: Option<unsafe extern "C" fn(*mut c_void, u32, *const u64)>,
    wait_sync_index: Option<unsafe extern "C" fn(*mut c_void)>,
    lock_queue: Option<unsafe extern "C" fn(*mut c_void)>,
    unlock_queue: Option<unsafe extern "C" fn(*mut c_void)>,
    set_signal_semaphore: Option<unsafe extern "C" fn(*mut c_void, u64)>,
}

/// `VkImageViewCreateInfo` without ash's lifetime marker — clang layout on 64-bit.
#[repr(C)]
struct RawImageViewCreateInfo {
    s_type: u32,
    p_next: usize,
    flags: u32,
    image: u64,
    view_type: u32,
    format: u32,
    components: [u32; 4],
    subresource_range: [u32; 5],
}

#[repr(C)]
struct RetroVulkanImage {
    image_view: u64,
    image_layout: u32,
    create_info: RawImageViewCreateInfo,
}

#[repr(C)]
struct RetroHwRenderNegotiationHeader {
    interface_type: u32,
    interface_version: u32,
}

struct VulkanHwState {
    accepted: bool,
    context_type: Option<HwContextType>,
    bottom_left_origin: bool,
    context_reset: Option<unsafe extern "C" fn()>,
    context_destroy: Option<unsafe extern "C" fn()>,
    negotiation: Option<*mut c_void>,
    interface: Option<Box<RetroHwRenderInterfaceVulkan>>,
    interface_ready: bool,
    instance: u64,
    gpu: u64,
    device: u64,
    queue: u64,
    queue_index: u32,
    get_instance_proc_addr: Option<unsafe extern "C" fn(u64, *const c_char) -> *const c_void>,
    get_device_proc_addr: Option<unsafe extern "C" fn(u64, *const c_char) -> *const c_void>,
    pending: Option<PendingVulkanFrame>,
    set_image_count: u64,
}

unsafe impl Send for VulkanHwState {}

impl VulkanHwState {
    const fn new() -> Self {
        Self {
            accepted: false,
            context_type: None,
            bottom_left_origin: false,
            context_reset: None,
            context_destroy: None,
            negotiation: None,
            interface: None,
            interface_ready: false,
            instance: 0,
            gpu: 0,
            device: 0,
            queue: 0,
            queue_index: 0,
            get_instance_proc_addr: None,
            get_device_proc_addr: None,
            pending: None,
            set_image_count: 0,
        }
    }
}

pub unsafe fn try_environment(cmd: u32, data: *mut c_void) -> Option<bool> {
    match cmd {
        ENV_SET_HW_RENDER => Some(unsafe { on_set_hw_render(data) }),
        ENV_GET_HW_RENDER_INTERFACE => Some(unsafe { on_get_hw_render_interface(data) }),
        ENV_SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE => {
            Some(unsafe { on_set_negotiation(data) })
        }
        ENV_GET_PREFERRED_HW_RENDER => Some(unsafe { on_get_preferred(data) }),
        _ => None,
    }
}

unsafe fn on_set_hw_render(data: *mut c_void) -> bool {
    if data.is_null() {
        return false;
    }
    let callback = unsafe { &mut *(data as *mut RetroHwRenderCallback) };
    let Some(context_type) = HwContextType::from_libretro(callback.context_type) else {
        log::info!(
            "SET_HW_RENDER refused: unsupported context_type {}",
            callback.context_type
        );
        return false;
    };
    if context_type.is_gl() {
        return crate::gfx::gl_hw::accept(callback);
    }
    if context_type != HwContextType::Vulkan {
        log::info!(
            "SET_HW_RENDER refused: {:?} (not Vulkan or OpenGL)",
            context_type
        );
        return false;
    }
    // This request is the Vulkan door. A previous GL accept must not keep eating
    // hardware frames.
    crate::gfx::gl_hw::note_vulkan_took_over();
    callback.get_current_framebuffer = Some(frontend_get_current_framebuffer);
    callback.get_proc_address = Some(frontend_get_proc_address);
    let mut state = lock_state();
    state.accepted = true;
    state.context_type.replace(context_type);
    state.bottom_left_origin = callback.bottom_left_origin;
    state.context_reset = callback.context_reset;
    state.context_destroy = callback.context_destroy;
    state.interface_ready = false;
    state.interface = None;
    log::info!(
        "SET_HW_RENDER accepted: Vulkan {}.{} bottom_left_origin={}",
        callback.version_major,
        callback.version_minor,
        callback.bottom_left_origin
    );
    drop(state);
    // If MoltenVK was prepared at Metal attach, install live handles and call context_reset
    // now. If prepare has not run yet, prepare_vulkan_hw will install when it lands.
    let installed = crate::gfx::moltenvk_device::try_install_into_vulkan_hw();
    if installed {
        log::info!("SET_HW_RENDER: installed shared MoltenVK handles and called context_reset");
    } else {
        log::info!(
            "SET_HW_RENDER: accepted; MoltenVK handles not ready yet (prepare_vulkan_hw after Metal attach)"
        );
    }
    true
}

unsafe fn on_get_preferred(data: *mut c_void) -> bool {
    if data.is_null() {
        return false;
    }
    unsafe { *(data as *mut u32) = HwContextType::Vulkan as u32 };
    true
}

unsafe fn on_set_negotiation(data: *mut c_void) -> bool {
    if data.is_null() {
        return false;
    }
    let header = unsafe { &*(data as *const RetroHwRenderNegotiationHeader) };
    if header.interface_type != HW_RENDER_INTERFACE_VULKAN {
        log::info!(
            "SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE refused: type {}",
            header.interface_type
        );
        return false;
    }
    if header.interface_version == 0
        || header.interface_version > HW_RENDER_NEGOTIATION_INTERFACE_VULKAN_VERSION
    {
        log::info!(
            "SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE refused: version {}",
            header.interface_version
        );
        return false;
    }
    lock_state().negotiation = Some(data);
    log::info!(
        "SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE stored (v{})",
        header.interface_version
    );
    true
}

unsafe fn on_get_hw_render_interface(data: *mut c_void) -> bool {
    if data.is_null() {
        return false;
    }
    let mut state = lock_state();
    if !state.accepted || !state.interface_ready {
        return false;
    }
    let iface = build_interface(&state);
    let ptr = iface.as_ref() as *const RetroHwRenderInterfaceVulkan as *const c_void;
    state.interface = Some(iface);
    unsafe { *(data as *mut *const c_void) = ptr };
    true
}

fn build_interface(state: &VulkanHwState) -> Box<RetroHwRenderInterfaceVulkan> {
    Box::new(RetroHwRenderInterfaceVulkan {
        interface_type: HW_RENDER_INTERFACE_VULKAN,
        interface_version: HW_RENDER_INTERFACE_VULKAN_VERSION,
        handle: HANDLE_TOKEN,
        instance: state.instance,
        gpu: state.gpu,
        device: state.device,
        get_device_proc_addr: state.get_device_proc_addr.or(Some(null_device_proc)),
        get_instance_proc_addr: state.get_instance_proc_addr.or(Some(null_instance_proc)),
        queue: state.queue,
        queue_index: state.queue_index,
        set_image: Some(frontend_set_image),
        get_sync_index: Some(frontend_get_sync_index),
        get_sync_index_mask: Some(frontend_get_sync_index_mask),
        set_command_buffers: Some(frontend_set_command_buffers),
        wait_sync_index: Some(frontend_wait_sync_index),
        lock_queue: Some(frontend_lock_queue),
        unlock_queue: Some(frontend_unlock_queue),
        set_signal_semaphore: Some(frontend_set_signal_semaphore),
    })
}

/// Contract-only readiness for host tests. Live cores need `install_vulkan_handles`.
pub fn mark_interface_ready_for_tests() {
    let mut state = lock_state();
    if !state.accepted {
        return;
    }
    state.interface_ready = true;
    let iface = build_interface(&state);
    state.interface = Some(iface);
}

pub fn install_vulkan_handles(
    instance: u64,
    gpu: u64,
    device: u64,
    queue: u64,
    queue_index: u32,
    get_instance_proc_addr: Option<unsafe extern "C" fn(u64, *const c_char) -> *const c_void>,
    get_device_proc_addr: Option<unsafe extern "C" fn(u64, *const c_char) -> *const c_void>,
) {
    let reset = {
        let mut state = lock_state();
        if !state.accepted {
            log::warn!("install_vulkan_handles: SET_HW_RENDER was never accepted");
            return;
        }
        state.instance = instance;
        state.gpu = gpu;
        state.device = device;
        state.queue = queue;
        state.queue_index = queue_index;
        state.get_instance_proc_addr = get_instance_proc_addr;
        state.get_device_proc_addr = get_device_proc_addr;
        state.interface_ready = true;
        let iface = build_interface(&state);
        state.interface = Some(iface);
        state.context_reset
    };
    if let Some(context_reset) = reset {
        unsafe { context_reset() };
    }
}

pub fn destroy_context() {
    let destroy = {
        let mut state = lock_state();
        let destroy = state.context_destroy;
        state.interface_ready = false;
        state.interface = None;
        state.pending = None;
        state.instance = 0;
        state.gpu = 0;
        state.device = 0;
        state.queue = 0;
        destroy
    };
    if let Some(context_destroy) = destroy {
        unsafe { context_destroy() };
    }
}

unsafe extern "C" fn frontend_get_current_framebuffer() -> usize {
    0
}

unsafe extern "C" fn frontend_get_proc_address(symbol: *const c_char) -> *const c_void {
    if symbol.is_null() {
        return std::ptr::null();
    }
    let name = unsafe { CStr::from_ptr(symbol) };
    let state = lock_state();
    if let Some(get_instance) = state.get_instance_proc_addr {
        if state.instance != 0 {
            return unsafe { get_instance(state.instance, symbol) };
        }
    }
    let _ = name;
    std::ptr::null()
}

unsafe extern "C" fn null_instance_proc(_i: u64, _n: *const c_char) -> *const c_void {
    std::ptr::null()
}
unsafe extern "C" fn null_device_proc(_d: u64, _n: *const c_char) -> *const c_void {
    std::ptr::null()
}

unsafe extern "C" fn frontend_set_image(
    _handle: *mut c_void,
    image: *const RetroVulkanImage,
    _num_semaphores: u32,
    _semaphores: *const u64,
    src_queue_family: u32,
) {
    if image.is_null() {
        return;
    }
    let image = unsafe { &*image };
    let mut state = lock_state();
    let width = state.pending.map(|p| p.width).unwrap_or(0);
    let height = state.pending.map(|p| p.height).unwrap_or(0);
    state.pending = Some(PendingVulkanFrame {
        image_view: image.image_view,
        image: image.create_info.image,
        image_layout: image.image_layout,
        width,
        height,
        src_queue_family,
    });
    state.set_image_count = state.set_image_count.saturating_add(1);
}

unsafe extern "C" fn frontend_get_sync_index(_h: *mut c_void) -> u32 {
    0
}
unsafe extern "C" fn frontend_get_sync_index_mask(_h: *mut c_void) -> u32 {
    0b1
}
unsafe extern "C" fn frontend_set_command_buffers(_h: *mut c_void, _n: u32, _c: *const u64) {}
unsafe extern "C" fn frontend_wait_sync_index(_h: *mut c_void) {}

unsafe extern "C" fn frontend_lock_queue(_handle: *mut c_void) {
    let _ = _handle;
    while QUEUE_LOCKED
        .compare_exchange_weak(
            false,
            true,
            std::sync::atomic::Ordering::Acquire,
            std::sync::atomic::Ordering::Relaxed,
        )
        .is_err()
    {
        std::hint::spin_loop();
    }
}

unsafe extern "C" fn frontend_unlock_queue(_handle: *mut c_void) {
    let _ = _handle;
    QUEUE_LOCKED.store(false, std::sync::atomic::Ordering::Release);
}

unsafe extern "C" fn frontend_set_signal_semaphore(_h: *mut c_void, _s: u64) {}

#[cfg(test)]
pub(crate) fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    match TEST_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    fn guard() -> std::sync::MutexGuard<'static, ()> {
        super::test_guard()
    }

    #[test]
    fn hw_render_callback_matches_clang_layout() {
        assert_eq!(size_of::<RetroHwRenderCallback>(), 64);
        assert_eq!(offset_of!(RetroHwRenderCallback, context_type), 0);
        assert_eq!(offset_of!(RetroHwRenderCallback, context_reset), 8);
        assert_eq!(
            offset_of!(RetroHwRenderCallback, get_current_framebuffer),
            16
        );
        assert_eq!(offset_of!(RetroHwRenderCallback, get_proc_address), 24);
        assert_eq!(offset_of!(RetroHwRenderCallback, depth), 32);
        assert_eq!(offset_of!(RetroHwRenderCallback, stencil), 33);
        assert_eq!(offset_of!(RetroHwRenderCallback, bottom_left_origin), 34);
        assert_eq!(offset_of!(RetroHwRenderCallback, version_major), 36);
        assert_eq!(offset_of!(RetroHwRenderCallback, version_minor), 40);
        assert_eq!(offset_of!(RetroHwRenderCallback, cache_context), 44);
        assert_eq!(offset_of!(RetroHwRenderCallback, context_destroy), 48);
        assert_eq!(offset_of!(RetroHwRenderCallback, debug_context), 56);
        assert!(align_of::<RetroHwRenderCallback>() >= 8);
    }

    #[test]
    fn vulkan_interface_handle_is_third_field() {
        assert_eq!(offset_of!(RetroHwRenderInterfaceVulkan, interface_type), 0);
        assert_eq!(
            offset_of!(RetroHwRenderInterfaceVulkan, interface_version),
            4
        );
        assert_eq!(offset_of!(RetroHwRenderInterfaceVulkan, handle), 8);
        assert_eq!(offset_of!(RetroHwRenderInterfaceVulkan, instance), 16);
        assert_eq!(offset_of!(RetroHwRenderInterfaceVulkan, gpu), 24);
        assert_eq!(offset_of!(RetroHwRenderInterfaceVulkan, device), 32);
        assert_eq!(
            offset_of!(RetroHwRenderInterfaceVulkan, get_device_proc_addr),
            40
        );
        assert_eq!(
            offset_of!(RetroHwRenderInterfaceVulkan, get_instance_proc_addr),
            48
        );
        assert_eq!(offset_of!(RetroHwRenderInterfaceVulkan, queue), 56);
        assert_eq!(offset_of!(RetroHwRenderInterfaceVulkan, queue_index), 64);
        assert_eq!(offset_of!(RetroHwRenderInterfaceVulkan, set_image), 72);
    }

    #[test]
    fn experimental_bits_are_required_on_41_and_43() {
        assert_eq!(ENV_GET_HW_RENDER_INTERFACE, 41 | 0x1_0000);
        assert_eq!(
            ENV_SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE,
            43 | 0x1_0000
        );
        assert_ne!(ENV_GET_HW_RENDER_INTERFACE, 41);
        assert_eq!(ENV_SET_HW_RENDER, 14);
        assert_eq!(ENV_GET_PREFERRED_HW_RENDER, 56);
    }

    #[test]
    fn set_hw_render_accepts_vulkan_and_fills_frontend_callbacks() {
        let _g = guard();
        reset();
        let mut callback = RetroHwRenderCallback {
            context_type: HwContextType::Vulkan as u32,
            context_reset: None,
            get_current_framebuffer: None,
            get_proc_address: None,
            depth: true,
            stencil: true,
            bottom_left_origin: false,
            version_major: 1,
            version_minor: 1,
            cache_context: false,
            context_destroy: None,
            debug_context: false,
        };
        let ok = unsafe {
            try_environment(ENV_SET_HW_RENDER, &mut callback as *mut _ as *mut c_void).unwrap()
        };
        assert!(ok);
        assert!(callback.get_current_framebuffer.is_some());
        assert!(callback.get_proc_address.is_some());
        assert_eq!(unsafe { callback.get_current_framebuffer.unwrap()() }, 0);
        assert!(status().set_hw_render_accepted);
        assert_eq!(status().context_type, Some(HwContextType::Vulkan));
        reset();
    }

    #[test]
    fn set_hw_render_refuses_d3d() {
        let _g = guard();
        reset();
        // OpenGL is no longer in this list. gl_hw accepts it. Direct3D stays refused.
        for bad in [7u32, 9u32] {
            let mut callback = RetroHwRenderCallback {
                context_type: bad,
                context_reset: None,
                get_current_framebuffer: None,
                get_proc_address: None,
                depth: false,
                stencil: false,
                bottom_left_origin: false,
                version_major: 0,
                version_minor: 0,
                cache_context: false,
                context_destroy: None,
                debug_context: false,
            };
            let ok = unsafe {
                try_environment(ENV_SET_HW_RENDER, &mut callback as *mut _ as *mut c_void).unwrap()
            };
            assert!(!ok, "context_type {bad} must be refused");
            assert!(callback.get_current_framebuffer.is_none());
        }
        assert!(!status().set_hw_render_accepted);
        reset();
    }

    #[test]
    fn preferred_hw_render_is_vulkan() {
        let _g = guard();
        let mut value: u32 = 0;
        let ok = unsafe {
            try_environment(
                ENV_GET_PREFERRED_HW_RENDER,
                &mut value as *mut u32 as *mut c_void,
            )
            .unwrap()
        };
        assert!(ok);
        assert_eq!(value, HwContextType::Vulkan as u32);
    }

    #[test]
    fn get_hw_render_interface_needs_prepared_context() {
        let _g = guard();
        reset();
        let mut callback = RetroHwRenderCallback {
            context_type: HwContextType::Vulkan as u32,
            context_reset: None,
            get_current_framebuffer: None,
            get_proc_address: None,
            depth: false,
            stencil: false,
            bottom_left_origin: false,
            version_major: 1,
            version_minor: 0,
            cache_context: false,
            context_destroy: None,
            debug_context: false,
        };
        assert!(unsafe {
            try_environment(ENV_SET_HW_RENDER, &mut callback as *mut _ as *mut c_void).unwrap()
        });
        let mut out: *const c_void = std::ptr::null();
        assert!(!unsafe {
            try_environment(
                ENV_GET_HW_RENDER_INTERFACE,
                &mut out as *mut _ as *mut c_void,
            )
            .unwrap()
        });
        mark_interface_ready_for_tests();
        assert!(unsafe {
            try_environment(
                ENV_GET_HW_RENDER_INTERFACE,
                &mut out as *mut _ as *mut c_void,
            )
            .unwrap()
        });
        assert!(!out.is_null());
        let header = unsafe { &*(out as *const RetroHwRenderInterfaceHeader) };
        assert_eq!(header.interface_type, HW_RENDER_INTERFACE_VULKAN);
        assert_eq!(header.interface_version, HW_RENDER_INTERFACE_VULKAN_VERSION);
        let full = unsafe { &*(out as *const RetroHwRenderInterfaceVulkan) };
        assert!(!full.handle.is_null());
        assert!(full.set_image.is_some());
        let image = RetroVulkanImage {
            image_view: 0xABCD,
            image_layout: 1,
            create_info: RawImageViewCreateInfo {
                s_type: 0,
                p_next: 0,
                flags: 0,
                image: 0x1111,
                view_type: 0,
                format: 0,
                components: [0; 4],
                subresource_range: [0; 5],
            },
        };
        unsafe {
            full.set_image.unwrap()(full.handle, &image, 0, std::ptr::null(), 0);
        }
        let pending = take_pending_frame().unwrap();
        assert_eq!(pending.image_view, 0xABCD);
        assert_eq!(pending.image, 0x1111);
        assert_eq!(status().set_image_count, 1);
        reset();
    }

    #[test]
    fn negotiation_interface_accepts_vulkan_v1_and_v2() {
        let _g = guard();
        reset();
        for version in [1u32, 2u32] {
            let mut header = RetroHwRenderNegotiationHeader {
                interface_type: HW_RENDER_INTERFACE_VULKAN,
                interface_version: version,
            };
            assert!(unsafe {
                try_environment(
                    ENV_SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE,
                    &mut header as *mut _ as *mut c_void,
                )
                .unwrap()
            });
        }
        assert!(status().negotiation_stored);
        let mut bad = RetroHwRenderNegotiationHeader {
            interface_type: 1,
            interface_version: 2,
        };
        assert!(!unsafe {
            try_environment(
                ENV_SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE,
                &mut bad as *mut _ as *mut c_void,
            )
            .unwrap()
        });
        reset();
    }

    #[test]
    fn unknown_env_command_is_not_claimed() {
        assert!(unsafe { try_environment(999_999, std::ptr::null_mut()) }.is_none());
    }

    #[test]
    fn retro_vulkan_image_create_info_image_offset() {
        // create_info at 16; image field at +24 inside VkImageViewCreateInfo → absolute 40.
        assert_eq!(size_of::<RawImageViewCreateInfo>(), 80);
        assert_eq!(offset_of!(RawImageViewCreateInfo, image), 24);
        assert_eq!(offset_of!(RetroVulkanImage, image_view), 0);
        assert_eq!(offset_of!(RetroVulkanImage, image_layout), 8);
        assert_eq!(offset_of!(RetroVulkanImage, create_info), 16);
        assert_eq!(size_of::<RetroVulkanImage>(), 96);
    }

    #[test]
    fn install_vulkan_handles_calls_context_reset() {
        let _g = guard();
        reset();
        static RESET_COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        unsafe extern "C" fn on_reset() {
            RESET_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        RESET_COUNT.store(0, std::sync::atomic::Ordering::SeqCst);
        let mut callback = RetroHwRenderCallback {
            context_type: HwContextType::Vulkan as u32,
            context_reset: Some(on_reset),
            get_current_framebuffer: None,
            get_proc_address: None,
            depth: false,
            stencil: false,
            bottom_left_origin: false,
            version_major: 1,
            version_minor: 0,
            cache_context: false,
            context_destroy: None,
            debug_context: false,
        };
        assert!(unsafe {
            try_environment(ENV_SET_HW_RENDER, &mut callback as *mut _ as *mut c_void).unwrap()
        });
        install_vulkan_handles(1, 2, 3, 4, 0, None, None);
        assert_eq!(RESET_COUNT.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(status().interface_ready);
        let mut out: *const c_void = std::ptr::null();
        assert!(unsafe {
            try_environment(
                ENV_GET_HW_RENDER_INTERFACE,
                &mut out as *mut _ as *mut c_void,
            )
            .unwrap()
        });
        let full = unsafe { &*(out as *const RetroHwRenderInterfaceVulkan) };
        assert_eq!(full.instance, 1);
        assert_eq!(full.device, 3);
        assert_eq!(full.queue, 4);
        reset();
    }
}
