//! The Souper board's music player (the BupChip) against the original CoreTone, sample for
//! sample. Made-up sample, instrument and song files exercise every script command: looped,
//! one-shot and backwards samples, volume and pitch slides (one running over the top), nested
//! and endless loops, a loop stack overflowing, calls, returns and a break unwinding sixteen
//! calls deep, sixteen channels at once loud enough to wrap round, priorities, panning, notes
//! across the whole range, a sample the package doesn't have, a song that isn't one, and every
//! command the game can write to $8007 (play, stop, pause, carry on, volume, reset).
//!
//! The original is CoreTone 1.2.2cz driven by ProSystem's BupChip layer through
//! tests/coretone/bupchip_harness.c (its first lines say how to build it). With
//! CORETONE_C=<the harness> every sample must be the same as ours, and WRITE_REF=1 then writes
//! tests/references/coretone.txt (FNV-1a hashes of our samples, every 50 steps), which the test
//! checks against without the harness.

use continuum_atari::bupchip::BupChip;
use continuum_atari::cart::Cart;
use continuum_atari::coretone::{Files, BUFFER_LEN};
use continuum_atari::machine::{Atari7800, Region};
use std::collections::HashMap;

const STEPS: usize = 1300;
const BLOCK: usize = 50;

// ---- making the files -------------------------------------------------------------------------

fn fixed(v: f64) -> u32 {
    (v * 65536.0).round() as u32
}

fn wait(n: u32) -> Vec<u8> {
    let mut v = vec![(n & 0x7F) as u8 | 0x80];
    let mut rest = n >> 7;
    while rest > 0 {
        v.push((rest & 0x7F) as u8 | 0x80);
        rest >>= 7;
    }
    assert!(v.len() <= 4);
    v
}

// Patch (instrument) commands.
fn vol(v: u8, adj: i16) -> Vec<u8> {
    let a = adj.to_le_bytes();
    vec![3, v, a[0], a[1]]
}
fn freq(offset: i32, adj: i32) -> Vec<u8> {
    [&[4u8][..], &offset.to_le_bytes(), &adj.to_le_bytes()].concat()
}
fn sample_loop(start: u16, end: u16) -> Vec<u8> {
    [&[2u8][..], &start.to_le_bytes(), &end.to_le_bytes()].concat()
}
const END: u8 = 0;
const SINGLE: u8 = 1;
const LOOP_START: u8 = 5;
const LOOP_END: u8 = 6;
const NOP: u8 = 7;

