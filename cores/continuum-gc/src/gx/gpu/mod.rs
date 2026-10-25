//! The GPU drawer: the CPU drawer's pixels (draw.rs, tev.rs, texture.rs sampling, efb.rs
//! blending), drawn by the phone's graphics chip through Vulkan (draw.wgsl, compiled to
//! draw.spv by examples/shaders.rs).
//!
//! Everything before the pixels stays on the CPU exactly as it was: reading vertices, the
//! transform unit, lighting, clipping, the viewport, culling, decoding textures and unpacking the
//! BP registers into a `Setup` per primitive. Where the CPU drawer then hands a `Job` to its
//! helper threads, this hands the same setup (packed into words, see `State`) and the same
//! screen-space triangles to the graphics chip, in chunks, while the CPU carries on. The EFB is
//! memory both share (`EfbMem`): at every point where the CPU drawer waits for its helpers
//! (`Gx::sync`: EFB copies, clears, the CPU's own EFB access, save states) this waits for the
//! graphics chip, and the CPU code reads and writes the same words it always did.
//!
//! The device is the app's (the libretro Vulkan interface, `crate::libretro`); in the tests on
//! the Linux machine it is SwiftShader's. If anything about it fails, the CPU drawer takes over
//! (`Gx::gpu_failed`) and the reason goes to the log.

use super::draw::{Band, Job, Target};
use super::efb::{EfbMem, EFB_HEIGHT, EFB_WIDTH};
use super::raster::{Screen, Setup};
use super::texture::Decoded;
use ash::vk;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};

/// The kernels (examples/shaders.rs makes this from draw.wgsl).
static SPIRV: &[u8] = include_bytes!("draw.spv");

// The layouts draw.wgsl reads (its constants have the same names).
const EFB_PIXELS: usize = EFB_WIDTH * EFB_HEIGHT;
const TRI_WORDS: usize = 112;
const VERT_WORDS: usize = 36;
const SETUP_WORDS: usize = 128;
const S_HEAD: usize = 0;
const S_SC0: usize = 1;
const S_SC1: usize = 2;
const S_PE: usize = 3;
const S_ALPHA: usize = 4;
const S_MISC: usize = 5;
const S_FOG_A: usize = 6;
const S_FOG_B: usize = 7;
const S_FOG_C: usize = 8;
const S_FOG_COLOR: usize = 9;
const S_ZTEX_BIAS: usize = 10;
const S_REGS: usize = 11;
const S_TC_SCALE: usize = 27;
const S_IND_MAP: usize = 43;
const S_IND_SCALE: usize = 44;
const S_IND_MTX: usize = 45;
const S_STAGES: usize = 66;
const STAGE_WORDS: usize = 5;
const S_MAPS: usize = 146;
const MAP_WORDS: usize = 27;
const MAX_LEVELS: usize = 11;
pub const STATE_WORDS: usize = S_MAPS + 8 * MAP_WORDS;

/// Triangles between two waits (the buffers are emptied at every wait).
const MAX_TRIS: usize = 16384;
/// Storage buffers the kernels use (draw.wgsl's bindings 0..).
const BINDINGS: u32 = 8;
/// Triangles per chunk handed to the graphics chip while the CPU carries on.
const CHUNK: usize = 512;
/// Words of state records between two waits.
const STATE_CAP: usize = 1 << 20;
/// The texture heap: decoded textures as the CPU decoded them, 4 bytes a texel.
const HEAP_BYTES: [usize; 3] = [128 << 20, 64 << 20, 32 << 20];
/// Vertices further out than this (pixels) are clipped in screen space first, so the
/// kernels' 32-bit edge arithmetic stays exact (draw.wgsl `mul2`); the guard band keeps
/// ordinary games far inside it.
const LIMIT: f32 = 4000.0;

/// What the frontend (or a test) hands over: an instance, its physical device, a device on it
/// and a queue that can run compute work.
#[derive(Clone, Copy)]
pub struct Handles {
    pub get_instance_proc_addr: vk::PFN_vkGetInstanceProcAddr,
    pub instance: vk::Instance,
    pub physical: vk::PhysicalDevice,
    pub device: vk::Device,
    pub queue: vk::Queue,
    pub queue_family: u32,
    /// The frontend's queue lock (libretro's lock_queue/unlock_queue), when the queue is shared.
    pub queue_lock: Option<QueueLock>,
}

#[derive(Clone, Copy)]
pub struct QueueLock {
    pub handle: *mut c_void,
    pub lock: unsafe extern "C" fn(*mut c_void),
    pub unlock: unsafe extern "C" fn(*mut c_void),
}

/// Counters for the telemetry line.
#[derive(Default, Clone, Copy, Debug)]
pub struct GpuStats {
    pub chunks: u64,
    pub waits: u64,
    pub triangles: u64,
    pub states: u64,
    pub textures: u64,
    pub texture_bytes: u64,
    pub heap_resets: u64,
    /// Time waiting for the graphics chip.
    pub wait_time: std::time::Duration,
}

struct Ctx {
    device: ash::Device,
    memory: vk::PhysicalDeviceMemoryProperties,
    queue: vk::Queue,
    lock: Option<QueueLock>,
    /// After a failure the graphics chip may still be working on: nothing is destroyed any more
    /// (freeing memory it still uses could take the whole app down); the little it holds is left.
    poisoned: std::sync::atomic::AtomicBool,
}

// SAFETY: Vulkan handles may be used from any thread with external synchronisation, which the
// owner (one game thread, the queue lock) provides; the queue lock's handle is the frontend's.
unsafe impl Send for Ctx {}
unsafe impl Sync for Ctx {}

impl Ctx {
    fn poisoned(&self) -> bool {
        self.poisoned.load(std::sync::atomic::Ordering::SeqCst)
    }
}

