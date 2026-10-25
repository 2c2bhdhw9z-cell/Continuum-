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

/// The frontend's side of the camera, the way the app's own one works: it keeps the core's frame
/// callback and hands it a frame on the core's thread, just before `retro_run`.
type CameraFrameRaw = unsafe extern "C" fn(*const u32, u32, u32, usize);
#[repr(C)]
struct CameraIface {
    caps: u64,
    width: u32,
    height: u32,
    start: Option<unsafe extern "C" fn() -> bool>,
    stop: Option<unsafe extern "C" fn()>,
    frame_raw: Option<CameraFrameRaw>,
    frame_gl: Option<unsafe extern "C" fn(u32, u32, *const f32)>,
    initialized: Option<unsafe extern "C" fn()>,
    deinitialized: Option<unsafe extern "C" fn()>,
}

/// What the core asked for and what it was told, so a test can check the whole handshake.
struct CameraMock {
    /// False to play a frontend with no camera to give.
    offered: bool,
    /// True when the app's "Allow camera in games" switch is on.
    allowed: bool,
    requests: u32,
    size: (u32, u32),
    frame: Option<CameraFrameRaw>,
    starts: u32,
    stops: u32,
}
static CAMERA: Mutex<CameraMock> = Mutex::new(CameraMock {
    offered: true,
    allowed: true,
    requests: 0,
    size: (0, 0),
    frame: None,
    starts: 0,
    stops: 0,
});

