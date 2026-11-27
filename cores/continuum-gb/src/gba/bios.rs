//! A high-level BIOS: the SWI calls games use, done in Rust, so no BIOS file is needed. The IRQ
//! path stays low-level (the mini BIOS in `bus.rs` dispatches to the game's handler).
//! IntrWait re-runs its own SWI after each interrupt until the wanted one has come.

use super::bus::Bus;
use super::bus::CPU_HZ;
use super::cpu::{Cpu, MODE_SVC, MODE_SYS};
use super::FRAME_CYCLES;

fn rd32(bus: &Bus, a: u32) -> u32 {
    bus.peek32(a)
}

fn wr32(bus: &mut Bus, a: u32, v: u32) {
    bus.write32(a, v);
}

pub fn swi(cpu: &mut Cpu, bus: &mut Bus, n: u32) {
    bus.bios_latch = 0xE3A0_2004;
    bus.idle(3);
    let r = &mut cpu.r;
    match n {
        0x00 => {
            // SoftReset
            for a in 0x0300_7E00u32..0x0300_8000 {
                bus.write8(a, 0);
            }
            let to_ram = bus.peek8(0x0300_7FFA) != 0;
            let fresh = Cpu::default();
            *cpu = fresh;
            cpu.pc = if to_ram { 0x0200_0000 } else { 0x0800_0000 };
        }
        0x01 => register_ram_reset(bus, r[0]),
        0x02 | 0x03 => {
            // Halt and Stop run inside the BIOS: the processor waits at the BIOS's own wait
            // ($70) for the interrupt, and the handler returns into that wait, not into the
            // game. Once the interrupt has been dealt with the wait returns to the game.
            bus.halt_return = Some(cpu.pc);
            cpu.pc = 0x70;
            bus.halted = true;
        }
        0x04 | 0x05 => {
            // VBlankIntrWait is IntrWait(throw the old flag away, VBlank). "Throw away" must
            // happen on the call itself and never on the re-runs this wait does after each
            // interrupt, or the flag the interrupt just set is thrown away too and the game
            // halts for ever. The wait says which it is in r3.
            let again = r[3] == WAITING;
            if n == 0x05 {
                r[1] = 1;
                if !again {
                    r[0] = 1;
                }
            }
            intr_wait(cpu, bus, again);
        }
        0x06 | 0x07 => {
            // Div and DivArm: one division, with the two registers the other way round.
            let (num, den) = if n == 6 { (r[0] as i32, r[1] as i32) } else { (r[1] as i32, r[0] as i32) };
            // The quotient is truncated toward zero, so the remainder takes the dividend's sign.
            // Dividing by zero divides nothing: the answer is the dividend's sign on its own, the
            // dividend comes back as the remainder, and r3 says 1. A real console sits in the
            // division loop for ever when the dividend is further from zero than that, which no
            // game survives and no high-level call can stand in for, so this is the answer mGBA
            // gives too rather than a hang. INT_MIN / -1 has no answer in 32 bits, and the
            // console leaves INT_MIN there instead of trapping.
            let (q, rem) = if den == 0 {
                (if num < 0 { -1 } else { 1 }, num)
            } else {
                (num.wrapping_div(den), num.wrapping_rem(den))
            };
            r[0] = q as u32;
            r[1] = rem as u32;
            r[3] = q.unsigned_abs();
        }
        0x08 => r[0] = isqrt(r[0]),
        0x09 => {
            // ArcTan: the BIOS's own approximation, and the two values its working leaves in r1
            // and r3 (a real call leaves them there, so a game that reads them back gets these).
            let (v, a, b) = arctan(r[0] as i32);
            r[0] = v as u32;
            r[1] = a as u32;
            r[3] = b as u32;
        }
        0x0A => {
            // ArcTan2: the pair is (x, y), y being the one on the vertical axis, and the answer
            // is built out of ArcTan in whichever quadrant the pair falls in - so it carries the
            // console's own approximation, the angle a game that rotates its objects was written
            // against, rather than a truer arctangent no console ever returned.
            let (v, a) = arctan2(r[0] as i32, r[1] as i32);
            r[0] = v as u32 & 0xFFFF;
            if let Some(a) = a {
                r[1] = a as u32;
            }
            r[3] = 0x170;
        }
        0x0B => cpu_set(bus, r[0], r[1], r[2]),
        0x0C => cpu_fast_set(bus, r[0], r[1], r[2]),
        0x0D => r[0] = 0xBAAE_187F,
        0x0E => bg_affine_set(bus, r[0], r[1], r[2]),
        0x0F => obj_affine_set(bus, r[0], r[1], r[2], r[3]),
        0x10 => bit_unpack(bus, r[0], r[1], r[2]),
        0x11 | 0x12 => {
            let out = lz77(bus, r[0]);
            write_out(bus, r[1], &out, n == 0x12);
        }
        0x13 => {
            let out = huffman(bus, r[0]);
            write_out(bus, r[1], &out, false);
        }
        0x14 | 0x15 => {
            let out = rle(bus, r[0]);
            write_out(bus, r[1], &out, n == 0x15);
        }
        0x16 | 0x17 | 0x18 => {
            let out = diff(bus, r[0], n == 0x18);
            write_out(bus, r[1], &out, n == 0x17);
        }
        0x19 => sound_bias(bus, r[0]),
        0x1A => sound_driver_init(bus, r[0]),
        0x1B => sound_driver_mode(bus, r[0]),
        0x1C => sound_driver_main(bus),
        0x1D => sound_driver_vsync(bus),
        0x1E => sound_channel_clear(bus),
        0x1F => r[0] = midi_key_2_freq(bus, r[0], r[1] as u8, r[2] as u8),
        0x28 => sound_driver_vsync_off(bus),
        0x29 => sound_driver_vsync_on(bus),
        0x2A => sound_get_jump_list(bus, r[0]),
        _ => {}
    }
    let _ = (MODE_SVC, MODE_SYS);
}

