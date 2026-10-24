//! Our stand-in for AX, the sound program ("microcode") most GameCube games upload to the DSP,
//! and for the small memory-card program games switch to when they unlock a card.
//!
//! AX mixes up to 64 voices in 5 ms frames at 32 kHz. Every frame the CPU sends a command list
//! (mail 0xBABE0000 | its length in 16-bit words, then its address); the list sets up the mixing
//! buffers, points at a chain of voice parameter blocks (PBs) in main memory, processes them,
//! mixes the auxiliary effect buses the CPU has worked on, and writes the finished frame back to
//! main memory, which the CPU then plays through the audio DMA. When the list is done the DSP
//! yields (0xDCD10002 with its interrupt) and waits for a task mail (0xCDD1xxxx).
//!
//! Each voice is read through the DSP's sample accelerator from audio RAM (4-bit ADPCM, 8- and
//! 16-bit PCM, with its loop and end handling), resampled by its 16.16 ratio, scaled by its
//! volume envelope, optionally low-pass filtered, and added to the main, AUX A and AUX B buses
//! (left, right, surround) with per-sample volume ramps. Parameter changes the CPU queued for a
//! given millisecond are applied at that millisecond.
//!
//! Written from the DSP's documented behaviour; no other emulator's code.

use crate::hw::{Hw, ARAM_SIZE};
use serde::{Deserialize, Serialize};

/// Samples per millisecond (AX always mixes at 32 kHz) and milliseconds per frame.
const SPMS: usize = 32;
const MS: usize = 5;
const FRAME: usize = SPMS * MS;

/// Parameter-block word offsets (16-bit words, big-endian in memory).
mod pb {
    pub const NEXT_HI: usize = 0;
    pub const NEXT_LO: usize = 1;
    pub const SRC_TYPE: usize = 4;
    pub const MIXER_CONTROL: usize = 6;
    pub const RUNNING: usize = 7;
    pub const IS_STREAM: usize = 8;
    /// Volume and volume delta of each bus, in mixing order: main L, R, S, AUX A L, R, S,
    /// AUX B L, R, S. (In memory the surround pairs are stored B, main, A.)
    pub const MIX: [usize; 9] = [9, 11, 23, 13, 15, 25, 17, 19, 21];
    pub const UPDATES: usize = 34; // five counts, then the list's address
    pub const UPDATES_HI: usize = 39;
    pub const UPDATES_LO: usize = 40;
    /// The last sample sent to each bus, same order as MIX.
    pub const DPOP: [usize; 9] = [41, 44, 47, 42, 45, 48, 43, 46, 49];
    pub const VOL: usize = 50;
    pub const VOL_DELTA: usize = 51;
    pub const LOOPING: usize = 55;
    pub const FORMAT: usize = 56;
    pub const LOOP_HI: usize = 57;
    pub const END_HI: usize = 59;
    pub const CUR_HI: usize = 61;
    pub const COEFS: usize = 63;
    pub const GAIN: usize = 79;
    pub const PRED_SCALE: usize = 80;
    pub const YN1: usize = 81;
    pub const YN2: usize = 82;
    pub const RATIO_HI: usize = 83;
    pub const FRAC: usize = 85;
    pub const LAST: usize = 86;
    pub const LOOP_PRED_SCALE: usize = 90;
    pub const LOOP_YN1: usize = 91;
    pub const LOOP_YN2: usize = 92;
    pub const LPF_ON: usize = 93;
    pub const LPF_YN1: usize = 94;
    pub const LPF_A0: usize = 95;
    pub const LPF_B0: usize = 96;
    pub const LOOP_COUNTER: usize = 97;
    /// Words the DSP reads and writes back (the oldest AX has no low-pass filter, and its loop
    /// counter follows the loop info directly).
    pub const WORDS: usize = 98;
    pub const WORDS_NO_LPF: usize = 94;
}

/// The two known AX families that differ in their parameter blocks: the oldest (Ector checksum
/// 0x4E8A8B21: Super Smash Bros. Melee, Crazy Taxi, Mario Party 4 and others) has no low-pass
/// filter and its own mixer-control bits.
pub const AX_OLDEST: u32 = 0x4E8A_8B21;

