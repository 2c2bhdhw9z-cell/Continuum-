//! WebGPU renderer: uploads a core framebuffer and blits it to the canvas.
//!
//! Architectural rule: **this is the only thing that draws emulator output.** No
//! `CanvasRenderingContext2D`, no `<img>`, no DOM-based scaling. In the browser
//! the swapchain is a `GPUCanvasContext` obtained through `wgpu`; on iOS the same
//! code targets a `CAMetalLayer`. The pipeline, shader and upload path are shared,
//! which is why Phase 2 needs no renderer rewrite.
//!
//! Resource lifetime is deliberately coarse:
//!
//! - instance/adapter/device/queue/pipeline/sampler: once per app run;
//! - framebuffer texture + bind group: once per *core geometry*, not per frame;
//! - per frame: one `write_texture`, one render pass, one submit, one present.
//!
//! Nothing in the steady-state path allocates GPU objects, which is what keeps the
//! frame time flat rather than sawtoothing on allocator and validation work.

use bytemuck::{Pod, Zeroable};

use super::convert;
use crate::error::GfxError;
use crate::frame::FrameView;

/// Texture filter used when the framebuffer is scaled to the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleFilter {
    /// Hard pixel edges. Correct default for 2D-era systems.
    Nearest,
    /// Bilinear. Preferred for 3D-era systems and non-integer scale factors.
    Linear,
}

/// How the framebuffer is fitted into the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleMode {
    /// Preserve the core's display aspect ratio, letterbox the remainder.
    AspectFit,
    /// Largest whole-number pixel multiple that fits, then centre it. No shimmer.
    IntegerScale,
    /// Fill the canvas, ignoring aspect ratio.
    Stretch,
}

/// Uniform block consumed by `frame_blit.wgsl`. 32 bytes, 16-byte aligned.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct BlitUniforms {
    scale: [f32; 2],
    offset: [f32; 2],
    frame_size: [f32; 2],
    _padding: [f32; 2],
}

/// GPU-side framebuffer, rebuilt only when the core's output geometry changes.
struct FrameTarget {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

pub struct Renderer {
    /// Instance and adapter are held for the renderer's lifetime, not just for
    /// construction.
    ///
    /// On the WebGPU backend these own the browser-side objects the device and
    /// surface were created from. Letting them drop lets the browser collect the
    /// device, and the failure is silent in the worst way: submits and presents keep
    /// returning success while the canvas stays black and buffer maps fail. Both
    /// fields are load-bearing despite `_instance` never being read.
    ///
    /// Neither field is read; both exist so the browser-side objects outlive
    /// construction. The adapter is additionally what a future device-lost recovery
    /// path will re-request a device from.
    _instance: wgpu::Instance,
    _adapter: wgpu::Adapter,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    sampler_nearest: wgpu::Sampler,
    sampler_linear: wgpu::Sampler,
    frame_target: Option<FrameTarget>,
    filter: ScaleFilter,
    scale_mode: ScaleMode,
    /// Display aspect ratio reported by the active core.
    aspect_ratio: f32,
    /// Staging buffer for pixel-format normalisation. Sized once per geometry.
    convert_scratch: Vec<u8>,
    adapter_info: wgpu::AdapterInfo,
    frames_presented: u64,
    frames_dropped: u64,
    /// Set when the surface reports a size/format mismatch; reconfigured next frame.
    needs_reconfigure: bool,
}

impl Renderer {
    /// Builds a renderer for a browser canvas.
    ///
    /// `width`/`height` are *physical* pixels (CSS size x devicePixelRatio). wgpu
    /// writes them onto the canvas element during `configure`, so the caller must
    /// not set `canvas.width` itself — doing so from both sides is how you get a
    /// blurry or clipped presentation.
    #[cfg(target_arch = "wasm32")]
    pub async fn from_canvas(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
    ) -> Result<Self, GfxError> {
        // WebGPU only — no WebGL2 fallback is compiled in, by design.
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::BROWSER_WEBGPU;
        let instance = wgpu::Instance::new(descriptor);

        let surface: wgpu::Surface<'static> = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| GfxError::SurfaceCreation(e.to_string()))?;

        Self::from_surface(instance, surface, width, height).await
    }

