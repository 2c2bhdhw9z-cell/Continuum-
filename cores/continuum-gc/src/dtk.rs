//! Music streamed by the disc drive itself ("DTK", gamecube.json G5): the drive decodes ADPCM
//! music straight off the disc while the game runs, and the audio interface mixes it in at its
//! own rate and volume. Ikaruga, 1080 Avalanche, Wave Race: Blue Storm, Eternal Darkness and Super
//! Monkey Ball play their music this way; without it, it is silent.
//!
//! The stream is 32-byte blocks of 28 stereo samples. Byte 0 is the left channel's header and
//! byte 1 the right's (bytes 2 and 3 repeat them): the top nibble picks one of four prediction
//! filters, the low nibble a shift. Each of the 28 data bytes holds one sample of each channel,
//! the left one in the low nibble. A sample is its 4-bit code placed at the top of 16 bits and
//! shifted down by the header's shift, plus the filter's prediction from that channel's last two
//! samples (kept with 6 more bits), with coefficients 0/0, 60/0, 115/-52 and 98/-55 in 64ths (the
//! same four as CD-XA's ADPCM).
//!
//! The drive's commands (DI E1): play a track (byte offset, length), which also becomes the
//! track it goes on with at the end, so a track loops until told otherwise; another play while
//! one is playing only changes what comes next (how games chain music); a length of 0 stops at
//! the end of the track; subcommand 1 stops at once. E2 reports whether it is playing, where it
//! is (in 32 KB steps), where the track starts and how long it is.

use crate::disc::Disc;
use serde::{Deserialize, Serialize};

const BLOCK: usize = 32;
const SAMPLES: usize = 28;
/// Bytes read from the disc at a time.
const READ_AHEAD: usize = 8192;
/// The prediction filters' coefficients, in 64ths, for the last sample and the one before it.
const FILTERS: [(i32, i32); 4] = [(0, 0), (60, 0), (115, -52), (98, -55)];

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Stream {
    pub playing: bool,
    start: u64,
    length: u32,
    position: u64,
    /// The track to go on with at the end of this one.
    next: (u64, u32),
    stop_at_end: bool,
    /// Each channel's last two samples, with 6 more bits.
    history: [[i32; 2]; 2],
    /// The block being played and how far into it.
    block: Vec<(i16, i16)>,
    at: usize,
    /// Bytes read ahead from the disc: where they start, and them (not part of a save state).
    #[serde(skip)]
    read_ahead: Option<(u64, Vec<u8>)>,
}

impl Stream {
    /// DI E1, subcommand 0: play `length` bytes of stream from byte `start` of the disc. A length
    /// of 0 asks to stop at the end of the track; while a track plays, a new one comes next.
    pub fn play(&mut self, start: u64, length: u32) {
        if length == 0 {
            self.stop_at_end = self.playing;
            return;
        }
        if self.playing && self.stop_at_end {
            return;
        }
        self.next = (start, length);
        if !self.playing {
            self.begin(start, length);
            self.playing = true;
        }
    }

    /// DI E1, subcommand 1: stop now.
    pub fn cancel(&mut self) {
        self.playing = false;
        self.stop_at_end = false;
        self.block.clear();
    }

    /// DI E2: 0 whether it plays, 1 the position (32 KB steps, in 4-byte units), 2 the track's
    /// start (4-byte units), 3 its length.
    pub fn status(&self, which: u32) -> u32 {
        match which {
            0 => self.playing as u32,
            1 => ((self.position & !0x7FFF) >> 2) as u32,
            2 => (self.start >> 2) as u32,
            3 => self.length,
            _ => 0,
        }
    }

    /// A track starts (or starts again): its first block predicts from silence.
    fn begin(&mut self, start: u64, length: u32) {
        self.start = start;
        self.length = length;
        self.position = start;
        self.history = [[0; 2]; 2];
        self.block.clear();
        self.at = 0;
    }