fn vk_err(what: &str) -> impl Fn(vk::Result) -> String + '_ {
    move |e| format!("{what}: {e:?}")
}

/// A storage buffer in memory the CPU sees too (on an iPhone all memory is shared), mapped for
/// as long as it lives.
struct Buffer {
    ctx: Arc<Ctx>,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    ptr: *mut u8,
    size: usize,
}

// SAFETY: as Ctx; the mapping is plain memory.
unsafe impl Send for Buffer {}
unsafe impl Sync for Buffer {}

impl Buffer {
    fn new(ctx: &Arc<Ctx>, size: usize) -> Result<Buffer, String> {
        let d = &ctx.device;
        let info = vk::BufferCreateInfo::default().size(size as u64).usage(vk::BufferUsageFlags::STORAGE_BUFFER).sharing_mode(vk::SharingMode::EXCLUSIVE);
        // SAFETY: valid device and create info.
        let buffer = unsafe { d.create_buffer(&info, None) }.map_err(vk_err("create buffer"))?;
        // SAFETY: the buffer was just made on this device.
        let req = unsafe { d.get_buffer_memory_requirements(buffer) };
        let want = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
        let types = &ctx.memory.memory_types[..ctx.memory.memory_type_count as usize];
        let pick = |extra: vk::MemoryPropertyFlags| {
            types.iter().enumerate().position(|(i, t)| req.memory_type_bits & (1 << i) != 0 && t.property_flags.contains(want | extra))
        };
        let Some(index) = pick(vk::MemoryPropertyFlags::DEVICE_LOCAL).or_else(|| pick(vk::MemoryPropertyFlags::empty())) else {
            // SAFETY: not used anywhere yet.
            unsafe { d.destroy_buffer(buffer, None) };
            return Err("no memory both the CPU and the graphics chip can use".into());
        };
        let alloc = vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(index as u32);
        // SAFETY: valid allocate info.
        let memory = match unsafe { d.allocate_memory(&alloc, None) } {
            Ok(m) => m,
            Err(e) => {
                // SAFETY: not used anywhere yet.
                unsafe { d.destroy_buffer(buffer, None) };
                return Err(format!("allocate {} MB: {e:?}", size >> 20));
            }
        };
        // SAFETY: the memory fits the buffer's requirements; mapped whole, once.
        let ptr = unsafe {
            d.bind_buffer_memory(buffer, memory, 0)
                .and_then(|_| d.map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty()))
        };
        let ptr = match ptr {
            Ok(p) => p as *mut u8,
            Err(e) => {
                // SAFETY: not used anywhere yet.
                unsafe {
                    d.destroy_buffer(buffer, None);
                    d.free_memory(memory, None);
                }
                return Err(format!("map memory: {e:?}"));
            }
        };
        // SAFETY: `size` bytes are mapped.
        unsafe { std::ptr::write_bytes(ptr, 0, size) };
        Ok(Buffer { ctx: ctx.clone(), buffer, memory, ptr, size })
    }

    fn words(&mut self) -> &mut [u32] {
        // SAFETY: the mapping is `size` bytes, 4-byte aligned (Vulkan maps at least that), and is
        // only touched by the game thread while the graphics chip is not using that part (see
        // the module notes).
        unsafe { std::slice::from_raw_parts_mut(self.ptr as *mut u32, self.size / 4) }
    }

    fn info(&self) -> vk::DescriptorBufferInfo {
        vk::DescriptorBufferInfo::default().buffer(self.buffer).offset(0).range(vk::WHOLE_SIZE)
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        if self.ctx.poisoned() {
            return;
        }
        // SAFETY: the graphics chip is idle (GpuDrawer's Drop waited) and nothing maps it any more.
        unsafe {
            self.ctx.device.destroy_buffer(self.buffer, None);
            self.ctx.device.free_memory(self.memory, None);
        }
    }
}

/// The GPU drawer.
pub struct GpuDrawer {
    ctx: Arc<Ctx>,
    efb: Arc<Buffer>,
    states: Buffer,
    tris: Buffer,
    /// Written and read only by the kernels (bound in the descriptor set).
    _setups: Buffer,
    _boxes: Buffer,
    _blocks: Buffer,
    heap: Buffer,
    counters: Buffer,
    shader: vk::ShaderModule,
    set_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pool: vk::DescriptorPool,
    set: vk::DescriptorSet,
    setup_pipe: vk::Pipeline,
    raster_pipe: vk::Pipeline,
    cmd_pool: vk::CommandPool,
    /// Command buffers and their fences; the first `used` are submitted since the last idle.
    cmds: Vec<(vk::CommandBuffer, vk::Fence)>,
    used: usize,
    states_used: usize,
    tris_used: usize,
    chunk_first: usize,
    /// The chunk's vertices' extent on screen (min x, min y, max x, max y).
    chunk_box: [f32; 4],
    /// The last state record written, and where (consecutive primitives mostly share one).
    last: Option<(u32, Box<[u32; STATE_WORDS]>)>,
    heap_used: usize,
    /// Textures in the heap, by the decoded texture's address (kept alive here, so the address
    /// cannot be reused), and where their first texel is.
    textures: HashMap<usize, (u32, Arc<Decoded>)>,
    pending_pixels: u64,
    pub stats: GpuStats,
    /// What the device is (for the log).
    pub device_name: String,
}

// SAFETY: see Ctx and Buffer.
unsafe impl Send for GpuDrawer {}

const EMPTY_BOX: [f32; 4] = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];

