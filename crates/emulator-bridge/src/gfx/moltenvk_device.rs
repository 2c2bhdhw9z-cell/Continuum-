//! Long-lived MoltenVK `VkInstance` / `VkDevice` / `VkQueue` for step 4 `SET_HW_RENDER`.
//!
//! The triangle proof in [`super::moltenvk`] creates a one-shot device and holds it for the
//! exported texture. Beetle PSX HW needs the same objects for the life of a session: the
//! frontend installs them through [`super::vulkan_hw::install_vulkan_handles`] and calls
//! the core's `context_reset`. Patterns match `moltenvk.rs`: filter instance/device
//! extensions; never force `VK_KHR_portability_enumeration`.

use super::vulkan_hw;

/// Handles handed to libretro's `retro_hw_render_interface_vulkan`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HwVulkanHandles {
    pub instance: u64,
    pub gpu: u64,
    pub device: u64,
    pub queue: u64,
    pub queue_index: u32,
}

/// Outcome of preparing (or reusing) the shared MoltenVK device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareReport {
    pub summary: String,
    pub handles: Option<HwVulkanHandles>,
    /// `install_vulkan_handles` ran because `SET_HW_RENDER` was already accepted.
    pub installed_into_hw: bool,
}

/// Prepares the shared MoltenVK device for hardware-rendered cores.
///
/// `frameworks_dir` is the bundle Frameworks directory. `metal_device` is wgpu's
/// `MTLDevice` pointer (must be non-zero on Apple). Safe to call more than once: a second
/// call reuses the existing device when handles are already live.
pub fn prepare_hw_context(frameworks_dir: &str, metal_device: u64) -> PrepareReport {
    #[cfg(target_vendor = "apple")]
    {
        apple::prepare(frameworks_dir, metal_device)
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        let _ = (frameworks_dir, metal_device);
        PrepareReport {
            summary: "Vulkan HW device: NOT AVAILABLE on this host (needs iOS MoltenVK + Metal)"
                .into(),
            handles: None,
            installed_into_hw: false,
        }
    }
}

/// Current shared handles, if prepare succeeded on this process.
pub fn hw_handles() -> Option<HwVulkanHandles> {
    #[cfg(target_vendor = "apple")]
    {
        apple::handles()
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        None
    }
}

/// If `SET_HW_RENDER` was accepted and a shared device exists, install handles and call
/// `context_reset`. Returns true when install ran.
pub fn try_install_into_vulkan_hw() -> bool {
    let Some(h) = hw_handles() else {
        return false;
    };
    if !vulkan_hw::status().set_hw_render_accepted {
        return false;
    }
    #[cfg(target_vendor = "apple")]
    {
        apple::install(h);
        true
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        let _ = h;
        false
    }
}

/// Runs after `retro_load_game` returned for a core whose `context_reset` was deferred (see
/// [`vulkan_hw::set_defer_reset`]). When the core sent a negotiation `create_device`, the core
/// makes its own `VkDevice` on the host's instance and physical device, and those handles are the
/// ones installed. Otherwise the shared device is installed as usual. `Err` means the core has no
/// usable graphics context and must not be run: the caller unloads it and shows the reason.
pub fn finish_deferred_install() -> Result<(), String> {
    if !vulkan_hw::status().set_hw_render_accepted || vulkan_hw::interface_ready() {
        return Ok(());
    }
    if hw_handles().is_none() {
        return Err("the Vulkan device is not ready (MoltenVK did not start)".into());
    }
    #[cfg(target_vendor = "apple")]
    {
        if let Some(create_device) = vulkan_hw::negotiation_create_device() {
            return apple::install_negotiated(create_device);
        }
        if try_install_into_vulkan_hw() {
            Ok(())
        } else {
            Err("the Vulkan device could not be handed to the core".into())
        }
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        Err("Vulkan is not available on this host".into())
    }
}