unsafe extern "C" fn camera_start() -> bool {
    let mut c = CAMERA.lock().unwrap();
    c.starts += 1;
    c.allowed
}
unsafe extern "C" fn camera_stop() {
    CAMERA.lock().unwrap().stops += 1;
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
        // GET_CAMERA_INTERFACE, 26 | RETRO_ENVIRONMENT_EXPERIMENTAL. The app serves raw pixels
        // only, so a core that does not ask for those is refused, as the real one refuses it.
        0x1001A => {
            let iface = &mut *(d as *mut CameraIface);
            let mut c = CAMERA.lock().unwrap();
            c.requests += 1;
            if !c.offered || iface.caps & 2 == 0 || iface.frame_raw.is_none() {
                return false;
            }
            c.size = (iface.width, iface.height);
            c.frame = iface.frame_raw;
            iface.start = Some(camera_start);
            iface.stop = Some(camera_stop);
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
    drive(&data, opts, n, change)
}

/// The same, for a cartridge the test builds itself.
fn drive(data: &[u8], opts: &[(&str, &str)], n: usize, change: Option<(usize, &[(&str, &str)])>) -> Out {
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
    // "Advance music quality: High" on a game that does not use Nintendo's music engine has to
    // change nothing whatsoever: same picture, same sound, sample for sample.
    let o = run(gba, &[("cboy_gba_music", "high")], 120, None);
    assert_eq!(last(&o).3, last(&b).3, "high-quality music must not touch the picture");
    assert_eq!(o.samples, b.samples, "a game with other music must sound exactly as it did");
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

/// A picture file as XRGB pixels.
fn png_xrgb(bytes: &[u8]) -> Vec<u32> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    let bpp = info.line_size / info.width as usize;
    let mut out = Vec::new();
    for y in 0..info.height as usize {
        for x in 0..info.width as usize {
            let p = &buf[y * info.line_size + x * bpp..];
            out.push(u32::from(p[0]) << 16 | u32::from(p[1]) << 8 | u32::from(p[2]));
        }
    }
    out
}

/// "Colour for black-and-white games", driven the way the app drives it. Off is the default and
/// leaves every picture exactly as it was. Automatic plays a black-and-white game on a Game Boy
/// Color, and the picture is the Color's own: dmg-acid2's reference for a Color, pixel for pixel.
/// A button scheme gives that scheme's colours. A colour game looks the same whatever is picked,
/// a console the player chose is kept, and a change mid-game to a Color's colours waits for the
/// next start while a change back to the Game Boy look happens at once.
#[test]
fn black_and_white_games_in_a_colours_colours() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (Some(_), Some(reference)) = (rom("dmg-acid2.gb"), rom("dmg-acid2-reference-cgb.png")) else {
        eprintln!("SKIPPED: test ROMs missing");
        return;
    };
    let dmg = "dmg-acid2.gb";
    let shot = |opts: &[(&str, &str)]| last(&run(dmg, opts, 60, None)).3.clone();
    let base = shot(&[]);
    assert!(base.iter().all(|p| continuum_gb::ppu::SHADES.contains(p)), "a Game Boy picture by default");
    assert_eq!(shot(&[("cboy_dmg_colorize", "off")]), base, "Off is the default");
    // Automatic: the Color's picture of this game.
    let auto = shot(&[("cboy_dmg_colorize", "gbc")]);
    let want = png_xrgb(&reference);
    let wrong = auto.iter().zip(&want).filter(|(a, b)| a != b).count();
    assert_eq!(wrong, 0, "dmg-acid2 in a Color's colours: {wrong} pixels differ from its reference");
    // Left: a blue background, red objects through OBP0 and green ones through OBP1.
    let five = |c: u32| {
        let v = u32::from(continuum_gb::compat::rgb15(c));
        let ch = |x: u32| (x << 3) | (x >> 2);
        ch(v & 31) << 16 | ch(v >> 5 & 31) << 8 | ch(v >> 10 & 31)
    };
    let left = continuum_gb::compat::PICKS[3].palettes;
    let allowed: Vec<u32> = left.bg.iter().chain(&left.obj0).chain(&left.obj1).map(|c| five(*c)).collect();
    let blue = shot(&[("cboy_dmg_colorize", "gbc_left")]);
    assert!(blue.iter().all(|p| allowed.contains(p)), "only the Left scheme's colours");
    assert!(blue.contains(&five(0x63A5FF)) && blue.contains(&five(0xFF8484)), "its background and its objects both show");
    assert_ne!(blue, auto);
    // The Color screen look applies to it like any Color picture.
    assert_ne!(shot(&[("cboy_dmg_colorize", "gbc"), ("cboy_gbc_color", "soft")]), auto);
    // A console the player chose is kept: a Game Boy shows its own shades.
    assert_eq!(shot(&[("cboy_dmg_colorize", "gbc"), ("cboy_gb_model", "dmg")]), base);
    // A Color console with the setting left alone looks as it always has: the Game Boy palette
    // over the same shades.
    assert_eq!(shot(&[("cboy_gb_model", "cgb")]), base);
    // A colour game is the same picture whatever is picked.
    let cgb = "cgb-acid2.gbc";
    if rom(cgb).is_some() {
        let colour = last(&run(cgb, &[], 60, None)).3.clone();
        for v in ["gbc", "gbc_right_b", "meadow"] {
            assert_eq!(last(&run(cgb, &[("cboy_dmg_colorize", v)], 60, None)).3, colour, "{v} on a colour game");
        }
    }
    // Mid-game: to a Color's colours waits for the next start; back to Off is at once.
    assert_eq!(last(&run(dmg, &[], 60, Some((30, &[("cboy_dmg_colorize", "gbc")])))).3, base);
    assert_eq!(last(&run(dmg, &[("cboy_dmg_colorize", "gbc")], 60, Some((30, &[("cboy_dmg_colorize", "off")])))).3, base);
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

/// The two settings whose text promises "Next start" really do wait for one. The app asks the
/// core for the sound rate and the shape of the picture as the game loads and keeps what it was
/// told, so moving either under a running game only made the sound crackle (32768 Hz played back
/// as 48000) and squashed the picture by about 3%. Needs no ROM download: the cartridge switches
/// the screen on and loops.
#[test]
fn the_next_start_settings_wait_for_the_next_start() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut data = vec![0u8; 0x8000];
    // LD A,$91 / LDH ($40),A (screen and background on) / JR -2.
    data[0x100..0x106].copy_from_slice(&[0x3E, 0x91, 0xE0, 0x40, 0x18, 0xFE]);
    // Sound: the same number of samples comes out whether or not the rate is changed mid-game.
    let base = drive(&data, &[], 60, None);
    let mid = drive(&data, &[], 60, Some((30, &[("cboy_sample_rate", "32768")])));
    assert_eq!(mid.samples.len(), base.samples.len(), "the sound rate moved under a running game");
    // Chosen before the game starts, it does take effect.
    let at_load = drive(&data, &[("cboy_sample_rate", "32768")], 60, None);
    let ratio = at_load.samples.len() as f64 / base.samples.len() as f64;
    assert!((ratio - 32768.0 / 48000.0).abs() < 0.02, "{ratio}");
    // Picture: the border keeps the shape the app set up with, and changes at the next start.
    let mid = drive(&data, &[], 30, Some((15, &[("cboy_sgb_border", "always")])));
    assert_eq!((last(&mid).1, last(&mid).2), (160, 144), "the picture changed shape under a running game");
    let at_load = drive(&data, &[("cboy_sgb_border", "always")], 30, None);
    assert_eq!((last(&at_load).1, last(&at_load).2), (256, 224));
    // Everything else still applies there and then: the palette on the same frames.
    let green = continuum_gb::options::palette("green");
    let live = drive(&data, &[], 30, Some((15, &[("cboy_dmg_palette", "green")])));
    assert!(last(&live).3.iter().all(|p| green.contains(p)));
}

