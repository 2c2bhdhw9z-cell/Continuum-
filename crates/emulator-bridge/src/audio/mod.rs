//! Audio pipeline: cores push PCM in, the host pulls PCM out.
//!
//! ```text
//!  core step            AudioSink (Rust, this module)              host
//!  ─────────            ────────────────────────────               ────
//!  submit_i16 ─┐                                          ┌─ drain(&mut [f32])
//!  submit_f32 ─┴─▶ i16→f32 ─▶ resample ─▶ ring buffer ────┴─▶ AudioWorklet (web)
//!                  (no heap)   (no heap)   (pre-allocated)     AURenderCallback (iOS)
//! ```
//!
//! Two properties this design protects:
//!
//! 1. **No per-frame allocation.** The ring is sized once per session; conversion
//!    and resampling use fixed scratch buffers owned by the sink.
//! 2. **The UI never sees sample rates.** Cores submit at their native rate, the
//!    host asks for its device rate, and the sink reconciles them. The web UI and
//!    Phase 2's Swift UI both just call `drain`.
//!
//! Libretro's two audio callbacks map onto this directly, which is the whole point
//! of the seam: `audio_sample(l, r)` → [`AudioSink::submit_i16`] with a 2-sample
//! slice, and `audio_sample_batch(data, frames)` → the same call with the batch.

mod resample;
mod ring;

pub use resample::Resampler;
pub use ring::{AudioRing, CHANNELS};

/// Output format description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioSpec {
    /// Rate the *core* produces.
    pub source_rate: u32,
    /// Rate the *device* consumes (`AudioContext.sampleRate`).
    pub output_rate: u32,
    pub channels: u32,
}

