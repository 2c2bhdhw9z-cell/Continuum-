//! Nintendo's own music engine, the one most Advance games use.
//!
//! It is called MusicPlayer2000 (or m4a, or Sappy) and it is software, not a chip: the game mixes
//! its own instruments together at a rate the little console could manage (usually 13379 samples
//! a second, eight bits each, and the sums are allowed to wrap round rather than stop at the
//! ends), and pours the result into the Advance's two recorded-sound channels. That is why Advance
//! music hisses and crunches even on a perfect emulator: the hiss is in the game, not in the
//! emulation.
//!
//! With the "Advance music quality" setting on High, this reads the engine's own working notes out
//! of the console's memory every frame - which instrument each of its twelve voices is playing,
//! how far through the sound it is, how loud each side is - and plays those same voices again at
//! the phone's full rate, with the steps between samples filled in. Same notes, same tune, same
//! loudness, none of the grit.
//!
//! Nothing is guessed. Every frame the engine's notes have to prove themselves (its marker word,
//! and each playing voice's sample has to be a real sample sitting where it says it is) and the
//! phone only takes over a recorded-sound channel that the engine's own transfer is really
//! feeding. The moment anything does not add up, this steps aside for that frame and the game is
//! heard exactly as the console made it.
//!
//! The engine's layout is public knowledge: the field order is Nintendo's, published in the
//! `pret` disassembly of the Pokemon games (`include/gba/m4a_internal.h` and `src/m4a_1.s`), and
//! the way to spot the engine in a cartridge is Bregalad and loveemu's, from the open-source
//! `sappy_detector` in GBA Mus Ripper.

use super::bus::Bus;

/// The word the engine leaves at the start of its working notes: 'Smsh' backwards, after the
/// people who wrote the engine's tool.
const MAGIC: u32 = 0x6873_6D53;

/// Where the engine leaves the address of its working notes. The console's own start-up code
/// reserves this spot for exactly that (GBATEK calls it the pointer to the sound buffer).
const NOTES_PTR: u32 = 0x0300_7FF0;

// The engine's working notes ("SoundInfo"), as laid out in m4a_internal.h.
const SI_IDENT: u32 = 0x00;
const SI_ECHO: u32 = 0x05;
const SI_MAX_VOICES: u32 = 0x06;
const SI_RATE: u32 = 0x14;
const SI_VOICES: u32 = 0x50;
const SI_MIX: u32 = 0x350;

/// Each of the two halves of the engine's finished mix, in samples.
const MIX_HALF: u32 = 1584;

/// Voices the engine can play at once ("MAX_DIRECTSOUND_CHANNELS").
const VOICES: usize = 12;
const VOICE_SIZE: u32 = 0x40;

// One voice ("SoundChannel").
const V_STATUS: u32 = 0x00;
const V_TYPE: u32 = 0x01;
const V_VOL_RIGHT: u32 = 0x0A;
const V_VOL_LEFT: u32 = 0x0B;
const V_RATE: u32 = 0x20;
const V_SAMPLE: u32 = 0x24;
const V_AT: u32 = 0x28;

/// This voice is doing something (start, stop, echo or any envelope stage: "SOUND_CHANNEL_SF_ON").
const ON: u8 = 0xC7;
/// Set while a voice is only just being started, before the engine has worked out where it plays
/// from, so there is nothing to copy yet.
const STARTING: u8 = 0x80;
/// This voice goes back round to the sample's loop point instead of stopping.
const LOOPS: u8 = 0x10;
/// The sound plays at the engine's own rate whatever note it was given.
const FIXED_RATE: u8 = 0x08;

// A sample ("WaveData"): sixteen bytes, then the eight-bit sound itself.
const W_KIND: u32 = 0x00;
const W_RATE: u32 = 0x04;
const W_LOOP_AT: u32 = 0x08;
const W_LENGTH: u32 = 0x0C;
const W_SOUND: u32 = 0x10;

/// Sample rates worth believing, in samples a second. Advance samples live between about 5 and
/// 32 thousand, and a note three octaves up from one of those still fits.
const RATE_RANGE: std::ops::RangeInclusive<u32> = 1_000..=100_000;

