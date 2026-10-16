//! cArcade through its libretro entry points, the way the app drives it: each of the Pac-Man
//! board's test programs (test-programs/pacman) is zipped under the file names of MAME's set
//! (pacman.zip, mspacman.zip, pacplus.zip, mspacmab.zip), loaded with `retro_load_game`, and run
//! with `retro_run`, and every frame's picture must hash to what MAME 0.289 drew for the same
//! program (tests/references/pacman). The inputs program is driven through the frontend's pads
//! and the DIP switch settings, so the controls and switches are held to MAME's too.
//!
//! The Galaxian family's programs (test-programs/galaxian and test-programs/konami) go through the
//! entry points too, zipped as MAME's galaxian, mooncrst, mooncrstuu, scramble, theend, scobra,
//! frogger and amidar sets: each frame the core hands out must be the board's picture averaged to
//! 224 x 256, from a board whose own frames are MAME's (tests/galaxian_board.rs and
//! tests/konami_board.rs), and on the frames before any input changes, MAME's recorded hashes.
//!
//! Also: the sets' zips (split, with the parent next to it), the refusals a player reads, save
//! states, Restart, the settings, and that the app files every set cArcade knows as arcade.

mod common;
mod galaxian;
mod konami;
mod measure;

use std::collections::{HashMap, HashSet};
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use common::*;
use continuum_arcade::libretro::*;
use continuum_arcade::pacman::{Pacman, Ports};

/// The core is one global: tests take turns.
static SERIAL: Mutex<()> = Mutex::new(());
/// What the frontend answers for each setting, and whether one changed.
static OPTS: Mutex<Option<HashMap<String, CString>>> = Mutex::new(None);
static UPDATED: Mutex<bool> = Mutex::new(false);
static DECLARED: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static HIDDEN: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static DISPLAY_CB: Mutex<Option<unsafe extern "C" fn() -> bool>> = Mutex::new(None);
/// The two pads: one bit per libretro joypad button.
static PADS: Mutex<[u16; 2]> = Mutex::new([0; 2]);
struct Out {
    frames: Vec<(u32, u32, Vec<u32>)>,
    samples: Vec<i16>,
}
static OUT: Mutex<Out> = Mutex::new(Out { frames: Vec::new(), samples: Vec::new() });

#[repr(C)]
struct Var {
    key: *const c_char,
    value: *const c_char,
}
#[repr(C)]
struct OptValue {
    value: *const c_char,
    label: *const c_char,
}
#[repr(C)]
struct OptDef {
    key: *const c_char,
    desc: *const c_char,
    desc_categorized: *const c_char,
    info: *const c_char,
    info_categorized: *const c_char,
    category_key: *const c_char,
    values: [OptValue; 128],
    default_value: *const c_char,
}
#[repr(C)]
struct OptionsV2 {
    categories: *const c_void,
    definitions: *const OptDef,
}
#[repr(C)]
struct Display {
    key: *const c_char,
    visible: bool,
}

unsafe extern "C" fn env(cmd: u32, d: *mut c_void) -> bool {
    match cmd {
        10 => true,
        15 => {
            let var = &mut *(d as *mut Var);
            let key = CStr::from_ptr(var.key).to_string_lossy().into_owned();
            let opts = OPTS.lock().unwrap();
            match opts.as_ref().and_then(|o| o.get(&key)) {
                Some(v) => {
                    var.value = v.as_ptr();
                    true
                }
                None => false,
            }
        }
        17 => {
            *(d as *mut bool) = std::mem::take(&mut *UPDATED.lock().unwrap());
            true
        }
        55 => {
            let disp = &*(d as *const Display);
            let key = CStr::from_ptr(disp.key).to_string_lossy().into_owned();
            let mut hidden = HIDDEN.lock().unwrap();
            let h = hidden.get_or_insert_with(HashSet::new);
            if disp.visible {
                h.remove(&key);
            } else {
                h.insert(key);
            }
            true
        }
        67 => {
            let opts = &*(d as *const OptionsV2);
            let mut decl = DECLARED.lock().unwrap();
            decl.clear();
            let mut def = opts.definitions;
            while !(*def).key.is_null() {
                let key = CStr::from_ptr((*def).key).to_string_lossy().into_owned();
                let dflt = CStr::from_ptr((*def).default_value).to_string_lossy().into_owned();
                decl.push((key, dflt));
                def = def.add(1);
            }
            true
        }
        69 => {
            let cb = &*(d as *const Option<unsafe extern "C" fn() -> bool>);
            *DISPLAY_CB.lock().unwrap() = *cb;
            true
        }
        _ => false,
    }
}

