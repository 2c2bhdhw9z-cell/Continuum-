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

/// How a core's single framebuffer is divided into screens.
///
/// Every system shipping today is [`ScreenSplit::Single`], and the reason this enum exists
/// before anything needs the other variant is the DS and the 3DS: both hand over ONE
/// framebuffer with TWO screens stacked inside it, and presenting that means drawing two
/// regions of one texture to two places. That is the same draw with different numbers, not a
/// second pass, so the generalisation belongs in the compositor rather than in whichever core
/// integration happens to arrive first.
///
/// Proving it now is deliberate and comes from the hardware-render plan: the composite pass is
/// generalised BEFORE any hardware core exists, because a mistake here is a rectangle in the
/// wrong place rather than a game that misbehaves for reasons that could be anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenSplit {
    /// One screen filling the whole framebuffer.
    #[default]
    Single,
    /// Two screens stacked vertically inside the framebuffer, presented stacked in the same
    /// order: the top half of the texture is drawn above the bottom half.
    VerticalPair,
}

impl ScreenSplit {
    /// How many screens this split produces. Never zero, and never above [`MAX_SCREENS`].
    pub fn count(self) -> u32 {
        match self {
            ScreenSplit::Single => 1,
            ScreenSplit::VerticalPair => 2,
        }
    }
}

/// Upper bound on screens in one composite pass. Matches `MAX_SCREENS` in `frame_blit.wgsl`.
///
/// Four rather than two. The DS and the 3DS need two, and a fixed-size uniform array costs 32
/// bytes per unused slot, which is not worth a second buffer layout to reclaim. Raising it means
/// changing the constant in both files, which is why they name each other.
const MAX_SCREENS: usize = 4;

/// One screen's placement: where it is sampled from, and where it is drawn.
///
/// PACKED INTO TWO `vec4`s RATHER THAN FOUR `vec2`s, and that is about alignment rather than
/// size. WGSL requires an element of an array in the uniform address space to be 16-byte
/// aligned; a struct of `vec2`s is 8-byte aligned, so it either fails to compile or quietly
/// acquires padding this side would then disagree with. A `vec4` is 16-byte aligned by
/// definition, so the two declarations cannot drift apart.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
struct ScreenUniform {
    /// `[scale_x, scale_y, offset_x, offset_y]` in clip space.
    dest: [f32; 4],
    /// `[scale_u, scale_v, offset_u, offset_v]` applied to the texture coordinate. Identity is
    /// `[1, 1, 0, 0]`, which samples the whole texture.
    source: [f32; 4],
}

impl ScreenUniform {
    /// The whole texture drawn to the whole target. What an unused slot holds, so a slot that
    /// somehow gets drawn shows the frame rather than a degenerate triangle.
    const fn identity() -> Self {
        Self {
            dest: [1.0, 1.0, 0.0, 0.0],
            source: [1.0, 1.0, 0.0, 0.0],
        }
    }
}

