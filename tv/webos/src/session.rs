//! One game being played: the core, its battery save and clock, save states and the picture.

use crate::files::{self, Paths};
use crate::library::Game;
use crate::retro::{self, Core, CoreDirs};
use std::time::Instant;

pub struct Session {
    pub core: Core,
    pub game: Game,
    sram_last: Vec<u8>,
    rtc_last: Vec<u8>,
    last_check: Instant,
    pub frames: u64,
    pub fps_shown: f64,
    fps_window: (Instant, u32),
    pub resumed: bool,
    pub core_time: f64,
}

fn srm_path(paths: &Paths, id: &str) -> std::path::PathBuf {
    paths.saves.join(format!("{}.srm", files::safe_name(id)))
}
fn rtc_path(paths: &Paths, id: &str) -> std::path::PathBuf {
    paths.saves.join(format!("{}.rtc", files::safe_name(id)))
}

impl Session {
    pub fn start(
        paths: &Paths,
        core_path: &str,
        game: &Game,
        options: Vec<(String, String)>,
        resume: bool,
    ) -> Result<Session, String> {
        let (rom_name, rom) = if files::ARCHIVE_EXTS.contains(&files::ext_of(&game.path.to_string_lossy()).as_str()) {
            files::read_archive(&game.path)?
        } else {
            (game.id.clone(), std::fs::read(&game.path).map_err(|e| format!("can't read the game: {e}"))?)
        };
        let dirs = CoreDirs {
            system: paths.system.to_string_lossy().into_owned(),
            saves: paths.saves.to_string_lossy().into_owned(),
        };
        let mut core = Core::open(core_path, &dirs, options)?;
        let path_for_core = paths.saves.join(&rom_name).to_string_lossy().into_owned();
        core.load(&path_for_core, rom)?;
        let mut s = Session {
            core,
            game: game.clone(),
            sram_last: Vec::new(),
            rtc_last: Vec::new(),
            last_check: Instant::now(),
            frames: 0,
            fps_shown: 0.0,
            fps_window: (Instant::now(), 0),
            resumed: false,
            core_time: 0.0,
        };
        // The battery save and clock go in before the first frame (cBoy reads the clock once).
        if let Ok(b) = std::fs::read(srm_path(paths, &game.id)) {
            if let Some(m) = s.core.memory(retro::MEMORY_SAVE_RAM) {
                let n = m.len().min(b.len());
                m[..n].copy_from_slice(&b[..n]);
            }
            s.sram_last = b;
        }
        if let Ok(b) = std::fs::read(rtc_path(paths, &game.id)) {
            if let Some(m) = s.core.memory(retro::MEMORY_RTC) {
                let n = m.len().min(b.len());
                m[..n].copy_from_slice(&b[..n]);
            }
            s.rtc_last = b;
        }
        if resume {
            let auto = files::state_file(paths, &game.id, None);
            if let Ok(st) = std::fs::read(&auto) {
                // run one frame first so the machine is fully set up
                s.core.run();
                if s.core.load_state(&st) {
                    s.resumed = true;
                } else {
                    crate::logln!("the auto-save for {} didn't load", game.id);
                }
            }
        }
        Ok(s)
    }

    pub fn run(&mut self, buttons: u16, stick: [i16; 4]) {
        let h = retro::host();
        h.buttons[0] = buttons;
        h.analog[0] = stick;
        let t = Instant::now();
        self.core.run();
        self.core_time += t.elapsed().as_secs_f64();
        self.frames += 1;
        self.fps_window.1 += 1;
        let el = self.fps_window.0.elapsed().as_secs_f64();
        if el >= 1.0 {
            let n = self.fps_window.1.max(1) as f64;
            self.fps_shown = n / el;
            self.fps_window = (Instant::now(), 0);
            if self.frames % 600 < 60 {
                crate::logln!("speed {:.1} fps, cBoy {:.1} ms a frame", self.fps_shown, self.core_time / n * 1000.0);
            }
            self.core_time = 0.0;
        }
    }

    /// Write the battery save and clock if they changed. Checked every few seconds while
    /// playing, and always on pause and quit.
    pub fn save_battery(&mut self, paths: &Paths, force: bool) {
        if !force && self.last_check.elapsed().as_secs() < 3 {
            return;
        }
        self.last_check = Instant::now();
        let mut sram_written = false;
        if let Some(m) = self.core.memory(retro::MEMORY_SAVE_RAM) {
            if m != self.sram_last.as_slice() {
                let data = m.to_vec();
                if files::write_atomic(srm_path(paths, &self.game.id), &data) {
                    self.sram_last = data;
                    sram_written = true;
                }
            }
        }
        if let Some(m) = self.core.memory(retro::MEMORY_RTC) {
            // The clock ticks every second, so it is written with the game's own save and on
            // pause and quit, not every time it moves.
            if force || sram_written || self.rtc_last.is_empty() {
                let data = m.to_vec();
                if files::write_atomic(rtc_path(paths, &self.game.id), &data) {
                    self.rtc_last = data;
                }
            }
        }
    }

    /// Save a state (None is the automatic one), with a small picture of the screen.
    pub fn save_state(&mut self, paths: &Paths, slot: Option<u32>) -> bool {
        let data = match self.core.save_state() {
            Some(d) => d,
            None => return false,
        };
        let f = files::state_file(paths, &self.game.id, slot);
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let ok = files::write_atomic(&f, &data);
        let h = retro::host();
        if ok && h.frame_w > 0 {
            files::write_png(&f.with_extension("png"), h.frame_w, h.frame_h, &h.frame);
        }
        ok
    }

    pub fn load_state(&mut self, paths: &Paths, slot: Option<u32>) -> bool {
        match std::fs::read(files::state_file(paths, &self.game.id, slot)) {
            Ok(d) => self.core.load_state(&d),
            Err(_) => false,
        }
    }

    pub fn finish(&mut self, paths: &Paths, auto_save: bool) {
        if auto_save {
            self.save_state(paths, None);
        }
        self.save_battery(paths, true);
    }
}
