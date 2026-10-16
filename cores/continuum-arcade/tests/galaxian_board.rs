//! The Galaxian board family against MAME's recorded runs, everywhere (no MAME needed): each of
//! cArcade's test programs (test-programs/galaxian) runs on cArcade's board, and every frame's
//! picture must hash to what MAME 0.289 drew for the same program, recorded in
//! tests/references/galaxian by tests/galaxian_mame.rs (which re-checks them whenever MAME is
//! here). The programs cover the tile layer with its column scroll and colours, the screen flips
//! and the memory mirrors (tiles), the eight sprites (sprites), the shells and the missile
//! (shots), the star field (stars), changes made while the beam draws (raster), the NMI's
//! latch and edge (nmi), the watchdog (watchdog), every input and DIP switch and the coin
//! lockout (inputs), and Moon Cresta's board with its graphics bank, plain and with its
//! scrambled program (mooncrst, mooncrst-encrypted).
//!
//! Also: save states round-trip mid-run, and the committed programs are what their sources
//! assemble to (when the assembler is here; scripts/build-test-programs.sh builds it).

mod galaxian;

use std::path::PathBuf;
use std::process::Command;

use continuum_arcade::galaxian::{Galaxian, Ports, Rotation};
use galaxian::*;

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
        let bad: Vec<usize> = ours
            .iter()
            .zip(&want)
            .enumerate()
            .filter(|(_, (o, &w))| video_hash(o) != w)
            .map(|(i, _)| i + 1)
            .collect();
        match bad.first() {
            None => eprintln!("{}: all {} frames match MAME's", case.name, case.frames),
            Some(f) => failures.push(format!(
                "{}: {} of {} frames differ from MAME's, the first frame {f}",
                case.name,
                bad.len(),
                case.frames
            )),
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn save_states_round_trip_mid_run() {
    for name in [
        "sprites",
        "stars",
        "raster",
        "watchdog",
        "mooncrst-encrypted",
    ] {
        let case = cases().into_iter().find(|c| c.name == name).unwrap();
        let mut b = Galaxian::new(case.variant, Rotation::Rot90, &roms(&case)).unwrap();
        b.set_ports(Ports {
            in0: 0,
            in1: 0,
            in2: 0x04,
            in3: 0,
        });
        for _ in 0..33 {
            b.run_frame();
        }
        let state = b.save_state();
        let run = |b: &mut Galaxian| -> Vec<Vec<u32>> {
            (0..30)
                .map(|_| {
                    b.run_frame();
                    b.video().to_vec()
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
    use continuum_arcade::galaxian::{
        CPU_HZ, CYCLES_PER_FRAME, FPS, HEIGHT, SAMPLES_PER_FRAME, SAMPLE_RATE, WIDTH,
    };
    assert_eq!((WIDTH, HEIGHT), (224, 768));
    assert_eq!(CPU_HZ, 18_432_000 / 6);
    assert_eq!(CYCLES_PER_FRAME, 50_688);
    assert!((FPS - 60.606_060_6).abs() < 1e-6, "{FPS}");
    assert_eq!((SAMPLE_RATE, SAMPLES_PER_FRAME), (48_000, 792));
    let case = cases().into_iter().find(|c| c.name == "tiles").unwrap();
    let mut b = Galaxian::new(case.variant, Rotation::Rot90, &roms(&case)).unwrap();
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
    let src = crate_dir().join("test-programs/galaxian");
    let tmp = crate_dir().join("target/assemble-check-galaxian");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    for e in std::fs::read_dir(&src).unwrap() {
        let p = e.unwrap().path();
        if matches!(p.extension().and_then(|x| x.to_str()), Some("asm" | "inc")) {
            std::fs::copy(&p, tmp.join(p.file_name().unwrap())).unwrap();
        }
    }
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
    let mut checked = 0;
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
    assert!(checked >= 11, "only {checked} programs assembled");
}