#[derive(Clone, Serialize, Deserialize, Default, Debug, PartialEq, Eq)]
enum MailState {
    #[default]
    WaitSize,
    WaitAddress,
    WaitTask,
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct AxState {
    /// The program's Ector checksum (which AX family it is).
    pub crc: u32,
    mail: MailState,
    list_size: u16,
    /// Main, AUX A and AUX B buses (left, right, surround), 160 samples each.
    bufs: Vec<i32>,
    compressor_pos: u16,
}

/// Dolphin's and the community's name for the checksum programs are recognised by: a rotate-xor
/// over the uploaded instruction memory.
pub fn ector(code: &[u8]) -> u32 {
    let mut c = 0u32;
    for &b in code {
        c ^= b as u32;
        c = c.rotate_left(3);
    }
    c
}

/// The bus each mixing slot is: main L, R, S, AUX A L, R, S, AUX B L, R, S.
fn bus(b: usize) -> std::ops::Range<usize> {
    b * FRAME..(b + 1) * FRAME
}

/// Mixer-control bits as the voice mixer reads them: for each of the nine buses, "on" at bit
/// 2n and "ramp the volume" at bit 2n + 1.
fn mixer_control(crc: u32, mc: u16) -> u32 {
    let mc = mc as u32;
    let (ml, mr, ms, al, ar, a_s, bl, br, bs) = (1 << 0, 1 << 2, 1 << 4, 1 << 6, 1 << 8, 1 << 10, 1 << 12, 1 << 14, 1 << 16);
    let ramp = |on: u32| on << 1;
    let mut r = 0;
    if crc == AX_OLDEST {
        r |= ml | mr;
        if mc & 0x10 != 0 {
            // Dolby Pro Logic II mixing.
            if mc & 0x6 == 0 {
                r |= bl | br;
            }
            if mc & 0x7 == 1 {
                r |= al | ar | a_s;
            }
        } else {
            if mc & 1 != 0 {
                r |= al | ar;
            }
            if mc & 2 != 0 {
                r |= bl | br;
            }
            if mc & 4 != 0 {
                r |= ms;
                if r & al != 0 {
                    r |= a_s;
                }
                if r & bl != 0 {
                    r |= bs;
                }
            }
        }
        if mc & 8 != 0 {
            r |= 0xAAAAAA;
        }
    } else {
        let bits = [(0x1, ml), (0x2, mr), (0x4, ms), (0x10, al), (0x20, ar), (0x80, a_s), (0x200, bl), (0x400, br), (0x1000, bs)];
        for (m, b) in bits {
            if mc & m != 0 {
                r |= b;
            }
        }
        if mc & 0x8 != 0 {
            r |= ramp(ml) | ramp(mr) | ramp(ms);
        }
        if mc & 0x40 != 0 {
            r |= ramp(al) | ramp(ar);
        }
        if mc & 0x100 != 0 {
            r |= ramp(a_s);
        }
        if mc & 0x800 != 0 {
            r |= ramp(bl) | ramp(br);
        }
        if mc & 0x2000 != 0 {
            r |= ramp(bs);
        }
    }
    r
}

fn clamp16(v: i64) -> i16 {
    v.clamp(-0x8000, 0x7FFF) as i16
}

/// The DSP's sample accelerator, as one voice sets it up for one millisecond.
struct Accel {
    start: u32,
    end: u32,
    cur: u32,
    format: u16,
    yn1: i16,
    yn2: i16,
    gain: i16,
    pred_scale: u16,
    stopped: bool,
}

impl Accel {
    fn aram(aram: &[u8], a: u32) -> u8 {
        aram[a as usize & (ARAM_SIZE - 1)]
    }

    /// The raw sample at the current address, by the format's access size.
    fn raw(&self, aram: &[u8]) -> u16 {
        match self.format & 3 {
            0 => {
                let b = Self::aram(aram, self.cur >> 1) as u16;
                if self.cur & 1 != 0 {
                    b & 15
                } else {
                    b >> 4
                }
            }
            1 => Self::aram(aram, self.cur) as u16,
            2 => ((Self::aram(aram, self.cur.wrapping_mul(2)) as u16) << 8) | Self::aram(aram, self.cur.wrapping_mul(2).wrapping_add(1)) as u16,
            _ => 0,
        }
    }

