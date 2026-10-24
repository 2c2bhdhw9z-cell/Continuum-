//! Where a program's instructions go, to decide which interpreter and JIT improvements pay:
//! instruction mix, the commonest pairs of neighbouring instructions (candidates for fused
//! handlers), how well function returns and register jumps could be predicted (a return stack,
//! or a small per-branch memory of recent targets), where loads and stores go, and the guest
//! functions that run the most instructions (candidates for host replacements).
//!
//! cargo run --release --example hot -- path warm_fields instructions
//! (CGC_SD / CGC_ARGV / CGC_ENV as boot)
use continuum_gc::bus::Bus;
use continuum_gc::libretro::open;
use continuum_gc::machine::Machine;
use std::collections::HashMap;

fn name(i: u32) -> String {
    let op = i >> 26;
    let xo = (i >> 1) & 0x3FF;
    let (d, a, b) = ((i >> 21) & 31, (i >> 16) & 31, (i >> 11) & 31);
    let spr = ((i >> 16) & 31) | (((i >> 11) & 31) << 5);
    let s = match op {
        4 => return format!("ps/{}", (i >> 1) & 0x1F),
        7 => "mulli",
        8 => "subfic",
        10 => "cmplwi",
        11 => "cmpwi",
        12 => "addic",
        13 => "addic.",
        14 => if a == 0 { "li" } else { "addi" },
        15 => if a == 0 { "lis" } else { "addis" },
        16 => if d & 0x14 == 0x14 { "bc-always" } else if d & 4 == 0 { "bdnz/bdz" } else { "bc" },
        17 => "sc",
        18 => if i & 1 != 0 { "bl" } else { "b" },
        19 => match xo {
            16 => if i & 1 != 0 { "blrl" } else if d & 0x14 == 0x14 { "blr" } else { "bclr-cond" },
            528 => if i & 1 != 0 { "bctrl" } else { "bctr" },
            50 => "rfi",
            150 => "isync",
            _ => "cr-logic",
        },
        20 => "rlwimi",
        21 => "rlwinm",
        23 => "rlwnm",
        24 => if i == 0x6000_0000 { "nop" } else { "ori" },
        25 => "oris",
        26 => "xori",
        27 => "xoris",
        28 => "andi.",
        29 => "andis.",
        32 => "lwz",
        33 => "lwzu",
        34 => "lbz",
        35 => "lbzu",
        36 => "stw",
        37 => "stwu",
        38 => "stb",
        39 => "stbu",
        40 => "lhz",
        41 => "lhzu",
        42 => "lha",
        43 => "lhau",
        44 => "sth",
        45 => "sthu",
        46 => "lmw",
        47 => "stmw",
        48 => "lfs",
        49 => "lfsu",
        50 => "lfd",
        51 => "lfdu",
        52 => "stfs",
        53 => "stfsu",
        54 => "stfd",
        55 => "stfdu",
        56 => "psq_l",
        57 => "psq_lu",
        59 => return format!("fp59/{}", (i >> 1) & 0x1F),
        60 => "psq_st",
        61 => "psq_stu",
        63 => return format!("fp63/{}", if (i >> 1) & 0x10 != 0 { (i >> 1) & 0x1F } else { xo }),
        31 => match xo {
            0 => "cmpw",
            32 => "cmplw",
            8 => "subfc",
            10 => "addc",
            11 => "mulhwu",
            19 => "mfcr",
            23 => "lwzx",
            24 => "slw",
            26 => "cntlzw",
            28 => "and",
            40 => "subf",
            60 => "andc",
            75 => "mulhw",
            83 => "mfmsr",
            86 => "dcbf",
            87 => "lbzx",
            104 => "neg",
            124 => "nor",
            136 => "subfe",
            138 => "adde",
            144 => "mtcrf",
            146 => "mtmsr",
            151 => "stwx",
            200 => "subfze",
            202 => "addze",
            215 => "stbx",
            235 => "mullw",
            266 => "add",
            279 => "lhzx",
            284 => "eqv",
            316 => "xor",
            339 => match spr { 8 => "mflr", 9 => "mfctr", 1 => "mfxer", 268 | 269 => "mftb", _ => "mfspr" },
            343 => "lhax",
            371 => "mftb",
            407 => "sthx",
            412 => "orc",
            444 => if d == b { "mr" } else { "or" },
            459 => "divwu",
            467 => match spr { 8 => "mtlr", 9 => "mtctr", 1 => "mtxer", _ => "mtspr" },
            470 => "dcbi",
            476 => "nand",
            491 => "divw",
            535 => "lfsx",
            536 => "srw",
            598 => "sync",
            599 => "lfdx",
            663 => "stfsx",
            727 => "stfdx",
            792 => "sraw",
            824 => "srawi",
            854 => "eieio",
            922 => "extsh",
            954 => "extsb",
            982 => "icbi",
            1014 => "dcbz",
            _ => return format!("31/{xo}"),
        },
        _ => return format!("op{op}"),
    };
    s.to_string()
}