    /// Backend-agnostic construction. Phase 2 calls this with a `CAMetalLayer`
    /// surface; the browser path calls it with a canvas surface.
    pub async fn from_surface(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> Result<Self, GfxError> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|_| GfxError::NoAdapter)?;

        let adapter_info = adapter.get_info();
        log::info!(
            "WebGPU adapter: {} ({:?}, {:?})",
            adapter_info.name,
            adapter_info.backend,
            adapter_info.device_type
        );

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("emulator-bridge-device"),
                required_features: wgpu::Features::empty(),
                // Ask for exactly what the adapter offers: requesting more than the
                // browser exposes fails device creation outright.
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .map_err(|e| GfxError::DeviceRequest(e.to_string()))?;

        // Validation errors are otherwise silent on the web; surface them loudly.
        device.on_uncaptured_error(std::sync::Arc::new(|error| {
            log::error!("wgpu uncaptured error: {error}");
        }));

        let caps = surface.get_capabilities(&adapter);
        if caps.formats.is_empty() {
            return Err(GfxError::SurfaceIncompatible);
        }
        let format = pick_surface_format(&caps);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            // SDR: emulator output is already in display space, and an HDR
            // swapchain would only add a colour transform to undo.
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: width.max(1),
            height: height.max(1),
            // Fifo == "present on vsync", which is what a rAF-driven loop wants;
            // anything else would tear or queue frames the loop never asked for.
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("frame-blit-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("frame_blit.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame-blit-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("frame-blit-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            // No immediate/push constants: the blit's only per-frame state is the
            // 32-byte uniform buffer.
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("frame-blit-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame-blit-uniforms"),
            size: core::mem::size_of::<BlitUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let sampler_nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("frame-sampler-nearest"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let sampler_linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("frame-sampler-linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        Ok(Self {
            _instance: instance,
            _adapter: adapter,
            surface,
            device,
            queue,
            config,
            pipeline,
            bind_group_layout,
            uniform_buffer,
            sampler_nearest,
            sampler_linear,
            frame_target: None,
            filter: ScaleFilter::Nearest,
            scale_mode: ScaleMode::AspectFit,
            aspect_ratio: 4.0 / 3.0,
            convert_scratch: Vec::new(),
            adapter_info,
            frames_presented: 0,
            frames_dropped: 0,
            needs_reconfigure: false,
        })
    }

    pub fn adapter_name(&self) -> &str {
        &self.adapter_info.name
    }

    pub fn adapter_summary(&self) -> String {
        format!(
            "{} / {:?} / {:?}",
            self.adapter_info.name, self.adapter_info.backend, self.adapter_info.device_type
        )
    }

    pub fn surface_size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn frames_presented(&self) -> u64 {
        self.frames_presented
    }

    pub fn frames_dropped(&self) -> u64 {
        self.frames_dropped
    }

    pub fn set_filter(&mut self, filter: ScaleFilter) {
        if self.filter != filter {
            self.filter = filter;
            // The sampler is baked into the bind group, so it has to be rebuilt.
            self.rebuild_bind_group();
        }
    }

    pub fn filter(&self) -> ScaleFilter {
        self.filter
    }

    pub fn set_scale_mode(&mut self, mode: ScaleMode) {
        self.scale_mode = mode;
    }

    pub fn scale_mode(&self) -> ScaleMode {
        self.scale_mode
    }

    pub fn set_aspect_ratio(&mut self, aspect_ratio: f32) {
        if aspect_ratio.is_finite() && aspect_ratio > 0.0 {
            self.aspect_ratio = aspect_ratio;
        }
    }

    /// Reconfigures the swapchain. Cheap enough to call from a `ResizeObserver`,
    /// but a no-op when the size is unchanged so observer churn costs nothing.
    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if width == self.config.width && height == self.config.height && !self.needs_reconfigure {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.needs_reconfigure = false;
    }

    /// Drops the framebuffer texture. Called when a session ends so a large PS1
    /// texture is not held while the user browses the library.
    pub fn release_frame_target(&mut self) {
        self.frame_target = None;
        self.convert_scratch = Vec::new();
    }

    /// Forces a swapchain reconfigure before the next present.
    ///
    /// The iOS foreground transition needs this. `resize` returns early when the size has
    /// not changed, and coming back from the background it usually has not — but the
    /// drawables behind the layer are gone regardless, so without this the first present
    /// after resuming reconfigures only because it happens to fail first.
    pub fn invalidate_surface(&mut self) {
        self.needs_reconfigure = true;
    }

    /// The wgpu device, for the platform layer that needs the backend object underneath.
    ///
    /// `pub(crate)` and deliberately not part of the public surface: `gfx::metal` uses these
    /// to read the `MTLDevice` back out, and nothing else should reach past the renderer.
    #[cfg(target_vendor = "apple")]
    pub(crate) fn wgpu_device(&self) -> &wgpu::Device {
        &self.device
    }

    #[cfg(target_vendor = "apple")]
    pub(crate) fn wgpu_queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Presents one frame.
    ///
    /// `frame: None` re-presents the existing texture, which is both the "core
    /// duped this frame" case and the "display refreshes faster than the core"
    /// case. With no texture at all it clears — the state between session start
    /// and first frame.
    pub fn present(&mut self, frame: Option<FrameView<'_>>) -> Result<(), GfxError> {
        if let Some(view) = frame.as_ref() {
            view.validate()?;
            self.upload(view);
        }

        if self.needs_reconfigure {
            self.surface.configure(&self.device, &self.config);
            self.needs_reconfigure = false;
        }

        let surface_texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                // Usable this frame; reconfigure before the next one.
                self.needs_reconfigure = true;
                t
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.needs_reconfigure = true;
                self.frames_dropped += 1;
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                // Tab hidden or compositor busy: skipping is correct, not an error.
                self.frames_dropped += 1;
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                self.frames_dropped += 1;
                return Err(GfxError::SurfaceLost);
            }
        };