    /// One decoded sample. Returns it and whether the end address was passed (the voice then
    /// loops or stops, which the caller decides).
    fn read(&mut self, aram: &[u8], coefs: &[i16; 16]) -> (i16, bool) {
        if self.stopped {
            return (0, false);
        }
        let raw = self.raw(aram) as i16;
        let k = ((self.pred_scale >> 4) & 7) as usize;
        let (c1, c2) = (coefs[2 * k] as i64, coefs[2 * k + 1] as i64);
        let (val, mut step);
        if (self.format >> 2) & 3 == 0 {
            // ADPCM: a 4-bit difference scaled by 2^scale plus the two-tap prediction.
            let mut n = (raw & 15) as i64;
            if n >= 8 {
                n -= 16;
            }
            let scale = 1i64 << (self.pred_scale & 15);
            val = clamp16(scale * n + ((0x400 + c1 * self.yn1 as i64 + c2 * self.yn2 as i64) >> 11));
            step = 2;
            self.yn2 = self.yn1;
            self.yn1 = val;
            self.cur = self.cur.wrapping_add(1);
            if self.end & 15 == 0 && self.cur == self.end {
                self.cur = self.start.wrapping_add(1);
            } else if self.end & 15 == 1 && self.cur == self.end.wrapping_sub(1) {
                self.cur = self.start;
            } else if self.cur & 15 == 0 {
                // A new 8-byte frame: its first byte is the next predictor and scale.
                self.pred_scale = Self::aram(aram, (self.cur & !15) >> 1) as u16;
                self.cur = self.cur.wrapping_add(2);
                step += 2;
            }
        } else {
            // PCM (also the MMIO modes, which AX does not use): gain, then the same taps.
            let shift = match (self.format >> 4) & 3 {
                0 => 11,
                2 => 16,
                _ => 0,
            };
            let v = ((self.gain as i64 * raw as i64) >> shift) + ((c1 * self.yn1 as i64) >> shift) + ((c2 * self.yn2 as i64) >> shift);
            val = v as i16;
            self.yn2 = self.yn1;
            self.yn1 = val;
            step = 2;
            if (self.format >> 2) & 3 != 1 {
                self.cur = self.cur.wrapping_add(1);
            }
        }
        let mut ended = false;
        if self.cur == self.end.wrapping_add(step - 1) {
            self.cur = self.start;
            self.stopped = true;
            ended = true;
        }
        self.cur &= 0xBFFF_FFFF;
        (val, ended)
    }
}

impl Hw {
    fn ram_words(&mut self, addr: u32, n: usize) -> Vec<u16> {
        self.dma_read(addr & 0x03FF_FFFF, n * 2).chunks_exact(2).map(|b| u16::from_be_bytes([b[0], b[1]])).collect()
    }

    fn ram_i32s(&mut self, addr: u32, n: usize) -> Vec<i32> {
        self.dma_read(addr & 0x03FF_FFFF, n * 4).chunks_exact(4).map(|b| i32::from_be_bytes([b[0], b[1], b[2], b[3]])).collect()
    }

    fn write_i32s(&mut self, addr: u32, v: &[i32]) {
        let bytes: Vec<u8> = v.iter().flat_map(|x| x.to_be_bytes()).collect();
        self.dma_write(addr & 0x03FF_FFFF, &bytes);
    }

    /// AX started (or was switched to).
    pub(crate) fn ax_start(&mut self, crc: u32) {
        self.dsp.hle.ax = Some(AxState { crc, bufs: vec![0; 9 * FRAME], ..Default::default() });
    }

    /// Whether AX has finished a frame and waits for a task mail (0xCDD1xxxx).
    pub(crate) fn ax_waiting_task(&self) -> bool {
        self.dsp.hle.ax.as_ref().is_some_and(|a| a.mail == MailState::WaitTask)
    }

    /// A mail from the CPU while AX runs (task switches and resets are the caller's).
    pub(crate) fn ax_mail(&mut self, mail: u32) {
        let Some(ax) = self.dsp.hle.ax.as_mut() else {
            return;
        };
        match ax.mail {
            MailState::WaitSize => {
                if mail >> 16 == 0xBABE {
                    ax.list_size = mail as u16;
                    ax.mail = MailState::WaitAddress;
                }
            }
            MailState::WaitAddress => {
                let size = ax.list_size as usize;
                ax.mail = MailState::WaitTask;
                let list = if size < 512 { self.ram_words(mail, size) } else { Vec::new() };
                self.ax_command_list(list);
                // Done with this frame: the DSP yields and waits for a task mail.
                self.dsp_mail_irq(0xDCD1_0002);
            }
            MailState::WaitTask => match mail & 0xFFFF {
                // Resume (answered), or continue (the next command list follows at once).
                0 => {
                    ax.mail = MailState::WaitSize;
                    self.dsp_mail_irq(0xDCD1_0001);
                }
                3 => ax.mail = MailState::WaitSize,
                _ => {}
            },
        }
    }

