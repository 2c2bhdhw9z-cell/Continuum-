//! POKEY's sound against MAME 0.289's: test-programs/pokey.asm plays every distortion, the 17 and
//! 9-bit noise, the 15 kHz and 1.79 MHz clocks, joined channels, both high-pass filters, settings
//! changed without restarting the counters, volume-only output written every line, the counters
//! stopped and restarted, two-tone mode both ways and asynchronous mode.
//!
//! Three things in MAME have to be allowed for, and nothing else:
//! - MAME can't record at POKEY's own 1,789,772 Hz (its limit is 1,000,000), so it records at
//!   894,887 Hz through its own resampler (pairs of POKEY's clocks averaged, then a four-point
//!   interpolation). The test puts POKEY's level at every clock through the same arithmetic.
//! - MAME rounds the console's clocks to whole hertz, MARIA's to 7,159,090 and the processor's
//!   (and so POKEY's) to 1,789,772, so its processor runs 2 parts in 7 million slower than a
//!   quarter of MARIA's clock, as it is on the console and in cAtari. Over the test that moves
//!   the processor's writes, which follow MARIA's lines (each comes just after a WSYNC), by up to
//!   two clocks against POKEY's own clock. The test moves each of our writes to the clock MAME's
//!   slower processor makes it on, and plays them through our POKEY again.
//! - MAME brings its sound up to date 50 times a second, and when that falls part-way through a
//!   clock, that clock gets the level before it (POKEY hasn't worked it out yet). Once in the
//!   test a change of level lands on such a clock.
//!
//! With MAME_POKEY=<MAME's recording> it checks MAME's recording sample by sample, and with
//! WRITE_REF=1 then writes tests/references/pokey.txt: FNV-1a hashes of POKEY's level at every
//! clock, every 65,536 clocks. Without MAME it checks those hashes. The recording is made by
//! `MAME=... python3 tests/mame/make_refs.py pokey` (it keeps mame-pokey/pokey.wav).

use continuum_atari::cart::Cart;
use continuum_atari::machine::{Atari7800, Region, LINE_CLOCKS};
use continuum_atari::pokey::Pokey;

const FS: u64 = 1_789_772;
const FT: u64 = 894_887;
const MAME_MARIA_HZ: u64 = 7_159_090;
const BLOCK: usize = 65_536;
const FRAMES: u32 = 210;
/// The largest allowed difference from MAME's recording, in its 16-bit steps: MAME's resampler
/// works on stretches of samples whose ends we can't see, so its fraction drifts by a few
/// thousandths of a sample from ours. One clock out of place anywhere costs thousands.
const TOLERANCE: i32 = 16;

fn fnv(data: &[u8]) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for &b in data {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01B3);
    }
    h
}

type Writes = Vec<(u64, u64, u16, u8)>;

/// Our console playing the test: POKEY's level at every clock, and every write to it.
fn play() -> (Vec<u8>, Writes) {
    let dir = env!("CARGO_MANIFEST_DIR");
    let rom = std::fs::read(format!("{dir}/test-programs/pokey.bin")).unwrap();
    let mut h = vec![0u8; 128];
    h[1..10].copy_from_slice(b"ATARI7800");
    h[49..53].copy_from_slice(&(rom.len() as u32).to_be_bytes());
    h[53..55].copy_from_slice(&1u16.to_be_bytes()); // a POKEY at $4000
    h.extend(rom);
    let bios = std::fs::read(format!("{dir}/test-programs/bios.bin")).unwrap();
    let mut m = Atari7800::new(Cart::load(&h).unwrap(), Region::Ntsc, Some(bios));
    m.sys.pokey_log = Some(Vec::new());
    m.sys.pokey_writes = Some(Vec::new());
    for _ in 0..FRAMES {
        m.run_frame();
    }
    (m.sys.pokey_log.take().unwrap(), m.sys.pokey_writes.take().unwrap())
}