unsafe extern "C" fn video(d: *const c_void, w: u32, h: u32, pitch: usize) {
    let px = std::slice::from_raw_parts(d as *const u8, pitch * h as usize);
    let frame: Vec<u32> = (0..h as usize)
        .flat_map(|y| {
            (0..w as usize).map(move |x| {
                let o = y * pitch + x * 4;
                u32::from_le_bytes([px[o], px[o + 1], px[o + 2], px[o + 3]])
            })
        })
        .collect();
    OUT.lock().unwrap().frames.push((w, h, frame));
}

unsafe extern "C" fn audio(d: *const i16, n: usize) -> usize {
    OUT.lock().unwrap().samples.extend_from_slice(std::slice::from_raw_parts(d, n * 2));
    n
}

unsafe extern "C" fn poll() {}

unsafe extern "C" fn input(port: u32, device: u32, _index: u32, id: u32) -> i16 {
    if device != 1 || port > 1 {
        return 0;
    }
    i16::from(PADS.lock().unwrap()[port as usize] >> id & 1 != 0)
}

/// A fresh frontend: no settings chosen, nothing pressed, then the core's environment.
fn frontend() {
    *OPTS.lock().unwrap() = Some(HashMap::new());
    *UPDATED.lock().unwrap() = false;
    *HIDDEN.lock().unwrap() = Some(HashSet::new());
    *PADS.lock().unwrap() = [0; 2];
    retro_set_environment(env);
    retro_set_video_refresh(video);
    retro_set_audio_sample_batch(audio);
    retro_set_input_poll(poll);
    retro_set_input_state(input);
    retro_init();
}

fn set_option(key: &str, value: &str) {
    let mut opts = OPTS.lock().unwrap();
    let o = opts.get_or_insert_with(HashMap::new);
    if o.get(key).map(|v| v.to_str().unwrap()) != Some(value) {
        o.insert(key.into(), CString::new(value).unwrap());
        *UPDATED.lock().unwrap() = true;
    }
}

