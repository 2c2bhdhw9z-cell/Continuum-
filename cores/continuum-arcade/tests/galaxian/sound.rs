//! Galaxian's analogue sound against MAME's model of the same circuit. cArcade's sound is an
//! approximation of the circuit (src/galaxian_sound.rs), and so is MAME's, so the two are compared
//! by what can be heard rather than sample for sample: test-programs/galaxian/sound.inc plays the
//! latches segment by segment, and in each segment the level (RMS) and the make-up of cArcade's
//! sound must be MAME's within a tolerance per kind of segment. The tone's counters are digital,
//! so its strongest frequency must agree closely and match the counters' own arithmetic; its
//! level with each VOL1/VOL2 setting checks the mixing resistors. The background oscillators sweep
//! (two peaks can trade places as they do), so their whole spectrum must have MAME's shape. HIT
//! and FIRE are noise, and the two noise generators aren't the same model (nor the same random
//! sequence), so for them it is the level and the spectrum's shape in octave bands, averaged over
//! many short windows (Welch's method) so that two different runs of noise can be compared.

use std::fs;
use std::path::Path;
use std::process::Command;

use continuum_arcade::galaxian::{self, Galaxian, Rotation, Variant};

use super::{crate_dir, roms, Case};

/// 192 kHz samples a frame.
pub const PER_FRAME: usize = galaxian::RAW_SAMPLES_PER_FRAME;

/// What a segment plays, which decides how closely it must agree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The tone at a pitch, with VOL1 and VOL2.
    Tone { pitch: u8, vol1: bool, vol2: bool },
    /// Nothing at all.
    Quiet,
    /// One or more background oscillators.
    Background,
    /// HIT or FIRE, or their tails.
    Noise,
    /// The last of a tail: only has to be quiet.
    Tail,
}

pub struct Segment {
    pub name: String,
    /// The script's frames (counted from the first NMI): [start, end).
    pub start: u32,
    pub end: u32,
    pub kind: Kind,
}

/// The segments of test-programs/galaxian/sound.inc, in its order.
pub fn segments() -> Vec<Segment> {
    let mut s = Vec::new();
    let mut f = 10;
    let mut push = |name: String, len: u32, kind: Kind, f: &mut u32| {
        s.push(Segment { name, start: *f, end: *f + len, kind });
        *f += len;
    };
    for pitch in [0x00u8, 0x60, 0xA0, 0xD0] {
        for (vol1, vol2) in [(false, false), (true, false), (false, true), (true, true)] {
            let name = format!("tone {pitch:02X} VOL1 {} VOL2 {}", u8::from(vol1), u8::from(vol2));
            push(name, 24, Kind::Tone { pitch, vol1, vol2 }, &mut f);
        }
    }
    push("silence".into(), 20, Kind::Quiet, &mut f);
    for n in ["FS1", "FS2", "FS3", "FS1 with LFO 0F", "FS1-FS3 with LFO 0A"] {
        push(n.into(), 60, Kind::Background, &mut f);
    }
    push("quiet".into(), 20, Kind::Quiet, &mut f);
    push("HIT".into(), 20, Kind::Noise, &mut f);
    push("HIT dying away".into(), 45, Kind::Noise, &mut f);
    push("FIRE".into(), 20, Kind::Noise, &mut f);
    push("FIRE dying away".into(), 40, Kind::Noise, &mut f);
    push("end".into(), 10, Kind::Tail, &mut f);
    assert_eq!(f, 869, "the segments don't add up to sound.inc's script");
    s
}

pub fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "sound",
            program: "sound",
            set: "galaxian",
            variant: Variant::Galaxian,
            frames: 871,
            events: vec![],
        },
        Case {
            name: "mcsound",
            program: "mcsound",
            set: "mooncrstuu",
            variant: Variant::MoonCresta { encrypted: false },
            frames: 871,
            events: vec![],
        },
    ]
}

