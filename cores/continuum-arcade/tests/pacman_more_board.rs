//! The Pac-Man driver's other machines against MAME's recorded runs, everywhere (no MAME needed):
//! each case of tests/common/more.rs (cArcade's own programs laid out and scrambled as each set's
//! chips are) runs on cArcade, loaded through the set list as the app loads it, and every frame's
//! picture and sound must hash to what MAME 0.289 produced, recorded in tests/references/pacman-more
//! by tests/pacman_more_mame.rs. The cases cover how each board's program and graphics ROMs are
//! wired (Eyes, Woodpecker, Ponpoko, Crush Roller 5, the scrambled Ms. Pac-Man bootlegs, the
//! blue/green swap), the interrupt vector through Piranha's, Naughty Mouse's and Ms. Pac-Man II's
//! boards, Make Trax's, Magic Brush's and Ms. Pac-Man II's protection, the Pengo bootlegs' map and
//! Champion Edition's two games. Also: their save states round-trip mid-run.

mod common;

use common::more::*;
use common::{audio_hash, video_hash};

#[test]
fn every_frame_matches_mames_recorded_run() {
    let mut failures = Vec::new();
    for case in cases() {
        let want = read_reference(&case);
        assert_eq!(want.len(), case.frames as usize, "{}: the reference has the wrong length", case.name);
        let ours = run_ours(&case);
        let bad: Vec<usize> = ours
            .iter()
            .zip(&want)
            .enumerate()
            .filter(|(_, (o, &(v, a)))| video_hash(&o.video) != v || audio_hash(&o.audio) != a)
            .map(|(i, _)| i + 1)
            .collect();
        if bad.is_empty() {
            eprintln!("{}: all {} frames match MAME's", case.name, case.frames);
        } else {
            failures.push(format!("{}: {} frames differ from MAME's, the first {}", case.name, bad.len(), bad[0]));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn save_states_round_trip_mid_run() {
    for name in ["ports-maketrax", "champion", "pengo", "vectors-nmouse"] {
        let case = cases().into_iter().find(|c| c.name == name).unwrap();
        let mut b = board(&case);
        b.set_ports(rest(&case));
        for _ in 0..45 {
            b.run_frame();
        }
        let state = b.save_state();
        let run = |b: &mut continuum_arcade::pacman::Pacman| -> Vec<(Vec<u32>, Vec<i16>)> {
            (0..40).map(|_| { b.run_frame(); (b.video().to_vec(), b.audio().to_vec()) }).collect()
        };
        let straight = run(&mut b);
        b.load_state(&state).unwrap();
        assert_eq!(b.save_state(), state, "{name}: the state reads back byte for byte");
        assert!(straight == run(&mut b), "{name}: the frames after a restore differ");
    }
}