    fn ax_command_list(&mut self, mut list: Vec<u16>) {
        let mut i = 0usize;
        let mut pb_addr = 0u32;
        let mut take = |list: &Vec<u16>, i: &mut usize| -> u16 {
            let v = list.get(*i).copied().unwrap_or(0x000F);
            *i += 1;
            v
        };
        let addr = |list: &Vec<u16>, i: &mut usize, take: &mut dyn FnMut(&Vec<u16>, &mut usize) -> u16| -> u32 {
            let hi = take(list, i) as u32;
            let lo = take(list, i) as u32;
            (hi << 16) | lo
        };
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > 4096 {
                break;
            }
            let cmd = take(&list, &mut i);
            match cmd {
                0x00 => {
                    let a = addr(&list, &mut i, &mut take);
                    self.ax_setup(a);
                }
                0x01 => {
                    let a = addr(&list, &mut i, &mut take);
                    let vols = [take(&list, &mut i), take(&list, &mut i), take(&list, &mut i)];
                    self.ax_download_mix(a, vols);
                }
                0x02 => pb_addr = addr(&list, &mut i, &mut take),
                0x03 => self.ax_process(pb_addr),
                0x04 | 0x05 => {
                    let w = addr(&list, &mut i, &mut take);
                    let r = addr(&list, &mut i, &mut take);
                    self.ax_mix_aux((cmd - 4) as usize, w, r);
                }
                0x06 => {
                    let a = addr(&list, &mut i, &mut take);
                    self.ax_upload_lrs(a);
                }
                0x07 => {
                    let a = addr(&list, &mut i, &mut take);
                    self.ax_set_main_lr(a, false);
                }
                0x08 => i += 10,
                0x09 => {
                    let r = addr(&list, &mut i, &mut take);
                    self.ax_mix_aux(1, 0, r);
                }
                0x0A..=0x0C => {}
                0x0D => {
                    // The list goes on elsewhere.
                    let a = addr(&list, &mut i, &mut take);
                    let n = take(&list, &mut i) as usize;
                    if n < 512 {
                        list = self.ram_words(a, n);
                    } else {
                        list.clear();
                    }
                    i = 0;
                }
                0x0E => {
                    let surround = addr(&list, &mut i, &mut take);
                    let lr = addr(&list, &mut i, &mut take);
                    self.ax_output(lr, surround);
                }
                0x0F => break,
                0x10 => {
                    let up = addr(&list, &mut i, &mut take);
                    let down = addr(&list, &mut i, &mut take);
                    self.ax_mix_auxb_lr(up, down);
                }
                0x11 => {
                    let a = addr(&list, &mut i, &mut take);
                    self.ax_set_main_lr(a, true);
                }
                0x12 => {
                    let threshold = take(&list, &mut i);
                    let frames = take(&list, &mut i);
                    let a = addr(&list, &mut i, &mut take);
                    self.ax_compressor(threshold, frames, a);
                }
                0x13 => {
                    let a: Vec<u32> = (0..6).map(|_| addr(&list, &mut i, &mut take)).collect();
                    self.ax_send_aux_and_mix(&a);
                }
                _ => break,
            }
        }
    }

    fn ax_bufs(&mut self) -> &mut Vec<i32> {
        let ax = self.dsp.hle.ax.as_mut().expect("AX running");
        if ax.bufs.len() != 9 * FRAME {
            ax.bufs = vec![0; 9 * FRAME];
        }
        &mut ax.bufs
    }

    /// SETUP: each bus starts from a value and a per-sample delta given in memory.
    fn ax_setup(&mut self, a: u32) {
        let init = self.ram_words(a, 27);
        let bufs = self.ax_bufs();
        for b in 0..9 {
            let value = (((init[3 * b] as u32) << 16) | init[3 * b + 1] as u32) as i32;
            let delta = init[3 * b + 2] as i16 as i32;
            for (j, s) in bufs[bus(b)].iter_mut().enumerate() {
                *s = if value == 0 { 0 } else { value.wrapping_add(j as i32 * delta) };
            }
        }
    }

    /// One set of left, right and surround samples from memory, added to the main, AUX A and
    /// AUX B buses, each scaled by its own volume.
    fn ax_download_mix(&mut self, a: u32, vols: [u16; 3]) {
        let src = self.ram_i32s(a, 3 * FRAME);
        let bufs = self.ax_bufs();
        for g in 0..3 {
            for c in 0..3 {
                let b = 3 * g + c;
                for k in 0..FRAME {
                    let s = (src[c * FRAME + k] as i64 * vols[g] as i64) >> 15;
                    bufs[b * FRAME + k] = bufs[b * FRAME + k].wrapping_add(s as i32);
                }
            }
        }
    }

