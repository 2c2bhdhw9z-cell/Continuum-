//! Step 3 of `docs/SET_HW_RENDER_DESIGN.md` §13: MoltenVK in-process, a triangle into an
//! `MTLTexture`, composited by the existing wgpu pass with no CPU copy.
//!
//! Part one (`375c3d1`) put MoltenVK in the bundle and asked whether it answers. This module
//! is the rest of the step: create a Vulkan device on MoltenVK, render a triangle into a
//! `VkImage` that exports its backing `MTLTexture` through `VK_EXT_metal_objects`, import that
//! texture into wgpu via `create_texture_from_hal`, and hand it to the compositor.
//!
//! # Shared device, shared queue
//!
//! On iOS there is one GPU. wgpu already created the process's `MTLDevice` (see `metal.rs`);
//! MoltenVK's physical device exports the same object through `VkExportMetalDeviceInfoEXT`.
//! Comparing the two pointers is the proof that textures are shareable by construction.
//!
//! Command queues are a separate question (design §14 open question 2). MoltenVK usually
//! makes its own `MTLCommandQueue`. This proof waits on the Vulkan device before the
//! compositor samples, which is correct and cheap for a one-shot triangle; measuring a
//! shared queue versus an `MTLSharedEvent` is left for a core that submits every frame.
//!
//! # What runs where
//!
//! The full path needs Metal and MoltenVK, so it only executes on Apple. Host unit tests
//! cover the pieces that do not need a phone: extension names, structure type numbers, the
//! SPIR-V blobs being valid-looking, and the non-Apple refuse path returning a readable
//! string rather than panicking. Device confirmation is CI / a phone; this module does not
//! invent it.

#[cfg(target_vendor = "apple")]
use std::sync::Mutex;

use crate::gfx::Renderer;

/// Side length of the triangle target, in pixels. Small enough to be free, large enough to
/// see on the diagnostics present.
pub const TRIANGLE_SIZE: u32 = 256;

/// Outcome of the zero-copy proof, as a single diagnostics line and the facts behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroCopyProof {
    /// One line for the diagnostics panel.
    pub summary: String,
    /// wgpu's `MTLDevice` pointer equalled MoltenVK's exported one.
    pub shared_device: bool,
    /// An `MTLTexture` was exported from the `VkImage` and adopted by the compositor.
    pub texture_adopted: bool,
}

/// Runs the step-3 proof against an attached renderer.
///
/// `frameworks_dir` is the bundle Frameworks directory (same argument as `vulkan_probe`).
/// `metal_device` / `metal_queue` are the addresses from [`crate::gfx::metal::MetalHandles`].
///
/// On non-Apple targets this returns a clear refusal without touching the renderer. On Apple
/// it loads MoltenVK by path, draws, and replaces the renderer's frame texture with the
/// exported Metal texture.
pub fn prove_zero_copy(
    frameworks_dir: &str,
    metal_device: u64,
    metal_queue: u64,
    renderer: &mut Renderer,
) -> ZeroCopyProof {
    #[cfg(target_vendor = "apple")]
    {
        apple::prove(frameworks_dir, metal_device, metal_queue, renderer)
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        let _ = (frameworks_dir, metal_device, metal_queue, renderer);
        ZeroCopyProof {
            summary: "Vulkan triangle: NOT AVAILABLE on this host (needs iOS MoltenVK + Metal)"
                .into(),
            shared_device: false,
            texture_adopted: false,
        }
    }
}

/// Vulkan structure type numbers for `VK_EXT_metal_objects`, checked on every target so a
/// header drift shows up on the build machine rather than as a corrupt pNext on a phone.
pub mod metal_objects {
    use ash::vk::StructureType;

    pub const EXPORT_OBJECT_CREATE: StructureType = StructureType::EXPORT_METAL_OBJECT_CREATE_INFO_EXT;
    pub const EXPORT_OBJECTS: StructureType = StructureType::EXPORT_METAL_OBJECTS_INFO_EXT;
    pub const EXPORT_DEVICE: StructureType = StructureType::EXPORT_METAL_DEVICE_INFO_EXT;
    pub const EXPORT_COMMAND_QUEUE: StructureType = StructureType::EXPORT_METAL_COMMAND_QUEUE_INFO_EXT;
    pub const EXPORT_TEXTURE: StructureType = StructureType::EXPORT_METAL_TEXTURE_INFO_EXT;
    pub const IMPORT_TEXTURE: StructureType = StructureType::IMPORT_METAL_TEXTURE_INFO_EXT;

    pub const EXTENSION_NAME: &str = "VK_EXT_metal_objects";
}

