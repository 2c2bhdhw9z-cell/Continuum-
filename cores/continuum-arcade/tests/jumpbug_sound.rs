//! Jump Bug's sound against MAME's: the `jumpbug` program (tests/galaxian) plays two tones and the
//! noise on the board's AY-3-8910 while the picture runs. Live, with the reference MAME here, MAME
//! 0.289 records it at 192 kHz and every one of our samples must be within 2 (of 16 bits) of
//! MAME's, MAME's level for silence taken off (its mixing rounds); the sums of each 1,024 of
//! MAME's samples are then what tests/references/galaxian/jumpbug-sound.txt holds. Everywhere,
//! ours must stay within 2 a sample of those sums. CARCADE_WRITE_REFERENCES=1 rewrites the file.

mod galaxian;

use continuum_arcade::galaxian::{Galaxian, Rotation};
use galaxian::*;

const FRAMES: usize = 150;
const BLOCK: usize = 1024;

fn ours() -> Vec<i32> {
    let case = cases().into_iter().find(|c| c.name == "jumpbug").unwrap();
    let mut b = Galaxian::new(case.variant, Rotation::Rot90, &roms(&case)).unwrap();
    let mut v = Vec::new();
    for _ in 0..FRAMES {
        b.run_frame();
        v.extend(b.raw_audio().iter().map(|&s| i32::from(s)));
    }
    v
}

fn reference_path() -> std::path::PathBuf {
    crate_dir().join("tests/references/galaxian/jumpbug-sound.txt")
}

#[test]
fn jump_bugs_sound_is_mames() {
    let ours = ours();
    if let Some(mame) = mame() {
        let case = cases().into_iter().find(|c| c.name == "jumpbug").unwrap();
        let work = crate_dir().join("target/mame-runs/jumpbug-sound");
        let _ = std::fs::remove_dir_all(&work);
        let roms = work.join("roms/jumpbug");
        std::fs::create_dir_all(&roms).unwrap();
        for (n, d) in files(&case) {
            std::fs::write(roms.join(n), d).unwrap();
        }
        let st = std::process::Command::new(&mame)
            .current_dir(&work)
            .env("SDL_VIDEODRIVER", "dummy")
            .env("SDL_AUDIODRIVER", "dummy")
            .args(["jumpbug", "-rompath", "roms", "-video", "none", "-sound", "none", "-nothrottle"])
            .args(["-skip_gameinfo", "-seconds_to_run", "4", "-samplerate", "192000", "-wavwrite", "sound.wav"])
            .output()
            .unwrap();
        assert!(st.status.success() || work.join("sound.wav").is_file());
        let wav = std::fs::read(work.join("sound.wav")).unwrap();
        let at = wav.windows(4).position(|w| w == b"data").unwrap() + 8;
        let theirs: Vec<i32> =
            wav[at..].chunks_exact(2).map(|c| i32::from(i16::from_le_bytes([c[0], c[1]]))).collect();
        let silence = theirs[0];
        let theirs: Vec<i32> = theirs.iter().map(|&s| s - silence).collect();
        assert!(theirs.len() >= ours.len());
        let worst = ours.iter().zip(&theirs).map(|(a, b)| (a - b).abs()).max().unwrap();
        assert!(worst <= 2, "a sample {worst} from MAME's");
        eprintln!("all {} samples within {worst} of MAME 0.289's", ours.len());
        let sums: Vec<i64> = theirs[..ours.len()].chunks(BLOCK).map(|c| c.iter().map(|&s| i64::from(s)).sum()).collect();
        let text: String = sums.iter().map(|s| format!("{s}\n")).collect();
        if std::env::var("CARCADE_WRITE_REFERENCES").is_ok_and(|v| v == "1") {
            std::fs::write(
                reference_path(),
                format!("# MAME 0.289's Jump Bug sound for the jumpbug program: the sum of each {BLOCK} samples at 192 kHz, silence taken off.\n{text}"),
            )
            .unwrap();
        }
    } else {
        eprintln!("no reference MAME here: checking against the recorded sums");
    }
    let want: Vec<i64> = std::fs::read_to_string(reference_path())
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.parse().unwrap())
        .collect();
    for (i, (c, w)) in ours.chunks(BLOCK).zip(&want).enumerate() {
        let s: i64 = c.iter().map(|&s| i64::from(s)).sum();
        assert!((s - w).abs() <= 2 * c.len() as i64, "block {i}: ours {s}, MAME's {w}");
    }
    // And it isn't silent.
    assert!(want.iter().any(|&s| s != 0));
}
