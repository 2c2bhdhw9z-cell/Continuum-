//! Our stand-ins for the sound programs ("microcodes") that programs upload to the DSP.
//!
//! The DSP is a separate sound processor; games and homebrew upload a program to it through the
//! boot ROM's mail protocol (handled in hw.rs) and then talk to it with mails. Instead of running
//! that program instruction by instruction, we recognise it (by a checksum of the uploaded code)
//! and do what it does directly, in Rust. Each stand-in is written from the program's own public
//! source or documented behaviour, never from another emulator's code.
//!
//! So far:
//! - ASND (the homebrew SDK's "accelerated sound" mixer by Hermes): 16 voices of 8/16-bit
//!   mono/stereo PCM mixed into 1024-frame, 48 kHz blocks. Written from its DSP source
//!   (libasnd/dsp_mixer/dsp_mixer.s).
//! - AESND (the homebrew SDK's newer "accelerated" mixer): voices played through the DSP's
//!   sample accelerator from audio RAM, 96-frame (2 ms) blocks with 16.16 pitch stepping.
//!   Written from its DSP source (libaesnd/dspcode/dspmixer.s).
//! - Unknown programs: they get the "ready" mail and every mail is taken, so programs do not hang.

use crate::hw::Hw;
use serde::{Deserialize, Serialize};

/// Frames in one ASND block (1024 stereo frames at 48 kHz).
const ASND_FRAMES: usize = 1024;

#[derive(Default, Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum Kind {
    #[default]
    None,
    Unknown,
    Asnd,
    Aesnd,
}

/// Frames in one AESND block (96 stereo frames at 48 kHz, 2 ms).
const AESND_FRAMES: usize = 96;

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct HleState {
    pub kind: Kind,
    /// Checksum of the uploaded code (shown in development read-outs).
    pub crc: u32,
    /// A mail announced a value to follow (ASND 0x123: the voice data address).
    expect: Option<u32>,
    /// ASND: main-memory address of the 64-byte voice block.
    asnd_vect: u32,
    /// ASND: the mixing buffer, right then left for each frame.
    asnd_buf: Vec<i16>,
    /// Task switching (0xCDD10001): mails still to take.
    switch_left: u32,
    /// AESND: main-memory address of the voice block, and the mixing buffer (right, left).
    aesnd_pb: u32,
    aesnd_buf: Vec<i16>,
    aesnd_out: u32,
}

/// CRC-32 (IEEE) of a byte slice.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Recognises a sound program by its code: the instructions that set up its command words.
fn identify(code: &[u8]) -> Kind {
    let w: Vec<u16> = code.chunks_exact(2).map(|b| u16::from_be_bytes([b[0], b[1]])).collect();
    let has = |a: u16, b: u16| w.windows(2).any(|p| p[0] == a && p[1] == b);
    // AESND: "lri $acc0.m, #0xface" (its command mails start 0xFACE).
    if has(0x009E, 0xFACE) && has(0x009E, 0xCDD1) {
        return Kind::Aesnd;
    }
    // ASND: "cmpi $acc1.m, #0x111" ... "#0x666" (its command words).
    if has(0x0381, 0x0111) && has(0x0381, 0x0666) && has(0x0381, 0x0123) {
        return Kind::Asnd;
    }
    Kind::Unknown
}

impl Hw {
    /// A sound program was uploaded and started (the boot ROM's 0x80F3D001 mail).
    pub(crate) fn dsp_hle_start(&mut self, maddr: u32, len: u32) {
        let code = self.dma_read(maddr & 0x03FF_FFFF, (len as usize).min(0x2000));
        let crc = crc32(&code);
        let kind = identify(&code);
        self.dsp.hle = HleState { kind, crc, ..Default::default() };
        // Every program starts by saying it is ready (0xDCD10000), with the DSP interrupt.
        self.dsp_mail_irq(0xDCD1_0000);
    }

