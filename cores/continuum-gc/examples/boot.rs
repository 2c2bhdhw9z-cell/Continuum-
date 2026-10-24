//! Development runner: boots a DOL/ELF/ISO, runs some fields, and reports what happened.
//! cargo run --release --example boot -- path [fields] [out.ppm]
use continuum_gc::libretro::open;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut m = open(&args[1]).expect("boots");
    // CGC_DISC=image: put a raw image in the drive (for programs that read data from a disc).
    if let Ok(d) = std::env::var("CGC_DISC") {
        m.hw.disc = Some(Box::new(continuum_gc::disc::PlainImage::open(&d).expect("disc image")));
    }
    // CGC_SD=image: an SD card (FAT image) in serial port 2.
    if let Ok(d) = std::env::var("CGC_SD") {
        m.hw.sd = Some(continuum_gc::sd::SdCard::open(&d).expect("sd image"));
    }
    // CGC_ARGV="a b" and CGC_ENV="K=V K2=V2": the command line and environment a loader passes.
    if let Ok(args) = std::env::var("CGC_ARGV") {
        let env = std::env::var("CGC_ENV").unwrap_or_default();
        let entry = m.cpu.pc;
        let ok = m.set_args(entry, &args.split(' ').collect::<Vec<_>>(), &env.split(' ').filter(|s| !s.is_empty()).collect::<Vec<_>>());
        eprintln!("arguments passed: {ok}");
    }
    let fields: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(120);
    let t = std::time::Instant::now();
    let mut pcs = std::collections::HashMap::<u32, u32>::new();
    let mut causes = [0u64; 32];
    let mut wav: Vec<i16> = Vec::new();
    for f in 0..fields {
        m.run_field(m.cpu_hz / 60);
        wav.extend_from_slice(&m.hw.audio_out);
        m.hw.audio_out.clear();
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
    let loud = wav.iter().filter(|s| s.unsigned_abs() > 64).count();
    eprintln!("sound: {} frames, {} loud samples, peak {}", wav.len() / 2, loud, wav.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0));
    if let Ok(path) = std::env::var("CGC_WAV") {
        let mut b = Vec::new();
        let data = (wav.len() * 2) as u32;
        b.extend_from_slice(b"RIFF"); b.extend_from_slice(&(36 + data).to_le_bytes()); b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes()); b.extend_from_slice(&1u16.to_le_bytes()); b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&48000u32.to_le_bytes()); b.extend_from_slice(&(48000u32 * 4).to_le_bytes()); b.extend_from_slice(&4u16.to_le_bytes()); b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data"); b.extend_from_slice(&data.to_le_bytes());
        for s in &wav { b.extend_from_slice(&s.to_le_bytes()); }
        std::fs::write(path, b).unwrap();
    }
    eprintln!("exceptions taken: {:?}", m.cpu.exceptions.iter().enumerate().filter(|(_, n)| **n > 0).map(|(v, n)| (format!("{:X}00", v), *n)).collect::<Vec<_>>());
    eprintln!("faults (vector, at, address): {:08X?}", m.cpu.fault_log);
    eprintln!("unknown registers: {:X?}", m.hw.unknown.iter().take(40).collect::<Vec<_>>());
    m.hw.gx.sync();
    eprintln!("graphics: {:?}", m.hw.gx.stats);
    eprintln!("disc commands: {:X?}", m.hw.di_commands);
    eprintln!("sound program: {:?} crc {:08X}", m.hw.dsp.hle.kind, m.hw.dsp.hle.crc);
    if let Some((a, l)) = m.hw.dsp.ucode {
        let code = m.hw.dma_read(a & 0x03FF_FFFF, 64);
        eprintln!("  at {a:08X}, {l} bytes: {:04X?}", code.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>());
    }
    if std::env::var("CGC_DI_TRACE").is_ok() {
        eprintln!("disc register writes: {:X?}", m.hw.di_trace);
    }
    eprintln!("uart: {}", String::from_utf8_lossy(&m.hw.uart).replace('\r', "\n"));
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