fn work_dir(name: &str) -> PathBuf {
    let d = crate_dir().join("target/libretro-test").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_zip(path: &Path, files: &[(String, Vec<u8>)]) {
    let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    std::fs::write(path, continuum_arcade::zip::write_stored(&refs)).unwrap();
}

/// `retro_load_game` with the zip's bytes and its path, as the app hands a game over.
fn load(path: &Path) -> bool {
    let bytes = std::fs::read(path).unwrap();
    let cpath = CString::new(path.to_str().unwrap()).unwrap();
    let info = RetroGameInfo {
        path: cpath.as_ptr(),
        data: bytes.as_ptr() as *const c_void,
        size: bytes.len(),
        meta: std::ptr::null(),
    };
    OUT.lock().unwrap().frames.clear();
    unsafe { retro_load_game(&info) }
}

fn run(frames: u32) -> Vec<(u32, u32, Vec<u32>)> {
    OUT.lock().unwrap().frames.clear();
    for _ in 0..frames {
        unsafe { retro_run() };
    }
    std::mem::take(&mut OUT.lock().unwrap().frames)
}

fn case(name: &str) -> Case {
    cases().into_iter().find(|c| c.name == name).unwrap()
}

/// Loads `case`'s program zipped as its MAME set in a folder of its own.
fn load_case(case: &Case) -> bool {
    let dir = work_dir(case.name);
    let zip = dir.join(format!("{}.zip", case.set));
    write_zip(&zip, &files(case));
    load(&zip)
}

#[test]
fn every_program_plays_through_the_entry_points_as_on_mame() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut failures = Vec::new();
    for case in cases().into_iter().filter(|c| c.name != "inputs") {
        frontend();
        assert!(load_case(&case), "{}: {}", case.name, last_log_line());
        let want = read_reference(&case);
        let mut board = Pacman::new(case.variant, &roms(&case)).unwrap();
        board.set_ports(Ports::default());
        OUT.lock().unwrap().samples.clear();
        let got = run(case.frames);
        let sound = std::mem::take(&mut OUT.lock().unwrap().samples);
        let mut bad = Vec::new();
        for (i, (w, h, frame)) in got.iter().enumerate() {
            assert_eq!((*w, *h), (224, 288), "{}: the picture is upright", case.name);
            if video_hash(frame) != want[i].0 {
                bad.push(i + 1);
            }
            // The sound is the board's, both channels, 792 samples a frame.
            board.run_frame();
            let mine = &sound[i * 792 * 2..(i + 1) * 792 * 2];
            let theirs: Vec<i16> = board.audio().iter().flat_map(|&s| [s, s]).collect();
            if mine != theirs.as_slice() {
                failures.push(format!("{}: frame {}'s sound isn't the board's", case.name, i + 1));
                break;
            }
        }
        if bad.is_empty() {
            eprintln!("{}: all {} frames match MAME's through the entry points", case.name, case.frames);
        } else {
            failures.push(format!("{}: {} of {} frames differ from MAME's, the first {}", case.name, bad.len(), case.frames, bad[0]));
        }
        retro_unload_game();
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// The inputs program, driven the way a player would: the stick, coin and start on the frontend's
/// pads, and the DIP switches and cabinet through the settings. Every frame must be MAME's, except
/// while the test has a switch the core doesn't offer turned on (the rack test, the service coin
/// and the service mode switch).
#[test]
fn the_controls_and_switches_reach_the_board_as_on_mame() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    frontend();
    let case = case("inputs");
    assert!(load_case(&case), "{}", last_log_line());
    let want = read_reference(&case);
    let (mut in0, mut in1, mut dsw1) = (0xFFu8, 0xFFu8, 0xC9u8);
    let (mut compared, mut skipped) = (0, 0);
    for frame in 1..=case.frames {
        for &(f, e) in &case.events {
            if f != frame {
                continue;
            }
            let (port, f): (Port, Box<dyn Fn(u8) -> u8>) = match e {
                Event::Press(p, b) => (p, Box::new(move |r| r & !b)),
                Event::Release(p, b) => (p, Box::new(move |r| r | b)),
                Event::Dip(p, m, v) => (p, Box::new(move |r| (r & !m) | (v & m))),
            };
            match port {
                Port::In0 => in0 = f(in0),
                Port::In1 => in1 = f(in1),
                Port::Dsw1 => dsw1 = f(dsw1),
            }
        }
        let low = |r: u8, b: u8| r & b == 0;
        // libretro joypad bits: B 0, Y 1, Select 2, Start 3, Up 4, Down 5, Left 6, Right 7.
        let pad = |up: bool, down: bool, left: bool, right: bool, select: bool, start: bool| {
            u16::from(select) << 2 | u16::from(start) << 3 | u16::from(up) << 4 | u16::from(down) << 5
                | u16::from(left) << 6 | u16::from(right) << 7
        };
        *PADS.lock().unwrap() = [
            pad(low(in0, 1), low(in0, 8), low(in0, 2), low(in0, 4), low(in0, 0x20), low(in1, 0x20)),
            pad(low(in1, 1), low(in1, 8), low(in1, 2), low(in1, 4), low(in0, 0x40), low(in1, 0x40)),
        ];
        set_option("carcade_pacman_coinage", ["free", "1c1g", "1c2g", "2c1g"][usize::from(dsw1 & 3)]);
        set_option("carcade_pacman_lives", ["1", "2", "3", "5"][usize::from(dsw1 >> 2 & 3)]);
        set_option("carcade_pacman_bonus", ["10000", "15000", "20000", "none"][usize::from(dsw1 >> 4 & 3)]);
        set_option("carcade_pacman_difficulty", if dsw1 & 0x40 != 0 { "normal" } else { "hard" });
        set_option("carcade_pacman_ghosts", if dsw1 & 0x80 != 0 { "usual" } else { "alternate" });
        set_option("carcade_pacman_cabinet", if in1 & 0x80 != 0 { "upright" } else { "cocktail" });
        let got = run(1);
        // The switches cArcade doesn't offer stay off.
        if low(in0, 0x10) || low(in0, 0x80) || low(in1, 0x10) {
            skipped += 1;
            continue;
        }
        compared += 1;
        assert_eq!(
            video_hash(&got[0].2),
            want[frame as usize - 1].0,
            "frame {frame}: in0 {in0:02x} in1 {in1:02x} dsw1 {dsw1:02x}"
        );
    }
    eprintln!("inputs: {compared} frames match MAME's ({skipped} left out with a switch cArcade doesn't offer on)");
    assert!(compared > 130);
    retro_unload_game();
}

#[test]
fn save_states_and_restart_through_the_entry_points() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    frontend();
    let case = case("sprites");
    assert!(load_case(&case));
    let fresh = run(10);
    run(15);
    let size = retro_serialize_size();
    let mut state = vec![0u8; size];
    assert!(unsafe { retro_serialize(state.as_mut_ptr() as *mut c_void, size) });
    let straight = run(30);
    assert!(unsafe { retro_unserialize(state.as_ptr() as *const c_void, size) });
    let mut again_state = vec![0u8; size];
    assert!(unsafe { retro_serialize(again_state.as_mut_ptr() as *mut c_void, size) });
    assert!(state == again_state, "the state reads back byte for byte");
    let again = run(30);
    assert!(straight == again, "the frames after a restore are the same frames");
    // Someone else's bytes are refused and change nothing.
    let junk = vec![7u8; size];
    assert!(!unsafe { retro_unserialize(junk.as_ptr() as *const c_void, size) });
    // Restart is the power switch: the frames after it are a fresh start's.
    retro_reset();
    assert!(run(10) == fresh, "Restart starts the game afresh");
    retro_unload_game();
    // A state of one set doesn't load into another.
    frontend();
    assert!(load_case(&self::case("mspacman")));
    assert!(!unsafe { retro_unserialize(state.as_ptr() as *const c_void, size) });
    retro_unload_game();
}