    /// AUX A or B: the bus goes to the CPU (for its effects), and the CPU's result from the
    /// previous frame is added to the main bus.
    fn ax_mix_aux(&mut self, aux: usize, write: u32, read: u32) {
        let first = 3 + 3 * aux;
        if write != 0 {
            let out: Vec<i32> = self.ax_bufs()[first * FRAME..(first + 3) * FRAME].to_vec();
            self.write_i32s(write, &out);
        }
        let back = self.ram_i32s(read, 3 * FRAME);
        let bufs = self.ax_bufs();
        for k in 0..3 * FRAME {
            bufs[k] = bufs[k].wrapping_add(back[k]);
        }
    }

    fn ax_upload_lrs(&mut self, a: u32) {
        let out: Vec<i32> = self.ax_bufs()[..3 * FRAME].to_vec();
        self.write_i32s(a, &out);
    }

    /// SET_LR (both sides the same) and SET_OPPOSITE_LR (left negated); surround silent.
    fn ax_set_main_lr(&mut self, a: u32, opposite: bool) {
        let src = self.ram_i32s(a, FRAME);
        let bufs = self.ax_bufs();
        for k in 0..FRAME {
            bufs[k] = if opposite { src[k].wrapping_neg() } else { src[k] };
            bufs[FRAME + k] = src[k];
            bufs[2 * FRAME + k] = 0;
        }
    }

    fn ax_mix_auxb_lr(&mut self, up: u32, down: u32) {
        let out: Vec<i32> = self.ax_bufs()[6 * FRAME..8 * FRAME].to_vec();
        self.write_i32s(up, &out);
        let src = self.ram_i32s(down, 2 * FRAME);
        let bufs = self.ax_bufs();
        for side in 0..2 {
            for k in 0..FRAME {
                let s = src[side * FRAME + k];
                bufs[(6 + side) * FRAME + k] = s;
                bufs[side * FRAME + k] = bufs[side * FRAME + k].wrapping_add(s);
            }
        }
    }

    fn ax_send_aux_and_mix(&mut self, a: &[u32]) {
        let auxa: Vec<i32> = self.ax_bufs()[3 * FRAME..6 * FRAME].to_vec();
        self.write_i32s(a[0], &auxa);
        let auxb_s: Vec<i32> = self.ax_bufs()[8 * FRAME..9 * FRAME].to_vec();
        self.write_i32s(a[1], &auxb_s);
        for (n, b) in [0usize, 1, 6, 7].into_iter().enumerate() {
            let src = self.ram_i32s(a[2 + n], FRAME);
            let bufs = self.ax_bufs();
            for k in 0..FRAME {
                bufs[b * FRAME + k] = bufs[b * FRAME + k].wrapping_add(src[k]);
            }
        }
    }

    /// The limiter: when a frame goes over the threshold, a ramp from the table pulls it down,
    /// then release ramps follow over the next frames.
    fn ax_compressor(&mut self, threshold: u16, release: u16, table: u32) {
        let triggered = {
            let bufs = self.ax_bufs();
            (0..FRAME).any(|k| bufs[k].unsigned_abs() > threshold as u32 || bufs[FRAME + k].unsigned_abs() > threshold as u32)
        };
        let pos = self.dsp.hle.ax.as_ref().map_or(0, |a| a.compressor_pos);
        let frame_bytes = (FRAME * 2) as u32;
        let offset = if triggered {
            self.dsp.hle.ax.as_mut().unwrap().compressor_pos = release;
            pos as u32 * frame_bytes
        } else if pos != 0 {
            let p = pos - 1;
            self.dsp.hle.ax.as_mut().unwrap().compressor_pos = p;
            (11 + p as u32) * frame_bytes
        } else {
            return;
        };
        let ramp = self.ram_words(table.wrapping_add(offset), FRAME);
        let bufs = self.ax_bufs();
        for k in 0..FRAME {
            let c = ramp[k] as i64;
            bufs[k] = ((bufs[k] as i64 * c) >> 15) as i32;
            bufs[FRAME + k] = ((bufs[FRAME + k] as i64 * c) >> 15) as i32;
        }
    }

    /// OUTPUT: main surround as 32-bit words, then main left and right as interleaved 16-bit
    /// samples, right first, clamped.
    fn ax_output(&mut self, lr: u32, surround: u32) {
        let s: Vec<i32> = self.ax_bufs()[2 * FRAME..3 * FRAME].to_vec();
        self.write_i32s(surround, &s);
        let bufs = self.ax_bufs();
        let mut out = Vec::with_capacity(FRAME * 4);
        for k in 0..FRAME {
            out.extend_from_slice(&clamp16(bufs[FRAME + k] as i64).to_be_bytes());
            out.extend_from_slice(&clamp16(bufs[k] as i64).to_be_bytes());
        }
        self.dma_write(lr & 0x03FF_FFFF, &out);
    }

