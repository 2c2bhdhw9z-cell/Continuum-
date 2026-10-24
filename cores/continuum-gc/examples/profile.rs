//! Development profiler: boots a program, runs N fields under a sampling profiler and prints
//! the functions where the time goes (self time and inclusive time).
//! cargo run --profile profiling --example profile -- path fields  (CGC_SD / CGC_ARGV / CGC_ENV as boot)
use continuum_gc::libretro::open;
use std::collections::HashMap;
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
    let warm: u32 = std::env::var("CGC_WARM").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    for _ in 0..warm { m.run_field(m.cpu_hz / 60); }
    let fields: u32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(120);
    let guard = pprof::ProfilerGuardBuilder::default().frequency(1000).build().unwrap();
    let t = std::time::Instant::now();
    for _ in 0..fields { m.run_field(m.cpu_hz / 60); }
    eprintln!("{fields} fields in {:.1}s", t.elapsed().as_secs_f64());
    let report = guard.report().build().unwrap();
    let mut selft: HashMap<String, usize> = HashMap::new();
    let mut incl: HashMap<String, usize> = HashMap::new();
    let mut total = 0;
    for (frames, count) in report.data.iter() {
        let count = *count as usize;
        total += count;
        let names: Vec<String> = frames.frames.iter().flat_map(|f| f.iter().map(|s| s.name())).collect();
        if let Some(first) = names.first() { *selft.entry(first.clone()).or_default() += count; }
        let mut seen = std::collections::HashSet::new();
        for n in &names { if seen.insert(n.clone()) { *incl.entry(n.clone()).or_default() += count; } }
    }
    let short = |s: &str| { let s = s.replace("continuum_gc::", ""); if s.len() > 110 { s[..110].to_string() } else { s } };
    let mut v: Vec<_> = selft.into_iter().collect(); v.sort_by_key(|x| std::cmp::Reverse(x.1));
    eprintln!("self time ({total} samples):");
    for (n, c) in v.iter().take(28) { eprintln!("  {:5.1}%  {}", *c as f64 * 100.0 / total as f64, short(n)); }
    let mut v: Vec<_> = incl.into_iter().collect(); v.sort_by_key(|x| std::cmp::Reverse(x.1));
    eprintln!("inclusive:");
    for (n, c) in v.iter().filter(|(n, _)| n.contains("continuum_gc")).take(25) { eprintln!("  {:5.1}%  {}", *c as f64 * 100.0 / total as f64, short(n)); }
}
