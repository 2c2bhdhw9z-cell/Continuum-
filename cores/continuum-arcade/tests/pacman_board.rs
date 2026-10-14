//! The Pac-Man board against MAME's recorded runs, everywhere (no MAME needed): each of cArcade's
//! test programs (test-programs/pacman) runs on cArcade's board, and every frame's picture and
//! sound must hash to what MAME 0.289 produced for the same program, recorded in
//! tests/references/pacman by tests/pacman_mame.rs (which re-checks them whenever MAME is here).
//! The programs cover the tile layer, the screen flip and the video RAM mirrors (video), the
//! eight sprites (sprites), the sound chip (sound), the interrupt in all three modes and its
//! vector (irq), the watchdog (watchdog), every input and DIP switch (inputs), Ms. Pac-Man's
//! daughterboard (mspacman), Pac-Man Plus's decoder (pacplus) and bootleg boards with ROM at
//! 8000h (upper).
//!
//! Also: save states round-trip mid-run, and the committed programs are what their sources
//! assemble to (when the assembler is here; scripts/build-test-programs.sh builds it).

mod common;

use std::path::PathBuf;
use std::process::Command;

use common::*;
use continuum_arcade::pacman::{Pacman, Ports};

#[test]
fn every_frame_matches_mames_recorded_run() {
    let mut failures = Vec::new();
    for case in cases() {
        let want = read_reference(&case);
        assert_eq!(
            want.len(),
            case.frames as usize,
            "{}: the reference has the wrong length",
            case.name
        );
        let ours = run_ours(&case);
        let mut first = None;
        let mut bad = 0;
        for (i, (o, &(v, a))) in ours.iter().zip(&want).enumerate() {
            let (picture, sound) = (video_hash(&o.video) == v, audio_hash(&o.audio) == a);
            if !(picture && sound) {
                bad += 1;
                first.get_or_insert((i + 1, picture, sound));
            }
        }
        match first {
            None => eprintln!("{}: all {} frames match MAME's", case.name, case.frames),
            Some((f, picture, sound)) => failures.push(format!(
                "{}: {bad} of {} frames differ from MAME's, the first frame {f} ({})",
                case.name,
                case.frames,
                match (picture, sound) {
                    (false, false) => "picture and sound",
                    (false, true) => "picture",
                    _ => "sound",
                }
            )),
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn save_states_round_trip_mid_run() {
    for name in ["sound", "sprites", "mspacman", "watchdog"] {
        let case = cases().into_iter().find(|c| c.name == name).unwrap();
        let mut b = Pacman::new(case.variant, &roms(&case)).unwrap();
        b.set_ports(Ports::default());
        for _ in 0..25 {
            b.run_frame();
        }
        let state = b.save_state();
        let run = |b: &mut Pacman| -> Vec<(Vec<u32>, Vec<i16>)> {
            (0..30)
                .map(|_| {
                    b.run_frame();
                    (b.video().to_vec(), b.audio().to_vec())
                })
                .collect()
        };
        let straight = run(&mut b);
        let end_state = b.save_state();
        b.load_state(&state).unwrap();
        assert_eq!(
            b.save_state(),
            state,
            "{name}: the state reads back byte for byte"
        );
        let again = run(&mut b);
        assert!(
            straight == again,
            "{name}: the frames after a restore differ"
        );
        assert_eq!(
            b.save_state(),
            end_state,
            "{name}: the state after the same frames differs"
        );
    }
}

#[test]
fn the_picture_is_upright_at_the_boards_frame_rate() {
    use continuum_arcade::pacman::{
        CPU_HZ, CYCLES_PER_FRAME, FPS, HEIGHT, SAMPLES_PER_FRAME, SAMPLE_RATE, WIDTH,
    };
    assert_eq!((WIDTH, HEIGHT), (224, 288));
    assert_eq!(CPU_HZ, 18_432_000 / 6);
    assert_eq!(CYCLES_PER_FRAME, 50_688);
    assert!((FPS - 60.606_060_6).abs() < 1e-6, "{FPS}");
    assert_eq!((SAMPLE_RATE, SAMPLES_PER_FRAME), (48_000, 792));
    let case = cases().into_iter().find(|c| c.name == "video").unwrap();
    let mut b = Pacman::new(case.variant, &roms(&case)).unwrap();
    b.run_frame();
    assert_eq!(b.video().len(), WIDTH * HEIGHT);
    assert_eq!(b.audio().len(), SAMPLES_PER_FRAME);
}

/// The assembler: $SJASMPLUS, or the one scripts/build-test-programs.sh builds.
fn assembler() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SJASMPLUS") {
        return Some(PathBuf::from(p));
    }
    let p = crate_dir().join("test-suites/tools/sjasmplus-1.24.0/sjasmplus");
    p.is_file().then_some(p)
}

#[test]
fn the_committed_programs_are_what_their_sources_assemble_to() {
    let Some(sj) = assembler() else {
        eprintln!("SKIPPED: no sjasmplus here (scripts/build-test-programs.sh builds one)");
        return;
    };
    let src = crate_dir().join("test-programs/pacman");
    let tmp = crate_dir().join("target/assemble-check");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    for e in std::fs::read_dir(&src).unwrap() {
        let p = e.unwrap().path();
        if matches!(p.extension().and_then(|x| x.to_str()), Some("asm" | "inc")) {
            std::fs::copy(&p, tmp.join(p.file_name().unwrap())).unwrap();
        }
    }
    let mut checked = 0;
    for e in std::fs::read_dir(&tmp).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|x| x.to_str()) == Some("asm") {
            let out = Command::new(&sj)
                .current_dir(&tmp)
                .arg("--nologo")
                .arg(&p)
                .output()
                .unwrap();
            assert!(out.status.success(), "{} doesn't assemble", p.display());
        }
    }
    for e in std::fs::read_dir(&tmp).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|x| x.to_str()) == Some("bin") {
            let name = p.file_name().unwrap();
            let committed = std::fs::read(src.join(name)).unwrap_or_default();
            assert!(
                std::fs::read(&p).unwrap() == committed,
                "{name:?} isn't what its source assembles to"
            );
            checked += 1;
        }
    }
    assert!(checked >= 17, "only {checked} programs assembled");
}
