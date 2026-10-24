//! Our stand-in for Nintendo's own sound program, the one its own studios used (the community
//! calls it the "Zelda" or DAC microcode): Luigi's Mansion, Animal Crossing, Pikmin, Super Mario
//! Sunshine, The Wind Waker, Mario Kart: Double Dash, Twilight Princess and others.
//!
//! It mixes voices in frames of 80 samples (2.5 ms at 32 kHz) into front, back and reverb buses,
//! and writes the front left and right frames to main memory for the CPU to play. Voices come
//! from audio RAM (AFC, Nintendo's 16-samples-per-block ADPCM in its 9-byte and 5-byte kinds, and
//! 8/16-bit PCM), from main memory (16-bit PCM), or from built-in waveforms (square, saw and four
//! patterns the game uploads). Each is resampled (a 4-tap filter from a table the game uploads, or
//! nearest-sample above a 4:1 ratio), optionally low-pass and biquad filtered, and mixed with
//! per-channel volume ramps, or by position ("Dolby") in the later versions. Reverb runs through
//! circular buffers in main memory with 8-tap filters.
//!
//! The versions differ: the early "light" ones (Luigi's Mansion, Animal Crossing, Pikmin 1) use a
//! simpler mail protocol, synchronised once per frame; the later ones use command mails counted
//! in advance, and synchronise every sixteen voices or every frame. Versions are told apart by
//! the Ector checksum of their code.
//!
//! Written from the program's documented behaviour; no other emulator's code.

use crate::hw::{Hw, ARAM_SIZE};
use serde::{Deserialize, Serialize};

/// Samples per bus per frame.
const N: usize = 0x50;

/// How one version of the program behaves.
#[derive(Clone, Copy, Default, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Variant {
    /// The early mail protocol (no Dolby mixing, one sync per frame).
    pub light: bool,
    /// Only four mixing destinations per voice (else six).
    pub four_dests: bool,
    /// 0x80-word voice blocks (the NTSC boot menu's), missing their 0x40-0x80 part.
    pub tiny_vpb: bool,
    /// Channel volumes give the step, not the target.
    pub explicit_step: bool,
    /// Synchronised once a frame with two mails of voice flags (else every 16 voices).
    pub sync_per_frame: bool,
    /// Command 0D does nothing.
    pub no_cmd_0d: bool,
    /// Command 0C is the GBA link's key exchange.
    pub gba_crypto: bool,
    /// Command 0C takes two words (purpose unknown).
    pub weird_0c: bool,
    /// The render command carries command 0D's two words.
    pub combined_0d: bool,
    /// Wii: no audio RAM; "audio RAM" is main memory from a base address.
    pub no_aram: bool,
    /// Positional volumes doubled.
    pub louder_dolby: bool,
}

