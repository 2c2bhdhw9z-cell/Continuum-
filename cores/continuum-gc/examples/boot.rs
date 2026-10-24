//! Development runner: boots a DOL/ELF/ISO, runs some fields, and reports what happened.
//! cargo run --release --example boot -- path [fields] [out.ppm]
use continuum_gc::libretro::open;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut m = open(&args[1]).expect("boots");
    let fields: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(120);
    let t = std::time::Instant::now();
    let mut pcs = std::collections::HashMap::<u32, u32>::new();
    let mut causes = [0u64; 32];
    for f in 0..fields {
        m.run_field(m.cpu_hz / 60);
        let c = m.hw.cause() & m.hw.pi.mask;
        for (b, n) in causes.iter_mut().enumerate() { if c & (1 << b) != 0 { *n += 1; } }
        *pcs.entry(m.cpu.pc & !0xFF).or_default() += 1;
        if f % 30 == 29 {
            eprintln!("field {f}: pc {:08X} lr {:08X} msr {:08X}", m.cpu.pc, m.cpu.lr, m.cpu.msr);
        }
    }
    eprintln!("pending causes at field ends (bit: fields): {:?} mask {:08X}", causes.iter().enumerate().filter(|(_, n)| **n > 0).collect::<Vec<_>>(), m.hw.pi.mask);
    eprintln!("dsp csr {:04X} ai cr {:08X} di sr {:08X} si comcsr {:08X} exi {:08X?}", m.hw.dsp.csr, m.hw.ai.cr, m.hw.di.sr, m.hw.si.comcsr, m.hw.exi.ch.iter().map(|c| c.csr).collect::<Vec<_>>());
    eprintln!("{fields} fields in {:.1}s", t.elapsed().as_secs_f64());
    eprintln!("exceptions taken: {:?}", m.cpu.exceptions.iter().enumerate().filter(|(_, n)| **n > 0).map(|(v, n)| (format!("{:X}00", v), *n)).collect::<Vec<_>>());
    eprintln!("faults (vector, at, address): {:08X?}", m.cpu.fault_log);
    eprintln!("unknown registers: {:X?}", m.hw.unknown.iter().take(40).collect::<Vec<_>>());
    eprintln!("gx fifo bytes waiting: {}", m.hw.gx_fifo.len());
    eprintln!("uart: {}", String::from_utf8_lossy(&m.hw.uart));
    let mut p: Vec<_> = pcs.into_iter().collect();
    p.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    eprintln!("where it was at field ends: {:X?}", &p[..p.len().min(8)]);
    // Instructions around the PC and LR, for llvm-mc.
    use continuum_gc::bus::Bus;
    let mut dump = String::new();
    for (name, at) in [("pc", m.cpu.pc), ("lr", m.cpu.lr)] {
        let base = (at & 0x01FF_FFFF).saturating_sub(0x40);
        dump.push_str(&format!("# {name} {at:08X}\n"));
        for k in 0..32u32 {
            let w = m.hw.read32(base + k * 4);
            dump.push_str(&format!("{:#04x} {:#04x} {:#04x} {:#04x}\n", w >> 24, (w >> 16) & 0xFF, (w >> 8) & 0xFF, w & 0xFF));
        }
    }
    std::fs::write("/tmp/gc-dump.txt", dump).unwrap();
    eprintln!("gprs: {:08X?}", &m.cpu.gpr[..16]);
    if let Some(out) = args.get(3) {
        if let Some((w, h, px)) = m.hw.vi_picture() {
            let mut ppm = format!("P6 {w} {h} 255\n").into_bytes();
            for p in px { ppm.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]); }
            std::fs::write(out, ppm).unwrap();
            eprintln!("picture {w}x{h} written");
        } else { eprintln!("no picture"); }
    }
}