/// cArcade's sound for `case`, 192 kHz.
pub fn run_ours(case: &Case) -> Vec<i16> {
    let mut b = Galaxian::new(case.variant, Rotation::Rot90, &roms(case)).unwrap();
    b.set_ports(galaxian::Ports { in0: 0, in1: 0, in2: 0x04, in3: 0 });
    let mut out = Vec::with_capacity(PER_FRAME * case.frames as usize);
    for _ in 0..case.frames {
        b.run_frame();
        out.extend_from_slice(b.raw_audio());
    }
    out
}

/// MAME's sound for `case`, recorded at 192 kHz.
pub fn run_mame(mame: &Path, case: &Case) -> Vec<i16> {
    let work = crate_dir().join("target/mame-runs/galaxian-sound").join(case.name);
    let _ = fs::remove_dir_all(&work);
    let rom_dir = work.join("roms").join(case.set);
    fs::create_dir_all(&rom_dir).unwrap();
    for (name, data) in super::files(case) {
        fs::write(rom_dir.join(name), data).unwrap();
    }
    let out = Command::new(mame)
        .current_dir(&work)
        .env("SDL_VIDEODRIVER", "dummy")
        .env("SDL_AUDIODRIVER", "dummy")
        .args([case.set, "-rompath", "roms", "-video", "none", "-sound", "none", "-nothrottle"])
        .args(["-skip_gameinfo", "-cfg_directory", "cfg", "-nvram_directory", "nvram"])
        .args(["-samplerate", "192000", "-wavwrite", "sound.wav", "-seconds_to_run"])
        .arg((case.frames / 60 + 2).to_string())
        .output()
        .expect("MAME runs");
    let log = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    fs::write(work.join("mame.log"), &log).unwrap();
    let wav = fs::read(work.join("sound.wav")).unwrap_or_else(|_| panic!("MAME wrote no sound:\n{log}"));
    let data = wav.windows(4).position(|w| w == b"data").expect("a WAV data chunk") + 8;
    let samples: Vec<i16> = wav[data..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
    assert!(samples.len() >= PER_FRAME * case.frames as usize, "MAME's sound is short:\n{log}");
    samples
}

/// What a segment sounds like: its level, its strongest frequency and its spectrum.
#[derive(Clone, Debug, PartialEq)]
pub struct Measure {
    /// RMS of the sound less its mean, in volts at the amplifier (a full-scale sample is 1 V).
    pub rms: f64,
    /// The strongest frequency between 40 Hz and 12 kHz, in Hz.
    pub freq: f64,
    /// The energy in twelfth-octave bands (about a semitone each) from 40 Hz to 12 kHz, from a
    /// 65,536-point spectrum (shorter segments are padded to that).
    pub fine: Vec<f64>,
    /// The energy in octave bands from 40 Hz (40-80 Hz, 80-160 Hz, ... up to 10.24 kHz),
    /// averaged over 4,096-sample windows half overlapping.
    pub octaves: Vec<f64>,
}

/// Measures a segment, from six frames after it starts (when the latches have settled) to its
/// end. Our frame n holds the samples from (n - 1) × 3168; the script's frame F runs in our
/// frame F + 1.
pub fn measure(samples: &[i16], seg: &Segment) -> Measure {
    let from = (seg.start as usize + 6) * PER_FRAME;
    let to = seg.end as usize * PER_FRAME;
    let v: Vec<f64> = samples[from..to].iter().map(|&s| f64::from(s) / 32767.0).collect();
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let rms = (v.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / v.len() as f64).sqrt();
    // The window: the whole segment if it fits in 65,536 samples, else its first 65,536, with a
    // Hann taper, padded with zeros to 65,536.
    const N: usize = 65_536;
    let len = v.len().min(N);
    let mut re = vec![0.0; N];
    for (i, r) in re.iter_mut().enumerate().take(len) {
        let hann = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / len as f64).cos();
        *r = (v[i] - mean) * hann;
    }
    let mut im = vec![0.0; N];
    fft(&mut re, &mut im);
    let mag: Vec<f64> = re.iter().zip(&im).map(|(r, i)| (r * r + i * i).sqrt()).collect();
    let bin_hz = 192_000.0 / N as f64;
    let (lo, hi) = ((40.0 / bin_hz) as usize, (12_000.0 / bin_hz) as usize);
    let peak = (lo..hi).max_by(|&a, &b| mag[a].total_cmp(&mag[b])).unwrap();
    // A parabola through the peak and its neighbours, on the logarithm of the magnitudes.
    let (a, b, c) = (mag[peak - 1].max(1e-12).ln(), mag[peak].max(1e-12).ln(), mag[peak + 1].max(1e-12).ln());
    let shift = if a - 2.0 * b + c != 0.0 { 0.5 * (a - c) / (a - 2.0 * b + c) } else { 0.0 };
    let mut fine = Vec::new();
    for band in 0..FINE_BANDS {
        let f0 = 40.0 * 2f64.powf(band as f64 / 12.0);
        let f1 = 40.0 * 2f64.powf((band + 1) as f64 / 12.0);
        let b0 = (f0 / bin_hz) as usize;
        let b1 = ((f1 / bin_hz) as usize).max(b0 + 1);
        fine.push(mag[b0..b1].iter().map(|m| m * m).sum::<f64>());
    }
    Measure { rms, freq: (peak as f64 + shift) * bin_hz, fine, octaves: octaves(&v, mean) }
}