impl Default for AudioSpec {
    fn default() -> Self {
        Self {
            source_rate: 48_000,
            output_rate: 48_000,
            channels: CHANNELS as u32,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioStats {
    pub queued_frames: u32,
    pub capacity_frames: u32,
    /// Buffer filled up and old audio was dropped — core is outrunning the device.
    pub overruns: u64,
    /// Device asked for more than was queued — a gap was padded with silence.
    pub underruns: u64,
    pub frames_submitted: u64,
    pub frames_drained: u64,
}

/// The audio seam. Cores only ever see this trait.
///
/// `&mut dyn AudioSink` is handed to the core during its step, so a core cannot
/// hold onto the sink or hand it to another thread — which keeps the
/// single-writer invariant of the ring buffer structurally true rather than
/// merely documented.
pub trait AudioSink: crate::MaybeSend {
    fn spec(&self) -> AudioSpec;

    /// Submits interleaved stereo `f32` in `-1.0..=1.0`.
    fn submit_f32(&mut self, interleaved: &[f32]);

    /// Submits interleaved stereo `i16` — the native libretro format.
    ///
    /// Implementations must convert without allocating.
    fn submit_i16(&mut self, interleaved: &[i16]);

    /// Fills `dst` with queued audio, returning the number of samples written.
    /// A short fill is an underrun and the caller pads the remainder with silence.
    fn drain(&mut self, dst: &mut [f32]) -> usize;

    fn stats(&self) -> AudioStats;

    /// Called on reset / load-state / seek, where continuing to play buffered audio
    /// from the abandoned timeline would be wrong.
    fn flush(&mut self);

    /// Informs the sink of the device rate once the host's audio graph exists.
    /// Called after the user gesture that unlocks `AudioContext`, since the real
    /// `sampleRate` is not knowable before then.
    fn set_output_rate(&mut self, output_rate: u32);
}

/// Scratch size for `i16` → `f32` conversion. Stack-resident, so conversion of an
/// arbitrarily long batch proceeds in chunks with zero heap traffic.
const CONVERT_CHUNK: usize = 1024;

/// The production sink: convert → resample → pre-allocated ring.
#[derive(Debug)]
pub struct RingAudioSink {
    ring: AudioRing,
    resampler: Resampler,
    channels: u32,
    /// Resampler output staging. Retains its capacity between frames.
    scratch: Vec<f32>,
}

impl RingAudioSink {
    /// `latency_frames` is video frames of audio to buffer (2–4 is typical).
    pub fn new(source_rate: u32, output_rate: u32, target_fps: f64, latency_frames: usize) -> Self {
        let fps = if target_fps.is_finite() && target_fps > 1.0 {
            target_fps
        } else {
            60.0
        };
        let rate = output_rate.max(source_rate).max(8_000) as f64;
        let frames_per_video_frame = (rate / fps).ceil() as usize;
        let capacity_frames = frames_per_video_frame * latency_frames.clamp(2, 8);

        let resampler = Resampler::new(source_rate, output_rate);
        // Pre-size the staging buffer for one video frame's worth of audio plus
        // slack, so steady-state `process` calls never reallocate.
        let scratch = Vec::with_capacity(
            resampler.max_output_samples(frames_per_video_frame * CHANNELS) + 64,
        );

        Self {
            ring: AudioRing::new(capacity_frames),
            resampler,
            channels: CHANNELS as u32,
            scratch,
        }
    }

    /// Convenience constructor for a core descriptor's declared rate.
    pub fn for_core(source_rate: u32, target_fps: f64) -> Self {
        Self::new(source_rate, source_rate, target_fps, 3)
    }

    pub fn ring(&self) -> &AudioRing {
        &self.ring
    }

    pub fn queued_frames(&self) -> usize {
        self.ring.available_frames()
    }
}

impl AudioSink for RingAudioSink {
    fn spec(&self) -> AudioSpec {
        AudioSpec {
            source_rate: self.resampler.source_rate(),
            output_rate: self.resampler.output_rate(),
            channels: self.channels,
        }
    }

    fn submit_f32(&mut self, interleaved: &[f32]) {
        if interleaved.is_empty() {
            return;
        }
        if self.resampler.is_passthrough() {
            self.ring.push(interleaved);
            return;
        }
        self.scratch.clear();
        self.resampler.process(interleaved, &mut self.scratch);
        // Split borrow: `scratch` is read while `ring` is mutated.
        let Self { ring, scratch, .. } = self;
        ring.push(scratch);
    }

    fn submit_i16(&mut self, interleaved: &[i16]) {
        const SCALE: f32 = 1.0 / 32_768.0;
        let mut chunk = [0.0f32; CONVERT_CHUNK];
        for block in interleaved.chunks(CONVERT_CHUNK) {
            let dst = &mut chunk[..block.len()];
            for (d, &s) in dst.iter_mut().zip(block) {
                *d = s as f32 * SCALE;
            }
            // Chunk boundaries are sample-aligned but may split a stereo frame;
            // CONVERT_CHUNK is even, so frames stay intact.
            self.submit_f32(dst);
        }
    }

    fn drain(&mut self, dst: &mut [f32]) -> usize {
        self.ring.drain_into(dst)
    }

    fn stats(&self) -> AudioStats {
        AudioStats {
            queued_frames: self.ring.available_frames() as u32,
            capacity_frames: self.ring.capacity_frames() as u32,
            overruns: self.ring.overruns(),
            underruns: self.ring.underruns(),
            frames_submitted: self.ring.frames_written(),
            frames_drained: self.ring.frames_read(),
        }
    }

    fn flush(&mut self) {
        self.ring.clear();
        self.resampler.reset();
    }

    fn set_output_rate(&mut self, output_rate: u32) {
        if output_rate == self.resampler.output_rate() {
            return;
        }
        self.resampler.set_output_rate(output_rate);
        // Buffered audio was resampled for the old rate; playing it at the new one
        // would pitch-shift the tail.
        self.ring.clear();
    }
}

/// Discards everything. Used by headless tests and by sessions running muted, so
/// the tick shape stays identical whether or not audio is live.
#[derive(Debug, Default)]
pub struct NullAudioSink {
    spec: AudioSpec,
    frames_submitted: u64,
}

impl NullAudioSink {
    pub fn new() -> Self {
        Self::default()
    }
}

impl AudioSink for NullAudioSink {
    fn spec(&self) -> AudioSpec {
        self.spec
    }

    fn submit_f32(&mut self, interleaved: &[f32]) {
        self.frames_submitted += (interleaved.len() / CHANNELS) as u64;
    }

    fn submit_i16(&mut self, interleaved: &[i16]) {
        self.frames_submitted += (interleaved.len() / CHANNELS) as u64;
    }

    fn drain(&mut self, _dst: &mut [f32]) -> usize {
        0
    }

    fn stats(&self) -> AudioStats {
        AudioStats {
            frames_submitted: self.frames_submitted,
            ..Default::default()
        }
    }

    fn flush(&mut self) {}

    fn set_output_rate(&mut self, output_rate: u32) {
        self.spec.output_rate = output_rate;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i16_submission_converts_to_unit_range() {
        let mut sink = RingAudioSink::new(48_000, 48_000, 60.0, 3);
        sink.submit_i16(&[i16::MAX, i16::MIN, 0, 0]);
        let mut out = [0.0; 4];
        assert_eq!(sink.drain(&mut out), 4);
        assert!((out[0] - 1.0).abs() < 1e-3, "{}", out[0]);
        assert!((out[1] + 1.0).abs() < 1e-3, "{}", out[1]);
        assert_eq!(out[2], 0.0);
    }

    #[test]
    fn large_i16_batch_survives_chunking() {
        let mut sink = RingAudioSink::new(48_000, 48_000, 60.0, 4);
        let batch: Vec<i16> = (0..CONVERT_CHUNK * 3).map(|i| (i % 100) as i16).collect();
        sink.submit_i16(&batch);
        assert_eq!(
            sink.stats().frames_submitted,
            (batch.len() / CHANNELS) as u64
        );
    }

    #[test]
    fn rate_mismatch_produces_more_output_than_input() {
        let mut sink = RingAudioSink::new(32_040, 48_000, 60.0, 3);
        let frames = 534; // one SNES frame at 32040 Hz / 60 fps
        sink.submit_f32(&vec![0.1; frames * CHANNELS]);
        assert!(
            sink.queued_frames() > frames,
            "expected upsampling, queued {}",
            sink.queued_frames()
        );
    }

    #[test]
    fn ring_is_sized_for_requested_latency() {
        let sink = RingAudioSink::new(48_000, 48_000, 60.0, 3);
        // 3 video frames at 48 kHz / 60 fps = 2400 frames.
        assert_eq!(sink.ring().capacity_frames(), 2400);
    }

    #[test]
    fn flush_drops_queued_audio() {
        let mut sink = RingAudioSink::new(48_000, 48_000, 60.0, 3);
        sink.submit_f32(&[0.5; 128]);
        sink.flush();
        assert_eq!(sink.queued_frames(), 0);
    }

    #[test]
    fn changing_output_rate_reconfigures_resampler() {
        let mut sink = RingAudioSink::new(44_100, 44_100, 60.0, 3);
        assert!(sink.spec().output_rate == 44_100);
        sink.set_output_rate(48_000);
        assert_eq!(sink.spec().output_rate, 48_000);
        assert_eq!(sink.queued_frames(), 0);
    }

    #[test]
    fn steady_state_submission_does_not_grow_scratch() {
        // Proxy for "no per-frame allocation": capacity must stabilise.
        let mut sink = RingAudioSink::new(32_040, 48_000, 60.0, 3);
        let frame = vec![0.2; 534 * CHANNELS];
        let mut drain = vec![0.0; 800 * CHANNELS];
        sink.submit_f32(&frame);
        sink.drain(&mut drain);
        let baseline = sink.scratch.capacity();
        for _ in 0..600 {
            sink.submit_f32(&frame);
            sink.drain(&mut drain);
        }
        assert_eq!(sink.scratch.capacity(), baseline);
    }
}