/// Exports a `VkImage` from a pending `set_image` into the compositor via metal_objects.
///
/// Returns `Ok(true)` when a texture was adopted, `Ok(false)` when there was nothing to do,
/// and `Err` when export was attempted and failed.
pub fn adopt_pending_frame(
    renderer: &mut crate::gfx::Renderer,
    image: u64,
    width: u32,
    height: u32,
    format: u32,
) -> Result<bool, String> {
    if image == 0 || width == 0 || height == 0 {
        return Ok(false);
    }
    #[cfg(target_vendor = "apple")]
    {
        apple::adopt_image(renderer, image, width, height, format)
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        let _ = (renderer, format);
        Ok(false)
    }
}

/// `vkQueueWaitIdle` on the queue a negotiated core (PPSSPP) created. No-op otherwise.
pub fn wait_core_queue_idle() {
    #[cfg(target_vendor = "apple")]
    apple::wait_core_queue_idle();
}

#[cfg(target_vendor = "apple")]
mod apple {
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;
    use std::sync::Mutex;

    use ash::khr::portability_enumeration;
    use ash::vk::{self, Handle};
    use ash::{Device, Entry, Instance};
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2_metal::{MTLTexture, MTLTextureType};

    use super::{HwVulkanHandles, PrepareReport};
    use crate::gfx::moltenvk::{FilteredDeviceExtensions, FilteredInstanceExtensions};
    use crate::gfx::vulkan_hw;
    use crate::gfx::Renderer;

    struct SharedDevice {
        _entry: Entry,
        instance: Instance,
        device: Device,
        phys: vk::PhysicalDevice,
        queue: vk::Queue,
        /// The graphics queue family `queue` came from; the core is told this index.
        queue_family: u32,
        metal_objects: ash::ext::metal_objects::Device,
    }

    // ash handles are process-local pointers; the Mutex serialises access.
    unsafe impl Send for SharedDevice {}

    static SHARED: Mutex<Option<SharedDevice>> = Mutex::new(None);

    /// A device the core made through the negotiation interface (PPSSPP). Its images are
    /// exported through this device, not the shared one. The frontend owns it, so it is
    /// destroyed when the next negotiated device replaces it, by which time the core that used
    /// it has been unloaded.
    struct CoreDevice {
        device: Device,
        queue: vk::Queue,
        metal_objects: ash::ext::metal_objects::Device,
    }
    unsafe impl Send for CoreDevice {}
    static CORE: Mutex<Option<CoreDevice>> = Mutex::new(None);

    pub(super) fn wait_core_queue_idle() {
        let guard = lock_core();
        if let Some(core) = guard.as_ref() {
            if core.queue != vk::Queue::null() {
                let _ = unsafe { core.device.queue_wait_idle(core.queue) };
            }
        }
    }