/// A cartridge that runs on both a plain Game Boy and a Game Boy Color (header $143 = $80, like
/// Pokemon Yellow): "Console for games that run on both" decides which one it gets, and that
/// changes the colours on screen and, for the Super Game Boy, the size of the picture.
/// Needs no ROM download: the cartridge switches the screen on and loops.
#[test]
fn a_cartridge_that_runs_on_both_consoles_follows_its_own_setting() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut data = vec![0u8; 0x8000];
    // LD A,$91 / LDH ($40),A (screen and background on) / JR -2.
    data[0x100..0x107].copy_from_slice(&[0x3E, 0x91, 0xE0, 0x40, 0x18, 0xFE, 0x00]);
    data[0x143] = 0x80; // runs on both
    let green = continuum_gb::options::palette("green");
    let shot = |opts: &[(&str, &str)]| -> (u32, u32, Vec<u32>) {
        *OPTS.lock().unwrap() = Some(opts.iter().map(|(k, v)| (k.to_string(), CString::new(*v).unwrap())).collect());
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
            for _ in 0..4 {
                retro_run();
            }
            retro_unload_game();
            retro_deinit();
        }
        let f = OUT.lock().unwrap().frames.iter().rev().find(|f| f.0).cloned().unwrap();
        (f.1, f.2, f.3)
    };
    // The default picks the Color, so the Game Boy palette is not used.
    let (w, h, px) = shot(&[("cboy_dmg_palette", "green")]);
    assert_eq!((w, h), (160, 144));
    assert!(!px.iter().all(|p| green.contains(p)), "Color by default");
    // Game Boy: the four shades of the chosen palette, and nothing else.
    let (w, h, px) = shot(&[("cboy_hybrid_model", "dmg"), ("cboy_dmg_palette", "green")]);
    assert_eq!((w, h), (160, 144));
    assert!(px.iter().all(|p| green.contains(p)), "Game Boy palette: {:06X}", px[0]);
    // Super Game Boy: its frame around the picture.
    let (w, h, _) = shot(&[("cboy_hybrid_model", "sgb")]);
    assert_eq!((w, h), (256, 224));
    // Advance: a Color machine again, so not the Game Boy palette.
    let (w, h, px) = shot(&[("cboy_hybrid_model", "agb"), ("cboy_dmg_palette", "green")]);
    assert_eq!((w, h), (160, 144));
    assert!(!px.iter().all(|p| green.contains(p)), "Advance runs it as a Color game");
}

