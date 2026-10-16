//! Konami's Scramble-family boards against MAME, live: each of cArcade's test programs runs on the
//! local reference MAME and on cArcade (tests/konami). Every frame's picture must match pixel for
//! pixel, and the sound Z80's every instruction must match MAME's trace (cycle count and
//! registers). The sound, which goes through filters cArcade approximates, is compared segment by
//! segment by level and spectrum. Then the reference files must be what MAME produced, so
//! tests/konami_board.rs, which runs everywhere, checks cArcade against MAME's real output.
//!
//! Without MAME it says SKIPPED and passes. CARCADE_WRITE_REFERENCES=1 rewrites the reference
//! files; CARCADE_CASE=<name> runs one case.

mod konami;
mod measure;

use konami::*;

#[test]
fn konami_boards_match_mame() {
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
        let mut lines = Vec::new();
        let ours = run_ours(&case, Some(&mut |l| lines.push(l)));
        let p = &case.ports;
        if theirs.ports != [p.in0, p.in1, p.in2, p.in3] {
            failures.push(format!("{}: MAME's ports start as {:02X?}, the test has {:02X?}", case.name, theirs.ports, [p.in0, p.in1, p.in2, p.in3]));
        }
        // The picture.
        let dir = crate_dir().join("target/mame-runs/konami").join(case.name);
        match ours.iter().zip(&theirs.video).position(|(o, t)| &o.video != t) {
            None => eprintln!("{}: all {} frames match MAME {version}", case.name, case.frames),
            Some(i) => {
                let (o, t) = (&ours[i].video, &theirs.video[i]);
                let n = o.iter().zip(t).filter(|(a, b)| a != b).count();
                let px = o.iter().zip(t).position(|(a, b)| a != b).unwrap();
                write_png(&dir.join(format!("ours-{:05}.png", i + 1)), o);
                write_png(&dir.join(format!("mame-{:05}.png", i + 1)), t);
                failures.push(format!(
                    "{}: frame {} picture: {n} pixels differ, the first at ({}, {}): ours {:06x}, MAME {:06x}",
                    case.name, i + 1, px % 224, px / 224, o[px], t[px]
                ));
            }
        }
        // The sound CPU.
        let mame_lines = read_mame_trace(&theirs.trace);
        // MAME's trace starts once the Lua script has run.
        let skip = mame_lines.first().and_then(|m| lines.iter().position(|l| l == m)).unwrap_or(0);
        let ours_lines = &lines[skip..];
        let n = mame_lines.len().min(ours_lines.len());
        match (0..n).find(|&i| mame_lines[i] != ours_lines[i]) {
            None if n > 100_000 => eprintln!("{}: the sound CPU's {n} instructions match MAME's", case.name),
            None => failures.push(format!("{}: only {n} sound CPU instructions to compare", case.name)),
            Some(i) => {
                let from = i.saturating_sub(3);
                failures.push(format!(
                    "{}: sound CPU instruction {i} differs\n  MAME {:?}\n  ours {:?}",
                    case.name,
                    &mame_lines[from..(i + 2).min(mame_lines.len())],
                    &ours_lines[from..(i + 2).min(ours_lines.len())]
                ));
            }
        }
        // The sound.
        let mut sound = Vec::new();
        for seg in compared_segments(&ours) {
            let mo = our_measure(&ours, seg);
            let mt = measure::measure(&theirs.audio[seg.0 * 792..seg.1 * 792], 48_000.0);
            eprintln!(
                "{}: frames {}-{}: ours {:.4} {:7.1} Hz, MAME {:.4} {:7.1} Hz, alike {:.3}",
                case.name, seg.0 + 1, seg.1, mo.rms, mo.freq, mt.rms, mt.freq, measure::likeness(&mo.fine, &mt.fine)
            );
            if let Err(e) = compare_sound(case.name, seg, &mo, &mt) {
                failures.push(e);
            }
            sound.push((seg, mt));
        }
        let reference = Reference {
            ports: theirs.ports,
            video: theirs.video.iter().map(|v| video_hash(v)).collect(),
            // MAME traces a little past the frames compared; the reference keeps what was compared.
            trace_first: mame_lines.first().cloned().unwrap_or_default(),
            trace_lines: n,
            trace: trace_hashes(&mame_lines[..n]),
            sound,
        };
        if write {
            write_reference(&case, &reference, &version);
        } else {
            let want = read_reference(&case);
            let same = want.ports == reference.ports
                && want.video == reference.video
                && want.trace == reference.trace
                && want.trace_first == reference.trace_first
                && want.sound.len() == reference.sound.len()
                && want.sound.iter().zip(&reference.sound).all(|(w, r)| w.0 == r.0 && (w.1.rms - r.1.rms).abs() < 1e-5);
            if !same {
                failures.push(format!("{}: the reference file isn't what MAME produces", case.name));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