/// The version for a program's Ector checksum, if it is one of these.
pub fn variant(ector: u32) -> Option<Variant> {
    let v = Variant::default();
    Some(match ector {
        // The GameCube's own menu, NTSC and PAL.
        0x24B2_2038 => Variant { light: true, four_dests: true, tiny_vpb: true, explicit_step: true, no_cmd_0d: true, weird_0c: true, ..v },
        0x6BA3_B3EA => Variant { light: true, four_dests: true, no_cmd_0d: true, ..v },
        // Pikmin 1 NTSC demo; Pikmin 1 NTSC and Animal Crossing.
        0xDF05_9F68 | 0x4BE6_A5CB => Variant { light: true, no_cmd_0d: true, gba_crypto: true, ..v },
        // Luigi's Mansion.
        0x42F6_4AC4 => Variant { light: true, no_cmd_0d: true, weird_0c: true, ..v },
        // Pikmin 1 PAL; Super Mario Sunshine.
        0x267F_D05A | 0x56D3_6052 => Variant { sync_per_frame: true, no_cmd_0d: true, ..v },
        // The Wind Waker.
        0x8684_0740 => v,
        // Four Swords Adventures, Mario Kart: Double Dash, Pikmin 2 and others.
        0x2FCD_F1EC => Variant { louder_dolby: true, ..v },
        // Twilight Princess (GameCube), Donkey Kong Jungle Beat.
        0x6CA3_3A6D => Variant { louder_dolby: true, combined_0d: true, ..v },
        // Wii: Twilight Princess, Super Mario Galaxy 1 and 2, the New Play Control Pikmins.
        0x6C3F_6F94 | 0xD643_001F | 0xB7EB_9A9C | 0xEAEB_38CC => Variant { no_aram: true, louder_dolby: true, combined_0d: true, ..v },
        _ => return None,
    })
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
enum Mail {
    #[default]
    Waiting,
    Rendering,
    Writing,
    Halted,
}

// Mixing buses, by index. The program names them by their address in its own memory.
const FL: usize = 0;
const FR: usize = 1;
const BL: usize = 2;
const BR: usize = 3;
const FL_REV: usize = 4;
const FR_REV: usize = 5;
const BL_REV: usize = 6;
const BR_REV: usize = 7;
const U0_REV: usize = 8;
const U1_REV: usize = 9;
const U0: usize = 10;
const U1: usize = 11;
const U2: usize = 12;
const BUSES: usize = 13;

fn bus_for(id: u16) -> Option<usize> {
    Some(match id {
        0x0D00 => FL,
        0x0D60 => FR,
        0x0F40 => BL,
        0x0CA0 => BR,
        0x0E80 => FL_REV,
        0x0EE0 => FR_REV,
        0x0C00 => BL_REV,
        0x0C50 => BR_REV,
        0x0DC0 => U0_REV,
        0x0E20 => U1_REV,
        0x09A0 => U0,
        0x0FA0 => U1,
        0x0B00 => U2,
        _ => return None,
    })
}

/// What the mail handler wants done once its state is put back.
enum After {
    Nothing,
    Switch,
    Reset,
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct ZeldaState {
    pub variant: Variant,
    mail: Mail,
    expected: u32,
    sync_max_voice: u32,
    skip_flags: Vec<u16>,
    second_half: bool,
    cmd: Vec<u32>,
    read: usize,
    write: usize,
    pending: u32,
    can_execute: bool,
    frames: u32,
    voices_per_frame: u16,
    frame: u32,
    voice: u32,
    // The renderer.
    prepared: bool,
    out_l: u32,
    out_r: u32,
    out_volume: u16,
    buses: Vec<Vec<i16>>,
    resampling: Vec<i16>,
    patterns: Vec<i16>,
    sine: Vec<i16>,
    afc: Vec<i16>,
    aram_base: u32,
    vpb_base: u32,
    reverb_base: u32,
    reverb_counts: [u16; 4],
    reverb_last8: Vec<[i16; 8]>,
}

impl ZeldaState {
    pub fn new(variant: Variant) -> ZeldaState {
        ZeldaState {
            variant,
            skip_flags: vec![0; 256],
            cmd: vec![0; 64],
            can_execute: true,
            buses: vec![vec![0; N]; BUSES],
            resampling: vec![0; 0x100],
            patterns: vec![0; 0x100],
            sine: vec![0; 0x80],
            afc: vec![0; 0x20],
            reverb_last8: vec![[0; 8]; 4],
            ..Default::default()
        }
    }

    fn rendering(&self) -> bool {
        self.frame != self.frames
    }

    fn write32(&mut self, v: u32) {
        let w = self.write;
        self.cmd[w] = v;
        self.write = (w + 1) % self.cmd.len();
    }

    fn read32(&mut self) -> u32 {
        if self.read == self.write {
            return 0;
        }
        let v = self.cmd[self.read];
        self.read = (self.read + 1) % self.cmd.len();
        v
    }
}

fn clamp16(v: i64) -> i16 {
    v.clamp(-0x8000, 0x7FFF) as i16
}

/// buf *= vol, in 1.15 (frac = 15) or 4.12 (frac = 12) fixed point, saturating.
fn scale_in_place(buf: &mut [i16], vol: u16, frac: u32) {
    for s in buf.iter_mut() {
        *s = clamp16(((*s as i32).wrapping_mul(vol as i32) >> frac) as i64);
    }
}

/// dst += src * vol (1.15, the product saturated; the sum wraps, as the DSP's does).
fn add_scaled(dst: &mut [i16], src: &[i16], vol: u16) {
    for (d, &s) in dst.iter_mut().zip(src) {
        let v = clamp16(((s as i32 * vol as i32) >> 15) as i64);
        *d = d.wrapping_add(v);
    }
}

/// dst += src * volume, the volume (16.16) ramping by `step` each sample. Returns the volume
/// after the last sample.
fn add_ramped(dst: &mut [i16], src: &[i16], mut vol: i32, step: i32) -> i32 {
    if vol == 0 && step == 0 {
        return vol;
    }
    for (d, &s) in dst.iter_mut().zip(src) {
        let v = (((vol >> 16) * s as i32) >> 16) as i16;
        *d = d.wrapping_add(v);
        vol = vol.wrapping_add(step);
    }
    vol
}

// Voice parameter block word offsets.
mod vpb {
    pub const ENABLED: usize = 0x00;
    pub const DONE: usize = 0x01;
    pub const RATIO: usize = 0x02;
    pub const RESET: usize = 0x04;
    pub const END_REACHED: usize = 0x05;
    pub const USE_CONSTANT: usize = 0x06;
    /// Six channels of (destination bus id, target volume, current volume, unused).
    pub const CHANNELS: usize = 0x08;
    pub const DOLBY_POSITION: usize = 0x28;
    pub const DOLBY_REVERB: usize = 0x29;
    pub const DOLBY_CURRENT: usize = 0x2A;
    pub const DOLBY_TARGET: usize = 0x2B;
    pub const USE_DOLBY: usize = 0x2C;
    pub const POS_FRAC: usize = 0x30;
    pub const AFC_LEFT: usize = 0x32;
    pub const CONSTANT: usize = 0x33;
    pub const POSITION: usize = 0x34;
    pub const BEFORE_LOOP: usize = 0x36;
    pub const ARAM_ADDR: usize = 0x38;
    pub const REMAINING: usize = 0x3A;
    pub const RESAMPLE_HISTORY: usize = 0x3C;
    pub const BIQUAD_X1: usize = 0x54;
    pub const BIQUAD_X2: usize = 0x55;
    pub const BIQUAD_Y1: usize = 0x56;
    pub const BIQUAD_Y2: usize = 0x57;
    /// The last decoded AFC block; its last two samples are the decoder's history.
    pub const AFC_SAMPLES: usize = 0x58;
    pub const AFC_Y2: usize = 0x66;
    pub const AFC_Y1: usize = 0x67;
    pub const LOWPASS_Y1: usize = 0x68;
    pub const LOWPASS_X1: usize = 0x69;
    pub const SOURCE: usize = 0x80;
    pub const LOOPING: usize = 0x81;
    pub const LOOP_Y1: usize = 0x82;
    pub const LOOP_Y2: usize = 0x83;
    pub const FILTERS: usize = 0x84;
    pub const END_REQUESTED: usize = 0x85;
    pub const LOOP_ADDR: usize = 0x88;
    pub const LOOP_START: usize = 0x8A;
    pub const BASE_ADDR: usize = 0x8C;
    pub const BIQUAD_B1: usize = 0xA4;
    pub const BIQUAD_B2: usize = 0xA5;
    pub const BIQUAD_A1: usize = 0xA6;
    pub const BIQUAD_A2: usize = 0xA7;
    pub const LOWPASS_COEFF: usize = 0xA8;
    pub const WORDS: usize = 0xC0;
}

/// A voice parameter block: 0xC0 words, the first 0x80 written back after each frame.
struct Vpb([u16; vpb::WORDS]);

impl Vpb {
    fn get32(&self, o: usize) -> u32 {
        ((self.0[o] as u32) << 16) | self.0[o + 1] as u32
    }
    fn set32(&mut self, o: usize, v: u32) {
        self.0[o] = (v >> 16) as u16;
        self.0[o + 1] = v as u16;
    }
    fn s(&self, o: usize) -> i16 {
        self.0[o] as i16
    }
    fn set_s(&mut self, o: usize, v: i16) {
        self.0[o] = v as u16;
    }
    /// The boot menu's 0x80-word layout into the full one: its read-only part sits at 0x40
    /// (not 0x80), its AFC samples at 0x30 (not 0x58), and the rest 0x18 earlier (no Dolby).
    fn widen(&mut self) {
        let w = &mut self.0;
        for i in 0..0x40 {
            w[0x80 + i] = w[0x40 + i];
            w[0x40 + i] = 0;
        }
        for i in 0..0x10 {
            w[0x58 + i] = w[0x30 + i];
            w[0x30 + i] = 0;
        }
        for i in 0..0x18 {
            w[0x30 + i] = w[0x18 + i];
            w[0x18 + i] = 0;
        }
    }
    fn narrow(&mut self) {
        let w = &mut self.0;
        for i in 0..0x18 {
            w[0x18 + i] = w[0x30 + i];
            w[0x30 + i] = 0;
        }
        for i in 0..0x10 {
            w[0x30 + i] = w[0x58 + i];
            w[0x58 + i] = 0;
        }
        for i in 0..0x40 {
            w[0x40 + i] = w[0x80 + i];
            w[0x80 + i] = 0;
        }
    }
}

/// Main-memory address as the DMA engines see it (segment bits dropped).
fn phys(a: u32) -> u32 {
    a & 0x1FFF_FFFF
}

impl Hw {
    fn zw(&mut self, addr: u32, n: usize) -> Vec<u16> {
        self.dma_read(phys(addr), n * 2).chunks_exact(2).map(|b| u16::from_be_bytes([b[0], b[1]])).collect()
    }

    fn zs(&mut self, addr: u32, n: usize) -> Vec<i16> {
        self.zw(addr, n).into_iter().map(|w| w as i16).collect()
    }

    fn zwrite(&mut self, addr: u32, words: &[i16]) {
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_be_bytes()).collect();
        self.dma_write(phys(addr), &bytes);
    }

    /// One byte of "audio RAM": the real thing, or on the Wii main memory from the base.
    fn z_aram(&mut self, z: &ZeldaState, off: u32) -> u8 {
        if z.variant.no_aram {
            self.dma_read(phys(z.aram_base.wrapping_add(off)), 1)[0]
        } else if (off as usize) < ARAM_SIZE {
            self.aram[off as usize]
        } else {
            0
        }
    }

    /// The program started: it says hello its own way.
    pub(crate) fn zelda_start(&mut self, variant: Variant) {
        self.dsp.hle.zelda = Some(Box::new(ZeldaState::new(variant)));
        if variant.light {
            self.dsp_post(0x8888_1111);
        } else {
            self.dsp_mail_irq(0xDCD1_0000);
            self.dsp_post(0xF355_1111);
        }
    }

    /// A mail from the CPU while the program runs.
    pub(crate) fn zelda_mail(&mut self, mail: u32) {
        let Some(mut z) = self.dsp.hle.zelda.take() else {
            return;
        };
        let after = if z.variant.light { self.zelda_mail_light(&mut z, mail) } else { self.zelda_mail_standard(&mut z, mail) };
        self.dsp.hle.zelda = Some(z);
        match after {
            After::Nothing => {}
            After::Switch => self.dsp_start_switch(),
            After::Reset => self.dsp_reset_to_rom(),
        }
    }

    fn zelda_mail_standard(&mut self, z: &mut ZeldaState, mail: u32) -> After {
        match z.mail {
            Mail::Waiting => {
                if mail & 0x8000_0000 != 0 {
                    // A task mail (the program does not check the 0xCDD1 prefix).
                    match mail & 0xFFFF {
                        1 => {
                            z.can_execute = true;
                            self.zelda_run(z);
                            return After::Switch;
                        }
                        2 => {
                            z.mail = Mail::Halted;
                            return After::Reset;
                        }
                        3 => {
                            z.can_execute = true;
                            self.zelda_run(z);
                        }
                        _ => z.mail = Mail::Halted,
                    }
                } else if mail & 0xFFFF == 0 {
                    z.mail = if z.rendering() { Mail::Rendering } else { Mail::Halted };
                } else {
                    z.mail = Mail::Writing;
                    z.expected = mail & 0xFFFF;
                }
            }
            Mail::Rendering => {
                if z.variant.sync_per_frame {
                    let base = if z.second_half { 2 } else { 0 };
                    z.skip_flags[base] = (mail >> 16) as u16;
                    z.skip_flags[base + 1] = mail as u16;
                    if z.second_half {
                        z.sync_max_voice = 0xFFFF;
                    }
                    self.zelda_render(z);
                    if z.second_half {
                        z.mail = Mail::Waiting;
                    }
                    z.second_half = !z.second_half;
                } else {
                    z.sync_max_voice = (((mail >> 16) & 0xF) + 1) << 4;
                    z.skip_flags[((mail >> 16) & 0xFF) as usize] = mail as u16;
                    self.zelda_render(z);
                    z.mail = Mail::Waiting;
                }
            }
            Mail::Writing => {
                z.write32(mail);
                z.expected -= 1;
                if z.expected == 0 {
                    z.pending += 1;
                    z.mail = Mail::Waiting;
                    self.zelda_run(z);
                }
            }
            Mail::Halted => {}
        }
        After::Nothing
    }

    fn zelda_mail_light(&mut self, z: &mut ZeldaState, mail: u32) -> After {
        match z.mail {
            Mail::Waiting => {
                // A command starts; how many words follow depends on the command.
                let cmd = (mail >> 24) & 0x7F;
                if cmd == 3 {
                    // Not a command: straight back to the dispatcher.
                    return After::Nothing;
                }
                z.write32(mail);
                z.expected = match cmd {
                    1 => 4,
                    2 => 2,
                    // In Animal Crossing's and Pikmin's version, 0B (the memory-card decode its
                    // library sends as DiplSec) and 0C (the GBA key, DagbSec) each take one
                    // word: read from the program's own command handlers (its jump table at
                    // 0x0062, and the mail reads in the handlers at 0x0721 and 0x0BA7).
                    0x0B | 0x0C if z.variant.gba_crypto => 1,
                    0x0C if z.variant.weird_0c => 2,
                    _ => 0,
                };
                if z.expected > 0 {
                    z.mail = Mail::Writing;
                } else {
                    z.pending += 1;
                    self.zelda_run(z);
                }
            }
            Mail::Writing => {
                z.write32(mail);
                z.expected -= 1;
                if z.expected == 0 {
                    z.pending += 1;
                    z.mail = Mail::Waiting;
                    self.zelda_run(z);
                }
            }
            Mail::Rendering => {
                // One sync mail a frame; every voice is rendered.
                z.sync_max_voice = u32::MAX;
                z.skip_flags.iter_mut().for_each(|f| *f = 0xFFFF);
                self.zelda_render(z);
                self.dsp.csr |= 0x80;
            }
            Mail::Halted => {}
        }
        After::Nothing
    }

    /// Acknowledges a command.
    fn zelda_ack(&mut self, z: &ZeldaState, sync: u16) {
        if z.variant.light {
            // The light protocol answers with the address of the command's handler.
            self.dsp_post(0x8000_0000 | (2 * ((sync as u32 >> 8) & 0x7F) + 0x62));
        } else {
            self.dsp_mail_irq(0xDCD1_0004);
            self.dsp_post(0xF355_0000 | sync as u32);
        }
    }

    fn zelda_run(&mut self, z: &mut ZeldaState) {
        if z.rendering() || !z.can_execute {
            return;
        }
        while z.pending > 0 {
            if z.read == z.write {
                z.pending = 0;
                break;
            }
            let m = z.read32();
            if m & 0x8000_0000 == 0 {
                continue;
            }
            let command = (m >> 24) & 0x7F;
            let sync = (m >> 16) as u16;
            let extra = m & 0xFFFF;
            z.pending -= 1;
            match command {
                0x0B if z.variant.gba_crypto => {
                    // The card decode's parameter block. Our memory card starts unlocked, so
                    // the card library never needs the answer; the word is taken and answered.
                    z.read32();
                    self.zelda_ack(z, sync);
                }
                0x00 | 0x0A | 0x0B | 0x0F => self.zelda_ack(z, sync),
                0x03 => {
                    if !z.variant.light {
                        self.zelda_ack(z, sync);
                    }
                }
                0x04..=0x09 => {
                    z.mail = Mail::Halted;
                    return;
                }
                0x01 => {
                    // Set-up: voice blocks, the resampling filter, the patterns, the positional
                    // volume table, the AFC predictors, the reverb blocks.
                    z.voices_per_frame = extra as u16;
                    z.vpb_base = z.read32();
                    let a = z.read32();
                    z.resampling = self.zs(a, 0x100);
                    z.patterns = self.zs(a + 0x200, 0x100);
                    if !z.variant.light {
                        z.sine = self.zs(a + 0x400, 0x80);
                    }
                    let afc = z.read32();
                    z.afc = self.zs(afc, 0x20);
                    z.reverb_base = z.read32();
                    self.zelda_ack(z, sync);
                }
                0x02 => {
                    // Render: this many frames, at this volume, into these buffers.
                    z.frames = (m >> 16) & 0xFF;
                    z.out_volume = extra as u16;
                    z.out_l = z.read32();
                    z.out_r = z.read32();
                    if z.variant.combined_0d {
                        z.read32();
                        z.read32();
                    }
                    z.frame = 0;
                    z.voice = 0;
                    if z.variant.light {
                        let frames = z.frames as u16;
                        self.zelda_ack(z, frames);
                        z.mail = Mail::Rendering;
                    } else {
                        self.zelda_render(z);
                    }
                    return;
                }
                0x0C => {
                    if z.variant.gba_crypto {
                        let a = z.read32();
                        self.zelda_gba_key(a);
                    } else if z.variant.weird_0c {
                        z.read32();
                        z.read32();
                    }
                    self.zelda_ack(z, sync);
                }
                0x0D => {
                    if !z.variant.no_cmd_0d {
                        z.read32();
                    }
                    self.zelda_ack(z, sync);
                }
                0x0E => {
                    z.aram_base = z.read32();
                    self.zelda_ack(z, sync);
                }
                _ => {
                    z.mail = Mail::Halted;
                    return;
                }
            }
        }
    }

    /// The GBA link's key exchange (the light versions do it here, before a separate program
    /// existed for it): the challenge unwrapped with the BIOS's "sedo" word, and the transfer
    /// parameters packed and wrapped the way the GBA's boot ROM expects.
    fn zelda_gba_key(&mut self, a: u32) {
        let w = |hw: &mut Hw, o: u32| -> u32 { u32::from_be_bytes(hw.dma_read(phys(a + o), 4).try_into().unwrap()) };
        let challenge = w(self, 0).swap_bytes();
        let palette = w(self, 4);
        let speed = w(self, 8) as u8 as i8 as i32;
        let length = w(self, 12);
        let dest = w(self, 16);
        self.dma_write(phys(dest), &(challenge ^ 0x6F64_6573).to_be_bytes());
        let mut coded: u32 = (if speed < 0 {
            (((-speed + 2) * 2) as u32) | (palette << 4)
        } else if speed == 0 {
            (palette * 2) | 0x70
        } else {
            (((speed - 1) * 2) as u32) | (palette << 4)
        }) & 0xFFFF;
        let body = (length.wrapping_add(7) & !7) as i32 - 0x200;
        let pairs: u32 = if body < 0 { 0 } else { (body / 8) as u32 & 0xFFFF };
        coded |= (pairs & 0x4000) >> 14;
        let mut t1 = (((pairs << 16) | 0x3F80) & 0x3F80_FFFF).wrapping_mul(2);
        t1 = t1.wrapping_add(((((t1 >> 8) as u8 as i8) as i16 as i32 as u32) & pairs) << 16);
        let t2 = ((coded & 0xFF) << 16).wrapping_add(t1 & 0xFF_0000).wrapping_add((t1 >> 8) & 0xFF_FF00);
        let mut t3 = (coded << 16) | ((t2 << 8) & 0xFF00_0000) | (t1 >> 16) | 0x8080_8080;
        t3 ^= if t3 & 0x200 != 0 { 0x6F64_6573 } else { 0x6177_614B };
        self.dma_write(phys(dest + 4), &t3.to_be_bytes());
    }

    /// Renders frames until done, or until the next voice is not synchronised yet.
    fn zelda_render(&mut self, z: &mut ZeldaState) {
        if !z.rendering() {
            return;
        }
        while z.frame < z.frames {
            if z.voice == 0 {
                self.zelda_prepare(z);
            }
            let voices = (z.voices_per_frame as u32).min((z.skip_flags.len() as u32) << 4);
            while z.voice < voices {
                if z.voice >= z.sync_max_voice {
                    return;
                }
                let flags = z.skip_flags[(z.voice >> 4) as usize];
                if flags & (1 << (15 - (z.voice & 15))) != 0 {
                    self.zelda_voice(z, z.voice);
                }
                z.voice += 1;
            }
            if !z.variant.light {
                let f = 0xFF00 | z.frame as u16;
                self.zelda_ack(z, f);
            }
            self.zelda_finish_frame(z);
            z.voice = 0;
            z.sync_max_voice = 0;
            z.frame += 1;
        }
        if z.variant.light {
            z.mail = Mail::Waiting;
        } else {
            // Frame end; commands wait for the CPU's go-ahead.
            self.dsp_mail_irq(0xDCD1_0005);
            z.can_execute = false;
        }
    }

    fn zelda_prepare(&mut self, z: &mut ZeldaState) {
        if z.prepared {
            return;
        }
        z.buses[FL].fill(0);
        z.buses[FR].fill(0);
        scale_in_place(&mut z.buses[BL], 0x6784, 15);
        scale_in_place(&mut z.buses[BR], 0x6784, 15);
        // Reverb from the previous frame, the back reverb buses folded into the front ones.
        self.zelda_reverb(z, false);
        let (blr, brr) = (z.buses[BL_REV].clone(), z.buses[BR_REV].clone());
        add_scaled(&mut z.buses[FL_REV], &blr, 0x7FFF);
        add_scaled(&mut z.buses[FR_REV], &blr, 0xB820);
        add_scaled(&mut z.buses[FL_REV][..0x28], &brr[0x28..], 0xB820);
        add_scaled(&mut z.buses[FR_REV][..0x28], &brr[0x28..], 0x7FFF);
        z.buses[BL_REV].fill(0);
        z.buses[BR_REV].fill(0);
        // Patterns 2 and 3 move each frame (0 and 1 are fixed).
        {
            let p2 = &mut z.patterns[0x80..0xC0];
            let (mut y2, mut y1) = (p2[0x3E] as i32, p2[0x3F] as i32);
            for i in (0..0x40).step_by(2) {
                let v = y2.wrapping_mul(y1).wrapping_sub((p2[i] as i32) << 16);
                y2 = y1;
                y1 = p2[i] as i32;
                p2[i] = (v >> 16) as i16;
                let v = 2i32.wrapping_mul(y2.wrapping_mul(y1).wrapping_add((p2[i + 1] as i32) << 16));
                y2 = y1;
                y1 = p2[i + 1] as i32;
                p2[i + 1] = (v >> 16) as i16;
            }
        }
        {
            let p3 = &mut z.patterns[0xC0..0x100];
            let (y2, y1) = (p3[0x3E] as i32, p3[0x3F] as i32);
            let acc = y1 as i16;
            let mut step = (p3[0] as i32).wrapping_add((y1.wrapping_mul(y2).wrapping_add((y2 << 16).wrapping_add(y1))) >> 16) as i16;
            step = (step & 0x1FF) | 0x2000;
            for (i, s) in p3.iter_mut().enumerate() {
                *s = acc.wrapping_add(((i as i32 + 1) * step as i32) as i16);
            }
        }
        z.prepared = true;
    }

    /// The four reverb blocks: before mixing, each one's circular buffer is read back (with
    /// the previous frame's last eight samples for the filter), filtered, sent to its
    /// destinations and kept; after mixing, its bus is written to the buffer and the buffer moves
    /// on.
    fn zelda_reverb(&mut self, z: &mut ZeldaState, after: bool) {
        if z.reverb_base == 0 {
            return;
        }
        let buses = [U0_REV, U1_REV, FL_REV, FR_REV];
        for k in 0..4 {
            let rpb = self.zw(z.reverb_base + 32 * k as u32, 16);
            if rpb[0] == 0 {
                continue;
            }
            let size = rpb[1];
            let base = ((rpb[2] as u32) << 16) | rpb[3] as u32;
            let at = base.wrapping_add(z.reverb_counts[k] as u32 * (N as u32 * 2));
            if !after {
                let mut buf = vec![0i16; N + 8];
                buf[..8].copy_from_slice(&z.reverb_last8[k]);
                let fresh = self.zs(at, N);
                buf[8..].copy_from_slice(&fresh);
                z.reverb_last8[k].copy_from_slice(&buf[N..N + 8]);
                let coeffs: Vec<i32> = rpb[8..16].iter().map(|&c| c as i16 as i32).collect();
                let filter = |buf: &mut Vec<i16>| {
                    for i in 0..N {
                        let mut s = 0i32;
                        for j in 0..8 {
                            s = s.wrapping_add(buf[i + j] as i32 * coeffs[j]);
                        }
                        buf[i] = clamp16((s >> 15) as i64);
                    }
                };
                if rpb[0] & 1 != 0 {
                    filter(&mut buf);
                }
                for d in 0..2 {
                    let (id, vol) = (rpb[4 + 2 * d], rpb[5 + 2 * d]);
                    if id == 0 {
                        continue;
                    }
                    if let Some(b) = bus_for(id) {
                        add_scaled(&mut z.buses[b], &buf[..N], vol);
                    }
                }
                if rpb[0] & 2 != 0 {
                    filter(&mut buf);
                }
                z.buses[buses[k]].copy_from_slice(&buf[..N]);
            } else {
                let data = z.buses[buses[k]].clone();
                self.zwrite(at, &data);
                z.reverb_counts[k] = if size == 0 { 0 } else { (z.reverb_counts[k] + 1) % size };
            }
        }
    }

    fn zelda_finish_frame(&mut self, z: &mut ZeldaState) {
        let vol = z.out_volume;
        scale_in_place(&mut z.buses[FL], vol, 12);
        scale_in_place(&mut z.buses[FR], vol, 12);
        let (l, r) = (z.buses[FL].clone(), z.buses[FR].clone());
        self.zwrite(z.out_l, &l);
        self.zwrite(z.out_r, &r);
        z.out_l = z.out_l.wrapping_add(2 * N as u32);
        z.out_r = z.out_r.wrapping_add(2 * N as u32);
        self.zelda_reverb(z, true);
        z.prepared = false;
    }

    fn zelda_vpb_words(z: &ZeldaState) -> usize {
        if z.variant.tiny_vpb {
            0x80
        } else {
            vpb::WORDS
        }
    }

    /// Mixes one voice into the buses.
    fn zelda_voice(&mut self, z: &mut ZeldaState, id: u32) {
        let words = Self::zelda_vpb_words(z);
        let addr = z.vpb_base.wrapping_add(id * words as u32 * 2);
        let mut v = Vpb([0; vpb::WORDS]);
        v.0[..words].copy_from_slice(&self.zw(addr, words));
        if z.variant.tiny_vpb {
            v.widen();
        }
        if v.0[vpb::ENABLED] == 0 || v.0[vpb::DONE] != 0 {
            return;
        }
        let mut input = [0i16; N];
        self.zelda_input(z, &mut v, &mut input);
        if v.0[vpb::LOWPASS_COEFF] != 0 {
            zelda_lowpass(&mut v, &mut input);
        }
        let biquad = v.0[vpb::FILTERS] & 0x20 != 0;
        if biquad && (v.0[vpb::BIQUAD_A2] != 0 || v.0[vpb::BIQUAD_A1] != 0 || v.0[vpb::BIQUAD_B2] != 0 || v.0[vpb::BIQUAD_B1] != 0x7FFF) {
            zelda_biquad(&mut v, &mut input);
        }
        if v.0[vpb::USE_DOLBY] != 0 {
            self.zelda_dolby(z, &mut v, &input);
        } else {
            let channels = if z.variant.four_dests { 4 } else { 6 };
            if v.0[vpb::END_REQUESTED] != 0 {
                let mut quiet = true;
                for c in 0..channels {
                    let o = vpb::CHANNELS + 4 * c;
                    let t = v.s(o + 2) / 2;
                    v.set_s(o + 1, t);
                    quiet &= t == 0;
                }
                if quiet {
                    v.0[vpb::DONE] = 1;
                }
            }
            for c in 0..channels {
                let o = vpb::CHANNELS + 4 * c;
                let bus_id = v.0[o];
                if bus_id == 0 {
                    continue;
                }
                let (target, current) = (v.s(o + 1), v.s(o + 2));
                let delta = if z.variant.explicit_step { target } else { target.wrapping_sub(current) };
                let step = ((delta as i32) << 16) / N as i32;
                if current == 0 && step == 0 {
                    continue;
                }
                let Some(b) = bus_for(bus_id) else { continue };
                let after = add_ramped(&mut z.buses[b], &input, (current as i32) << 16, step);
                v.set_s(o + 2, (after >> 16) as i16);
            }
        }
        if v.0[vpb::USE_CONSTANT] == 0 {
            v.0[vpb::RESET] = 0;
        }
        // Only the first part is written back; the rest is the game's.
        let back = if z.variant.tiny_vpb {
            v.narrow();
            0x40
        } else {
            0x80
        };
        let out: Vec<i16> = v.0[..back].iter().map(|&w| w as i16).collect();
        self.zwrite(addr, &out);
    }

    /// Positional mixing: the voice's place (left/right, front/back) picks four quadrant
    /// volumes from the sine table, ramped from the current to the target volume, with a share
    /// to the reverb buses.
    fn zelda_dolby(&mut self, z: &mut ZeldaState, v: &mut Vpb, input: &[i16; N]) {
        if v.0[vpb::END_REQUESTED] != 0 {
            let t = v.s(vpb::DOLBY_CURRENT) / 2;
            v.set_s(vpb::DOLBY_TARGET, t);
            if t == 0 {
                v.0[vpb::DONE] = 1;
            }
        }
        let pos = v.0[vpb::DOLBY_POSITION];
        let (x, y) = (((pos >> 8) & 0x7F) as usize, (pos & 0x7F) as usize);
        let sine = |i: usize| z.sine.get(i).copied().unwrap_or(0) as i32;
        let (right, back, left, front) = (sine(x), sine(y), sine(x ^ 0x7F), sine(y ^ 0x7F));
        let shift = if z.variant.louder_dolby { 15 } else { 16 };
        let mut quad = [(left * front) >> shift, (left * back) >> shift, (right * front) >> shift, (right * back) >> shift].map(|q| q as i16);
        let delta = v.s(vpb::DOLBY_TARGET).wrapping_sub(v.s(vpb::DOLBY_CURRENT)) as i32;
        let deltas = quad.map(|q| ((q as u16 as i32 * delta) >> shift) as i16);
        let current = v.s(vpb::DOLBY_CURRENT) as i32;
        for q in quad.iter_mut() {
            *q = ((*q as i32 * current) >> shift) as i16;
        }
        let reverb = v.s(vpb::DOLBY_REVERB) as i32;
        let rq = quad.map(|q| ((q as i32 * reverb) >> shift) as i16);
        let rd = deltas.map(|d| ((d as i32 * reverb) >> shift) as i16);
        let targets = [(FL, quad[0], deltas[0]), (BL, quad[1], deltas[1]), (FR, quad[2], deltas[2]), (BR, quad[3], deltas[3]), (FL_REV, rq[0], rd[0]), (BL_REV, rq[1], rd[1]), (FR_REV, rq[2], rd[2]), (BR_REV, rq[3], rd[3])];
        for (b, vol, d) in targets {
            add_ramped(&mut z.buses[b], input, (vol as i32) << 16, ((d as i32) << 16) / N as i32);
        }
        v.0[vpb::DOLBY_CURRENT] = v.0[vpb::DOLBY_TARGET];
    }

    /// The voice's 80 input samples: from its source, then resampled.
    fn zelda_input(&mut self, z: &mut ZeldaState, v: &mut Vpb, out: &mut [i16; N]) {
        // Raw samples, after the four kept from the previous frame.
        let mut raw = vec![0i16; 4 + 0x500 + 0x10];
        for i in 0..4 {
            raw[i] = v.s(vpb::RESAMPLE_HISTORY + i);
        }
        if v.0[vpb::USE_CONSTANT] != 0 {
            out.fill(v.s(vpb::CONSTANT));
            return;
        }
        let ratio = v.0[vpb::RATIO] as u32;
        let needed = ((v.0[vpb::POS_FRAC] as u32 + N as u32 * ratio) >> 12) as usize;
        match v.0[vpb::SOURCE] {
            src @ (0 | 3) => {
                // Square waves: 50% and 25% duty.
                let shift = if src == 0 { 1 } else { 2 };
                let mask = (1u32 << shift) - 1;
                let r = ratio << (shift - 1);
                let mut pos = (v.0[vpb::POS_FRAC] as u32) << shift;
                for s in out.iter_mut() {
                    *s = if (pos >> 16) & mask != 0 { 0xC000u16 as i16 } else { 0x4000 };
                    pos = pos.wrapping_add(r);
                }
                v.0[vpb::POS_FRAC] = ((pos >> shift) & 0xFFFF) as u16;
            }
            1 => {
                // Saw wave.
                let mut pos = v.0[vpb::POS_FRAC] as u32;
                for s in out.iter_mut() {
                    *s = (pos & 0xFFFF) as u16 as i16;
                    pos = pos.wrapping_add(ratio >> 1);
                }
                v.0[vpb::POS_FRAC] = (pos & 0xFFFF) as u16;
            }
            src @ (7 | 10 | 4 | 11 | 12) => {
                // The uploaded patterns, 0x40 samples each.
                let (index, variable) = match src {
                    7 => (0, false),
                    10 => (0, true),
                    4 => (1, false),
                    11 => (2, false),
                    _ => (3, false),
                };
                let pattern: Vec<i16> = z.patterns[index * 0x40..index * 0x40 + 0x40].to_vec();
                let mut pos = (v.0[vpb::POS_FRAC] as u32) << 6;
                let step = ratio << 5;
                for (i, s) in out.iter_mut().enumerate() {
                    *s = pattern[((pos >> 16) & 0x3F) as usize];
                    pos = pos.wrapping_add(step) % (0x40 << 16);
                    if variable {
                        let br = z.buses[BR][i] as i32;
                        pos = ((pos << 10) as i32).wrapping_add(br.wrapping_mul(ratio as i32)) as u32 >> 10;
                    }
                }
                v.0[vpb::POS_FRAC] = (pos >> 6) as u16;
            }
            8 => {
                self.zelda_pcm(z, v, &mut raw[4..], needed, 1);
                zelda_resample(z, v, &raw, out);
            }
            16 => {
                self.zelda_pcm(z, v, &mut raw[4..], needed, 2);
                zelda_resample(z, v, &raw, out);
            }
            5 | 9 => {
                self.zelda_afc(z, v, &mut raw[4..], needed);
                zelda_resample(z, v, &raw, out);
            }
            33 => {
                self.zelda_from_main(v, &mut raw[4..], needed);
                zelda_resample(z, v, &raw, out);
            }
            _ => out.fill(0),
        }
    }

    /// 8- or 16-bit PCM from audio RAM, looping or ending at the loop's end.
    fn zelda_pcm(&mut self, z: &ZeldaState, v: &mut Vpb, dst: &mut [i16], mut want: usize, size: u32) {
        if v.0[vpb::DONE] != 0 {
            dst[..want].fill(0);
            return;
        }
        let restart = |v: &mut Vpb| {
            let start = v.get32(vpb::LOOP_START);
            let pos = v.get32(vpb::POSITION);
            v.set32(vpb::REMAINING, start.wrapping_sub(pos));
            v.set32(vpb::ARAM_ADDR, v.get32(vpb::BASE_ADDR).wrapping_add(pos.wrapping_mul(size)));
        };
        if v.0[vpb::RESET] != 0 {
            restart(v);
        }
        v.0[vpb::END_REACHED] = 0;
        let mut at = 0;
        let mut guard = 0;
        while want > 0 {
            guard += 1;
            if guard > 64 {
                dst[at..at + want].fill(0);
                break;
            }
            if v.0[vpb::END_REACHED] != 0 {
                v.0[vpb::END_REACHED] = 0;
                if v.0[vpb::LOOPING] == 0 {
                    dst[at..at + want].fill(0);
                    v.0[vpb::DONE] = 1;
                    break;
                }
                let la = v.get32(vpb::LOOP_ADDR);
                v.set32(vpb::POSITION, la);
                restart(v);
            }
            let n = (v.get32(vpb::REMAINING) as usize).min(want);
            let mut a = v.get32(vpb::ARAM_ADDR);
            for k in 0..n {
                dst[at + k] = if size == 1 {
                    ((self.z_aram(z, a) as i8 as i16) << 8) as i16
                } else {
                    i16::from_be_bytes([self.z_aram(z, a), self.z_aram(z, a + 1)])
                };
                a = a.wrapping_add(size);
            }
            at += n;
            want -= n;
            v.set32(vpb::REMAINING, v.get32(vpb::REMAINING) - n as u32);
            v.set32(vpb::ARAM_ADDR, a);
            if v.get32(vpb::REMAINING) == 0 {
                v.0[vpb::END_REACHED] = 1;
            }
        }
    }

    /// AFC from audio RAM: 16-sample blocks (9 bytes: 4-bit; 5 bytes: 2-bit), decoded in whole
    /// blocks, the spare samples of the last block kept in the voice for the next frame.
    fn zelda_afc(&mut self, z: &ZeldaState, v: &mut Vpb, dst: &mut [i16], mut want: usize) {
        if v.0[vpb::RESET] != 0 {
            v.0[vpb::AFC_Y1] = 0;
            v.0[vpb::AFC_Y2] = 0;
            v.0[vpb::AFC_LEFT] = 0;
            let start = v.get32(vpb::LOOP_START);
            v.set32(vpb::REMAINING, start);
            let base = v.get32(vpb::BASE_ADDR);
            v.set32(vpb::ARAM_ADDR, base);
        }
        if v.0[vpb::DONE] != 0 {
            dst[..want].fill(0);
            return;
        }
        if v.0[vpb::AFC_LEFT] > 0x10 {
            v.0[vpb::AFC_LEFT] = 0x10;
        }
        let mut at = 0usize;
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > 64 {
                dst[at..at + want].fill(0);
                return;
            }
            // Samples already decoded.
            let left = v.0[vpb::AFC_LEFT] as usize;
            let take = left.min(want);
            for i in 0..take {
                dst[at + i] = v.s(vpb::AFC_SAMPLES + 0x10 - left + i);
            }
            v.0[vpb::AFC_LEFT] = (left - take) as u16;
            at += take;
            want -= take;
            let remaining = v.get32(vpb::REMAINING) as usize;
            if want == 0 {
                return;
            } else if want <= remaining {
                let blocks = want.div_ceil(16);
                let decoded = blocks * 16;
                if decoded < remaining {
                    v.0[vpb::AFC_LEFT] = (decoded - want) as u16;
                    v.set32(vpb::REMAINING, (remaining - decoded) as u32);
                } else {
                    v.0[vpb::AFC_LEFT] = (remaining - want) as u16;
                    v.set32(vpb::REMAINING, 0);
                }
                let mut block = vec![0i16; decoded];
                self.zelda_afc_decode(z, v, &mut block, blocks);
                dst[at..at + want].copy_from_slice(&block[..want]);
                if v.0[vpb::AFC_LEFT] != 0 {
                    for i in 0..0x10 {
                        v.set_s(vpb::AFC_SAMPLES + i, block[decoded - 0x10 + i]);
                    }
                    let ls = v.get32(vpb::LOOP_START);
                    if v.get32(vpb::REMAINING) == 0 && ls != 0 {
                        // Line the kept samples up for the loop's next pass.
                        let left = v.0[vpb::AFC_LEFT] as usize;
                        let mut from = ((ls + 0xF) & 0xF) as isize;
                        for i in 0..left {
                            let val = if from >= 0 { v.0[vpb::AFC_SAMPLES + from as usize] } else { 0 };
                            v.0[vpb::AFC_SAMPLES + 0x10 - i - 1] = val;
                            from -= 1;
                        }
                    }
                }
                return;
            } else {
                // More wanted than there is: the end, or the loop.
                if remaining > 0 {
                    want -= remaining;
                    let blocks = remaining.div_ceil(16);
                    let mut block = vec![0i16; blocks * 16];
                    self.zelda_afc_decode(z, v, &mut block, blocks);
                    dst[at..at + remaining].copy_from_slice(&block[..remaining]);
                    at += remaining;
                }
                if v.0[vpb::LOOPING] == 0 {
                    v.0[vpb::DONE] = 1;
                    dst[at..at + want].fill(0);
                    return;
                }
                let source = v.0[vpb::SOURCE] as u32;
                let la = v.get32(vpb::LOOP_ADDR);
                let start = v.get32(vpb::BASE_ADDR).wrapping_add((la >> 4) * source);
                v.set32(vpb::ARAM_ADDR, start);
                v.0[vpb::AFC_Y2] = v.0[vpb::LOOP_Y2];
                v.0[vpb::AFC_Y1] = v.0[vpb::LOOP_Y1];
                let mut block = [0i16; 16];
                self.zelda_afc_decode(z, v, &mut block, 1);
                for i in 0..16 {
                    v.set_s(vpb::AFC_SAMPLES + i, block[i]);
                }
                let left = 0x10 - (la & 0xF);
                v.0[vpb::AFC_LEFT] = left as u16;
                let rem = v.get32(vpb::LOOP_START).wrapping_sub(left).wrapping_sub(la);
                v.set32(vpb::REMAINING, rem);
            }
        }
    }

    fn zelda_afc_decode(&mut self, z: &ZeldaState, v: &mut Vpb, dst: &mut [i16], blocks: usize) {
        let source = v.0[vpb::SOURCE] as u32;
        let mut a = v.get32(vpb::ARAM_ADDR);
        v.set32(vpb::ARAM_ADDR, a.wrapping_add(blocks as u32 * source));
        let (mut y1, mut y2) = (v.s(vpb::AFC_Y1) as i32, v.s(vpb::AFC_Y2) as i32);
        for b in 0..blocks {
            let head = self.z_aram(z, a);
            a = a.wrapping_add(1);
            let delta = (1i32 << (head >> 4)) as i16 as i32;
            let k = (head & 0xF) as usize;
            let mut nib = [0i32; 16];
            if source == 9 {
                for i in (0..16).step_by(2) {
                    let byte = self.z_aram(z, a);
                    a = a.wrapping_add(1);
                    nib[i] = (((byte >> 4) as i16) << 12) as i32 >> 1;
                    nib[i + 1] = (((byte & 0xF) as i16) << 12) as i32 >> 1;
                }
            } else {
                for i in (0..16).step_by(4) {
                    let byte = self.z_aram(z, a);
                    a = a.wrapping_add(1);
                    for j in 0..4 {
                        nib[i + j] = ((((byte >> (6 - 2 * j)) & 3) as i16) << 14) as i32 >> 1;
                    }
                }
            }
            let (c1, c2) = (z.afc.get(2 * k).copied().unwrap_or(0) as i32, z.afc.get(2 * k + 1).copied().unwrap_or(0) as i32);
            for (i, &n) in nib.iter().enumerate() {
                let s = delta.wrapping_mul(n).wrapping_add(y1.wrapping_mul(c1)).wrapping_add(y2.wrapping_mul(c2)) >> 11;
                let s = s.clamp(-0x8000, 0x7FFF);
                dst[b * 16 + i] = s as i16;
                y2 = y1;
                y1 = s;
            }
        }
        v.set_s(vpb::AFC_Y2, y2 as i16);
        v.set_s(vpb::AFC_Y1, y1 as i16);
    }

    /// 16-bit PCM straight from main memory, with its loop.
    fn zelda_from_main(&mut self, v: &mut Vpb, dst: &mut [i16], want: usize) {
        let addr = v.get32(vpb::BASE_ADDR).wrapping_add(v.0[vpb::POSITION] as u32 * 2);
        let remaining = v.get32(vpb::REMAINING) as usize;
        if want > remaining {
            let mut last = 0i16;
            if remaining > 0 {
                let s = self.zs(addr, remaining);
                dst[..remaining].copy_from_slice(&s);
                last = s[remaining - 1];
            }
            dst[remaining..want].fill(last);
            v.0[vpb::POSITION] = v.0[vpb::POSITION].wrapping_add(remaining as u16);
            v.set32(vpb::REMAINING, 0);
            v.0[vpb::DONE] = 1;
        } else {
            v.set32(vpb::REMAINING, (remaining - want) as u32);
            let before = v.0[vpb::LOOP_START].wrapping_sub(v.0[vpb::POSITION]);
            v.0[vpb::BEFORE_LOOP] = before;
            let before = before as usize;
            if want <= before {
                let s = self.zs(addr, want);
                dst[..want].copy_from_slice(&s);
                v.0[vpb::POSITION] = v.0[vpb::POSITION].wrapping_add(want as u16);
            } else {
                let s = self.zs(addr, before);
                dst[..before].copy_from_slice(&s);
                let la = v.get32(vpb::LOOP_ADDR);
                v.set32(vpb::BASE_ADDR, la);
                let rest = want - before;
                v.0[vpb::POSITION] = rest as u16;
                let s = self.zs(la, rest);
                dst[before..want].copy_from_slice(&s);
            }
        }
    }
}

