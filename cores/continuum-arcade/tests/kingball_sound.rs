//! King & Balloon's speech board against MAME's, as an approximation: the `kingball` program sends
//! the speech board a new command every 8 frames and our `kbspeech` program plays square waves, a
//! ramp and a pattern through its DAC. cArcade runs the speech CPU on its own clock, each command
//! reaching it at the moment the main CPU sent it; MAME interleaves the two CPUs its own way, so
//! the DAC's changes come within a sample or two of MAME's, not on the same sample. Live, MAME
//! 0.289 records it at 192 kHz: the sample-to-sample changes of ours and MAME's (where MAME's
//! isn't clipped) must agree in size (within 10%) and correlate at 0.75 or better, and then
//! tests/references/galaxian/kingball-sound.txt keeps the size of our changes, each 4,096 samples,
//! so everywhere else the speech is held to what was checked.

mod galaxian;

use continuum_arcade::galaxian::{Galaxian, Rotation};
use galaxian::*;

const FRAMES: usize = 150;
const BLOCK: usize = 4096;

fn ours() -> Vec<f64> {
    let case = cases().into_iter().find(|c| c.name == "kingball").unwrap();
    let mut b = Galaxian::new(case.variant, Rotation::Rot90, &roms(&case)).unwrap();
    let mut v = Vec::new();
    for _ in 0..FRAMES {
        b.run_frame();
        v.extend(b.raw_audio().iter().map(|&s| f64::from(s)));
    }
    v
}

fn energy(v: &[f64]) -> Vec<f64> {
    v.windows(2).collect::<Vec<_>>().chunks(BLOCK).map(|c| c.iter().map(|w| (w[1] - w[0]).abs()).sum()).collect()
}

fn reference_path() -> std::path::PathBuf {
    crate_dir().join("tests/references/galaxian/kingball-sound.txt")
}

#[test]
fn king_and_balloons_speech_is_close_to_mames() {
    let ours = ours();
    if let Some(mame) = mame() {
        let case = cases().into_iter().find(|c| c.name == "kingball").unwrap();
        let work = crate_dir().join("target/mame-runs/kingball-sound");
        let _ = std::fs::remove_dir_all(&work);
        let roms = work.join("roms/kingball");
        std::fs::create_dir_all(&roms).unwrap();
        for (n, d) in files(&case) {
            std::fs::write(roms.join(n), d).unwrap();
        }
        std::process::Command::new(&mame)
            .current_dir(&work)
            .env("SDL_VIDEODRIVER", "dummy")
            .env("SDL_AUDIODRIVER", "dummy")
            .args(["kingball", "-rompath", "roms", "-video", "none", "-sound", "none", "-nothrottle"])
            .args(["-skip_gameinfo", "-seconds_to_run", "4", "-samplerate", "192000", "-wavwrite", "sound.wav"])
            .output()
            .unwrap();
        let wav = std::fs::read(work.join("sound.wav")).unwrap();
        let at = wav.windows(4).position(|w| w == b"data").unwrap() + 8;
        let theirs: Vec<f64> =
            wav[at..].chunks_exact(2).map(|c| f64::from(i16::from_le_bytes([c[0], c[1]]))).collect();
        let n = ours.len().min(theirs.len());
        let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
        for i in 20_000..n {
            if theirs[i].abs() > 32_000.0 || theirs[i - 1].abs() > 32_000.0 {
                continue;
            }
            let (x, y) = (ours[i] - ours[i - 1], theirs[i] - theirs[i - 1]);
            sxy += x * y;
            sxx += x * x;
            syy += y * y;
        }
        let corr = sxy / (sxx * syy).sqrt();
        let scale = sxy / sxx;
        eprintln!("changes correlate at {corr:.3}, MAME's {scale:.3} times ours");
        assert!(corr >= 0.75, "the speech correlates with MAME's at only {corr:.3}");
        assert!((0.9..=1.1).contains(&scale), "MAME's speech is {scale:.3} times ours");
        if std::env::var("CARCADE_WRITE_REFERENCES").is_ok_and(|v| v == "1") {
            let text: String = energy(&ours).iter().map(|e| format!("{e:.0}\n")).collect();
            std::fs::write(
                reference_path(),
                format!("# cArcade's King & Balloon sound for the kingball program, as checked against MAME 0.289 (changes correlating at {corr:.3}, MAME's {scale:.3} times ours): the sum of the sizes of the sample-to-sample changes of each {BLOCK} samples at 192 kHz.\n{text}"),
            )
            .unwrap();
        }
    }
    // Everywhere: the speech as it was when it was checked against MAME, within 2% a block.
    let want: Vec<f64> = std::fs::read_to_string(reference_path())
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.parse().unwrap())
        .collect();
    let got = energy(&ours);
    let (sum_got, sum_want): (f64, f64) = (got.iter().sum(), want.iter().sum());
    assert!(sum_want > 0.0, "no speech");
    assert!((sum_got / sum_want - 1.0).abs() < 0.02, "ours {sum_got:.0}, as checked {sum_want:.0}");
}