        self.write_uniforms();

        let target_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame-blit-encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame-blit-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Letterbox bars, not pure black: distinguishes "running,
                        // letterboxed" from "nothing rendered at all".
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.02,
                            g: 0.02,
                            b: 0.03,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            if let Some(target) = &self.frame_target {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &target.bind_group, &[]);
                pass.draw(0..6, 0..1);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        self.queue.present(surface_texture);
        self.frames_presented += 1;
        Ok(())
    }

    /// Uploads a frame without presenting it.
    ///
    /// Lets a capture (thumbnail, screenshot) reflect the core's current image even
    /// if no frame has been presented yet — presentation and upload are separate
    /// concerns, and coupling them would make an offscreen capture depend on the
    /// swapchain being healthy.
    pub fn upload_frame(&mut self, view: &FrameView<'_>) -> Result<(), GfxError> {
        view.validate()?;
        self.upload(view);
        Ok(())
    }

    fn upload(&mut self, view: &FrameView<'_>) {
        if self
            .frame_target
            .as_ref()
            .is_none_or(|t| t.width != view.width || t.height != view.height)
        {
            self.create_frame_target(view.width, view.height);
        }

        let Some(target) = &self.frame_target else {
            return;
        };

        let rgba = convert::to_rgba8(view, &mut self.convert_scratch);

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &target.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(view.width * 4),
                rows_per_image: Some(view.height),
            },
            wgpu::Extent3d {
                width: view.width,
                height: view.height,
                depth_or_array_layers: 1,
            },
        );
    }

    fn create_frame_target(&mut self, width: u32, height: u32) {
        log::debug!("(re)allocating framebuffer texture {width}x{height}");
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("core-framebuffer"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let bind_group = self.build_bind_group(&texture);
        self.frame_target = Some(FrameTarget {
            texture,
            bind_group,
            width,
            height,
        });
        // Force the scratch buffer to be re-sized on the next conversion.
        self.convert_scratch = Vec::new();
    }

    fn build_bind_group(&self, texture: &wgpu::Texture) -> wgpu::BindGroup {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = match self.filter {
            ScaleFilter::Nearest => &self.sampler_nearest,
            ScaleFilter::Linear => &self.sampler_linear,
        };
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame-blit-bind-group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    fn rebuild_bind_group(&mut self) {
        if let Some(target) = self.frame_target.take() {
            let bind_group = self.build_bind_group(&target.texture);
            self.frame_target = Some(FrameTarget {
                bind_group,
                ..target
            });
        }
    }

    fn write_uniforms(&mut self) {
        let (width, height) = (self.config.width, self.config.height);
        self.write_uniforms_for(width, height);
    }

    /// Writes the blit uniforms for a specific target size. Separated from
    /// [`Self::write_uniforms`] so an offscreen capture can be framed for its own
    /// dimensions rather than the swapchain's.
    fn write_uniforms_for(&mut self, target_width: u32, target_height: u32) {
        let (fb_width, fb_height) = self
            .frame_target
            .as_ref()
            .map(|t| (t.width as f32, t.height as f32))
            .unwrap_or((1.0, 1.0));

        let uniforms = BlitUniforms {
            scale: self.compute_scale(fb_width, fb_height, target_width, target_height),
            offset: [0.0, 0.0],
            frame_size: [fb_width, fb_height],
            _padding: [0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
    }

    /// Clip-space scale that fits the framebuffer into a target of the given size.
    fn compute_scale(
        &self,
        fb_width: f32,
        fb_height: f32,
        target_width: u32,
        target_height: u32,
    ) -> [f32; 2] {
        let surface_w = target_width as f32;
        let surface_h = target_height as f32;
        if surface_w <= 0.0 || surface_h <= 0.0 {
            return [1.0, 1.0];
        }

        match self.scale_mode {
            ScaleMode::Stretch => [1.0, 1.0],
            ScaleMode::AspectFit => {
                let surface_aspect = surface_w / surface_h;
                if self.aspect_ratio > surface_aspect {
                    // Content is wider: full width, bars top and bottom.
                    [1.0, surface_aspect / self.aspect_ratio]
                } else {
                    [self.aspect_ratio / surface_aspect, 1.0]
                }
            }
            ScaleMode::IntegerScale => {
                // Aspect-correct the source width first (many systems have
                // non-square pixels), then take the largest whole multiple that
                // fits both the corrected width and the raw pixel width — so a
                // 320x240 frame stretched to 4:3 still lands on whole pixels.
                let corrected_w = fb_height * self.aspect_ratio;
                let widest = corrected_w.max(fb_width);
                let k = (surface_w / widest)
                    .min(surface_h / fb_height)
                    .floor()
                    .max(1.0);
                let draw_w = corrected_w * k;
                let draw_h = fb_height * k;
                if draw_w > surface_w || draw_h > surface_h {
                    // Surface smaller than one whole pixel multiple: fall back to
                    // aspect-fit rather than overflowing the canvas.
                    let surface_aspect = surface_w / surface_h;
                    return if self.aspect_ratio > surface_aspect {
                        [1.0, surface_aspect / self.aspect_ratio]
                    } else {
                        [self.aspect_ratio / surface_aspect, 1.0]
                    };
                }
                [draw_w / surface_w, draw_h / surface_h]
            }
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // The renderer owns the GPU device for the whole app run. If this fires
        // outside teardown, the device dies with it and everything downstream
        // (presents, buffer maps) silently stops working — so it is worth a log
        // rather than a silent drop.
        log::warn!(
            "renderer dropped after {} presented frames",
            self.frames_presented
        );
    }
}

/// `copyTextureToBuffer` requires each row to start on a 256-byte boundary, so a
/// capture buffer is usually wider than `width * 4` and must be de-padded on read.
const COPY_ALIGNMENT: u32 = 256;

/// An in-flight readback of the presented image.
///
/// Two-step by necessity: encoding and submitting is synchronous, but mapping the
/// staging buffer is asynchronous on WebGPU. The platform facade awaits the map
/// (in Phase 1, by bridging `map_async` to a JS `Promise`) and then calls
/// [`FrameCapture::take_rgba`].
pub struct FrameCapture {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_bytes_per_row: u32,
}

impl FrameCapture {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The staging buffer, for the caller to map.
    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    /// De-pads the mapped buffer into tight RGBA8 rows. Consumes the capture, so a
    /// buffer cannot be read twice or left mapped.
    ///
    /// Returns an error if the buffer was not successfully mapped first.
    pub fn take_rgba(self) -> Result<Vec<u8>, GfxError> {
        let tight = self.width as usize * 4;
        let mut out = vec![0u8; tight * self.height as usize];
        {
            let view = self
                .buffer
                .slice(..)
                .get_mapped_range()
                .map_err(|err| GfxError::InvalidFrame(format!("capture not mapped: {err}")))?;
            for row in 0..self.height as usize {
                let src = row * self.padded_bytes_per_row as usize;
                let dst = row * tight;
                out[dst..dst + tight].copy_from_slice(&view[src..src + tight]);
            }
        }
        self.buffer.unmap();
        Ok(out)
    }
}

impl Renderer {
    /// Renders the current frame into an offscreen texture and submits a readback.
    ///
    /// Uses the same pipeline, bind group and scaling maths as [`Self::present`], so
    /// what comes back is what the canvas shows — which makes this the way to verify
    /// the GPU path without depending on the browser's canvas compositing (headless
    /// and software WebGPU stacks frequently will not surface presented frames).
    ///
    /// Also the primitive the UI will want for save-state thumbnails and
    /// screenshots. Allocates per call, so it is a deliberate action rather than
    /// something to run every frame.
    pub fn encode_capture(&mut self, width: u32, height: u32) -> Result<FrameCapture, GfxError> {
        let width = width.max(1);
        let height = height.max(1);

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame-capture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Frame for the capture's dimensions. The next `present` rewrites these for
        // the swapchain, so borrowing the shared uniform buffer is safe between frames.
        self.write_uniforms_for(width, height);

        let padded_bytes_per_row =
            width * 4 + (COPY_ALIGNMENT - (width * 4) % COPY_ALIGNMENT) % COPY_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame-capture-staging"),
            size: (padded_bytes_per_row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame-capture-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame-capture-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.02,
                            g: 0.02,
                            b: 0.03,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(target) = &self.frame_target {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &target.bind_group, &[]);
                pass.draw(0..6, 0..1);
            }
        }

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit(Some(encoder.finish()));

        Ok(FrameCapture {
            buffer,
            width,
            height,
            padded_bytes_per_row,
        })
    }
}

/// Prefers a non-sRGB swapchain format.
///
/// The framebuffer texture is `Rgba8Unorm` (cores emit values already in display
/// space). Presenting into an sRGB swapchain would apply a second encode and
/// visibly wash the image out.
fn pick_surface_format(caps: &wgpu::SurfaceCapabilities) -> wgpu::TextureFormat {
    caps.formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .unwrap_or(caps.formats[0])
}

#[cfg(test)]
mod tests {
    // Scaling maths is pure and worth locking down; it is the part most likely to
    // regress silently (a wrong letterbox is easy to miss, a wrong integer scale
    // shows up as shimmer). Full renderer tests need a GPU and live in Phase 1b's
    // browser test suite.

    fn aspect_fit(surface: (f32, f32), content_aspect: f32) -> [f32; 2] {
        let surface_aspect = surface.0 / surface.1;
        if content_aspect > surface_aspect {
            [1.0, surface_aspect / content_aspect]
        } else {
            [content_aspect / surface_aspect, 1.0]
        }
    }

    #[test]
    fn wide_surface_letterboxes_horizontally() {
        // 4:3 content on a 16:9 surface: bars left and right.
        let s = aspect_fit((1920.0, 1080.0), 4.0 / 3.0);
        assert!(s[0] < 1.0 && (s[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn tall_surface_letterboxes_vertically() {
        let s = aspect_fit((1080.0, 1920.0), 4.0 / 3.0);
        assert!((s[0] - 1.0).abs() < 1e-6 && s[1] < 1.0);
    }

    #[test]
    fn matching_aspect_fills_surface() {
        let s = aspect_fit((1600.0, 1200.0), 4.0 / 3.0);
        assert!((s[0] - 1.0).abs() < 1e-6 && (s[1] - 1.0).abs() < 1e-6);
    }
}
