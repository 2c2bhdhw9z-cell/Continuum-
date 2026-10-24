//! Interpreter speed on the console machine: a mixed loop (integer, load/store, compare,
//! branch) run by the plain and the fast interpreter, in millions of instructions a second.
use continuum_gc::bus::Bus;
use continuum_gc::disc::Console;
use continuum_gc::dol::{Executable, Section};
use continuum_gc::machine::Machine;
fn main() {
    // loop: addi r3,r3,1; rlwinm r4,r3,4,0,27; add r5,r4,r3; stw r5,0x100(r6); lwz r7,0x100(r6);
    //       cmpw r7,r5; addi r8,r8,2; xor r9,r8,r3; bne- out; b loop      (r6 = 0x80100000)
    let code: Vec<u32> = vec![0x3CC0_8010, 0x3863_0001, 0x5464_2036, 0x7CA4_1A14, 0x90A6_0100, 0x80E6_0100, 0x7C07_2800, 0x3908_0002, 0x7D09_1A78, 0x4082_0008, 0x4BFF_FFDC, 0x4800_0000];
    let bytes: Vec<u8> = code.iter().flat_map(|w| w.to_be_bytes()).collect();
    let exe = Executable { entry: 0x8000_3100, sections: vec![Section { addr: 0x8000_3100, data: bytes }], bss: vec![] };
    for fast in [false, true] {
        let mut m = Machine::new(Console::GameCube);
        m.boot_executable(&exe);
        if !fast { m.fast = None; }
        let t = std::time::Instant::now();
        let mut fields = 0;
        while t.elapsed().as_secs_f64() < 3.0 { m.run_field(m.cpu_hz / 60); fields += 1; }
        let s = t.elapsed().as_secs_f64();
        let _ = m.hw.read32(0);
        println!("{}: {:.0} million instructions a second ({} fields)", if fast { "fast " } else { "plain" }, m.cpu.cycles as f64 / s / 1e6, fields);
    }
}