/// The BIOS's own sound driver (SWI 0x19, 0x1A-0x1F, 0x28-0x2A), as GBATEK documents it.
///
/// A game using it hands the driver a `SoundArea` in RAM once (`SoundDriverInit`), sets its
/// mode, then every frame calls `SoundDriverVSync` straight after the V-Blank interrupt and
/// `SoundDriverMain` after its background and sprite work. The driver mixes its eight virtual
/// channels into its own mix buffer and pours that buffer through DMA 1 and 2 into the two
/// recorded-sound FIFOs, clocked by Timer 0. This is the older driver, not the game-side
/// MusicPlayer2000 engine (`mp2k.rs`): a game running that engine never calls these SWIs, so
/// none of this touches it.
///
/// What is exact here is the ABI: the `SoundArea`/`SndCh`/`WaveData` layouts, the mode word,
/// `MidiKey2Freq`'s formula and `SoundBias`'s levels. What is this core's own reading of the
/// driver, because the BIOS's mixing code itself is not public, is marked below: the envelope
/// steps, the mix buffer holding one frame a half, the reverb as a one-frame echo, and the
/// jump list (whose entries are addresses inside the BIOS ROM and so have no HLE equivalent).

/// The mode word `SoundDriverInit` leaves behind: reverb off, eight channels at once, master
/// volume 15, playback at 13379 Hz, 8-bit output. (Bits 8-11 = 8, 12-15 = 15, 16-19 = 4,
/// 20-23 = 9.)
pub const SOUND_DRIVER_DEFAULT_MODE: u32 = 0x0094_F800;

/// What `SoundDriverInit` writes into `SoundArea.ident`: the area has been initialized.
/// GBATEK only says the flag is what the system checks; games never read it themselves.
const SOUND_AREA_IDENT: u32 = 1;

/// `SoundArea` offsets (GBATEK).
const SA_DMA_COUNT: u32 = 0x04;
const SA_REVERB: u32 = 0x05;
const SA_VCHN: u32 = 0x14;
const SA_PCMBUF: u32 = 0x194;
/// One virtual channel (`SndCh`) is 48 bytes; the driver has eight.
const SNDCH_COUNT: usize = 8;
const SNDCH_SIZE: u32 = 0x30;
/// `SndCh` offsets: the documented fields, then the driver's own scratch in the fields GBATEK
/// marks "user access prohibited" (r2[0] the envelope stage, r2[1] the last `sf` seen, r3[0]
/// the sample position in 16.16 fixed point, r3[1] the envelope volume).
const CH_SF: u32 = 0x00;
const CH_RV: u32 = 0x02;
const CH_LV: u32 = 0x03;
const CH_AT: u32 = 0x04;
const CH_DE: u32 = 0x05;
const CH_SU: u32 = 0x06;
const CH_RE: u32 = 0x07;
const CH_STAGE: u32 = 0x08;
const CH_LAST_SF: u32 = 0x09;
const CH_FR: u32 = 0x0C;
const CH_WP: u32 = 0x10;
const CH_POS: u32 = 0x14;
const CH_ENV: u32 = 0x18;
/// `sf` bits: the channel is playing, and it has been keyed off (release).
const CH_ON: u8 = 0x80;
const CH_KEY_OFF: u8 = 0x40;
/// Envelope stages, in `CH_STAGE`.
const ENV_ATTACK: u8 = 0;
const ENV_DECAY: u8 = 1;
const ENV_SUSTAIN: u8 = 2;
const ENV_RELEASE: u8 = 3;
/// `WaveData` offsets (GBATEK).
const W_STAT: u32 = 0x02;
const W_FREQ: u32 = 0x04;
const W_LOOP: u32 = 0x08;
const W_SIZE: u32 = 0x0C;
const W_DATA: u32 = 0x10;
/// `WaveData.stat`: a forward loop (0x4000) rather than a one-shot (0x0000).
const W_LOOPS: u16 = 0x4000;
/// Samples in each half of the driver's mix buffer. At the default 13379 Hz one frame holds
/// 13379 / 59.7275 = 224 samples, so a half is one frame: the first half is the right mix
/// (DMA 1, FIFO A) and the second half the left mix (DMA 2, FIFO B). `SoundDriverMain`
/// mixes both halves fresh every frame, and `SoundDriverVSync` rewinds both DMAs to the
/// start of their half: a DMA walks forward through its half as the frame goes on, so
/// without that rewind it would walk on past the buffer.
const PCM_BF: usize = 224;
/// Playback rates for `SoundDriverMode` bits 16-19, values 1-12 (GBATEK).
const PLAYBACK_RATES: [u32; 12] =
    [5734, 7884, 10512, 13379, 15768, 18157, 21024, 26758, 31536, 36314, 40137, 42048];