    /// A mail from the DSP to the CPU with the DSP interrupt, as a running program sends them.
    fn dsp_mail_irq(&mut self, mail: u32) {
        self.dsp_post(mail);
        self.dsp.csr |= 0x80;
    }

    /// A mail from the CPU while a sound program runs.
    pub(crate) fn dsp_hle_mail(&mut self, mail: u32) {
        if self.dsp.hle.switch_left > 0 {
            self.dsp.hle.switch_left -= 1;
            if self.dsp.hle.switch_left == 0 {
                // The next task's code is in place; it starts and says so.
                self.dsp_mail_irq(0xDCD1_0001);
            }
            return;
        }
        if mail >> 16 == 0xCDD1 {
            match mail & 0xFFFF {
                // Task switch: the next task's transfers follow (ASND takes 8 mails, AESND 10).
                0x0001 => self.dsp.hle.switch_left = if self.dsp.hle.kind == Kind::Aesnd { 10 } else { 8 },
                // Back to the boot ROM.
                0x0002 => {
                    self.dsp.ucode = None;
                    self.dsp.hle = HleState::default();
                    self.dsp_post(0x8071_FEED);
                }
                _ => {}
            }
            return;
        }
        match self.dsp.hle.kind {
            Kind::Asnd => self.asnd_mail(mail),
            Kind::Aesnd => self.aesnd_mail(mail),
            _ => {}
        }
    }

    // ------------------------------------------------------------------ ASND

    fn asnd_mail(&mut self, mail: u32) {
        if let Some(cmd) = self.dsp.hle.expect.take() {
            if cmd == 0x123 {
                self.dsp.hle.asnd_vect = mail & 0x03FF_FFFF;
            }
            return;
        }
        if self.dsp.hle.asnd_buf.len() != ASND_FRAMES * 2 {
            self.dsp.hle.asnd_buf = vec![0; ASND_FRAMES * 2];
        }
        match mail & 0xFFFF {
            0x0111 => {
                self.dsp.hle.asnd_buf.iter_mut().for_each(|s| *s = 0);
                self.asnd_voice();
            }
            0x0112 => {
                let out = self.asnd_out_buf();
                let bytes = self.dma_read(out, ASND_FRAMES * 4);
                for (k, s) in self.dsp.hle.asnd_buf.iter_mut().enumerate() {
                    *s = i16::from_be_bytes([bytes[2 * k], bytes[2 * k + 1]]);
                }
                self.asnd_voice();
            }
            0x0123 => self.dsp.hle.expect = Some(0x123),
            0x0222 => self.asnd_voice(),
            0x0666 => {
                let out = self.asnd_out_buf();
                let bytes: Vec<u8> = self.dsp.hle.asnd_buf.iter().flat_map(|s| s.to_be_bytes()).collect();
                self.dma_write(out, &bytes);
                self.dsp_mail_irq(0xDCD1_0004);
            }
            0x0999 => self.dsp_mail_irq(0xDCD1_0003),
            _ => self.dsp_mail_irq(0xDCD1_0004),
        }
    }

    fn asnd_out_buf(&mut self) -> u32 {
        let v = self.dsp.hle.asnd_vect;
        u32::from_be_bytes(self.dma_read(v, 4).try_into().unwrap()) & 0x03FF_FFFF
    }