/// The cartridge clock block (RETRO_MEMORY_RTC), which the frontend keeps as `<game>.rtc` and
/// writes back before the first frame: with "Real time" the clock carries on from where it was,
/// plus the time the game was closed. MBC3 (Pokemon Gold's) and HuC3. Needs no ROM download.
#[test]
fn cartridge_clock_runs_on_while_the_game_is_closed() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let cart = |kind: u8| {
        let mut d = vec![0u8; 0x8000];
        d[0x100..0x102].copy_from_slice(&[0x18, 0xFE]); // JR -2
        d[0x147] = kind;
        d[0x149] = 3;
        d
    };
    // Loads the cart, hands it `block` as a frontend does, runs, and returns the block it keeps.
    let clock_after = |data: Vec<u8>, block: [u8; 24], restart: bool| -> [u8; 24] {
        *OPTS.lock().unwrap() = Some(HashMap::new());
        unsafe {
            retro_set_environment(env);
            retro_set_video_refresh(video);
            retro_set_audio_sample_batch(audio);
            retro_set_input_poll(poll);
            retro_set_input_state(input);
            retro_init();
            let info = RetroGameInfo { path: std::ptr::null(), data: data.as_ptr() as *const c_void, size: data.len(), meta: std::ptr::null() };
            assert!(retro_load_game(&info));
            assert_eq!(retro_get_memory_size(1), 24);
            std::ptr::copy_nonoverlapping(block.as_ptr(), retro_get_memory_data(1) as *mut u8, 24);
            retro_run();
            if restart {
                retro_reset();
                retro_run();
            }
            let mut out = [0u8; 24];
            std::ptr::copy_nonoverlapping(retro_get_memory_data(1) as *const u8, out.as_mut_ptr(), 24);
            retro_unload_game();
            retro_deinit();
            out
        }
    };
    // MBC3 + clock + RAM + battery: day 5, 13:20:10, closed 30 days and an hour (more than the
    // ten days the old catch-up stopped at).
    let mut block = [0u8; 24];
    block[..5].copy_from_slice(&[10, 20, 13, 5, 0]);
    block[16..24].copy_from_slice(&(now - (30 * 86_400 + 3_600)).to_le_bytes());
    let b = clock_after(cart(0x10), block, false);
    assert_eq!((b[1], b[2], b[3], b[4] & 0xC1), (20, 14, 35, 0), "MBC3 clock after 30 days and an hour: {b:?}");
    // HuC3: 10:00 on day 5, closed an hour, comes back 11:00 on day 5, and Restart keeps it.
    let mut block = [0u8; 24];
    block[..4].copy_from_slice(&[600u16 as u8, (600u16 >> 8) as u8, 5, 0]);
    block[16..24].copy_from_slice(&(now - 3_600).to_le_bytes());
    let b = clock_after(cart(0xFE), block, true);
    let (minutes, days) = (u16::from(b[0]) | u16::from(b[1]) << 8, u16::from(b[2]) | u16::from(b[3]) << 8);
    assert_eq!((minutes, days), (660, 5), "HuC3 clock after an hour closed and a Restart");
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

/// One `LD A,v` / `LD [at],A` pair.
fn store(code: &mut Vec<u8>, at: u16, v: u8) {
    code.extend_from_slice(&[0x3E, v, 0xEA, at as u8, (at >> 8) as u8]);
}

/// A Game Boy Camera cartridge that keeps taking pictures: it switches its RAM on, leaves a byte
/// of its own in the 256 bytes just before where the picture goes, programs the exposure and the
/// dither matrix, and loops on the trigger. Built here, so no ROM download is needed.
fn camera_cart() -> Vec<u8> {
    let mut data = vec![0u8; 0x8000];
    data[0x100..0x104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]); // NOP / JP $0150
    data[0x147] = 0xFC; // Game Boy Camera
    data[0x149] = 3; // 32 KB of cartridge RAM
    let mut code: Vec<u8> = Vec::new();
    store(&mut code, 0x0000, 0x0A);
    store(&mut code, 0xA0FF, 0x5A);
    let again = 0x0150 + code.len() as u16;
    store(&mut code, 0x4000, 0x10); // the camera registers in the A000-BFFF window
    store(&mut code, 0xA002, 0x03); // exposure $0300
    store(&mut code, 0xA003, 0x00);
    for cell in 0..16u16 {
        for (i, threshold) in [116u8, 124, 132].iter().enumerate() {
            store(&mut code, 0xA006 + cell * 3 + i as u16, *threshold);
        }
    }
    store(&mut code, 0xA000, 0x03); // take the picture
    store(&mut code, 0x4000, 0x00); // back to RAM bank 0, where the picture lands
    code.extend_from_slice(&[0xC3, again as u8, (again >> 8) as u8]);
    data[0x150..0x150 + code.len()].copy_from_slice(&code);
    data
}

