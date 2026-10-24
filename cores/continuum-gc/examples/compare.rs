//! Proves the fast interpreter identical to the plain one: runs the same program on both,
//! field by field, and compares the CPU registers, cycle counts, main memory and the picture.
//! cargo run --release --example compare -- path fields   (CGC_SD / CGC_ARGV / CGC_ENV as boot)
//! CGC_JIT=1 turns our JIT on in the fast one (on ARM64; on x86 build with --features unicorn).
use continuum_gc::libretro::open;
use continuum_gc::machine::Machine;
fn setup(path: &str) -> Machine {
    let mut m = open(path).expect("boots");
    if let Ok(d) = std::env::var("CGC_SD") {
        m.hw.sd = Some(continuum_gc::sd::SdCard::open(&d).expect("sd image"));
    }
    if let Ok(args) = std::env::var("CGC_ARGV") {
        let env = std::env::var("CGC_ENV").unwrap_or_default();
        let entry = m.cpu.pc;
        m.set_args(entry, &args.split(' ').collect::<Vec<_>>(), &env.split(' ').filter(|s| !s.is_empty()).collect::<Vec<_>>());
    }
    m
}
fn hash(b: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for c in b.chunks(8) {
        let mut w = [0u8; 8];
        w[..c.len()].copy_from_slice(c);
        h = (h ^ u64::from_le_bytes(w)).wrapping_mul(0x100000001b3);
    }
    h
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let fields: u32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(60);
    let mut plain = setup(&a[1]);
    plain.fast = None;
    let mut fast = setup(&a[1]);
    if std::env::var("CGC_JIT").is_ok() {
        assert!(fast.enable_jit(), "no JIT code memory here (on x86, build with --features unicorn)");
    }
    let (mut tp, mut tf) = (0.0, 0.0);
    for f in 0..fields {
        let t = std::time::Instant::now();
        plain.run_field(plain.cpu_hz / 60);
        tp += t.elapsed().as_secs_f64();
        let t = std::time::Instant::now();
        fast.run_field(fast.cpu_hz / 60);
        tf += t.elapsed().as_secs_f64();
        let pic = |m: &mut Machine| m.hw.vi_picture().map(|(_, _, px)| hash(&px.iter().flat_map(|p| p.to_le_bytes()).collect::<Vec<_>>()));
        let same_pic = pic(&mut plain) == pic(&mut fast);
        let (p, q) = (&plain.cpu, &fast.cpu);
        let same_regs = p.gpr == q.gpr && p.pc == q.pc && p.msr == q.msr && p.cr == q.cr && p.lr == q.lr && p.ctr == q.ctr && p.ps0 == q.ps0 && p.ps1 == q.ps1 && p.cycles == q.cycles && p.tb == q.tb && p.dec == q.dec;
        let same_mem = hash(&plain.hw.mem1) == hash(&fast.hw.mem1);
        if !(same_regs && same_mem && same_pic) {
            eprintln!("DIFFERENT after field {f}: registers same {same_regs}, memory same {same_mem}, picture same {same_pic}");
            eprintln!("plain pc {:08X} cycles {} / fast pc {:08X} cycles {}", p.pc, p.cycles, q.pc, q.cycles);
            for r in 0..32 { if p.gpr[r] != q.gpr[r] { eprintln!("  r{r}: {:08X} vs {:08X}", p.gpr[r], q.gpr[r]); } }
            std::process::exit(1);
        }
    }
    let st = &fast.fast.as_ref().unwrap().stats;
    eprintln!("IDENTICAL for {fields} fields ({} cycles). plain {tp:.1}s, fast {tf:.1}s ({:.2}x). blocks built {}, run {}, invalidations {}", fast.cpu.cycles, tp / tf, st.blocks_built, st.blocks_run, st.invalidations);
    eprintln!("msr {:08X} ibat0 {:08X?} dbat0 {:08X?} pc {:08X}", fast.cpu.msr, fast.cpu.ibat[0], fast.cpu.dbat[0], fast.cpu.pc);
    if let Some(j) = fast.fast.as_ref().unwrap().jit.as_ref() {
        eprintln!("JIT: {:?}, code {} KB", j.stats, j.used_bytes() / 1024);
    }
}