    fn lock_core() -> std::sync::MutexGuard<'static, Option<CoreDevice>> {
        match CORE.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }

    #[repr(C)]
    struct RetroVulkanContext {
        gpu: vk::PhysicalDevice,
        device: vk::Device,
        queue: vk::Queue,
        queue_family_index: u32,
        presentation_queue: vk::Queue,
        presentation_queue_family_index: u32,
    }

    type CreateDeviceFn = unsafe extern "C" fn(
        *mut RetroVulkanContext,
        vk::Instance,
        vk::PhysicalDevice,
        vk::SurfaceKHR,
        vk::PFN_vkGetInstanceProcAddr,
        *const *const c_char,
        u32,
        *const *const c_char,
        u32,
        *const vk::PhysicalDeviceFeatures,
    ) -> bool;

    pub(super) fn install_negotiated(create_device: *const std::ffi::c_void) -> Result<(), String> {
        let (instance_fn, instance_raw, phys, gipa, portability) = {
            let guard = lock();
            let ctx = guard.as_ref().ok_or_else(|| "no shared MoltenVK instance".to_string())?;
            let exts = unsafe { ctx.instance.enumerate_device_extension_properties(ctx.phys) }
                .map_err(|e| format!("enumerate device extensions: {e}"))?;
            let portability = exts.iter().any(|e| {
                let name = unsafe { CStr::from_ptr(e.extension_name.as_ptr() as *const c_char) };
                name == ash::khr::portability_subset::NAME
            });
            (
                ctx.instance.fp_v1_0().clone(),
                ctx.instance.handle(),
                ctx.phys,
                ctx._entry.static_fn().get_instance_proc_addr,
                portability,
            )
        };
        // The previous core's device: that core is gone, nothing uses it any more.
        if let Some(old) = lock_core().take() {
            unsafe {
                let _ = old.device.device_wait_idle();
                old.device.destroy_device(None);
            }
        }
        let mut required: Vec<*const c_char> = vec![vk::EXT_METAL_OBJECTS_NAME.as_ptr()];
        if portability {
            required.push(ash::khr::portability_subset::NAME.as_ptr());
        }
        let mut out = RetroVulkanContext {
            gpu: vk::PhysicalDevice::null(),
            device: vk::Device::null(),
            queue: vk::Queue::null(),
            queue_family_index: 0,
            presentation_queue: vk::Queue::null(),
            presentation_queue_family_index: 0,
        };
        let create: CreateDeviceFn = unsafe { std::mem::transmute(create_device) };
        let ok = unsafe {
            create(
                &mut out,
                instance_raw,
                phys,
                vk::SurfaceKHR::null(),
                gipa,
                required.as_ptr(),
                required.len() as u32,
                std::ptr::null(),
                0,
                std::ptr::null(),
            )
        };
        if !ok || out.device == vk::Device::null() || out.queue == vk::Queue::null() {
            return Err("the core could not create its Vulkan device".into());
        }
        let device = unsafe { Device::load(&instance_fn, out.device) };
        let metal_objects = {
            let guard = lock();
            let ctx = guard.as_ref().ok_or_else(|| "shared instance went away".to_string())?;
            ash::ext::metal_objects::Device::new(&ctx.instance, &device)
        };
        *lock_core() = Some(CoreDevice { device, queue: out.queue, metal_objects });
        let gpu = if out.gpu == vk::PhysicalDevice::null() { phys } else { out.gpu };
        install(HwVulkanHandles {
            instance: instance_raw.as_raw(),
            gpu: gpu.as_raw(),
            device: out.device.as_raw(),
            queue: out.queue.as_raw(),
            queue_index: out.queue_family_index,
        });
        Ok(())
    }

    fn lock() -> std::sync::MutexGuard<'static, Option<SharedDevice>> {
        match SHARED.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }

    pub(super) fn handles() -> Option<HwVulkanHandles> {
        let guard = lock();
        let ctx = guard.as_ref()?;
        Some(HwVulkanHandles {
            instance: ctx.instance.handle().as_raw(),
            gpu: ctx.phys.as_raw(),
            device: ctx.device.handle().as_raw(),
            queue: ctx.queue.as_raw(),
            queue_index: ctx.queue_family,
        })
    }

    pub(super) fn prepare(frameworks_dir: &str, metal_device: u64) -> PrepareReport {
        if metal_device == 0 {
            return PrepareReport {
                summary: "Vulkan HW device: no MTLDevice yet; attach Metal before preparing"
                    .into(),
                handles: None,
                installed_into_hw: false,
            };
        }

        {
            let guard = lock();
            if guard.is_some() {
                drop(guard);
                let h = handles();
                let installed = super::try_install_into_vulkan_hw();
                return PrepareReport {
                    summary: format!(
                        "Vulkan HW device: reused existing MoltenVK VkDevice (instance={:#x})",
                        h.map(|x| x.instance).unwrap_or(0)
                    ),
                    handles: h,
                    installed_into_hw: installed,
                };
            }
        }

        match create(frameworks_dir, metal_device) {
            Ok((ctx, shared_ok)) => {
                let handles = HwVulkanHandles {
                    instance: ctx.instance.handle().as_raw(),
                    gpu: ctx.phys.as_raw(),
                    device: ctx.device.handle().as_raw(),
                    queue: ctx.queue.as_raw(),
                    queue_index: ctx.queue_family,
                };
                *lock() = Some(ctx);
                let installed = {
                    // install needs SHARED populated
                    if vulkan_hw::status().set_hw_render_accepted {
                        install(handles);
                        true
                    } else {
                        false
                    }
                };
                let share_note = if shared_ok {
                    "shares wgpu MTLDevice"
                } else {
                    "MTLDevice pointer differed from wgpu (textures may not be shareable)"
                };
                PrepareReport {
                    summary: format!(
                        "Vulkan HW device: OK. MoltenVK VkInstance/VkDevice/VkQueue ready for \
                         SET_HW_RENDER ({share_note}; installed={installed})"
                    ),
                    handles: Some(handles),
                    installed_into_hw: installed,
                }
            }
            Err(reason) => PrepareReport {
                summary: format!("Vulkan HW device: FAILED ({reason})"),
                handles: None,
                installed_into_hw: false,
            },
        }
    }

    pub(super) fn install(h: HwVulkanHandles) {
        // A plain install means this core uses the shared device; images are exported from it.
        let shared_device = lock().as_ref().map(|c| c.device.handle().as_raw());
        let uses_shared = shared_device == Some(h.device);
        if let Some(old) = uses_shared.then(|| lock_core().take()).flatten() {
            unsafe {
                let _ = old.device.device_wait_idle();
                old.device.destroy_device(None);
            }
        }
        vulkan_hw::install_vulkan_handles(
            h.instance,
            h.gpu,
            h.device,
            h.queue,
            h.queue_index,
            Some(get_instance_proc_addr),
            Some(get_device_proc_addr),
        );
    }

    pub(super) fn adopt_image(
        renderer: &mut Renderer,
        image_raw: u64,
        width: u32,
        height: u32,
        format: u32,
    ) -> Result<bool, String> {
        // Read the image as what it is: R8G8B8A8 sampled as BGRA swaps red and blue.
        let wgpu_format = if format == vulkan_hw::VK_FORMAT_R8G8B8A8_UNORM {
            wgpu::TextureFormat::Rgba8Unorm
        } else {
            wgpu::TextureFormat::Bgra8Unorm
        };
        let guard = lock();
        let ctx = guard
            .as_ref()
            .ok_or_else(|| "no shared MoltenVK device; call prepare_vulkan_hw first".to_string())?;
        let image = vk::Image::from_raw(image_raw);
        let core_guard = lock_core();
        let (export_device, export_fn) = match core_guard.as_ref() {
            Some(core) => (core.device.handle(), core.metal_objects.fp().export_metal_objects_ext),
            None => (ctx.device.handle(), ctx.metal_objects.fp().export_metal_objects_ext),
        };

        let mut texture_info = vk::ExportMetalTextureInfoEXT::default()
            .image(image)
            .plane(vk::ImageAspectFlags::COLOR);
        let mut objects = vk::ExportMetalObjectsInfoEXT::default().push_next(&mut texture_info);
        unsafe {
            (export_fn)(export_device, &mut objects);
        }
        drop(core_guard);
        let mtl_texture = texture_info.mtl_texture;
        if mtl_texture.is_null() {
            return Err("vkExportMetalObjectsEXT returned a null MTLTexture for set_image".into());
        }

        let retained: Retained<ProtocolObject<dyn MTLTexture>> = unsafe {
            Retained::retain(mtl_texture as *mut ProtocolObject<dyn MTLTexture>)
        }
        .ok_or_else(|| "Retained::retain on set_image MTLTexture returned None".to_string())?;

        let hal_texture = unsafe {
            wgpu::hal::metal::Device::texture_from_raw(
                retained,
                wgpu_format,
                MTLTextureType::Type2D,
                1,
                1,
                wgpu::hal::CopyExtent {
                    width,
                    height,
                    depth: 1,
                },
                None,
            )
        };

        let descriptor = wgpu::TextureDescriptor {
            label: Some("beetle-hw-frame"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };

        let texture = unsafe {
            renderer.wgpu_device().create_texture_from_hal::<wgpu::hal::api::Metal>(
                hal_texture,
                &descriptor,
                wgpu::TextureUses::RESOURCE,
            )
        };
        renderer.adopt_frame_texture(texture, width, height);
        Ok(true)
    }

    fn create(frameworks_dir: &str, metal_device: u64) -> Result<(SharedDevice, bool), String> {
        let path = format!(
            "{}/MoltenVK.framework/MoltenVK",
            frameworks_dir.trim_end_matches('/')
        );
        let entry = unsafe { Entry::load_from(&path) }.map_err(|e| format!("load MoltenVK: {e}"))?;

        let app_name = CString::new("Continuum").unwrap();
        let eng_name = CString::new("emulator-bridge").unwrap();
        let app_info = vk::ApplicationInfo::default()
            .application_name(&app_name)
            .application_version(vk::make_api_version(0, 0, 1, 0))
            .engine_name(&eng_name)
            .engine_version(vk::make_api_version(0, 0, 1, 0))
            .api_version(vk::API_VERSION_1_1);

        let available_instance = unsafe { entry.enumerate_instance_extension_properties(None) }
            .map_err(|e| format!("enumerate instance extensions: {e}"))?;
        let available_instance_names: Vec<&str> = available_instance
            .iter()
            .map(|e| {
                unsafe { CStr::from_ptr(e.extension_name.as_ptr() as *const c_char) }
                    .to_str()
                    .unwrap_or("")
            })
            .collect();
        let instance_plan =
            FilteredInstanceExtensions::from_available(available_instance_names.iter().copied());

        let portability = portability_enumeration::NAME;
        let mut instance_exts: Vec<*const c_char> = Vec::new();
        let mut instance_flags = vk::InstanceCreateFlags::empty();
        if instance_plan.portability_enumeration {
            instance_exts.push(portability.as_ptr());
            instance_flags |= vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR;
        }
        let create_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_extension_names(&instance_exts)
            .flags(instance_flags);

        let instance = unsafe { entry.create_instance(&create_info, None) }
            .map_err(|e| format!("vkCreateInstance: {e}"))?;

        // Everything between the instance and the device can fail; the instance is destroyed on
        // the way out so a failed prepare does not leak it.
        let picked = (|| -> Result<_, String> {
            let phys = unsafe { instance.enumerate_physical_devices() }
                .map_err(|e| format!("enumerate devices: {e}"))?
                .into_iter()
                .next()
                .ok_or_else(|| "MoltenVK reported no physical devices".to_string())?;

            let queue_family = unsafe { instance.get_physical_device_queue_family_properties(phys) }
                .into_iter()
                .enumerate()
                .find(|(_, p)| p.queue_flags.contains(vk::QueueFlags::GRAPHICS))
                .map(|(i, _)| i as u32)
                .ok_or_else(|| "no graphics queue family".to_string())?;

            let queue_priorities = [1.0f32];
            let queue_info = vk::DeviceQueueCreateInfo::default()
                .queue_family_index(queue_family)
                .queue_priorities(&queue_priorities);

            let available_device = unsafe { instance.enumerate_device_extension_properties(phys) }
                .map_err(|e| format!("enumerate device extensions: {e}"))?;
            let available_device_names: Vec<&str> = available_device
                .iter()
                .map(|e| {
                    unsafe { CStr::from_ptr(e.extension_name.as_ptr() as *const c_char) }
                        .to_str()
                        .unwrap_or("")
                })
                .collect();
            let device_plan =
                FilteredDeviceExtensions::from_available(available_device_names.iter().copied())?;

            let metal_objects_name = vk::EXT_METAL_OBJECTS_NAME;
            let portability_subset = ash::khr::portability_subset::NAME;
            let mut device_exts: Vec<*const c_char> = Vec::new();
            device_exts.push(metal_objects_name.as_ptr());
            if device_plan.portability_subset {
                device_exts.push(portability_subset.as_ptr());
            }
            let device_info = vk::DeviceCreateInfo::default()
                .queue_create_infos(std::slice::from_ref(&queue_info))
                .enabled_extension_names(&device_exts);
            let device = unsafe { instance.create_device(phys, &device_info, None) }
                .map_err(|e| format!("vkCreateDevice: {e}"))?;
            Ok((phys, queue_family, device))
        })();
        let (phys, queue_family, device) = match picked {
            Ok(v) => v,
            Err(reason) => {
                unsafe { instance.destroy_instance(None) };
                return Err(reason);
            }
        };
        let queue = unsafe { device.get_device_queue(queue_family, 0) };
        let metal_objects = ash::ext::metal_objects::Device::new(&instance, &device);

        let mut device_info_export = vk::ExportMetalDeviceInfoEXT::default();
        let mut objects_info =
            vk::ExportMetalObjectsInfoEXT::default().push_next(&mut device_info_export);
        unsafe {
            (metal_objects.fp().export_metal_objects_ext)(device.handle(), &mut objects_info);
        }
        let molten_device = device_info_export.mtl_device as u64;
        let shared_ok = molten_device != 0 && molten_device == metal_device;

        Ok((
            SharedDevice {
                _entry: entry,
                instance,
                device,
                phys,
                queue,
                queue_family,
                metal_objects,
            },
            shared_ok,
        ))
    }

    unsafe extern "C" fn get_instance_proc_addr(
        instance: u64,
        name: *const c_char,
    ) -> *const std::ffi::c_void {
        let guard = lock();
        let Some(ctx) = guard.as_ref() else {
            return std::ptr::null();
        };
        let inst = if instance == 0 {
            vk::Instance::null()
        } else {
            vk::Instance::from_raw(instance)
        };
        let f = unsafe { ctx._entry.get_instance_proc_addr(inst, name) };
        match f {
            Some(func) => func as *const std::ffi::c_void,
            None => std::ptr::null(),
        }
    }

    unsafe extern "C" fn get_device_proc_addr(
        device: u64,
        name: *const c_char,
    ) -> *const std::ffi::c_void {
        let guard = lock();
        let Some(ctx) = guard.as_ref() else {
            return std::ptr::null();
        };
        if device == 0 {
            return std::ptr::null();
        }
        let dev = vk::Device::from_raw(device);
        // ash exposes vkGetDeviceProcAddr on Instance, not Device (Vulkan 1.0).
        let f = unsafe { ctx.instance.get_device_proc_addr(dev, name) };
        match f {
            Some(func) => func as *const std::ffi::c_void,
            None => std::ptr::null(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_apple_prepare_is_readable() {
        let report = prepare_hw_context("/no/frameworks", 0);
        assert!(report.handles.is_none());
        assert!(report.summary.contains("NOT AVAILABLE") || report.summary.contains("no MTLDevice") || report.summary.contains("FAILED") || report.summary.contains("OK") || report.summary.contains("reused"));
        // On Linux CI this is the refuse path.
        #[cfg(not(target_vendor = "apple"))]
        {
            assert!(report.summary.contains("NOT AVAILABLE"));
            assert!(!report.installed_into_hw);
        }
    }

    #[test]
    fn try_install_without_prepare_is_false() {
        // Fresh process in tests may still have leftover state from vulkan_hw tests;
        // without handles, install must not claim success.
        if hw_handles().is_none() {
            assert!(!try_install_into_vulkan_hw());
        }
    }
}