impl GpuDrawer {
    /// Makes everything the kernels need on the given device.
    ///
    /// # Safety
    /// The handles must be live and stay live until the drawer is dropped.
    pub unsafe fn new(h: &Handles) -> Result<GpuDrawer, String> {
        let entry = ash::Entry::from_static_fn(ash::StaticFn { get_instance_proc_addr: h.get_instance_proc_addr });
        let instance = ash::Instance::load(entry.static_fn(), h.instance);
        let device = ash::Device::load(instance.fp_v1_0(), h.device);
        let props = instance.get_physical_device_properties(h.physical);
        let memory = instance.get_physical_device_memory_properties(h.physical);
        let device_name = std::ffi::CStr::from_ptr(props.device_name.as_ptr()).to_string_lossy().into_owned();
        let limits = props.limits;
        if limits.max_compute_work_group_invocations < 64 || limits.max_push_constants_size < 16 {
            return Err(format!("{device_name} cannot run the drawing kernels"));
        }
        let ctx = Arc::new(Ctx { device, memory, queue: h.queue, lock: h.queue_lock, poisoned: false.into() });
        let max_buffer = limits.max_storage_buffer_range as usize;
        let efb = Arc::new(Buffer::new(&ctx, 2 * EFB_PIXELS * 4)?);
        let states = Buffer::new(&ctx, STATE_CAP * 4)?;
        let tris = Buffer::new(&ctx, MAX_TRIS * TRI_WORDS * 4)?;
        let setups = Buffer::new(&ctx, MAX_TRIS * SETUP_WORDS * 4)?;
        let boxes = Buffer::new(&ctx, MAX_TRIS * 16)?;
        let blocks = Buffer::new(&ctx, MAX_TRIS / 64 * 16)?;
        let counters = Buffer::new(&ctx, 64)?;
        let mut heap = Err(String::new());
        for size in HEAP_BYTES {
            if size <= max_buffer {
                heap = Buffer::new(&ctx, size);
                if heap.is_ok() {
                    break;
                }
            }
        }
        let heap = heap.map_err(|e| format!("texture memory: {e}"))?;
        let d = &ctx.device;

        let code = ash::util::read_spv(&mut std::io::Cursor::new(SPIRV)).map_err(|e| format!("kernels: {e}"))?;
        let shader = d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&code), None).map_err(vk_err("create shader module"))?;
        let bindings: Vec<vk::DescriptorSetLayoutBinding> = (0..BINDINGS)
            .map(|b| {
                vk::DescriptorSetLayoutBinding::default()
                    .binding(b)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::COMPUTE)
            })
            .collect();
        let set_layout = d.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings), None).map_err(vk_err("descriptor set layout"))?;
        let push = [vk::PushConstantRange::default().stage_flags(vk::ShaderStageFlags::COMPUTE).offset(0).size(16)];
        let set_layouts = [set_layout];
        let layout = d.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts).push_constant_ranges(&push), None).map_err(vk_err("pipeline layout"))?;
        let stage = |name: &'static std::ffi::CStr| vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::COMPUTE).module(shader).name(name);
        let infos = [
            vk::ComputePipelineCreateInfo::default().stage(stage(c"setup")).layout(layout),
            vk::ComputePipelineCreateInfo::default().stage(stage(c"raster")).layout(layout),
        ];
        let pipes = d.create_compute_pipelines(vk::PipelineCache::null(), &infos, None).map_err(|(_, e)| format!("compile the drawing kernels: {e:?}"))?;
        let sizes = [vk::DescriptorPoolSize::default().ty(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(BINDINGS)];
        let pool = d.create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().max_sets(1).pool_sizes(&sizes), None).map_err(vk_err("descriptor pool"))?;
        let set = d.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(pool).set_layouts(&set_layouts)).map_err(vk_err("descriptor set"))?[0];
        let infos: Vec<[vk::DescriptorBufferInfo; 1]> = [&*efb, &states, &tris, &setups, &boxes, &heap, &counters, &blocks].iter().map(|b| [b.info()]).collect();
        let writes: Vec<vk::WriteDescriptorSet> = infos
            .iter()
            .enumerate()
            .map(|(i, info)| vk::WriteDescriptorSet::default().dst_set(set).dst_binding(i as u32).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(info))
            .collect();
        d.update_descriptor_sets(&writes, &[]);
        let cmd_pool = d
            .create_command_pool(&vk::CommandPoolCreateInfo::default().queue_family_index(h.queue_family).flags(vk::CommandPoolCreateFlags::TRANSIENT), None)
            .map_err(vk_err("command pool"))?;
        let heap_mb = heap.size >> 20;
        Ok(GpuDrawer {
            ctx: ctx.clone(),
            efb,
            states,
            tris,
            _setups: setups,
            _boxes: boxes,
            _blocks: blocks,
            heap,
            counters,
            shader,
            set_layout,
            layout,
            pool,
            set,
            setup_pipe: pipes[0],
            raster_pipe: pipes[1],
            cmd_pool,
            cmds: Vec::new(),
            used: 0,
            states_used: 0,
            tris_used: 0,
            chunk_first: 0,
            chunk_box: EMPTY_BOX,
            last: None,
            heap_used: 0,
            textures: HashMap::new(),
            pending_pixels: 0,
            stats: GpuStats::default(),
            device_name: format!("{device_name}, {heap_mb} MB for textures"),
        })
    }

    /// The EFB's colour and depth words in the graphics chip's memory.
    pub fn efb(&self) -> (EfbMem, EfbMem) {
        let p = self.efb.ptr as *mut u32;
        let keep: Arc<dyn std::any::Any + Send + Sync> = self.efb.clone();
        // SAFETY: the buffer holds both halves and `keep` keeps it alive; the drawer only lets the
        // graphics chip at them between a submit and the next wait (module notes).
        unsafe { (EfbMem::mapped(p, EFB_PIXELS, keep.clone()), EfbMem::mapped(p.add(EFB_PIXELS), EFB_PIXELS, keep)) }
    }

    /// After a failure: nothing is destroyed from now on (see `Ctx::poisoned`).
    pub fn poison(self) {
        self.ctx.poisoned.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    // ------------------------------------------------------------------ handing work over

    /// Queues one primitive's triangles (what the CPU drawer would make one `Job` of).
    pub(in crate::gx) fn submit(&mut self, s: &Setup, tris: &[[Screen; 3]]) -> Result<(), String> {
        let nattr = 8 + 3 * s.num_tex;
        let mut st = self.state(s)?;
        for t in tris {
            if t.iter().any(|v| !v.x.is_finite() || !v.y.is_finite()) {
                continue;
            }
            if t.iter().all(|v| v.x.abs() < LIMIT && v.y.abs() < LIMIT) {
                self.triangle(&mut st, s, t, nattr)?;
            } else {
                for piece in clip_far(t, nattr) {
                    self.triangle(&mut st, s, &piece, nattr)?;
                }
            }
        }
        Ok(())
    }

    fn triangle(&mut self, st: &mut u32, s: &Setup, t: &[Screen; 3], nattr: usize) -> Result<(), String> {
        if self.tris_used == MAX_TRIS {
            self.flush()?;
            self.idle()?;
            self.empty();
            *st = self.state(s)?;
        }
        let w = self.tris.words();
        let base = self.tris_used * TRI_WORDS;
        w[base] = *st;
        w[base + 1] = 0;
        let b = &mut self.chunk_box;
        for (k, v) in t.iter().enumerate() {
            let o = base + 2 + k * VERT_WORDS;
            w[o] = v.x.to_bits();
            w[o + 1] = v.y.to_bits();
            w[o + 2] = v.z.to_bits();
            w[o + 3] = v.inv_w.to_bits();
            for (d, a) in w[o + 4..o + 4 + nattr].iter_mut().zip(&v.attr[..nattr]) {
                *d = a.to_bits();
            }
            b[0] = b[0].min(v.x);
            b[1] = b[1].min(v.y);
            b[2] = b[2].max(v.x);
            b[3] = b[3].max(v.y);
        }
        self.tris_used += 1;
        self.stats.triangles += 1;
        if self.tris_used - self.chunk_first >= CHUNK {
            self.flush()?;
        }
        Ok(())
    }

    /// The state record for `s` (packed as draw.wgsl reads it), written if it is not the last one.
    fn state(&mut self, s: &Setup) -> Result<u32, String> {
        let mut offsets = [None; 8];
        let mut full = false;
        for (m, d) in s.pipe.tex.iter().enumerate() {
            if let Some(d) = d {
                offsets[m] = self.texture(d);
                full |= offsets[m].is_none();
            }
        }
        if full {
            // The heap is full: once the graphics chip is done with it, start it again.
            self.flush()?;
            self.idle()?;
            self.empty();
            self.heap_used = 0;
            self.textures.clear();
            self.stats.heap_resets += 1;
            for (m, d) in s.pipe.tex.iter().enumerate() {
                // A texture bigger than the whole heap is left out (sampled as zero).
                offsets[m] = d.as_ref().and_then(|d| self.texture(d));
            }
        }
        let mut words = Box::new([0u32; STATE_WORDS]);
        pack_state(s, &offsets, &mut words);
        if let Some((at, last)) = &self.last {
            if last[..] == words[..] {
                return Ok(*at);
            }
        }
        if self.states_used + STATE_WORDS > STATE_CAP {
            self.flush()?;
            self.idle()?;
            self.empty();
        }
        let at = self.states_used;
        self.states.words()[at..at + STATE_WORDS].copy_from_slice(&words[..]);
        self.states_used += STATE_WORDS;
        self.stats.states += 1;
        self.last = Some((at as u32, words));
        Ok(at as u32)
    }

    /// Where texture `d` starts in the heap (copied in now if it is new); None when it does not fit.
    fn texture(&mut self, d: &Arc<Decoded>) -> Option<u32> {
        let key = Arc::as_ptr(d) as usize;
        if let Some((at, _)) = self.textures.get(&key) {
            return Some(*at);
        }
        let total: usize = d.levels.iter().take(MAX_LEVELS).map(|l| l.2.len()).sum();
        if self.heap_used + total > self.heap.size / 4 {
            return None;
        }
        let at = self.heap_used;
        let heap = self.heap.words();
        let mut o = at;
        for (_, _, px) in d.levels.iter().take(MAX_LEVELS) {
            // RGBA bytes, one word a texel, red in the low byte (draw.wgsl `texel`).
            for (dst, p) in heap[o..o + px.len()].iter_mut().zip(px) {
                *dst = u32::from_le_bytes(*p);
            }
            o += px.len();
        }
        self.heap_used += total;
        self.stats.textures += 1;
        self.stats.texture_bytes += 4 * total as u64;
        self.textures.insert(key, (at as u32, d.clone()));
        Some(at as u32)
    }

    /// Hands the triangles queued since the last chunk to the graphics chip.
    fn flush(&mut self) -> Result<(), String> {
        let n = self.tris_used - self.chunk_first;
        if n == 0 {
            return Ok(());
        }
        let first = self.chunk_first;
        // Chunks start on a block of 64 (draw.wgsl `blocks`): full chunks are a multiple of 64
        // and a partial one is only ever the last before the buffers are emptied.
        debug_assert!(first.is_multiple_of(64));
        self.chunk_first = self.tris_used;
        let b = std::mem::replace(&mut self.chunk_box, EMPTY_BOX);
        // The tiles the chunk's vertices span (a pixel either side for the rounding to 1/16).
        let tile = |v: f32, max: u32| ((v / 16.0).floor().clamp(0.0, max as f32)) as u32;
        let (cols, rows) = ((EFB_WIDTH / 16) as u32 - 1, (EFB_HEIGHT / 16) as u32 - 1);
        let (tx0, ty0) = (tile(b[0] - 1.0, cols), tile(b[1] - 1.0, rows));
        let (tx1, ty1) = (tile(b[2] + 1.0, cols).max(tx0), tile(b[3] + 1.0, rows).max(ty0));
        let (cb, fence) = self.next_cmd()?;
        let d = &self.ctx.device;
        let words = |a: u32, b: u32, c: u32, e: u32| -> [u8; 16] {
            let mut o = [0u8; 16];
            for (k, v) in [a, b, c, e].into_iter().enumerate() {
                o[4 * k..4 * k + 4].copy_from_slice(&v.to_ne_bytes());
            }
            o
        };
        let barrier = |src: vk::PipelineStageFlags, dst: vk::PipelineStageFlags, sa: vk::AccessFlags, da: vk::AccessFlags| {
            let m = [vk::MemoryBarrier::default().src_access_mask(sa).dst_access_mask(da)];
            // SAFETY: recording into `cb`.
            unsafe { d.cmd_pipeline_barrier(cb, src, dst, vk::DependencyFlags::empty(), &m, &[], &[]) };
        };
        let compute = vk::PipelineStageFlags::COMPUTE_SHADER;
        let rw = vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE;
        // SAFETY: `cb` is a fresh command buffer from our pool; every handle is ours and live.
        unsafe {
            d.begin_command_buffer(cb, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT))
                .map_err(vk_err("begin command buffer"))?;
            // The previous chunk's pixels are in the EFB before this one's are drawn over them.
            barrier(compute, compute, vk::AccessFlags::SHADER_WRITE, rw);
            d.cmd_bind_descriptor_sets(cb, vk::PipelineBindPoint::COMPUTE, self.layout, 0, &[self.set], &[]);
            d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::COMPUTE, self.setup_pipe);
            d.cmd_push_constants(cb, self.layout, vk::ShaderStageFlags::COMPUTE, 0, &words(first as u32, n as u32, 0, 0));
            d.cmd_dispatch(cb, n.div_ceil(64) as u32, 1, 1);
            barrier(compute, compute, vk::AccessFlags::SHADER_WRITE, vk::AccessFlags::SHADER_READ);
            d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::COMPUTE, self.raster_pipe);
            d.cmd_push_constants(cb, self.layout, vk::ShaderStageFlags::COMPUTE, 0, &words(first as u32, n as u32, tx0, ty0));
            d.cmd_dispatch(cb, tx1 - tx0 + 1, ty1 - ty0 + 1, 1);
            // What the CPU reads after the wait.
            barrier(compute, vk::PipelineStageFlags::HOST, vk::AccessFlags::SHADER_WRITE, vk::AccessFlags::HOST_READ);
            d.end_command_buffer(cb).map_err(vk_err("end command buffer"))?;
            let cbs = [cb];
            let submit = [vk::SubmitInfo::default().command_buffers(&cbs)];
            if let Some(l) = self.ctx.lock {
                (l.lock)(l.handle);
            }
            let r = d.queue_submit(self.ctx.queue, &submit, fence);
            if let Some(l) = self.ctx.lock {
                (l.unlock)(l.handle);
            }
            r.map_err(vk_err("submit"))?;
        }
        self.stats.chunks += 1;
        Ok(())
    }

    fn next_cmd(&mut self) -> Result<(vk::CommandBuffer, vk::Fence), String> {
        if self.used == self.cmds.len() {
            if self.cmds.len() >= 256 {
                self.idle()?;
            } else {
                let d = &self.ctx.device;
                let info = vk::CommandBufferAllocateInfo::default().command_pool(self.cmd_pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1);
                // SAFETY: our pool, our device.
                let cb = unsafe { d.allocate_command_buffers(&info) }.map_err(vk_err("command buffer"))?[0];
                // SAFETY: plain fence.
                let fence = unsafe { d.create_fence(&vk::FenceCreateInfo::default(), None) }.map_err(vk_err("fence"))?;
                self.cmds.push((cb, fence));
            }
        }
        let c = self.cmds[self.used];
        self.used += 1;
        Ok(c)
    }

    /// Waits until the graphics chip has done everything handed over (the last fence covers all
    /// earlier work on the queue), then makes the command buffers reusable.
    fn idle(&mut self) -> Result<(), String> {
        if self.used == 0 {
            return Ok(());
        }
        let t = std::time::Instant::now();
        let d = &self.ctx.device;
        let fence = self.cmds[self.used - 1].1;
        // SAFETY: a fence of ours that was submitted. Twenty seconds: a chip that has not finished
        // by then is not going to (SwiftShader, drawing on the test machine's processor, can
        // take seconds for a big chunk).
        let r = unsafe { d.wait_for_fences(&[fence], true, 20_000_000_000) };
        self.stats.wait_time += t.elapsed();
        match r {
            Ok(()) => {}
            Err(vk::Result::TIMEOUT) => return Err("the graphics chip did not finish drawing (20 s)".into()),
            Err(e) => return Err(format!("waiting for the graphics chip: {e:?}")),
        }
        let fences: Vec<vk::Fence> = self.cmds[..self.used].iter().map(|c| c.1).collect();
        // SAFETY: all signalled; the command buffers are done.
        unsafe {
            d.reset_fences(&fences).map_err(vk_err("reset fences"))?;
            d.reset_command_pool(self.cmd_pool, vk::CommandPoolResetFlags::empty()).map_err(vk_err("reset command pool"))?;
        }
        self.used = 0;
        let c = self.counters.words();
        self.pending_pixels += c[0] as u64;
        c[0] = 0;
        Ok(())
    }

    /// Empties the triangle and state buffers (only when the graphics chip is idle).
    fn empty(&mut self) {
        debug_assert!(self.used == 0 && self.chunk_first == self.tris_used);
        self.states_used = 0;
        self.tris_used = 0;
        self.chunk_first = 0;
        self.last = None;
    }

    /// Waits until everything queued is drawn into the EFB. Returns the pixels drawn since the
    /// last wait.
    pub fn wait(&mut self) -> Result<u64, String> {
        if self.tris_used == 0 && self.used == 0 {
            return Ok(std::mem::take(&mut self.pending_pixels));
        }
        self.flush()?;
        self.idle()?;
        self.empty();
        self.stats.waits += 1;
        // Decoded textures stay in the heap from picture to picture; when it is getting full it
        // starts again here, where nothing is using it.
        if self.heap_used > self.heap.size / 4 / 4 * 3 {
            self.heap_used = 0;
            self.textures.clear();
            self.stats.heap_resets += 1;
        }
        Ok(std::mem::take(&mut self.pending_pixels))
    }
}