/// A track's script, with labels for calls.
#[derive(Default)]
struct Track {
    b: Vec<u8>,
    labels: HashMap<&'static str, usize>,
    calls: Vec<(usize, &'static str)>,
}

impl Track {
    fn op(&mut self, bytes: &[u8]) -> &mut Self {
        self.b.extend_from_slice(bytes);
        self
    }
    fn wait(&mut self, n: u32) -> &mut Self {
        self.b.extend(wait(n));
        self
    }
    fn priority(&mut self, p: u8) -> &mut Self {
        self.op(&[0, p])
    }
    fn pan(&mut self, l: i8, r: i8) -> &mut Self {
        self.op(&[1, l as u8, r as u8])
    }
    fn instrument(&mut self, i: u8) -> &mut Self {
        self.op(&[2, i])
    }
    fn on(&mut self, n: u8) -> &mut Self {
        self.op(&[3, n])
    }
    fn off(&mut self) -> &mut Self {
        self.op(&[4])
    }
    fn pitch(&mut self, p: i32, adj: i32) -> &mut Self {
        self.op(&[5]).op(&p.to_le_bytes()).op(&adj.to_le_bytes())
    }
    fn loop_start(&mut self, count: i8) -> &mut Self {
        self.op(&[6, count as u8])
    }
    fn loop_end(&mut self) -> &mut Self {
        self.op(&[7])
    }
    fn call(&mut self, to: &'static str) -> &mut Self {
        self.b.push(8);
        self.calls.push((self.b.len(), to));
        self.op(&[0; 4])
    }
    fn ret(&mut self) -> &mut Self {
        self.op(&[9])
    }
    fn brk(&mut self) -> &mut Self {
        self.op(&[10])
    }
    fn nop(&mut self) -> &mut Self {
        self.op(&[11])
    }
    fn mood(&mut self, m: i32) -> &mut Self {
        self.op(&[12]).op(&m.to_le_bytes())
    }
    fn end(&mut self, b: u8) -> &mut Self {
        self.op(&[b])
    }
    fn label(&mut self, name: &'static str) -> &mut Self {
        self.labels.insert(name, self.b.len());
        self
    }
    fn bytes(&self) -> Vec<u8> {
        let mut b = self.b.clone();
        for &(at, to) in &self.calls {
            let rel = self.labels[to] as i64 - (at as i64 + 4);
            b[at..at + 4].copy_from_slice(&(rel as i32).to_le_bytes());
        }
        b
    }
}

/// Five samples: a sine, a dying noise burst, a saw, a short square and a long random one.
fn sample_package() -> Vec<u8> {
    let mut seed = 0x1234_5678u32;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let sine: Vec<u8> = (0..64).map(|i| ((i as f64 * std::f64::consts::TAU / 64.0).sin() * 100.0).round() as i8 as u8).collect();
    let noise: Vec<u8> = (0..300).map(|i| (((rand() % 255) as i32 - 127) * (300 - i) / 300) as i8 as u8).collect();
    let saw: Vec<u8> = (0..1000).map(|i| ((i * 7) % 256) as u8).collect();
    let square: Vec<u8> = vec![127, 127, 127, 127, 127, 128, 128, 128, 128, 128];
    let random: Vec<u8> = (0..4000).map(|_| rand() as u8).collect();
    let samples: [(&[u8], f64, f64); 5] = [
        (&sine, 8000.0, 125.0),
        (&noise, 22050.5, 440.0),
        (&saw, 16000.25, 261.63),
        (&square, 48000.0, 3.5),
        (&random, 31250.75, 1000.0),
    ];
    let mut p = b"CSMP".to_vec();
    p.extend((samples.len() as u32).to_le_bytes());
    let mut at = 8 + 16 * samples.len();
    for (data, sf, bf) in samples {
        for w in [at as u32, data.len() as u32, fixed(sf), fixed(bf)] {
            p.extend(w.to_le_bytes());
        }
        at += data.len();
    }
    for (data, _, _) in samples {
        p.extend_from_slice(data);
    }
    p
}

/// Nine instruments: (sample, script, the script for letting go of the note).
fn instrument_package() -> Vec<u8> {
    let deep_start: Vec<u8> = (0..17).flat_map(|_| [LOOP_START, 1]).collect();
    let deep_end = vec![LOOP_END; 17];
    let instruments: Vec<(u32, Vec<u8>, Vec<u8>)> = vec![
        // 0: a sine pad, decaying, with an endless vibrato.
        (0, [vol(100, -16), sample_loop(0, 64), wait(30), vol(70, 0), vec![LOOP_START, 0xFF], freq(0x2000, 0),
             wait(5), freq(-0x1000, 0x10), wait(5), vec![LOOP_END]].concat(),
         [vol(60, -400), wait(30), vec![END]].concat()),
        // 1: a noise pluck, played once.
        (1, [vol(127, 0), vec![SINGLE], wait(100), vec![END]].concat(), vec![END]),
        // 2: a saw, rising, through nested loops, then backwards and forwards again.
        (2, [vol(90, 8), sample_loop(200, 1000), wait(3), vec![LOOP_START, 3, NOP, LOOP_START, 2], freq(1000, 50),
             wait(1), vec![LOOP_END, LOOP_END], freq(-0x30000, 0), wait(40), freq(0, 0), wait(20),
             vec![LOOP_START, 0xFF], wait(50), vec![LOOP_END]].concat(),
         [vol(80, -50), wait(10), vec![END]].concat()),
        // 3: a sample the package doesn't have.
        (7, [sample_loop(0, 4), vol(127, 0), wait(10), vec![END]].concat(), vec![END]),
        // 4: a short square played once at a negative volume, with long waits.
        (3, [vol(255, 0), vec![SINGLE], wait(128), vol(200, -3), wait(20000), vec![END]].concat(), vec![END]),
        // 5: loops nested past the stack's sixteen (the seventeenth count is read as SINGLE).
        (4, [sample_loop(1000, 3990), vol(110, -2), deep_start, wait(2), deep_end, wait(60), vec![END]].concat(),
         [vol(50, -100), wait(5), vec![END]].concat()),
        // 6: a saw played once, its pitch sliding down through nothing.
        (2, [vol(127, 0), vec![SINGLE], freq(0, -3000), wait(100), vec![END]].concat(), vec![END]),
        // 7: a sine whose volume rises past the top and wraps round.
        (0, [vol(0, 300), sample_loop(0, 64), wait(120), vol(64, 0), vec![0x80, NOP], wait(50), vec![END]].concat(),
         [vol(30, -1), wait(10), vec![END]].concat()),
        // 8: the long sample, looped, steady.
        (4, [vol(120, 0), sample_loop(1000, 3990), vec![LOOP_START, 0xFF], wait(100), vec![LOOP_END]].concat(),
         [vol(120, -600), wait(25), vec![END]].concat()),
    ];
    let mut p = b"CINS".to_vec();
    p.extend((instruments.len() as u32).to_le_bytes());
    let mut at = 8 + 12 * instruments.len();
    for (sample, main, off) in &instruments {
        for w in [*sample, at as u32, main.len() as u32] {
            p.extend(w.to_le_bytes());
        }
        at += main.len() + off.len();
    }
    for (_, main, off) in &instruments {
        p.extend(main);
        p.extend(off);
    }
    p
}

fn song(tracks: &[(i8, &Track)]) -> Vec<u8> {
    let mut s = b"CMUS".to_vec();
    s.extend((tracks.len() as u32).to_le_bytes());
    let mut at = 8 + 5 * tracks.len();
    let scripts: Vec<Vec<u8>> = tracks.iter().map(|(_, t)| t.bytes()).collect();
    for ((p, _), b) in tracks.iter().zip(&scripts) {
        s.push(*p as u8);
        s.extend((at as u32).to_le_bytes());
        at += b.len();
    }
    for b in scripts {
        s.extend(b);
    }
    s
}

fn t() -> Track {
    Track::default()
}

fn songs() -> Vec<Vec<u8>> {
    // 0: the main one.
    let mut a = t();
    a.pan(127, 127).instrument(0).on(60).wait(20).on(64).wait(20).off().wait(30);
    a.loop_start(3).on(67).wait(4).off().wait(4).loop_end();
    a.pitch(0x800, 0x10).instrument(2).on(48).wait(60).mood(7).call("sub").off().wait(100);
    a.pitch(0, 0).instrument(5).on(100).wait(30).off().wait(10).instrument(6).on(60).wait(40).end(13);
    a.label("sub").on(52).wait(10).nop().on(55).wait(10).ret();
    let mut b = t();
    b.pan(127, 30).instrument(1).loop_start(-1).on(48).wait(7).on(55).wait(129).loop_end();
    let mut c = t();
    c.priority(200).instrument(2).pan(64, 127).on(36).wait(50).pitch(-0x400, 0).wait(30).off().wait(20);
    c.instrument(4).on(0).wait(30).instrument(7).on(72).wait(150).priority(0);
    let mut d = t();
    d.instrument(3).on(127).wait(10).off().instrument(4).on(5).wait(50).instrument(5).on(90).wait(40).off();
    d.wait(20).end(0x40);
    let mut e = t();
    e.instrument(1).on(60).wait(10).end(13);
    let main = song(&[(10, &a), (5, &b), (-3, &c), (7, &d), (0, &e)]);

    // 1: eighteen tracks at once (two too many), loud enough to wrap round.
    let loud_tracks: Vec<Track> = (0..18u8)
        .map(|i| {
            let mut x = t();
            x.pan(127, (127 - 8 * i as i32) as i8).instrument(if i % 2 == 0 { 0 } else { 8 }).on(40 + i);
            x.wait(80 + i as u32).off().wait(40).end(13);
            x
        })
        .collect();
    let loud_refs: Vec<(i8, &Track)> = loud_tracks.iter().enumerate().map(|(i, x)| (20 + i as i8, x)).collect();
    let loud = song(&loud_refs);

    // 2: not a song.
    let mut bad = song(&[(10, &e)]);
    bad[0] = b'X';

    // 3: calls and a break, and loops of every count.
    let mut a = t();
    a.instrument(0).call("long").on(72).wait(20).off().wait(10).end(13);
    a.label("long").on(60).wait(1000).ret();
    let mut b = t();
    b.instrument(1).call("c").on(48).wait(30).end(13);
    b.label("c").on(64).wait(40).brk().on(67).ret();
    let mut c = t();
    c.ret().nop().loop_start(0).on(50).wait(5).loop_end().loop_start(1).on(52).wait(5).loop_end();
    c.loop_start(2).on(51).wait(3).loop_end().wait(5).mood(-5).end(13);
    let breaks = song(&[(9, &a), (9, &b), (9, &c)]);

    // 4: two tracks that end at once.
    let (mut a, mut b) = (t(), t());
    a.end(13);
    b.end(0x55);
    let empty = song(&[(10, &a), (10, &b)]);

    // 5: every note, up and down.
    let mut a = t();
    a.instrument(8);
    for n in 0..128 {
        a.on(n).wait(2);
    }
    a.off().wait(30).end(13);
    let mut b = t();
    b.instrument(4).pan(100, -100);
    for n in (0..128).rev() {
        b.on(n).wait(3);
    }
    b.end(13);
    let sweep = song(&[(10, &a), (10, &b)]);

    // 6: a call into itself until the stack is full, a break that unwinds it, a note a track of
    // negative priority can't play, and a loop end inside a call.
    let mut a = t();
    a.instrument(8).label("r").on(60).wait(1).call("r").on(80).wait(10).off().wait(5).end(13);
    let mut b = t();
    b.instrument(1).wait(100).brk().on(50).wait(10).end(13);
    let mut c = t();
    c.on(11).end(13);
    let mut d = t();
    d.call("d").label("after").on(70).wait(5).ret().wait(5).end(13);
    d.label("d").on(60).wait(5).loop_end().end(13);
    let deep = song(&[(12, &a), (12, &b), (-1, &c), (4, &d)]);

    vec![main, loud, bad, breaks, empty, sweep, deep]
}

/// The writes to $8007: (step, byte), in order.
fn commands() -> Vec<(usize, u8)> {
    vec![
        (0, 0x00),
        (1, 0x80),
        (150, 0xC0 | 20),
        (200, 0x03),
        (230, 0x02),
        (260, 0xC0 | 31),
        (300, 0x81),
        (400, 0x40),
        (405, 0x83),
        (520, 0x82),
        (525, 0x9F),
        (530, 0x86),
        (700, 0x84),
        (705, 0x80),
        (705, 0xC7),
        (800, 0x01),
        (800, 0x3F),
        (800, 0x7F),
        (801, 0x80),
        (900, 0x00),
        (901, 0x85),
        (1100, 0x03),
        (1110, 0x80),
        (1150, 0x02),
    ]
}

fn ours() -> Vec<i16> {
    let files = Files::new(sample_package(), instrument_package(), songs()).unwrap();
    let mut chip = BupChip::new(files);
    let cmds = commands();
    let mut next = 0;
    let mut out = Vec::with_capacity(STEPS * BUFFER_LEN);
    for step in 0..STEPS {
        while next < cmds.len() && cmds[next].0 == step {
            chip.command(cmds[next].1);
            next += 1;
        }
        let mut b = [0i16; BUFFER_LEN];
        chip.step(&mut b);
        out.extend(b);
    }
    out
}

/// The original's samples, from the harness, given the same files and commands.
fn theirs(harness: &str) -> Vec<i16> {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("coretone");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("samples.smp"), sample_package()).unwrap();
    std::fs::write(dir.join("instruments.ins"), instrument_package()).unwrap();
    let mut list = String::from("samples.smp\r\ninstruments.ins\r\n");
    for (i, s) in songs().iter().enumerate() {
        std::fs::write(dir.join(format!("song{i}.mus")), s).unwrap();
        list += &format!("song{i}.mus\r\n");
    }
    std::fs::write(dir.join("files.txt"), list).unwrap();
    let cmds: String = commands().iter().map(|(s, b)| format!("{s} {b}\n")).collect();
    std::fs::write(dir.join("commands.txt"), cmds).unwrap();
    let out = dir.join("theirs.raw");
    let status = std::process::Command::new(harness)
        .arg(&dir)
        .arg(STEPS.to_string())
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success(), "the harness failed");
    std::fs::read(out).unwrap().chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

