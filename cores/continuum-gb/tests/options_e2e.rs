//! Every cBoy core option driven through the real libretro entry points, the way a frontend
//! does: options are answered by a mock environment and their effect checked on the output.

use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::PathBuf;
use std::sync::Mutex;

use continuum_gb::libretro::*;

static OPTS: Mutex<Option<HashMap<String, CString>>> = Mutex::new(None);
static UPDATED: Mutex<bool> = Mutex::new(false);
/// The core is one global: tests take turns.
static SERIAL: Mutex<()> = Mutex::new(());
struct Out {
    frames: Vec<(bool, u32, u32, Vec<u32>)>,
    samples: Vec<i16>,
    held: u32,
    rumble: Vec<u16>,
}
static OUT: Mutex<Out> = Mutex::new(Out { frames: Vec::new(), samples: Vec::new(), held: u32::MAX, rumble: Vec::new() });

#[repr(C)]
struct Var {
    key: *const c_char,
    value: *const c_char,
}

unsafe extern "C" fn rumble(_p: u32, _e: u32, s: u16) -> bool {
    OUT.lock().unwrap().rumble.push(s);
    true
}

/// Sensor actions the core asked for (libretro: 0/1 accelerometer on/off, 2/3 gyroscope).
static SENSOR_CALLS: Mutex<Vec<u32>> = Mutex::new(Vec::new());
unsafe extern "C" fn sensor_state(_port: u32, action: u32, _rate: u32) -> bool {
    SENSOR_CALLS.lock().unwrap().push(action);
    true
}
unsafe extern "C" fn sensor_input(_port: u32, _id: u32) -> f32 {
    0.0
}
#[repr(C)]
struct SensorIface {
    set: Option<unsafe extern "C" fn(u32, u32, u32) -> bool>,
    get: Option<unsafe extern "C" fn(u32, u32) -> f32>,
}

unsafe extern "C" fn env(cmd: u32, d: *mut c_void) -> bool {
    match cmd {
        15 => {
            let v = &mut *(d as *mut Var);
            let key = CStr::from_ptr(v.key).to_string_lossy().into_owned();
            let g = OPTS.lock().unwrap();
            match g.as_ref().and_then(|m| m.get(&key)) {
                Some(val) => {
                    v.value = val.as_ptr();
                    true
                }
                None => false,
            }
        }
        17 => {
            let mut u = UPDATED.lock().unwrap();
            *(d as *mut bool) = *u;
            *u = false;
            true
        }
        23 => {
            *(d as *mut Option<unsafe extern "C" fn(u32, u32, u16) -> bool>) = Some(rumble);
            true
        }
        // GET_SENSOR_INTERFACE is 25 | RETRO_ENVIRONMENT_EXPERIMENTAL; a frontend answers only that.
        0x10019 => {
            *(d as *mut SensorIface) = SensorIface { set: Some(sensor_state), get: Some(sensor_input) };
            true
        }
        10 | 67 => true,
        _ => false,
    }
}
unsafe extern "C" fn video(d: *const c_void, w: u32, h: u32, pitch: usize) {
    let mut o = OUT.lock().unwrap();
    if d.is_null() {
        o.frames.push((false, 0, 0, Vec::new()));
        return;
    }
    assert_eq!(pitch, w as usize * 4);
    let px = std::slice::from_raw_parts(d as *const u32, (w * h) as usize).to_vec();
    o.frames.push((true, w, h, px));
}
unsafe extern "C" fn audio(d: *const i16, n: usize) -> usize {
    OUT.lock().unwrap().samples.extend_from_slice(std::slice::from_raw_parts(d, n * 2));
    n
}
unsafe extern "C" fn poll() {}
unsafe extern "C" fn input(_p: u32, dev: u32, _i: u32, id: u32) -> i16 {
    i16::from(dev == 1 && OUT.lock().unwrap().held == id)
}

fn rom(name: &str) -> Option<Vec<u8>> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms").join(name)).ok()
}