    /// Mixes one voice into the block, exactly as ASND's mixer does: sample-and-hold
    /// resampling to 48 kHz by a frequency counter, per-voice volume, saturating mix, a start
    /// delay, and a second buffer to chain or loop.
    fn asnd_voice(&mut self) {
        let vect = self.dsp.hle.asnd_vect;
        let mut v = self.dma_read(vect, 64);
        let rd32 = |v: &[u8], o: usize| u32::from_be_bytes([v[o], v[o + 1], v[o + 2], v[o + 3]]);
        let rd16 = |v: &[u8], o: usize| u16::from_be_bytes([v[o], v[o + 1]]);
        let wr32 = |v: &mut [u8], o: usize, x: u32| v[o..o + 4].copy_from_slice(&x.to_be_bytes());
        let wr16 = |v: &mut [u8], o: usize, x: u16| v[o..o + 2].copy_from_slice(&x.to_be_bytes());
        let flags_h = rd16(&v, 8) as u32;
        let flags_l = rd16(&v, 10);
        let kind = flags_l & 7;
        let looped = flags_l & 0x100 != 0;
        let paused = flags_l & 0x200 != 0;
        if !paused {
            let mut addr = rd32(&v, 12);
            let mut end = rd32(&v, 16);
            let freq = rd32(&v, 20);
            // Held samples (already volume-scaled): AXH0 (first, "left") and AXH1 (second).
            let mut h0 = rd16(&v, 26) as i16; // SMP_R slot holds AXH0
            let mut h1 = rd16(&v, 24) as i16; // SMP_L slot holds AXH1
            let mut counter = rd32(&v, 28);
            let mut vol = (rd16(&v, 32) as i16, rd16(&v, 34) as i16);
            let step = flags_h & 0xFFFF;
            // Switches to the second buffer; returns false when there is none.
            let change = |v: &mut Vec<u8>, addr: &mut u32, end: &mut u32, vol: &mut (i16, i16)| -> bool {
                *vol = (rd16(v, 44) as i16, rd16(v, 46) as i16);
                wr16(v, 32, vol.0 as u16);
                wr16(v, 34, vol.1 as u16);
                *end = rd32(v, 40);
                wr32(v, 16, *end);
                *addr = rd32(v, 36);
                wr32(v, 12, *addr);
                wr32(v, 48, *addr);
                if !looped {
                    wr32(v, 36, 0);
                    wr32(v, 40, 0);
                }
                *addr != 0
            };
            let mut ok = true;
            if addr == 0 {
                ok = change(&mut v, &mut addr, &mut end, &mut vol);
            }
            if ok {
                let delay = rd32(&v, 4);
                let frames = if delay != 0 {
                    wr32(&mut v, 4, delay - delay.min(ASND_FRAMES as u32));
                    0
                } else {
                    ASND_FRAMES
                };
                let fetch = |hw: &mut Hw, addr: u32, vol: (i16, i16)| -> (i16, i16) {
                    let word = |hw: &mut Hw, a: u32| -> u16 {
                        let b = hw.dma_read(a & 0x03FF_FFFE, 2);
                        u16::from_be_bytes([b[0], b[1]])
                    };
                    let (a, b) = match kind {
                        0 | 4 => {
                            let w = word(hw, addr);
                            let byte = if addr & 1 == 0 { (w >> 8) as u8 } else { w as u8 };
                            let byte = if kind == 4 { byte ^ 0x80 } else { byte };
                            let s = ((byte as u16) << 8) as i16;
                            (s, s)
                        }
                        1 => {
                            let s = word(hw, addr) as i16;
                            (s, s)
                        }
                        2 | 6 => {
                            let w = word(hw, addr) ^ if kind == 6 { 0x8080 } else { 0 };
                            ((w & 0xFF00) as i16, (w << 8) as i16)
                        }
                        3 => (word(hw, addr) as i16, word(hw, addr + 2) as i16),
                        5 => {
                            let s = word(hw, addr).swap_bytes() as i16;
                            (s, s)
                        }
                        _ => (word(hw, addr).swap_bytes() as i16, word(hw, addr + 2).swap_bytes() as i16),
                    };
                    let scale = |s: i16, vl: i16| (((s as i32) * (vl as i32)) >> 8) as i16;
                    (scale(a, vol.0), scale(b, vol.1))
                };
                for f in 0..frames {
                    let buf = &mut self.dsp.hle.asnd_buf;
                    buf[2 * f] = (buf[2 * f] as i32 + h1 as i32).clamp(-32768, 32767) as i16;
                    buf[2 * f + 1] = (buf[2 * f + 1] as i32 + h0 as i32).clamp(-32768, 32767) as i16;
                    counter = counter.wrapping_add(freq);
                    if counter <= 48000 {
                        continue;
                    }
                    let mut next = addr;
                    if freq <= 48000 {
                        counter -= 48000;
                        next = next.wrapping_add(step);
                    } else {
                        while counter > 48000 {
                            counter -= 48000;
                            next = next.wrapping_add(step);
                        }
                    }
                    if next >= end {
                        // The end of this buffer: on to the second one, or silence.
                        if !change(&mut v, &mut addr, &mut end, &mut vol) {
                            h0 = 0;
                            h1 = 0;
                            continue;
                        }
                    } else {
                        addr = next;
                    }
                    let (a, b) = fetch(self, addr, vol);
                    h0 = a;
                    h1 = b;
                }
                if addr == 0 {
                    change(&mut v, &mut addr, &mut end, &mut vol);
                }
            }
            wr32(&mut v, 12, addr);
            wr32(&mut v, 28, counter);
            wr16(&mut v, 26, h0 as u16);
            wr16(&mut v, 24, h1 as u16);
        }
        self.dma_write(vect, &v);
        self.dsp_mail_irq(0xDCD1_0004);
    }