/// A camera frame the way the app delivers one: XRGB8888 pixels in memory, larger than the
/// sensor so the core has to shrink it, four vertical bands of grey, and the rows the cartridge
/// throws away (the top and bottom of the frame) in the opposite shade.
fn camera_test_frame(bands: [u8; 4]) -> (Vec<u32>, u32, u32) {
    let (w, h) = (256usize, 240usize);
    let mut px = vec![0u32; w * h];
    for y in 0..h {
        for x in 0..w {
            let v = bands[x / 64];
            let v = if (8..h - 8).contains(&y) { v } else { 255 - v };
            px[y * w + x] = u32::from(v) * 0x0001_0101;
        }
    }
    (px, w as u32, h as u32)
}

fn reset_camera(offered: bool, allowed: bool) {
    *CAMERA.lock().unwrap() =
        CameraMock { offered, allowed, requests: 0, size: (0, 0), frame: None, starts: 0, stops: 0 };
}

unsafe fn set_callbacks() {
    retro_set_environment(env);
    retro_set_video_refresh(video);
    retro_set_audio_sample_batch(audio);
    retro_set_input_poll(poll);
    retro_set_input_state(input);
    retro_init();
}

/// One tile of each of the four bands of the picture in cartridge RAM, as the Game Boy colour
/// its two bitplanes hold. Also checks the game's own first 256 bytes are still there.
unsafe fn picture_bands() -> Vec<u8> {
    let ram = retro_get_memory_data(0) as *const u8;
    let len = retro_get_memory_size(0);
    assert!(!ram.is_null() && len >= 0x100 + 14 * 16 * 16, "no cartridge RAM to read");
    let ram = std::slice::from_raw_parts(ram, len);
    assert_eq!(ram[0xFF], 0x5A, "the picture trampled the game's own 256 bytes");
    [0usize, 4, 8, 12]
        .iter()
        .map(|t| u8::from(ram[0x100 + t * 16] != 0) | u8::from(ram[0x100 + t * 16 + 1] != 0) << 1)
        .collect()
}

/// The phone's camera reaches the Game Boy Camera cartridge through the real libretro camera
/// interface: the core asks for it as the game loads, starts it on the first frame, is handed
/// frames on its own thread, and the picture turns up in cartridge RAM where the game looks for
/// it. Needs no ROM download: the cartridge is built above.
#[test]
fn the_phones_camera_reaches_the_game_boy_camera() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = camera_cart();
    *OPTS.lock().unwrap() = Some(HashMap::new());
    reset_camera(true, true);
    unsafe {
        set_callbacks();
        let info = RetroGameInfo { path: std::ptr::null(), data: data.as_ptr() as *const c_void, size: data.len(), meta: std::ptr::null() };
        assert!(retro_load_game(&info));
        {
            let c = CAMERA.lock().unwrap();
            assert_eq!(c.requests, 1, "the core did not ask the app for a camera");
            assert_eq!(c.size, (128, 120), "the core asked for the wrong size");
            assert_eq!(c.starts, 0, "the camera was started before the game ran");
        }
        retro_run();
        assert_eq!(CAMERA.lock().unwrap().starts, 1, "the core did not start the camera");
        // No frame yet, so an even grey: every band the same, and no made-up picture.
        assert_eq!(picture_bands(), vec![1, 1, 1, 1], "a picture appeared from nowhere");

        // Frames, delivered the way the app does it: on this thread, just before retro_run.
        let shades = |bands: [u8; 4]| -> Vec<u8> {
            let (px, w, h) = camera_test_frame(bands);
            let frame = CAMERA.lock().unwrap().frame.expect("the core registered no frame callback");
            frame(px.as_ptr(), w, h, w as usize * 4);
            retro_run();
            picture_bands()
        };
        assert_eq!(shades([0, 64, 152, 255]), vec![3, 2, 1, 0], "the picture did not arrive");
        assert_eq!(shades([255, 152, 64, 0]), vec![0, 1, 2, 3], "the next frame did not replace it");

        retro_unload_game();
        assert_eq!(CAMERA.lock().unwrap().stops, 1, "the camera was left running after the game");
        retro_deinit();
    }
}

