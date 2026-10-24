//! Development profiler: boots a program, runs N fields under a sampling profiler and prints
//! the functions where the time goes (self time and inclusive time).
//! cargo run --profile profiling --example profile -- path fields  (CGC_SD / CGC_ARGV / CGC_ENV as boot)
#[cfg(not(target_arch = "x86_64"))]
fn main() {
    eprintln!("the profiler runs on the x86 test machine only");
}
#[cfg(target_arch = "x86_64")]
use continuum_gc::libretro::open;
#[cfg(target_arch = "x86_64")]
use std::collections::HashMap;
#[cfg(target_arch = "x86_64")]
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
    // For the hottest leaves, the first frame of ours above them (where inlined helpers live).
    let mut parents: HashMap<(String, String), usize> = HashMap::new();
    let mut incl: HashMap<String, usize> = HashMap::new();
    let mut total = 0;
    for (frames, count) in report.data.iter() {
        let count = *count as usize;
        total += count;
        let names: Vec<String> = frames.frames.iter().flat_map(|f| f.iter().map(|s| s.name())).collect();
        if let Some(first) = names.first() {
            *selft.entry(first.clone()).or_default() += count;
            if let Some(p) = names.iter().skip(1).find(|n| n.contains("continuum_gc") && !n.contains("{{closure}}")) {
                *parents.entry((first.clone(), p.clone())).or_default() += count;
            }
        }
        let mut seen = std::collections::HashSet::new();
        for n in &names { if seen.insert(n.clone()) { *incl.entry(n.clone()).or_default() += count; } }
    }
    let short = |s: &str| { let s = s.replace("continuum_gc::", ""); if s.len() > 110 { s[..110].to_string() } else { s } };
    let mut v: Vec<_> = selft.into_iter().collect(); v.sort_by_key(|x| std::cmp::Reverse(x.1));
    eprintln!("self time ({total} samples):");
    for (n, c) in v.iter().take(28) { eprintln!("  {:5.1}%  {}", *c as f64 * 100.0 / total as f64, short(n)); }
    // CGC_LINES=name: self time by source line for leaf frames whose function name has `name`.
    if let Ok(want) = std::env::var("CGC_LINES") {
        let mut lines: HashMap<String, usize> = HashMap::new();
        let mut in_fn = 0;
        for (frames, count) in report.data.iter() {
            let Some(leaf) = frames.frames.first() else { continue };
            // The innermost symbol of the leaf frame (inlined code keeps its own line).
            if let Some(sym) = leaf.first() {
                let outer = leaf.last().map(|s| s.name()).unwrap_or_default();
                if outer.contains(&want) || sym.name().contains(&want) {
                    in_fn += *count as usize;
                    // Every symbol at the address (inlined functions and the one they are in).
                    let key = leaf.iter().map(|s| {
                        let file = s.filename.as_ref().and_then(|f| f.file_name()).map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
                        let name = short(&s.name());
                        let name = name.rsplit("::").next().unwrap_or("").to_string();
                        format!("{name}@{file}:{}", s.lineno.unwrap_or(0))
                    }).collect::<Vec<_>>().join(" < ");
                    *lines.entry(key).or_default() += *count as usize;
                }
            }
        }
        let mut lv: Vec<_> = lines.into_iter().collect(); lv.sort_by_key(|x| std::cmp::Reverse(x.1));
        eprintln!("lines in {want} ({:.1}% of all samples):", in_fn as f64 * 100.0 / total as f64);
        for (k, c) in lv.iter().take(40) { eprintln!("  {:5.1}%  {k}", *c as f64 * 100.0 / total as f64); }
    }
    let mut pv: Vec<_> = parents.into_iter().collect(); pv.sort_by_key(|x| std::cmp::Reverse(x.1));
    eprintln!("hottest leaves by caller:");
    for ((leaf, p), c) in pv.iter().take(16) { eprintln!("  {:5.1}%  {}  <-  {}", *c as f64 * 100.0 / total as f64, short(leaf), short(p)); }
    let mut v: Vec<_> = incl.into_iter().collect(); v.sort_by_key(|x| std::cmp::Reverse(x.1));
    eprintln!("inclusive:");
    for (n, c) in v.iter().filter(|(n, _)| n.contains("continuum_gc")).take(25) { eprintln!("  {:5.1}%  {}", *c as f64 * 100.0 / total as f64, short(n)); }
}
