//! Floating-point and paired-single speed: a loop of the kind of maths 3D code does (paired
//! loads and stores, multiply-adds, sums, scalar single-precision maths), run by the plain and the
//! fast interpreter, in millions of instructions a second. Our own program, hand-assembled.
//!
//! cargo run --release --example fpbench
use continuum_gc::bus::Bus;
use continuum_gc::disc::Console;
use continuum_gc::dol::{Executable, Section};
use continuum_gc::machine::Machine;

fn psq(op: u32, d: u32, a: u32, disp: i32) -> u32 {
    (op << 26) | (d << 21) | (a << 16) | (disp as u32 & 0xFFF)
}
fn ps(xo: u32, d: u32, a: u32, b: u32, c: u32) -> u32 {
    (4 << 26) | (d << 21) | (a << 16) | (b << 11) | (c << 6) | (xo << 1)
}
fn fp59(xo: u32, d: u32, a: u32, b: u32, c: u32) -> u32 {
    (59 << 26) | (d << 21) | (a << 16) | (b << 11) | (c << 6) | (xo << 1)
}

fn main() {
    let lis = |t: u32, v: u32| (15 << 26) | (t << 21) | (v & 0xFFFF);
    let ori = |a: u32, s: u32, v: u32| (24 << 26) | (s << 21) | (a << 16) | (v & 0xFFFF);
    let mut code = vec![
        lis(3, 0x8010),             // r3: the inputs
        lis(4, 0x8011),             // r4: the outputs
        psq(56, 4, 3, 16),          // constants f4, f5, f7
        psq(56, 5, 3, 24),
        psq(56, 7, 3, 32),
        lis(6, 0),
        ori(6, 6, 1000),
        (31 << 26) | (6 << 21) | (9 << 16) | (467 << 1), // mtctr r6
    ];
    let top = code.len();
    code.extend([
        psq(56, 1, 3, 0),           // psq_l f1, 0(r3)
        psq(56, 2, 3, 8),           // psq_l f2, 8(r3)
        psq(56, 3, 3, 40),          // psq_l f3, 40(r3)
        ps(29, 6, 1, 3, 4),         // ps_madd f6, f1, f4, f3
        ps(14, 8, 2, 6, 5),         // ps_madds0 f8, f2, f5, f6
        ps(10, 9, 8, 6, 8),         // ps_sum0 f9, f8, f8, f6
        ps(25, 10, 9, 0, 7),        // ps_mul f10, f9, f7
        fp59(29, 11, 1, 3, 2),      // fmadds f11, f1, f2, f3
        fp59(25, 12, 11, 0, 4),     // fmuls f12, f11, f4
        fp59(21, 13, 12, 1, 0),     // fadds f13, f12, f1
        psq(60, 10, 4, 0),          // psq_st f10, 0(r4)
        psq(60, 9, 4, 8),           // psq_st f9, 8(r4)
    ]);
    let back = -4 * (code.len() - top) as i32;
    code.push((16 << 26) | (16 << 21) | (back as u32 & 0xFFFC)); // bdnz top
    code.push((18 << 26) | ((-4 * code.len() as i32) as u32 & 0x03FF_FFFC)); // b start
    let bytes: Vec<u8> = code.iter().flat_map(|w| w.to_be_bytes()).collect();
    let floats: [f32; 12] = [0.5, 1.25, -0.75, 2.0, 0.875, -1.5, 0.25, 0.125, 1.0625, -0.5, 0.75, 0.3];
    let data: Vec<u8> = floats.iter().flat_map(|f| f.to_bits().to_be_bytes()).collect();
    // CGC_WRITE=path: also save it as a DOL (for the profiler).
    if let Ok(path) = std::env::var("CGC_WRITE") {
        let mut dol = vec![0u8; 0x100];
        let mut body: Vec<u8> = Vec::new();
        for (k, (addr, words)) in [(0x8000_3100u32, &bytes), (0x8010_0000, &data)].iter().enumerate() {
            let off = 0x100 + body.len() as u32;
            dol[k * 4..k * 4 + 4].copy_from_slice(&off.to_be_bytes());
            dol[0x48 + k * 4..0x4C + k * 4].copy_from_slice(&addr.to_be_bytes());
            dol[0x90 + k * 4..0x94 + k * 4].copy_from_slice(&(words.len() as u32).to_be_bytes());
            body.extend_from_slice(words);
        }
        dol[0xE0..0xE4].copy_from_slice(&0x8000_3100u32.to_be_bytes());
        dol.extend(body);
        std::fs::write(path, dol).expect("write");
    }
    let exe = Executable { entry: 0x8000_3100, sections: vec![Section { addr: 0x8000_3100, data: bytes }, Section { addr: 0x8010_0000, data }], bss: vec![] };
    let modes = std::env::var("CGC_MODES").unwrap_or_else(|_| "plain,fast,jit".into());
    for mode in modes.split(',') {
        let mut m = Machine::new(Console::GameCube);
        m.boot_executable(&exe);
        if mode == "plain" {
            m.fast = None;
        }
        if mode == "jit" && !m.enable_jit() {
            println!("jit: no code memory here");
            continue;
        }
        let t = std::time::Instant::now();
        let secs: f64 = std::env::var("CGC_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(3.0);
        while t.elapsed().as_secs_f64() < secs {
            m.run_field(m.cpu_hz / 60);
        }
        let s = t.elapsed().as_secs_f64();
        let out: Vec<u32> = (0..4).map(|k| m.hw.read32(0x11_0000 + 4 * k)).collect();
        println!("{mode}: {:.0} million instructions a second (results {out:08X?})", m.cpu.cycles as f64 / s / 1e6);
    }
}