/// A byte in RAM, written the way `register_ram_reset` writes RAM: straight into the work
/// memory, or through the bus anywhere else (the SoundArea lives in work RAM in practice).
fn mem8(bus: &mut Bus, a: u32, v: u8) {
    match a >> 24 {
        0x02 => bus.ewram[(a & 0x3FFFF) as usize] = v,
        0x03 => bus.iwram[(a & 0x7FFF) as usize] = v,
        _ => bus.write8(a, v),
    }
}

fn mem32(bus: &mut Bus, a: u32, v: u32) {
    for i in 0..4 {
        mem8(bus, a + i, (v >> (i * 8)) as u8);
    }
}

fn mem_zero(bus: &mut Bus, a: u32, n: u32) {
    for i in 0..n {
        mem8(bus, a + i, 0);
    }
}

/// SWI 0x1F: the frequency a channel plays a waveform at, for a MIDI key and a fine adjustment
/// (256 = one semitone). The waveform's own `freq` is its sampling rate raised to its original
/// key (`sampling rate x 2^((180-original key)/12)`), so playing it at another key is that
/// value raised by the difference in keys. The BIOS reads the word at `wa+4` itself, which is
/// also why the function can read BIOS bytes when `wa` points into the BIOS; this core's mini
/// BIOS is not the real one, so that trick only works with a real BIOS loaded.
fn midi_key_2_freq(bus: &Bus, wa: u32, mk: u8, fp: u8) -> u32 {
    let freq = bus.peek32(wa.wrapping_add(W_FREQ));
    if freq == 0 {
        return 0;
    }
    let semis = (f64::from(mk) + f64::from(fp) / 256.0 - 180.0) / 12.0;
    (f64::from(freq) * 2f64.powf(semis)) as u32
}

/// SWI 0x19: the SOUNDBIAS level. 0 means level 0, anything else level 0x200; the upper bits
/// of the register (the amplitude, bits 14-15) are kept. The real call steps there with short
/// delays; the level it lands on is what matters, so this sets it straight away.
fn sound_bias(bus: &mut Bus, level: u32) {
    let cur = bus.peek16(0x0400_0088);
    let next = (cur & 0xFC00) | if level == 0 { 0 } else { 0x200 };
    bus.write16(0x0400_0088, next as u16);
}

/// SWI 0x1A: start the driver on a work area. The area is zeroed (channels, mix buffer and
/// all) and marked initialized; the mode goes back to its defaults and the DMA is left
/// stopped until the first `SoundDriverMain`/`SoundDriverVSync`. Call once, at start-up.
fn sound_driver_init(bus: &mut Bus, area: u32) {
    if area == 0 {
        return;
    }
    mem_zero(bus, area, SA_PCMBUF + (PCM_BF * 2) as u32);
    mem32(bus, area, SOUND_AREA_IDENT);
    bus.sound_area = area;
    bus.sound_mode = SOUND_DRIVER_DEFAULT_MODE;
    bus.sound_vsync_on = true;
}

/// SWI 0x1B: the driver's mode word (GBATEK). Bit 7 set applies bits 0-6 as the reverb value;
/// bits 8-11 the channels at once, 12-15 the master volume, 16-19 the playback rate,
/// 20-23 the output bits.
fn sound_driver_mode(bus: &mut Bus, mode: u32) {
    bus.sound_mode = mode;
    if mode & 0x80 != 0 && bus.sound_area != 0 {
        mem8(bus, bus.sound_area + SA_REVERB, (mode & 0x7F) as u8);
    }
}

fn sound_rate(mode: u32) -> u32 {
    match (mode >> 16) & 0xF {
        1..=12 => PLAYBACK_RATES[((mode >> 16) & 0xF) as usize - 1],
        _ => 13_379,
    }
}

fn sound_max_channels(mode: u32) -> usize {
    match ((mode >> 8) & 0xF) as usize {
        0 => SNDCH_COUNT,
        n => n.min(SNDCH_COUNT),
    }
}

fn sound_master(mode: u32) -> i32 {
    match ((mode >> 12) & 0xF) as i32 {
        0 => 15,
        v => v,
    }
}

fn sound_dac_bits(mode: u32) -> u32 {
    match (mode >> 20) & 0xF {
        8..=11 => 17 - ((mode >> 20) & 0xF),
        _ => 8,
    }
}

/// Whether DMA channel 1 or 2 is switched on (its control register's enable bit).
fn sound_dma_on(bus: &Bus, ch: usize) -> bool {
    bus.peek16(0x0400_00BA + (ch as u32 - 1) * 12 + 12) & 0x8000 != 0
}