    /// The next stereo sample from the drive (silence when nothing plays).
    pub fn sample(&mut self, disc: Option<&mut Box<dyn Disc>>) -> (i16, i16) {
        if !self.playing {
            return (0, 0);
        }
        if self.at >= self.block.len() {
            if self.position + BLOCK as u64 > self.start + self.length as u64 {
                // The end of the track: stop, or go on with the next one.
                if self.stop_at_end {
                    self.cancel();
                    return (0, 0);
                }
                let (start, length) = self.next;
                self.begin(start, length);
            }
            let mut b = [0u8; BLOCK];
            if disc.is_none_or(|d| !self.read(d.as_mut(), &mut b)) {
                self.cancel();
                return (0, 0);
            }
            self.decode(&b);
            self.position += BLOCK as u64;
        }
        let s = self.block[self.at];
        self.at += 1;
        s
    }

    /// One block from the disc, through the read-ahead.
    fn read(&mut self, disc: &mut dyn Disc, out: &mut [u8; BLOCK]) -> bool {
        let at = self.position;
        let fresh = match &self.read_ahead {
            Some((from, bytes)) => at < *from || at + BLOCK as u64 > *from + bytes.len() as u64,
            None => true,
        };
        if fresh {
            let mut bytes = vec![0u8; READ_AHEAD];
            if disc.read(at, &mut bytes).is_err() {
                return false;
            }
            self.read_ahead = Some((at, bytes));
        }
        let (from, bytes) = self.read_ahead.as_ref().unwrap();
        let k = (at - from) as usize;
        out.copy_from_slice(&bytes[k..k + BLOCK]);
        true
    }

    fn decode(&mut self, b: &[u8; BLOCK]) {
        self.block.clear();
        self.at = 0;
        for &d in &b[BLOCK - SAMPLES..] {
            let left = decode_sample(&mut self.history[0], b[0], d & 0xF);
            let right = decode_sample(&mut self.history[1], b[1], d >> 4);
            self.block.push((left, right));
        }
    }
}