fn fnv(data: &[i16]) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for v in data {
        for b in v.to_le_bytes() {
            h = (h ^ b as u64).wrapping_mul(0x0100_0000_01B3);
        }
    }
    h
}

#[test]
fn the_music_player_matches_the_original_sample_for_sample() {
    let ours = ours();
    assert_eq!(ours.len(), STEPS * BUFFER_LEN);
    assert!(ours.iter().any(|&v| v != 0));
    if let Ok(harness) = std::env::var("CORETONE_C") {
        let theirs = theirs(&harness);
        assert_eq!(theirs.len(), ours.len(), "the original made a different number of samples");
        if let Some(i) = (0..ours.len()).find(|&i| ours[i] != theirs[i]) {
            let (step, at) = (i / BUFFER_LEN, i % BUFFER_LEN);
            panic!(
                "step {step}, sample {} ({}): ours {} theirs {}",
                at / 2,
                if at % 2 == 0 { "left" } else { "right" },
                ours[i],
                theirs[i]
            );
        }
        eprintln!("all {} samples match the original", ours.len());
    }
    let hashes: Vec<String> = ours.chunks(BLOCK * BUFFER_LEN).map(|c| format!("{:016x}", fnv(c))).collect();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/references/coretone.txt");
    if std::env::var("WRITE_REF").is_ok() {
        assert!(std::env::var("CORETONE_C").is_ok(), "write the reference only after comparing with the original");
        std::fs::write(path, hashes.join("\n") + "\n").unwrap();
        return;
    }
    let want: Vec<String> = std::fs::read_to_string(path).unwrap().lines().map(str::to_string).collect();
    for (i, (h, w)) in hashes.iter().zip(&want).enumerate() {
        assert_eq!(h, w, "steps {} to {} differ from the original's", i * BLOCK, (i + 1) * BLOCK - 1);
    }
    assert_eq!(hashes.len(), want.len());
}

