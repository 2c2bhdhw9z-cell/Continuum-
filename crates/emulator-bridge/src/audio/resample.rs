//! Sample-rate conversion between a core's clock and the output device.
//!
//! Cores run at whatever rate their hardware did — 32040 Hz (SNES), 44100 Hz
//! (PS1), 48000 Hz (modern) — while `AudioContext.sampleRate` is decided by the
//! browser/OS (44100 or 48000). Something must reconcile the two, and doing it in
//! Rust keeps the behaviour identical on web and iOS.
//!
//! This is a linear interpolator: cheap, allocation-free, and adequate for the
//! scaffold. It aliases on aggressive downsampling.
//!
//! TODO(phase1b): replace with a windowed-sinc / polyphase FIR before shipping
//! audio quality anyone would judge. The [`Resampler`] surface stays the same.

use super::CHANNELS;

#[derive(Debug)]
pub struct Resampler {
    source_rate: f64,
    output_rate: f64,
    /// Input frames consumed per output frame.
    ratio: f64,
    /// Fractional read position within the input stream.
    position: f64,
    /// Trailing frame of the previous batch, so interpolation is continuous
    /// across `process` calls instead of clicking at every boundary.
    last_frame: [f32; CHANNELS],
    primed: bool,
}

impl Resampler {
    pub fn new(source_rate: u32, output_rate: u32) -> Self {
        let source_rate = if source_rate == 0 {
            48_000
        } else {
            source_rate
        } as f64;
        let output_rate = if output_rate == 0 {
            48_000
        } else {
            output_rate
        } as f64;
        Self {
            source_rate,
            output_rate,
            ratio: source_rate / output_rate,
            position: 0.0,
            last_frame: [0.0; CHANNELS],
            primed: false,
        }
    }

    pub fn source_rate(&self) -> u32 {
        self.source_rate as u32
    }

    pub fn output_rate(&self) -> u32 {
        self.output_rate as u32
    }

    /// True when input and output rates match closely enough to bypass conversion.
    pub fn is_passthrough(&self) -> bool {
        (self.ratio - 1.0).abs() < 1e-9
    }

    pub fn set_output_rate(&mut self, output_rate: u32) {
        if output_rate > 0 {
            self.output_rate = output_rate as f64;
            self.ratio = self.source_rate / self.output_rate;
        }
    }

    pub fn set_source_rate(&mut self, source_rate: u32) {
        if source_rate > 0 {
            self.source_rate = source_rate as f64;
            self.ratio = self.source_rate / self.output_rate;
        }
    }

    pub fn reset(&mut self) {
        self.position = 0.0;
        self.last_frame = [0.0; CHANNELS];
        self.primed = false;
    }

    /// Upper bound on output samples for a given input length. Lets callers size a
    /// scratch buffer once instead of growing it mid-stream.
    pub fn max_output_samples(&self, input_samples: usize) -> usize {
        let in_frames = input_samples / CHANNELS;
        ((in_frames as f64 / self.ratio).ceil() as usize + 2) * CHANNELS
    }

    /// Resamples `input` (interleaved stereo) into `out`, appending to it.
    ///
    /// `out` is caller-owned and reused across frames; it is only ever extended up
    /// to its existing capacity in steady state, so this does not allocate once the
    /// session has warmed up.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }
        let in_frames = input.len() / CHANNELS;
        if in_frames == 0 {
            return;
        }

        if self.is_passthrough() {
            out.extend_from_slice(&input[..in_frames * CHANNELS]);
            return;
        }

        if !self.primed {
            self.last_frame = [input[0], input[1]];
            self.primed = true;
        }

        // `position` is relative to the start of `input`; index -1 refers to the
        // carried-over frame from the previous call.
        while self.position < in_frames as f64 {
            let idx = self.position.floor() as isize;
            let frac = (self.position - idx as f64) as f32;

            let a = if idx < 0 {
                self.last_frame
            } else {
                let base = idx as usize * CHANNELS;
                [input[base], input[base + 1]]
            };
            let b_idx = idx + 1;
            let b = if (b_idx as usize) < in_frames {
                let base = b_idx as usize * CHANNELS;
                [input[base], input[base + 1]]
            } else {
                a
            };

            out.push(a[0] + (b[0] - a[0]) * frac);
            out.push(a[1] + (b[1] - a[1]) * frac);
            self.position += self.ratio;
        }

        self.position -= in_frames as f64;
        let last = (in_frames - 1) * CHANNELS;
        self.last_frame = [input[last], input[last + 1]];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_is_bit_exact() {
        let mut r = Resampler::new(48_000, 48_000);
        assert!(r.is_passthrough());
        let input = [0.1, 0.2, 0.3, 0.4];
        let mut out = Vec::new();
        r.process(&input, &mut out);
        assert_eq!(out, input);
    }

    #[test]
    fn upsampling_roughly_doubles_frame_count() {
        let mut r = Resampler::new(24_000, 48_000);
        let input: Vec<f32> = (0..200).map(|i| i as f32).collect(); // 100 frames
        let mut out = Vec::new();
        r.process(&input, &mut out);
        let out_frames = out.len() / CHANNELS;
        assert!((198..=202).contains(&out_frames), "got {out_frames} frames");
    }

    #[test]
    fn downsampling_roughly_halves_frame_count() {
        let mut r = Resampler::new(48_000, 24_000);
        let input: Vec<f32> = (0..200).map(|i| i as f32).collect();
        let mut out = Vec::new();
        r.process(&input, &mut out);
        let out_frames = out.len() / CHANNELS;
        assert!((49..=51).contains(&out_frames), "got {out_frames} frames");
    }

    #[test]
    fn output_stays_within_max_estimate() {
        let mut r = Resampler::new(32_040, 48_000);
        let input: Vec<f32> = (0..1068).map(|i| (i % 7) as f32).collect();
        let budget = r.max_output_samples(input.len());
        let mut out = Vec::new();
        r.process(&input, &mut out);
        assert!(out.len() <= budget, "{} > {budget}", out.len());
    }

    #[test]
    fn streaming_matches_ratio_over_many_batches() {
        // Drift is the real risk: fractional position must carry across calls.
        let mut r = Resampler::new(32_040, 48_000);
        let batch: Vec<f32> = vec![0.25; 534 * CHANNELS];
        let mut total_out = 0usize;
        let mut out = Vec::new();
        for _ in 0..100 {
            out.clear();
            r.process(&batch, &mut out);
            total_out += out.len() / CHANNELS;
        }
        let expected = (534.0 * 100.0 * 48_000.0 / 32_040.0) as usize;
        let drift = (total_out as isize - expected as isize).abs();
        assert!(drift <= 2, "drifted {drift} frames over 100 batches");
    }
}
