//! Pre-allocated circular buffer for interleaved stereo PCM.
//!
//! Allocated once when a session starts and never again: `push` and `drain_into`
//! only move cursors and copy into existing storage. Nothing in the audio path
//! touches the heap after construction, which is what keeps a 60 Hz tick free of
//! allocator jitter.
//!
//! Single writer (the core step), single reader (the host drain), same thread —
//! so no locks and no atomics are required.

/// Interleaved stereo. One *frame* = one sample per channel.
pub const CHANNELS: usize = 2;

#[derive(Debug)]
pub struct AudioRing {
    buf: Box<[f32]>,
    /// Read cursor, in samples (not frames).
    head: usize,
    /// Write cursor, in samples.
    tail: usize,
    len: usize,
    overruns: u64,
    underruns: u64,
    frames_written: u64,
    frames_read: u64,
}

impl AudioRing {
    /// `capacity_frames` is the latency ceiling. Too small and a slow tick starves
    /// the device; too large and input-to-sound lag becomes audible. Roughly four
    /// video frames' worth is the usual compromise.
    pub fn new(capacity_frames: usize) -> Self {
        let capacity = capacity_frames.max(256) * CHANNELS;
        Self {
            buf: vec![0.0; capacity].into_boxed_slice(),
            head: 0,
            tail: 0,
            len: 0,
            overruns: 0,
            underruns: 0,
            frames_written: 0,
            frames_read: 0,
        }
    }

    pub fn capacity_samples(&self) -> usize {
        self.buf.len()
    }

    pub fn capacity_frames(&self) -> usize {
        self.buf.len() / CHANNELS
    }

    pub fn available_samples(&self) -> usize {
        self.len
    }

    pub fn available_frames(&self) -> usize {
        self.len / CHANNELS
    }

    pub fn overruns(&self) -> u64 {
        self.overruns
    }

    pub fn underruns(&self) -> u64 {
        self.underruns
    }

    pub fn frames_written(&self) -> u64 {
        self.frames_written
    }

    pub fn frames_read(&self) -> u64 {
        self.frames_read
    }

    pub fn clear(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.len = 0;
    }

    /// Appends interleaved samples.
    ///
    /// On overflow the *oldest* audio is discarded. Dropping the newest instead
    /// would keep the buffer permanently full and turn a transient hiccup into
    /// sustained latency; dropping the oldest costs one audible discontinuity and
    /// then recovers.
    pub fn push(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let cap = self.buf.len();

        if samples.len() >= cap {
            // Burst exceeds the entire ring: keep only the newest `cap` samples.
            let start = samples.len() - cap;
            self.buf.copy_from_slice(&samples[start..]);
            self.head = 0;
            self.tail = 0;
            self.len = cap;
            self.overruns += 1;
            self.frames_written += (samples.len() / CHANNELS) as u64;
            return;
        }

        // Two memcpy runs at most, rather than a per-sample modulo loop.
        let first = (cap - self.tail).min(samples.len());
        self.buf[self.tail..self.tail + first].copy_from_slice(&samples[..first]);
        let rest = samples.len() - first;
        if rest > 0 {
            self.buf[..rest].copy_from_slice(&samples[first..]);
        }
        self.tail = (self.tail + samples.len()) % cap;

        let free = cap - self.len;
        if samples.len() > free {
            // Writer lapped the reader; drag the read cursor along.
            let lost = samples.len() - free;
            self.head = (self.head + lost) % cap;
            self.len = cap;
            self.overruns += 1;
        } else {
            self.len += samples.len();
        }
        self.frames_written += (samples.len() / CHANNELS) as u64;
    }

    /// Copies up to `dst.len()` samples into `dst`, returning the count written.
    /// A short read is a legitimate underrun; the caller pads with silence.
    pub fn drain_into(&mut self, dst: &mut [f32]) -> usize {
        let cap = self.buf.len();
        let n = dst.len().min(self.len);
        if n > 0 {
            let first = (cap - self.head).min(n);
            dst[..first].copy_from_slice(&self.buf[self.head..self.head + first]);
            let rest = n - first;
            if rest > 0 {
                dst[first..n].copy_from_slice(&self.buf[..rest]);
            }
            self.head = (self.head + n) % cap;
            self.len -= n;
            self.frames_read += (n / CHANNELS) as u64;
        }
        if n < dst.len() {
            self.underruns += 1;
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drains_in_order() {
        let mut ring = AudioRing::new(256);
        ring.push(&[1.0, 2.0, 3.0, 4.0]);
        let mut out = [0.0; 4];
        assert_eq!(ring.drain_into(&mut out), 4);
        assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(ring.available_frames(), 0);
    }

    #[test]
    fn wraps_without_corruption() {
        let mut ring = AudioRing::new(256);
        let cap = ring.capacity_samples();
        // Push/drain enough to wrap the cursors several times.
        let chunk: Vec<f32> = (0..100).map(|i| i as f32).collect();
        let mut out = vec![0.0; 100];
        for _ in 0..(cap / 100 * 3) {
            ring.push(&chunk);
            assert_eq!(ring.drain_into(&mut out), 100);
            assert_eq!(out, chunk);
        }
        assert_eq!(ring.overruns(), 0);
    }

    #[test]
    fn overrun_drops_oldest_and_counts() {
        let mut ring = AudioRing::new(256); // 512 samples
        let cap = ring.capacity_samples();
        let burst: Vec<f32> = (0..cap + 4).map(|i| i as f32).collect();
        ring.push(&burst);
        assert!(ring.overruns() > 0);
        assert_eq!(ring.available_samples(), cap);
        let mut out = [0.0; 1];
        ring.drain_into(&mut out);
        // The oldest 4 samples were discarded, so reading resumes at index 4.
        assert_eq!(out[0], 4.0);
    }

    #[test]
    fn partial_overflow_keeps_newest() {
        let mut ring = AudioRing::new(256);
        let cap = ring.capacity_samples();
        ring.push(&vec![0.0; cap - 2]);
        ring.push(&[1.0, 2.0, 3.0, 4.0]); // 2 samples too many
        assert_eq!(ring.overruns(), 1);
        assert_eq!(ring.available_samples(), cap);
        let mut out = vec![0.0; cap];
        ring.drain_into(&mut out);
        assert_eq!(&out[cap - 4..], &[1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn short_read_reports_underrun() {
        let mut ring = AudioRing::new(256);
        ring.push(&[0.5, 0.5]);
        let mut out = [0.0; 8];
        assert_eq!(ring.drain_into(&mut out), 2);
        assert_eq!(ring.underruns(), 1);
    }
}
