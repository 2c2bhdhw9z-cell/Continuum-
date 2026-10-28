//! The XM expansion module's YM2151 FM chip against MAME 0.289's, sample for sample:
//! test-programs/xm.asm plays every way the four operators can be connected, feedback, pitches
//! across the range with detune and multiples, the envelope's rates and sustain, key scaling,
//! three channels at once on their own sides of the stereo, the LFO on all four waveforms with
//! vibrato and tremolo, and the noise generator.
//!
//! MAME puts the XM's sound on a speaker of its own, so the second channel of its recording is the
//! chip's two sides added, clipped to 16 bits. It records at the chip's own rate, so there is no
//! resampling: with MAME_YM=<MAME's recording> every sample must be ours exactly, and WRITE_REF=1
//! then writes tests/references/xm.txt (FNV-1a hashes of our samples, every 8,192). The recording
//! is made by `MAME=... python3 tests/mame/make_refs.py xm`.
//!
//! Two things in MAME are allowed for, and nothing else:
//! - it keeps the chip's sample rate as a whole number of hertz (3,579,544 over 64, truncated to
//!   55,930; the board's own is 55,930.4), so the test runs the chip on that rate;
//! - it rounds the console's clocks to whole hertz, so its processor runs 2 parts in 7 million
//!   slower than a quarter of MARIA's clock, as it is on the board. Over the test that moves the
//!   writes, which follow MARIA's lines (each comes just after a WSYNC), by up to a clock against
//!   the chip's sample grid. The test puts each write on the sample MAME's slower processor makes
//!   it on, and plays them through a chip of ours again.

use continuum_atari::cart::Cart;
use continuum_atari::machine::{Atari7800, Region};
use continuum_atari::ym2151::Ym2151;

const FRAMES: u32 = 240;
const BLOCK: usize = 8192;
/// MAME's sample rate for the chip, and its processor's and MARIA's clocks.
const RATE: u64 = 55_930;
const MARIA_HZ: u64 = 7_159_090;
const CPU_HZ: u64 = 1_789_772;

fn fnv(data: impl IntoIterator<Item = i32>) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for v in data {
        for b in v.to_le_bytes() {
            h = (h ^ b as u64).wrapping_mul(0x0100_0000_01B3);
        }
    }
    h
}

/// Our console playing the test with the XM plugged in: every write to the FM chip (its MARIA
/// clock and its line's start), and how many samples the chip made.
fn play() -> (Vec<(u64, u64, u8, u8)>, usize) {
    let dir = env!("CARGO_MANIFEST_DIR");
    let rom = std::fs::read(format!("{dir}/test-programs/xm.bin")).unwrap();
    let mut h = vec![0u8; 128];
    h[1..10].copy_from_slice(b"ATARI7800");
    h[49..53].copy_from_slice(&(rom.len() as u32).to_be_bytes());
    h[63] = 1; // the expansion module
    h.extend(rom);
    let bios = std::fs::read(format!("{dir}/test-programs/bios.bin")).unwrap();
    let mut m = Atari7800::new(Cart::load(&h).unwrap(), Region::Ntsc, Some(bios));
    m.sys.plug_xm(None);
    m.sys.set_ym_period(MARIA_HZ, RATE);
    m.sys.ym_log = Some(Vec::new());
    m.sys.ym_writes = Some(Vec::new());
    for _ in 0..FRAMES {
        m.run_frame();
        m.sys.out.clear();
    }
    let samples = m.sys.ym_log.take().unwrap().len();
    (m.sys.ym_writes.take().unwrap(), samples)
}

/// The same writes on MAME's sample grid. Each write comes a fixed number of processor cycles
/// after its line began, and three things put MAME's moment for it a little from ours:
/// - MAME's processor runs on its own rounded clock, so when MARIA lets it go at the start of a
///   line it carries on from its own next cycle, up to half a cycle after the line (the line is
///   113.5 of its cycles long). cAtari's processor keeps its own beat through the wait, as the
///   console's does, so it starts up to half a cycle before the line.
/// - MAME fetches an instruction's first byte at the end of the one before, so after a wait its
///   next write lands one cycle earlier in the instruction than on the console.
/// - A write takes effect from the next sample boundary at or after it (both emulators bring the
///   chip right up to the write first).
fn on_mame_grid(writes: &[(u64, u64, u8, u8)]) -> Vec<(u64, u8, u8)> {
    writes
        .iter()
        .map(|&(clock, line_start, port, v)| {
            // Our processor's beat through the wait, and the cycles from there to the write.
            let resume = line_start - line_start % 4;
            let n = (clock - resume) / 4;
            // MAME's first cycle at or after the line's start, then the write's cycle.
            let first = (line_start * CPU_HZ).div_ceil(MARIA_HZ);
            let cycle = first + n - 1;
            ((cycle * RATE).div_ceil(CPU_HZ), port, v)
        })
        .collect()
}

