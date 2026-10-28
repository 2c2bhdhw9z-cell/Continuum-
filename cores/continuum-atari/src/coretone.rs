//! CoreTone, the wavetable synthesizer behind the "BupChip": the music player on Rikki & Vikki's
//! Souper cartridge. It plays the game's own sample, instrument and song files: sixteen channels
//! of 8-bit samples mixed into 48 kHz stereo, with every script stepping 240 times a second.
//!
//! This is CoreTone 1.2.2cz by Osman Celimli, translated to Rust. Its notice:
//!
//! ```text
//! Copyright (C) 2015 - 2016 Osman Celimli
//!
//! This software is provided 'as-is', without any express or implied
//! warranty.  In no event will the authors be held liable for any damages
//! arising from the use of this software.
//!
//! Permission is granted to anyone to use this software for any purpose,
//! including commercial applications, and to alter it and redistribute it
//! freely, subject to the following restrictions:
//!
//! 1. The origin of this software must not be misrepresented; you must not
//!    claim that you wrote the original software. If you use this software
//!    in a product, an acknowledgment in the product documentation would be
//!    appreciated but is not required.
//! 2. Altered source versions must be plainly marked as such, and must not be
//!    misrepresented as being the original software.
//! 3. This notice may not be removed or altered from any source distribution.
//! ```
//!
//! ALTERED SOURCE VERSION: this is not the original software. It was translated from C to Rust
//! for Continuum's Atari 7800 core (cAtari) in 2026, and changed as follows.
//! - The packages are byte buffers, and positions in them stand in for the C's pointers. The
//!   whole state can be saved and loaded.
//! - Only what the BupChip uses is here: music, pause and volume. The sound-effect queues, the
//!   render callback, the mutex stubs and the information calls are left out, and so are the
//!   address-alignment checks (the packages are copied in).
//! - Where the C would read outside a package or crash, this does something safe instead:
//!   - a read past the end of a script gives a command that ends the script, and a sample read
//!     past its package is silent;
//!   - a patch command from 8 to 12 (the C looks those up past the end of its command table)
//!     stops the patch, as 13 and up do;
//!   - a note above 127 plays at 0 Hz.
//! - Where the C would never get out of a loop, this stops the track, patch or channel instead.
//!   That covers a script that keeps running commands without ever waiting, and a sample loop
//!   that can't come back round.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Output samples a second, and how many times a second the scripts step.
pub const RENDER_RATE: u32 = 48_000;
pub const DECODE_RATE: u32 = 240;
/// Stereo samples made each step, and the buffer they go in (left and right interleaved).
pub const BUFFER_SAMPLES: usize = (RENDER_RATE / DECODE_RATE) as usize;
pub const BUFFER_LEN: usize = BUFFER_SAMPLES * 2;

const CHANNELS: usize = 16;
const DEFAULT_VOLUME: i8 = 127;
const STACK_DEPTH: u32 = 16;
const SAMPLES_MAX: u32 = 256;
const NOTE_INVALID: u8 = 0x80;
const CALL_TAG: i32 = -128;
/// What a read past the end of a script gives: a command that ends it.
const PAST_END: u8 = 0x7F;
/// Commands a script may run in one step before it counts as stuck (where the C would hang).
const STUCK: u32 = 1 << 20;

/// The files CoreTone plays from, which never change once loaded: the sample package ("CSMP"),
/// the instrument package ("CINS") and the songs ("CMUS"), with the tables worked out from them.
#[derive(Default)]
pub struct Files {
    samples: Vec<u8>,
    instruments: Vec<u8>,
    songs: Vec<Vec<u8>>,
    sample_count: u32,
    /// Each sample's frequency ratio as 32.32: its sample rate over 48 kHz times the pitch it
    /// was recorded at, doubled.
    ratio: Vec<u64>,
    /// The 128 MIDI notes' frequencies (A440 tuning), 16.16 Hz.
    notes: Vec<i32>,
}

