//! The Donkey Kong boards against MAME's recorded runs, everywhere (no MAME needed): each of
//! cArcade's test programs (test-programs/dkong) runs on cArcade's Donkey Kong or Donkey Kong Jr.
//! board, and every frame's picture must hash to what MAME 0.289 drew for the same program; on the
//! sound programs the 8035's every instruction must hash to MAME's trace, and the sound must
//! follow MAME's by the measures tests/dkong/sound.rs holds it to (every frame's level where only
//! the DAC sounds; level and spectrum over the approximated analogue effects). The references are
//! in tests/references/dkong, written by tests/dkong_mame.rs (which re-checks them whenever MAME is
//! here). The programs cover the tile map, its colours, the palette banks and the flip (tiles),
//! the sprites, their DMA and the 16-a-line limit (sprites), changes made while the beam draws
//! (raster), every input and DIP switch (inputs), the NMI latch (nmi), the watchdog's reset of the
//! whole board (watchdog), Jr.'s board with its scattered ROMs, extra RAM and tile bank (dkjr), and
//! both sound boards (dksound, jrsound): the DAC, the tune ROM, the command latch and interrupt,
//! the latches the 8035 reads, the status bit and the analogue effects.
//!
//! Also: save states round-trip mid-run, the picture's size and the board's frame rate, and the
//! committed programs are what their sources assemble to (when the assembler is here;
//! scripts/build-test-programs.sh builds it).

mod dkong;
mod measure;

use std::path::PathBuf;
use std::process::Command;

use continuum_arcade::dkong::Dkong;
use dkong::*;