    // ------------------------------------------------------------------ AESND

    fn aesnd_mail(&mut self, mail: u32) {
        if let Some(cmd) = self.dsp.hle.expect.take() {
            if cmd == 0x80 {
                self.dsp.hle.aesnd_pb = mail & 0x03FF_FFFF;
            }
            return;
        }
        if self.dsp.hle.aesnd_buf.len() != AESND_FRAMES * 2 {
            self.dsp.hle.aesnd_buf = vec![0; AESND_FRAMES * 2];
        }
        if mail >> 16 == 0xDEAD {
            self.dsp_mail_irq(0xDCD1_0003);
            return;
        }
        if mail >> 16 != 0xFACE {
            return;
        }
        match mail & 0xFFFF {
            0x0010 => {
                self.dsp.hle.aesnd_buf.iter_mut().for_each(|s| *s = 0);
                self.aesnd_voice();
            }
            0x0020 => self.aesnd_voice(),
            0x0080 => self.dsp.hle.expect = Some(0x80),
            0x0100 => {
                let out = self.dsp.hle.aesnd_out;
                let bytes: Vec<u8> = self.dsp.hle.aesnd_buf.iter().flat_map(|s| s.to_be_bytes()).collect();
                self.dma_write(out, &bytes);
                self.dsp_mail_irq(0xDCD1_0002);
            }
            0x0200 => self.dsp_mail_irq(0xDCD1_0002),
            _ => {}
        }
    }

    /// One accelerator read (8- or 16-bit PCM from audio RAM; main memory on the Wii), with the
    /// accelerator's wrap from the end address back to the start.
    fn accel_read(&mut self, cur: &mut u32, start: u32, end: u32, wide: bool) -> i16 {
        let v = if self.wii {
            let a = if wide { *cur * 2 } else { *cur };
            let b = self.dma_read(a & 0x1FFF_FFFF, 2);
            if wide {
                i16::from_be_bytes([b[0], b[1]])
            } else {
                ((b[0] as u16) << 8) as i16
            }
        } else if wide {
            let a = (*cur as usize * 2) & (crate::hw::ARAM_SIZE - 1);
            i16::from_be_bytes([self.aram[a], self.aram[(a + 1) & (crate::hw::ARAM_SIZE - 1)]])
        } else {
            ((self.aram[*cur as usize & (crate::hw::ARAM_SIZE - 1)] as u16) << 8) as i16
        };
        *cur = if *cur == end { start } else { cur.wrapping_add(1) };
        v
    }