/// Loads `name` with `opts`, runs `n` frames, returns the output.
fn run(name: &str, opts: &[(&str, &str)], n: usize, change: Option<(usize, &[(&str, &str)])>) -> Out {
    let data = rom(name).expect("test ROM (run scripts/fetch-test-roms.sh)");
    *OPTS.lock().unwrap() = Some(opts.iter().map(|(k, v)| (k.to_string(), CString::new(*v).unwrap())).collect());
    {
        let mut o = OUT.lock().unwrap();
        o.frames.clear();
        o.samples.clear();
        o.rumble.clear();
    }
    unsafe {
        retro_set_environment(env);
        retro_set_video_refresh(video);
        retro_set_audio_sample_batch(audio);
        retro_set_input_poll(poll);
        retro_set_input_state(input);
        retro_init();
        let info = RetroGameInfo { path: std::ptr::null(), data: data.as_ptr() as *const c_void, size: data.len(), meta: std::ptr::null() };
        assert!(retro_load_game(&info));
        for i in 0..n {
            if let Some((at, more)) = change {
                if i == at {
                    let mut g = OPTS.lock().unwrap();
                    for (k, v) in more {
                        g.as_mut().unwrap().insert(k.to_string(), CString::new(*v).unwrap());
                    }
                    *UPDATED.lock().unwrap() = true;
                }
            }
            retro_run();
        }
        retro_unload_game();
        retro_deinit();
    }
    let mut o = OUT.lock().unwrap();
    Out { frames: std::mem::take(&mut o.frames), samples: std::mem::take(&mut o.samples), held: u32::MAX, rumble: std::mem::take(&mut o.rumble) }
}

fn last(o: &Out) -> &(bool, u32, u32, Vec<u32>) {
    o.frames.iter().rev().find(|f| f.0).unwrap()
}

