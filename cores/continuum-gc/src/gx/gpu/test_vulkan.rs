//! A Vulkan device on whatever this machine has (SwiftShader on the Linux test machine:
//! VK_ICD_FILENAMES pointing at its vk_swiftshader_icd.json, LD_LIBRARY_PATH at its
//! libvulkan.so.1), for the GPU drawer's tests and tools. Included by path; not part of the core.

use ash::vk;
use continuum_gc::gx::gpu::Handles;

pub struct TestVulkan {
    _entry: ash::Entry,
    instance: ash::Instance,
    device: ash::Device,
    pub handles: Handles,
}

impl Drop for TestVulkan {
    fn drop(&mut self) {
        // SAFETY: everything made on them is gone (the drawers are dropped first).
        unsafe {
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

pub fn vulkan() -> Option<TestVulkan> {
    // SAFETY: loading the system's Vulkan loader.
    let entry = unsafe { ash::Entry::load() }.ok()?;
    let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
    // SAFETY: plain create infos.
    unsafe {
        let instance = entry.create_instance(&vk::InstanceCreateInfo::default().application_info(&app), None).ok()?;
        let physical = *instance.enumerate_physical_devices().ok()?.first()?;
        let family = instance
            .get_physical_device_queue_family_properties(physical)
            .iter()
            .position(|q| q.queue_flags.contains(vk::QueueFlags::COMPUTE))? as u32;
        let prio = [1.0];
        let queues = [vk::DeviceQueueCreateInfo::default().queue_family_index(family).queue_priorities(&prio)];
        let device = instance.create_device(physical, &vk::DeviceCreateInfo::default().queue_create_infos(&queues), None).ok()?;
        let queue = device.get_device_queue(family, 0);
        let handles = Handles {
            get_instance_proc_addr: entry.static_fn().get_instance_proc_addr,
            instance: instance.handle(),
            physical,
            device: device.handle(),
            queue,
            queue_family: family,
            queue_lock: None,
        };
        Some(TestVulkan { _entry: entry, instance, device, handles })
    }
}
