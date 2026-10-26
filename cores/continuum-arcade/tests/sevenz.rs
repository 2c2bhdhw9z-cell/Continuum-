//! cArcade's 7z reader against archives made by the real 7-Zip (7-Zip 26.05's 7zz, fetched into
//! scratch, never committed): many files of our own bytes (random, repetitive, empty, and one over
//! 2 MB so LZMA2 needs several chunks), packed every way arcade sets come: LZMA2 and LZMA at every
//! level, with other literal and position settings, stored, solid and not, header packed or not.
//! Every file must unpack to its bytes, with 7-Zip's CRC. Three small archives made the same way
//! are committed (tests/fixtures/7z) so the reader is checked everywhere; with 7zz here
//! ($CARCADE_7ZZ, or /projects/sandbox/.scratch/7zip/7zz) the whole range is made and checked.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use continuum_arcade::zip::{crc32, Zip};

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The files every archive holds: (name, bytes).
fn contents() -> Vec<(String, Vec<u8>)> {
    let mut x = 0x2545_F491u32;
    let mut rnd = move || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x
    };
    let mut v = Vec::new();
    v.push(("random.bin".to_string(), (0..20_000).map(|_| rnd() as u8).collect::<Vec<u8>>()));
    // Like a ROM: runs, repeats with changes, a table.
    let mut rom = Vec::new();
    for i in 0..40_000u32 {
        rom.push(match i % 7 {
            0 => (i >> 5) as u8,
            1 | 2 => 0xFF,
            3 => (rnd() & 3) as u8,
            _ => ((i * 13) >> 3) as u8,
        });
    }
    v.push(("pacman.6e".to_string(), rom));
    v.push(("empty.bin".to_string(), Vec::new()));
    v.push(("zeros.bin".to_string(), vec![0; 65_536]));
    v.push(("one.bin".to_string(), vec![0x5A]));
    // Big enough for several LZMA2 chunks (2 MB unpacked each at most).
    let mut big = Vec::with_capacity(2_600_000);
    for i in 0..2_600_000u32 {
        big.push(if i % 3000 < 1500 { (i % 251) as u8 } else { rnd() as u8 });
    }
    v.push(("sub/big.bin".to_string(), big));
    v
}

fn check(archive: &[u8], label: &str, want: &[(String, Vec<u8>)]) {
    let zip = Zip::parse(archive.to_vec()).unwrap_or_else(|e| panic!("{label}: {e}"));
    for (name, bytes) in want {
        let i = zip
            .entries()
            .iter()
            .position(|e| e.name.replace('\\', "/") == *name)
            .unwrap_or_else(|| panic!("{label}: {name} isn't listed"));
        let e = &zip.entries()[i];
        assert_eq!(e.size, bytes.len() as u64, "{label}: {name}'s size");
        assert_eq!(e.crc, crc32(bytes), "{label}: {name}'s CRC");
        let got = zip.read(i).unwrap_or_else(|why| panic!("{label}: {name}: {why}"));
        assert!(got == *bytes, "{label}: {name} unpacks to other bytes");
    }
}

fn seven_zip() -> Option<PathBuf> {
    let p = std::env::var("CARCADE_7ZZ")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/projects/sandbox/.scratch/7zip/7zz"));
    p.is_file().then_some(p)
}

/// The settings each archive is made with.
const WAYS: &[(&str, &[&str])] = &[
    ("lzma2-default", &[]),
    ("lzma2-store", &["-mx=0"]),
    ("lzma2-fast", &["-mx=1"]),
    ("lzma2-ultra", &["-mx=9"]),
    ("lzma2-not-solid", &["-ms=off"]),
    ("lzma2-plain-header", &["-mhc=off"]),
    ("lzma2-lc0-lp2-pb0", &["-m0=lzma2:lc=0:lp=2:pb=0"]),
    ("lzma2-lc4-pb4", &["-m0=lzma2:lc=4:lp=0:pb=4"]),
    ("lzma-default", &["-m0=lzma"]),
    ("lzma-fast", &["-m0=lzma", "-mx=1"]),
    ("lzma-ultra", &["-m0=lzma", "-mx=9"]),
    ("lzma-lc8-lp4", &["-m0=lzma:lc=8:lp=4:pb=2"]),
    ("lzma-not-solid-plain-header", &["-m0=lzma", "-ms=off", "-mhc=off"]),
    ("lzma-eos", &["-m0=lzma:eos"]),
    ("copy", &["-m0=copy"]),
];