#[test]
fn every_frame_instruction_and_sound_matches_mames_recorded_run() {
    let mut failures = Vec::new();
    for case in cases() {
        let want = Reference::read(&case);
        assert_eq!(want.video.len(), case.frames as usize, "{}: the reference has the wrong length", case.name);
        let mut lines = Vec::new();
        let ours = if case.sound.is_some() {
            // A frame more: MAME's reference keeps every instruction that starts before our last
            // frame ends, and the one that straddles the end only runs in the frame after.
            let longer = Case { frames: case.frames + 1, events: case.events.clone(), ..case };
            let mut ours = run_ours(&longer, Some(&mut |l: String| lines.push(l)));
            ours.truncate(case.frames as usize);
            ours
        } else {
            run_ours(&case, None)
        };
        let bad: Vec<usize> =
            ours.iter().zip(&want.video).enumerate().filter(|(_, (o, &w))| video_hash(&o.video) != w).map(|(i, _)| i + 1).collect();
        if let Some(f) = bad.first() {
            failures.push(format!("{}: {} of {} frames differ from MAME's, the first frame {f}", case.name, bad.len(), case.frames));
        }
        if case.sound.is_none() {
            eprintln!("{}: all {} frames match MAME's", case.name, case.frames);
            continue;
        }
        assert!(want.trace_lines > 0 && !want.levels.is_empty(), "{}: the reference has no sound", case.name);
        match aligned(&lines, &want.trace_first) {
            None => failures.push(format!("{}: MAME's first 8035 line isn't in ours: {}", case.name, want.trace_first)),
            Some(ours_lines) => {
                let ours_lines = &ours_lines[..want.trace_lines.min(ours_lines.len())];
                let hashes = trace_hashes(ours_lines);
                if ours_lines.len() != want.trace_lines {
                    failures.push(format!("{}: our trace has {} of MAME's {} lines", case.name, ours_lines.len(), want.trace_lines));
                } else if let Some(b) = (0..want.trace.len()).find(|&b| hashes.get(b) != Some(&want.trace[b])) {
                    failures.push(format!("{}: the 8035's trace differs from MAME's within lines {} to {}", case.name, b * 1024, b * 1024 + 1023));
                }
            }
        }
        failures.extend(sound::compare_run(&case, &ours, &want.levels, &want.segments));
        eprintln!(
            "{}: {} frames, {} 8035 instructions and {} sound segments checked",
            case.name,
            case.frames,
            want.trace_lines,
            want.segments.len()
        );
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn save_states_round_trip_mid_run() {
    for case in cases() {
        let mut b = Dkong::new(case.variant, &roms(&case)).unwrap();
        b.set_ports(case.ports);
        // Mid-script on every program (the sound programs' scripts are well under way by then).
        for _ in 0..161 {
            b.run_frame();
        }
        let state = b.save_state();
        let run = |b: &mut Dkong| -> Vec<(Vec<u32>, Vec<i16>)> {
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
        assert_eq!(b.save_state(), state, "{}: the state reads back byte for byte", case.name);
        assert!(run(&mut b) == straight, "{}: the frames and sound after a restore differ", case.name);
        assert_eq!(b.save_state(), end_state, "{}: the state after the same frames differs", case.name);
    }
}

#[test]
fn a_state_only_loads_on_its_own_board() {
    let all = cases();
    let dk = all.iter().find(|c| c.name == "tiles").unwrap();
    let jr = all.iter().find(|c| c.name == "dkjr").unwrap();
    let mut a = Dkong::new(dk.variant, &roms(dk)).unwrap();
    let mut b = Dkong::new(jr.variant, &roms(jr)).unwrap();
    a.run_frame();
    b.run_frame();
    assert!(b.load_state(&a.save_state()).is_err(), "a Donkey Kong state loads on Jr.'s board");
    assert!(a.load_state(&b.save_state()).is_err(), "a Jr. state loads on Donkey Kong's board");
    assert!(a.load_state(b"not a state").is_err(), "junk loads as a state");
}

#[test]
fn the_picture_is_upright_at_the_boards_frame_rate() {
    use continuum_arcade::dkong::{AS_PER_CYCLE, AS_PER_FRAME, AS_PER_SOUND_CYCLE, FPS, HEIGHT, SAMPLES_PER_FRAME, SAMPLE_RATE, WIDTH};
    // MAME: 256 x 224 of a 384 x 264 raster at 6.144 MHz, turned 270 degrees; the Z80 at
    // 3.072 MHz, the 8035 at 6 MHz (400 kHz machine cycles).
    assert_eq!((WIDTH, HEIGHT), (224, 256));
    assert!((FPS - 60.606_061).abs() < 1e-6, "{FPS}");
    assert_eq!(AS_PER_FRAME, 1_000_000_000_000_000_000 / 6_144_000 * 384 * 264);
    assert_eq!(AS_PER_CYCLE, 1_000_000_000_000_000_000 / 3_072_000);
    assert_eq!(AS_PER_SOUND_CYCLE, 1_000_000_000_000_000_000 / 400_000);
    assert_eq!((SAMPLE_RATE, SAMPLES_PER_FRAME), (48_000, 792));
    for case in cases().iter().filter(|c| c.name == "tiles" || c.name == "dkjr") {
        let mut b = Dkong::new(case.variant, &roms(case)).unwrap();
        b.run_frame();
        assert_eq!(b.video().len(), WIDTH * HEIGHT, "{}", case.name);
        assert_eq!(b.audio().len(), SAMPLES_PER_FRAME, "{}", case.name);
        assert_eq!(b.frame_count(), 1, "{}", case.name);
    }
}

/// The assembler: $SJASMPLUS, or the one scripts/build-test-programs.sh builds.
fn assembler() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SJASMPLUS") {
        return Some(PathBuf::from(p));
    }
    let p = crate_dir().join("test-suites/tools/sjasmplus-1.24.0/sjasmplus");
    p.is_file().then_some(p)
}

/// The Z80 programs and the graphics (sjasmplus); the 8035 programs (*.a48, The Macro Assembler
/// AS) are checked by tests/mcs48 when AS is here.
#[test]
fn the_committed_programs_are_what_their_sources_assemble_to() {
    let Some(sj) = assembler() else {
        eprintln!("SKIPPED: no sjasmplus here (scripts/build-test-programs.sh builds one)");
        return;
    };
    let src = crate_dir().join("test-programs/dkong");
    let tmp = crate_dir().join("target/assemble-check-dkong");
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
            let out = Command::new(&sj).current_dir(&tmp).arg("--nologo").arg(&p).output().unwrap();
            assert!(out.status.success(), "{} doesn't assemble", p.display());
        }
    }
    let mut checked = 0;
    for e in std::fs::read_dir(&tmp).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|x| x.to_str()) == Some("bin") {
            let name = p.file_name().unwrap();
            let committed = std::fs::read(src.join(name)).unwrap_or_default();
            assert!(std::fs::read(&p).unwrap() == committed, "{name:?} isn't what its source assembles to");
            checked += 1;
        }
    }
    // Nine programs, and the tiles, sprites and PROMs.
    assert_eq!(checked, 12, "only {checked} files assembled");
}
