//! Development tracer: boots, runs `skip` instructions, then prints the next `n` PCs and the
//! DSP registers. cargo run --release --example trace -- path skip n
use continuum_gc::libretro::open;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut m = open(&a[1]).unwrap();
    let skip: u64 = a[2].parse().unwrap();
    let n: u64 = a[3].parse().unwrap();
    for _ in 0..skip { m.cpu.external_line = m.hw.irq(); m.cpu.step(&mut m.hw); }
    for _ in 0..n {
        let pc = m.cpu.pc;
        m.cpu.external_line = m.hw.irq();
        m.cpu.step(&mut m.hw);
        println!("{pc:08X} r8={:08X} r9={:08X} csr={:04X} mbox={:08X} full={}", m.cpu.gpr[8], m.cpu.gpr[9], m.hw.dsp.csr, m.hw.dsp.dsp_mbox, m.hw.dsp.dsp_mbox_full);
    }
}
