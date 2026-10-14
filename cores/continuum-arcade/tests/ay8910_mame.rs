//! The AY-3-8910 against MAME's, sample for sample. tests/ay8910/jumpbug.asm (ours, assembled
//! with The Macro Assembler AS) runs on Jump Bug's board, whose AY-3-8910 MAME clocks at 1.536
//! MHz, so the chip's output (clock / 8) is exactly 192,000 samples a second: MAME 0.289 recorded
//! it at that rate, and tests/ay8910/jumpbug.mame keeps a hash of each 4,096 samples (made by
//! tests/ay8910/mame_ay.py). The script plays every channel's tone at many periods, the noise at
//! many periods and through each channel, all sixteen envelope shapes, envelope periods 0, 1, 2,
//! 3, 9 and 300, periods and shapes changed mid-wave, the masked register bits, and the mixer's
//! combinations, one channel at a time.
//!
//! Here the same program runs on our Z80 with our AY-3-8910 wired the same way, each register
//! write landing on the sample MAME's would (its Z80 writes 4 T-states into LD (HL),A or
//! LD (DE),A; the chip ticks every 16 T-states). MAME's board turns a level into a voltage
//! through its own model of the chip's output stage, so the reference also keeps MAME's value for
//! each fixed amplitude and each envelope level, measured by the script's calibration at its
//! start; every sample we make, put through that table, must equal MAME's.

use std::path::PathBuf;

use continuum_arcade::ay8910::{Ay8910, NoPorts};
use continuum_arcade::z80::{Bus, Z80};

/// Jump Bug's board as the program uses it: ROM, RAM for the stack, and the AY-3-8910 at 5800h
/// (data) and 5900h (the address latch). Writes to the chip are collected per instruction.
struct JumpBug {
    rom: Vec<u8>,
    ram: Vec<u8>,
    /// (true for the address latch, the value), in order.
    writes: Vec<(bool, u8)>,
}

impl Bus for JumpBug {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.rom.get(usize::from(addr)).copied().unwrap_or(0),
            0x4000..=0x47FF => self.ram[usize::from(addr - 0x4000)],
            _ => 0xFF,
        }
    }
    fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0x4000..=0x47FF => self.ram[usize::from(addr - 0x4000)] = val,
            0x5800..=0x58FF => self.writes.push((false, val)),
            0x5900..=0x59FF => self.writes.push((true, val)),
            _ => {}
        }
    }
    fn input(&mut self, _port: u16) -> u8 {
        0xFF
    }
    fn output(&mut self, _port: u16, _val: u8) {}
}

struct Reference {
    fixed: Vec<i16>,
    envelope: Vec<i16>,
    samples: usize,
    hashes: Vec<u64>,
}

fn reference() -> Reference {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ay8910/jumpbug.mame");
    let text = std::fs::read_to_string(path).unwrap();
    let mut lines = text.lines().filter(|l| !l.starts_with('#'));
    let mut table = |name: &str| -> Vec<i16> {
        let l = lines.next().unwrap();
        let rest = l.strip_prefix(name).expect(name);
        rest.split_whitespace()
            .map(|v| v.parse().unwrap())
            .collect()
    };
    let fixed = table("fixed ");
    let envelope = table("envelope ");
    let samples = lines
        .next()
        .and_then(|l| l.strip_prefix("samples "))
        .unwrap()
        .parse()
        .unwrap();
    let hashes = lines.map(|l| u64::from_str_radix(l, 16).unwrap()).collect();
    Reference {
        fixed,
        envelope,
        samples,
        hashes,
    }
}

/// What MAME's board would put out for one tick of our chip: the one channel playing, through
/// MAME's table for its mode (envelope or fixed); with all three at 0, the level-0 value.
fn mame_value(ay: &Ay8910, levels: [u8; 3], r: &Reference) -> i16 {
    let playing: Vec<usize> = (0..3).filter(|&ch| levels[ch] != 0).collect();
    assert!(
        playing.len() <= 1,
        "the script plays one channel at a time: {levels:?}"
    );
    match playing.first() {
        None => r.fixed[0],
        Some(&ch) => {
            let level = usize::from(levels[ch]);
            if ay.regs[8 + ch] & 0x10 != 0 {
                r.envelope[level]
            } else {
                r.fixed[level]
            }
        }
    }
}

/// Our recording of the program: one value per tick of the chip, `count` of them.
fn play(r: &Reference) -> Vec<i16> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ay8910/jumpbug.bin");
    let mut bus = JumpBug {
        rom: std::fs::read(path).unwrap(),
        ram: vec![0; 0x800],
        writes: Vec::new(),
    };
    let mut z80 = Z80::new();
    let mut ay = Ay8910::new();
    let mut out = Vec::with_capacity(r.samples);
    let mut t: u64 = 0;
    while out.len() < r.samples {
        let op = bus.read(z80.pc);
        let start = t;
        t += u64::from(z80.step(&mut bus));
        for (address, v) in std::mem::take(&mut bus.writes) {
            // Only LD (DE),A and LD (HL),A write to the chip; the write is 4 T-states in.
            assert!(
                op == 0x12 || op == 0x77,
                "a chip write from opcode {op:02X}"
            );
            let at = start + 4;
            // The samples the chip made before the write: one each 16 T-states.
            while (out.len() as u64) < at.div_ceil(16) && out.len() < r.samples {
                let levels = ay.tick();
                out.push(mame_value(&ay, levels, r));
            }
            if address {
                ay.write_address(v);
            } else {
                ay.write_data(v, &mut NoPorts);
            }
        }
        while (out.len() as u64) < t / 16 && out.len() < r.samples {
            let levels = ay.tick();
            out.push(mame_value(&ay, levels, r));
        }
    }
    out
}

fn fnv1a(samples: &[i16]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for v in samples {
        for b in v.to_le_bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3);
        }
    }
    h
}

#[test]
fn every_sample_matches_mame() {
    let r = reference();
    let ours = play(&r);
    // With AY_MAME_RAW set to mame_ay.py's mame.raw, the first differing sample is shown.
    if let Ok(raw) = std::env::var("AY_MAME_RAW") {
        let bytes = std::fs::read(raw).unwrap();
        let mame: Vec<i16> = bytes
            .chunks(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        if let Some(i) = (0..ours.len().min(mame.len())).find(|&i| ours[i] != mame[i]) {
            let from = i.saturating_sub(8);
            panic!(
                "sample {i} differs\n  MAME {:?}\n  ours {:?}",
                &mame[from..(i + 8).min(mame.len())],
                &ours[from..(i + 8).min(ours.len())]
            );
        }
    }
    let hashes: Vec<u64> = ours.chunks(4096).map(fnv1a).collect();
    assert_eq!(hashes.len(), r.hashes.len());
    if let Some(b) = (0..hashes.len()).find(|&b| hashes[b] != r.hashes[b]) {
        panic!(
            "our output differs from MAME's within samples {} to {} (rerun mame_ay.py with \
             AY_KEEP=1 and set AY_MAME_RAW to see where)",
            b * 4096,
            (b * 4096 + 4095).min(r.samples - 1)
        );
    }
    eprintln!("all {} samples match MAME's", r.samples);
}
