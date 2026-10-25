//! Telemetry: the files that say what really happened on a phone, and the package that carries
//! them off it (owner, 25 October 2026: put the telemetry in to get what is needed).
//!
//! The always-on part is in `telemetry.c` (every emulator's log lines, a crash catcher, a frame
//! watchdog that samples where the game thread is during a hitch or a freeze). This side adds:
//!
//! - **The sound recorder.** The last 30 seconds of exactly what the emulator handed the app,
//!   at the emulator's own rate, kept in memory. The package writes it out as `sound.wav`, so the
//!   sound can be listened to and measured off the phone.
//! - **Per-second figures**, as `app:` lines in the core log: frames run, how long a frame took
//!   (average and worst), the longest gap between two frames, and how much sound came out.
//! - **The package**: one .zip with all of it, for the app to upload.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[cfg(feature = "native-core")]
mod native {
    use core::ffi::c_char;
    extern "C" {
        pub fn continuum_tel_open(dir: *const c_char);
        pub fn continuum_tel_note(text: *const c_char);
        pub fn continuum_tel_frame_begin();
        pub fn continuum_tel_frame_end();
    }
}

static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Starts the files in `dir` (the same folder as the activity log). Once per launch.
pub fn open(dir: &Path) {
    *DIR.lock().unwrap_or_else(|p| p.into_inner()) = Some(dir.to_path_buf());
    #[cfg(feature = "native-core")]
    if let Ok(c) = std::ffi::CString::new(dir.to_string_lossy().as_bytes()) {
        // SAFETY: a NUL-terminated path for the length of the call.
        unsafe { native::continuum_tel_open(c.as_ptr()) };
    }
}

/// One `app:` line in the core log.
pub fn note(text: &str) {
    #[cfg(feature = "native-core")]
    if let Ok(c) = std::ffi::CString::new(text.replace('\0', " ")) {
        // SAFETY: a NUL-terminated line for the length of the call.
        unsafe { native::continuum_tel_note(c.as_ptr()) };
    }
    #[cfg(not(feature = "native-core"))]
    let _ = text;
}

/// Around `retro_run`, for the watchdog.
pub fn frame_begin() {
    // SAFETY: no arguments; the C side only stores the time and the thread.
    #[cfg(feature = "native-core")]
    unsafe {
        native::continuum_tel_frame_begin()
    };
}

pub fn frame_end() {
    // SAFETY: as above.
    #[cfg(feature = "native-core")]
    unsafe {
        native::continuum_tel_frame_end()
    };
}

// ------------------------------------------------------------------------- the sound recorder

const KEEP_SECONDS: usize = 30;

struct Recorder {
    core: String,
    rate: u32,
    /// Interleaved stereo.
    samples: VecDeque<i16>,
    // Per-second figures.
    second_start: Option<Instant>,
    frames: u32,
    run_total: Duration,
    run_max: Duration,
    gap_max: Duration,
    last_end: Option<Instant>,
    sound_frames: u64,
    seconds: u64,
}

static RECORDER: Mutex<Option<Recorder>> = Mutex::new(None);

/// A game started on `core` at `rate` Hz: the recording starts again.
pub fn game_started(core: &str, rate: u32) {
    *RECORDER.lock().unwrap_or_else(|p| p.into_inner()) = Some(Recorder {
        core: core.to_string(),
        rate: rate.max(1),
        samples: VecDeque::with_capacity(rate.max(1) as usize * 2 * KEEP_SECONDS),
        second_start: None,
        frames: 0,
        run_total: Duration::ZERO,
        run_max: Duration::ZERO,
        gap_max: Duration::ZERO,
        last_end: None,
        sound_frames: 0,
        seconds: 0,
    });
    note(&format!("game started: {core}, sound at {rate} Hz"));
}

/// The emulator's sample rate changed mid-game.
pub fn rate_changed(rate: u32) {
    let mut guard = RECORDER.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(r) = guard.as_mut() {
        if r.rate != rate && rate > 0 {
            note(&format!("sound rate changed {} -> {rate} Hz (recording restarted)", r.rate));
            r.rate = rate;
            r.samples.clear();
        }
    }
}