/// The same writes on MAME's clocks, played through a POKEY of ours: each write keeps its place
/// after its line's start, and the line starts where MAME's slower processor sees it (its last
/// clock before the line, as on cAtari, but MAME's lines fall a little earlier every line).
fn on_mame_clocks(writes: &Writes, clocks: usize) -> (Vec<u8>, usize) {
    let mut moved = Vec::with_capacity(writes.len());
    let mut changed = 0;
    for &(clock, maria, addr, v) in writes {
        let line = maria / LINE_CLOCKS;
        let ours = line * LINE_CLOCKS / 4;
        let mame = line * LINE_CLOCKS * FS / MAME_MARIA_HZ;
        let at = clock - ours + mame;
        changed += (at != clock) as usize;
        moved.push((at, addr, v));
    }
    let mut p = Pokey::default();
    let mut out = Vec::with_capacity(clocks);
    let mut w = moved.iter().peekable();
    for c in 0..clocks as u64 {
        while let Some(&&(at, addr, v)) = w.peek() {
            if at > c {
                break;
            }
            p.write(addr as u8, v);
            w.next();
        }
        out.push(p.clock());
    }
    (out, changed)
}

/// MAME's recording: (channels, samples interleaved).
fn wav(path: &str) -> (usize, Vec<i16>) {
    let d = std::fs::read(path).unwrap();
    let mut i = 12;
    let mut channels = 1;
    while i + 8 <= d.len() {
        let id = &d[i..i + 4];
        let n = u32::from_le_bytes(d[i + 4..i + 8].try_into().unwrap()) as usize;
        if id == b"fmt " {
            channels = u16::from_le_bytes([d[i + 10], d[i + 11]]) as usize;
        }
        if id == b"data" {
            let end = (i + 8 + n).min(d.len());
            let s = d[i + 8..end].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
            return (channels, s);
        }
        i += 8 + n + (n & 1);
    }
    panic!("no data in {path}");
}

/// MAME 0.289's "lofi" resampler (src/emu/resampler.cpp), from POKEY's rate to the recording's,
/// on POKEY's output as MAME scales it (the four volumes added, times 744, over 32,768), then the
/// recording's conversion to 16 bits. `shift` lines our clocks up with MAME's.
struct Lofi {
    t0: Vec<f32>,
    t1: Vec<f32>,
}

impl Lofi {
    fn new() -> Lofi {
        let at = |i: usize| i as f32 / 4096.0;
        let mut t0: Vec<f32> = (0..=0x1000).map(|i| (at(i) - at(i) * at(i) * at(i)) / 6.0).collect();
        t0[0x1000] = 0.0;
        let mut t1: Vec<f32> = (0..=0x1000).map(|i| at(i) + (at(i) * at(i) - at(i) * at(i) * at(i)) / 2.0).collect();
        // When interpolating the four weights add up to 1, which sets the upper half of t1.
        for i in 2049..4096 {
            t1[i] = (1.0 + t0[i] as f64 + t0[4096 - i] as f64 - t1[4096 - i] as f64) as f32;
        }
        t1[0x1000] = 1.0;
        Lofi { t0, t1 }
    }

    fn sample(&self, levels: &[u8], n: u64, shift: i64) -> Option<i32> {
        let src = |k: i64| -> Option<f32> {
            // MAME brings its sound up to date 50 times a second; when that falls part-way
            // through a clock, the clock it falls in is filled before POKEY has worked it out,
            // with the level before it.
            let j = k as u64 * 50 / FS + 1;
            let late = k > 0 && !(j * FS).is_multiple_of(50) && j * FS / 50 == k as u64;
            let k = k + shift - late as i64;
            if k < 0 {
                return Some(0.0);
            }
            let l = *levels.get(k as usize)? as i32;
            Some((l * 744).min(0x7FFF) as f32 * (1.0f32 / 32768.0))
        };
        let seconds = n / FT;
        let dsamp = n % FT;
        let ssamp = dsamp * FS * 0x1000 / FT;
        let mut ssample = (ssamp >> 12) + FS * seconds;
        let mut phase = (ssamp & 0xFFF) as u32;
        let delta = (ssample % 2) as u32;
        phase = (phase | (delta << 12)) / 2;
        ssample -= delta as u64;
        let base = ssample as i64 - 8;
        let mut s = [0f32; 4];
        for (k, v) in s.iter_mut().enumerate() {
            let a = src(base + 2 * k as i64)?;
            let b = src(base + 2 * k as i64 + 1)?;
            *v = (0.0 + a + b) / 2.0;
        }
        let c = phase as usize;
        let out = -s[0] * self.t0[0x1000 - c] + s[1] * self.t1[0x1000 - c] + s[2] * self.t1[c] - s[3] * self.t0[c];
        Some(((out * 32768.0) as i32).clamp(-32768, 32767))
    }
}