/// A little-endian 32-bit word at `at`, or 0 past the end.
fn word(data: &[u8], at: usize) -> u32 {
    at.checked_add(4)
        .and_then(|end| data.get(at..end))
        .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

impl Files {
    /// Checks the two packages and works out their tables. A song that isn't one just doesn't
    /// play when it is asked for.
    pub fn new(samples: Vec<u8>, instruments: Vec<u8>, songs: Vec<Vec<u8>>) -> Result<Files, String> {
        if samples.get(0..4) != Some(b"CSMP") {
            return Err("not a CoreTone sample package".into());
        }
        let sample_count = word(&samples, 4);
        if sample_count > SAMPLES_MAX {
            return Err("too many samples".into());
        }
        if instruments.get(0..4) != Some(b"CINS") {
            return Err("not a CoreTone instrument package".into());
        }
        // Fr = Sf / (Rf * Bf), doubled, from each sample's 16.16 rate and recorded pitch.
        let fixed = |v: u32| (v & 0xFFFF) as f64 / 65536.0 + (v >> 16) as f64;
        let ratio = (0..sample_count as usize)
            .map(|x| {
                let e = 8 + 16 * x;
                let (sf, bf) = (fixed(word(&samples, e + 8)), fixed(word(&samples, e + 12)));
                let fr = (sf / (RENDER_RATE as f64 * bf)) * 2.0;
                let int = fr.floor();
                let frac = (fr - int) * 4_294_967_296.0;
                ((int as i32 as u32 as u64) << 32) | frac as u32 as u64
            })
            .collect();
        // F(n) = 2^((n - 69) / 12) * 440 Hz.
        let notes = (0..128)
            .map(|n| {
                let f = 2f64.powf((n as f64 - 69.0) / 12.0) * 440.0;
                let int = f.floor();
                let frac = (f - int) * 65536.0;
                (((int as i16 as u16 as u32) << 16) | frac as u16 as u32) as i32
            })
            .collect();
        Ok(Files { samples, instruments, songs, sample_count, ratio, notes })
    }

    pub fn song_count(&self) -> usize {
        self.songs.len()
    }

    /// Where sample `s` starts in its package and how long it is; a sample that isn't in the
    /// package is the C's silent stand-in, no length.
    fn sample(&self, s: u32) -> (Option<u32>, u32) {
        if s < self.sample_count {
            let e = 8 + 16 * s as usize;
            (Some(word(&self.samples, e)), word(&self.samples, e + 4))
        } else {
            (None, 0)
        }
    }

    /// The 16.16 step through sample `s` that plays it at `freq` (16.16 Hz).
    fn phase(&self, s: u32, freq: i32) -> i32 {
        match self.ratio.get(s as usize) {
            Some(&fr) if s < self.sample_count => ((freq as i64 as u64).wrapping_mul(fr) >> 32) as u32 as i32,
            _ => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum Mode {
    #[default]
    Off,
    Single,
    Loop,
}

/// What a channel plays: a sample, at a volume and a pitch.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Channel {
    mode: Mode,
    /// Where its sample starts in the sample package (None: the silent stand-in).
    sample: Option<u32>,
    sample_len: u16,
    vol_main: i8,
    vol_left: i8,
    vol_right: i8,
    /// The 16.16 place in the sample, and how far it moves each output sample.
    phase_acc: i32,
    phase_adj: i32,
    loop_start: i16,
    loop_end: i16,
}

/// A channel's script: the instrument's, for a note.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Patch {
    instrument: bool,
    priority: i32,
    /// Where its script starts in the instrument package, how far in it is, where the part for
    /// letting go of the note starts, and how many steps it waits.
    script: u32,
    offset: u32,
    note_off: u32,
    del: u32,
    freq_base: i32,
    freq_pitch: i32,
    pitch_adj: i32,
    freq_offset: i32,
    offset_adj: i32,
    vol_cur: i16,
    vol_adj: i16,
    stack_pos: u32,
    loop_stack: [i32; 16],
    addr_stack: [u32; 16],
}

/// One of a song's tracks: a script that plays notes on its channel.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Track {
    priority: i32,
    recalc_vol: bool,
    inst_sel: u32,
    note: u8,
    /// The song its script is in, where the script starts, how far in it is, and how many steps
    /// it waits.
    song: usize,
    script: u32,
    offset: u32,
    del: u32,
    vol_main: i8,
    pan_left: i8,
    pan_right: i8,
    vol_left: i8,
    vol_right: i8,
    stack_pos: u32,
    loop_stack: [i32; 16],
    addr_stack: [u32; 16],
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CoreTone {
    #[serde(skip)]
    files: Arc<Files>,
    paused: bool,
    all_stop_req: bool,
    mus_track: usize,
    mus_vol: i8,
    mus_playing: bool,
    mus_mood: i32,
    mus_play_req: bool,
    mus_stop_req: bool,
    mus_atten_req: bool,
    channels: [Channel; CHANNELS],
    patches: [Patch; CHANNELS],
    tracks: [Track; CHANNELS],
}

/// Sample `i` of the sample starting at `base` (silent outside the package).
fn sample_at(samples: &[u8], base: Option<u32>, i: u16) -> i32 {
    base.and_then(|b| samples.get(b as usize + i as usize)).map_or(0, |&v| v as i8 as i32)
}

/// The high byte of a 16-bit value, signed: how CoreTone scales one volume by another.
fn hi(v: i32) -> i32 {
    ((v as i16) >> 8) as i32
}

impl CoreTone {
    /// CoreTone set up with `files` (ct_init), nothing playing.
    pub fn new(files: Arc<Files>) -> CoreTone {
        let mut t = CoreTone {
            files,
            paused: false,
            all_stop_req: false,
            mus_track: 0,
            mus_vol: DEFAULT_VOLUME,
            mus_playing: false,
            mus_mood: 0,
            mus_play_req: false,
            mus_stop_req: false,
            mus_atten_req: true,
            channels: Default::default(),
            patches: Default::default(),
            tracks: Default::default(),
        };
        t.reset();
        t
    }

    /// Back to how it starts, keeping its files.
    pub fn reset(&mut self) {
        *self = CoreTone {
            files: Arc::clone(&self.files),
            paused: false,
            all_stop_req: false,
            mus_track: 0,
            mus_vol: DEFAULT_VOLUME,
            mus_playing: false,
            mus_mood: 0,
            mus_play_req: false,
            mus_stop_req: false,
            mus_atten_req: true,
            channels: Default::default(),
            patches: Default::default(),
            tracks: Default::default(),
        };
        for t in self.tracks.iter_mut() {
            t.note = NOTE_INVALID;
        }
    }

    pub fn files(&self) -> &Arc<Files> {
        &self.files
    }

    /// Puts the files back after a saved state is loaded (states don't carry them).
    pub fn set_files(&mut self, files: Arc<Files>) {
        self.files = files;
    }

    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// Stops everything at the next step.
    pub fn stop_all(&mut self) {
        self.all_stop_req = true;
    }

    /// Starts song `song` at the next step, stopping the one playing.
    pub fn play_music(&mut self, song: usize) {
        self.stop_music();
        self.mus_track = song;
        self.mus_play_req = true;
    }

    pub fn stop_music(&mut self) {
        self.mus_stop_req = true;
    }

    /// The music's volume from the next step: 0 silent, 127 loudest.
    pub fn atten_music(&mut self, vol: i8) {
        self.mus_vol = vol;
        self.mus_atten_req = true;
    }

    pub fn music_playing(&self) -> bool {
        self.mus_playing
    }

    /// The playing song's mood flag (0 when nothing plays).
    pub fn mood(&self) -> i32 {
        self.mus_mood
    }

    /// One step (ct_update): the requests, the songs' and instruments' scripts, then 200 stereo
    /// samples into `out`.
    pub fn update(&mut self, out: &mut [i16; BUFFER_LEN]) {
        // Stop requests first, to free the channels before anything new starts.
        if self.mus_stop_req || self.all_stop_req {
            for x in 0..CHANNELS {
                self.tracks[x].priority = 0;
                self.tracks[x].note = NOTE_INVALID;
                if self.patches[x].instrument || self.all_stop_req {
                    self.patches[x].instrument = false;
                    self.patches[x].priority = 0;
                    self.channels[x].mode = Mode::Off;
                }
            }
            self.mus_playing = false;
            self.mus_mood = 0;
        }
        self.all_stop_req = false;
        self.mus_stop_req = false;

        if self.mus_play_req {
            self.mus_playing = self.music_setup(self.mus_track);
            self.mus_mood = 0;
        }
        self.mus_play_req = false;

        if self.mus_atten_req {
            for t in self.tracks.iter_mut() {
                t.recalc_vol = true;
                t.vol_main = self.mus_vol;
            }
            self.mus_atten_req = false;
        }

        if self.mus_playing && !self.paused {
            let mut active = 0;
            for x in 0..CHANNELS {
                self.music_decode(x);
                if self.tracks[x].priority != 0 {
                    active += 1;
                }
            }
            if active == 0 {
                self.mus_playing = false;
                self.mus_mood = 0;
            }
        }

        // The first channel playing writes the buffer; the rest add to it.
        let mut stamp = true;
        if !self.paused {
            for x in 0..CHANNELS {
                if self.patches[x].priority != 0 {
                    self.patch_decode(x);
                    self.patch_recalc(x);
                }
                if self.channels[x].mode != Mode::Off {
                    self.render(x, out, stamp);
                    stamp = false;
                }
            }
        }
        if stamp {
            out.fill(0);
        }
    }

    // ---- channels --------------------------------------------------------------------------

    fn render(&mut self, x: usize, out: &mut [i16; BUFFER_LEN], stamp: bool) {
        let samples = &self.files.samples;
        let ch = &mut self.channels[x];
        let sl = hi(ch.vol_main as i32 * ch.vol_left as i32);
        let sr = hi(ch.vol_main as i32 * ch.vol_right as i32);
        let mut put = |k: usize, s: i32| {
            let (l, r) = ((s * sl) as i16, (s * sr) as i16);
            if stamp {
                out[k] = l;
                out[k + 1] = r;
            } else {
                out[k] = out[k].wrapping_add(l);
                out[k + 1] = out[k + 1].wrapping_add(r);
            }
        };
        let mut k = 0;
        match ch.mode {
            Mode::Single => {
                // Through the sample to its end, then the channel is off.
                while k < BUFFER_LEN {
                    put(k, sample_at(samples, ch.sample, (ch.phase_acc as u32 >> 16) as u16));
                    k += 2;
                    ch.phase_acc = ch.phase_acc.wrapping_add(ch.phase_adj);
                    if (ch.phase_acc as u32 >> 16) as u16 >= ch.sample_len {
                        ch.mode = Mode::Off;
                        ch.phase_adj = 0;
                        if stamp {
                            out[k..].fill(0);
                        }
                        break;
                    }
                }
            }
            Mode::Loop => {
                // Past the loop's end (or its start, going backwards), back into the loop.
                let len = (ch.loop_end as i32 - ch.loop_start as i32) as u32;
                let back = ch.phase_adj < 0;
                while k < BUFFER_LEN {
                    put(k, sample_at(samples, ch.sample, (ch.phase_acc as u32 >> 16) as u16));
                    k += 2;
                    ch.phase_acc = ch.phase_acc.wrapping_add(ch.phase_adj);
                    let mut h = (ch.phase_acc as u32 >> 16) as u16;
                    let mut turns = 0u32;
                    if back {
                        while (h as i32) < ch.loop_start as i32 && turns <= 0x10000 {
                            h = (h as u32).wrapping_add(len) as u16;
                            turns += 1;
                        }
                    } else {
                        while (h as i32) >= ch.loop_end as i32 && turns <= 0x10000 {
                            h = (h as u32).wrapping_sub(len) as u16;
                            turns += 1;
                        }
                    }
                    ch.phase_acc = ((ch.phase_acc as u32 & 0xFFFF) | ((h as u32) << 16)) as i32;
                    if turns > 0x10000 {
                        // A loop that can't come back round (the C hangs here).
                        ch.mode = Mode::Off;
                        ch.phase_adj = 0;
                        if stamp {
                            out[k..].fill(0);
                        }
                        break;
                    }
                }
            }
            Mode::Off => {}
        }
    }

    // ---- patches ---------------------------------------------------------------------------

    fn patch_byte(&self, x: usize) -> u8 {
        let p = &self.patches[x];
        self.files.instruments.get(p.script as usize + p.offset as usize).copied().unwrap_or(PAST_END)
    }

    fn patch_next(&mut self, x: usize) -> u8 {
        let b = self.patch_byte(x);
        self.patches[x].offset = self.patches[x].offset.wrapping_add(1);
        b
    }

    fn patch_u16(&mut self, x: usize) -> u16 {
        u16::from_le_bytes([self.patch_next(x), self.patch_next(x)])
    }

    fn patch_i32(&mut self, x: usize) -> i32 {
        i32::from_le_bytes([self.patch_next(x), self.patch_next(x), self.patch_next(x), self.patch_next(x)])
    }

    /// The channel's next pitch and volume from its patch, once a step.
    fn patch_recalc(&mut self, x: usize) {
        let p = &mut self.patches[x];
        p.freq_pitch = p.freq_pitch.wrapping_add(p.pitch_adj);
        p.freq_offset = p.freq_offset.wrapping_add(p.offset_adj);
        self.channels[x].phase_adj = if p.instrument {
            p.freq_base.wrapping_add(p.freq_pitch).wrapping_add(p.freq_offset)
        } else {
            p.freq_offset
        };
        p.vol_cur = p.vol_cur.wrapping_add(p.vol_adj);
        self.channels[x].vol_main = (p.vol_cur >> 8) as i8;
    }

    fn key_on(&mut self, x: usize) {
        let ch = &mut self.channels[x];
        ch.mode = Mode::Off;
        ch.phase_acc = 0;
        let p = &mut self.patches[x];
        p.freq_offset = 0;
        p.offset_adj = 0;
        p.vol_cur = 0;
        p.vol_adj = 0;
        p.offset = 0;
        p.stack_pos = 0;
        p.del = 0;
    }

    fn key_off(&mut self, x: usize) {
        let p = &mut self.patches[x];
        if p.instrument {
            p.offset = p.note_off;
            p.stack_pos = 0;
            p.del = 0;
        } else {
            self.channels[x].mode = Mode::Off;
            p.priority = 0;
        }
    }

    /// Runs the patch's script until it waits or ends.
    fn patch_decode(&mut self, x: usize) {
        let mut budget = STUCK;
        while self.patches[x].priority != 0 && self.patches[x].del == 0 {
            budget -= 1;
            if budget == 0 {
                self.channels[x].mode = Mode::Off;
                self.patches[x].priority = 0;
                break;
            }
            let mut b = self.patch_byte(x);
            if b & 0x80 != 0 {
                // A wait: up to four bytes of seven bits each, the low bits first.
                let (mut shift, mut n) = (0, 0);
                while b & 0x80 != 0 && n < 4 {
                    self.patches[x].del |= ((b & 0x7F) as u32) << shift;
                    self.patches[x].offset = self.patches[x].offset.wrapping_add(1);
                    b = self.patch_byte(x);
                    shift += 7;
                    n += 1;
                }
                continue;
            }
            self.patches[x].offset = self.patches[x].offset.wrapping_add(1);
            match b {
                0 => {
                    // End.
                    self.channels[x].mode = Mode::Off;
                    self.patches[x].instrument = false;
                    self.patches[x].priority = 0;
                }
                1 => self.channels[x].mode = Mode::Single,
                2 => {
                    let (s, e) = (self.patch_u16(x), self.patch_u16(x));
                    let ch = &mut self.channels[x];
                    ch.mode = Mode::Loop;
                    ch.loop_start = s as i16;
                    ch.loop_end = e as i16;
                }
                3 => {
                    let v = self.patch_next(x);
                    let a = self.patch_u16(x);
                    self.patches[x].vol_cur = ((v as u16) << 8) as i16;
                    self.patches[x].vol_adj = a as i16;
                }
                4 => {
                    let (o, a) = (self.patch_i32(x), self.patch_i32(x));
                    self.patches[x].freq_offset = o;
                    self.patches[x].offset_adj = a;
                }
                5 => {
                    // Loop start (a full stack leaves the count byte to be read as a command).
                    if self.patches[x].stack_pos < STACK_DEPTH {
                        let count = self.patch_next(x) as i8 as i32;
                        let p = &mut self.patches[x];
                        p.loop_stack[p.stack_pos as usize] = count;
                        p.addr_stack[p.stack_pos as usize] = p.offset;
                        p.stack_pos += 1;
                    }
                }
                6 => {
                    // Loop end: counts of 0 and 1 go on, 2 and up go back and count down, below
                    // 0 go back for ever.
                    let p = &mut self.patches[x];
                    if p.stack_pos > 0 {
                        let u = (p.stack_pos - 1) as usize;
                        let c = p.loop_stack[u];
                        if (0..2).contains(&c) {
                            p.stack_pos = u as u32;
                        } else if c < 0 {
                            p.offset = p.addr_stack[u];
                        } else {
                            p.offset = p.addr_stack[u];
                            p.loop_stack[u] -= 1;
                        }
                    }
                }
                7 => {}
                _ => {
                    self.channels[x].mode = Mode::Off;
                    self.patches[x].priority = 0;
                }
            }
        }
        if self.patches[x].priority != 0 {
            self.patches[x].del = self.patches[x].del.wrapping_sub(1);
        }
    }

    // ---- music -----------------------------------------------------------------------------

    fn track_byte(&self, x: usize) -> u8 {
        let t = &self.tracks[x];
        self.files
            .songs
            .get(t.song)
            .and_then(|s| s.get(t.script as usize + t.offset as usize))
            .copied()
            .unwrap_or(PAST_END)
    }

    fn track_next(&mut self, x: usize) -> u8 {
        let b = self.track_byte(x);
        self.tracks[x].offset = self.tracks[x].offset.wrapping_add(1);
        b
    }

    fn track_i32(&mut self, x: usize) -> i32 {
        i32::from_le_bytes([self.track_next(x), self.track_next(x), self.track_next(x), self.track_next(x)])
    }

    fn instrument_word(&self, i: usize) -> u32 {
        word(&self.files.instruments, 8 + 4 * i)
    }

    /// The track's left and right volumes from its volume and panning, passed to its channel
    /// when it plays an instrument.
    fn recalc_vol(&mut self, x: usize) {
        let t = &mut self.tracks[x];
        let vol = t.vol_main as i32;
        let side = move |pan: i8| ((((vol * pan as i32) as i16 as i32) << 1) as i16 >> 8) as i8;
        t.vol_left = side(t.pan_left);
        t.vol_right = side(t.pan_right);
        t.recalc_vol = false;
        if self.patches[x].instrument {
            self.channels[x].vol_left = t.vol_left;
            self.channels[x].vol_right = t.vol_right;
        }
    }

    /// The end of a track, or a priority of 0: it lets go of its channel.
    fn release(&mut self, x: usize) {
        if self.patches[x].instrument {
            self.channels[x].mode = Mode::Off;
            self.patches[x].priority = 0;
            self.patches[x].instrument = false;
        }
    }

    fn note_on(&mut self, x: usize) {
        // Only on a channel it already plays an instrument on, or one of lower priority.
        if !(self.patches[x].priority < self.tracks[x].priority || self.patches[x].instrument) {
            return;
        }
        let note = self.track_next(x);
        self.tracks[x].note = note;
        self.patches[x].priority = self.tracks[x].priority;
        self.patches[x].instrument = true;
        let at = self.tracks[x].inst_sel as usize * 3;
        let sample = self.instrument_word(at);
        self.patches[x].script = self.instrument_word(at + 1);
        self.patches[x].note_off = self.instrument_word(at + 2);
        let (base, len) = self.files.sample(sample);
        self.channels[x].sample = base;
        let freq = self.files.notes.get(note as usize).copied().unwrap_or(0);
        self.patches[x].freq_base = self.files.phase(sample, freq);
        self.channels[x].sample_len = len as u16;
        self.key_on(x);
        self.channels[x].vol_left = self.tracks[x].vol_left;
        self.channels[x].vol_right = self.tracks[x].vol_right;
    }

    /// Runs the track's script until it waits or ends.
    fn music_decode(&mut self, x: usize) {
        if self.tracks[x].recalc_vol {
            self.recalc_vol(x);
        }
        let mut budget = STUCK;
        while self.tracks[x].priority != 0 && self.tracks[x].del == 0 {
            budget -= 1;
            if budget == 0 {
                self.tracks[x].priority = 0;
                self.release(x);
                break;
            }
            let mut b = self.track_byte(x);
            if b & 0x80 != 0 {
                let (mut shift, mut n) = (0, 0);
                while b & 0x80 != 0 && n < 4 {
                    self.tracks[x].del |= ((b & 0x7F) as u32) << shift;
                    self.tracks[x].offset = self.tracks[x].offset.wrapping_add(1);
                    b = self.track_byte(x);
                    shift += 7;
                    n += 1;
                }
                continue;
            }
            self.tracks[x].offset = self.tracks[x].offset.wrapping_add(1);
            match b {
                0 => {
                    // Priority (0 to 255).
                    let p = self.track_next(x) as i32;
                    self.tracks[x].priority = p;
                    if p == 0 {
                        self.tracks[x].note = NOTE_INVALID;
                        self.release(x);
                    }
                }
                1 => {
                    self.tracks[x].pan_left = self.track_next(x) as i8;
                    self.tracks[x].pan_right = self.track_next(x) as i8;
                    self.recalc_vol(x);
                }
                2 => self.tracks[x].inst_sel = self.track_next(x) as u32,
                3 => self.note_on(x),
                4 => {
                    if self.patches[x].instrument {
                        self.tracks[x].note = NOTE_INVALID;
                        self.key_off(x);
                    }
                }
                5 => {
                    let (p, a) = (self.track_i32(x), self.track_i32(x));
                    self.patches[x].freq_pitch = p;
                    self.patches[x].pitch_adj = a;
                }
                6 => {
                    if self.tracks[x].stack_pos < STACK_DEPTH {
                        let count = self.track_next(x) as i8 as i32;
                        let t = &mut self.tracks[x];
                        t.loop_stack[t.stack_pos as usize] = count;
                        t.addr_stack[t.stack_pos as usize] = t.offset;
                        t.stack_pos += 1;
                    }
                }
                7 => {
                    let t = &mut self.tracks[x];
                    if t.stack_pos > 0 {
                        let u = (t.stack_pos - 1) as usize;
                        let c = t.loop_stack[u];
                        if (0..2).contains(&c) {
                            t.stack_pos = u as u32;
                        } else if c < 0 {
                            t.offset = t.addr_stack[u];
                        } else {
                            t.offset = t.addr_stack[u];
                            t.loop_stack[u] -= 1;
                        }
                    }
                }
                8 => {
                    // Call: a 32-bit offset from just after it.
                    if self.tracks[x].stack_pos < STACK_DEPTH {
                        let t = &mut self.tracks[x];
                        t.loop_stack[t.stack_pos as usize] = CALL_TAG;
                        t.addr_stack[t.stack_pos as usize] = t.offset.wrapping_add(4);
                        let rel = self.track_i32(x);
                        let t = &mut self.tracks[x];
                        t.offset = t.offset.wrapping_add(rel as u32);
                        t.stack_pos += 1;
                    }
                }
                9 => {
                    let t = &mut self.tracks[x];
                    if t.stack_pos > 0 {
                        let u = (t.stack_pos - 1) as usize;
                        if t.loop_stack[u] == CALL_TAG {
                            t.offset = t.addr_stack[u];
                            t.stack_pos = u as u32;
                        }
                    }
                }
                10 => {
                    // Break: every track returns from its outermost call at once.
                    for t in self.tracks.iter_mut() {
                        let mut y = 0;
                        while y < t.stack_pos {
                            if t.loop_stack[y as usize] == CALL_TAG {
                                t.offset = t.addr_stack[y as usize];
                                t.stack_pos = y;
                                t.del = 0;
                            }
                            y += 1;
                        }
                    }
                }
                11 => {}
                12 => self.mus_mood = self.track_i32(x),
                _ => {
                    self.tracks[x].priority = 0;
                    self.release(x);
                }
            }
        }
        if self.tracks[x].priority != 0 {
            self.tracks[x].del = self.tracks[x].del.wrapping_sub(1);
        }
    }

    /// Sets the tracks up for song `song`; false if it isn't one.
    fn music_setup(&mut self, song: usize) -> bool {
        let files = Arc::clone(&self.files);
        let Some(data) = files.songs.get(song).filter(|d| d.get(0..4) == Some(b"CMUS")) else {
            return false;
        };
        let count = word(data, 4).min(CHANNELS as u32) as usize;
        for x in 0..count {
            let e = 8 + 5 * x;
            let pri = data.get(e).map_or(0, |&b| b as i8 as i32);
            let off = word(data, e + 1);
            let t = &mut self.tracks[x];
            t.priority = pri;
            t.recalc_vol = true;
            t.inst_sel = 0;
            t.note = NOTE_INVALID;
            t.song = song;
            t.script = off;
            t.offset = 0;
            t.del = 0;
            t.pan_left = DEFAULT_VOLUME;
            t.pan_right = DEFAULT_VOLUME;
            t.stack_pos = 0;
            self.patches[x].freq_pitch = 0;
            self.patches[x].pitch_adj = 0;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sample package of one looped square wave and an instrument that plays it at full
    /// volume, with the given patch script.
    fn files(script: &[u8], songs: Vec<Vec<u8>>) -> Arc<Files> {
        let mut smp = b"CSMP".to_vec();
        smp.extend(1u32.to_le_bytes());
        smp.extend(24u32.to_le_bytes());
        smp.extend(8u32.to_le_bytes());
        smp.extend((8000u32 << 16).to_le_bytes());
        smp.extend((1000u32 << 16).to_le_bytes());
        smp.extend([100, 100, 100, 100, 156, 156, 156, 156]);
        let mut ins = b"CINS".to_vec();
        ins.extend(1u32.to_le_bytes());
        ins.extend([0u32, 20, 0].iter().flat_map(|v| v.to_le_bytes()));
        ins.extend(script);
        Arc::new(Files::new(smp, ins, songs).unwrap())
    }

    /// One track: note 69 (A440) on instrument 0, then a long wait.
    fn song() -> Vec<u8> {
        let mut s = b"CMUS".to_vec();
        s.extend(1u32.to_le_bytes());
        s.push(10);
        s.extend(13u32.to_le_bytes());
        s.extend([3, 69, 0xFF, 0x7F]);
        s
    }

    fn steps(t: &mut CoreTone, n: usize) -> Vec<i16> {
        let mut out = Vec::new();
        for _ in 0..n {
            let mut b = [0i16; BUFFER_LEN];
            t.update(&mut b);
            out.extend(b);
        }
        out
    }

    #[test]
    fn a_note_plays_its_sample_at_its_pitch() {
        // Loop the whole sample at full volume and wait.
        let f = files(&[3, 127, 0, 0, 2, 0, 0, 8, 0, 0xFF, 0x7F], vec![song()]);
        let mut t = CoreTone::new(f);
        t.play_music(0);
        let out = steps(&mut t, 2);
        // A440 through a sample of a 1000 Hz tone at 8 kHz: CoreTone's doubled ratio makes that
        // 2 x 0.44 x 8000 / 48000 of a sample each output sample, 9611 as 16.16.
        assert_eq!(t.files.notes[69], 440 << 16);
        assert_eq!(t.patches[0].freq_base, 9611);
        let left: Vec<i16> = out.iter().step_by(2).copied().collect();
        // Volume 127 x panning 126 scales by 62: the wave's 100 and -100 come out as 6200.
        assert!(left.iter().all(|&v| v == 6200 || v == -6200), "{:?}", &left[..20]);
        assert!(left.contains(&-6200));
    }

    #[test]
    fn bad_commands_and_endless_loops_stop_instead_of_crashing_or_hanging() {
        for script in [
            &[3, 127, 0, 0, 2, 0, 0, 8, 0, 9][..],        // command 9, past the C's table
            &[3, 127, 0, 0, 2, 0, 0, 8, 0, 5, 0xFF, 6][..], // an endless loop with no wait
            &[3, 127, 0, 0, 2, 4, 0, 4, 0, 0xFF, 0x7F][..], // a loop with no length
            &[3, 127, 0, 0, 2, 0, 0, 0, 0, 0xFF, 0x7F][..], // a loop ending at 0
            &[3, 127, 0, 0, 1][..],                       // a script running off its end
        ] {
            let mut t = CoreTone::new(files(script, vec![song()]));
            t.play_music(0);
            steps(&mut t, 4);
            assert_eq!(t.channels[0].mode, Mode::Off, "{script:?}");
        }
        // A song that isn't one plays nothing; a note above 127 stands still on its first sample.
        let mut t = CoreTone::new(files(&[0], vec![b"XMUS".to_vec()]));
        t.play_music(0);
        assert!(steps(&mut t, 2).iter().all(|&v| v == 0) && !t.music_playing());
        let mut high = song();
        high[14] = 200;
        let mut t = CoreTone::new(files(&[3, 127, 0, 0, 2, 0, 0, 8, 0, 0xFF, 0x7F], vec![high]));
        t.play_music(0);
        assert!(steps(&mut t, 2).iter().all(|&v| v == 6200));
    }

    #[test]
    fn a_pause_silences_it_and_a_saved_state_carries_on() {
        let f = files(&[3, 127, 0, 0, 2, 0, 0, 8, 0, 0xFF, 0x7F], vec![song()]);
        let mut t = CoreTone::new(Arc::clone(&f));
        t.play_music(0);
        steps(&mut t, 3);
        t.pause();
        assert!(steps(&mut t, 1).iter().all(|&v| v == 0));
        t.resume();
        let saved = bincode::serialize(&t).unwrap();
        let mut back: CoreTone = bincode::deserialize(&saved).unwrap();
        back.set_files(f);
        assert_eq!(steps(&mut t, 5), steps(&mut back, 5));
    }
}