    /// Mixes one AESND voice into the 96-frame block: held, volume-scaled samples mixed with
    /// saturation, a 16.16 pitch counter pulling new samples through the accelerator, and a
    /// start delay.
    fn aesnd_voice(&mut self) {
        let pb = self.dsp.hle.aesnd_pb;
        let mut v = self.dma_read(pb, 64);
        let rd32 = |v: &[u8], o: usize| u32::from_be_bytes([v[o], v[o + 1], v[o + 2], v[o + 3]]);
        let rd16 = |v: &[u8], o: usize| u16::from_be_bytes([v[o], v[o + 1]]);
        let wr32 = |v: &mut [u8], o: usize, x: u32| v[o..o + 4].copy_from_slice(&x.to_be_bytes());
        let wr16 = |v: &mut [u8], o: usize, x: u16| v[o..o + 2].copy_from_slice(&x.to_be_bytes());
        self.dsp.hle.aesnd_out = rd32(&v, 0) & 0x03FF_FFFF;
        let flags_h = rd16(&v, 40);
        let flags_l = rd16(&v, 42);
        let start = rd32(&v, 4);
        if flags_l & 0x08 == 0 && flags_h & 0x4000 != 0 && start != 0 {
            let end = rd32(&v, 8);
            let mut cur = rd32(&v, 12);
            let freq = ((rd16(&v, 22) as u32) << 16) | rd16(&v, 24) as u32;
            let mut counter = rd16(&v, 26) as u32;
            let mut left = rd16(&v, 28) as i16;
            let mut right = rd16(&v, 30) as i16;
            let (vol_l, vol_r) = (rd16(&v, 32) as i16 as i32, rd16(&v, 34) as i16 as i32);
            let stereo = flags_l & 1 != 0;
            let wide = flags_l & 2 != 0;
            let unsigned = flags_l & 4 != 0;
            let mut first = 0;
            let delay = rd32(&v, 36);
            if delay != 0 {
                let mut d = delay;
                first = AESND_FRAMES;
                for k in 0..AESND_FRAMES {
                    d -= 1;
                    if d == 0 {
                        first = k;
                        break;
                    }
                }
                wr32(&mut v, 36, d);
            }
            let (mut yn1, mut yn2) = (rd16(&v, 16) as i16, rd16(&v, 18) as i16);
            for f in first..AESND_FRAMES {
                let buf = &mut self.dsp.hle.aesnd_buf;
                buf[2 * f] = (buf[2 * f] as i32 + right as i32).clamp(-32768, 32767) as i16;
                buf[2 * f + 1] = (buf[2 * f + 1] as i32 + left as i32).clamp(-32768, 32767) as i16;
                let acc = counter + freq;
                let n = acc >> 16;
                counter = acc & 0xFFFF;
                if n == 0 || n >= 0x8000 {
                    continue;
                }
                let (mut s0, mut s1) = (0i16, 0i16);
                for _ in 0..n {
                    s0 = self.accel_read(&mut cur, start, end, wide);
                    yn2 = yn1;
                    yn1 = s0;
                    if stereo {
                        s1 = self.accel_read(&mut cur, start, end, wide);
                        yn2 = yn1;
                        yn1 = s1;
                    }
                }
                if !stereo {
                    s1 = s0;
                }
                if unsigned {
                    s0 = (s0 as u16 ^ 0x8000) as i16;
                    s1 = (s1 as u16 ^ 0x8000) as i16;
                }
                let scale = |s: i16, vl: i32| ((s as i32 * vl) >> 8).clamp(-32768, 32767) as i16;
                left = scale(s0, vol_l);
                right = scale(s1, vol_r);
            }
            wr32(&mut v, 12, cur);
            wr16(&mut v, 16, yn1 as u16);
            wr16(&mut v, 18, yn2 as u16);
            wr16(&mut v, 26, counter as u16);
            wr16(&mut v, 28, left as u16);
            wr16(&mut v, 30, right as u16);
        }
        // Every processed voice is marked finished for the CPU side.
        let fh = rd16(&v, 40) | 0x0010;
        wr16(&mut v, 40, fh);
        self.dma_write(pb, &v);
        self.dsp_mail_irq(0xDCD1_0004);
    }
}