/// Point DMA 1 and 2 at the start of their half of the mix buffer (DMA 1 reads the first
/// half into FIFO A, DMA 2 the second into FIFO B) and switch them on, repeating, in words,
/// as the FIFOs ask for more. Each channel is switched off first so the new source address
/// is picked up: that rewind is what `SoundDriverVSync` does every frame, because a DMA
/// walks forward through its half as the frame goes on.
fn sound_dma_point(bus: &mut Bus) {
    let base = bus.sound_area + SA_PCMBUF;
    for (ch, fifo) in [(1u32, 0x0400_00A0u32), (2, 0x0400_00A4)] {
        let reg = 0x0400_00B0 + ch * 12;
        let src = base + (ch - 1) * PCM_BF as u32;
        bus.write16(reg, src as u16);
        bus.write16(reg + 2, (src >> 16) as u16);
        bus.write16(reg + 4, fifo as u16);
        bus.write16(reg + 6, (fifo >> 16) as u16);
        bus.write16(reg + 8, 0); // count: a FIFO transfer always moves four words
        bus.write16(reg + 10, 0); // off, so the new source is picked up...
        bus.write16(reg + 10, 0xB600); // ...on, repeating, words, as the sound asks
    }
}

/// Start the driver's sound the first time: both recorded-sound channels on, Timer 0
/// overflowing at the playback rate to clock the FIFOs, then DMA on both halves.
fn sound_dma_start(bus: &mut Bus) {
    // SOUNDCNT_H the way the music engine sets it up (DMA A and B on, from Timer 0), and
    // the sound circuit on, before any FIFO write: writes into a FIFO are ignored while it
    // is off.
    bus.write16(0x0400_0082, 0x210C);
    bus.write16(0x0400_0084, 0x0080);
    let rate = sound_rate(bus.sound_mode);
    let reload = 0x1_0000u32.saturating_sub((CPU_HZ / u64::from(rate.max(1))) as u32);
    bus.write16(0x0400_0100, reload as u16);
    bus.write16(0x0400_0102, 0x0080);
    sound_dma_point(bus);
}

/// SWI 0x1C: mix one frame of the driver's channels into both halves of the mix buffer
/// (right into the first, left into the second). Call every 1/60 of a second, after
/// `SoundDriverVSync`.
fn sound_driver_main(bus: &mut Bus) {
    let area = bus.sound_area;
    if area == 0 || bus.peek32(area) != SOUND_AREA_IDENT {
        return;
    }
    let mode = bus.sound_mode;
    let rate = sound_rate(mode);
    let n = ((u64::from(rate) * u64::from(FRAME_CYCLES) / CPU_HZ) as usize).min(PCM_BF);
    let mut right = vec![0i32; n];
    let mut left = vec![0i32; n];
    for ch in 0..sound_max_channels(mode) {
        mix_channel(bus, area + SA_VCHN + SNDCH_SIZE * ch as u32, rate, n, &mut right, &mut left);
    }
    // Each half still holds last frame's mix for its own side: the reverb is an echo of
    // that, fed back by the driver's reverb value when the mode asked for reverb. The
    // previous byte is read before the new one is written over it.
    let reverb = if mode & 0x80 != 0 { bus.peek8(area + SA_REVERB) as i32 } else { 0 };
    let master = sound_master(mode);
    let dac = sound_dac_bits(mode);
    let mask = if dac < 8 { 0xFFu8 << (8 - dac) } else { 0xFF };
    for (side, acc) in [(0usize, &right[..]), (1, &left[..])] {
        for i in 0..n {
            let mut out = (acc[i] * master / 15).clamp(-128, 127);
            if reverb > 0 {
                let at = area + SA_PCMBUF + (side * PCM_BF + i) as u32;
                let prev = bus.peek8(at) as i8 as i32;
                out = (out + (reverb * prev >> 7)).clamp(-128, 127);
            }
            let byte = ((out as u8) & mask) as i8 as u8;
            mem8(bus, area + SA_PCMBUF + (side * PCM_BF + i) as u32, byte);
        }
    }
    // Nothing is feeding the FIFOs yet: start the DMA on both halves. Once it is running,
    // `SoundDriverVSync` owns rewinding it; this leaves it alone.
    if bus.sound_vsync_on && !sound_dma_on(bus, 1) && !sound_dma_on(bus, 2) {
        sound_dma_start(bus);
    }
}

