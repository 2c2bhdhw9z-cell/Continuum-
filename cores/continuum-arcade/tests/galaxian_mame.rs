//! The Galaxian board family against MAME, live: each of cArcade's test programs runs on the local
//! reference MAME (built from source, never shipped; see tests/galaxian) and on cArcade's board,
//! and every frame's picture must match pixel for pixel. Then the committed reference files must
//! be what MAME produced, so tests/galaxian_board.rs, which runs everywhere, checks cArcade against
//! MAME's real output.
//!
//! The sound is compared too (galaxian/sound.rs): Galaxian's and Moon Cresta's analogue sound,
//! which cArcade approximates, segment by segment against MAME's model of the circuit.
//!
//! Without MAME it says SKIPPED and passes. CARCADE_WRITE_REFERENCES=1 rewrites the reference
//! files from MAME's runs; CARCADE_CASE=<name> runs one case.

mod galaxian;

use galaxian::*;

#[test]
fn galaxian_boards_match_mame() {
    let Some(mame) = mame() else {
        eprintln!("SKIPPED: no reference MAME here (set CARCADE_MAME to its binary)");
        return;
    };
    let version = mame_version(&mame);
    let write = std::env::var("CARCADE_WRITE_REFERENCES").is_ok_and(|v| v == "1");
    let only = std::env::var("CARCADE_CASE").ok();
    let mut failures = Vec::new();
    for case in cases() {
        if only.as_deref().is_some_and(|o| o != case.name) {
            continue;
        }
        let theirs = run_mame(&mame, &case);
        let ours = run_ours(&case);
        if let Some(d) = first_difference(&case, &ours, &theirs) {
            failures.push(d);
        } else {
            eprintln!("{}: {} frames match MAME {version}", case.name, case.frames);
        }
        if write {
            write_reference(&case, &theirs, &version);
        } else {
            let want = read_reference(&case);
            let got: Vec<u64> = theirs.iter().map(|f| video_hash(f)).collect();
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

#[test]
fn galaxian_sound_matches_mame() {
    let Some(mame) = mame() else {
        eprintln!("SKIPPED: no reference MAME here (set CARCADE_MAME to its binary)");
        return;
    };
    let version = mame_version(&mame);
    let write = std::env::var("CARCADE_WRITE_REFERENCES").is_ok_and(|v| v == "1");
    let mut failures = Vec::new();
    for case in sound::cases() {
        let theirs = sound::run_mame(&mame, &case);
        let ours = sound::run_ours(&case);
        let mut measures = Vec::new();
        for seg in sound::segments() {
            let (o, t) = (sound::measure(&ours, &seg), sound::measure(&theirs, &seg));
            eprintln!(
                "{}: {:24} ours {:.4} V {:8.1} Hz, MAME {:.4} V {:8.1} Hz, alike {:.3} / octaves {:.3}",
                case.name,
                seg.name,
                o.rms,
                o.freq,
                t.rms,
                t.freq,
                sound::likeness(&o.fine, &t.fine),
                sound::likeness(&o.octaves, &t.octaves)
            );
            if let Err(e) = sound::compare(&seg, &o, &t) {
                failures.push(format!("{}: {e}", case.name));
            }
            measures.push(t);
        }
        if write {
            sound::write_reference(&case, &measures, &version);
        } else {
            let want = sound::read_reference(&case);
            // The file keeps 6 digits of the level, 3 decimals of the frequency and 5 of each band.
            let close = want.len() == measures.len()
                && want.iter().zip(&measures).all(|(w, m)| {
                    (w.rms - m.rms).abs() < 1e-5
                        && (w.freq - m.freq).abs() < 0.01
                        && w.fine.iter().zip(&m.fine).all(|(a, b)| (a - b).abs() <= 1e-4 * b.abs() + 1e-30)
                        && w.octaves.iter().zip(&m.octaves).all(|(a, b)| (a - b).abs() <= 1e-4 * b.abs() + 1e-30)
                });
            if !close {
                failures.push(format!("{}: the reference file isn't what MAME produces", case.name));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