#[test]
fn a_split_clone_plays_with_its_parent_next_to_it() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    frontend();
    let case = case("video");
    let dir = work_dir("split");
    let all = files(&case);
    // pacman.zip as a split set: only the chips that differ from Puck Man's.
    let own: Vec<(String, Vec<u8>)> = all.iter().filter(|(n, _)| n.starts_with("pacman.")).cloned().collect();
    write_zip(&dir.join("pacman.zip"), &own);
    assert!(!load(&dir.join("pacman.zip")));
    assert_eq!(last_log_line(), "This one needs puckman.zip next to it.");
    // Puck Man's zip next to it, its PROMs under its own names (any letter case).
    let prom = |n: &str| all.iter().find(|(m, _)| m == n).unwrap().1.clone();
    let parent = vec![
        ("pm1-1.7f".to_string(), prom("82s123.7f")),
        ("pm1-4.4a".to_string(), prom("82s126.4a")),
        ("pm1-3.1m".to_string(), prom("82s126.1m")),
        ("pm1-2.3m".to_string(), prom("82s126.3m")),
    ];
    write_zip(&dir.join("PuckMan.zip"), &parent);
    assert!(load(&dir.join("pacman.zip")), "{}", last_log_line());
    let want = read_reference(&case);
    let got = run(case.frames);
    assert!(got.iter().zip(&want).all(|(g, w)| video_hash(&g.2) == w.0), "frame for frame as MAME");
    retro_unload_game();
}