/// One virtual channel's frame: its envelope steps once (all the envelope times are per
/// 1/60 of a second), then its waveform is stepped through at its frequency and added to
/// both sides of the mix, by its right and left volumes.
fn mix_channel(bus: &mut Bus, ch: u32, rate: u32, n: usize, right: &mut [i32], left: &mut [i32]) {
    let sf = bus.peek8(ch + CH_SF) as u8;
    if sf & CH_ON == 0 {
        mem8(bus, ch + CH_LAST_SF, 0);
        return;
    }
    let last_sf = bus.peek8(ch + CH_LAST_SF) as u8;
    let (mut stage, mut vol): (u8, u32) = if last_sf & CH_ON == 0 {
        // A note just started: volume from zero, sample from the top.
        mem32(bus, ch + CH_POS, 0);
        (ENV_ATTACK, 0u32)
    } else {
        (bus.peek8(ch + CH_STAGE) as u8, bus.peek32(ch + CH_ENV))
    };
    mem8(bus, ch + CH_LAST_SF, sf);
    if sf & CH_KEY_OFF != 0 {
        stage = ENV_RELEASE;
    }
    let (at, de, su, re) = (
        bus.peek8(ch + CH_AT) as u32,
        bus.peek8(ch + CH_DE) as u32,
        bus.peek8(ch + CH_SU) as u32,
        bus.peek8(ch + CH_RE) as u32,
    );
    match stage {
        ENV_ATTACK => {
            // No attack time means straight to full volume; otherwise up by `at` a frame.
            vol = if at == 0 { 255 } else { (vol + at).min(255) };
            if vol >= 255 {
                vol = 255;
                stage = ENV_DECAY;
            }
        }
        ENV_DECAY => {
            vol = vol * de / 256;
            if vol <= su {
                vol = su;
                stage = ENV_SUSTAIN;
            }
        }
        ENV_SUSTAIN => vol = su,
        _ => {
            vol = vol * re / 256;
            if vol == 0 {
                mem8(bus, ch + CH_SF, 0);
                mem8(bus, ch + CH_LAST_SF, 0);
                return;
            }
            stage = ENV_RELEASE;
        }
    }
    mem8(bus, ch + CH_STAGE, stage);
    mem32(bus, ch + CH_ENV, vol);
    let wp = bus.peek32(ch + CH_WP);
    let fr = bus.peek32(ch + CH_FR);
    if wp == 0 || fr == 0 {
        return;
    }
    let (rv, lv) = (bus.peek8(ch + CH_RV) as i32, bus.peek8(ch + CH_LV) as i32);
    let loops = bus.peek16(wp + W_STAT) & u32::from(W_LOOPS) != 0;
    let size = bus.peek32(wp + W_SIZE);
    let loop_at = bus.peek32(wp + W_LOOP);
    if size == 0 {
        return;
    }
    // `fr` is the sample rate this note plays at, in Hz; one output sample moves the
    // position by fr/rate samples, kept in 16.16 fixed point.
    let step = ((u64::from(fr) << 16) / u64::from(rate.max(1))) as u32;
    let mut pos = bus.peek32(ch + CH_POS);
    for i in 0..n {
        let mut idx = pos >> 16;
        if idx >= size {
            if loops && loop_at < size {
                pos = pos.wrapping_sub((size - loop_at) << 16);
                idx = pos >> 16;
            }
            if idx >= size {
                mem8(bus, ch + CH_SF, 0);
                mem8(bus, ch + CH_LAST_SF, 0);
                break;
            }
        }
        let sample = bus.peek8(wp + W_DATA + idx) as i8 as i32;
        right[i] += sample * vol as i32 * rv / 65536;
        left[i] += sample * vol as i32 * lv / 65536;
        pos = pos.wrapping_add(step);
    }
    mem32(bus, ch + CH_POS, pos);
}

/// SWI 0x1D: reset the sound DMA, straight after the V-Blank interrupt. Both DMAs are
/// rewound to the start of their half of the freshly mixed buffer, and the driver's
/// transfer count moves on. With V-Blank DMA stopped (SWI 0x28) this does nothing.
fn sound_driver_vsync(bus: &mut Bus) {
    let area = bus.sound_area;
    if area == 0 || !bus.sound_vsync_on || bus.peek32(area) != SOUND_AREA_IDENT {
        return;
    }
    let count = bus.peek8(area + SA_DMA_COUNT) as u8;
    mem8(bus, area + SA_DMA_COUNT, count.wrapping_add(1));
    sound_dma_point(bus);
}

/// SWI 0x1E: clear every direct sound channel and stop the sound.
fn sound_channel_clear(bus: &mut Bus) {
    let area = bus.sound_area;
    if area == 0 {
        return;
    }
    for ch in 0..SNDCH_COUNT {
        mem_zero(bus, area + SA_VCHN + SNDCH_SIZE * ch as u32, SNDCH_SIZE);
    }
    mem_zero(bus, area + SA_PCMBUF, (PCM_BF * 2) as u32);
    for ch in [1, 2] {
        bus.write16(0x0400_00BA + (ch as u32 - 1) * 12 + 12, 0);
    }
    // Empty the FIFOs as well, then put the routing back the way the driver leaves it.
    bus.write16(0x0400_0082, 0x8800);
    bus.write16(0x0400_0082, 0x210C);
}

/// SWI 0x28: stop the sound DMA (for when V-Blank interrupts stop and `SoundDriverVSync`
/// cannot be called every 1/60 of a second: the DMA would run past the buffer otherwise).
fn sound_driver_vsync_off(bus: &mut Bus) {
    bus.sound_vsync_on = false;
    for ch in [1, 2] {
        bus.write16(0x0400_00BA + (ch as u32 - 1) * 12 + 12, 0);
    }
}

/// SWI 0x29: restart the sound DMA stopped by `SoundDriverVSyncOff`.
fn sound_driver_vsync_on(bus: &mut Bus) {
    bus.sound_vsync_on = true;
    if bus.sound_area != 0 && bus.peek32(bus.sound_area) == SOUND_AREA_IDENT {
        sound_dma_start(bus);
    }
}