const OCTAVES: usize = 8;

fn octaves(v: &[f64], mean: f64) -> Vec<f64> {
    const W: usize = 4096;
    let mut power = vec![0.0; W / 2];
    let mut windows = 0;
    let mut start = 0;
    while start + W <= v.len() {
        let mut re: Vec<f64> = (0..W)
            .map(|i| {
                let hann = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / W as f64).cos();
                (v[start + i] - mean) * hann
            })
            .collect();
        let mut im = vec![0.0; W];
        fft(&mut re, &mut im);
        for (p, (r, i)) in power.iter_mut().zip(re.iter().zip(&im)) {
            *p += r * r + i * i;
        }
        windows += 1;
        start += W / 2;
    }
    let bin_hz = 192_000.0 / W as f64;
    (0..OCTAVES)
        .map(|o| {
            let (f0, f1) = (40.0 * f64::from(1u32 << o), 40.0 * f64::from(2u32 << o));
            let (b0, b1) = ((f0 / bin_hz).round() as usize, (f1 / bin_hz).round() as usize);
            power[b0..b1].iter().sum::<f64>() / f64::from(windows.max(1))
        })
        .collect()
}

/// Twelfth-octave bands from 40 Hz up to 12 kHz (a multiple of four, for the third-octave ones).
const FINE_BANDS: usize = 100;

