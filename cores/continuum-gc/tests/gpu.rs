//! The GPU drawer against the CPU drawer, on whatever Vulkan this machine has (SwiftShader on the
//! Linux test machine: VK_ICD_FILENAMES pointing at its vk_swiftshader_icd.json, LD_LIBRARY_PATH
//! at its libvulkan.so.1). Without Vulkan the drawing tests say so and pass.

#[path = "../src/gx/gpu/shader_build.rs"]
mod shader_build;
#[path = "../src/gx/gpu/test_vulkan.rs"]
mod test_vulkan;

use continuum_gc::gx::gpu::{picture_check, GpuDrawer};
use test_vulkan::vulkan;

/// Development: which random primitives the two drawers draw differently, one at a time
/// (cargo test --release --test gpu each -- --ignored --nocapture).
#[test]
#[ignore]
fn each_primitive() {
    let Some(vk) = vulkan() else { return };
    // SAFETY: live handles, outliving the drawer.
    let mut g = unsafe { GpuDrawer::new(&vk.handles) }.expect("GPU drawer");
    let seed: u64 = std::env::var("GPU_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    for (i, c, text) in continuum_gc::gx::gpu::picture_check_each(&mut g, 0x5EED_0000 + seed, 120).unwrap() {
        eprintln!("primitive {i}: {c}\n    {text}");
    }
}

#[test]
fn the_kernels_in_the_core_are_the_ones_in_draw_wgsl() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/gx/gpu");
    let wgsl = std::fs::read_to_string(format!("{dir}/draw.wgsl")).unwrap();
    let spv = shader_build::compile(&wgsl).unwrap();
    let embedded = std::fs::read(format!("{dir}/draw.spv")).unwrap();
    assert!(spv == embedded, "src/gx/gpu/draw.spv is out of date: cargo run --example shaders");
}

/// Random primitives over every combiner, texture, depth, blending and format setting, drawn by
/// both drawers from the same random EFB: the same pixels drawn, and the colours the same but
/// for floating-point rounding (a colour one apart where an interpolated value sits on a
/// boundary, now and then a neighbouring texel).
#[test]
fn the_gpu_draws_what_the_cpu_draws() {
    let Some(vk) = vulkan() else {
        eprintln!("no Vulkan on this machine: GPU drawer not tested");
        return;
    };
    // SAFETY: live handles, outliving the drawer.
    let mut g = unsafe { GpuDrawer::new(&vk.handles) }.expect("GPU drawer");
    eprintln!("device: {}", g.device_name);
    let seeds: u64 = std::env::var("GPU_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(12);
    let (mut px, mut differ, mut differ_4, mut depth) = (0usize, 0usize, 0usize, 0usize);
    for seed in 0..seeds {
        let c = picture_check(&mut g, 0x5EED_0000 + seed, 120).expect("drawn");
        eprintln!("seed {seed}: {c}");
        let drawn_apart = c.gpu_drawn.abs_diff(c.cpu_drawn);
        assert!(drawn_apart * 1000 <= c.cpu_drawn, "seed {seed}: {} pixels drawn, {} on the processor", c.gpu_drawn, c.cpu_drawn);
        px += c.pixels;
        differ += c.differ;
        differ_4 += c.differ_4;
        depth += c.depth_differ;
    }
    eprintln!("all: {differ} of {px} differ, {differ_4} by more than 4, {depth} depths");
    assert!(differ_4 * 1000 < px, "too many pixels differ by more than 4: {differ_4} of {px}");
}

/// Triangles reaching thousands of pixels off the screen: the GPU drawer cuts them down to a box
/// around the screen first (its edge arithmetic is 32-bit); the same pixels are drawn.
#[test]
fn far_off_screen_triangles_draw_the_same() {
    let Some(vk) = vulkan() else {
        eprintln!("no Vulkan on this machine: GPU drawer not tested");
        return;
    };
    // SAFETY: live handles, outliving the drawer.
    let mut g = unsafe { GpuDrawer::new(&vk.handles) }.expect("GPU drawer");
    for seed in 0..3 {
        let c = continuum_gc::gx::gpu::picture_check_with(&mut g, 0xFA2_0000 + seed, 40, true).expect("drawn");
        eprintln!("far seed {seed}: {c}");
        assert!(c.differ_4 * 1000 < c.pixels, "far seed {seed}: {c}");
        assert!(c.gpu_drawn.abs_diff(c.cpu_drawn) * 1000 <= c.cpu_drawn, "far seed {seed}: {c}");
    }
}
