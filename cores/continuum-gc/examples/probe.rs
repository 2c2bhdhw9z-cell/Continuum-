//! Development probe: boots a program and runs it instruction by instruction, with stops and a
//! trace that shows what each instruction changed.
//!
//! cargo run --release --example probe -- path [options]
//!   --max N        give up after N instructions (default 2 billion)
//!   --break PC     stop the K-th time the CPU reaches PC (hex)  [--hits K, default 1]
//!   --fault        stop at the first DSI/ISI/program/alignment fault
//!   --trace N      after stopping (or from the start with --trace-from-start), trace N
//!                  instructions: PC, instruction word, registers it changed
//!   --mem ADDR LEN dump memory (effective address, hex) when done
//!   --backtrace    walk the stack's back chain and print the return addresses
use continuum_gc::bus::Bus;
use continuum_gc::libretro::open;

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s.trim_start_matches("0x"), 16).expect("hex number")
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut m = open(&a[1]).expect("boots");
    let mut max: u64 = 2_000_000_000;
    let mut brk: Option<u32> = None;
    let mut hits = 1u32;
    let mut fault = false;
    let mut trace = 0u64;
    let mut trace_from_start = false;
    let mut mems: Vec<(u32, u32)> = Vec::new();
    let mut backtrace = false;
    let mut i = 2;
    while i < a.len() {
        match a[i].as_str() {
            "--max" => { max = a[i + 1].parse().unwrap(); i += 1; }
            "--break" => { brk = Some(hex(&a[i + 1])); i += 1; }
            "--hits" => { hits = a[i + 1].parse().unwrap(); i += 1; }
            "--fault" => fault = true,
            "--trace" => { trace = a[i + 1].parse().unwrap(); i += 1; }
            "--trace-from-start" => trace_from_start = true,
            "--mem" => { mems.push((hex(&a[i + 1]), hex(&a[i + 2]))); i += 2; }
            "--backtrace" => backtrace = true,
            o => panic!("unknown option {o}"),
        }
        i += 1;
    }
    let per_line = m.cycles_per_line();
    let mut line_cycles = 0u64;
    let mut step = |m: &mut continuum_gc::machine::Machine| {
        m.cpu.external_line = m.hw.irq();
        let c = m.cpu.step(&mut m.hw);
        line_cycles += c;
        if line_cycles >= per_line {
            line_cycles -= per_line;
            m.hw.vi_line();
            m.hw.ai_samples(3);
        }
    };
    let mut ran = 0u64;
    let mut seen = 0u32;
    if !trace_from_start {
        while ran < max {
            if Some(m.cpu.pc) == brk {
                seen += 1;
                if seen >= hits { break; }
            }
            if fault && !m.cpu.fault_log.is_empty() { break; }
            step(&mut m);
            ran += 1;
        }
        eprintln!("stopped after {ran} instructions at {:08X}", m.cpu.pc);
    }
    eprintln!("gpr: {:08X?}", m.cpu.gpr);
    eprintln!("lr {:08X} ctr {:08X} cr {:08X} msr {:08X} srr0 {:08X} srr1 {:08X} dar {:08X} xer {:08X}", m.cpu.lr, m.cpu.ctr, m.cpu.cr, m.cpu.msr, m.cpu.srr0, m.cpu.srr1, m.cpu.dar, m.cpu.xer);
    eprintln!("faults: {:08X?}", m.cpu.fault_log);
    let rd = |m: &mut continuum_gc::machine::Machine, ea: u32| -> u32 {
        let pa = if ea >= 0x8000_0000 { ea & 0x1FFF_FFFF } else { ea };
        m.hw.read32(pa)
    };
    if backtrace {
        let mut sp = m.cpu.gpr[1];
        eprint!("back chain:");
        for _ in 0..24 {
            let next = rd(&mut m, sp);
            if next == 0 || next == 0xFFFF_FFFF { break; }
            let lr = rd(&mut m, next + 4);
            eprint!(" {lr:08X}");
            sp = next;
        }
        eprintln!();
    }
    for _ in 0..trace {
        let pc = m.cpu.pc;
        let pa = if m.cpu.msr & 0x20 != 0 && pc >= 0x8000_0000 { pc & 0x1FFF_FFFF } else { pc };
        let inst = m.hw.read32(pa);
        let before = m.cpu.gpr;
        let (lr, ctr, cr) = (m.cpu.lr, m.cpu.ctr, m.cpu.cr);
        step(&mut m);
        let mut ch = String::new();
        for r in 0..32 {
            if m.cpu.gpr[r] != before[r] { ch.push_str(&format!(" r{r}={:08X}", m.cpu.gpr[r])); }
        }
        if m.cpu.lr != lr { ch.push_str(&format!(" lr={:08X}", m.cpu.lr)); }
        if m.cpu.ctr != ctr { ch.push_str(&format!(" ctr={:08X}", m.cpu.ctr)); }
        if m.cpu.cr != cr { ch.push_str(&format!(" cr={:08X}", m.cpu.cr)); }
        println!("{pc:08X} {inst:08X}{ch}");
    }
    for (addr, len) in mems {
        for row in (0..len).step_by(16) {
            let mut s = format!("{:08X}:", addr + row);
            for k in (0..16).step_by(4) { s.push_str(&format!(" {:08X}", rd(&mut m, addr + row + k))); }
            println!("{s}");
        }
    }
}
