//! How long turning the TV picture (the XFB, in YUV) into the frontend's RGB picture takes, per
//! field, for a 640x480 interlaced picture. cargo run --release --example vibench
use continuum_gc::bus::Bus;
use continuum_gc::disc::Console;
use continuum_gc::machine::Machine;
fn main() {
    let mut m = Machine::new(Console::GameCube);
    // 640 wide, 240 lines a field, interlaced, both fields' framebuffers set.
    m.hw.write16(0x0C00_2000, (240 << 4) | 6);
    m.hw.write16(0x0C00_2048, (40 << 8) | 80);
    m.hw.write32(0x0C00_201C, 0x1000_0000 | (0x10_0000 >> 5));
    m.hw.write32(0x0C00_2024, 0x1000_0000 | (0x10_0500 >> 5));
    m.hw.write16(0x0C00_2002, 1);
    for (k, b) in m.hw.mem1[0x10_0000..0x10_0000 + 640 * 480 * 2].iter_mut().enumerate() {
        *b = (k * 7) as u8;
    }
    let t = std::time::Instant::now();
    let n = 300;
    let mut sum = 0u64;
    for _ in 0..n {
        let (w, h, px) = m.hw.vi_picture().unwrap();
        sum += px[(w * h / 2) as usize] as u64;
    }
    println!("{}x? picture: {:.2} ms a field ({sum})", 640, t.elapsed().as_secs_f64() * 1000.0 / n as f64);
}
