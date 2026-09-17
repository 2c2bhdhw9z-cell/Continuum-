//! The Metal surface, and the one `MTLDevice` the whole process shares.
//!
//! # Which side creates the device
//!
//! `docs/SET_HW_RENDER_DESIGN.md` §2 and the header of `native/ios/MetalCanvas.swift` both
//! said Swift creates the `MTLDevice` and the engine adopts it. Building it proved that
//! impossible with `wgpu` 30, and the conclusion is the *opposite* of what those documents
//! assumed, so the reasoning is recorded here rather than in a commit message.
//!
//! Two facts settle it, both read out of `wgpu-hal-30.0.1`:
//!
//! 1. **There is no public constructor that takes an existing `MTLDevice`.** The only path
//!    from a device to a `metal::Adapter` is `AdapterShared::expose`, and it is private
//!    (`src/metal/mod.rs:428`). `Instance::create_adapter_from_hal` therefore cannot be
//!    reached with a device somebody else made. The queue half *is* public
//!    (`Queue::queue_from_raw`); the device half is not.
//! 2. **`configure` overwrites the layer's device anyway.** `Surface::configure` calls
//!    `CAMetalLayer::setDevice` with wgpu's own device (`src/metal/surface.rs:276`). So even
//!    if Swift assigns its device to the layer, the first configure replaces it, and Swift
//!    is left holding a device that owns nothing the layer will ever draw.
//!
//! Fact 2 is the important one: it means "Swift owns the device" was never merely
//! unsupported, it was actively undone one call later. A build that looked correct would
//! have produced two devices, and the first attempt to share a texture between the core and
//! the compositor would have failed with no obvious cause.
//!
//! So the direction is inverted: **wgpu creates the device, and Swift adopts it** through
//! [`MetalHandles`]. The invariant the design actually cares about — one `MTLDevice` for
//! wgpu, for Swift, and later for MoltenVK, so every texture is shareable by construction —
//! is preserved exactly. Only the ownership arrow moves.
//!
//! On iOS this costs nothing in device selection: `wgpu-hal` enumerates through
//! `objc2_metal::MTLCopyAllDevices`, which on iOS is a shim that returns whatever
//! `MTLCreateSystemDefaultDevice()` would have (`objc2-metal-0.3.2/src/device.rs`). There is
//! one GPU, and this is it.

use core::ffi::c_void;

use crate::error::GfxError;
use crate::gfx::Renderer;

/// The Metal objects the renderer created, addressed as integers so they can cross UniFFI.
///
/// Raw addresses rather than a typed handle because UniFFI has no pointer type, and these
/// travel exactly once: Swift asks for them after `attach_metal` succeeds. Neither is
/// retained on the way out — the renderer owns both for as long as it lives, which is
/// longer than the view that reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetalHandles {
    /// `id<MTLDevice>`.
    pub device: u64,
    /// `id<MTLCommandQueue>`.
    pub queue: u64,
}

/// Builds a renderer that draws into an existing `CAMetalLayer`.
///
/// # Safety
///
/// `layer` must be a live `CAMetalLayer` that outlives the returned [`Renderer`]. Swift
/// holds it as the backing layer of a `UIView`, which satisfies this as long as the view is
/// not released while a session is running.
pub unsafe fn renderer_from_metal_layer(
    layer: *mut c_void,
    width: u32,
    height: u32,
) -> Result<Renderer, GfxError> {
    if layer.is_null() {
        return Err(GfxError::SurfaceCreation(
            "attach_metal was given a null CAMetalLayer".into(),
        ));
    }

    // METAL only. The `vulkan` feature is compiled in for MoltenVK's benefit on the core
    // side, but the compositor must not quietly land on it: a Vulkan surface here would
    // present through a second translation layer for no reason.
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::METAL;
    let instance = wgpu::Instance::new(descriptor);

    // SAFETY: delegated to this function's contract — the caller guarantees `layer` is a
    // live CAMetalLayer outliving the renderer.
    let surface = unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer))
    }
    .map_err(|error| GfxError::SurfaceCreation(error.to_string()))?;

    // `from_surface` is the same constructor the browser uses, which is the point: the
    // backend differs, the renderer does not. It is async because `request_adapter` and
    // `request_device` are; on Metal both resolve without ever pending, and `attach_metal`
    // is a synchronous call from `layoutSubviews`, so it is driven to completion here rather
    // than infecting the UniFFI surface with async.
    pollster::block_on(Renderer::from_surface(instance, surface, width, height))
}

/// Reads back the `MTLDevice` and `MTLCommandQueue` the renderer is using.
///
/// Returns `None` if the renderer is not on the Metal backend, which on iOS cannot happen
/// but is not worth asserting: a `None` here is a diagnostic, an `unwrap` is a crash.
pub fn metal_handles(renderer: &Renderer) -> Option<MetalHandles> {
    // SAFETY: both handles are only read, never destroyed or mutated, and the guards are
    // dropped before this returns. The addresses stay valid because the renderer owns the
    // device and queue for its whole lifetime — the `_instance`/`_adapter` fields on
    // `Renderer` exist to guarantee exactly that.
    let device = unsafe { renderer.wgpu_device().as_hal::<wgpu::hal::api::Metal>() }?;
    let queue = unsafe { renderer.wgpu_queue().as_hal::<wgpu::hal::api::Metal>() }?;

    let device_ptr: *const _ = &**device.raw_device();
    let queue_ptr: *const _ = queue.as_raw();

    Some(MetalHandles {
        device: device_ptr as *const c_void as u64,
        queue: queue_ptr as *const c_void as u64,
    })
}
