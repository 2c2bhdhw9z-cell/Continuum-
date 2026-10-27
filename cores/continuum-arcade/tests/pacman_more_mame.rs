//! The Pac-Man driver's other machines against MAME, live: each case of tests/common/more.rs runs
//! on the local reference MAME and on cArcade (loaded through the set list, as the app loads it),
//! and every frame's picture must match pixel for pixel and its sound sample for sample. Then the
//! committed reference files must be what MAME produced, so tests/pacman_more_board.rs, which runs
//! everywhere, checks cArcade against MAME's real output.
//!
//! Without MAME it says SKIPPED and passes. CARCADE_WRITE_REFERENCES=1 rewrites the references.

mod common;

use common::more::*;
use common::{audio_hash, first_difference, mame, mame_version, video_hash};

#[test]
fn the_pacman_drivers_other_boards_match_mame() {
    let Some(mame) = mame() else {
        eprintln!("SKIPPED: no reference MAME here (set CARCADE_MAME to its binary)");
        return;
    };
    let version = mame_version(&mame);
    let write = std::env::var("CARCADE_WRITE_REFERENCES").is_ok_and(|v| v == "1");
    let only = std::env::var("CARCADE_ONLY").ok();
    let mut failures = Vec::new();
    for case in cases() {
        if only.as_deref().is_some_and(|o| o != case.name) {
            continue;
        }
        let theirs = run_mame(&mame, &case);
        let ours = run_ours(&case);
        if let Some(d) = first_difference(case.name, &ours, &theirs) {
            failures.push(d);
        } else {
            eprintln!("{}: {} frames match MAME {version}", case.name, case.frames);
        }
        if write {
            write_reference(&case, &theirs, &version);
        } else {
            let got: Vec<(u64, u64)> =
                theirs.iter().map(|f| (video_hash(&f.video), audio_hash(&f.audio))).collect();
            if read_reference(&case) != got {
                failures.push(format!("{}: the reference file isn't what MAME produces", case.name));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