#[test]
fn a_game_it_cant_play_is_one_plain_sentence() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    frontend();
    let dir = work_dir("refusals");
    let refuse = |name: &str, files: &[(String, Vec<u8>)]| -> String {
        let p = dir.join(name);
        write_zip(&p, files);
        assert!(!load(&p), "{name} loaded");
        last_log_line()
    };
    let plays = "Our arcade emulator can\u{2019}t play this one yet. Right now it plays Pac-Man, Ms. Pac-Man, Galaxian, Scramble, Frogger, Amidar and games on the same boards.";
    assert_eq!(refuse("sf2.zip", &[("sf2_30a.bin".into(), vec![1; 4096])]), plays);
    assert_eq!(refuse("my game.zip", &[]), plays);
    let mut short = files(&case("video"));
    short.retain(|(n, _)| n != "pacman.6h");
    assert_eq!(
        refuse("pacman.zip", &short),
        "This copy of Pac-Man is missing a piece. A complete copy of pacman.zip fixes it."
    );
    let p = dir.join("galaxian.zip");
    std::fs::write(&p, b"not a zip").unwrap();
    assert!(!load(&p));
    assert_eq!(
        last_log_line(),
        "This file isn\u{2019}t a working arcade zip. Another copy of the game usually fixes it."
    );
    // A game that won't play leaves nothing loaded behind it.
    assert_eq!(retro_serialize_size(), 0);
}

#[test]
fn the_switches_are_settings_and_only_the_games_own_show() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    frontend();
    let declared = DECLARED.lock().unwrap().clone();
    let keys: Vec<&str> = declared.iter().map(|(k, _)| k.as_str()).collect();
    for k in ["coinage", "lives", "bonus", "difficulty", "ghosts", "cabinet"] {
        assert!(keys.contains(&format!("carcade_pacman_{k}").as_str()), "{k}: {keys:?}");
    }
    assert!(declared.contains(&("carcade_pacman_lives".into(), "3".into())));
    // Ms. Pac-Man has no ghost names switch; the other boards' switches are all hidden.
    assert!(load_case(&case("mspacman")));
    let others: HashSet<String> = keys.iter().filter(|k| !k.starts_with("carcade_pacman_")).map(|k| k.to_string()).collect();
    assert!(others.len() > 50, "the Galaxian family's switches: {}", others.len());
    let hidden = || HIDDEN.lock().unwrap().clone().unwrap_or_default();
    let mut want = others.clone();
    want.insert("carcade_pacman_ghosts".to_string());
    assert_eq!(hidden(), want);
    // The frontend asks again before it lists them: the same answer.
    HIDDEN.lock().unwrap().replace(HashSet::new());
    let cb = DISPLAY_CB.lock().unwrap().expect("a display callback");
    assert!(unsafe { cb() });
    assert_eq!(hidden(), want);
    retro_unload_game();
    assert!(load_case(&case("video")));
    assert_eq!(hidden(), others, "Pac-Man has every Pac-Man switch");
    retro_unload_game();
    // Scramble shows its own switches and only those.
    let k = konami::cases().into_iter().find(|c| c.name == "scramble").unwrap();
    let dir = work_dir("scramble-switches");
    let files: Vec<(String, Vec<u8>)> = konami::files(&k).into_iter().map(|(n, _, _, d)| (n, d)).collect();
    write_zip(&dir.join("scramble.zip"), &files);
    assert!(load(&dir.join("scramble.zip")), "{}", last_log_line());
    let shown: Vec<&str> = keys.iter().copied().filter(|k| !hidden().contains(*k)).collect();
    assert_eq!(shown, ["carcade_scramble_lives", "carcade_scramble_coins", "carcade_scramble_cabinet"]);
    retro_unload_game();
    assert!(load_case(&case("video")));
    // The picture, its shape and its pace.
    let mut av = std::mem::MaybeUninit::<RetroSystemAvInfo>::uninit();
    let av = unsafe {
        retro_get_system_av_info(av.as_mut_ptr());
        av.assume_init()
    };
    assert_eq!((av.geometry.base_width, av.geometry.base_height), (224, 288));
    assert_eq!(av.geometry.aspect_ratio, 0.75);
    assert!((av.timing.fps - 6_144_000.0 / (384.0 * 264.0)).abs() < 1e-9);
    assert_eq!(av.timing.sample_rate, 48_000.0);
    let mut sys = std::mem::MaybeUninit::<RetroSystemInfo>::uninit();
    let sys = unsafe {
        retro_get_system_info(sys.as_mut_ptr());
        sys.assume_init()
    };
    let text = |p: *const c_char| unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_string();
    assert_eq!(text(sys.library_name), "cArcade");
    assert_eq!(text(sys.valid_extensions), "zip");
    assert!(!sys.need_fullpath);
    retro_unload_game();
}