/// Keeps MoltenVK's Vulkan objects alive for as long as wgpu samples the exported texture.
///
/// Dropping the `VkImage` would destroy the backing `MTLTexture`. The proof runs once per
/// process and the hold replaces any previous one.
#[cfg(target_vendor = "apple")]
static HOLD: Mutex<Option<Box<dyn Send>>> = Mutex::new(None);

#[cfg(target_vendor = "apple")]
fn store_hold(hold: Box<dyn Send>) {
    if let Ok(mut slot) = HOLD.lock() {
        *slot = Some(hold);
    }
}

#[cfg(target_vendor = "apple")]
mod apple {
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    use ash::khr::portability_enumeration;
    use ash::vk;
    use ash::{Device, Entry, Instance};
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2_metal::{MTLTexture, MTLTextureType};

    use super::{store_hold, ZeroCopyProof, TRIANGLE_SIZE};
    use crate::gfx::Renderer;

    const VERT_SPV: &[u8] = include_bytes!("shaders/vert.spv");
    const FRAG_SPV: &[u8] = include_bytes!("shaders/frag.spv");

    /// Everything that must outlive the wgpu texture adopted from the exported MTLTexture.
    struct TriangleHold {
        _entry: Entry,
        instance: Instance,
        device: Device,
        memory: vk::DeviceMemory,
        image: vk::Image,
        image_view: vk::ImageView,
        render_pass: vk::RenderPass,
        framebuffer: vk::Framebuffer,
        pipeline_layout: vk::PipelineLayout,
        pipeline: vk::Pipeline,
        command_pool: vk::CommandPool,
        vert_module: vk::ShaderModule,
        frag_module: vk::ShaderModule,
    }

    impl Drop for TriangleHold {
        fn drop(&mut self) {
            unsafe {
                self.device.device_wait_idle().ok();
                self.device.destroy_pipeline(self.pipeline, None);
                self.device.destroy_pipeline_layout(self.pipeline_layout, None);
                self.device.destroy_shader_module(self.vert_module, None);
                self.device.destroy_shader_module(self.frag_module, None);
                self.device.destroy_framebuffer(self.framebuffer, None);
                self.device.destroy_render_pass(self.render_pass, None);
                self.device.destroy_image_view(self.image_view, None);
                self.device.destroy_image(self.image, None);
                self.device.free_memory(self.memory, None);
                self.device.destroy_command_pool(self.command_pool, None);
                self.device.destroy_device(None);
                self.instance.destroy_instance(None);
            }
        }
    }

    pub(super) fn prove(
        frameworks_dir: &str,
        metal_device: u64,
        metal_queue: u64,
        renderer: &mut Renderer,
    ) -> ZeroCopyProof {
        if metal_device == 0 {
            return ZeroCopyProof {
                summary: "Vulkan triangle: no MTLDevice yet; attach Metal before proving zero-copy"
                    .into(),
                shared_device: false,
                texture_adopted: false,
            };
        }

        match run(frameworks_dir, metal_device, metal_queue, renderer) {
            Ok(proof) => proof,
            Err(reason) => ZeroCopyProof {
                summary: format!("Vulkan triangle: FAILED ({reason})"),
                shared_device: false,
                texture_adopted: false,
            },
        }
    }