impl Drop for GpuDrawer {
    fn drop(&mut self) {
        if self.ctx.poisoned() {
            return;
        }
        if self.idle().is_err() {
            self.ctx.poisoned.store(true, std::sync::atomic::Ordering::SeqCst);
            return;
        }
        let d = &self.ctx.device;
        // SAFETY: the graphics chip is done with all of it.
        unsafe {
            for (_, f) in &self.cmds {
                d.destroy_fence(*f, None);
            }
            d.destroy_command_pool(self.cmd_pool, None);
            d.destroy_pipeline(self.setup_pipe, None);
            d.destroy_pipeline(self.raster_pipe, None);
            d.destroy_descriptor_pool(self.pool, None);
            d.destroy_pipeline_layout(self.layout, None);
            d.destroy_descriptor_set_layout(self.set_layout, None);
            d.destroy_shader_module(self.shader, None);
        }
    }
}

/// Packs a primitive's setup the way draw.wgsl reads it. `heap[m]`: where texture map m's first
/// texel is, when it has one.
fn pack_state(s: &Setup, heap: &[Option<u32>; 8], w: &mut [u32; STATE_WORDS]) {
    let p = &s.pipe;
    let b = |v: bool| v as u32;
    w[S_HEAD] = (p.stages.len() as u32 & 255) | ((s.num_chans as u32 & 3) << 8) | ((s.num_tex as u32 & 15) << 16) | ((p.num_ind as u32 & 7) << 24);
    let half = |v: i32| v as u32 & 0xFFFF;
    w[S_SC0] = half(s.sc.0) | (half(s.sc.1) << 16);
    w[S_SC1] = half(s.sc.2) | (half(s.sc.3) << 16);
    let pe = &s.pe;
    w[S_PE] = b(pe.z_enable)
        | ((pe.z_func & 7) << 1)
        | (b(pe.z_update) << 4)
        | (b(pe.z_early) << 5)
        | (b(pe.blend) << 6)
        | (b(pe.logic) << 7)
        | (b(pe.subtract) << 8)
        | ((pe.src_factor & 7) << 9)
        | ((pe.dst_factor & 7) << 12)
        | ((pe.logic_op & 15) << 15)
        | (b(pe.color_update) << 19)
        | (b(pe.alpha_update) << 20)
        | (b(pe.dst_alpha.is_some()) << 21)
        | ((pe.format & 7) << 22);
    w[S_ALPHA] = pe.dst_alpha.unwrap_or(0) as u32
        | ((p.alpha_ref[0] as u32 & 255) << 8)
        | ((p.alpha_ref[1] as u32 & 255) << 16)
        | ((p.alpha_comp[0] & 7) << 24)
        | ((p.alpha_comp[1] & 7) << 27)
        | ((p.alpha_logic & 3) << 30);
    let need = s.need_lod.iter().enumerate().fold(0u32, |a, (t, n)| a | (b(*n) << t));
    w[S_MISC] = (p.fog_type & 7) | (b(p.fog_ortho) << 3) | ((p.ztex_op & 3) << 4) | ((p.ztex_fmt & 3) << 6) | (need << 8);
    w[S_FOG_A] = p.fog_a.to_bits();
    w[S_FOG_B] = p.fog_b.to_bits();
    w[S_FOG_C] = p.fog_c.to_bits();
    w[S_FOG_COLOR] = (p.fog_color[0] as u32 & 255) | ((p.fog_color[1] as u32 & 255) << 8) | ((p.fog_color[2] as u32 & 255) << 16);
    w[S_ZTEX_BIAS] = p.ztex_bias;
    for k in 0..4 {
        for c in 0..4 {
            w[S_REGS + 4 * k + c] = p.regs[k][c] as u32;
        }
    }
    for t in 0..8 {
        w[S_TC_SCALE + 2 * t] = p.tc_scale[t].0.to_bits();
        w[S_TC_SCALE + 2 * t + 1] = p.tc_scale[t].1.to_bits();
    }
    let (mut im, mut is) = (0u32, 0u32);
    for i in 0..4 {
        im |= ((p.ind_map[i] as u32 & 7) | ((p.ind_coord[i] as u32 & 7) << 3)) << (8 * i);
        is |= ((p.ind_scale[i].0 & 15) | ((p.ind_scale[i].1 & 15) << 4)) << (8 * i);
    }
    w[S_IND_MAP] = im;
    w[S_IND_SCALE] = is;
    for (m, (mtx, scale)) in p.ind_mtx.iter().enumerate() {
        let o = S_IND_MTX + 7 * m;
        for r in 0..2 {
            for c in 0..3 {
                w[o + 3 * r + c] = mtx[r][c].to_bits();
            }
        }
        w[o + 6] = scale.to_bits();
    }
    for (n, st) in p.stages.iter().take(16).enumerate() {
        let o = S_STAGES + STAGE_WORDS * n;
        let mut w0 = 0;
        let mut w1 = 0;
        for k in 0..4 {
            w0 |= ((st.c_in[k].row as u32 & 15) | (b(st.c_in[k].alpha) << 4)) << (5 * k);
            w1 |= (st.a_in[k] as u32 & 15) << (4 * k);
            w1 |= (st.ras_swizzle[k] as u32 & 3) << (16 + 2 * k);
            w1 |= (st.tex_swizzle[k] as u32 & 3) << (24 + 2 * k);
        }
        w[o] = w0;
        w[o + 1] = w1;
        w[o + 2] = (st.kc_val[0] as u32 & 255) | ((st.kc_val[1] as u32 & 255) << 8) | ((st.kc_val[2] as u32 & 255) << 16) | ((st.ka_val as u32 & 255) << 24);
        w[o + 3] = (st.c_bias as u32 & 3)
            | (b(st.c_sub) << 2)
            | (b(st.c_clamp) << 3)
            | ((st.c_scale as u32 & 3) << 4)
            | ((st.c_dest as u32 & 3) << 6)
            | ((st.a_bias as u32 & 3) << 8)
            | (b(st.a_sub) << 10)
            | (b(st.a_clamp) << 11)
            | ((st.a_scale as u32 & 3) << 12)
            | ((st.a_dest as u32 & 3) << 14)
            | ((st.texmap as u32 & 7) << 16)
            | ((st.texcoord as u32 & 7) << 19)
            | (b(st.tex_on) << 22)
            | ((st.ras as u32 & 7) << 23);
        w[o + 4] = st.ind;
    }
    for m in 0..8 {
        let (Some(d), Some(at)) = (&p.tex[m], heap[m]) else { continue };
        let t = &p.maps[m];
        let o = S_MAPS + MAP_WORDS * m;
        let nlev = d.levels.len().min(MAX_LEVELS);
        w[o + 1] = (t.wrap_s & 3) | ((t.wrap_t & 3) << 2) | (b(t.mag_linear) << 4) | (b(t.min_linear) << 5) | ((t.mip & 3) << 6) | ((nlev as u32) << 8) | (1 << 15);
        w[o + 2] = t.lod_bias.to_bits();
        w[o + 3] = t.min_lod.to_bits();
        w[o + 4] = t.max_lod.to_bits();
        let mut at = at;
        for (l, (lw, lh, px)) in d.levels.iter().take(nlev).enumerate() {
            w[o + 5 + 2 * l] = at;
            w[o + 6 + 2 * l] = (lw & 0xFFFF) | ((lh & 0xFFFF) << 16);
            at += px.len() as u32;
        }
    }
}