/// The effective address of a load or store, or None.
fn data_ea(m: &Machine, i: u32) -> Option<u32> {
    let op = i >> 26;
    let (a, b) = (((i >> 16) & 31) as usize, ((i >> 11) & 31) as usize);
    let base = |upd: bool| if a == 0 && !upd { 0 } else { m.cpu.gpr[a] };
    match op {
        32..=55 => Some(base(op & 1 == 1).wrapping_add((i & 0xFFFF) as i16 as i32 as u32)),
        56 | 57 | 60 | 61 => Some(base(op & 1 == 1).wrapping_add((((i & 0xFFF) << 20) as i32 >> 20) as u32)),
        31 => {
            let xo = (i >> 1) & 0x3FF;
            matches!(xo, 23 | 55 | 87 | 119 | 151 | 183 | 215 | 247 | 279 | 311 | 343 | 375 | 407 | 439 | 535 | 567 | 599 | 631 | 663 | 695 | 727 | 759 | 983 | 1014)
                .then(|| base(matches!(xo, 55 | 119 | 183 | 247 | 311 | 375 | 439 | 567 | 631 | 695 | 759)).wrapping_add(m.cpu.gpr[b]))
        }
        _ => None,
    }
}

fn top<K: Clone>(map: &HashMap<K, u64>, n: usize) -> Vec<(K, u64)> {
    let mut v: Vec<(K, u64)> = map.iter().map(|(k, c)| (k.clone(), *c)).collect();
    v.sort_by(|x, y| y.1.cmp(&x.1));
    v.truncate(n);
    v
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut m = open(&a[1]).expect("boots");
    if let Ok(d) = std::env::var("CGC_SD") {
        m.hw.sd = Some(continuum_gc::sd::SdCard::open(&d).expect("sd image"));
    }
    if let Ok(args) = std::env::var("CGC_ARGV") {
        let env = std::env::var("CGC_ENV").unwrap_or_default();
        let entry = m.cpu.pc;
        m.set_args(entry, &args.split(' ').collect::<Vec<_>>(), &env.split(' ').filter(|s| !s.is_empty()).collect::<Vec<_>>());
    }
    let warm: u32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(60);
    let total: u64 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(200_000_000);
    for _ in 0..warm {
        m.run_field(m.cpu_hz / 60);
    }
    let per_line = m.cycles_per_line();
    let mut line_cycles = 0u64;

    let mut kinds: HashMap<String, u64> = HashMap::new();
    let mut pairs: HashMap<(String, String), u64> = HashMap::new();
    let mut funcs: HashMap<u32, u64> = HashMap::new();
    let mut buckets: HashMap<u32, u64> = HashMap::new();
    // Return prediction: a shadow return stack, and per-branch memories of the last 1 and 2
    // targets (what the fast interpreter's block links are).
    let mut stack: Vec<(u32, u32)> = Vec::new();
    let (mut returns, mut rsb_hits) = (0u64, 0u64);
    let mut last_targets: HashMap<u32, [u32; 2]> = HashMap::new();
    let (mut indirect, mut hit1, mut hit2) = (0u64, 0u64, 0u64);
    let (mut mem_ops, mut mem_window, mut mem_mmio, mut mem_other) = (0u64, 0u64, 0u64, 0u64);
    let mut mem_mb = [0u64; 32];
    let (mut idle_cycles, mut ran) = (0u64, 0u64);
    let mut prev: Option<(u32, String)> = None;
    let mut func: u32 = 0;
    while ran < total {
        if m.hw.irq_dirty {
            m.cpu.external_line = m.hw.irq();
            m.hw.irq_dirty = false;
        }
        if m.cpu.msr & 0x0004_0000 != 0 && !(m.cpu.msr & 0x8000 != 0 && (m.cpu.external_line || m.cpu.dec_pending)) {
            // Asleep: jump to the next event, as the machine does.
            let skip = (per_line - line_cycles.min(per_line - 1)).min(m.cpu.cycles_to_decrementer()).max(1);
            m.cpu.advance_time(skip);
            idle_cycles += skip;
            line_cycles += skip;
        } else {
            let pc = m.cpu.pc;
            let pa = if m.cpu.msr & 0x20 != 0 && pc >= 0x8000_0000 { pc & 0x1FFF_FFFF } else { pc };
            let i = m.hw.read32(pa);
            let k = name(i);
            *kinds.entry(k.clone()).or_default() += 1;
            *funcs.entry(func).or_default() += 1;
            *buckets.entry(pc & !0x3F).or_default() += 1;
            if let Some((ppc, pk)) = &prev {
                if *ppc == pc.wrapping_sub(4) {
                    *pairs.entry((pk.clone(), k.clone())).or_default() += 1;
                }
            }
            if let Some(ea) = data_ea(&m, i) {
                mem_ops += 1;
                if m.cpu.msr & 0x10 != 0 && ea.wrapping_sub(0x8000_0000) < 0x0180_0000 {
                    mem_window += 1;
                    mem_mb[(ea.wrapping_sub(0x8000_0000) >> 20) as usize] += 1;
                } else if ea >> 24 == 0xCC {
                    mem_mmio += 1;
                } else {
                    mem_other += 1;
                }
            }
            let c = m.cpu.step(&mut m.hw);
            let next = m.cpu.pc;
            let op = i >> 26;
            let xo = (i >> 1) & 0x3FF;
            let link = matches!(op, 16 | 18) && i & 1 != 0 || op == 19 && matches!(xo, 16 | 528) && i & 1 != 0;
            let is_blr = op == 19 && xo == 16;
            let is_reg_jump = op == 19 && matches!(xo, 16 | 528);
            if is_reg_jump && next != pc.wrapping_add(4) {
                indirect += 1;
                let e = last_targets.entry(pc).or_insert([0, 0]);
                if e[0] == next {
                    hit1 += 1;
                    hit2 += 1;
                } else if e[1] == next {
                    hit2 += 1;
                    *e = [next, e[0]];
                } else {
                    *e = [next, e[0]];
                }
            }
            if is_blr && !link && next != pc.wrapping_add(4) {
                returns += 1;
                if stack.last().map(|e| e.0) == Some(next) {
                    rsb_hits += 1;
                    func = stack.pop().unwrap().1;
                } else {
                    stack.clear();
                    func = 0;
                }
            }
            if link && next != pc.wrapping_add(4) {
                stack.push((pc.wrapping_add(4), func));
                if stack.len() > 64 {
                    stack.remove(0);
                }
                func = next;
            }
            prev = Some((pc, k));
            ran += 1;
            line_cycles += c;
        }
        if line_cycles >= per_line {
            line_cycles -= per_line;
            m.hw.vi_line();
            m.hw.ai_samples(3);
            m.hw.audio_line();
        }
    }
    let pct = |x: u64| 100.0 * x as f64 / ran as f64;
    println!("{ran} instructions run ({idle_cycles} cycles asleep)");
    println!("\ninstruction mix (top 40):");
    for (k, c) in top(&kinds, 40) {
        println!("  {:6.2}%  {k}", pct(c));
    }
    println!("\nneighbouring pairs (top 40):");
    for ((x, y), c) in top(&pairs, 40) {
        println!("  {:6.2}%  {x} + {y}", pct(c));
    }
    println!("\nloads and stores: {:.1}% of instructions; in main memory through the standard mapping {:.1}%, hardware registers {:.1}%, elsewhere {:.1}%", pct(mem_ops), 100.0 * mem_window as f64 / mem_ops.max(1) as f64, 100.0 * mem_mmio as f64 / mem_ops.max(1) as f64, 100.0 * mem_other as f64 / mem_ops.max(1) as f64);
    println!("  by megabyte of main memory: {:?}", mem_mb.iter().take(24).map(|c| format!("{:.1}", 100.0 * *c as f64 / mem_ops.max(1) as f64)).collect::<Vec<_>>());
    println!("  data BATs: {:08X?}  instruction BATs: {:08X?}", &m.cpu.dbat[..4], &m.cpu.ibat[..4]);
    println!("\nregister jumps taken (blr, bctr...): {:.2}% of instructions; the same target as last time at that branch {:.1}%, one of the last two {:.1}%", pct(indirect), 100.0 * hit1 as f64 / indirect.max(1) as f64, 100.0 * hit2 as f64 / indirect.max(1) as f64);
    println!("returns: {:.2}% of instructions; predicted by a return stack {:.1}%", pct(returns), 100.0 * rsb_hits as f64 / returns.max(1) as f64);
    println!("\nhottest code (64-byte pieces):");
    for (b, c) in top(&buckets, 20) {
        println!("  {:6.2}%  {b:08X}", pct(c));
    }
    println!("\nfunctions running the most instructions (by call target; 0 = outside any call seen):");
    for (f, c) in top(&funcs, 15) {
        println!("  {:6.2}%  {f:08X}", pct(c));
    }
}