/// The first instruction of the engine's "start this song number" routine, which is the same in
/// every cartridge that carries the engine. There are two, because the engine was built afresh
/// part way through the Advance's life. From Bregalad and loveemu's `sappy_detector`.
const START_SONG: [[u8; 30]; 2] = [
    [
        0x00, 0xB5, 0x00, 0x04, 0x07, 0x4A, 0x08, 0x49, 0x40, 0x0B, 0x40, 0x18, 0x83, 0x88, 0x59,
        0x00, 0xC9, 0x18, 0x89, 0x00, 0x89, 0x18, 0x0A, 0x68, 0x01, 0x68, 0x10, 0x1C, 0x00, 0xF0,
    ],
    [
        0x00, 0xB5, 0x00, 0x04, 0x07, 0x4B, 0x08, 0x49, 0x40, 0x0B, 0x40, 0x18, 0x82, 0x88, 0x51,
        0x00, 0x89, 0x18, 0x89, 0x00, 0xC9, 0x18, 0x0A, 0x68, 0x01, 0x68, 0x10, 0x1C, 0x00, 0xF0,
    ],
];

/// The routine keeps the address of the song list forty bytes in.
const SONG_LIST_AT: usize = 40;

/// Reads a cartridge address as the cartridge itself would.
fn rom32(rom: &[u8], at: usize) -> Option<u32> {
    let b = rom.get(at..at + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// A cartridge address (the Advance sees the cartridge at $08000000 and again at $09000000), and
/// where it lands in the file.
fn in_rom(rom: &[u8], addr: u32) -> Option<usize> {
    if (addr >> 24) & 0xFE != 8 {
        return None;
    }
    let at = (addr & 0x01FF_FFFF) as usize;
    if at < rom.len() { Some(at) } else { None }
}

/// Where this cartridge keeps its list of songs, if its music is this engine's. `None` means the
/// game does its music some other way, and nothing below ever runs.
///
/// Finding the engine's "start this song number" routine is only the first half: the address it
/// carries has to really hold a list of songs, and one of those songs has to point at a set of
/// instruments that reads like one, with a real sample behind it. A cartridge that merely happens
/// to contain those thirty bytes somewhere in its graphics cannot pass all three.
pub fn song_list(rom: &[u8]) -> Option<u32> {
    // The routine opens with `push {lr}`, which is two bytes and sits on an even address, so the
    // cartridge is walked two bytes at a time looking for those and only then for the rest. A
    // cartridge can be 32 MB and this runs when a game is opened, so it has to be cheap.
    let len = START_SONG[0].len();
    for (i, pair) in rom.chunks_exact(2).enumerate() {
        if pair != [0x00, 0xB5] {
            continue;
        }
        let at = i * 2;
        if at + len > rom.len() {
            break;
        }
        if START_SONG.iter().any(|s| rom[at..at + len] == *s) {
            if let Some(list) = song_list_at(rom, at) {
                return Some(list);
            }
        }
    }
    None
}

fn song_list_at(rom: &[u8], routine: usize) -> Option<u32> {
    let list = rom32(rom, routine + SONG_LIST_AT)?;
    let start = in_rom(rom, list)?;
    // Each entry is the song's address and two numbers. An empty slot is allowed; a slot holding
    // something that is not a cartridge address ends the list.
    for i in 0..1024 {
        let Some(song) = rom32(rom, start + i * 8) else { break };
        if song == 0 {
            continue;
        }
        let Some(header) = in_rom(rom, song) else { break };
        if instruments_look_right(rom, header) {
            return Some(list);
        }
    }
    None
}

/// Does this song's set of instruments read like one? Each instrument is twelve bytes: what kind
/// it is, which note it is tuned to, then the sample it plays and how it fades in and out. A
/// recorded instrument has to point at a real sample for this to be the engine's work.
fn instruments_look_right(rom: &[u8], song_header: usize) -> bool {
    let tracks = *rom.get(song_header).unwrap_or(&0);
    if !(1..=16).contains(&tracks) {
        return false;
    }
    let Some(set) = rom32(rom, song_header + 4).and_then(|a| in_rom(rom, a)) else { return false };
    // Instruments are numbered the way a keyboard is, up to 128 of them. Many of the early ones
    // are the console's own beeps, or unused, so one good recorded instrument is enough.
    (0..128).any(|i| {
        let at = set + i * 12;
        let kind = *rom.get(at).unwrap_or(&0xFF);
        if kind != 0 && kind != FIXED_RATE {
            return false; // a beep, a drum kit or the end of the set: keep looking
        }
        rom32(rom, at + 4)
            .and_then(|a| in_rom(rom, a))
            .is_some_and(|w| sample_looks_right(rom, w))
    })
}

fn sample_looks_right(rom: &[u8], at: usize) -> bool {
    let (Some(kind), Some(rate), Some(length)) =
        (rom32(rom, at), rom32(rom, at + W_RATE as usize), rom32(rom, at + W_LENGTH as usize))
    else {
        return false;
    };
    // The top of the first word says whether the sound loops; the rest of it has to be zero,
    // which means eight-bit sound stored as it is.
    kind & !0x4000_0000 == 0
        && RATE_RANGE.contains(&(rate / 1024))
        && length > 0
        && at + W_SOUND as usize + length as usize <= rom.len()
}

/// What the engine's notes say about one voice: which sample, how far through it, how fast, and
/// whether it goes back round.
struct Said {
    sample: u32,
    at: u32,
    rate: u32,
    loops: bool,
}

/// One of the engine's twelve voices, ready to play at the phone's rate.
#[derive(Clone, Default)]
struct Voice {
    /// The sound this voice plays next, with its loops already followed through, so playing it is
    /// a walk along an array and can no longer go wrong.
    sound: Vec<i8>,
    /// How far along `sound` we are, and how far one output sample moves us: both 32.32.
    at: u64,
    step: u64,
    /// How loud this voice is on the right and on the left, as the engine worked it out.
    vol: [i32; 2],
    /// Which sample is playing, and where in it the engine was last time, so a note carrying on
    /// is told from a new one.
    sample: u32,
    was_at: u32,
    playing: bool,
}

/// The engine's mix, made again at the phone's rate.
#[derive(Clone, Default)]
pub struct Mp2k {
    /// The player asked for the high-quality mix ("Advance music quality").
    pub on: bool,
    /// Where this cartridge keeps its songs, when its music is this engine's.
    pub song_list: Option<u32>,
    voices: Vec<Voice>,
    /// Which half of the engine's finished mix feeds each of the two recorded-sound channels
    /// (the first half is the right one on a real Advance), or `None` where the game is feeding
    /// that channel itself and must be left alone.
    half_of: [Option<usize>; 2],
    /// The two halves, for the sample being handed over now.
    now: [i32; 2],
    /// There is a mix to hand over.
    pub live: bool,
}

impl Mp2k {
    pub fn new(rom: &[u8]) -> Mp2k {
        Mp2k { song_list: song_list(rom), ..Default::default() }
    }

    /// Forgets where every voice had got to, keeping the game's own settings. Used when a saved
    /// game is loaded: the next frame reads the engine's notes again from scratch.
    pub fn restart(&mut self) {
        self.voices.clear();
        self.half_of = [None, None];
        self.now = [0, 0];
        self.live = false;
    }

    /// What the engine would have put into recorded-sound channel `n` for this sample, or `None`
    /// to leave that channel as the console made it.
    pub fn lane(&self, n: usize) -> Option<i32> {
        self.half_of[n].map(|h| self.now[h])
    }

    /// Works out the next output sample for both halves of the mix.
    pub fn step(&mut self) {
        let mut mix = [0i64; 2];
        for v in &mut self.voices {
            if !v.playing {
                continue;
            }
            let i = (v.at >> 32) as usize;
            if i + 1 >= v.sound.len() {
                // Past what was copied out this frame. The next frame reads the engine again, so
                // leave this voice out until then rather than reading off the end of the sound.
                v.playing = false;
                continue;
            }
            // Between two samples, instead of standing on the nearer one: this is where the hiss
            // goes.
            let along = i64::from((v.at >> 16) as u16);
            let s = i64::from(v.sound[i]) * (0x1_0000 - along) + i64::from(v.sound[i + 1]) * along;
            mix[0] += s * i64::from(v.vol[0]);
            mix[1] += s * i64::from(v.vol[1]);
            v.at += v.step;
        }
        // The engine divides each voice by 256 as it adds it up, and keeps eight bits, so its sums
        // wrap round when a tune is loud. This keeps the whole sum and stops at the ends instead,
        // which is the quieter, cleaner half of what High does.
        self.now = [
            (mix[0] >> 24).clamp(-128, 127) as i32,
            (mix[1] >> 24).clamp(-128, 127) as i32,
        ];
    }

    /// Reads the engine's working notes, once a frame, and gets every playing voice ready to be
    /// played again at `out_hz`. Anything that does not add up leaves the game sounding exactly as
    /// the console made it until the next frame.
    pub fn sync(&mut self, bus: &Bus, out_hz: u32) {
        if !self.on || self.song_list.is_none() {
            self.restart();
            return;
        }
        let Some(notes) = read32(bus, NOTES_PTR).and_then(|p| ram(bus, p, SI_MIX + MIX_HALF * 2).map(|_| p))
        else {
            self.restart();
            return;
        };
        if read32(bus, notes + SI_IDENT) != Some(MAGIC) {
            // The engine is part way through its own work (it changes this word while it is
            // busy). Nothing to read safely; carry on with what the last frame said.
            return;
        }
        if !self.read_voices(bus, notes, out_hz) {
            self.live = false;
            return;
        }
        self.half_of = mix_halves(bus, notes + SI_MIX);
        self.live = self.half_of.iter().any(Option::is_some);
    }

    /// True when every playing voice was read and is ready.
    fn read_voices(&mut self, bus: &Bus, notes: u32, out_hz: u32) -> bool {
        let Some(most) = read8(bus, notes + SI_MAX_VOICES) else { return false };
        let Some(engine_rate) = read32(bus, notes + SI_RATE) else { return false };
        if !(1..=VOICES).contains(&(most as usize)) || !RATE_RANGE.contains(&engine_rate) {
            return false;
        }
        // The engine can fold an echo of its own mix back into itself. This does not, so a game
        // that asks for one keeps the console's mix, echo and all, rather than losing it.
        if read8(bus, notes + SI_ECHO) != Some(0) {
            return false;
        }
        self.voices.resize(VOICES, Voice::default());
        for i in 0..VOICES {
            let base = notes + SI_VOICES + VOICE_SIZE * i as u32;
            let (Some(status), Some(kind)) = (read8(bus, base + V_STATUS), read8(bus, base + V_TYPE))
            else {
                return false;
            };
            if status & ON == 0 || status & STARTING != 0 {
                self.voices[i].playing = false;
                continue;
            }
            // Only plain eight-bit sound, played straight. The later engine can also squeeze a
            // sample down or play it backwards; those are left to the console's own mix.
            if kind & !FIXED_RATE != 0 {
                return false;
            }
            let rate = if kind & FIXED_RATE != 0 { engine_rate } else { read32(bus, base + V_RATE).unwrap_or(0) };
            if !RATE_RANGE.contains(&rate) {
                return false;
            }
            let (Some(sample), Some(at), Some(right), Some(left)) = (
                read32(bus, base + V_SAMPLE),
                read32(bus, base + V_AT),
                read8(bus, base + V_VOL_RIGHT),
                read8(bus, base + V_VOL_LEFT),
            ) else {
                return false;
            };
            let said = Said { sample, at, rate, loops: status & LOOPS != 0 };
            if !self.load_voice(bus, i, said, out_hz) {
                return false;
            }
            self.voices[i].vol = [i32::from(right), i32::from(left)];
        }
        true
    }

    /// Copies out the sound one voice will need before the next frame, loops followed through, and
    /// keeps it playing from where it already was so a held note does not jump.
    fn load_voice(&mut self, bus: &Bus, i: usize, said: Said, out_hz: u32) -> bool {
        let Said { sample, at, rate, loops } = said;
        let Some(head) = ram(bus, sample, W_SOUND) else { return false };
        let word = |o: u32| {
            let b = &head[o as usize..o as usize + 4];
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        };
        let (kind, loop_at, length) = (word(W_KIND), word(W_LOOP_AT), word(W_LENGTH));
        // As stored, eight bits a sample, and the only other thing the first word may say is that
        // it loops. Its own rate has to make sense as well.
        if kind & !0x4000_0000 != 0 || !RATE_RANGE.contains(&(word(W_RATE) / 1024)) || length == 0 {
            return false;
        }
        if loops && loop_at >= length {
            return false;
        }
        let Some(sound) = ram(bus, sample + W_SOUND, length) else { return false };
        // Where the engine had got to has to be inside that sound. Nothing else proves this is
        // really the engine's own table rather than something that happens to look like it.
        let Some(from) = at.checked_sub(sample + W_SOUND).filter(|f| *f <= length) else { return false };

        // A frame of sound at this rate, with half a frame spare; one frame is 59.7275 of a
        // second. Capped, because a note played very high up would ask for a great deal.
        let want = ((u64::from(rate) * 3 / 2 / 60) + 8).min(2048) as usize;
        let v = &mut self.voices[i];
        v.sound.clear();
        let mut n = from;
        while v.sound.len() < want {
            if n >= length {
                if !loops {
                    break;
                }
                n = loop_at;
            }
            v.sound.push(sound[n as usize] as i8);
            n += 1;
        }
        v.step = (u64::from(rate) << 32) / u64::from(out_hz.max(1));
        // The same note still going: keep its place, less however far the engine itself moved on,
        // so there is no jump at the frame's edge. A new note, or a place that has drifted more
        // than a few samples from the engine's, starts where the engine says.
        let moved = if v.playing && v.sample == sample { from.checked_sub(v.was_at) } else { None };
        v.at = match moved.map(|m| v.at as i64 - (i64::from(m) << 32)) {
            // A sample behind (which is only the rounding of the step) up to a few ahead: that is
            // a note playing on, and keeping its place is what stops a buzz at every frame's edge.
            Some(d) if (-(1i64 << 32)..(4i64 << 32)).contains(&d) => d.max(0) as u64,
            _ => 0,
        };
        v.sample = sample;
        v.was_at = from;
        v.playing = true;
        true
    }
}

/// Which recorded-sound channel each half of the engine's mix is being poured into. Only a channel
/// whose own transfer really is reading the engine's mix is taken over; a game feeding one itself
/// (a spoken line, say) is left alone.
fn mix_halves(bus: &Bus, mix: u32) -> [Option<usize>; 2] {
    let mut out = [None, None];
    for (n, lane) in out.iter_mut().enumerate() {
        // The engine pours its first half through transfer 1 and its second through transfer 2.
        let base = 0xB0 + (n + 1) * 12;
        let reg = |o: usize| u32::from(bus.io[base + o]) | u32::from(bus.io[base + o + 1]) << 8;
        let control = reg(10);
        let to = reg(4) | reg(6) << 16;
        let from = reg(0) | reg(2) << 16;
        let fifo = 0x0400_00A0 + 4 * n as u32;
        if control & 0x8000 == 0 || (control >> 12) & 3 != 3 || to != fifo {
            continue;
        }
        let along = from.wrapping_sub(mix);
        if along < MIX_HALF * 2 {
            *lane = Some((along / MIX_HALF) as usize);
        }
    }
    out
}

/// The console's memory, where the engine's notes and its sounds may live: the two work memories
/// and the cartridge. Nothing else is read, so no address out of the game can send this anywhere
/// it should not go.
fn ram(bus: &Bus, addr: u32, len: u32) -> Option<&[u8]> {
    let (mem, at) = match addr >> 24 {
        0x02 => (&bus.ewram, addr & 0x3FFFF),
        0x03 => (&bus.iwram, addr & 0x7FFF),
        0x08..=0x0D => (&bus.rom, addr & 0x01FF_FFFF),
        _ => return None,
    };
    let at = at as usize;
    mem.get(at..at.checked_add(len as usize)?)
}

fn read8(bus: &Bus, addr: u32) -> Option<u8> {
    ram(bus, addr, 1).map(|b| b[0])
}

fn read32(bus: &Bus, addr: u32) -> Option<u32> {
    ram(bus, addr, 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROM_BASE: u32 = 0x0800_0000;
    const IWRAM_BASE: u32 = 0x0300_0000;
    /// Where the test puts the engine's working notes in the console's fast memory.
    const NOTES: u32 = IWRAM_BASE + 0x1000;
    const SAMPLE_RATE: u32 = 8_000;
    const OUT_HZ: u32 = 48_000;

    fn put32(m: &mut [u8], at: usize, v: u32) {
        m[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// A cartridge carrying the engine: its "start this song number" routine, a song list, one
    /// song, a set of instruments, and a real sample for the first recorded instrument.
    ///
    /// The sample is a sawtooth, so every one of its bytes differs from the last: a test cannot
    /// pass by reading the wrong byte, or by standing still.
    fn engine_rom(build: usize) -> Vec<u8> {
        let mut rom = vec![0u8; 0x8000];
        rom[..4].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes()); // the game: sit still
        rom[0xB2] = 0x96; // the byte that marks an Advance cartridge
        let routine = 0x1000;
        rom[routine..routine + 30].copy_from_slice(&START_SONG[build]);
        let (list, song, set, sample) = (0x2000usize, 0x3000usize, 0x4000usize, 0x5000usize);
        put32(&mut rom, routine + SONG_LIST_AT, ROM_BASE + list as u32);
        put32(&mut rom, list, ROM_BASE + song as u32);
        rom[song] = 4; // four tracks
        put32(&mut rom, song + 4, ROM_BASE + set as u32);
        // The first instrument is one of the console's own beeps, so the reader has to walk past
        // it to the recorded one behind it, the way a real set of instruments is laid out.
        rom[set] = 1;
        rom[set + 12] = 0;
        put32(&mut rom, set + 12 + 4, ROM_BASE + sample as u32);
        put32(&mut rom, sample + W_RATE as usize, SAMPLE_RATE * 1024);
        put32(&mut rom, sample + W_LENGTH as usize, 256);
        for i in 0..256 {
            rom[sample + W_SOUND as usize + i] = (i as i32 - 128) as u8;
        }
        rom
    }

    /// A machine with that cartridge, the engine's notes written into memory by hand as the engine
    /// would have left them, and its two transfers pouring the mix into the sound channels.
    fn machine(voices: &[(u32, u32, u8, u8)]) -> super::super::Gba {
        let mut gba = super::super::Gba::new(engine_rom(0));
        gba.bus.mp2k.on = true;
        let w32 = |bus: &mut Bus, addr: u32, v: u32| {
            let at = (addr & 0x7FFF) as usize;
            bus.iwram[at..at + 4].copy_from_slice(&v.to_le_bytes());
        };
        w32(&mut gba.bus, NOTES + SI_IDENT, MAGIC);
        gba.bus.iwram[((NOTES + SI_MAX_VOICES) & 0x7FFF) as usize] = 8;
        w32(&mut gba.bus, NOTES + SI_RATE, 13_379);
        for (i, &(rate, at, right, left)) in voices.iter().enumerate() {
            let base = NOTES + SI_VOICES + VOICE_SIZE * i as u32;
            let b = |bus: &mut Bus, o: u32, v: u8| bus.iwram[((base + o) & 0x7FFF) as usize] = v;
            b(&mut gba.bus, V_STATUS, 0x03); // playing, in its envelope, not looping
            b(&mut gba.bus, V_TYPE, 0);
            b(&mut gba.bus, V_VOL_RIGHT, right);
            b(&mut gba.bus, V_VOL_LEFT, left);
            w32(&mut gba.bus, base + V_RATE, rate);
            w32(&mut gba.bus, base + V_SAMPLE, ROM_BASE + 0x5000);
            w32(&mut gba.bus, base + V_AT, ROM_BASE + 0x5000 + W_SOUND + at);
        }
        // The engine's address for its notes, where the console keeps it.
        w32(&mut gba.bus, NOTES_PTR, NOTES);
        gba.bus.apu.out_hz = OUT_HZ;
        // Transfer 1 pours the first half of the mix into channel A, transfer 2 the second into B.
        for n in 0..2u32 {
            let base = 0xB0 + (n + 1) * 12;
            let mix = NOTES + SI_MIX + MIX_HALF * n;
            let set = |bus: &mut Bus, o: u32, v: u32| {
                bus.io[(base + o) as usize] = v as u8;
                bus.io[(base + o + 1) as usize] = (v >> 8) as u8;
            };
            set(&mut gba.bus, 0, mix & 0xFFFF);
            set(&mut gba.bus, 2, mix >> 16);
            set(&mut gba.bus, 4, (0x0400_00A0 + 4 * n) & 0xFFFF);
            set(&mut gba.bus, 6, (0x0400_00A0 + 4 * n) >> 16);
            set(&mut gba.bus, 10, 0xB600); // on, repeating, words, poured as the sound asks for it
        }
        gba
    }

    /// The sample byte this test's sawtooth holds at `n`.
    fn saw(n: u32) -> i64 {
        i64::from(n as i32 - 128)
    }

    /// One frame of the machine's finished sound, as the app would be handed it. `engine` off is
    /// a cartridge whose music is not this engine's.
    fn sound_of_a_frame(setting_high: bool, engine: bool) -> Vec<i16> {
        let mut gba = machine(&[(SAMPLE_RATE, 0, 200, 100)]);
        gba.bus.mp2k.on = setting_high;
        if !engine {
            gba.bus.mp2k.song_list = None;
        }
        // The two recorded-sound channels on at full, the way the engine sets them up: the first
        // on the right, the second on the left.
        gba.bus.write16(0x0400_0082, 0x210C);
        // Two frames in, so the once-a-frame reading of the engine has happened, then listen to
        // the frame after it.
        for _ in 0..2 {
            gba.run_frame();
        }
        gba.bus.apu.samples.clear();
        gba.run_frame();
        std::mem::take(&mut gba.bus.apu.samples)
    }

    #[test]
    fn the_setting_changes_the_music_and_nothing_else() {
        let as_the_console_made_it = sound_of_a_frame(false, true);
        let high = sound_of_a_frame(true, true);
        let other_music = sound_of_a_frame(true, false);

        assert!(!as_the_console_made_it.is_empty(), "a frame of sound must have come out");
        assert_eq!(high.len(), as_the_console_made_it.len(), "the same amount of sound either way");
        // Nothing else in this machine makes a sound, and the game's engine is not really running,
        // so with the setting left alone there is nothing to hear...
        assert!(as_the_console_made_it.iter().all(|s| *s == 0), "nothing but the music is touched");
        // ...and on High the voice the engine's notes describe is heard instead.
        assert!(high.iter().any(|s| *s != 0), "the music must come through");
        // Louder on the right than the left, as that voice's own volumes say: the two halves of
        // the engine's mix must not be crossed over.
        let loudest = |s: &[i16], side: usize| s.iter().skip(side).step_by(2).map(|v| v.abs()).max().unwrap_or(0);
        let (l, r) = (loudest(&high, 0), loudest(&high, 1));
        assert!(r > l && l > 0, "right {r} must be the louder of the two, and both heard");
        // A cartridge whose music is not this engine's is left exactly as the console made it,
        // even with the setting on High.
        assert_eq!(other_music, as_the_console_made_it, "other music must be untouched");
    }

    #[test]
    fn the_engine_is_spotted_in_a_cartridge_and_nowhere_else() {
        for build in 0..2 {
            assert_eq!(song_list(&engine_rom(build)), Some(ROM_BASE + 0x2000), "build {build}");
        }
        // A game that does its music some other way.
        assert_eq!(song_list(&vec![0u8; 0x8000]), None);
        // The thirty bytes sitting in a cartridge that has no song list behind them: graphics can
        // say anything, so the address they carry has to hold real songs.
        let mut lookalike = engine_rom(0);
        put32(&mut lookalike, 0x1000 + SONG_LIST_AT, 0x0200_0000);
        assert_eq!(song_list(&lookalike), None, "an address that is not in the cartridge");
        let mut empty = engine_rom(0);
        put32(&mut empty, 0x2000, 0x0800_7FFF);
        assert_eq!(song_list(&empty), None, "a song list pointing at nothing like a song");
        let mut no_sample = engine_rom(0);
        put32(&mut no_sample, 0x5000 + W_LENGTH as usize, 0x0100_0000);
        assert_eq!(song_list(&no_sample), None, "an instrument whose sample does not fit");
        let mut squeezed = engine_rom(0);
        put32(&mut squeezed, 0x5000, 1);
        assert_eq!(song_list(&squeezed), None, "a sample squeezed down, which this cannot play");
    }

    #[test]
    fn a_voice_is_played_again_at_the_phones_rate() {
        // One voice, at 8000 samples a second, starting at the top of the sample, louder on the
        // right than the left.
        let mut gba = machine(&[(SAMPLE_RATE, 0, 200, 100)]);
        gba.bus.mp2k_sync();
        assert!(gba.bus.mp2k.live, "the engine's notes must have been read");
        // Six samples of the sawtooth, stepped through six at a time because 48000 is six times
        // 8000: every output sample lands between two of the sample's own, and each one is worked
        // out by hand here from the sawtooth.
        let step = (u64::from(SAMPLE_RATE) << 32) / u64::from(OUT_HZ);
        let mut at = 0u64;
        for n in 0..36 {
            gba.bus.mp2k.step();
            let i = (at >> 32) as u32;
            let along = i64::from((at >> 16) as u16);
            let s = saw(i) * (0x1_0000 - along) + saw(i + 1) * along;
            for (lane, vol) in [(0usize, 200i64), (1, 100)] {
                let want = ((s * vol) >> 24).clamp(-128, 127) as i32;
                assert_eq!(gba.bus.mp2k.lane(lane), Some(want), "sample {n}, channel {lane}");
            }
            at += step;
        }
    }

    #[test]
    fn a_voice_goes_round_its_loop_and_a_plain_one_stops() {
        // A sample that loops back to byte 200 of 256, read from byte 250: six bytes left, then
        // round again. The engine's own loop flag is what says so.
        let mut gba = machine(&[(SAMPLE_RATE, 250, 255, 255)]);
        let base = NOTES + SI_VOICES;
        gba.bus.iwram[((base + V_STATUS) & 0x7FFF) as usize] = 0x03 | LOOPS;
        let sample = 0x5000 + W_LOOP_AT as usize;
        gba.bus.rom[sample..sample + 4].copy_from_slice(&200u32.to_le_bytes());
        gba.bus.mp2k_sync();
        let sound = &gba.bus.mp2k.voices[0].sound;
        assert!(sound.len() >= 100, "a frame of sound must be ready, got {}", sound.len());
        let want: Vec<i8> = (250..256).chain(200..256).chain(200..256).map(|n: i32| (n - 128) as i8).collect();
        assert_eq!(sound[..100], want[..100], "it must go back to the loop point, not off the end");
        // The same sample without the engine's loop flag simply runs out.
        let mut plain = machine(&[(SAMPLE_RATE, 250, 255, 255)]);
        plain.bus.mp2k_sync();
        assert_eq!(plain.bus.mp2k.voices[0].sound.len(), 6, "six bytes left and then nothing");
    }

    #[test]
    fn a_held_note_keeps_its_place_across_a_frame() {
        let mut gba = machine(&[(SAMPLE_RATE, 0, 255, 255)]);
        gba.bus.mp2k_sync();
        // Thirty output samples is five of the sample's own, at a sixth of the speed.
        for _ in 0..30 {
            gba.bus.mp2k.step();
        }
        let part_way = gba.bus.mp2k.voices[0].at;
        assert!(part_way > 4u64 << 32, "five samples along, give or take the rounding");
        let base = NOTES + SI_VOICES;
        let engine_at = |gba: &mut super::super::Gba, n: u32| {
            let at = ROM_BASE + 0x5000 + W_SOUND + n;
            gba.bus.iwram[((base + V_AT) & 0x7FFF) as usize..][..4].copy_from_slice(&at.to_le_bytes());
            gba.bus.mp2k_sync();
            gba.bus.mp2k.voices[0].at
        };
        assert_eq!(
            engine_at(&mut gba, 3),
            part_way - (3u64 << 32),
            "the same note must carry on from where it was, three samples nearer"
        );
        // The engine jumping a long way (a tune restarting, say) is not a note playing on, so the
        // place is taken from the engine again instead of being carried into nonsense.
        assert_eq!(engine_at(&mut gba, 120), 0, "a long jump starts from the engine's own place");
        // A different sample in the same voice is a new note, and starts where the engine says.
        let other = ROM_BASE + 0x6000;
        let mut rom = std::mem::take(&mut gba.bus.rom);
        put32(&mut rom, 0x6000 + W_RATE as usize, SAMPLE_RATE * 1024);
        put32(&mut rom, 0x6000 + W_LENGTH as usize, 256);
        gba.bus.rom = rom;
        gba.bus.iwram[((base + V_SAMPLE) & 0x7FFF) as usize..][..4].copy_from_slice(&other.to_le_bytes());
        gba.bus.iwram[((base + V_AT) & 0x7FFF) as usize..][..4]
            .copy_from_slice(&(other + W_SOUND).to_le_bytes());
        gba.bus.mp2k_sync();
        assert_eq!(gba.bus.mp2k.voices[0].at, 0, "a new note starts at the engine's own place");
    }

    #[test]
    fn anything_that_does_not_add_up_hands_the_sound_back_to_the_console() {
        let good = |gba: &mut super::super::Gba| {
            gba.bus.mp2k_sync();
            gba.bus.mp2k.live
        };
        assert!(good(&mut machine(&[(SAMPLE_RATE, 0, 255, 255)])));

        // The marker word gone: not this engine, or not ready.
        let mut no_magic = machine(&[(SAMPLE_RATE, 0, 255, 255)]);
        no_magic.bus.iwram[((NOTES + SI_IDENT) & 0x7FFF) as usize] = 0;
        assert!(!good(&mut no_magic));

        // A voice playing from outside its own sample, which is what a table that merely looks
        // like the engine's would give.
        let mut adrift = machine(&[(SAMPLE_RATE, 0, 255, 255)]);
        let at = NOTES + SI_VOICES + V_AT;
        adrift.bus.iwram[(at & 0x7FFF) as usize..][..4].copy_from_slice(&0x0800_0000u32.to_le_bytes());
        assert!(!good(&mut adrift));

        // A sound squeezed down, which only the console's own mix can play.
        let mut squeezed = machine(&[(SAMPLE_RATE, 0, 255, 255)]);
        squeezed.bus.rom[0x5000] = 1;
        assert!(!good(&mut squeezed));

        // A note so high it could not be one.
        let mut silly = machine(&[(400_000, 0, 255, 255)]);
        assert!(!good(&mut silly));

        // A game asking the engine for an echo: its mix has something in it this one does not
        // make, so the game keeps the console's own.
        let mut echo = machine(&[(SAMPLE_RATE, 0, 255, 255)]);
        echo.bus.iwram[((NOTES + SI_ECHO) & 0x7FFF) as usize] = 50;
        assert!(!good(&mut echo));

        // The game pouring its own sound into channel A: that channel is left alone, and only the
        // one the engine is really feeding is taken over.
        let mut own = machine(&[(SAMPLE_RATE, 0, 255, 255)]);
        // Transfer 1 reading the game's own buffer in the big work memory instead of the mix.
        own.bus.io[0xBC..0xC0].copy_from_slice(&0x0200_0000u32.to_le_bytes());
        own.bus.mp2k_sync();
        assert_eq!(own.bus.mp2k.lane(0), None, "the game's own channel stays the game's");
        assert!(own.bus.mp2k.lane(1).is_some(), "the engine's channel is still taken over");

        // And a game whose music is not this engine's never gets this far.
        let mut other_engine = machine(&[(SAMPLE_RATE, 0, 255, 255)]);
        other_engine.bus.mp2k.song_list = None;
        assert!(!good(&mut other_engine));
    }
}