/// A Souper cartridge described by a .cdf with the music files: its program writes "reset" and
/// "play song 0" to $8007, and the console's sound carries the music.
#[test]
fn the_console_plays_the_music_a_game_asks_for() {
    let mut rom = vec![0u8; 32 * 0x4000];
    let last = 31 * 0x4000;
    // LDA #0, STA $8007, LDA #$80, STA $8007, then JMP to itself.
    let program = [0xA9, 0x00, 0x8D, 0x07, 0x80, 0xA9, 0x80, 0x8D, 0x07, 0x80, 0x4C, 0x0A, 0xC0];
    rom[last..last + program.len()].copy_from_slice(&program);
    rom[last + 0x3FFC..last + 0x3FFE].copy_from_slice(&[0x00, 0xC0]);
    let cdf = "ProSystem\r\nSOUPER\r\nTest\r\nRnV\\game.bin\r\nCORETONE\r\nMusic\\a.smp\r\nMusic\\a.ins\r\nMusic\\0.mus\r\n";
    let files: HashMap<&str, Vec<u8>> = HashMap::from([
        ("RnV/game.bin", rom.clone()),
        ("Music/a.smp", sample_package()),
        ("Music/a.ins", instrument_package()),
        ("Music/0.mus", songs().remove(0)),
    ]);
    let cart = Cart::from_cdf(cdf, |f| files.get(f).cloned()).unwrap();
    assert_eq!(cart.bupchip.as_ref().map(|b| b.song_count()), Some(1));
    let mut m = Atari7800::new(cart, Region::Ntsc, None);
    for _ in 0..30 {
        m.run_frame();
    }
    assert!(m.sys.cart.bupchip.as_ref().unwrap().music_playing());
    let out = std::mem::take(&mut m.sys.out);
    assert!(out.iter().any(|s| s.bup.0.abs() > 100.0) && out.iter().any(|s| s.bup.1.abs() > 100.0));
    assert!(out.iter().all(|s| s.tia == 0.0));

    // Without the two packages the game still plays, without music.
    let cart = Cart::from_cdf(cdf, |f| files.get(f).filter(|_| f != "Music/a.ins").cloned()).unwrap();
    assert!(cart.bupchip.is_none());
}