/// Uniform block consumed by `frame_blit.wgsl`. 144 bytes.
///
/// The field order is not cosmetic: `screens` has to start at a 16-byte boundary for the array
/// alignment rule above, and the three scalars before it add up to exactly 16.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct BlitUniforms {
    frame_size: [f32; 2],
    screen_count: u32,
    _padding: u32,
    screens: [ScreenUniform; MAX_SCREENS],
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
    /// How the framebuffer is divided into screens. See [`ScreenSplit`].
    screen_split: ScreenSplit,
    /// Instances the next draw issues, kept in step with the uniform array by
    /// `write_uniforms_for`. Cached because the draw does not write uniforms, and a count that
    /// disagreed with the array would draw an instance whose placement was never written.
    screen_count: u32,
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
    /// Backend-agnostic construction. The iOS path calls this with a `CAMetalLayer`
    /// surface, and it is deliberately not written against Metal: a surface is a surface,
    /// which is what kept this file honest while there were two platforms and is what will
    /// keep it honest when Android arrives.
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
            screen_split: ScreenSplit::Single,
            screen_count: 1,
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

    /// Sets how the framebuffer is divided into screens. See [`ScreenSplit`].
    ///
    /// A 144-byte buffer write and a change to one instance count, so this is free to call at any
    /// time and takes effect on the next presented frame. No pipeline or bind group is touched.
    ///
    /// Nothing selects anything but [`ScreenSplit::Single`] yet, because no dual-screen core is
    /// integrated. It is reachable now so that the compositor is proven before one is.
    pub fn set_screen_split(&mut self, split: ScreenSplit) {
        if self.screen_split == split {
            return;
        }
        self.screen_split = split;
        self.write_uniforms();
    }

    pub fn screen_split(&self) -> ScreenSplit {
        self.screen_split
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
    /// Unconditional, unlike [`Self::wgpu_queue`] below. The Metal handoff needs this only on
    /// Apple, but the frame capture needs it on every target: a readback has to wait for the GPU,
    /// and waiting means polling the device. Keeping it target-gated would have made capture an
    /// Apple-only feature for no reason other than where the accessor happened to live.
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
                pass.draw(0..6, 0..self.screen_count.max(1));
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

        let fitted = self.compute_scale(fb_width, fb_height, target_width, target_height);
        let screens = Self::screen_layout(self.screen_split, fitted);
        // Cached because the draw needs it and the draw does not write uniforms: this and
        // `screens` are produced together, so a count that disagreed with the array would mean
        // drawing an instance whose placement was never written.
        self.screen_count = self.screen_split.count();

        let uniforms = BlitUniforms {
            frame_size: [fb_width, fb_height],
            screen_count: self.screen_count,
            _padding: 0,
            screens,
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
    }

    /// Divides an already-fitted rectangle into one placement per screen.
    ///
    /// Pure, associated and taking the fitted scale rather than reading `self`, so the whole of
    /// the layout arithmetic can be unit tested with no GPU, no device and no core. That is the
    /// point of doing this step before a hardware core exists.
    ///
    /// `fitted` is the clip-space half-extent the framebuffer occupies AS A WHOLE, which is what
    /// [`Self::compute_scale`] already returns. The stacked case then subdivides that rectangle
    /// rather than recomputing an aspect fit per screen, and the two results tile it exactly:
    /// the pair together covers the same area one screen would have, so switching split cannot
    /// change how much of the window is used or where the letterbox falls.
    fn screen_layout(split: ScreenSplit, fitted: [f32; 2]) -> [ScreenUniform; MAX_SCREENS] {
        let mut screens = [ScreenUniform::identity(); MAX_SCREENS];
        let [sx, sy] = fitted;

        match split {
            ScreenSplit::Single => {
                screens[0] = ScreenUniform {
                    dest: [sx, sy, 0.0, 0.0],
                    source: [1.0, 1.0, 0.0, 0.0],
                };
            }
            ScreenSplit::VerticalPair => {
                let half = sy * 0.5;
                // Top screen. Clip space is y-up, so the top half is the POSITIVE offset, while
                // its source is the SMALLER v because the shader flips v before applying this.
                // The two conventions disagreeing is exactly the kind of thing that renders
                // upside down or swapped, which is why it is spelled out rather than inferred.
                screens[0] = ScreenUniform {
                    dest: [sx, half, 0.0, half],
                    source: [1.0, 0.5, 0.0, 0.0],
                };
                screens[1] = ScreenUniform {
                    dest: [sx, half, 0.0, -half],
                    source: [1.0, 0.5, 0.0, 0.5],
                };
            }
        }
        screens
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
                pass.draw(0..6, 0..self.screen_count.max(1));
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


#[cfg(test)]
mod screen_layout_tests {
    use super::{BlitUniforms, Renderer, ScreenSplit, ScreenUniform, MAX_SCREENS};

    /// The compositor's placement arithmetic, called for real rather than reimplemented.
    ///
    /// `screen_layout` is an associated function taking the already-fitted scale precisely so
    /// that it can be reached from here: the rest of the renderer needs a GPU, a surface and a
    /// device, and none of those are available on the machine this is built on. The step this
    /// belongs to is the one that generalises the composite pass before any hardware-rendered
    /// core exists, and being testable on a Linux box with no GPU is most of why it goes first.
    fn layout(split: ScreenSplit, fitted: [f32; 2]) -> [ScreenUniform; MAX_SCREENS] {
        Renderer::screen_layout(split, fitted)
    }

    /// Clip-space vertical span a placement covers, as `(bottom, top)`.
    fn vertical_span(screen: &ScreenUniform) -> (f32, f32) {
        let (scale, offset) = (screen.dest[1], screen.dest[3]);
        (offset - scale, offset + scale)
    }

    /// Texture-space vertical span a placement samples, as `(top_v, bottom_v)`.
    fn source_span(screen: &ScreenUniform) -> (f32, f32) {
        let (scale, offset) = (screen.source[1], screen.source[3]);
        (offset, offset + scale)
    }

    #[test]
    fn a_single_screen_is_exactly_what_the_one_quad_version_drew() {
        // THE REGRESSION GUARD FOR NINE WORKING SYSTEMS. Before this step the shader applied one
        // scale and a zero offset to a fullscreen quad and sampled the whole texture. If the
        // single-screen case is not still precisely that, every system already shipping changes
        // how it is framed, and a slightly wrong letterbox is the kind of thing that is noticed
        // late and blamed on something else.
        let screens = layout(ScreenSplit::Single, [0.75, 1.0]);
        assert_eq!(screens[0].dest, [0.75, 1.0, 0.0, 0.0]);
        assert_eq!(screens[0].source, [1.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn unused_slots_hold_the_identity_rather_than_zeroes() {
        // A zeroed slot is a degenerate triangle, which draws nothing and looks identical to a
        // draw that never happened. The identity draws the frame, so a count that ever ran long
        // is visible rather than silent.
        let screens = layout(ScreenSplit::Single, [1.0, 1.0]);
        for screen in &screens[1..] {
            assert_eq!(*screen, ScreenUniform::identity());
        }
    }

    #[test]
    fn a_vertical_pair_stacks_top_above_bottom() {
        let screens = layout(ScreenSplit::VerticalPair, [1.0, 1.0]);

        let (top_bottom_edge, top_top_edge) = vertical_span(&screens[0]);
        let (bottom_bottom_edge, bottom_top_edge) = vertical_span(&screens[1]);

        // Clip space is y-up, so the first screen must sit above the second.
        assert!(top_bottom_edge >= bottom_top_edge - 1e-6,
                "the top screen must not hang below the bottom one");
        assert!((top_top_edge - 1.0).abs() < 1e-6);
        assert!((bottom_bottom_edge + 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_vertical_pair_samples_the_top_half_for_the_top_screen() {
        // THE CONVENTION MOST LIKELY TO BE INVERTED. The shader flips v before applying the
        // source rect, so top-down texture coordinates are what arrive here: the screen drawn
        // HIGHER in clip space is the one sampling the SMALLER v. Getting this backwards swaps
        // the two screens of a DS, which looks deliberate and is not.
        let screens = layout(ScreenSplit::VerticalPair, [1.0, 1.0]);

        assert_eq!(source_span(&screens[0]), (0.0, 0.5), "top screen samples the top half");
        assert_eq!(source_span(&screens[1]), (0.5, 1.0), "bottom screen samples the bottom half");
    }

    #[test]
    fn a_vertical_pair_tiles_the_same_area_one_screen_would_have() {
        // Switching split must not change how much of the window is used or where the letterbox
        // falls: the pair subdivides the fitted rectangle rather than each half being fitted on
        // its own. Exactly adjacent, with no gap and no overlap.
        let fitted = [0.8, 0.6];
        let single = layout(ScreenSplit::Single, fitted);
        let pair = layout(ScreenSplit::VerticalPair, fitted);

        let (single_low, single_high) = vertical_span(&single[0]);
        let (top_low, top_high) = vertical_span(&pair[0]);
        let (bottom_low, bottom_high) = vertical_span(&pair[1]);

        assert!((top_high - single_high).abs() < 1e-6, "pair reaches the same top edge");
        assert!((bottom_low - single_low).abs() < 1e-6, "pair reaches the same bottom edge");
        assert!((top_low - bottom_high).abs() < 1e-6, "no gap and no overlap between them");

        // Width is untouched by a vertical split.
        assert!((pair[0].dest[0] - fitted[0]).abs() < 1e-6);
        assert!((pair[1].dest[0] - fitted[0]).abs() < 1e-6);
    }

    #[test]
    fn every_split_fits_inside_the_uniform_array() {
        // The draw issues `count` instances against a fixed-size array, so a split that counted
        // higher than the array is long would read past it. Checked here rather than trusted,
        // because the two numbers live in different declarations.
        for split in [ScreenSplit::Single, ScreenSplit::VerticalPair] {
            let count = split.count();
            assert!(count >= 1, "a split with no screens would present nothing");
            assert!(count as usize <= MAX_SCREENS,
                    "{split:?} wants {count} screens and the array holds {MAX_SCREENS}");
        }
    }

    #[test]
    fn the_uniform_block_matches_what_the_shader_declares() {
        // THE AGREEMENT THAT FAILS SILENTLY. `frame_blit.wgsl` declares this same block, and WGSL
        // requires an array element in the uniform address space to be 16-byte aligned. If the
        // scalars before `screens` stop adding up to 16, or `ScreenUniform` stops being 32 bytes,
        // the shader reads every placement from the wrong offset: the geometry is garbage and
        // nothing reports an error, because both sides still compile.
        assert_eq!(core::mem::size_of::<ScreenUniform>(), 32);
        assert_eq!(core::mem::size_of::<BlitUniforms>(), 16 + 32 * MAX_SCREENS);

        // `screens` has to begin exactly one 16-byte block in.
        let uniforms = BlitUniforms {
            frame_size: [0.0, 0.0],
            screen_count: 0,
            _padding: 0,
            screens: [ScreenUniform::identity(); MAX_SCREENS],
        };
        let base = &uniforms as *const _ as usize;
        let screens = &uniforms.screens as *const _ as usize;
        assert_eq!(screens - base, 16, "screens must start at a 16-byte boundary");
    }
}


#[cfg(test)]
mod shader_tests {
    /// Parses and validates `frame_blit.wgsl` the way wgpu will at runtime.
    ///
    /// THE ONLY CHECK IN THIS PROJECT THAT CAN CATCH A SHADER MISTAKE WITHOUT A PHONE. The shader
    /// is embedded with `include_str!` and compiled by wgpu at device creation, so nothing about
    /// it is a Rust compile error: a wrong type, a missing binding or a uniform alignment
    /// violation all build cleanly and then present a black screen on a device with no debugger.
    /// naga is wgpu's own shader front end and is already in the dependency tree at the same
    /// version, so running it here is the same validation the runtime performs, several minutes
    /// earlier and on the machine doing the building.
    ///
    /// This matters more from here on than it did before. The hardware-render work ahead changes
    /// this shader repeatedly, and the composite pass is exactly where a mistake is invisible
    /// until it is on screen.
    #[test]
    fn the_blit_shader_parses_and_validates() {
        let source = include_str!("frame_blit.wgsl");
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|err| panic!("frame_blit.wgsl does not parse:\n{}", err.emit_to_string(source)));

        // Validated with the same capability set a plain fragment pipeline gets, so a feature
        // that would need enabling on the device cannot slip in unnoticed.
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        );
        if let Err(err) = validator.validate(&module) {
            panic!("frame_blit.wgsl does not validate:\n{}", err.emit_to_string(source));
        }
    }

    /// The shader's screen array has to be as long as the Rust one.
    ///
    /// READ OUT OF THE SHADER TEXT, because nothing else connects the two numbers. `MAX_SCREENS`
    /// is declared once in `renderer.rs` and once in `frame_blit.wgsl`, and raising only the Rust
    /// one writes placements past the end of what the shader declares: both still compile, both
    /// still run, and the extra screens read whatever follows in the uniform buffer. Comparing the
    /// declarations is crude and is the only thing that actually holds them together.
    #[test]
    fn the_shader_declares_as_many_screens_as_the_engine_writes() {
        let source = include_str!("frame_blit.wgsl");
        let declared = source
            .split("array<Screen,")
            .nth(1)
            .and_then(|tail| tail.split('>').next())
            .map(str::trim)
            .and_then(|count| count.parse::<usize>().ok())
            .expect("frame_blit.wgsl should declare screens as array<Screen, N>");

        assert_eq!(
            declared,
            super::MAX_SCREENS,
            "the shader holds {declared} screens and the engine writes {}",
            super::MAX_SCREENS
        );
    }

    /// The entry points the pipeline asks for by name, which is a runtime lookup and therefore
    /// another thing no compiler checks. Renaming one in the shader and not in `renderer.rs` fails
    /// at device creation, which on this project means it fails on a phone.
    #[test]
    fn the_shader_has_the_entry_points_the_pipeline_names() {
        let source = include_str!("frame_blit.wgsl");
        let module = naga::front::wgsl::parse_str(source).expect("shader parses");

        let names: Vec<&str> = module
            .entry_points
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert!(names.contains(&"vs_main"), "expected a vs_main entry point, found {names:?}");
        assert!(names.contains(&"fs_main"), "expected an fs_main entry point, found {names:?}");
    }
}