#[test]
fn pokey_sounds_as_on_mame() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let (ours, writes) = play();
    let (levels, moved) = on_mame_clocks(&writes, ours.len());
    let reference = format!("{dir}/tests/references/pokey.txt");
    if let Ok(path) = std::env::var("MAME_POKEY") {
        let (channels, data) = wav(&path);
        assert_eq!(channels, 2, "MAME records the console's TIA and the cartridge's POKEY apart");
        assert!(data.iter().step_by(2).all(|&v| v == 0), "the TIA stays silent");
        let mame: Vec<i16> = data.iter().skip(1).step_by(2).copied().collect();
        let lofi = Lofi::new();
        // MAME's console starts its program twelve lines later than ours (as the TIA's test
        // finds, 24 of its samples): line the clocks up where the sound starts, then to the clock
        // on its first tenth of a second.
        let first = mame.iter().position(|&v| v != 0).expect("MAME's recording has sound") as u64;
        let ours_first = levels.iter().position(|&v| v != 0).expect("our POKEY plays") as i64;
        let guess = ours_first - (first * FS / FT) as i64;
        let span = first..first + 90_000;
        let (_, shift) = (guess - 64..=guess + 64)
            .map(|shift| {
                let bad = span
                    .clone()
                    .filter(|&n| lofi.sample(&levels, n, shift).is_some_and(|p| (p - mame[n as usize] as i32).abs() > TOLERANCE))
                    .count();
                (bad, shift)
            })
            .min()
            .unwrap();
        let mut worst = (0i32, 0u64);
        let mut bad = Vec::new();
        let mut checked = 0u64;
        for n in 0..mame.len() as u64 {
            let Some(p) = lofi.sample(&levels, n, shift) else { break };
            let d = (p - mame[n as usize] as i32).abs();
            checked += 1;
            if d > worst.0 {
                worst = (d, n);
            }
            if d > TOLERANCE {
                bad.push(n);
            }
        }
        let differ = ours.iter().zip(&levels).filter(|(a, b)| a != b).count();
        eprintln!(
            "{} writes, {moved} of them moved to MAME's clocks ({differ} of {} clocks change); MAME's console \
             {} clocks behind ours; {checked} of its samples checked, {} differ by more than {TOLERANCE}; \
             the largest difference {} (sample {})",
            writes.len(),
            levels.len(),
            -shift,
            bad.len(),
            worst.0,
            worst.1
        );
        if let Some(&n) = bad.first() {
            let at = (n * FS / FT) as i64 + shift;
            for k in n.saturating_sub(3)..n + 6 {
                eprintln!("  sample {k}: ours {:?}, MAME {}", lofi.sample(&levels, k, shift), mame[k as usize]);
            }
            panic!("first difference at MAME's sample {n}, our clock {at} (frame {})", at * 4 / (263 * LINE_CLOCKS) as i64);
        }
        if std::env::var("WRITE_REF").is_ok() {
            let clocks = (checked * FS / FT) as usize / BLOCK * BLOCK;
            let mut f = String::new();
            f += "# POKEY's level (its four volumes added) at every clock as MAME 0.289's a7800 plays\n";
            f += "# test-programs/pokey.asm, our writes on MAME's clocks; made by tests/pokey_mame.rs from\n";
            f += &format!("# MAME's recording at 894,887 Hz: {checked} samples, none more than {} out (the most\n", worst.0);
            f += &format!("# allowed is {TOLERANCE}). FNV-1a 64 of each {BLOCK} clocks.\n");
            f += &format!("frames {FRAMES}\nclocks {clocks}\n");
            for (k, b) in levels[..clocks].chunks(BLOCK).enumerate() {
                f += &format!("block {k} {:016x}\n", fnv(b));
            }
            std::fs::write(&reference, f).unwrap();
        }
        return;
    }
    let text = std::fs::read_to_string(&reference).unwrap();
    let clocks: usize = text.lines().find_map(|l| l.strip_prefix("clocks ")).unwrap().trim().parse().unwrap();
    assert!(levels.len() >= clocks);
    for l in text.lines().filter_map(|l| l.strip_prefix("block ")) {
        let mut it = l.split_whitespace();
        let k: usize = it.next().unwrap().parse().unwrap();
        let want = u64::from_str_radix(it.next().unwrap(), 16).unwrap();
        let got = fnv(&levels[k * BLOCK..(k + 1) * BLOCK]);
        assert_eq!(got, want, "POKEY's levels in clocks {} to {} differ from MAME's", k * BLOCK, (k + 1) * BLOCK);
    }
    eprintln!("all {clocks} of POKEY's clocks match MAME's");
}