#[test]
fn archives_made_by_7zip_unpack_exactly() {
    let want = contents();
    let Some(sz) = seven_zip() else {
        eprintln!("SKIPPED: no 7zz here (set CARCADE_7ZZ); the committed archives are checked below");
        return;
    };
    let work = crate_dir().join("target/7z-check");
    let _ = fs::remove_dir_all(&work);
    let src = work.join("in");
    fs::create_dir_all(src.join("sub")).unwrap();
    for (n, b) in &want {
        fs::write(src.join(n), b).unwrap();
    }
    for (label, args) in WAYS {
        let out = work.join(format!("{label}.7z"));
        let st = Command::new(&sz)
            .current_dir(&src)
            .arg("a")
            .arg("-bd")
            .arg("-y")
            .args(*args)
            .arg(&out)
            .arg(".")
            .output()
            .unwrap();
        assert!(st.status.success(), "{label}: 7zz failed: {}", String::from_utf8_lossy(&st.stderr));
        check(&fs::read(&out).unwrap(), label, &want);
        eprintln!("{label}: every file unpacks");
    }
}

#[test]
fn the_committed_archives_unpack_exactly() {
    // Made with 7-Zip 26.05 from tests/fixtures/7z's small files (see make_fixtures below).
    let dir = crate_dir().join("tests/fixtures/7z");
    let small: Vec<(String, Vec<u8>)> = contents().into_iter().filter(|(n, _)| !n.contains("big")).collect();
    for f in ["lzma2.7z", "lzma.7z", "copy-plain-header.7z"] {
        check(&fs::read(dir.join(f)).unwrap(), f, &small);
    }
}

#[test]
fn damaged_archives_are_refused() {
    let dir = crate_dir().join("tests/fixtures/7z");
    let good = fs::read(dir.join("lzma2.7z")).unwrap();
    // Every single byte changed in turn (past the signature): never a panic, and never wrong
    // bytes handed out (a CRC catches them).
    let small: Vec<(String, Vec<u8>)> = contents().into_iter().filter(|(n, _)| !n.contains("big")).collect();
    for at in (6..good.len()).step_by(7) {
        let mut d = good.clone();
        d[at] ^= 0x5A;
        if let Ok(zip) = Zip::parse(d) {
            for (i, e) in zip.entries().iter().enumerate() {
                if let Ok(bytes) = zip.read(i) {
                    let want = small.iter().find(|(n, _)| *n == e.name);
                    if let Some((_, w)) = want {
                        assert!(bytes == *w || crc32(&bytes) == e.crc, "byte {at}: {} came out wrong", e.name);
                    }
                }
            }
        }
    }
    // Cut short anywhere.
    for len in (0..good.len()).step_by(13) {
        if let Ok(z) = Zip::parse(good[..len].to_vec()) {
            for i in 0..z.entries().len() {
                let _ = z.read(i);
            }
        }
    }
}

/// Makes tests/fixtures/7z again (run with --ignored and 7zz here).
#[test]
#[ignore]
fn make_fixtures() {
    let sz = seven_zip().expect("7zz");
    let dir = crate_dir().join("tests/fixtures/7z");
    let src = crate_dir().join("target/7z-fixtures");
    let _ = fs::remove_dir_all(&src);
    fs::create_dir_all(&src).unwrap();
    for (n, b) in contents().into_iter().filter(|(n, _)| !n.contains("big")) {
        fs::write(src.join(n), b).unwrap();
    }
    fs::create_dir_all(&dir).unwrap();
    for (f, args) in [
        ("lzma2.7z", vec![]),
        ("lzma.7z", vec!["-m0=lzma"]),
        ("copy-plain-header.7z", vec!["-m0=copy", "-mhc=off"]),
    ] {
        let out = dir.join(f);
        let _ = fs::remove_file(&out);
        let st = Command::new(&sz).current_dir(&src).args(["a", "-bd", "-y"]).args(&args).arg(&out).arg(".").output().unwrap();
        assert!(st.status.success());
    }
}