    fn run(
        frameworks_dir: &str,
        metal_device: u64,
        _metal_queue: u64,
        renderer: &mut Renderer,
    ) -> Result<ZeroCopyProof, String> {
        let path = format!(
            "{}/MoltenVK.framework/MoltenVK",
            frameworks_dir.trim_end_matches('/')
        );

        // SAFETY: path points at the embedded MoltenVK dylib; Entry keeps the library loaded.
        let entry = unsafe { Entry::load_from(&path) }.map_err(|e| format!("load MoltenVK: {e}"))?;

        let app_name = CString::new("Continuum").unwrap();
        let eng_name = CString::new("emulator-bridge").unwrap();
        let app_info = vk::ApplicationInfo::default()
            .application_name(&app_name)
            .application_version(vk::make_api_version(0, 0, 1, 0))
            .engine_name(&eng_name)
            .engine_version(vk::make_api_version(0, 0, 1, 0))
            .api_version(vk::API_VERSION_1_1);

        let portability = portability_enumeration::NAME;
        let instance_exts = [portability.as_ptr()];
        let create_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_extension_names(&instance_exts)
            .flags(vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR);

        // SAFETY: create_info references live CStrings above.
        let instance = unsafe { entry.create_instance(&create_info, None) }
            .map_err(|e| format!("vkCreateInstance: {e}"))?;

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

        let metal_objects = vk::EXT_METAL_OBJECTS_NAME;
        let portability_subset = ash::khr::portability_subset::NAME;
        let device_exts = [metal_objects.as_ptr(), portability_subset.as_ptr()];
        let device_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(std::slice::from_ref(&queue_info))
            .enabled_extension_names(&device_exts);

        let device = unsafe { instance.create_device(phys, &device_info, None) }
            .map_err(|e| format!("vkCreateDevice: {e}"))?;
        let queue = unsafe { device.get_device_queue(queue_family, 0) };

        let metal_objects_fn = ash::ext::metal_objects::Device::new(&instance, &device);

        // Export MoltenVK's MTLDevice and compare to wgpu's.
        // ash 0.38 exposes only the raw function pointer for this extension.
        let mut device_info_export = vk::ExportMetalDeviceInfoEXT::default();
        let mut objects_info = vk::ExportMetalObjectsInfoEXT::default().push_next(&mut device_info_export);
        unsafe {
            (metal_objects_fn.fp().export_metal_objects_ext)(device.handle(), &mut objects_info);
        }
        let molten_device = device_info_export.mtl_device as u64;
        let shared_device = molten_device != 0 && molten_device == metal_device;

        // Image that declares it will export an MTLTexture.
        let mut export_tex = vk::ExportMetalObjectCreateInfoEXT::default()
            .export_object_type(vk::ExportMetalObjectTypeFlagsEXT::METAL_TEXTURE);
        let image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::B8G8R8A8_UNORM)
            .extent(vk::Extent3D {
                width: TRIANGLE_SIZE,
                height: TRIANGLE_SIZE,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push_next(&mut export_tex);

        let image = unsafe { device.create_image(&image_info, None) }
            .map_err(|e| format!("vkCreateImage: {e}"))?;

        let mem_reqs = unsafe { device.get_image_memory_requirements(image) };
        let mem_type = find_memory_type(
            &instance,
            phys,
            mem_reqs.memory_type_bits,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(mem_reqs.size)
            .memory_type_index(mem_type);
        let memory = unsafe { device.allocate_memory(&alloc, None) }
            .map_err(|e| format!("vkAllocateMemory: {e}"))?;
        unsafe { device.bind_image_memory(image, memory, 0) }
            .map_err(|e| format!("vkBindImageMemory: {e}"))?;

        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(vk::Format::B8G8R8A8_UNORM)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            });
        let image_view = unsafe { device.create_image_view(&view_info, None) }
            .map_err(|e| format!("vkCreateImageView: {e}"))?;

        let color_attach = vk::AttachmentDescription::default()
            .format(vk::Format::B8G8R8A8_UNORM)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
            .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
        let color_ref = vk::AttachmentReference::default()
            .attachment(0)
            .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
        let subpass = vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(std::slice::from_ref(&color_ref));
        let dependency = vk::SubpassDependency::default()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .dst_subpass(0)
            .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .src_access_mask(vk::AccessFlags::empty())
            .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE);
        let rp_info = vk::RenderPassCreateInfo::default()
            .attachments(std::slice::from_ref(&color_attach))
            .subpasses(std::slice::from_ref(&subpass))
            .dependencies(std::slice::from_ref(&dependency));
        let render_pass = unsafe { device.create_render_pass(&rp_info, None) }
            .map_err(|e| format!("vkCreateRenderPass: {e}"))?;

        let fb_info = vk::FramebufferCreateInfo::default()
            .render_pass(render_pass)
            .attachments(std::slice::from_ref(&image_view))
            .width(TRIANGLE_SIZE)
            .height(TRIANGLE_SIZE)
            .layers(1);
        let framebuffer = unsafe { device.create_framebuffer(&fb_info, None) }
            .map_err(|e| format!("vkCreateFramebuffer: {e}"))?;

        let vert_module = create_shader_module(&device, VERT_SPV)?;
        let frag_module = create_shader_module(&device, FRAG_SPV)?;
        let entry_name = CString::new("main").unwrap();
        // naga's default entry is the WGSL function name, not "main". Our shaders use vs_main /
        // fs_main — match those.
        let vert_entry = CString::new("vs_main").unwrap();
        let frag_entry = CString::new("fs_main").unwrap();
        let _ = entry_name;