/// No camera, no harm: an app that has none to give, or whose camera switch is off, leaves the
/// Game Boy Camera showing an even grey and the game running. And an ordinary cartridge never
/// asks for a camera at all, so no normal game makes the phone ask for camera permission.
#[test]
fn a_game_with_no_camera_still_runs_and_an_ordinary_game_never_asks() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = camera_cart();
    *OPTS.lock().unwrap() = Some(HashMap::new());
    let grey = |offered: bool, allowed: bool| -> (Vec<u8>, u32) {
        reset_camera(offered, allowed);
        unsafe {
            set_callbacks();
            let info = RetroGameInfo { path: std::ptr::null(), data: data.as_ptr() as *const c_void, size: data.len(), meta: std::ptr::null() };
            assert!(retro_load_game(&info), "the game must still load without a camera");
            for _ in 0..3 {
                retro_run();
            }
            let bands = picture_bands();
            let starts = CAMERA.lock().unwrap().starts;
            retro_unload_game();
            retro_deinit();
            (bands, starts)
        }
    };
    // The app has no camera: the core never gets a start, and the picture is an even grey.
    assert_eq!(grey(false, false), (vec![1, 1, 1, 1], 0), "refused camera");
    // The app has one but the switch is off: started, told no, and still an even grey.
    assert_eq!(grey(true, false), (vec![1, 1, 1, 1], 1), "camera not allowed");

    // An ordinary cartridge: no camera is ever asked for.
    let mut plain = vec![0u8; 0x8000];
    plain[0x100..0x102].copy_from_slice(&[0x18, 0xFE]); // JR -2
    plain[0x147] = 0x1B; // MBC5 + RAM + battery
    plain[0x149] = 3;
    reset_camera(true, true);
    unsafe {
        set_callbacks();
        let info = RetroGameInfo { path: std::ptr::null(), data: plain.as_ptr() as *const c_void, size: plain.len(), meta: std::ptr::null() };
        assert!(retro_load_game(&info));
        retro_run();
        retro_unload_game();
        retro_deinit();
    }
    let c = CAMERA.lock().unwrap();
    assert_eq!((c.requests, c.starts), (0, 0), "an ordinary game asked for the camera");
}

/// Loads a ROM with `opts`, runs `before` frames, resets, runs `after` frames.
fn drive_with_reset(data: &[u8], opts: &[(&str, &str)], before: usize, after: usize) -> Out {
    *OPTS.lock().unwrap() = Some(opts.iter().map(|(k, v)| (k.to_string(), CString::new(*v).unwrap())).collect());
    {
        let mut o = OUT.lock().unwrap();
        o.frames.clear();
        o.samples.clear();
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
        for _ in 0..before {
            retro_run();
        }
        retro_reset();
        for _ in 0..after {
            retro_run();
        }
        retro_unload_game();
        retro_deinit();
    }
    let mut o = OUT.lock().unwrap();
    Out { frames: std::mem::take(&mut o.frames), samples: std::mem::take(&mut o.samples), held: u32::MAX, rumble: Vec::new() }
}

fn brightness(px: &[u32]) -> u64 {
    px.iter().map(|p| u64::from((p >> 16) & 0xFF) + u64::from((p >> 8) & 0xFF) + u64::from(p & 0xFF)).sum()
}