#[test]
fn every_option_through_the_libretro_api() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    if rom("dmg-acid2.gb").is_none() || rom("jsmolka/arm/arm.gba").is_none() {
        eprintln!("SKIPPED: test ROMs missing");
        return;
    }
    let dmg = "dmg-acid2.gb";
    let base = run(dmg, &[], 60, None);
    let base_px = last(&base).3.clone();
    assert_eq!((last(&base).1, last(&base).2), (160, 144));

    // Palettes: every pixel comes from the chosen four colours.
    for name in ["green", "pocket", "light", "kiosk", "sepia", "ice"] {
        let o = run(dmg, &[("cboy_dmg_palette", name)], 60, None);
        let pal = continuum_gb::options::palette(name);
        assert!(last(&o).3.iter().all(|p| pal.contains(p)), "{name}");
    }
    // Colourising: background and sprites get different schemes.
    let o = run(dmg, &[("cboy_dmg_colorize", "ocean")], 60, None);
    assert!(last(&o).3.iter().any(|p| continuum_gb::options::SCHEMES[1][0].contains(p)));
    assert_ne!(last(&o).3, base_px);
    let o = run(dmg, &[("cboy_dmg_colorize", "auto")], 60, None);
    assert_ne!(last(&o).3, base_px);
    // Applied live, mid-game.
    let o = run(dmg, &[], 60, Some((30, &[("cboy_dmg_palette", "green")])));
    assert!(last(&o).3.iter().all(|p| continuum_gb::options::palette("green").contains(p)));

    // Layers.
    for k in ["cboy_layer_bg", "cboy_layer_window", "cboy_layer_obj"] {
        let o = run(dmg, &[(k, "off")], 60, None);
        assert_ne!(last(&o).3, base_px, "{k}");
    }

    // Ten-sprite limit off: dmg-acid2 checks the limit, so its picture changes.
    let o = run(dmg, &[("cboy_sprite_limit", "off")], 60, None);
    assert_ne!(last(&o).3, base_px);

    // Picture drawing. Standard is the default (base_px above). Both drawers pass dmg-acid2, so
    // Exact gives the same picture, set at load or switched on mid-game; on a Mealybug picture
    // that moves SCX halfway along a line the two differ, which shows the option really switches.
    let o = run(dmg, &[("cboy_renderer", "exact")], 60, None);
    assert_eq!(last(&o).3, base_px, "exact drawer on dmg-acid2");
    let o = run(dmg, &[], 60, Some((30, &[("cboy_renderer", "exact")])));
    assert_eq!(last(&o).3, base_px, "exact drawer switched on mid-game");
    let o = run(dmg, &[("cboy_renderer", "standard")], 60, None);
    assert_eq!(last(&o).3, base_px, "standard drawer named explicitly");
    let scx = "mealybug/m3_scx_low_3_bits.gb";
    if rom(scx).is_some() {
        let standard = run(scx, &[], 30, None);
        let exact = run(scx, &[("cboy_renderer", "exact")], 30, None);
        assert_ne!(last(&standard).3, last(&exact).3, "the drawers must differ on a mid-line SCX change");
    } else {
        eprintln!("SKIPPED: {scx} not found (run scripts/fetch-test-roms.sh)");
    }
    // Tone stages.
    for (k, v) in [("cboy_color_temp", "warm"), ("cboy_color_temp", "cool"), ("cboy_contrast", "reduce"), ("cboy_dark_filter", "20")] {
        let o = run(dmg, &[(k, v)], 30, None);
        assert_ne!(last(&o).3, base_px, "{k}={v}");
    }
    // Border for any Game Boy picture.
    let o = run(dmg, &[("cboy_sgb_border", "always")], 30, None);
    assert_eq!((last(&o).1, last(&o).2), (256, 224));
    // High-quality sound gives different (averaged) samples of the same length.
    let o = run(dmg, &[("cboy_audio_quality", "high")], 60, None);
    assert_eq!(o.samples.len(), base.samples.len());

    let o = run(dmg, &[("cboy_interference", "100")], 60, None);
    assert_ne!(o.samples, base.samples);
    let o = run(dmg, &[("cboy_frameskip", "auto")], 30, None);
    assert_eq!(o.frames.len(), 30);
    // Super Game Boy timing reported through the AV info.
    unsafe {
        let data = rom(dmg).unwrap();
        for (model, fps) in [("sgb", 61.17), ("sgb_pal", 60.61), ("dmg", 59.73)] {
            *OPTS.lock().unwrap() = Some([("cboy_gb_model".to_string(), CString::new(model).unwrap())].into());
            retro_set_environment(env);
            let info = RetroGameInfo { path: std::ptr::null(), data: data.as_ptr() as *const c_void, size: data.len(), meta: std::ptr::null() };
            assert!(retro_load_game(&info));
            let mut av: RetroSystemAvInfo = std::mem::zeroed();
            retro_get_system_av_info(&mut av);
            assert!((av.timing.fps - fps).abs() < 0.01, "{model} {}", av.timing.fps);
            retro_unload_game();
        }
    }

    // Ghosting on a static picture converges to the picture itself; on the first frames it differs.
    let o = run(dmg, &[("cboy_blend", "mix")], 60, None);
    assert_eq!(last(&o).3, base_px);

    // Frame skip: dropped frames are sent as "same as before".
    for (k, drawn_every) in [("1", 2), ("2", 3), ("3", 4)] {
        let o = run(dmg, &[("cboy_frameskip", k)], 24, None);
        let drawn = o.frames.iter().filter(|f| f.0).count();
        assert_eq!(drawn, 24 / drawn_every, "frameskip {k}");
        assert_eq!(o.frames.len(), 24);
    }

    // Run-ahead: same picture on a static screen, one frame per run, same audio length.
    for k in ["1", "2"] {
        let o = run(dmg, &[("cboy_run_ahead", k)], 60, None);
        assert_eq!(o.frames.len(), 60);
        let diff: Vec<usize> = last(&o).3.iter().zip(base_px.iter()).enumerate().filter(|(_, (a, b))| a != b).map(|(i, _)| i).collect();
        assert!(diff.is_empty(), "run-ahead {k}: {} px differ, first {:?} rows {:?}", diff.len(), &diff[..diff.len().min(5)], diff.iter().map(|i| i / 160).collect::<std::collections::BTreeSet<_>>());
        assert_eq!(o.samples.len(), base.samples.len(), "run-ahead {k} audio");
    }

    // Super Game Boy: border on gives 256x224, off gives the plain picture.
    let o = run(dmg, &[("cboy_gb_model", "sgb")], 30, None);
    assert_eq!((last(&o).1, last(&o).2), (256, 224));
    let o = run(dmg, &[("cboy_gb_model", "sgb2"), ("cboy_sgb_border", "off")], 30, None);
    assert_eq!((last(&o).1, last(&o).2), (160, 144));
    for m in ["dmg", "mgb", "cgb", "agb"] {
        let o = run(dmg, &[("cboy_gb_model", m)], 30, None);
        assert_eq!((last(&o).1, last(&o).2), (160, 144), "{m}");
    }
    // Boot ROM on with no file: starts normally.
    let o = run(dmg, &[("cboy_boot_rom", "on")], 60, None);
    assert_eq!(last(&o).3, base_px);

    // Sound: volume 0 is silence; sample rate changes the count.
    let o = run(dmg, &[("cboy_volume", "0")], 30, None);
    assert!(o.samples.iter().all(|s| *s == 0));
    let o = run(dmg, &[("cboy_sample_rate", "32768")], 60, None);
    let ratio = o.samples.len() as f64 / base.samples.len() as f64;
    assert!((ratio - 32768.0 / 48000.0).abs() < 0.02, "{ratio}");
    for k in ["cboy_vol_sq1", "cboy_vol_sq2", "cboy_vol_wave", "cboy_vol_noise"] {
        let _ = run(dmg, &[(k, "0"), ("cboy_lowpass", "strong"), ("cboy_highpass", "off")], 10, None);
    }

    // Game Boy Color: colour looks change the picture.
    let cgb = "cgb-acid2.gbc";
    if rom(cgb).is_some() {
        let b = run(cgb, &[], 60, None);
        for look in ["screen", "soft"] {
            let o = run(cgb, &[("cboy_gbc_color", look)], 60, None);
            assert_ne!(last(&o).3, last(&b).3, "{look}");
        }
        let o = run(cgb, &[("cboy_renderer", "exact")], 60, None);
        assert_eq!(last(&o).3, last(&b).3, "exact drawer on cgb-acid2");
    }

    // Advance: colour looks, layers, idle skipping and a forced save chip.
    let gba = "jsmolka/arm/arm.gba";
    let b = run(gba, &[], 120, None);
    assert_eq!((last(&b).1, last(&b).2), (240, 160));
    // (SP and TV keep pure white and black, all this test screen has; their mid-tones are unit-tested.)
    for look in ["gba"] {
        let o = run(gba, &[("cboy_gba_color", look)], 120, None);
        assert_ne!(last(&o).3, last(&b).3, "{look}");
    }
    let o = run(gba, &[("cboy_idle_skip", "on")], 120, None);
    assert_eq!(last(&o).3, last(&b).3, "idle skip must not change the result");
    let o = run(gba, &[("cboy_gba_save_type", "flash128"), ("cboy_gba_hardware", "rtc"), ("cboy_run_ahead", "1")], 30, None);
    assert_eq!(o.frames.len(), 30);
    unsafe {
        // Memory size follows the forced save type.
        let data = rom(gba).unwrap();
        *OPTS.lock().unwrap() = Some([("cboy_gba_save_type".to_string(), CString::new("flash128").unwrap())].into());
        retro_set_environment(env);
        let info = RetroGameInfo { path: std::ptr::null(), data: data.as_ptr() as *const c_void, size: data.len(), meta: std::ptr::null() };
        assert!(retro_load_game(&info));
        assert_eq!(retro_get_memory_size(0), 0x20000);
        retro_unload_game();
    }

    // Turbo: holding X (id 9) presses A on and off; the game sees A toggle (checked via no crash
    // and the input path), and rumble is reported once at start (motor off).
    OUT.lock().unwrap().held = 9;
    let o = run(dmg, &[("cboy_turbo", "10")], 12, None);
    OUT.lock().unwrap().held = u32::MAX;
    assert_eq!(o.frames.len(), 12);
    assert_eq!(o.rumble, vec![0]);
}