/// How alike two spectra are in shape: the cosine of the angle between them (1 is the same).
pub fn likeness(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let nb: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

/// An in-place radix-2 FFT (the length a power of two).
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (ang * k as f64).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

/// The tone's strongest part, from the counters: the 74393 steps at 1.536 MHz / (256 - pitch);
/// QC is an eighth of that and QD a sixteenth. QD (through VOL2's 15 kΩ) outweighs QC (22 kΩ)
/// unless VOL1 adds QC's second path (10 kΩ).
pub fn tone_frequency(pitch: u8, vol1: bool, vol2: bool) -> f64 {
    let step = 1_536_000.0 / (256.0 - f64::from(pitch));
    if vol2 && !vol1 {
        step / 16.0
    } else {
        step / 8.0
    }
}

/// The tolerances: the level relative to MAME's, the strongest frequency relative to MAME's (tones
/// only), and the least likeness of the spectrum (background) or of the band energies (noise).
pub const TONE_LEVEL: f64 = 0.01;
pub const TONE_FREQ: f64 = 0.002;
pub const BACKGROUND_LEVEL: f64 = 0.03;
pub const BACKGROUND_LIKENESS: f64 = 0.95;
pub const NOISE_LEVEL: f64 = 0.10;
pub const NOISE_LIKENESS: f64 = 0.98;
/// Quiet is under a millivolt, the end of a tail under 30 mV (HIT is about 240 mV).
pub const QUIET: f64 = 0.001;
pub const TAIL: f64 = 0.03;

/// Whether cArcade's segment agrees with MAME's, or how it doesn't.
pub fn compare(seg: &Segment, ours: &Measure, mame: &Measure) -> Result<(), String> {
    let rel = |a: f64, b: f64| (a - b).abs() / b.abs().max(1e-9);
    let level = |tol: f64| {
        if rel(ours.rms, mame.rms) > tol {
            Err(format!(
                "{}: level {:.4} V, MAME {:.4} V ({:.1}% apart, {:.0}% allowed)",
                seg.name, ours.rms, mame.rms, 100.0 * rel(ours.rms, mame.rms), 100.0 * tol
            ))
        } else {
            Ok(())
        }
    };
    let alike = |what: &str, a: &[f64], b: &[f64], least: f64| {
        let l = likeness(a, b);
        if l < least {
            Err(format!("{}: the {what} is {l:.3} like MAME's, {least} needed", seg.name))
        } else {
            Ok(())
        }
    };
    let quiet = |most: f64| {
        if ours.rms < most && mame.rms < most {
            Ok(())
        } else {
            Err(format!("{}: should be under {most} V: ours {:.4} V, MAME {:.4} V", seg.name, ours.rms, mame.rms))
        }
    };
    match seg.kind {
        Kind::Tone { pitch, vol1, vol2 } => {
            level(TONE_LEVEL)?;
            if rel(ours.freq, mame.freq) > TONE_FREQ {
                return Err(format!("{}: strongest at {:.1} Hz, MAME {:.1} Hz", seg.name, ours.freq, mame.freq));
            }
            let want = tone_frequency(pitch, vol1, vol2);
            if rel(ours.freq, want) > TONE_FREQ {
                return Err(format!("{}: strongest at {:.1} Hz, the counters give {want:.1} Hz", seg.name, ours.freq));
            }
            Ok(())
        }
        Kind::Quiet => quiet(QUIET),
        Kind::Tail => quiet(TAIL),
        Kind::Background => {
            level(BACKGROUND_LEVEL)?;
            alike("spectrum", &ours.fine, &mame.fine, BACKGROUND_LIKENESS)
        }
        Kind::Noise => {
            level(NOISE_LEVEL)?;
            alike("octave spectrum", &ours.octaves, &mame.octaves, NOISE_LIKENESS)
        }
    }
}

pub fn reference_path(case: &Case) -> std::path::PathBuf {
    crate_dir().join("tests/references/galaxian").join(format!("{}.txt", case.name))
}

/// MAME's measures, one line per segment.
pub fn read_reference(case: &Case) -> Vec<Measure> {
    let p = reference_path(case);
    let text = fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let w: Vec<&str> = l.split('\t').collect();
            let list = |s: &str| s.split(' ').map(|v| v.parse().unwrap()).collect();
            Measure { rms: w[1].parse().unwrap(), freq: w[2].parse().unwrap(), fine: list(w[3]), octaves: list(w[4]) }
        })
        .collect()
}

pub fn write_reference(case: &Case, measures: &[Measure], mame_version: &str) {
    let mut s = format!(
        "# {}: MAME {} running set {} with cArcade's test program {}, recorded at 192 kHz.\n\
         # Per segment of test-programs/galaxian/sound.inc (tests/galaxian/sound.rs), tab-separated:\n\
         # its name, the RMS of the sound in volts at the amplifier, its strongest frequency in Hz,\n\
         # its energy in twelfth-octave bands from 40 Hz, and in octave bands from 40 Hz.\n",
        case.name, mame_version, case.set, case.program
    );
    for (seg, m) in segments().iter().zip(measures) {
        let list = |v: &[f64]| v.iter().map(|e| format!("{e:.4e}")).collect::<Vec<_>>().join(" ");
        s += &format!("{}\t{:.6}\t{:.3}\t{}\t{}\n", seg.name, m.rms, m.freq, list(&m.fine), list(&m.octaves));
    }
    fs::write(reference_path(case), s).unwrap();
}