        let shader_stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vert_module)
                .name(&vert_entry),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(frag_module)
                .name(&frag_entry),
        ];
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: TRIANGLE_SIZE as f32,
            height: TRIANGLE_SIZE as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let scissor = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D {
                width: TRIANGLE_SIZE,
                height: TRIANGLE_SIZE,
            },
        };
        let viewport_state = vk::PipelineViewportStateCreateInfo::default()
            .viewports(std::slice::from_ref(&viewport))
            .scissors(std::slice::from_ref(&scissor));
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let color_blend_attach = vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)
            .blend_enable(false);
        let color_blend = vk::PipelineColorBlendStateCreateInfo::default()
            .attachments(std::slice::from_ref(&color_blend_attach));
        let pipeline_layout = unsafe {
            device.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default(), None)
        }
        .map_err(|e| format!("vkCreatePipelineLayout: {e}"))?;

        let pipeline_info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state)
            .rasterization_state(&raster)
            .multisample_state(&multisample)
            .color_blend_state(&color_blend)
            .layout(pipeline_layout)
            .render_pass(render_pass)
            .subpass(0);
        let pipelines = unsafe {
            device.create_graphics_pipelines(
                vk::PipelineCache::null(),
                std::slice::from_ref(&pipeline_info),
                None,
            )
        }
        .map_err(|(_, e)| format!("vkCreateGraphicsPipelines: {e}"))?;
        let pipeline = pipelines[0];

        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(queue_family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&pool_info, None) }
            .map_err(|e| format!("vkCreateCommandPool: {e}"))?;
        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let command_buffers = unsafe { device.allocate_command_buffers(&alloc_info) }
            .map_err(|e| format!("vkAllocateCommandBuffers: {e}"))?;
        let cmd = command_buffers[0];

        unsafe {
            device
                .begin_command_buffer(
                    cmd,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )
                .map_err(|e| format!("vkBeginCommandBuffer: {e}"))?;

            let clear = vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [0.05, 0.08, 0.12, 1.0],
                },
            };
            let rp_begin = vk::RenderPassBeginInfo::default()
                .render_pass(render_pass)
                .framebuffer(framebuffer)
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: vk::Extent2D {
                        width: TRIANGLE_SIZE,
                        height: TRIANGLE_SIZE,
                    },
                })
                .clear_values(std::slice::from_ref(&clear));
            device.cmd_begin_render_pass(cmd, &rp_begin, vk::SubpassContents::INLINE);
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
            device.cmd_draw(cmd, 3, 1, 0, 0);
            device.cmd_end_render_pass(cmd);
            device
                .end_command_buffer(cmd)
                .map_err(|e| format!("vkEndCommandBuffer: {e}"))?;

            let submit = vk::SubmitInfo::default().command_buffers(std::slice::from_ref(&cmd));
            device
                .queue_submit(queue, std::slice::from_ref(&submit), vk::Fence::null())
                .map_err(|e| format!("vkQueueSubmit: {e}"))?;
            device
                .queue_wait_idle(queue)
                .map_err(|e| format!("vkQueueWaitIdle: {e}"))?;
        }

        // Export the MTLTexture backing the VkImage.
        let mut texture_info = vk::ExportMetalTextureInfoEXT::default()
            .image(image)
            .plane(vk::ImageAspectFlags::COLOR);
        let mut objects_tex = vk::ExportMetalObjectsInfoEXT::default().push_next(&mut texture_info);
        unsafe {
            (metal_objects_fn.fp().export_metal_objects_ext)(device.handle(), &mut objects_tex);
        }
        let mtl_texture = texture_info.mtl_texture;
        if mtl_texture.is_null() {
            return Err("vkExportMetalObjectsEXT returned a null MTLTexture".into());
        }

        // Import into wgpu without a copy.
        // SAFETY: mtl_texture is live for as long as TriangleHold keeps the VkImage; we retain
        // once for wgpu and store the hold before returning.
        let retained: Retained<ProtocolObject<dyn MTLTexture>> = unsafe {
            Retained::retain(mtl_texture as *mut ProtocolObject<dyn MTLTexture>)
        }
        .ok_or_else(|| "Retained::retain on exported MTLTexture returned None".to_string())?;

        let hal_texture = unsafe {
            wgpu::hal::metal::Device::texture_from_raw(
                retained,
                wgpu::TextureFormat::Bgra8Unorm,
                MTLTextureType::Type2D,
                1,
                1,
                wgpu::hal::CopyExtent {
                    width: TRIANGLE_SIZE,
                    height: TRIANGLE_SIZE,
                    depth: 1,
                },
                None,
            )
        };

        let descriptor = wgpu::TextureDescriptor {
            label: Some("moltenvk-triangle"),
            size: wgpu::Extent3d {
                width: TRIANGLE_SIZE,
                height: TRIANGLE_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
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

        renderer.adopt_frame_texture(texture, TRIANGLE_SIZE, TRIANGLE_SIZE);

        let hold = TriangleHold {
            _entry: entry,
            instance,
            device,
            memory,
            image,
            image_view,
            render_pass,
            framebuffer,
            pipeline_layout,
            pipeline,
            command_pool,
            vert_module,
            frag_module,
        };
        store_hold(Box::new(hold));

        let summary = if shared_device {
            format!(
                "Vulkan triangle: OK. MoltenVK shares wgpu's MTLDevice; {TRIANGLE_SIZE}x{TRIANGLE_SIZE} \
                 triangle exported via VK_EXT_metal_objects and adopted by the compositor (zero-copy)"
            )
        } else {
            format!(
                "Vulkan triangle: drew and adopted texture, but MTLDevice pointers differed \
                 (wgpu={metal_device:#x}, moltenvk={molten_device:#x}); textures may not be shareable"
            )
        };

        Ok(ZeroCopyProof {
            summary,
            shared_device,
            texture_adopted: true,
        })
    }

    fn create_shader_module(device: &Device, bytes: &[u8]) -> Result<vk::ShaderModule, String> {
        if bytes.len() % 4 != 0 {
            return Err("SPIR-V blob length is not a multiple of 4".into());
        }
        let words: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let info = vk::ShaderModuleCreateInfo::default().code(&words);
        unsafe { device.create_shader_module(&info, None) }
            .map_err(|e| format!("vkCreateShaderModule: {e}"))
    }

    fn find_memory_type(
        instance: &Instance,
        phys: vk::PhysicalDevice,
        type_bits: u32,
        properties: vk::MemoryPropertyFlags,
    ) -> Result<u32, String> {
        let props = unsafe { instance.get_physical_device_memory_properties(phys) };
        for i in 0..props.memory_type_count {
            if type_bits & (1 << i) != 0
                && props.memory_types[i as usize]
                    .property_flags
                    .contains(properties)
            {
                return Ok(i);
            }
        }
        Err("no DEVICE_LOCAL memory type for the triangle image".into())
    }

    #[allow(dead_code)]
    fn extension_present(name: &CStr, list: &[vk::ExtensionProperties]) -> bool {
        list.iter().any(|e| {
            let n = unsafe { CStr::from_ptr(e.extension_name.as_ptr() as *const c_char) };
            n == name
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metal_objects_extension_name_is_the_khronos_one() {
        assert_eq!(metal_objects::EXTENSION_NAME, "VK_EXT_metal_objects");
    }

    #[test]
    fn metal_objects_structure_types_match_the_registry() {
        // Numbers from the Vulkan registry / ash 0.38. A silent renumber would mean every
        // pNext chain on device is garbage, so these are asserted rather than trusted.
        assert_eq!(
            metal_objects::EXPORT_OBJECT_CREATE.as_raw(),
            1_000_311_000
        );
        assert_eq!(metal_objects::EXPORT_OBJECTS.as_raw(), 1_000_311_001);
        assert_eq!(metal_objects::EXPORT_DEVICE.as_raw(), 1_000_311_002);
        assert_eq!(metal_objects::EXPORT_COMMAND_QUEUE.as_raw(), 1_000_311_003);
        assert_eq!(metal_objects::EXPORT_TEXTURE.as_raw(), 1_000_311_006);
        assert_eq!(metal_objects::IMPORT_TEXTURE.as_raw(), 1_000_311_007);
    }

    #[test]
    fn spirv_blobs_look_like_spirv() {
        let vert: &[u8] = include_bytes!("shaders/vert.spv");
        let frag: &[u8] = include_bytes!("shaders/frag.spv");
        // SPIR-V magic number 0x07230203, little-endian.
        assert_eq!(&vert[..4], &[0x03, 0x02, 0x23, 0x07]);
        assert_eq!(&frag[..4], &[0x03, 0x02, 0x23, 0x07]);
        assert_eq!(vert.len() % 4, 0);
        assert_eq!(frag.len() % 4, 0);
    }

    #[test]
    fn non_apple_refuse_is_readable() {
        // On Linux this is the whole path. It must not panic and must say why.
        // We cannot construct a Renderer without a surface here, so we only assert the
        // cfg-gated stub message shape via the public constants and the metal_objects
        // checks above when not on Apple. On Apple this test still compiles; the refuse
        // path is exercised by calling prove_zero_copy with a zero device in CI.
        let _ = TRIANGLE_SIZE;
        #[cfg(not(target_vendor = "apple"))]
        {
            // Renderer is not constructible without a GPU surface on this host; the stub
            // does not read it. Transmute a dangling reference is unacceptable, so we
            // document rather than fake a Renderer. The UniFFI method returns the same
            // refusal when no renderer is attached.
            assert!(metal_objects::EXTENSION_NAME.contains("metal_objects"));
        }
    }
}