/// A triangle with a vertex beyond `LIMIT`, clipped in screen space to a box well outside the
/// EFB. Every attribute here (x, y, z, 1/w and the attributes divided by w) is linear across
/// the screen, so the pieces draw the same pixels.
fn clip_far(t: &[Screen; 3], nattr: usize) -> Vec<[Screen; 3]> {
    let lerp = |a: &Screen, b: &Screen, f: f32| {
        let mut o = *a;
        o.x = a.x + (b.x - a.x) * f;
        o.y = a.y + (b.y - a.y) * f;
        o.z = a.z + (b.z - a.z) * f;
        o.inv_w = a.inv_w + (b.inv_w - a.inv_w) * f;
        for k in 0..nattr {
            o.attr[k] = a.attr[k] + (b.attr[k] - a.attr[k]) * f;
        }
        o
    };
    let edge = 2048.0f32;
    let planes: [&dyn Fn(&Screen) -> f32; 4] = [
        &|v| v.x + edge,
        &|v| (EFB_WIDTH as f32 + edge) - v.x,
        &|v| v.y + edge,
        &|v| (EFB_HEIGHT as f32 + edge) - v.y,
    ];
    let mut poly: Vec<Screen> = t.to_vec();
    for plane in planes {
        let mut next = Vec::with_capacity(poly.len() + 2);
        for i in 0..poly.len() {
            let (a, b) = (&poly[i], &poly[(i + 1) % poly.len()]);
            let (da, db) = (plane(a), plane(b));
            if da >= 0.0 {
                next.push(*a);
            }
            if (da >= 0.0) != (db >= 0.0) {
                next.push(lerp(a, b, da / (da - db)));
            }
        }
        poly = next;
        if poly.len() < 3 {
            return Vec::new();
        }
    }
    (1..poly.len() - 1).map(|i| [poly[0], poly[i], poly[i + 1]]).collect()
}

