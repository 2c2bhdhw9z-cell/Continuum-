//! Konami's Scramble-family boards against MAME's recorded runs, everywhere (no MAME needed):
//! each test program (test-programs/konami) runs on cArcade's Scramble, The End, Super Cobra,
//! Frogger and Amidar boards, and every frame's picture must hash to what MAME 0.289 drew, the
//! sound Z80's every instruction must hash to MAME's trace, and each sound segment must be MAME's
//! within the tolerances in tests/konami (the filters are approximated). The references are in
//! tests/references/konami, written by tests/konami_mame.rs. The programs cover the tiles, column
//! scroll and colours, the sprites, the shots, both flips, the stars and their blinking, the
//! backgrounds, mid-frame changes, every input and three DIP switches through the first PPI, the
//! protection chip, the sound commands and interrupts through the second PPI, the sound board's
//! silencing, and the watchdog's reset of the whole board; and on the sound board, tones on every
//! channel of both AYs with each filter switch, the timer, the latch and the noise.
//!
//! Also: save states round-trip mid-run, and the committed programs are what their sources
//! assemble to.

mod konami;
mod measure;

use continuum_arcade::galaxian::{Galaxian, Rotation};
use konami::*;

#[test]
fn every_frame_and_instruction_matches_mames_recorded_run() {
    let mut failures = Vec::new();
    for case in cases() {
        let want = read_reference(&case);
        let p = &case.ports;
        assert_eq!(want.ports, [p.in0, p.in1, p.in2, p.in3], "{}: the ports", case.name);
        let mut lines = Vec::new();
        let ours = run_ours(&case, Some(&mut |l| lines.push(l)));
        let bad: Vec<usize> = ours.iter().zip(&want.video).enumerate().filter(|(_, (o, &w))| video_hash(&o.video) != w).map(|(i, _)| i + 1).collect();
        if let Some(f) = bad.first() {
            failures.push(format!("{}: {} of {} frames differ from MAME's, the first frame {f}", case.name, bad.len(), case.frames));
        }
        let skip = lines.iter().position(|l| *l == want.trace_first).unwrap_or(0);
        let ours_lines = &lines[skip..(skip + want.trace_lines).min(lines.len())];
        let hashes = trace_hashes(ours_lines);
        if let Some(b) = (0..want.trace.len()).find(|&b| hashes.get(b) != Some(&want.trace[b])) {
            failures.push(format!("{}: the sound CPU's trace differs from MAME's within lines {} to {}", case.name, b * 1024, b * 1024 + 1023));
        }
        let segs = compared_segments(&ours);
        if segs != want.sound.iter().map(|s| s.0).collect::<Vec<_>>() {
            failures.push(format!("{}: the sound's segments aren't MAME's run's", case.name));
            continue;
        }
        for (seg, m) in &want.sound {
            if let Err(e) = compare_sound(case.name, *seg, &our_measure(&ours, *seg), m) {
                failures.push(e);
            }
        }
        eprintln!("{}: {} frames, {} sound CPU instructions and {} sound segments checked", case.name, case.frames, want.trace_lines, want.sound.len());
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn save_states_round_trip_mid_run() {
    for case in cases() {
        let mut b = Galaxian::new(case.variant, Rotation::Rot90, &roms(&case)).unwrap();
        b.set_ports(case.ports);
        // Past the watchdog's reset at frame 158, so the state has seen it.
        for _ in 0..161 {
            b.run_frame();
        }
        let state = b.save_state();
        let run = |b: &mut Galaxian| -> Vec<(Vec<u32>, Vec<i16>)> {
            (0..30).map(|_| { b.run_frame(); (b.video().to_vec(), b.audio().to_vec()) }).collect()
        };
        let straight = run(&mut b);
        let end_state = b.save_state();
        b.load_state(&state).unwrap();
        assert_eq!(b.save_state(), state, "{}: the state reads back byte for byte", case.name);
        assert!(run(&mut b) == straight, "{}: the frames after a restore differ", case.name);
        assert_eq!(b.save_state(), end_state, "{}: the state after the same frames differs", case.name);
    }
}

#[test]
fn the_committed_programs_are_what_their_sources_assemble_to() {
    let sj = crate_dir().join("test-suites/tools/sjasmplus-1.24.0/sjasmplus");
    let sj = std::env::var("SJASMPLUS").map(std::path::PathBuf::from).unwrap_or(sj);
    if !sj.is_file() {
        eprintln!("SKIPPED: no sjasmplus here (scripts/build-test-programs.sh builds one)");
        return;
    }
    let src = crate_dir().join("test-programs/konami");
    let tmp = crate_dir().join("target/assemble-check-konami");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    for e in std::fs::read_dir(&src).unwrap() {
        let p = e.unwrap().path();
        if matches!(p.extension().and_then(|x| x.to_str()), Some("asm" | "inc")) {
            std::fs::copy(&p, tmp.join(p.file_name().unwrap())).unwrap();
        }
    }
    let mut checked = 0;
    for e in std::fs::read_dir(&src).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|x| x.to_str()) == Some("asm") {
            let out = std::process::Command::new(&sj).current_dir(&tmp).arg("--nologo").arg(p.file_name().unwrap()).output().unwrap();
            assert!(out.status.success(), "{} doesn't assemble", p.display());
        }
    }
    for e in std::fs::read_dir(&tmp).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|x| x.to_str()) == Some("bin") {
            let name = p.file_name().unwrap();
            assert!(std::fs::read(&p).unwrap() == std::fs::read(src.join(name)).unwrap_or_default(), "{name:?} isn't what its source assembles to");
            checked += 1;
        }
    }
    assert_eq!(checked, 6, "six programs");
}