/// Resamples the raw samples (four kept ones first) into the voice's 80, by its 4.12 ratio: a
/// 4-tap filter chosen by the position's fraction, or the nearest sample above 4:1.
fn zelda_resample(z: &ZeldaState, v: &mut Vpb, raw: &[i16], out: &mut [i16; N]) {
    let ratio = v.0[vpb::RATIO] as u32;
    let mut pos = v.0[vpb::POS_FRAC] as u32;
    let at = |i: usize| raw.get(i).copied().unwrap_or(0) as i64;
    if ratio >> 12 >= 4 {
        for s in out.iter_mut() {
            pos = pos.wrapping_add(ratio);
            *s = at((pos >> 12) as usize) as i16;
        }
    } else {
        for s in out.iter_mut() {
            let c = (((pos & 0xFFF) >> 6) * 4) as usize;
            let base = (pos >> 12) as usize;
            let mut sum = 0i64;
            for i in 0..4 {
                sum += 2 * z.resampling.get(c + i).copied().unwrap_or(0) as i64 * at(base + i);
            }
            *s = clamp16(sum >> 16);
            pos = pos.wrapping_add(ratio);
        }
    }
    for i in 0..4 {
        v.set_s(vpb::RESAMPLE_HISTORY + i, at((pos >> 12) as usize + i) as i16);
    }
    v.set_s(vpb::CONSTANT, out[N - 1]);
    v.0[vpb::POS_FRAC] = (pos & 0xFFF) as u16;
}