// ---------------------------------------------------------------------- Gx's slot

/// Where Gx keeps its GPU drawer. A copy of Gx (a save state) has none.
#[derive(Default)]
pub struct Slot(Mutex<Option<Box<GpuDrawer>>>);

impl Clone for Slot {
    fn clone(&self) -> Self {
        Slot::default()
    }
}

impl Slot {
    pub(in crate::gx) fn get(&mut self) -> Option<&mut GpuDrawer> {
        self.0.get_mut().unwrap_or_else(|p| p.into_inner()).as_deref_mut()
    }

    pub(in crate::gx) fn put(&mut self, g: GpuDrawer) {
        *self.0.get_mut().unwrap_or_else(|p| p.into_inner()) = Some(Box::new(g));
    }

    pub(in crate::gx) fn take(&mut self) -> Option<GpuDrawer> {
        self.0.get_mut().unwrap_or_else(|p| p.into_inner()).take().map(|b| *b)
    }

    pub(in crate::gx) fn stats(&self) -> Option<GpuStats> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map(|g| g.stats)
    }

    /// `wait` through a shared borrow (save states).
    pub(in crate::gx) fn wait_shared(&self) {
        let mut g = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(g) = g.as_mut() {
            if let Ok(n) = g.wait() {
                g.pending_pixels += n;
            }
        }
    }
}