/// The app files a zip as arcade without asking "Which system?" when it knows the set's name
/// (crates/emulator-bridge/src/import/detect.rs), and some of these sets' zips don't look like
/// rom chips to it, so every set cArcade plays must be in that list.
#[test]
fn the_app_files_every_set_it_plays_as_arcade() {
    let detect = crate_dir().join("../../crates/emulator-bridge/src/import/detect.rs");
    let Ok(text) = std::fs::read_to_string(&detect) else {
        eprintln!("SKIPPED: no {} here", detect.display());
        return;
    };
    let missing: Vec<&str> = continuum_arcade::sets::SETS
        .iter()
        .map(|s| s.name)
        .filter(|n| !text.contains(&format!("\"{n}\"")))
        .collect();
    assert!(missing.is_empty(), "not in detect.rs's arcade sets: {missing:?}");
}

/// The Galaxian family's test programs through the entry points: each zipped as its MAME set.
#[test]
fn the_galaxian_family_plays_through_the_entry_points() {
    use continuum_arcade::board::galaxian_picture;
    use continuum_arcade::galaxian::{Galaxian, Rotation};
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // (name, set, files, roms, variant, MAME's hashes, the first frame inputs change on)
    let mut runs = Vec::new();
    for c in galaxian::cases() {
        let files = galaxian::files(&c);
        let first = c.events.first().map_or(u32::MAX, |e| e.0);
        runs.push((c.name.to_string(), c.set, files, galaxian::roms(&c), c.variant, galaxian::read_reference(&c), first, c.frames.min(120)));
    }
    for c in konami::cases() {
        let files = konami::files(&c).into_iter().map(|(n, _, _, d)| (n, d)).collect();
        let first = c.events.first().map_or(u32::MAX, |e| e.0);
        runs.push((c.name.to_string(), c.set, files, konami::roms(&c), c.variant, konami::read_reference(&c).video, first, 120));
    }
    let mut failures = Vec::new();
    for (name, set, files, roms, variant, want, first, frames) in runs {
        frontend();
        let dir = work_dir(&format!("galaxian-{name}"));
        let zip = dir.join(format!("{set}.zip"));
        write_zip(&zip, &files);
        assert!(load(&zip), "{name}: {}", last_log_line());
        // The board as the core runs it: the set's ports at rest, with the factory switches.
        let spec = continuum_arcade::sets::find(set).unwrap();
        let mut board = Galaxian::new(variant, Rotation::Rot90, &roms).unwrap();
        let p = spec.rest;
        board.set_ports(continuum_arcade::galaxian::Ports { in0: p[0], in1: p[1], in2: p[2], in3: p[3] });
        OUT.lock().unwrap().samples.clear();
        let got = run(frames);
        let sound = std::mem::take(&mut OUT.lock().unwrap().samples);
        let mut picture = vec![0u32; 224 * 256];
        for (i, (w, h, frame)) in got.iter().enumerate() {
            assert_eq!((*w, *h), (224, 256), "{name}: the picture");
            board.run_frame();
            if (i as u32) + 1 < first && galaxian::video_hash(board.video()) != want[i] {
                failures.push(format!("{name}: frame {} isn't MAME's", i + 1));
                break;
            }
            galaxian_picture(board.video(), &mut picture);
            if *frame != picture {
                failures.push(format!("{name}: frame {} isn't the board's picture", i + 1));
                break;
            }
            let theirs: Vec<i16> = board.audio().iter().flat_map(|&s| [s, s]).collect();
            if sound[i * 792 * 2..(i + 1) * 792 * 2] != theirs[..] {
                failures.push(format!("{name}: frame {}'s sound isn't the board's", i + 1));
                break;
            }
        }
        eprintln!("{name}: {} frames through the entry points", got.len());
        retro_unload_game();
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