/// SWI 0x2A: fill a 0x120-byte buffer with the 36 extra sound functions' entry points. Those
/// are addresses inside the BIOS ROM, so they have no HLE equivalent: the buffer is zeroed,
/// which a caller can tell apart from a real BIOS's table, rather than being handed
/// addresses that point nowhere.
fn sound_get_jump_list(bus: &mut Bus, dst: u32) {
    if dst == 0 {
        return;
    }
    mem_zero(bus, dst, 0x120);
}

/// Left in r3 while a wait is halted, so the same wait running again knows it is not a new call.
/// A real BIOS leaves rubbish in r0, r1 and r3 after any of its calls (only r2 and r4 upwards
/// are promised back), so no game can mind what is in there, and the register travels with save
/// states and run-ahead the way the rest of the machine does.
const WAITING: u32 = 0xCB07_7A17;

fn intr_wait(cpu: &mut Cpu, bus: &mut Bus, again: bool) {
    // The BIOS's own copy of acknowledged interrupts lives at $03007FF8 (IWRAM end - 8).
    bus.io[0x208] = 1; // IME
    let flags = bus.peek16(0x0300_7FF8) as u16;
    let want = cpu.r[1] as u16;
    if !again && cpu.r[0] != 0 {
        // Discard old flags on the call itself, then wait for a new one.
        bus.write16(0x0300_7FF8, flags & !want);
    } else if flags & want != 0 {
        bus.write16(0x0300_7FF8, flags & !want);
        cpu.r[3] = 0;
        return;
    }
    // Not yet: halt and run this SWI again after the interrupt returns.
    cpu.r[0] = 0;
    cpu.r[3] = WAITING;
    bus.halted = true;
    cpu.pc = cpu.pc.wrapping_sub(if cpu.thumb() { 2 } else { 4 });
    cpu.cpsr &= !0x80;
}

/// The whole part of the square root of `x`, by the bit-by-bit method the console's own Sqrt
/// uses: it settles two bits of the answer at a time, so the answer can never come out one too
/// high for a value just under a square (which a floating-point square root has to be trusted
/// not to do). The console's Sqrt is unsigned: a negative word is a huge one.
fn isqrt(mut x: u32) -> u32 {
    let mut res = 0;
    let mut bit = 1u32 << 30;
    while bit > x {
        bit >>= 2;
    }
    while bit != 0 {
        if x >= res + bit {
            x -= res + bit;
            res = (res >> 1) + bit;
        } else {
            res >>= 1;
        }
        bit >>= 2;
    }
    res
}

/// ArcTan: the angle of a tangent in a turn of 65536, worked out from the polynomial a real
/// console's ArcTan uses. It returns the angle, the polynomial's own term and the coefficient it
/// ended on, which are what the call leaves in r1 and r3 - a game may read them back, however
/// little they promise. Every step is 32 bits on the console, so a huge argument wraps there
/// rather than stopping the machine.
fn arctan(x: i32) -> (i32, i32, i32) {
    let a = (x.wrapping_mul(x) >> 14).wrapping_neg();
    let mut b = (0xA9i32.wrapping_mul(a) >> 14).wrapping_add(0x390);
    for k in [0x91C, 0xFB6, 0x16AA, 0x2081, 0x3651, 0xA2F9] {
        b = (b.wrapping_mul(a) >> 14).wrapping_add(k);
    }
    (x.wrapping_mul(b) >> 16, a, b)
}

/// ArcTan2: the angle of the pair (x, y), y being the one on the vertical axis, in the same
/// turn of 65536. The console reduces the pair to ArcTan of the smaller magnitude over the
/// larger and adds the quadrant's own quarter turn, so on an axis the answer is exact and
/// anywhere else it is the approximation a real console gives. The second value is what the
/// polynomial left in r1; on an axis the console never runs it, so nothing is handed back.
fn arctan2(x: i32, y: i32) -> (i32, Option<i32>) {
    if y == 0 {
        return (if x >= 0 { 0 } else { 0x8000 }, None);
    }
    if x == 0 {
        return (if y >= 0 { 0x4000 } else { 0xC000 }, None);
    }
    let at = |r: i32| {
        let (v, a, _) = arctan(r);
        (v, a)
    };
    // The larger magnitude goes underneath after both are shifted into 32 bits, as the console
    // does, so a huge pair wraps rather than stopping the machine.
    let ratio = |up: i32, down: i32| up.wrapping_shl(14).wrapping_div(down);
    // The sign of the ratio carries which side of an axis the pair is on, so the quadrant is
    // added afterwards rather than folded into the ratio.
    let (v, a) = if y >= 0 {
        if x >= 0 {
            if x >= y {
                at(ratio(y, x))
            } else {
                let (v, a) = at(ratio(x, y));
                (0x4000i32.wrapping_sub(v), a)
            }
        } else if x.wrapping_neg() >= y {
            let (v, a) = at(ratio(y, x));
            (v.wrapping_add(0x8000), a)
        } else {
            let (v, a) = at(ratio(x, y));
            (0x4000i32.wrapping_sub(v), a)
        }
    } else if x <= 0 {
        if x.wrapping_neg() > y.wrapping_neg() {
            let (v, a) = at(ratio(y, x));
            (v.wrapping_add(0x8000), a)
        } else {
            let (v, a) = at(ratio(x, y));
            (0xC000i32.wrapping_sub(v), a)
        }
    } else if x >= y.wrapping_neg() {
        let (v, a) = at(ratio(y, x));
        (v.wrapping_add(0x1_0000), a)
    } else {
        let (v, a) = at(ratio(x, y));
        (0xC000i32.wrapping_sub(v), a)
    };
    (v, Some(a))
}