// ---------------------------------------------------------------------- the picture check

/// How the GPU drawer's pixels compare with the CPU drawer's for the same work.
#[derive(Default, Debug, Clone, Copy)]
pub struct Comparison {
    pub pixels: usize,
    /// Colour words that differ at all, by more than 4 in a channel, and the largest difference.
    pub differ: usize,
    pub differ_4: usize,
    pub max: i32,
    pub depth_differ: usize,
    /// The largest depth difference.
    pub depth_max: u32,
    pub cpu_drawn: u64,
    pub gpu_drawn: u64,
}

impl std::fmt::Display for Comparison {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} of {} pixels differ ({} by more than 4, at most {}), {} depths differ (at most {}); {} pixels drawn, {} on the processor",
            self.differ, self.pixels, self.differ_4, self.max, self.depth_differ, self.depth_max, self.gpu_drawn, self.cpu_drawn
        )
    }
}

/// Draws `work` with the CPU drawer and with `g` from the same starting EFB, and compares.
/// The drawer's own EFB is used (and left holding the result).
pub(in crate::gx) fn compare(g: &mut GpuDrawer, start_color: &[u32], start_depth: &[u32], work: &[(Arc<Setup>, Vec<[Screen; 3]>)]) -> Result<Comparison, String> {
    let mut color = start_color.to_vec();
    let mut depth = start_depth.to_vec();
    let mut cpu_drawn = 0;
    for (s, tris) in work {
        let job = Job { setup: s.clone(), tris: tris.clone(), target: Target::new(&mut color, &mut depth) };
        cpu_drawn += job.run(Band::ALL);
    }
    g.wait()?;
    let (mut gc, mut gd) = g.efb();
    gc.copy_from_slice(start_color);
    gd.copy_from_slice(start_depth);
    for (s, tris) in work {
        g.submit(s, tris)?;
    }
    let gpu_drawn = g.wait()?;
    let mut c = Comparison { pixels: EFB_PIXELS, cpu_drawn, gpu_drawn, ..Default::default() };
    for i in 0..EFB_PIXELS {
        if gd[i] != depth[i] {
            c.depth_differ += 1;
            c.depth_max = c.depth_max.max(gd[i].abs_diff(depth[i]));
        }
        if gc[i] != color[i] {
            c.differ += 1;
            let d = (0..4).map(|k| (((gc[i] >> (8 * k)) & 255) as i32 - ((color[i] >> (8 * k)) & 255) as i32).abs()).max().unwrap_or(0);
            if d > 4 {
                c.differ_4 += 1;
            }
            c.max = c.max.max(d);
        }
    }
    Ok(c)
}

