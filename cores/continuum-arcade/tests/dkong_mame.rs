//! The Donkey Kong boards against MAME, live: each of cArcade's test programs runs on the local
//! reference MAME (built from source, never shipped) and on cArcade's board. Every frame's picture
//! must match pixel for pixel, and on the sound programs every 8035 instruction must match MAME's
//! trace, and the sound must follow MAME's: where only the DAC sounds, sample for sample within
//! two steps, and everywhere by the measures tests/dkong/sound.rs holds it to. Then the committed
//! reference files must be what MAME produced, so tests/dkong_board.rs, which runs everywhere,
//! checks cArcade against MAME's real output.
//!
//! Without MAME it says SKIPPED and passes. CARCADE_WRITE_REFERENCES=1 rewrites the reference
//! files from MAME's runs; CARCADE_CASE=<name> runs one case.

mod dkong;
mod measure;

use dkong::*;

#[test]
#[allow(clippy::needless_range_loop)]
fn dkong_boards_match_mame() {
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
        let mut ours_trace = Vec::new();
        let ours = if case.sound.is_some() {
            run_ours(&case, Some(&mut |l: String| ours_trace.push(l)))
        } else {
            run_ours(&case, None)
        };
        match first_difference(&case, &ours, &theirs.video) {
            Some(d) => failures.push(d),
            None => eprintln!("{}: {} frames match MAME {version}", case.name, case.frames),
        }
        if case.sound.is_some() {
            match trace_difference(&ours_trace, &theirs.trace) {
                Some(d) => failures.push(format!("{}: {d}", case.name)),
                None => eprintln!("{}: all {} 8035 instructions match MAME's trace", case.name, theirs.trace.len()),
            }
        }
        let r = Reference::from_run(&theirs, &ours, case.sound.is_some());
        if case.sound.is_some() {
            let bad = sound::compare_run(&case, &ours, &r.levels, &r.segments);
            let audio: Vec<i16> = ours.iter().flat_map(|f| f.audio.iter().copied()).collect();
            let mut worst = (0, 0);
            for s in sound::segments(&ours).iter().filter(|s| sound::kind(&case, s.seg) == sound::Kind::Digital) {
                for k in (s.first - 1) * sound::SAMPLES..s.last * sound::SAMPLES {
                    let d = (i32::from(audio[k]) - i32::from(theirs.audio[k])).abs();
                    if d > worst.0 {
                        worst = (d, k);
                    }
                }
            }
            if worst.0 > 2 {
                failures.push(format!(
                    "{}: sample {} (frame {}) is {} steps from MAME's, where only the DAC sounds",
                    case.name,
                    worst.1,
                    worst.1 / sound::SAMPLES + 1,
                    worst.0
                ));
            }
            if bad.is_empty() {
                eprintln!("{}: the sound follows MAME's (where only the DAC sounds, at most {} steps off a sample)", case.name, worst.0);
            }
            failures.extend(bad);
        }
        if write {
            r.write(&case, &version);
        } else if Reference::read_text(&case) != r.render(&case, &version) {
            failures.push(format!("{}: the reference file isn't what MAME produces", case.name));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