fn register_ram_reset(bus: &mut Bus, f: u32) {
    if f & 1 != 0 { bus.ewram.iter_mut().for_each(|b| *b = 0) }
    if f & 2 != 0 { bus.iwram[..0x7E00].iter_mut().for_each(|b| *b = 0) }
    if f & 4 != 0 { bus.pal.iter_mut().for_each(|b| *b = 0) }
    if f & 8 != 0 { bus.vram.iter_mut().for_each(|b| *b = 0) }
    if f & 16 != 0 { bus.oam.iter_mut().for_each(|b| *b = 0) }
    if f & 0x80 != 0 {
        for a in (0x0400_0000u32..0x0400_0060).step_by(2) {
            bus.write16(a, 0);
        }
    }
}

fn cpu_set(bus: &mut Bus, src: u32, dst: u32, ctl: u32) {
    let count = ctl & 0x1F_FFFF;
    let fixed = ctl & (1 << 24) != 0;
    let word = ctl & (1 << 26) != 0;
    let step = if word { 4 } else { 2 };
    let (mut s, mut d) = (src, dst);
    for _ in 0..count {
        if word {
            let v = bus.read32(s & !3);
            bus.write32(d & !3, v);
        } else {
            let v = bus.read16(s & !1);
            bus.write16(d & !1, v as u16);
        }
        if !fixed {
            s = s.wrapping_add(step);
        }
        d = d.wrapping_add(step);
    }
}

fn cpu_fast_set(bus: &mut Bus, src: u32, dst: u32, ctl: u32) {
    let count = (ctl & 0x1F_FFFF).div_ceil(8) * 8;
    let fixed = ctl & (1 << 24) != 0;
    let (mut s, mut d) = (src & !3, dst & !3);
    let fill = bus.read32(s);
    for _ in 0..count {
        let v = if fixed { fill } else { bus.read32(s) };
        bus.write32(d, v);
        s = s.wrapping_add(4);
        d = d.wrapping_add(4);
    }
}

fn sin_cos(angle: u32) -> (i32, i32) {
    let t = f64::from((angle >> 8) & 0xFF) * std::f64::consts::TAU / 256.0;
    ((t.sin() * 16384.0) as i32, (t.cos() * 16384.0) as i32)
}

fn bg_affine_set(bus: &mut Bus, mut src: u32, mut dst: u32, n: u32) {
    for _ in 0..n {
        let ox = rd32(bus, src) as i32;
        let oy = rd32(bus, src + 4) as i32;
        let dx = i32::from(bus.peek16(src + 8) as i16);
        let dy = i32::from(bus.peek16(src + 10) as i16);
        let sx = i32::from(bus.peek16(src + 12) as i16);
        let sy = i32::from(bus.peek16(src + 14) as i16);
        let (sin, cos) = sin_cos(bus.peek16(src + 16));
        let pa = (sx * cos) >> 14;
        let pb = -(sx * sin) >> 14;
        let pc = (sy * sin) >> 14;
        let pd = (sy * cos) >> 14;
        bus.write16(dst, pa as u16);
        bus.write16(dst + 2, pb as u16);
        bus.write16(dst + 4, pc as u16);
        bus.write16(dst + 6, pd as u16);
        wr32(bus, dst + 8, (ox - (pa * dx + pb * dy)) as u32);
        wr32(bus, dst + 12, (oy - (pc * dx + pd * dy)) as u32);
        src += 20;
        dst += 16;
    }
}

fn obj_affine_set(bus: &mut Bus, mut src: u32, mut dst: u32, n: u32, stride: u32) {
    for _ in 0..n {
        let sx = i32::from(bus.peek16(src) as i16);
        let sy = i32::from(bus.peek16(src + 2) as i16);
        let (sin, cos) = sin_cos(bus.peek16(src + 4));
        let vals = [(sx * cos) >> 14, -(sx * sin) >> 14, (sy * sin) >> 14, (sy * cos) >> 14];
        for v in vals {
            bus.write16(dst, v as u16);
            dst = dst.wrapping_add(stride);
        }
        src += 8;
    }
}