mod scene;

/// Development: `picture_check`'s primitives one at a time, each from the same random EFB, with
/// a description of every primitive whose colours differ by more than 4 anywhere.
#[doc(hidden)]
pub fn picture_check_each(g: &mut GpuDrawer, seed: u64, primitives: usize) -> Result<Vec<(usize, Comparison, String)>, String> {
    let (color, depth, work) = scene::random_work(seed, primitives, false);
    let mut out = Vec::new();
    for (i, w) in work.iter().enumerate() {
        let c = compare(g, &color, &depth, std::slice::from_ref(w))?;
        if c.differ_4 > 0 || c.gpu_drawn != c.cpu_drawn {
            let s = &w.0;
            let p = &s.pipe;
            let stages: Vec<String> = p
                .stages
                .iter()
                .map(|st| {
                    format!(
                        "[c {:?} b{} s{} cl{} sc{} d{} | a {:?} b{} s{} cl{} sc{} d{} | tex {} map {} tc {} ras {} ind {:#x}]",
                        st.c, st.c_bias, st.c_sub as u8, st.c_clamp as u8, st.c_scale, st.c_dest, st.a, st.a_bias, st.a_sub as u8, st.a_clamp as u8, st.a_scale, st.a_dest, st.tex_on as u8, st.texmap, st.texcoord, st.ras, st.ind
                    )
                })
                .collect();
            let maps: Vec<String> = (0..8)
                .filter_map(|m| {
                    p.tex[m].as_ref().map(|d| {
                        let t = &p.maps[m];
                        format!("map{m} {}x{} levels {} wrap {}/{} lin {}/{} mip {} bias {:.2} lod {:.2}..{:.2}", d.levels[0].0, d.levels[0].1, d.levels.len(), t.wrap_s, t.wrap_t, t.mag_linear as u8, t.min_linear as u8, t.mip, t.lod_bias, t.min_lod, t.max_lod)
                    })
                })
                .collect();
            let text = format!(
                "chans {} tex {} ind {} fog {} ztex {}/{} alpha {:?} {:?} {} pe {:?} need_lod {:?}\n    {}\n    {}",
                s.num_chans, s.num_tex, p.num_ind, p.fog_type, p.ztex_op, p.ztex_fmt, p.alpha_comp, p.alpha_ref, p.alpha_logic, s.pe, s.need_lod, stages.join("\n    "), maps.join("; ")
            );
            out.push((i, c, text));
        }
    }
    Ok(out)
}

/// The check the core runs when it starts the GPU drawer (and the tests run): `primitives` random
/// primitives drawn both ways from the same random EFB. The textures it used are dropped after.
pub fn picture_check(g: &mut GpuDrawer, seed: u64, primitives: usize) -> Result<Comparison, String> {
    picture_check_with(g, seed, primitives, false)
}

/// `picture_check`, with vertices far off the screen when `far` (tests).
#[doc(hidden)]
pub fn picture_check_with(g: &mut GpuDrawer, seed: u64, primitives: usize, far: bool) -> Result<Comparison, String> {
    let (color, depth, work) = scene::random_work(seed, primitives, far);
    let c = compare(g, &color, &depth, &work)?;
    g.heap_used = 0;
    g.textures.clear();
    Ok(c)
}