fn zelda_lowpass(v: &mut Vpb, buf: &mut [i16; N]) {
    let reset = v.0[vpb::RESET] != 0;
    let mut y1 = if reset { 0 } else { v.s(vpb::LOWPASS_Y1) as i64 };
    let mut x1 = if reset { 0 } else { v.s(vpb::LOWPASS_X1) as i64 };
    let coeff = v.0[vpb::LOWPASS_COEFF] as i64;
    for s in buf.iter_mut() {
        let x0 = *s as i64;
        let y0 = clamp16((((x0 - x1) * coeff) >> 7) + y1);
        *s = y0;
        y1 = y0 as i64;
        x1 = x0;
    }
    v.set_s(vpb::LOWPASS_Y1, y1 as i16);
    v.set_s(vpb::LOWPASS_X1, x1 as i16);
}

fn zelda_biquad(v: &mut Vpb, buf: &mut [i16; N]) {
    let (mut x1, mut x2, mut y1, mut y2) = (v.s(vpb::BIQUAD_X1) as i64, v.s(vpb::BIQUAD_X2) as i64, v.s(vpb::BIQUAD_Y1) as i64, v.s(vpb::BIQUAD_Y2) as i64);
    let (b1, b2, a1, a2) = (v.s(vpb::BIQUAD_B1) as i64, v.s(vpb::BIQUAD_B2) as i64, v.s(vpb::BIQUAD_A1) as i64, v.s(vpb::BIQUAD_A2) as i64);
    for s in buf.iter_mut() {
        let x0 = *s as i64;
        let y0 = clamp16((b1 * x1 + b2 * x2 + a1 * y1 + a2 * y2) >> 15);
        *s = y0;
        x2 = x1;
        x1 = x0;
        y2 = y1;
        y1 = y0 as i64;
    }
    v.set_s(vpb::BIQUAD_X1, x1 as i16);
    v.set_s(vpb::BIQUAD_X2, x2 as i16);
    v.set_s(vpb::BIQUAD_Y1, y1 as i16);
    v.set_s(vpb::BIQUAD_Y2, y2 as i16);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(hw: &mut Hw, addr: u32, w: &[u16]) {
        let b: Vec<u8> = w.iter().flat_map(|x| x.to_be_bytes()).collect();
        hw.dma_write(addr, &b);
    }

    fn take_mail(hw: &mut Hw) -> Option<u32> {
        if !hw.dsp.dsp_mbox_full {
            return None;
        }
        let m = hw.dsp.dsp_mbox | 0x8000_0000;
        hw.dsp.dsp_mbox_full = false;
        hw.dsp_deliver();
        Some(m)
    }

    /// The light protocol (Luigi's Mansion, Animal Crossing): the hello, a set-up command, a
    /// render command and one sync mail per frame, each answered as the program answers, and a
    /// voice of 16-bit PCM from audio RAM mixed to the front left output.
    #[test]
    fn light_protocol_renders_a_voice() {
        let mut hw = Hw::new(false);
        hw.zelda_start(variant(0x42F6_4AC4).unwrap());
        assert_eq!(take_mail(&mut hw), Some(0x8888_1111));
        // Tables: a resampling filter that passes the fourth tap at half (2 * 0x4000 >> 16), no
        // reverb.
        let tables = 0x10_0000u32;
        let mut t = vec![0u16; 0x400];
        for row in 0..0x40 {
            t[row * 4 + 3] = 0x4000;
        }
        words(&mut hw, tables, &t);
        let afc = 0x11_0000u32;
        words(&mut hw, afc, &[0u16; 0x20]);
        // One voice: PCM16 from audio RAM at ratio 1.0, to the front left bus at full volume.
        let vpbs = 0x12_0000u32;
        let mut v = [0u16; vpb::WORDS];
        v[vpb::ENABLED] = 1;
        v[vpb::RATIO] = 0x1000;
        v[vpb::RESET] = 1;
        v[vpb::CHANNELS] = 0x0D00;
        v[vpb::CHANNELS + 1] = 0x7FFF;
        v[vpb::CHANNELS + 2] = 0x7FFF;
        v[vpb::SOURCE] = 16;
        v[vpb::LOOP_START + 1] = 1000; // loop end, in samples
        v[vpb::BASE_ADDR + 1] = 0x4000; // audio RAM byte address
        words(&mut hw, vpbs, &v);
        for k in 0..1000u32 {
            let s = ((k as i32 * 37) % 2000 - 1000) as i16;
            hw.aram[0x4000 + 2 * k as usize..0x4000 + 2 * k as usize + 2].copy_from_slice(&s.to_be_bytes());
        }
        // Set-up: command 01 with one voice, then the four words.
        for m in [0x8100_0001u32, vpbs, tables, afc, 0] {
            hw.zelda_mail(m);
        }
        assert_eq!(take_mail(&mut hw), Some(0x8000_0064));
        // Render two frames at volume 1.0 (4.12) into two buffers.
        let (l, r) = (0x13_0000u32, 0x14_0000u32);
        for m in [0x8202_1000u32, l, r] {
            hw.zelda_mail(m);
        }
        assert_eq!(take_mail(&mut hw), Some(0x8000_0062));
        hw.dsp.csr = 0;
        hw.zelda_mail(0);
        assert!(hw.dsp.csr & 0x80 != 0, "a frame raises the DSP interrupt");
        hw.zelda_mail(0);
        let out = hw.dma_read(l, 2 * N * 2);
        let right = hw.dma_read(r, 2 * N * 2);
        let sample = |k: i32| -> i32 { if k < 0 { 0 } else { (k * 37) % 2000 - 1000 } };
        for k in 0..2 * N {
            let got = i16::from_be_bytes([out[2 * k], out[2 * k + 1]]) as i32;
            // The filter's fourth tap at half, then the channel at 0x7FFF: input sample k - 1
            // (three history samples come first, then the new ones).
            let x = (2 * 0x4000 * sample(k as i32 - 1)) >> 16;
            let want = (0x7FFF * x) >> 16;
            assert_eq!(got, want, "sample {k}");
            assert_eq!(i16::from_be_bytes([right[2 * k], right[2 * k + 1]]), 0);
        }
        // The voice moved on 160 samples.
        let back = hw.dma_read(vpbs, vpb::WORDS * 2);
        let w = |o: usize| u16::from_be_bytes([back[2 * o], back[2 * o + 1]]) as u32;
        assert_eq!((w(vpb::ARAM_ADDR) << 16 | w(vpb::ARAM_ADDR + 1)) - 0x4000, 2 * 2 * N as u32);
    }

    /// AFC: a 9-byte block decodes its sixteen 4-bit differences, scaled by 2^exponent.
    #[test]
    fn afc_blocks_decode() {
        let mut hw = Hw::new(false);
        let z = ZeldaState::new(variant(0x4BE6_A5CB).unwrap());
        // Exponent 2, predictor 0 (zero coefficients): sample = 4 * nibble.
        let block = [0x20u8, 0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF];
        hw.aram[0x100..0x109].copy_from_slice(&block);
        let mut v = Vpb([0; vpb::WORDS]);
        v.0[vpb::SOURCE] = 9;
        v.set32(vpb::ARAM_ADDR, 0x100);
        let mut out = [0i16; 16];
        hw.zelda_afc_decode(&z, &mut v, &mut out, 1);
        let want: Vec<i16> = (0..16).map(|n: i16| if n < 8 { 4 * n } else { 4 * (n - 16) }).collect();
        assert_eq!(out.to_vec(), want);
        assert_eq!(v.get32(vpb::ARAM_ADDR), 0x109);
    }

    /// The later protocol (The Wind Waker): hello, counted command mails, acknowledgements with
    /// their sync words, voices synchronised sixteen at a time, a sync per frame, and the frame
    /// end that holds commands until the CPU says continue.
    #[test]
    fn standard_protocol_answers_as_the_program_does() {
        let mut hw = Hw::new(false);
        hw.zelda_start(variant(0x8684_0740).unwrap());
        assert_eq!(take_mail(&mut hw), Some(0xDCD1_0000));
        assert_eq!(take_mail(&mut hw), Some(0xF355_1111));
        let tables = 0x10_0000u32;
        words(&mut hw, tables, &[0u16; 0x480]);
        let afc = 0x11_0000u32;
        words(&mut hw, afc, &[0u16; 0x20]);
        let vpbs = 0x12_0000u32;
        words(&mut hw, vpbs, &[0u16; vpb::WORDS]);
        // Five command words follow: set-up with one voice.
        for m in [5u32, 0x8100_0001, vpbs, tables, afc, 0] {
            hw.zelda_mail(m);
        }
        assert_eq!(take_mail(&mut hw), Some(0xDCD1_0004));
        assert_eq!(take_mail(&mut hw), Some(0xF355_8100));
        // Render two frames.
        let (l, r) = (0x13_0000u32, 0x14_0000u32);
        for m in [3u32, 0x8202_1000, l, r] {
            hw.zelda_mail(m);
        }
        assert_eq!(take_mail(&mut hw), None, "nothing until the voices are synchronised");
        // Frame 0: sync header, then the flags for voices 0-15.
        hw.zelda_mail(0);
        hw.zelda_mail(0x0000_FFFF);
        assert_eq!(take_mail(&mut hw), Some(0xDCD1_0004));
        assert_eq!(take_mail(&mut hw), Some(0xF355_FF00));
        hw.zelda_mail(0);
        hw.zelda_mail(0x0000_FFFF);
        assert_eq!(take_mail(&mut hw), Some(0xDCD1_0004));
        assert_eq!(take_mail(&mut hw), Some(0xF355_FF01));
        assert_eq!(take_mail(&mut hw), Some(0xDCD1_0005), "frame end");
        // Commands wait for the go-ahead.
        hw.zelda_mail(1);
        hw.zelda_mail(0x8000_0000);
        assert_eq!(take_mail(&mut hw), None);
        hw.zelda_mail(0xCDD1_0003);
        assert_eq!(take_mail(&mut hw), Some(0xDCD1_0004));
        assert_eq!(take_mail(&mut hw), Some(0xF355_8000));
    }

    #[test]
    fn versions_are_recognised() {
        assert!(variant(0x42F6_4AC4).unwrap().light);
        assert!(variant(0x4BE6_A5CB).unwrap().gba_crypto);
        assert!(variant(0x56D3_6052).unwrap().sync_per_frame);
        assert!(!variant(0x8684_0740).unwrap().light);
        assert!(variant(0x1234_5678).is_none());
    }
}