/// One sample of one channel: the code, scaled by the header's shift, plus the filter's
/// prediction from the channel's last two samples (which are kept in 64ths).
fn decode_sample(history: &mut [i32; 2], header: u8, code: u8) -> i16 {
    let (k1, k2) = FILTERS.get((header >> 4) as usize).copied().unwrap_or((0, 0));
    let predicted = ((k1 * history[0] + k2 * history[1] + 32) >> 6).clamp(-0x20_0000, 0x1F_FFFF);
    // The 4-bit code, signed, at the top of a 16-bit sample, then shifted down.
    let scaled = (((code as i32) << 28) >> 16) >> (header & 0xF);
    let value = (scaled << 6) + predicted;
    history[1] = history[0];
    history[0] = value;
    (value >> 6).clamp(-32768, 32767) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A disc that is just bytes.
    struct Bytes(Vec<u8>);
    impl Disc for Bytes {
        fn read(&mut self, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
            let start = offset as usize;
            if start >= self.0.len() {
                return Err(std::io::Error::other("past the end"));
            }
            let n = buf.len().min(self.0.len() - start);
            buf[..n].copy_from_slice(&self.0[start..start + n]);
            buf[n..].fill(0);
            Ok(())
        }
        fn size(&self) -> u64 {
            self.0.len() as u64
        }
    }

    /// Our own encoder for the test: per block, the filter and shift that track the wave best,
    /// the nearest code for each sample (both channels the same).
    fn encode(samples: &[i16]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut history = [0i32; 2];
        for chunk in samples.chunks(SAMPLES) {
            let mut best_block = (i64::MAX, Vec::new(), history);
            for header in (0..4u8).flat_map(|f| (0..=12u8).map(move |s| (f << 4) | s)) {
                let (mut h, mut codes, mut total) = (history, Vec::new(), 0i64);
                for &s in chunk {
                    let mut best = (0u8, i64::MAX, h);
                    for code in 0..16u8 {
                        let mut t = h;
                        let err = (decode_sample(&mut t, header, code) as i64 - s as i64).abs();
                        if err < best.1 {
                            best = (code, err, t);
                        }
                    }
                    h = best.2;
                    total += best.1;
                    codes.push(best.0 | (best.0 << 4));
                }
                if total < best_block.0 {
                    let mut block = vec![header; 4];
                    block.extend(codes);
                    best_block = (total, block, h);
                }
            }
            history = best_block.2;
            let mut block = best_block.1;
            block.resize(BLOCK, 0);
            out.extend(block);
        }
        out
    }

    #[test]
    fn decodes_a_sine_close_to_what_was_encoded() {
        let wave: Vec<i16> = (0..SAMPLES * 40).map(|k| ((k as f64 * 0.05).sin() * 12000.0) as i16).collect();
        let mut disc: Box<dyn Disc> = Box::new(Bytes(encode(&wave)));
        let mut s = Stream::default();
        s.play(0, (wave.len() / SAMPLES * BLOCK) as u32);
        for (k, want) in wave.iter().enumerate() {
            let (l, r) = s.sample(Some(&mut disc));
            assert_eq!(l, r, "both channels carry the same here");
            assert!((l as i32 - *want as i32).abs() < 700, "sample {k}: {l} for {want}");
        }
    }

    #[test]
    fn filters_and_shifts_as_specified() {
        let mut h = [0i32; 2];
        // Filter 0, shift 12: code 1 is 1, code 15 (-1) is -1.
        assert_eq!(decode_sample(&mut h, 0x0C, 1), 1);
        assert_eq!(decode_sample(&mut h, 0x0C, 0xF), -1);
        // Filter 0, shift 0: code 7 is 7 << 12; code 8 is -8 << 12.
        assert_eq!(decode_sample(&mut h, 0x00, 7), 7 << 12);
        assert_eq!(decode_sample(&mut h, 0x00, 8), -8 << 12);
        // Filter 1 (60/64 of the last sample): from 4096 with code 0, 3840.
        let mut h = [4096 << 6, 0];
        assert_eq!(decode_sample(&mut h, 0x10, 0), 3840);
        // Filter 2: 115/64 of the last minus 52/64 of the one before (1390.6, rounded down).
        let mut h = [1000 << 6, 500 << 6];
        assert_eq!(decode_sample(&mut h, 0x20, 0), 1390);
        // Loud enough to clip.
        let mut h = [30000 << 6, 0];
        assert_eq!(decode_sample(&mut h, 0x10, 7), 32767);
    }

    #[test]
    fn a_track_loops_until_told_to_stop_at_its_end() {
        // Two blocks of track A (all +1 steps), two of track B (all -1 steps).
        let mut bytes = Vec::new();
        for header_code in [(0x0Cu8, 0x11u8), (0x0C, 0x11), (0x0C, 0xFF), (0x0C, 0xFF)] {
            let mut b = vec![header_code.0; 4];
            b.extend(std::iter::repeat_n(header_code.1, SAMPLES));
            bytes.extend(b);
        }
        let mut disc: Box<dyn Disc> = Box::new(Bytes(bytes));
        let mut s = Stream::default();
        s.play(0, 64);
        let take = |s: &mut Stream, d: &mut Box<dyn Disc>, n: usize| (0..n).map(|_| s.sample(Some(d)).0).collect::<Vec<_>>();
        assert!(take(&mut s, &mut disc, 56).iter().all(|v| *v == 1));
        assert_eq!(s.status(0), 1);
        // The end of track A: it starts again.
        assert!(take(&mut s, &mut disc, 28).iter().all(|v| *v == 1));
        // Track B is queued: it comes after this pass of A.
        s.play(64, 64);
        assert_eq!(s.status(2), 0, "still in A");
        assert!(take(&mut s, &mut disc, 28).iter().all(|v| *v == 1));
        assert!(take(&mut s, &mut disc, 28).iter().all(|v| *v == -1));
        assert_eq!(s.status(2), 16);
        assert_eq!(s.status(3), 64);
        // Stop at the end of the track: the rest of B plays out, then silence.
        s.play(0, 0);
        assert!(take(&mut s, &mut disc, 28).iter().all(|v| *v == -1));
        assert!(take(&mut s, &mut disc, 10).iter().all(|v| *v == 0));
        assert_eq!(s.status(0), 0);
        // Cancel stops at once.
        s.play(0, 64);
        take(&mut s, &mut disc, 3);
        s.cancel();
        assert_eq!(s.sample(Some(&mut disc)), (0, 0));
    }
}