    /// Every voice in the chain, one millisecond at a time.
    fn ax_process(&mut self, mut addr: u32) {
        let crc = self.dsp.hle.ax.as_ref().map_or(0, |a| a.crc);
        let lpf = crc != AX_OLDEST;
        let words = if lpf { pb::WORDS } else { pb::WORDS_NO_LPF };
        let mut seen = 0;
        while addr != 0 && seen < 256 {
            seen += 1;
            let raw = self.ram_words(addr, words);
            // Into the full layout (the oldest AX has no filter words; its loop counter moves).
            let mut p = [0u16; pb::WORDS];
            if lpf {
                p.copy_from_slice(&raw);
            } else {
                p[..pb::LPF_ON].copy_from_slice(&raw[..pb::LPF_ON]);
                p[pb::LOOP_COUNTER] = raw[pb::LPF_ON];
            }
            let ua = ((p[pb::UPDATES_HI] as u32) << 16) | p[pb::UPDATES_LO] as u32;
            let updates = self.ram_words(ua, 64);
            let mut done = 0usize;
            for ms in 0..MS {
                // Parameter changes for this millisecond: (word offset, value) pairs.
                let n = p[pb::UPDATES + ms] as usize;
                if done + n <= 32 {
                    for u in done..done + n {
                        let (off, v) = (updates[2 * u] as usize, updates[2 * u + 1]);
                        if off < pb::WORDS {
                            p[off] = v;
                        }
                    }
                }
                done += n;
                self.ax_voice(&mut p, ms, crc, lpf);
            }
            let back: Vec<u16> = if lpf {
                p.to_vec()
            } else {
                let mut v = p[..pb::LPF_ON].to_vec();
                v.push(p[pb::LOOP_COUNTER]);
                v
            };
            let bytes: Vec<u8> = back.iter().flat_map(|w| w.to_be_bytes()).collect();
            self.dma_write(addr & 0x03FF_FFFF, &bytes);
            addr = ((p[pb::NEXT_HI] as u32) << 16) | p[pb::NEXT_LO] as u32;
        }
    }

