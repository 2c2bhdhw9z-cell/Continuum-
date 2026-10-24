//! The drawer against itself: records, field by field, fingerprints of the colour buffer, the
//! depth buffer and the picture shown, so that a faster drawer can be proved to draw exactly
//! the same thing. Also reports how long drawing took.
//!
//! cargo run --release --example drawcheck -- path fields record|check file
//! (CGC_SD / CGC_ARGV / CGC_ENV as boot)
use continuum_gc::libretro::open;
use std::fmt::Write as _;

fn hash<T: Copy>(v: &[T]) -> u64 {
    // SAFETY: plain values viewed as bytes.
    let b = unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) };
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for c in b.chunks(8) {
        let mut w = [0u8; 8];
        w[..c.len()].copy_from_slice(c);
        h = (h ^ u64::from_le_bytes(w)).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let fields: u32 = a[2].parse().expect("fields");
    let mode = a[3].as_str();
    let file = &a[4];
    let mut m = open(&a[1]).expect("boots");
    if let Ok(d) = std::env::var("CGC_SD") {
        m.hw.sd = Some(continuum_gc::sd::SdCard::open(&d).expect("sd image"));
    }
    if let Ok(args) = std::env::var("CGC_ARGV") {
        let env = std::env::var("CGC_ENV").unwrap_or_default();
        let entry = m.cpu.pc;
        m.set_args(entry, &args.split(' ').collect::<Vec<_>>(), &env.split(' ').filter(|s| !s.is_empty()).collect::<Vec<_>>());
    }
    let expected: Vec<String> = if mode == "check" { std::fs::read_to_string(file).expect("recorded file").lines().map(String::from).collect() } else { Vec::new() };
    let mut out = String::new();
    let t = std::time::Instant::now();
    let mut differences = 0;
    for f in 0..fields {
        m.run_field(m.cpu_hz / 60);
        let pic = m.hw.vi_picture().map(|(_, _, px)| hash(&px)).unwrap_or(0);
        let line = format!("{f} {:016x} {:016x} {pic:016x}", hash(&m.hw.gx.efb_color), hash(&m.hw.gx.efb_depth));
        if mode == "check" {
            if expected.get(f as usize) != Some(&line) {
                if differences == 0 {
                    eprintln!("DIFFERENT at field {f}:\n  now  {line}\n  was  {}", expected.get(f as usize).map_or("(nothing)", |s| s.as_str()));
                }
                differences += 1;
            }
        } else {
            writeln!(out, "{line}").unwrap();
        }
    }
    let total = t.elapsed().as_secs_f64();
    let draw = m.hw.gx.stats.draw_time.as_secs_f64();
    let st = &m.hw.gx.stats;
    eprintln!("{fields} fields in {total:.2}s; drawing {draw:.2}s ({:.0}%); {} triangles, {} pixels", 100.0 * draw / total, st.triangles, st.pixels);
    if mode == "check" {
        if differences == 0 {
            eprintln!("SAME as recorded for all {fields} fields");
        } else {
            eprintln!("{differences} fields differ");
            std::process::exit(1);
        }
    } else {
        std::fs::write(file, out).expect("write");
        eprintln!("recorded to {file}");
    }
}
