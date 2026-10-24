//! Plain-interpreter speed on the host: a tight integer loop, in millions of instructions a second.
use continuum_gc::bus::{Bus, FlatRam};
use continuum_gc::cpu::Cpu;
fn main() {
    let mut ram = FlatRam::new(0x10000);
    // loop: addi r3,r3,1; rlwinm r4,r3,4,0,27; add r5,r4,r3; stw r5,0x100(0); lwz r6,0x100(0); b loop
    for (k, w) in [0x3863_0001u32, 0x5464_2036, 0x7CA4_1A14, 0x90A0_0100, 0x80C0_0100, 0x4BFF_FFEC].iter().enumerate() {
        ram.write32(k as u32 * 4, *w);
    }
    let mut cpu = Cpu::default();
    cpu.pc = 0;
    cpu.msr = 0;
    let n = 50_000_000u64;
    let t = std::time::Instant::now();
    cpu.run(&mut ram, n);
    let s = t.elapsed().as_secs_f64();
    println!("{:.0} million instructions a second", n as f64 / s / 1e6);
}
