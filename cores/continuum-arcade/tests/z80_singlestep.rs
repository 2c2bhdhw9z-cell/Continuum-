//! SingleStepTests' Z80 suite (MIT): 1604 files, one per opcode, 1000 cases each. Every case
//! must pass on every register (WZ, R, Q, the interrupt flip-flops and the mode included), every
//! byte of memory it lists, every I/O port value, the order and values of every memory and I/O
//! access the instruction makes, and its T-states.
//!
//! Fetch the suite first with scripts/fetch-test-suites.sh; without it this test says SKIPPED.
//! `SST_ONLY="ed b0"` runs one file, for chasing a failure.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use continuum_arcade::z80::{Bus, Z80};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case<'a> {
    name: String,
    initial: State,
    #[serde(rename = "final")]
    fin: State,
    /// The bus between every two T-states: address, data, and which of read, write, memory
    /// request and I/O request are active.
    #[serde(borrow)]
    cycles: Vec<(Option<u16>, Option<u8>, &'a str)>,
    #[serde(default, borrow)]
    ports: Vec<(u16, u8, &'a str)>,
}

#[derive(Deserialize)]
struct State {
    pc: u16,
    sp: u16,
    a: u8,
    b: u8,
    c: u8,
    d: u8,
    e: u8,
    f: u8,
    h: u8,
    l: u8,
    i: u8,
    r: u8,
    ei: u8,
    wz: u16,
    ix: u16,
    iy: u16,
    af_: u16,
    bc_: u16,
    de_: u16,
    hl_: u16,
    im: u8,
    p: u8,
    q: u8,
    iff1: u8,
    iff2: u8,
    ram: Vec<(u16, u8)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    Read(u16, u8),
    Write(u16, u8),
    In(u16, u8),
    Out(u16, u8),
}

/// 64 KB of memory, the port values the case says its IN instructions read, and a log of every
/// access in order.
struct TestBus {
    mem: Vec<u8>,
    touched: Vec<u16>,
    port_reads: Vec<(u16, u8)>,
    next_port: usize,
    log: Vec<Access>,
}

impl Bus for TestBus {
    fn read(&mut self, addr: u16) -> u8 {
        let v = self.mem[addr as usize];
        self.log.push(Access::Read(addr, v));
        v
    }
    fn write(&mut self, addr: u16, val: u8) {
        self.mem[addr as usize] = val;
        self.touched.push(addr);
        self.log.push(Access::Write(addr, val));
    }
    fn input(&mut self, port: u16) -> u8 {
        // The value the case gives for its next port read; a read it doesn't expect gets 0xFF
        // and shows up as a difference in the access log.
        let v = match self.port_reads.get(self.next_port) {
            Some(&(_, v)) => v,
            None => 0xFF,
        };
        self.next_port += 1;
        self.log.push(Access::In(port, v));
        v
    }
    fn output(&mut self, port: u16, val: u8) {
        self.log.push(Access::Out(port, val));
    }
}

/// The accesses the case's bus record shows, in order. A read's data is on the bus one sample
/// after its request; a write's is on it with the request.
fn expected_accesses(cycles: &[(Option<u16>, Option<u8>, &str)]) -> Result<Vec<Access>, String> {
    let mut out = Vec::new();
    for (i, &(addr, data, pins)) in cycles.iter().enumerate() {
        let next_data = || cycles.get(i + 1).and_then(|c| c.1);
        let a = addr.unwrap_or(0);
        match pins {
            "r-m-" => out.push(Access::Read(a, next_data().ok_or("read with no data")?)),
            "-wm-" => out.push(Access::Write(a, data.ok_or("write with no data")?)),
            "r--i" => out.push(Access::In(a, next_data().ok_or("port read with no data")?)),
            "-w-i" => out.push(Access::Out(a, data.ok_or("port write with no data")?)),
            "----" => {}
            other => return Err(format!("unknown bus state {other:?}")),
        }
    }
    Ok(out)
}

fn load(z: &mut Z80, s: &State) {
    z.a = s.a;
    z.f = s.f;
    z.b = s.b;
    z.c = s.c;
    z.d = s.d;
    z.e = s.e;
    z.h = s.h;
    z.l = s.l;
    z.af2 = s.af_;
    z.bc2 = s.bc_;
    z.de2 = s.de_;
    z.hl2 = s.hl_;
    z.ix = s.ix;
    z.iy = s.iy;
    z.sp = s.sp;
    z.pc = s.pc;
    z.i = s.i;
    z.r = s.r;
    z.wz = s.wz;
    z.iff1 = s.iff1 != 0;
    z.iff2 = s.iff2 != 0;
    z.im = s.im;
    z.ei = s.ei != 0;
    z.p = s.p != 0;
    z.q = s.q;
    z.halted = false;
    z.irq = false;
    z.nmi_pending = false;
}

/// Every register that differs from the case's final state, as "name expected got".
fn register_diffs(z: &Z80, s: &State) -> Vec<String> {
    let pairs: [(&str, u32, u32); 25] = [
        ("a", s.a.into(), z.a.into()),
        ("f", s.f.into(), z.f.into()),
        ("b", s.b.into(), z.b.into()),
        ("c", s.c.into(), z.c.into()),
        ("d", s.d.into(), z.d.into()),
        ("e", s.e.into(), z.e.into()),
        ("h", s.h.into(), z.h.into()),
        ("l", s.l.into(), z.l.into()),
        ("af'", s.af_.into(), z.af2.into()),
        ("bc'", s.bc_.into(), z.bc2.into()),
        ("de'", s.de_.into(), z.de2.into()),
        ("hl'", s.hl_.into(), z.hl2.into()),
        ("ix", s.ix.into(), z.ix.into()),
        ("iy", s.iy.into(), z.iy.into()),
        ("sp", s.sp.into(), z.sp.into()),
        ("pc", s.pc.into(), z.pc.into()),
        ("i", s.i.into(), z.i.into()),
        ("r", s.r.into(), z.r.into()),
        ("wz", s.wz.into(), z.wz.into()),
        ("iff1", s.iff1.into(), u32::from(z.iff1)),
        ("iff2", s.iff2.into(), u32::from(z.iff2)),
        ("im", s.im.into(), z.im.into()),
        ("ei", s.ei.into(), u32::from(z.ei)),
        ("p", s.p.into(), u32::from(z.p)),
        ("q", s.q.into(), z.q.into()),
    ];
    pairs
        .iter()
        .filter(|(_, want, got)| want != got)
        .map(|(name, want, got)| format!("{name} {want:#x} got {got:#x}"))
        .collect()
}

/// Runs one case; the description of what went wrong, if anything did.
fn run_case(bus: &mut TestBus, case: &Case) -> Option<String> {
    for &addr in &bus.touched {
        bus.mem[addr as usize] = 0;
    }
    bus.touched.clear();
    bus.log.clear();
    for &(addr, v) in &case.initial.ram {
        bus.mem[addr as usize] = v;
        bus.touched.push(addr);
    }
    bus.port_reads = case
        .ports
        .iter()
        .filter(|p| p.2 == "r")
        .map(|p| (p.0, p.1))
        .collect();
    bus.next_port = 0;

    let mut z = Z80::new();
    load(&mut z, &case.initial);
    let t = z.step(bus);

    let mut wrong = register_diffs(&z, &case.fin);
    if t as usize != case.cycles.len() {
        wrong.push(format!("T-states {} got {t}", case.cycles.len()));
    }
    for &(addr, want) in &case.fin.ram {
        let got = bus.mem[addr as usize];
        if got != want {
            wrong.push(format!("({addr:#06x}) {want:#04x} got {got:#04x}"));
        }
    }
    match expected_accesses(&case.cycles) {
        Ok(want) if want != bus.log => {
            wrong.push(format!("accesses {want:x?} got {:x?}", bus.log));
        }
        Ok(_) => {}
        Err(e) => wrong.push(format!("bus record: {e}")),
    }
    let want_out: Vec<(u16, u8)> = case
        .ports
        .iter()
        .filter(|p| p.2 == "w")
        .map(|p| (p.0, p.1))
        .collect();
    let got_out: Vec<(u16, u8)> = bus
        .log
        .iter()
        .filter_map(|a| {
            if let Access::Out(p, v) = *a {
                Some((p, v))
            } else {
                None
            }
        })
        .collect();
    if want_out != got_out {
        wrong.push(format!("port writes {want_out:x?} got {got_out:x?}"));
    }
    if wrong.is_empty() {
        None
    } else {
        Some(format!("{}: {}", case.name, wrong.join(", ")))
    }
}

#[test]
fn singlestep_z80() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-suites/singlestep-z80/v1");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!(
            "SKIPPED: {} not found (run scripts/fetch-test-suites.sh)",
            dir.display()
        );
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    let only = std::env::var("SST_ONLY").ok();
    if let Some(only) = &only {
        files.retain(|p| {
            p.file_stem()
                .is_some_and(|s| s.to_string_lossy() == only.as_str())
        });
        assert!(!files.is_empty(), "SST_ONLY={only}: no such file");
    } else {
        // The fetch script checks the whole suite; a partial copy must not pass as the suite.
        assert_eq!(
            files.len(),
            1604,
            "the suite should have 1604 files; fetch it again"
        );
    }

    let next = AtomicUsize::new(0);
    let cases = AtomicUsize::new(0);
    let failures: Mutex<Vec<(String, usize, Vec<String>)>> = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(4);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                let mut bus = TestBus {
                    mem: vec![0; 0x10000],
                    touched: Vec::new(),
                    port_reads: Vec::new(),
                    next_port: 0,
                    log: Vec::new(),
                };
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = files.get(i) else { break };
                    let bytes = std::fs::read(path).unwrap();
                    let list: Vec<Case> = serde_json::from_slice(&bytes)
                        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                    let mut failed = Vec::new();
                    let mut count = 0;
                    for case in &list {
                        if let Some(why) = run_case(&mut bus, case) {
                            count += 1;
                            if failed.len() < 3 {
                                failed.push(why);
                            }
                        }
                    }
                    cases.fetch_add(list.len(), Ordering::Relaxed);
                    if count > 0 {
                        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
                        failures.lock().unwrap().push((name, count, failed));
                    }
                }
            });
        }
    });

    let mut failures = failures.into_inner().unwrap();
    failures.sort();
    let total = cases.load(Ordering::Relaxed);
    let failed: usize = failures.iter().map(|f| f.1).sum();
    for (name, count, examples) in &failures {
        eprintln!("{name}: {count} of 1000 failed");
        for e in examples {
            eprintln!("    {e}");
        }
    }
    eprintln!(
        "SingleStepTests Z80: {} of {total} cases passed, in {} files",
        total - failed,
        files.len()
    );
    assert!(
        failures.is_empty(),
        "{failed} cases failed in {} files",
        failures.len()
    );
}