/// After one frame: how long it took, and the sound it made.
pub fn frame_done(started: Instant, run: Duration, sound: &[i16]) {
    let mut guard = RECORDER.lock().unwrap_or_else(|p| p.into_inner());
    let Some(r) = guard.as_mut() else { return };
    let cap = r.rate as usize * 2 * KEEP_SECONDS;
    r.samples.extend(sound.iter().copied());
    let over = r.samples.len().saturating_sub(cap);
    if over > 0 {
        r.samples.drain(..over);
    }
    if let Some(prev) = r.last_end {
        r.gap_max = r.gap_max.max(started.saturating_duration_since(prev));
    }
    r.last_end = Some(started + run);
    r.frames += 1;
    r.run_total += run;
    r.run_max = r.run_max.max(run);
    r.sound_frames += (sound.len() / 2) as u64;
    let begun = *r.second_start.get_or_insert(started);
    let span = started.saturating_duration_since(begun);
    if span >= Duration::from_secs(1) {
        r.seconds += 1;
        let line = format!(
            "second {}: {} frames, frame time avg {:.1} ms max {:.1} ms, longest gap {:.1} ms, \
             sound {} frames ({:.0}/s at {} Hz)",
            r.seconds,
            r.frames,
            r.run_total.as_secs_f64() * 1000.0 / r.frames.max(1) as f64,
            r.run_max.as_secs_f64() * 1000.0,
            r.gap_max.as_secs_f64() * 1000.0,
            r.sound_frames,
            r.sound_frames as f64 / span.as_secs_f64(),
            r.rate
        );
        r.second_start = Some(started);
        r.frames = 0;
        r.run_total = Duration::ZERO;
        r.run_max = Duration::ZERO;
        r.gap_max = Duration::ZERO;
        r.sound_frames = 0;
        drop(guard);
        note(&line);
    }
}

/// A 16-bit stereo WAV of the recording, or None when nothing was recorded.
pub fn sound_wav() -> Option<(String, Vec<u8>)> {
    let guard = RECORDER.lock().unwrap_or_else(|p| p.into_inner());
    let r = guard.as_ref()?;
    if r.samples.is_empty() {
        return None;
    }
    Some((r.core.clone(), wav(r.rate, r.samples.iter().copied())))
}

fn wav(rate: u32, samples: impl ExactSizeIterator<Item = i16>) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&2u16.to_le_bytes()); // stereo
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

// --------------------------------------------------------------------------------- the package

/// Writes `continuum-diagnostics.zip` into `out_dir` and returns its path: the core logs, crash
/// files and activity logs of this launch and the two before it, the sound recording, and
/// `about.txt` (`about` from the app: build, phone, JIT).
pub fn package(out_dir: &Path, about: &str) -> Result<PathBuf, String> {
    let dir = DIR
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .ok_or("telemetry was not started")?;
    let staging = out_dir.join("continuum-diagnostics");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    for base in ["core-log", "crash", "activity"] {
        for suffix in ["", "-previous", "-older"] {
            let name = format!("{base}{suffix}.txt");
            let from = dir.join(&name);
            if from.is_file() {
                std::fs::copy(&from, staging.join(&name)).map_err(|e| e.to_string())?;
            }
        }
    }
    let mut about = about.to_string();
    about.push_str(&format!("\n{}\n", crate::jit::technical_line()));
    if let Some((core, bytes)) = sound_wav() {
        about.push_str(&format!("sound.wav: the last {} s from {core}\n", bytes.len() / 4 / sample_rate().max(1) as usize));
        std::fs::write(staging.join("sound.wav"), bytes).map_err(|e| e.to_string())?;
    }
    std::fs::write(staging.join("about.txt"), about).map_err(|e| e.to_string())?;
    let zip = out_dir.join("continuum-diagnostics.zip");
    crate::import::archive::zip_directory(&staging, &zip)?;
    let _ = std::fs::remove_dir_all(&staging);
    Ok(zip)
}

fn sample_rate() -> u32 {
    RECORDER.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map_or(0, |r| r.rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recording_keeps_thirty_seconds_and_writes_a_wav() {
        game_started("test", 1000);
        let t = Instant::now();
        for i in 0..40 {
            frame_done(t + Duration::from_millis(i * 1000), Duration::from_millis(5), &vec![1i16; 2000]);
        }
        let (core, bytes) = sound_wav().unwrap();
        assert_eq!(core, "test");
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(bytes.len(), 44 + 30 * 1000 * 4);
    }
}
