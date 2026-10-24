//! Development aid: draws the no-picture status screen for a program after some fields.
//! cargo run --release --example notice_shot -- path fields out.ppm
use continuum_gc::libretro::open;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut m = open(&a[1]).expect("boots");
    let fields: u32 = a[2].parse().unwrap();
    for _ in 0..fields {
        m.run_field(m.cpu_hz / 60);
    }
    let lines = m.status_lines(15, &[(m.cpu.pc & !0xFF, fields)]);
    let mut f = vec![0u32; 640 * 480];
    continuum_gc::notice::draw(&mut f, 640, 480, &lines);
    let mut ppm = b"P6 640 480 255\n".to_vec();
    for p in f { ppm.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]); }
    std::fs::write(&a[3], ppm).unwrap();
    for l in lines { println!("{l}"); }
}
