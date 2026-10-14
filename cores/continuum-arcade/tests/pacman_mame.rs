//! The Pac-Man board against MAME, live: each of cArcade's test programs runs on the local
//! reference MAME (built from source, never shipped; see tests/common) and on cArcade's board,
//! and every frame's picture must match pixel for pixel and every frame's sound sample for
//! sample (192 kHz voice sums, recovered exactly from MAME's 16-bit samples). Then the committed
//! reference files must be what MAME produced, so tests/pacman_board.rs, which runs everywhere,
//! checks cArcade against MAME's real output.
//!
//! Without MAME it says SKIPPED and passes. CARCADE_WRITE_REFERENCES=1 rewrites the reference
//! files from MAME's runs.

mod common;

use common::*;

#[test]
fn pacman_board_matches_mame() {
    let Some(mame) = mame() else {
        eprintln!("SKIPPED: no reference MAME here (set CARCADE_MAME to its binary)");
        return;
    };
    let version = mame_version(&mame);
    let write = std::env::var("CARCADE_WRITE_REFERENCES").is_ok_and(|v| v == "1");
    let mut failures = Vec::new();
    for case in cases() {
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
            let want = read_reference(&case);
            let got: Vec<(u64, u64)> = theirs
                .iter()
                .map(|f| (video_hash(&f.video), audio_hash(&f.audio)))
                .collect();
            if want != got {
                failures.push(format!(
                    "{}: the reference file isn't what MAME produces",
                    case.name
                ));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
