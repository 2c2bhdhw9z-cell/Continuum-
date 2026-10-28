//! The BupChip: the music player on Rikki & Vikki's Souper cartridge (its BupBoop board). The
//! game writes one-byte commands to $8007, and the board plays the songs from its own files with
//! CoreTone (coretone.rs). The commands, as BupSystem (the game's own emulator) and ProSystem
//! take them:
//! - $00: reset (stop everything, carry on if paused, full volume); $02: carry on; $03: pause;
//!   the other values below $40 do nothing;
//! - $40-$7F: stop the song;
//! - $80-$BF: play song n (the low five bits), or stop if there is no such song;
//! - $C0-$FF: the music's volume, 0-31 in the low five bits (CoreTone's 0-127 as 4 x n, plus 3
//!   when n is odd).
//!
//! A new command takes effect at CoreTone's next step. The board runs on its own clock, so the
//! music keeps its time whatever the console's TV system: 48,000 samples a second, the songs
//! stepping every 200 of them.

use crate::coretone::{CoreTone, Files, BUFFER_LEN, BUFFER_SAMPLES};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Serialize, Deserialize)]
pub struct BupChip {
    tone: CoreTone,
    /// The step being played out, and the next sample in it (0: time for a new step).
    block: Vec<i16>,
    pos: usize,
}

impl BupChip {
    pub fn new(files: Files) -> BupChip {
        BupChip { tone: CoreTone::new(Arc::new(files)), block: vec![0; BUFFER_LEN], pos: 0 }
    }

    /// Switched on afresh: nothing playing.
    pub fn reset(&mut self) {
        self.tone.reset();
        self.block.fill(0);
        self.pos = 0;
    }

    /// Puts the files back after a saved state is loaded (states don't carry them).
    pub fn restore_files(&mut self, from: &BupChip) {
        self.tone.set_files(Arc::clone(from.tone.files()));
    }

    pub fn song_count(&self) -> usize {
        self.tone.files().song_count()
    }

    pub fn music_playing(&self) -> bool {
        self.tone.music_playing()
    }

    /// A write to $8007.
    pub fn command(&mut self, data: u8) {
        match data & 0xC0 {
            0x00 => match data {
                0 => {
                    self.tone.stop_all();
                    self.tone.resume();
                    self.tone.atten_music(127);
                }
                2 => self.tone.resume(),
                3 => self.tone.pause(),
                _ => {}
            },
            0x40 => self.tone.stop_music(),
            0x80 => {
                let song = (data & 0x1F) as usize;
                if song < self.song_count() {
                    self.tone.play_music(song);
                } else {
                    self.tone.stop_music();
                }
            }
            _ => {
                let n = (data & 0x1F) as i32;
                self.tone.atten_music((n * 4 + if n & 1 != 0 { 3 } else { 0 }) as i8);
            }
        }
    }

    /// One of CoreTone's steps: 200 stereo samples, left and right interleaved.
    pub fn step(&mut self, out: &mut [i16; BUFFER_LEN]) {
        self.tone.update(out);
    }

    /// The next of its 48 kHz samples, left and right.
    pub fn next_sample(&mut self) -> (i16, i16) {
        if self.pos == 0 {
            let mut b = [0i16; BUFFER_LEN];
            self.tone.update(&mut b);
            self.block.clear();
            self.block.extend_from_slice(&b);
        }
        let s = (self.block[self.pos * 2], self.block[self.pos * 2 + 1]);
        self.pos = (self.pos + 1) % BUFFER_SAMPLES;
        s
    }
}
