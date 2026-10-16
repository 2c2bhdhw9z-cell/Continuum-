//! The sound programs' sound against MAME's. Where only the DAC sounds (all of Donkey Kong's
//! test, and Jr.'s once its power-on sound has died away, up to its analogue effects) cArcade's
//! output follows MAME's sample for sample, within a step or two, so every frame's level and
//! mean must agree closely. The analogue effects are approximations of MAME's model, with
//! oscillators of their own, so those stretches (the script's segments) must agree by level,
//! strongest frequency and spectrum, as tests/measure measures them.

use super::{Case, FrameOut};
use crate::measure::{self, Measure};

pub const RATE: f64 = 48_000.0;
pub const SAMPLES: usize = 792;

/// A frame's level: its RMS less the mean, and the mean, as fractions of full scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    pub rms: f64,
    pub mean: f64,
}

pub fn level(samples: &[i16]) -> Level {
    let v: Vec<f64> = samples.iter().map(|&s| f64::from(s) / 32767.0).collect();
    let mean = v.iter().sum::<f64>() / v.len().max(1) as f64;
    let rms = (v.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / v.len().max(1) as f64).sqrt();
    Level { rms, mean }
}

/// How a stretch of the script is held to MAME's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The DAC alone: every frame's level, closely.
    Digital,
    /// A DAC tone over the tail of Jr.'s power-on sound: its level and strongest frequency.
    Tone,
    /// The analogue effects (and Jr.'s power-on sound, and their tails): level and spectrum.
    Analogue,
}

/// The script's segments (SEG, as each frame ends): the frames each covers, 1-based, inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub seg: u8,
    pub first: usize,
    pub last: usize,
}

pub fn segments(frames: &[FrameOut]) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut start = 0;
    for i in 1..=frames.len() {
        if i == frames.len() || frames[i].sound_state.0 != frames[start].sound_state.0 {
            out.push(Segment { seg: frames[start].sound_state.0, first: start + 1, last: i });
            start = i;
        }
    }
    out
}

pub fn kind(case: &Case, seg: u8) -> Kind {
    match case.name {
        "jrsound" => match seg {
            1..=3 => Kind::Tone,
            4..=16 => Kind::Digital,
            _ => Kind::Analogue,
        },
        _ => Kind::Digital,
    }
}

/// The frames a segment's measures are taken over: its own, less the first two (the change
/// settling) and the last (the next step comes at its end).
pub fn measured_frames(s: &Segment) -> Option<(usize, usize)> {
    let (a, b) = (s.first + 2, s.last.saturating_sub(1));
    (b >= a + 4).then_some((a, b))
}

/// Frame `n` (1-based) of a run's sound.
pub fn frame(audio: &[i16], n: usize) -> &[i16] {
    &audio[(n - 1) * SAMPLES..n * SAMPLES]
}

pub fn measure_frames(audio: &[i16], first: usize, last: usize) -> Measure {
    measure::measure(&audio[(first - 1) * SAMPLES..last * SAMPLES], RATE)
}

/// The tolerances. A frame of the DAC alone: its level and mean within this much of full scale
/// (a step is 1/32767). A tone: its level within 1%, its strongest frequency within 0.5%. The
/// analogue effects: the level within 5% and the twelfth-octave spectrum at least 0.98 alike; a
/// quiet stretch (under 1% of full scale on both) passes on its level.
pub const FRAME_LEVEL: f64 = 0.0002;
pub const TONE_LEVEL: f64 = 0.01;
pub const TONE_FREQ: f64 = 0.005;
pub const ANALOGUE_LEVEL: f64 = 0.05;
pub const ANALOGUE_LIKENESS: f64 = 0.98;
pub const QUIET: f64 = 0.01;

pub fn compare_frame(n: usize, ours: Level, mame: Level) -> Result<(), String> {
    if (ours.rms - mame.rms).abs() > FRAME_LEVEL || (ours.mean - mame.mean).abs() > FRAME_LEVEL {
        return Err(format!(
            "frame {n}: level {:.5} mean {:.5}, MAME {:.5} mean {:.5}",
            ours.rms, ours.mean, mame.rms, mame.mean
        ));
    }
    Ok(())
}

pub fn compare_segment(kind: Kind, s: &Segment, ours: &Measure, mame: &Measure) -> Result<(), String> {
    let rel = |a: f64, b: f64| (a - b).abs() / b.abs().max(1e-9);
    let name = format!("segment {} (frames {}-{})", s.seg, s.first, s.last);
    match kind {
        Kind::Digital => Ok(()),
        Kind::Tone => {
            if rel(ours.rms, mame.rms) > TONE_LEVEL {
                return Err(format!("{name}: level {:.4}, MAME {:.4}", ours.rms, mame.rms));
            }
            if rel(ours.freq, mame.freq) > TONE_FREQ {
                return Err(format!("{name}: strongest at {:.1} Hz, MAME {:.1} Hz", ours.freq, mame.freq));
            }
            Ok(())
        }
        Kind::Analogue => {
            if ours.rms < QUIET && mame.rms < QUIET {
                return Ok(());
            }
            if rel(ours.rms, mame.rms) > ANALOGUE_LEVEL {
                return Err(format!("{name}: level {:.4}, MAME {:.4}", ours.rms, mame.rms));
            }
            let l = measure::likeness(&ours.fine, &mame.fine);
            if l < ANALOGUE_LIKENESS {
                return Err(format!("{name}: the spectrum is {l:.3} like MAME's, {ANALOGUE_LIKENESS} needed"));
            }
            Ok(())
        }
    }
}

/// Every frame and segment of a run's sound against MAME's levels and measures; the failures.
pub fn compare_run(case: &Case, ours: &[FrameOut], mame_levels: &[Level], mame_segments: &[(Segment, Measure)]) -> Vec<String> {
    let audio: Vec<i16> = ours.iter().flat_map(|f| f.audio.iter().copied()).collect();
    let segs = segments(ours);
    let mut bad = Vec::new();
    for s in &segs {
        if kind(case, s.seg) != Kind::Digital {
            continue;
        }
        for n in s.first..=s.last {
            if let Err(e) = compare_frame(n, level(frame(&audio, n)), mame_levels[n - 1]) {
                bad.push(format!("{}: {e}", case.name));
            }
        }
    }
    let theirs: Vec<Segment> = mame_segments.iter().map(|m| m.0).collect();
    let measured: Vec<Segment> = segs.iter().copied().filter(|s| measured_frames(s).is_some()).collect();
    if theirs != measured {
        bad.push(format!("{}: the script's segments aren't where MAME's run had them", case.name));
        return bad;
    }
    for (s, m) in mame_segments {
        let (a, b) = measured_frames(s).unwrap();
        if let Err(e) = compare_segment(kind(case, s.seg), s, &measure_frames(&audio, a, b), m) {
            bad.push(format!("{}: {e}", case.name));
        }
    }
    bad
}