/// With the console on Advance and the Game Boy Player reset on, a reset first lets the game play
/// on while the picture fades to black (35 frames: full for the first few, black for the last
/// few), then holds the console dark and silent (94 frames), then the game starts again. Without
/// the setting, a reset is at once.
#[test]
fn a_game_boy_player_reset_fades_out_and_pauses() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Some(data) = rom("cgb-acid2.gbc") else { return };
    let opts = [("cboy_gb_model", "agb"), ("cboy_gbp_reset", "on")];
    let o = drive_with_reset(&data, &opts, 60, 35 + 94 + 60);
    let shown: Vec<&Vec<u32>> = o.frames.iter().filter(|f| f.0).map(|f| &f.3).collect();
    assert_eq!(shown.len(), 60 + 35 + 94 + 60);
    let full = brightness(shown[59]);
    assert!(full > 0);
    let fade: Vec<u64> = shown[60..95].iter().map(|f| brightness(f)).collect();
    assert_eq!(fade[0], full, "the fade starts at full brightness");
    assert!(fade.windows(2).all(|w| w[1] <= w[0]), "and only ever gets darker: {fade:?}");
    assert!(fade[17] < full * 3 / 4 && fade[17] > full / 4, "half way it is half dark");
    assert_eq!(fade[34], 0, "and it ends black");
    assert!(shown[95..95 + 94].iter().all(|f| brightness(f) == 0), "then the console is held, dark");
    assert!(brightness(shown[shown.len() - 1]) > 0, "and the game starts again");
    // Without the setting, the reset is at once: no frame after it is dark.
    let o = drive_with_reset(&data, &[("cboy_gb_model", "agb")], 60, 60);
    let shown: Vec<&Vec<u32>> = o.frames.iter().filter(|f| f.0).map(|f| &f.3).collect();
    assert!(shown[60..].iter().all(|f| brightness(f) > 0));
}

/// The real Super Game Boy runs a little fast, the Super Game Boy 2 and the setting's other
/// choice at a Game Boy's speed: the frame rate the frontend is told says which.
#[test]
fn the_super_game_boy_speed() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Some(data) = rom("dmg-acid2.gb") else { return };
    let fps = |opts: &[(&str, &str)]| -> f64 {
        *OPTS.lock().unwrap() = Some(opts.iter().map(|(k, v)| (k.to_string(), CString::new(*v).unwrap())).collect());
        unsafe {
            retro_set_environment(env);
            retro_set_video_refresh(video);
            retro_set_audio_sample_batch(audio);
            retro_set_input_poll(poll);
            retro_set_input_state(input);
            retro_init();
            let info = RetroGameInfo { path: std::ptr::null(), data: data.as_ptr() as *const c_void, size: data.len(), meta: std::ptr::null() };
            assert!(retro_load_game(&info));
            let mut av: RetroSystemAvInfo = std::mem::zeroed();
            retro_get_system_av_info(&mut av);
            retro_unload_game();
            retro_deinit();
            av.timing.fps
        }
    };
    let gb = 4_194_304.0 / 70_224.0;
    assert!((fps(&[("cboy_gb_model", "sgb")]) - 21_477_272.0 / 5.0 / 70_224.0).abs() < 1e-6, "a real Super Game Boy");
    assert!((fps(&[("cboy_gb_model", "sgb2")]) - gb).abs() < 1e-6, "the Super Game Boy 2 has a Game Boy's clock");
    assert!((fps(&[("cboy_gb_model", "sgb"), ("cboy_sgb_speed", "normal")]) - gb).abs() < 1e-6, "or the player asks for it");
    assert!((fps(&[("cboy_gb_model", "dmg")]) - gb).abs() < 1e-6);
}

/// "Automatic (Pocket colours on a Pocket)": a Pocket shows its own colours, any other Game Boy
/// stays grey.
#[test]
fn the_pocket_shows_its_own_colours_when_asked() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Some(data) = rom("dmg-acid2.gb") else { return };
    let colours = |opts: &[(&str, &str)]| -> std::collections::BTreeSet<u32> {
        last(&run_data(&data, opts, 30)).3.iter().copied().collect()
    };
    let pocket = colours(&[("cboy_gb_model", "mgb"), ("cboy_dmg_palette", "auto")]);
    assert!(pocket.contains(&0xC4CFA1), "the Pocket's lightest colour: {pocket:X?}");
    let grey = colours(&[("cboy_gb_model", "dmg"), ("cboy_dmg_palette", "auto")]);
    assert_eq!(grey, colours(&[("cboy_gb_model", "dmg")]), "a Game Boy stays as it was");
    // Through "Let games see a newer console", a Game Boy game on Auto becomes a Pocket game.
    let newer = colours(&[("cboy_newer_console", "pocket"), ("cboy_dmg_palette", "auto")]);
    assert_eq!(newer, pocket);
}

fn run_data(data: &[u8], opts: &[(&str, &str)], n: usize) -> Out {
    drive(data, opts, n, None)
}