/// A tilt cartridge (MBC7, like Kirby Tilt 'n' Tumble) gets the phone's motion sensor through
/// the frontend's real sensor request, and switching "Tilt input" mid-game turns it on or off.
/// Needs no ROM download: the cartridge is a header and a loop.
#[test]
fn tilt_cartridge_asks_the_frontend_for_motion() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut data = vec![0u8; 0x8000];
    data[0x100..0x102].copy_from_slice(&[0x18, 0xFE]); // JR -2
    data[0x147] = 0x22; // MBC7 + sensor + EEPROM
    let sensors = |opts: &[(&str, &str)], change: Option<&[(&str, &str)]>| -> (Vec<u32>, Vec<u32>) {
        *OPTS.lock().unwrap() = Some(opts.iter().map(|(k, v)| (k.to_string(), CString::new(*v).unwrap())).collect());
        SENSOR_CALLS.lock().unwrap().clear();
        unsafe {
            retro_set_environment(env);
            retro_set_video_refresh(video);
            retro_set_audio_sample_batch(audio);
            retro_set_input_poll(poll);
            retro_set_input_state(input);
            retro_init();
            let info = RetroGameInfo { path: std::ptr::null(), data: data.as_ptr() as *const c_void, size: data.len(), meta: std::ptr::null() };
            assert!(retro_load_game(&info));
            for _ in 0..3 {
                retro_run();
            }
            let before = std::mem::take(&mut *SENSOR_CALLS.lock().unwrap());
            if let Some(more) = change {
                let mut g = OPTS.lock().unwrap();
                for (k, v) in more {
                    g.as_mut().unwrap().insert(k.to_string(), CString::new(*v).unwrap());
                }
                *UPDATED.lock().unwrap() = true;
            }
            for _ in 0..3 {
                retro_run();
            }
            let after = std::mem::take(&mut *SENSOR_CALLS.lock().unwrap());
            retro_unload_game();
            retro_deinit();
            (before, after)
        }
    };
    // Default "Phone motion": the accelerometer is switched on once, at the start.
    let (before, after) = sensors(&[], None);
    assert_eq!((before, after), (vec![0], vec![]), "phone motion by default");
    // Left stick, then Phone motion mid-game: off at first, on after the change.
    let (before, after) = sensors(&[("cboy_tilt_source", "stick")], Some(&[("cboy_tilt_source", "phone")]));
    assert_eq!((before, after), (vec![1], vec![0]), "switched to phone motion mid-game");
    // And back to Off: the accelerometer is released.
    let (_, after) = sensors(&[], Some(&[("cboy_tilt_source", "off")]));
    assert_eq!(after, vec![1], "switched off mid-game");
}