fn bit_unpack(bus: &mut Bus, src: u32, dst: u32, info: u32) {
    let len = bus.peek16(info);
    let sw = bus.peek8(info + 2);
    let dw = bus.peek8(info + 3);
    let off = rd32(bus, info + 4);
    let zero = off & (1 << 31) != 0;
    let off = off & 0x7FFF_FFFF;
    let (mut out, mut bits, mut d) = (0u32, 0u32, dst);
    for i in 0..len {
        let byte = bus.peek8(src + i);
        let mut b = 0;
        while b < 8 {
            let v = (byte >> b) & ((1 << sw) - 1);
            let v = if v != 0 || zero { v + off } else { v };
            out |= (v & ((1u64 << dw) - 1) as u32) << bits;
            bits += dw;
            if bits >= 32 {
                wr32(bus, d, out);
                d += 4;
                out = 0;
                bits = 0;
            }
            b += sw;
        }
    }
}

fn write_out(bus: &mut Bus, dst: u32, out: &[u8], halfwords: bool) {
    if halfwords {
        for (i, pair) in out.chunks(2).enumerate() {
            let v = u16::from(pair[0]) | u16::from(*pair.get(1).unwrap_or(&0)) << 8;
            bus.write16(dst + i as u32 * 2, v);
        }
    } else {
        for (i, b) in out.iter().enumerate() {
            bus.write8(dst + i as u32, *b);
        }
    }
}

/// LZ77 unpacking. Badly made data can ask for a run that carries on past the size it declared;
/// a real console writes those bytes anyway, past the end of the space the game set aside, while
/// VisualBoy Advance stopped at the declared size. Some old fan-made games were built against
/// that, so `bus.vba_compat` (a core option, on by default) stops there too.
fn lz77(bus: &Bus, src: u32) -> Vec<u8> {
    let size = (rd32(bus, src) >> 8) as usize;
    let keep_inside = bus.vba_compat;
    let mut out = Vec::with_capacity(size);
    let mut s = src + 4;
    while out.len() < size {
        let flags = bus.peek8(s) as u8;
        s += 1;
        for bit in (0..8).rev() {
            if out.len() >= size {
                break;
            }
            if flags & (1 << bit) != 0 {
                let b0 = bus.peek8(s) as usize;
                let b1 = bus.peek8(s + 1) as usize;
                s += 2;
                let disp = ((b0 & 0xF) << 8 | b1) + 1;
                let len = (b0 >> 4) + 3;
                for _ in 0..len {
                    if keep_inside && out.len() >= size {
                        break;
                    }
                    let v = if disp <= out.len() { out[out.len() - disp] } else { 0 };
                    out.push(v);
                }
            } else {
                out.push(bus.peek8(s) as u8);
                s += 1;
            }
        }
    }
    if keep_inside {
        out.truncate(size);
    }
    out
}

fn rle(bus: &Bus, src: u32) -> Vec<u8> {
    let size = (rd32(bus, src) >> 8) as usize;
    let mut out = Vec::with_capacity(size);
    let mut s = src + 4;
    while out.len() < size {
        let f = bus.peek8(s) as usize;
        s += 1;
        if f & 0x80 != 0 {
            let v = bus.peek8(s) as u8;
            s += 1;
            for _ in 0..(f & 0x7F) + 3 {
                out.push(v);
            }
        } else {
            for _ in 0..(f & 0x7F) + 1 {
                out.push(bus.peek8(s) as u8);
                s += 1;
            }
        }
    }
    out.truncate(size);
    out
}

fn huffman(bus: &Bus, src: u32) -> Vec<u8> {
    let header = rd32(bus, src);
    let bits = header & 0xF;
    let size = (header >> 8) as usize;
    let tree_size = bus.peek8(src + 4);
    let root = src + 5;
    let mut data = src + 4 + (tree_size + 1) * 2;
    let mut out = Vec::with_capacity(size);
    let mut acc = 0u32;
    let mut acc_bits = 0;
    let mut node = root;
    let mut node_val = bus.peek8(node);
    while out.len() < size {
        let word = rd32(bus, data);
        data += 4;
        for i in (0..32).rev() {
            let dir = (word >> i) & 1;
            let child = (node & !1) + (node_val & 0x3F) * 2 + 2 + dir;
            let leaf = node_val & (0x80 >> dir) != 0;
            if leaf {
                let v = bus.peek8(child);
                acc |= (v & ((1 << bits) - 1)) << acc_bits;
                acc_bits += bits;
                if acc_bits == 32 {
                    out.extend_from_slice(&acc.to_le_bytes());
                    acc = 0;
                    acc_bits = 0;
                }
                node = root;
            } else {
                node = child;
            }
            node_val = bus.peek8(node);
            if out.len() >= size {
                break;
            }
        }
    }
    out.truncate(size);
    out
}

fn diff(bus: &Bus, src: u32, sixteen: bool) -> Vec<u8> {
    let size = (rd32(bus, src) >> 8) as usize;
    let mut out = Vec::with_capacity(size);
    if sixteen {
        let mut prev = 0u16;
        for i in (0..size).step_by(2) {
            prev = prev.wrapping_add(bus.peek16(src + 4 + i as u32) as u16);
            out.extend_from_slice(&prev.to_le_bytes());
        }
    } else {
        let mut prev = 0u8;
        for i in 0..size {
            prev = prev.wrapping_add(bus.peek8(src + 4 + i as u32) as u8);
            out.push(prev);
        }
    }
    out
}
