//! Boots, runs until the CPU first reaches `stop_pc` (or `limit` instructions), dumps code at
//! `addr` and the registers. cargo run --release --example dis_at -- path addr stop_pc limit
use continuum_gc::bus::Bus;
use continuum_gc::libretro::open;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut m = open(&a[1]).unwrap();
    let hex = |s: &str| u32::from_str_radix(s.trim_start_matches("0x"), 16).unwrap();
    let addr = hex(&a[2]);
    let stop = hex(&a[3]);
    let limit: u64 = a[4].parse().unwrap();
    let mut hist = std::collections::VecDeque::new();
    let mut prev = m.cpu.clone();
    for _ in 0..limit {
        if m.cpu.pc == stop || !m.cpu.fault_log.is_empty() { break; }
        prev = m.cpu.clone();
        hist.push_back(m.cpu.pc);
        if hist.len() > 40 { hist.pop_front(); }
        m.cpu.external_line = m.hw.irq();
        m.cpu.step(&mut m.hw);
    }
    eprintln!("last PCs: {:08X?}", hist);
    eprintln!("before the last instruction: gpr {:08X?}", prev.gpr);
    eprintln!("fault log {:08X?}", m.cpu.fault_log);
    eprintln!("gpr: {:08X?}\nlr {:08X} ctr {:08X} cr {:08X}", m.cpu.gpr, m.cpu.lr, m.cpu.ctr, m.cpu.cr);
    let mut dump = format!("# at {addr:08X}\n");
    for k in 0..32u32 {
        let w = m.hw.read32(((addr & 0x01FF_FFFF) - 0x40) + k * 4);
        dump.push_str(&format!("{:#04x} {:#04x} {:#04x} {:#04x}\n", w >> 24, (w >> 16) & 0xFF, (w >> 8) & 0xFF, w & 0xFF));
    }
    std::fs::write("/tmp/gc-dump.txt", dump).unwrap();
}