/// A chip of ours with those writes, one sample at a time.
fn replay(writes: &[(u64, u8, u8)], samples: usize) -> Vec<(i32, i32)> {
    let mut y = Ym2151::default();
    let mut out = Vec::with_capacity(samples);
    let mut w = writes.iter().peekable();
    for i in 0..samples as u64 {
        while let Some(&&(at, port, v)) = w.peek() {
            if at > i {
                break;
            }
            y.write(port, v, 0);
            w.next();
        }
        out.push(y.sample(0));
    }
    out
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

#[test]
fn the_ym2151_sounds_as_on_mame_sample_for_sample() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let (writes, samples) = play();
    let moved = on_mame_grid(&writes);
    let ours = replay(&moved, samples);
    // MAME's speaker adds the chip's two sides.
    let sum: Vec<i32> = ours.iter().map(|&(l, r)| (l + r).clamp(-32768, 32767)).collect();
    let reference = format!("{dir}/tests/references/xm.txt");
    if let Ok(path) = std::env::var("MAME_YM") {
        let (channels, data) = wav(&path);
        assert_eq!(channels, 2, "MAME records the console's sound and the XM's apart");
        assert!(data.iter().step_by(2).all(|&v| v == 0), "the console's own sound stays silent");
        let mame: Vec<i16> = data.iter().skip(1).step_by(2).copied().collect();
        let n = mame.len().min(sum.len());
        assert!(n > 200_000, "{n} samples");
        let bad: Vec<usize> = (0..n).filter(|&i| sum[i] != mame[i] as i32).collect();
        eprintln!("{} writes, {n} samples checked, {} differ", writes.len() / 2, bad.len());
        if let Some(&i) = bad.first() {
            for k in i.saturating_sub(4)..(i + 6).min(n) {
                eprintln!("  {k}: ours {:6} (L {:6} R {:6}), MAME {:6}", sum[k], ours[k].0, ours[k].1, mame[k]);
            }
            panic!("sample {i} differs (frame {}): ours {}, MAME {}", i as u64 * 60 / RATE, sum[i], mame[i]);
        }
        if std::env::var("WRITE_REF").is_ok() {
            let count = n / BLOCK * BLOCK;
            let mut f = String::new();
            f += "# MAME 0.289's a7800 with the XM expansion module playing test-programs/xm.asm, its\n";
            f += "# YM2151 recorded at the chip's own 55,930 Hz; made by tests/ym2151_mame.rs, which\n";
            f += &format!("# checked all {n} of its samples against ours. FNV-1a 64 of each {BLOCK} (the\n");
            f += "# chip's two sides added, as MAME's speaker adds them).\n";
            f += &format!("frames {FRAMES}\nsamples {count}\n");
            for (k, b) in sum[..count].chunks(BLOCK).enumerate() {
                f += &format!("block {k} {:016x}\n", fnv(b.iter().copied()));
            }
            std::fs::write(&reference, f).unwrap();
        }
        return;
    }
    let text = std::fs::read_to_string(&reference).unwrap();
    let count: usize = text.lines().find_map(|l| l.strip_prefix("samples ")).unwrap().trim().parse().unwrap();
    assert!(sum.len() >= count, "{} samples, the reference has {count}", sum.len());
    for l in text.lines().filter_map(|l| l.strip_prefix("block ")) {
        let mut it = l.split_whitespace();
        let k: usize = it.next().unwrap().parse().unwrap();
        let want = u64::from_str_radix(it.next().unwrap(), 16).unwrap();
        let got = fnv(sum[k * BLOCK..(k + 1) * BLOCK].iter().copied());
        assert_eq!(got, want, "samples {} to {} differ from MAME's", k * BLOCK, (k + 1) * BLOCK);
    }
    eprintln!("all {count} of the YM2151's samples match MAME's");
}
