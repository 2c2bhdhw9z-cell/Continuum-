//! The GPU drawer against the CPU drawer on a real program: the same game in two machines, one
//! drawing on the processor and one on the graphics chip (SwiftShader here), field by field,
//! comparing the pictures shown and the EFBs.
//!
//! VK_ICD_FILENAMES=.../vk_swiftshader_icd.json LD_LIBRARY_PATH=... \
//!   cargo run --release --example gpucheck -- game.dol fields
//! (CGC_SD / CGC_ARGV / CGC_ENV as boot, each machine on its own copy of the SD card image;
//! CGC_SHOW=n writes field n's two pictures as PPM files.)
#[path = "../src/gx/gpu/test_vulkan.rs"]
mod test_vulkan;

use continuum_gc::gx::gpu::GpuDrawer;
use continuum_gc::libretro::open;
use continuum_gc::machine::Machine;

fn boot(path: &str, which: &str) -> Machine {
    let mut m = open(path).expect("boots");
    if let Ok(d) = std::env::var("CGC_SD") {
        // The game writes to its card: each machine gets a fresh copy.
        let copy = format!("/tmp/gpucheck-sd-{which}.img");
        std::fs::copy(&d, &copy).expect("copy the SD image");
        m.hw.sd = Some(continuum_gc::sd::SdCard::open(&copy).expect("sd image"));
    }
    if let Ok(args) = std::env::var("CGC_ARGV") {
        let env = std::env::var("CGC_ENV").unwrap_or_default();
        let entry = m.cpu.pc;
        m.set_args(entry, &args.split(' ').collect::<Vec<_>>(), &env.split(' ').filter(|s| !s.is_empty()).collect::<Vec<_>>());
    }
    m
}

fn ppm(path: &str, w: u32, h: u32, px: &[u32]) {
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in px {
        out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    std::fs::write(path, out).expect("write picture");
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let fields: u32 = a[2].parse().expect("fields");
    let show: Option<u32> = std::env::var("CGC_SHOW").ok().and_then(|s| s.parse().ok());
    let vk = test_vulkan::vulkan().expect("a Vulkan device (SwiftShader: set VK_ICD_FILENAMES and LD_LIBRARY_PATH)");
    let mut cpu = boot(&a[1], "cpu");
    let mut gpu = boot(&a[1], "gpu");
    // SAFETY: live handles, outliving the drawer.
    let g = unsafe { GpuDrawer::new(&vk.handles) }.expect("GPU drawer");
    eprintln!("device: {}", g.device_name);
    gpu.hw.gx.attach_gpu(g);
    let (mut t_cpu, mut t_gpu) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
    let (mut pic_fields, mut pic_px, mut pic_bad, mut efb_px) = (0, 0usize, 0usize, 0usize);
    for f in 0..fields {
        let t = std::time::Instant::now();
        cpu.run_field(cpu.cpu_hz / 60);
        cpu.hw.gx.sync();
        t_cpu += t.elapsed();
        let t = std::time::Instant::now();
        gpu.run_field(gpu.cpu_hz / 60);
        gpu.hw.gx.sync();
        t_gpu += t.elapsed();
        assert!(gpu.hw.gx.gpu_drawing(), "the GPU drawer stopped at field {f}: {:?}", gpu.hw.gx.gpu_error);
        assert_eq!(cpu.cpu.pc, gpu.cpu.pc, "the two machines went different ways at field {f}");
        let (pc, pg) = (cpu.hw.vi_picture(), gpu.hw.vi_picture());
        let differ_efb = cpu.hw.gx.efb_color.iter().zip(gpu.hw.gx.efb_color.iter()).filter(|(a, b)| a != b).count();
        efb_px += differ_efb;
        if let (Some((w, h, a)), Some((_, _, b))) = (&pc, &pg) {
            let mut bad = 0;
            let mut max = 0;
            let mut differ = 0;
            for (x, y) in a.iter().zip(b.iter()) {
                if x != y {
                    differ += 1;
                    let d = (0..3).map(|k| (((x >> (8 * k)) & 255) as i32 - ((y >> (8 * k)) & 255) as i32).abs()).max().unwrap();
                    max = max.max(d);
                    if d > 8 {
                        bad += 1;
                    }
                }
            }
            pic_fields += 1;
            pic_px += a.len();
            pic_bad += bad;
            if differ > 0 && (f % 60 == 0 || bad > 100) {
                eprintln!("field {f}: picture {w}x{h}: {differ} pixels differ, {bad} by more than 8 (at most {max}); EFB {differ_efb} words differ");
            }
            if show == Some(f) {
                ppm(&format!("/tmp/gpucheck-{f}-cpu.ppm"), *w, *h, a);
                ppm(&format!("/tmp/gpucheck-{f}-gpu.ppm"), *w, *h, b);
                eprintln!("field {f}: pictures in /tmp/gpucheck-{f}-cpu.ppm and -gpu.ppm");
            }
        }
    }
    let gs = gpu.hw.gx.gpu_stats().unwrap();
    eprintln!(
        "{fields} fields; {pic_fields} with a picture: {pic_bad} of {pic_px} picture pixels differ by more than 8; {efb_px} EFB words differed in all; CPU drawer {:.2}s, GPU drawer {:.2}s; gpu {gs:?}",
        t_cpu.as_secs_f64(),
        t_gpu.as_secs_f64()
    );
    eprintln!("triangles {} vs {}, pixels {} vs {}", cpu.hw.gx.stats.triangles, gpu.hw.gx.stats.triangles, cpu.hw.gx.stats.pixels, gpu.hw.gx.stats.pixels);
    eprintln!(
        "processor time handing primitives over (vertices, setup, and with the CPU drawer its pixels): CPU drawer {:.3}s, GPU drawer {:.3}s",
        cpu.hw.gx.stats.draw_time.as_secs_f64(),
        gpu.hw.gx.stats.draw_time.as_secs_f64()
    );
}