    /// One voice for one millisecond.
    fn ax_voice(&mut self, p: &mut [u16; pb::WORDS], ms: usize, crc: u32, lpf: bool) {
        if p[pb::RUNNING] != 1 {
            return;
        }
        let hilo = |p: &[u16; pb::WORDS], o: usize| ((p[o] as u32) << 16) | p[o + 1] as u32;
        let mut coefs = [0i16; 16];
        for (k, c) in coefs.iter_mut().enumerate() {
            *c = p[pb::COEFS + k] as i16;
        }
        let mut acc = Accel {
            start: hilo(p, pb::LOOP_HI) & 0x3FFF_FFFF,
            end: hilo(p, pb::END_HI) & 0x3FFF_FFFF,
            cur: hilo(p, pb::CUR_HI) & 0xBFFF_FFFF,
            format: p[pb::FORMAT],
            yn1: p[pb::YN1] as i16,
            yn2: p[pb::YN2] as i16,
            gain: p[pb::GAIN] as i16,
            pred_scale: p[pb::PRED_SCALE] & 0x7F,
            stopped: false,
        };
        // Samples come through the accelerator; at the end address the voice loops (with the
        // loop's ADPCM state; a stream keeps its history and counts the loop) or stops.
        let next = |acc: &mut Accel, p: &mut [u16; pb::WORDS], aram: &[u8]| -> i16 {
            let (v, ended) = acc.read(aram, &coefs);
            if ended {
                if p[pb::LOOPING] != 0 {
                    acc.pred_scale = p[pb::LOOP_PRED_SCALE] & 0x7F;
                    if p[pb::IS_STREAM] != 1 {
                        acc.yn1 = p[pb::LOOP_YN1] as i16;
                        acc.yn2 = p[pb::LOOP_YN2] as i16;
                    } else {
                        p[pb::LOOP_COUNTER] = p[pb::LOOP_COUNTER].wrapping_add(1);
                    }
                    acc.stopped = false;
                } else {
                    p[pb::RUNNING] = 0;
                }
            }
            v
        };
        let mut samples = [0i16; SPMS];
        let ratio = hilo(p, pb::RATIO_HI);
        let mut pos = p[pb::FRAC] as u32;
        let mut last = [p[pb::LAST] as i16, p[pb::LAST + 1] as i16, p[pb::LAST + 2] as i16, p[pb::LAST + 3] as i16];
        {
            let aram = std::mem::take(&mut self.aram);
            if p[pb::SRC_TYPE] == 2 {
                // No conversion: one input sample per output sample.
                for s in samples.iter_mut() {
                    *s = next(&mut acc, p, &aram);
                }
                last.copy_from_slice(&samples[SPMS - 4..]);
            } else {
                // Linear interpolation (the 4-tap filter needs coefficients from the DSP's
                // ROM, which is not shipped; linear is what it falls back to). The history
                // holds the last four inputs; output interpolates the oldest two.
                let mut hist = last;
                let mut idx = 0usize; // the oldest entry
                for s in samples.iter_mut() {
                    pos = pos.wrapping_add(ratio);
                    while pos >= 0x10000 {
                        hist[idx & 3] = next(&mut acc, p, &aram);
                        idx += 1;
                        pos -= 0x10000;
                    }
                    let frac = pos & 0xFFFF;
                    *s = if frac != 0 {
                        let (s0, s1) = (hist[idx & 3] as i64, hist[(idx + 1) & 3] as i64);
                        ((s0 * (0x10000 - frac) as i64 + s1 * frac as i64) >> 16) as i16
                    } else {
                        hist[idx & 3]
                    };
                }
                for k in 0..4 {
                    last[k] = hist[(idx + k) & 3];
                }
            }
            self.aram = aram;
        }
        p[pb::FRAC] = pos as u16;
        for k in 0..4 {
            p[pb::LAST + k] = last[k] as u16;
        }
        p[pb::CUR_HI] = (acc.cur >> 16) as u16;
        p[pb::CUR_HI + 1] = acc.cur as u16;
        p[pb::YN1] = acc.yn1 as u16;
        p[pb::YN2] = acc.yn2 as u16;
        p[pb::PRED_SCALE] = acc.pred_scale;

        // Volume envelope (signed on the GameCube), then the optional low-pass filter.
        for s in samples.iter_mut() {
            let vol = p[pb::VOL] as i16 as i64;
            *s = clamp16((*s as i64 * vol) >> 15);
            p[pb::VOL] = p[pb::VOL].wrapping_add(p[pb::VOL_DELTA]);
        }
        if lpf && p[pb::LPF_ON] != 0 {
            let (a0, b0) = (p[pb::LPF_A0] as i64, p[pb::LPF_B0] as i16 as i64);
            let mut y = p[pb::LPF_YN1] as i16;
            for s in samples.iter_mut() {
                y = clamp16((a0 * *s as i64 + b0 * y as i64) >> 15);
                *s = y;
            }
            p[pb::LPF_YN1] = y as u16;
        }

        // Onto the buses, each with its volume and, if asked, a per-sample volume ramp.
        let mc = mixer_control(crc, p[pb::MIXER_CONTROL]);
        let bufs = self.ax_bufs();
        for b in 0..9 {
            if mc & (1 << (2 * b)) == 0 {
                continue;
            }
            let ramp = mc & (2 << (2 * b)) != 0;
            let (vo, dp) = (pb::MIX[b], pb::DPOP[b]);
            let mut vol = p[vo];
            let delta = if ramp { p[vo + 1] } else { 0 };
            let out = &mut bufs[b * FRAME + ms * SPMS..b * FRAME + (ms + 1) * SPMS];
            let mut last16 = 0i16;
            for (o, &s) in out.iter_mut().zip(samples.iter()) {
                last16 = clamp16((s as i64 * vol as i64) >> 15);
                *o = o.wrapping_add(last16 as i32);
                vol = vol.wrapping_add(delta);
            }
            p[vo] = vol;
            p[dp] = last16 as u16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PCM16 voice at ratio 1 with no conversion and full volume reaches the main buses
    /// unchanged, and the frame comes out right-then-left in main memory.
    #[test]
    fn a_pcm16_voice_reaches_the_output() {
        let mut hw = Hw::new(false);
        // 200 samples of a ramp in audio RAM at word address 0x1000.
        for k in 0..200u32 {
            let v = (k as i16 * 100).to_be_bytes();
            hw.aram[(0x2000 + 2 * k) as usize] = v[0];
            hw.aram[(0x2000 + 2 * k + 1) as usize] = v[1];
        }
        let mut p = [0u16; pb::WORDS];
        p[pb::SRC_TYPE] = 2;
        p[pb::MIXER_CONTROL] = 0x3; // main L and R
        p[pb::RUNNING] = 1;
        p[pb::MIX[0]] = 0x8000;
        p[pb::MIX[1]] = 0x4000;
        p[pb::VOL] = 0x7FFF;
        p[pb::FORMAT] = 0x0A;
        p[pb::LOOP_HI + 1] = 0x1000;
        p[pb::END_HI + 1] = 0x1000 + 199;
        p[pb::CUR_HI + 1] = 0x1000;
        p[pb::GAIN] = 0x0800;
        let pb_addr = 0x10_0000u32;
        let bytes: Vec<u8> = p.iter().flat_map(|w| w.to_be_bytes()).collect();
        hw.dma_write(pb_addr, &bytes);
        hw.ax_start(0x07F8_8145);
        // Command list: SETUP (zeros), PB_ADDR, PROCESS, OUTPUT, END.
        let zero_init = 0x11_0000u32;
        hw.dma_write(zero_init, &[0u8; 54]);
        let (surround, lr) = (0x12_0000u32, 0x13_0000u32);
        let list: Vec<u16> = vec![0x00, (zero_init >> 16) as u16, zero_init as u16, 0x02, (pb_addr >> 16) as u16, pb_addr as u16, 0x03, 0x0E, (surround >> 16) as u16, surround as u16, (lr >> 16) as u16, lr as u16, 0x0F];
        let lb: Vec<u8> = list.iter().flat_map(|w| w.to_be_bytes()).collect();
        let list_addr = 0x14_0000u32;
        hw.dma_write(list_addr, &lb);
        hw.ax_mail(0xBABE_0000 | list.len() as u32);
        hw.ax_mail(list_addr);
        let out = hw.dma_read(lr, FRAME * 4);
        for k in 0..FRAME {
            let r = i16::from_be_bytes([out[4 * k], out[4 * k + 1]]);
            let l = i16::from_be_bytes([out[4 * k + 2], out[4 * k + 3]]);
            let s = (k as i64 * 100 * 0x7FFF) >> 15;
            let expect_l = ((s * 0x8000) >> 15) as i16;
            let expect_r = ((s * 0x4000) >> 15) as i16;
            assert_eq!((l, r), (expect_l, expect_r), "sample {k}");
        }
        // The voice moved on 160 samples, and the yield mail is waiting.
        let back = hw.dma_read(pb_addr, pb::WORDS * 2);
        let cur = ((u16::from_be_bytes([back[2 * pb::CUR_HI], back[2 * pb::CUR_HI + 1]]) as u32) << 16) | u16::from_be_bytes([back[2 * pb::CUR_HI + 2], back[2 * pb::CUR_HI + 3]]) as u32;
        assert_eq!(cur, 0x1000 + 160);
        assert_eq!(hw.dsp.dsp_mbox | 0x8000_0000, 0xDCD1_0002 | 0x8000_0000);
    }

    /// ADPCM decoding: a frame header sets the scale and predictor, the nibbles are signed
    /// differences, and the next frame's header is read at the frame boundary.
    #[test]
    fn adpcm_frames_decode() {
        let mut aram = vec![0u8; ARAM_SIZE];
        // Frame 0 at byte 0: header 0x00 (predictor 0, scale 1), then nibbles 1,2,...,7,-8,...
        aram[0] = 0x02; // scale 4
        for k in 1..8 {
            aram[k] = 0x12;
        }
        aram[8] = 0x10; // next frame: predictor 1, scale 1
        for k in 9..16 {
            aram[k] = 0x11;
        }
        let mut coefs = [0i16; 16];
        coefs[2] = 0x0800; // predictor 1: y = x + yn1 * 1.0
        let mut a = Accel { start: 2, end: 0x40, cur: 2, format: 0, yn1: 0, yn2: 0, gain: 0, pred_scale: 0x02, stopped: false };
        let mut got = Vec::new();
        for _ in 0..16 {
            got.push(a.read(&aram, &coefs).0);
        }
        // Frame 0: nibbles alternate 1, 2 times 4.
        assert_eq!(&got[..14], &[4, 8, 4, 8, 4, 8, 4, 8, 4, 8, 4, 8, 4, 8]);
        // Frame 1 (predictor 1, scale 2^0): each nibble 1 plus the previous sample.
        assert_eq!(&got[14..], &[9, 10]);
        assert_eq!(a.cur, 16 + 2 + 2);
    }

    #[test]
    fn ector_is_the_rotate_xor() {
        assert_eq!(ector(&[]), 0);
        assert_eq!(ector(&[1]), 8);
        assert_eq!(ector(&[1, 0]), 64);
    }
}