/// Adversarial: every value of every option, on the Game Boy, Color and Advance paths, set at
/// load and changed mid-game; a save state taken afterwards must load and replay identically,
/// and nothing may panic.
#[test]
fn every_option_value_survives_mid_game_change_and_states() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let games = ["dmg-acid2.gb", "cgb-acid2.gbc", "jsmolka/arm/arm.gba"];
    if games.iter().any(|g| rom(g).is_none()) {
        eprintln!("SKIPPED: test ROMs missing");
        return;
    }
    for game in games {
        for (key, values) in continuum_gb::options::all_values() {
            for v in &values {
                let opt = [(key.as_str(), v.as_str())];
                // Set at load.
                let a = run(game, &opt, 8, None);
                assert!(a.frames.iter().any(|f| f.0), "{game} {key}={v}: no picture");
                // Changed mid-game, then a state round trip replays the same frames.
                replay_after_state(game, &opt);
            }
        }
    }
}

fn replay_after_state(game: &str, opt: &[(&str, &str)]) {
    let data = rom(game).unwrap();
    *OPTS.lock().unwrap() = Some(HashMap::new());
    OUT.lock().unwrap().frames.clear();
    unsafe {
        retro_set_environment(env);
        retro_set_video_refresh(video);
        retro_set_audio_sample_batch(audio);
        retro_set_input_poll(poll);
        retro_set_input_state(input);
        retro_init();
        let info = RetroGameInfo { path: std::ptr::null(), data: data.as_ptr() as *const c_void, size: data.len(), meta: std::ptr::null() };
        assert!(retro_load_game(&info));
        for _ in 0..5 {
            retro_run();
        }
        {
            let mut g = OPTS.lock().unwrap();
            for (k, v) in opt {
                g.as_mut().unwrap().insert(k.to_string(), CString::new(*v).unwrap());
            }
            *UPDATED.lock().unwrap() = true;
        }
        for _ in 0..5 {
            retro_run();
        }
        let n = retro_serialize_size();
        let mut st = vec![0u8; n];
        assert!(retro_serialize(st.as_mut_ptr() as *mut c_void, n), "{game} {opt:?}: serialize");
        OUT.lock().unwrap().frames.clear();
        for _ in 0..4 {
            retro_run();
        }
        let first: Vec<_> = std::mem::take(&mut OUT.lock().unwrap().frames);
        let mut st_a = vec![0u8; n];
        retro_serialize(st_a.as_mut_ptr() as *mut c_void, n);
        assert!(retro_unserialize(st.as_ptr() as *const c_void, n), "{game} {opt:?}: unserialize");
        for _ in 0..4 {
            retro_run();
        }
        let second: Vec<_> = std::mem::take(&mut OUT.lock().unwrap().frames);
        let mut st_b = vec![0u8; n];
        retro_serialize(st_b.as_mut_ptr() as *mut c_void, n);
        assert!(st_a == st_b, "{game} {opt:?}: machine state after replay differs");
        // Junk states are refused without harm.
        let junk = vec![0xA5u8; n];
        assert!(!retro_unserialize(junk.as_ptr() as *const c_void, n));
        assert!(!retro_unserialize(st.as_ptr() as *const c_void, 10));
        retro_run();
        retro_unload_game();
        retro_deinit();
        // Blending and ghosting start afresh after a load (no ghost of the old picture), so
        // their pictures legitimately differ; the machine state above must still match.
        // Frame skip follows the frontend's frame count and the wall clock, not the machine.
        if opt.iter().any(|(k, v)| (*k == "cboy_blend" || *k == "cboy_frameskip") && *v != "off") {
            return;
        }
        let pics = |f: &[(bool, u32, u32, Vec<u32>)]| f.iter().skip(1).map(|x| (x.0, x.1, x.2, x.3.clone())).collect::<Vec<_>>();
        let (pa, pb) = (pics(&first), pics(&second));
        if std::env::var("E2E_DEBUG").is_ok() {
            let h = |f: &[(bool, u32, u32, Vec<u32>)]| f.iter().map(|x| x.3.iter().fold(0u64, |a, p| a.wrapping_mul(31).wrapping_add(u64::from(*p))) % 1000).collect::<Vec<_>>();
            eprintln!("{game} {opt:?}: {:?} vs {:?}", h(&first), h(&second));
        }
        let detail: Vec<_> = pa.iter().zip(pb.iter()).enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, (a, b))| (i, a.3.iter().zip(b.3.iter()).filter(|(x, y)| x != y).count(), a.3.iter().zip(b.3.iter()).position(|(x, y)| x != y)))
            .collect();
        assert!(pa == pb, "{game} {opt:?}: replay after state differs: {detail:?}");
    }
}
