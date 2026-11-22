//! Continuum for LG TVs (webOS). A light, remote-friendly player that runs the owner's own cores
//! (cBoy for Game Boy, Game Boy Color and Game Boy Advance).

mod app;
mod audio;
mod config;
mod draw;
mod files;
mod gl;
mod input;
mod library;
mod log;
mod retro;
mod sdl;
mod session;
mod settings_page;
mod swirl;
mod ui;
mod upload;

use std::time::Instant;

fn rss_mb() -> f64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    for l in s.lines() {
        if let Some(v) = l.strip_prefix("VmRSS:") {
            let kb: f64 = v.trim().trim_end_matches("kB").trim().parse().unwrap_or(0.0);
            return kb / 1024.0;
        }
    }
    0.0
}

/// `continuum-tv --bench <core.so> <game> [frames]`: run a game as fast as possible with no
/// picture or sound, pressing Start and A now and then to get past title screens, and print the
/// speed. BENCH_OPTS="cboy_idle_skip=on,..." sets core options for the run.
fn bench(args: &[String]) {
    let core_path = &args[0];
    let rom_path = &args[1];
    let frames: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1800);
    let dirs = retro::CoreDirs { system: "/tmp".into(), saves: "/tmp".into() };
    let preset: Vec<(String, String)> = std::env::var("BENCH_OPTS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|kv| kv.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
        .collect();
    let mut core = retro::Core::open(core_path, &dirs, preset).expect("core");
    let (name, rom) = if files::ARCHIVE_EXTS.contains(&files::ext_of(rom_path).as_str()) {
        files::read_archive(std::path::Path::new(rom_path)).expect("archive")
    } else {
        (rom_path.clone(), std::fs::read(rom_path).expect("rom"))
    };
    core.load(&name, rom).expect("load");
    println!("core {} {} av {:?}", core.info.name, core.info.version, core.av);
    let t0 = Instant::now();
    let mut slow = 0f64;
    let mut window = Instant::now();
    let mut window_frames = 0;
    for i in 0..frames {
        let phase = i % 120;
        let h = retro::host();
        h.buttons[0] = if phase < 6 {
            1 << retro::START
        } else if (40..46).contains(&phase) {
            1 << retro::A
        } else if phase > 60 {
            1 << retro::RIGHT
        } else {
            0
        };
        h.audio.clear();
        core.run();
        window_frames += 1;
        if window_frames == 60 {
            let fps = 60.0 / window.elapsed().as_secs_f64();
            if slow == 0.0 || fps < slow {
                slow = fps;
            }
            window = Instant::now();
            window_frames = 0;
        }
    }
    let secs = t0.elapsed().as_secs_f64();
    println!("frames {frames} in {secs:.2}s = {:.1} fps average, slowest second {:.1} fps", frames as f64 / secs, slow);
    let t1 = Instant::now();
    let st = core.save_state().map(|s| s.len()).unwrap_or(0);
    println!("save state {} bytes in {:.1} ms", st, t1.elapsed().as_secs_f64() * 1000.0);
    println!("memory in use {:.1} MB", rss_mb());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "--bench" {
        bench(&args[2..]);
        return;
    }
    match app::App::new() {
        Ok(mut a) => a.run(),
        Err(e) => {
            logln!("Continuum couldn't start: {e}");
            std::process::exit(1);
        }
    }
}
