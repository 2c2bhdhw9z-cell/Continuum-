//! The app: the window, the pages (Home, Library, Favorites, Settings, Add games), the sheets
//! over them, and playing a game.

use crate::audio::Audio;
use crate::config::{Action, Settings};
use crate::draw::{self, white, with_alpha, Draw, Shape, Texture, BOLD, MEDIUM, REGULAR, VH, VW};
use crate::files::{self, Paths};
use crate::input::{Input, Nav};
use crate::library::{Game, Library, System};
use crate::retro::{self, OptionCategory, OptionDef};
use crate::sdl::{self, Event, Sdl};
use crate::session::Session;
use crate::settings_page::{self, Row};
use crate::swirl::{self, Palette};
use crate::ui::{self, id, Ctx, Item, Kind, Rect, Theme};
use crate::upload::{self, Upload};
use std::collections::{HashMap, HashSet};
use std::ffi::{c_void, CString};
use std::time::{Duration, Instant};

pub struct Images {
    gb: Option<Texture>,
    gbc: Option<Texture>,
    gba: Option<Texture>,
}

impl Images {
    pub fn console(&self, s: System) -> Option<&Texture> {
        match s {
            System::Gb => self.gb.as_ref(),
            System::Gbc => self.gbc.as_ref(),
            System::Gba => self.gba.as_ref(),
        }
    }
}

const TAB_NAMES: [&str; 4] = ["Home", "Library", "Favorites", "Settings"];
const PAGE_ADD: usize = 4;

#[derive(Clone)]
enum Act {
    Tab(usize),
    AddGames,
    Filter(Option<System>),
    Play(String, bool),
    GameMenu(String),
    Row(Row),
    Fold(&'static str),
    Sort,
    Layout,
    Shuffle,
    Resume,
    SaveState,
    LoadLast,
    Slots(String),
    Reset,
    FastLatch,
    GameSettings,
    QuitGame,
    Slot(String, u32),
    SlotSave(u32),
    SlotLoad(String, u32),
    SlotDelete(String, u32),
    SlotDeleteNow(String, u32),
    Favourite(String),
    Remove(String),
    RemoveNow(String),
    ConfirmYes,
    Close,
    QuitApp,
}

enum Overlay {
    Pause,
    GameMenu(String),
    Slots(String),
    SlotMenu(String, u32),
    Confirm { title: String, body: String, yes: String, act: Act, danger: bool },
    Settings,
}

struct Layer {
    ov: Overlay,
    focus: u64,
    scroll: f32,
    scroll_anim: f32,
}

pub struct App {
    sdl: Sdl,
    win: *mut c_void,
    d: Draw,
    s: Settings,
    paths: Paths,
    lib: Library,
    input: Input,
    page: usize,
    focus: [u64; 5],
    scroll: [f32; 5],
    scroll_anim: [f32; 5],
    page_started: Instant,
    filter: Option<System>,
    layers: Vec<Layer>,
    session: Option<Session>,
    fast_latch: bool,
    audio: Option<Audio>,
    game_tex: Option<Texture>,
    swirl_tex: Option<Texture>,
    swirl_buf: Vec<u8>,
    swirl_clock: f64,
    swirl_drawn: f64,
    palette_key: String,
    palette: Palette,
    items: Vec<Item>,
    acts: HashMap<u64, Act>,
    anim: HashMap<u64, f32>,
    hscroll: HashMap<String, f32>,
    images: Images,
    upload: Option<Upload>,
    qr_tex: Option<(String, Texture)>,
    thumbs: HashMap<String, (u64, Texture)>,
    toast: Option<(String, Instant)>,
    start: Instant,
    last_frame: Instant,
    core_path: String,
    core_defs: (Vec<OptionDef>, Vec<OptionCategory>),
    core_hidden: HashMap<String, bool>,
    core_defaults: HashMap<String, String>,
    core_vals: HashMap<String, String>,
    core_version: String,
    open_cards: HashSet<&'static str>,
    settings_dirty: Option<Instant>,
    core_dirty: Option<Instant>,
    refresh: f64,
    frame_times: Vec<f64>,
    vsync_ok: bool,
    quit: bool,
    last_scan: Instant,
    debug_input: bool,
}

fn load_fonts() -> Result<Vec<ab_glyph::FontVec>, String> {
    let dirs = [
        std::env::var("CONTINUUM_FONTS").unwrap_or_default(),
        "/usr/share/fonts".to_string(),
        "/usr/share/fonts/truetype/dejavu".to_string(),
    ];
    let names = [
        ["LG_Smart_UI-Light.ttf", "DejaVuSans-ExtraLight.ttf"],
        ["LG_Smart_UI-Regular.ttf", "DejaVuSans.ttf"],
        ["LG_Smart_UI-SemiBold.ttf", "DejaVuSans.ttf"],
        ["LG_Smart_UI-Bold.ttf", "DejaVuSans-Bold.ttf"],
    ];
    let mut fonts = Vec::new();
    let mut last: Option<Vec<u8>> = None;
    for choice in names.iter() {
        let mut got = None;
        'find: for d in dirs.iter().filter(|d| !d.is_empty()) {
            for n in choice.iter() {
                if let Ok(b) = std::fs::read(format!("{d}/{n}")) {
                    if let Ok(f) = ab_glyph::FontVec::try_from_vec(b.clone()) {
                        got = Some(f);
                        last = Some(b);
                        break 'find;
                    }
                }
            }
        }
        match got {
            Some(f) => fonts.push(f),
            None => match last.clone().and_then(|b| ab_glyph::FontVec::try_from_vec(b).ok()) {
                Some(f) => fonts.push(f),
                None => return Err("no fonts found".into()),
            },
        }
    }
    Ok(fonts)
}

fn texture_png(d: &Draw, bytes: &[u8]) -> Option<Texture> {
    let (w, h, px) = files::read_png(bytes)?;
    Some(d.texture_rgba(w, h, &px, crate::gl::LINEAR))
}

impl App {
    pub fn new() -> Result<App, String> {
        let paths = Paths::new();
        crate::log::open(&paths.log());
        crate::logln!("Continuum for TV {} starting", env!("CARGO_PKG_VERSION"));
        let sdl_path = paths.app.join("lib/libSDL2-2.0.so.0");
        let sdl = Sdl::open(&sdl_path.to_string_lossy()).or_else(|_| Sdl::open("libSDL2-2.0.so.0"))?;
        sdl.hint("SDL_WEBOS_ACCESS_POLICY_KEYS_BACK", "true");
        sdl.hint("SDL_WEBOS_ACCESS_POLICY_KEYS_EXIT", "true");
        sdl.hint("SDL_WEBOS_CURSOR_SLEEP_TIME", "1000");
        sdl.hint("SDL_GAMECONTROLLER_USE_BUTTON_LABELS", "0");
        unsafe {
            if (sdl.Init)(sdl::INIT_VIDEO | sdl::INIT_AUDIO | sdl::INIT_GAMECONTROLLER | sdl::INIT_JOYSTICK | sdl::INIT_EVENTS | sdl::INIT_TIMER) != 0 {
                return Err(format!("SDL didn't start: {}", sdl.error()));
            }
            (sdl.GL_SetAttribute)(sdl::GL_CONTEXT_PROFILE_MASK, sdl::GL_CONTEXT_PROFILE_ES);
            (sdl.GL_SetAttribute)(sdl::GL_CONTEXT_MAJOR_VERSION, 2);
            (sdl.GL_SetAttribute)(sdl::GL_CONTEXT_MINOR_VERSION, 0);
            (sdl.GL_SetAttribute)(sdl::GL_RED_SIZE, 8);
            (sdl.GL_SetAttribute)(sdl::GL_GREEN_SIZE, 8);
            (sdl.GL_SetAttribute)(sdl::GL_BLUE_SIZE, 8);
            (sdl.GL_SetAttribute)(sdl::GL_ALPHA_SIZE, 0);
            (sdl.GL_SetAttribute)(sdl::GL_DEPTH_SIZE, 0);
            (sdl.GL_SetAttribute)(sdl::GL_DOUBLEBUFFER, 1);
        }
        let title = CString::new("Continuum").unwrap();
        // webOS gives apps a 1920x1080 picture and scales it to the 4K panel (asking for
        // 3840x2160 still gives 1920x1080, measured on the owner's TV).
        let (ww, wh) = if std::env::var_os("CONTINUUM_WINDOWED").is_some() { (1280, 720) } else { (1920, 1080) };
        let flags = sdl::WINDOW_OPENGL | sdl::WINDOW_SHOWN | if std::env::var_os("CONTINUUM_WINDOWED").is_some() { 0 } else { sdl::WINDOW_FULLSCREEN };
        let win = unsafe { (sdl.CreateWindow)(title.as_ptr(), 0, 0, ww, wh, flags) };
        if win.is_null() {
            return Err(format!("no window: {}", sdl.error()));
        }
        let ctx = unsafe { (sdl.GL_CreateContext)(win) };
        if ctx.is_null() {
            return Err(format!("no OpenGL: {}", sdl.error()));
        }
        unsafe {
            (sdl.GL_MakeCurrent)(win, ctx);
            if (sdl.GL_SetSwapInterval)(1) != 0 {
                crate::logln!("no vsync: {}", sdl.error());
            }
            (sdl.ShowCursor)(0);
        }
        let (mut dw, mut dh) = (0, 0);
        unsafe { (sdl.GL_GetDrawableSize)(win, &mut dw, &mut dh) };
        if dw <= 0 {
            dw = ww;
            dh = wh;
        }
        let getp = |name: &str| -> *mut c_void {
            let c = CString::new(name).unwrap();
            unsafe { (sdl.GL_GetProcAddress)(c.as_ptr()) }
        };
        let gl = crate::gl::Gl::load(&getp)?;
        crate::logln!("screen {dw}x{dh}, GPU {} ({})", gl.string(crate::gl::RENDERER), gl.string(crate::gl::VERSION));
        let fonts = load_fonts()?;
        let d = Draw::new(gl, fonts, dw, dh)?;
        let images = Images {
            gb: texture_png(&d, include_bytes!("../assets/console-gb.png")),
            gbc: texture_png(&d, include_bytes!("../assets/console-gbc.png")),
            gba: texture_png(&d, include_bytes!("../assets/console-gba.png")),
        };
        let s = Settings::load(&paths.settings());
        let lib = Library::load(&paths);
        let core_path = paths.app.join("cores/libcontinuum_gb.so").to_string_lossy().into_owned();
        let core_vals: HashMap<String, String> = std::fs::read_to_string(paths.core_options())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let mut input = Input::new();
        input.open_pads(&sdl);
        let mut app = App {
            sdl,
            win,
            d,
            palette: Palette::new(&s.swirl_colours()),
            palette_key: String::new(),
            s,
            paths,
            lib,
            input,
            page: 0,
            focus: [0; 5],
            scroll: [0.0; 5],
            scroll_anim: [0.0; 5],
            page_started: Instant::now(),
            filter: None,
            layers: Vec::new(),
            session: None,
            fast_latch: false,
            audio: None,
            game_tex: None,
            swirl_tex: None,
            swirl_buf: Vec::new(),
            swirl_clock: 0.0,
            swirl_drawn: -1.0,
            items: Vec::new(),
            acts: HashMap::new(),
            anim: HashMap::new(),
            hscroll: HashMap::new(),
            images,
            upload: None,
            qr_tex: None,
            thumbs: HashMap::new(),
            toast: None,
            start: Instant::now(),
            last_frame: Instant::now(),
            core_path,
            core_defs: (Vec::new(), Vec::new()),
            core_hidden: HashMap::new(),
            core_defaults: HashMap::new(),
            core_vals,
            core_version: String::new(),
            open_cards: HashSet::new(),
            settings_dirty: None,
            core_dirty: None,
            refresh: 60.0,
            frame_times: Vec::new(),
            vsync_ok: true,
            quit: false,
            last_scan: Instant::now(),
            debug_input: false,
        };
        app.debug_input = app.paths.data.join("debug-input").exists();
        app.read_core_options();
        Ok(app)
    }

    /// Open cBoy once to learn its settings (it declares them as soon as it is opened).
    fn read_core_options(&mut self) {
        let dirs = retro::CoreDirs {
            system: self.paths.system.to_string_lossy().into_owned(),
            saves: self.paths.saves.to_string_lossy().into_owned(),
        };
        match retro::Core::open(&self.core_path, &dirs, self.core_preset()) {
            Ok(core) => {
                self.core_version = core.info.version.split(' ').next().unwrap_or("").to_string();
                self.take_core_options();
                drop(core);
            }
            Err(e) => crate::logln!("cBoy didn't open: {e}"),
        }
    }

    fn take_core_options(&mut self) {
        let h = retro::host();
        self.core_defs = (h.options.defs.clone(), h.options.categories.clone());
        self.core_hidden = h.options.hidden.clone();
        self.core_defaults = h.options.defs.iter().map(|d| (d.key.clone(), d.default.clone())).collect();
    }

    /// The core's options to start a game with: the player's choices, and the TV's own defaults
    /// where a phone default makes no sense (there is no motion sensor in a TV).
    fn core_preset(&self) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = self.core_vals.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (k, dv) in [("cboy_tilt_source", "stick"), ("cboy_gyro_source", "stick")] {
            if !self.core_vals.contains_key(k) {
                v.push((k.to_string(), dv.to_string()));
            }
        }
        v
    }

    fn toast(&mut self, s: impl Into<String>) {
        self.toast = Some((s.into(), Instant::now()));
    }

    // ------------------------------------------------------------------------------------
    // the loop

    pub fn run(&mut self) {
        while !self.quit {
            let now = Instant::now();
            let dt = now.duration_since(self.last_frame).as_secs_f64().min(0.1);
            self.last_frame = now;
            self.measure_refresh(dt);
            self.events();
            self.input.tick();
            if self.session.is_some() && self.layers.is_empty() {
                self.game_frame();
            } else {
                self.menu_input();
            }
            self.housekeeping();
            self.draw_frame(dt as f32);
            unsafe { (self.sdl.GL_SwapWindow)(self.win) };
            if !self.vsync_ok {
                let spent = now.elapsed().as_secs_f64();
                let want = 1.0 / 60.0;
                if spent < want {
                    std::thread::sleep(Duration::from_secs_f64(want - spent));
                }
            }
        }
        self.shutdown();
    }

    fn measure_refresh(&mut self, dt: f64) {
        if self.frame_times.len() < 240 {
            if dt > 0.0 {
                self.frame_times.push(dt);
            }
            if self.frame_times.len() == 240 {
                let mut v = self.frame_times[60..].to_vec();
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let med = v[v.len() / 2];
                if med < 0.012 {
                    self.vsync_ok = false;
                    crate::logln!("the screen doesn't wait for refresh ({:.1} ms frames); pacing by timer", med * 1000.0);
                } else {
                    let hz = 1.0 / med;
                    self.refresh = if (55.0..62.0).contains(&hz) { 60.0 } else if (48.0..52.0).contains(&hz) { 50.0 } else { hz };
                    crate::logln!("screen refresh {:.2} Hz (using {})", hz, self.refresh);
                }
            }
        }
    }

    fn events(&mut self) {
        let mut e = Event::new();
        let in_game = self.session.is_some() && self.layers.is_empty();
        while unsafe { (self.sdl.PollEvent)(&mut e) } != 0 {
            if self.debug_input {
                let k = e.kind();
                if k == sdl::KEYDOWN || k == sdl::KEYUP {
                    let (sc, kc, rep) = e.key();
                    crate::logln!("event {k:#x} scancode {sc} keycode {kc} repeat {rep}");
                } else if k != 0x400 && k != 0x401 && k != 0x402 && k != 0x403 {
                    crate::logln!("event {k:#x}");
                }
            }
            match e.kind() {
                sdl::QUIT | sdl::APP_TERMINATING => self.quit = true,
                sdl::APP_WILLENTERBACKGROUND | sdl::APP_DIDENTERBACKGROUND => self.went_away(),
                sdl::WINDOWEVENT => {
                    let w = e.window_event();
                    if w == sdl::WINDOWEVENT_FOCUS_LOST || w == sdl::WINDOWEVENT_HIDDEN {
                        self.went_away();
                    }
                }
                _ => {
                    self.input.event(&self.sdl, &e, &self.s, in_game);
                }
            }
        }
    }

    /// The TV switched to something else (Home button, an input): pause and save at once.
    fn went_away(&mut self) {
        if let Some(sess) = self.session.as_mut() {
            sess.save_battery(&self.paths, true);
            if self.s.auto_save {
                sess.save_state(&self.paths, None);
            }
            if self.layers.is_empty() {
                self.layers.push(Layer { ov: Overlay::Pause, focus: id("p:resume"), scroll: 0.0, scroll_anim: 0.0 });
            }
        }
        if let Some(a) = self.audio.as_mut() {
            a.pause(&self.sdl);
        }
        self.save_settings_now();
    }

    fn housekeeping(&mut self) {
        if let Some(t) = self.settings_dirty {
            if t.elapsed() > Duration::from_millis(800) {
                self.save_settings_now();
            }
        }
        if let Some(t) = self.core_dirty {
            if t.elapsed() > Duration::from_millis(800) {
                if let Ok(txt) = serde_json::to_string_pretty(&self.core_vals) {
                    files::write_atomic(self.paths.core_options(), txt.as_bytes());
                }
                self.core_dirty = None;
            }
        }
        // games arriving through either transfer show up by themselves
        let arrived = self.upload.as_ref().map(|u| std::mem::take(&mut u.shared.lock().unwrap().changed)).unwrap_or(false);
        if self.session.is_none() && (arrived || self.last_scan.elapsed() > Duration::from_secs(20)) {
            let before = self.lib.games.len();
            self.lib.scan(&self.paths);
            self.last_scan = Instant::now();
            let n = self.lib.games.len();
            if n > before {
                let msg = if n - before == 1 { "A new game is in your library".to_string() } else { format!("{} new games are in your library", n - before) };
                self.toast(msg);
            }
        }
        if let Some(u) = &self.upload {
            if !u.running() {
                self.upload = None;
                self.qr_tex = None;
            }
        }
    }

    fn save_settings_now(&mut self) {
        self.s.save(&self.paths.settings());
        self.settings_dirty = None;
    }

    fn shutdown(&mut self) {
        if let Some(mut sess) = self.session.take() {
            sess.finish(&self.paths, self.s.auto_save);
        }
        self.save_settings_now();
        if let Ok(txt) = serde_json::to_string_pretty(&self.core_vals) {
            files::write_atomic(self.paths.core_options(), txt.as_bytes());
        }
        if let Some(u) = &self.upload {
            u.stop();
        }
        if let Some(a) = self.audio.as_mut() {
            a.close(&self.sdl);
        }
        crate::logln!("Continuum closed");
        unsafe { (self.sdl.Quit)() };
    }

    // ------------------------------------------------------------------------------------
    // playing

    fn play(&mut self, game_id: &str, fresh: bool) {
        let g = match self.lib.find(game_id).and_then(|i| self.lib.by_index(i)).cloned() {
            Some(g) => g,
            None => return,
        };
        if let Some(mut old) = self.session.take() {
            old.finish(&self.paths, self.s.auto_save);
        }
        match Session::start(&self.paths, &self.core_path, &g, self.core_preset(), self.s.auto_resume && !fresh) {
            Ok(sess) => {
                let rate = sess.core.av.timing.sample_rate.round() as u32;
                let need_new = self.audio.as_ref().map(|a| a.rate != rate).unwrap_or(true);
                if need_new {
                    if let Some(a) = self.audio.as_mut() {
                        a.close(&self.sdl);
                    }
                    self.audio = Audio::open(&self.sdl, rate.max(8000), self.s.audio_latency_ms);
                }
                if let Some(a) = self.audio.as_mut() {
                    a.target_ms = self.s.audio_latency_ms;
                    a.pause(&self.sdl);
                }
                self.take_core_options();
                if sess.resumed {
                    self.toast("Picked up where you left off");
                }
                crate::logln!("playing {} ({:?}), {:?}", g.id, g.system, sess.core.av);
                self.session = Some(sess);
                self.lib.mark_played(&g.id);
                self.layers.clear();
                self.input.clear();
                self.fast_latch = false;
            }
            Err(e) => {
                crate::logln!("couldn't start {}: {e}", g.id);
                self.toast(format!("That game didn't start: {e}"));
            }
        }
    }

    fn quit_game(&mut self) {
        if let Some(mut sess) = self.session.take() {
            sess.finish(&self.paths, self.s.auto_save);
            let id = sess.game.id.clone();
            self.thumbs.remove(&id);
            crate::logln!("left {id} after {} frames", sess.frames);
        }
        if let Some(a) = self.audio.as_mut() {
            a.pause(&self.sdl);
        }
        self.input.rumble(&self.sdl, 0);
        self.layers.clear();
        self.input.clear();
        self.lib.scan(&self.paths);
    }

    fn open_pause(&mut self) {
        if let Some(a) = self.audio.as_mut() {
            a.pause(&self.sdl);
        }
        if let Some(sess) = self.session.as_mut() {
            sess.save_battery(&self.paths, true);
        }
        self.input.rumble(&self.sdl, 0);
        self.input.clear();
        self.layers.push(Layer { ov: Overlay::Pause, focus: id("p:resume"), scroll: 0.0, scroll_anim: 0.0 });
    }

    fn game_frame(&mut self) {
        let pressed: Vec<Action> = std::mem::take(&mut self.input.pressed);
        for a in pressed {
            match a {
                Action::Menu => {
                    self.open_pause();
                    return;
                }
                Action::SaveState => {
                    let ok = self.session.as_mut().map(|s| s.save_state(&self.paths, Some(0))).unwrap_or(false);
                    self.toast(if ok { "Quick saved" } else { "That didn't save" });
                }
                Action::LoadState => {
                    let ok = self.session.as_mut().map(|s| s.load_state(&self.paths, Some(0))).unwrap_or(false);
                    self.toast(if ok { "Quick loaded" } else { "No quick save yet" });
                }
                _ => {}
            }
        }
        let (mask, stick, held_fast) = self.input.game_state(&self.s);
        let fast = held_fast || self.fast_latch;
        let runs = if fast { self.s.fast_speed.max(2) } else { 1 };
        let sess = match self.session.as_mut() {
            Some(s) => s,
            None => return,
        };
        retro::host().fast_forwarding = fast;
        for _ in 0..runs {
            retro::host().audio.clear();
            sess.run(mask, stick);
            if !fast {
                if let Some(a) = self.audio.as_mut() {
                    let t = sess.core.av.timing;
                    let samples = std::mem::take(&mut retro::host().audio);
                    a.push(&self.sdl, &samples, t.sample_rate, t.fps, self.refresh, self.s.volume);
                    retro::host().audio = samples;
                }
            }
        }
        if fast {
            if let Some(a) = self.audio.as_mut() {
                a.pause(&self.sdl);
            }
        }
        let h = retro::host();
        if h.rumble_changed {
            h.rumble_changed = false;
            if self.s.rumble {
                self.input.rumble(&self.sdl, h.rumble[0]);
            }
        }
        if h.shutdown {
            h.shutdown = false;
            self.quit_game();
            return;
        }
        sess.save_battery(&self.paths, false);
        // upload the new picture
        if h.frame_new && h.frame_w > 0 {
            h.frame_new = false;
            let (w, hh) = (h.frame_w, h.frame_h);
            let bytes = unsafe { std::slice::from_raw_parts(h.frame.as_ptr() as *const u8, h.frame.len() * 4) };
            match self.game_tex.as_mut() {
                Some(t) => self.d.update_texture(t, w, hh, bytes),
                None => self.game_tex = Some(self.d.texture_rgba(w, hh, bytes, crate::gl::LINEAR)),
            }
        }
    }

    // ------------------------------------------------------------------------------------
    // menus

    fn cur_focus(&self) -> u64 {
        match self.layers.last() {
            Some(l) => l.focus,
            None => self.focus[self.page],
        }
    }

    fn set_focus(&mut self, f: u64) {
        match self.layers.last_mut() {
            Some(l) => l.focus = f,
            None => self.focus[self.page] = f,
        }
        // focusing a tab switches to it, the TV way
        if self.layers.is_empty() {
            if let Some(Act::Tab(n)) = self.acts.get(&f).cloned() {
                if n != self.page {
                    self.switch_page(n, true);
                }
            }
        }
    }

    fn switch_page(&mut self, n: usize, keep_tab_focus: bool) {
        if n == self.page {
            return;
        }
        self.page = n;
        self.page_started = Instant::now();
        if keep_tab_focus && n < 4 {
            self.focus[n] = id(&format!("tab:{n}"));
        }
        if n == PAGE_ADD {
            self.open_upload();
        }
    }

    fn open_upload(&mut self) {
        if self.upload.as_ref().map(|u| u.running()).unwrap_or(false) {
            return;
        }
        match Upload::start(self.paths.games.clone()) {
            Ok(u) => {
                self.qr_tex = None;
                self.upload = Some(u);
            }
            Err(e) => self.toast(e),
        }
    }

    fn menu_input(&mut self) {
        let navs: Vec<Nav> = std::mem::take(&mut self.input.nav);
        self.input.pressed.clear();
        for n in navs {
            let f = self.cur_focus();
            let item = self.items.iter().find(|i| i.id == f).copied();
            match n {
                Nav::Up | Nav::Down | Nav::Left | Nav::Right => {
                    let (dx, dy) = match n {
                        Nav::Up => (0.0, -1.0),
                        Nav::Down => (0.0, 1.0),
                        Nav::Left => (-1.0, 0.0),
                        _ => (1.0, 0.0),
                    };
                    if let Some(it) = item {
                        if it.kind == Kind::Adjust && dx != 0.0 {
                            if let Some(Act::Row(r)) = self.acts.get(&f).cloned() {
                                self.adjust(&r, dx as i32);
                            }
                            continue;
                        }
                        if let Some(to) = ui::navigate(&self.items, f, dx, dy) {
                            self.set_focus(to);
                        }
                    } else if let Some(first) = self.items.first() {
                        let fid = first.id;
                        self.set_focus(fid);
                    }
                }
                Nav::Ok => {
                    if let Some(a) = self.acts.get(&f).cloned() {
                        self.activate(a);
                    }
                }
                Nav::Back => self.back(),
                Nav::Options => {
                    if let Some(Act::Play(gid, _)) = self.acts.get(&f).cloned() {
                        self.push(Overlay::GameMenu(gid), id("gm:play"));
                    }
                }
                Nav::Favourite => {
                    if let Some(Act::Play(gid, _)) = self.acts.get(&f).cloned() {
                        let on = self.lib.toggle_favourite(&gid);
                        self.toast(if on { "Added to Favorites" } else { "Taken out of Favorites" });
                    }
                }
                Nav::TabPrev | Nav::TabNext => {
                    if self.layers.is_empty() {
                        let cur = if self.page >= 4 { 0 } else { self.page };
                        let n2 = if n == Nav::TabNext { (cur + 1) % 4 } else { (cur + 3) % 4 };
                        self.switch_page(n2, true);
                    }
                }
            }
        }
    }

    fn push(&mut self, ov: Overlay, focus: u64) {
        self.layers.push(Layer { ov, focus, scroll: 0.0, scroll_anim: 0.0 });
    }

    fn back(&mut self) {
        if let Some(l) = self.layers.last() {
            if matches!(l.ov, Overlay::Pause) {
                self.resume();
                return;
            }
            self.layers.pop();
            return;
        }
        if self.page == PAGE_ADD {
            self.switch_page(0, false);
            return;
        }
        let tab = id(&format!("tab:{}", self.page));
        if self.page != 0 {
            if self.focus[self.page] != tab {
                self.focus[self.page] = tab;
            } else {
                self.switch_page(0, true);
            }
            return;
        }
        if self.focus[0] != tab && self.scroll[0] > 0.0 {
            self.focus[0] = 0;
            self.scroll[0] = 0.0;
            return;
        }
        self.push(
            Overlay::Confirm { title: "Leave Continuum?".into(), body: "Your games and saves stay right here.".into(), yes: "Leave".into(), act: Act::QuitApp, danger: false },
            id("c:no"),
        );
    }

    fn resume(&mut self) {
        self.layers.clear();
        self.input.clear();
    }

    fn adjust(&mut self, r: &Row, dir: i32) {
        match r {
            Row::Choice { key, values, .. } => {
                let cur = settings_page::get(&self.s, &self.core_vals, &self.core_defaults, key);
                let i = values.iter().position(|(v, _)| *v == cur).unwrap_or(0) as i32;
                let n = values.len() as i32;
                if n == 0 {
                    return;
                }
                let j = ((i + dir) % n + n) % n;
                let v = values[j as usize].0.clone();
                self.change(key, &v);
            }
            Row::Slider { key, min, max, step, .. } => {
                let cur: f64 = settings_page::get(&self.s, &self.core_vals, &self.core_defaults, key).parse().unwrap_or(*min);
                let v = ((cur + *step * dir as f64) / step).round() * step;
                let v = v.clamp(*min, *max);
                self.change(key, &format!("{v}"));
            }
            Row::Toggle { key, .. } => {
                let cur = settings_page::get(&self.s, &self.core_vals, &self.core_defaults, key) == "true";
                self.change(key, if cur { "false" } else { "true" });
            }
            _ => {}
        }
    }

    fn change(&mut self, key: &str, v: &str) {
        let is_core = settings_page::set(&mut self.s, &mut self.core_vals, key, v);
        if is_core {
            let k = &key[5..];
            self.core_dirty = Some(Instant::now());
            if let Some(sess) = self.session.as_mut() {
                retro::host().options.set(k, v);
                sess.core.refresh_option_display();
                self.core_hidden = retro::host().options.hidden.clone();
            }
        } else {
            self.settings_dirty = Some(Instant::now());
            if key == "audio_latency_ms" {
                if let Some(a) = self.audio.as_mut() {
                    a.target_ms = self.s.audio_latency_ms;
                }
            }
        }
    }

    fn activate(&mut self, a: Act) {
        match a {
            Act::Tab(n) => self.switch_page(n, true),
            Act::AddGames => self.switch_page(PAGE_ADD, false),
            Act::Filter(f) => {
                self.filter = if self.filter == f { None } else { f };
                if self.page != 1 {
                    self.switch_page(1, false);
                }
            }
            Act::Play(gid, fresh) => self.play(&gid, fresh),
            Act::GameMenu(gid) => self.push(Overlay::GameMenu(gid), id("gm:play")),
            Act::Row(r) => match &r {
                Row::Toggle { .. } => self.adjust(&r, 1),
                Row::Choice { .. } => self.adjust(&r, 1),
                Row::Action { key, .. } => self.row_action(key),
                _ => {}
            },
            Act::Fold(k) => {
                if !self.open_cards.remove(k) {
                    self.open_cards.insert(k);
                }
            }
            Act::Sort => {
                let next = match self.s.library_sort.as_str() {
                    "name" => "added",
                    "added" => "played",
                    _ => "name",
                };
                self.change("library_sort", next);
            }
            Act::Layout => {
                let next = if self.s.library_layout == "console" { "grid" } else { "console" };
                self.change("library_layout", next);
            }
            Act::Shuffle => {
                let n = self.lib.games.len();
                if n > 0 {
                    let i = (files::now_secs() as usize ^ (self.start.elapsed().as_nanos() as usize)) % n;
                    let gid = self.lib.games[i].id.clone();
                    self.play(&gid, false);
                }
            }
            Act::Resume => self.resume(),
            Act::SaveState => {
                let ok = self.session.as_mut().map(|s| s.save_state(&self.paths, Some(0))).unwrap_or(false);
                self.toast(if ok { "Saved" } else { "That didn't save" });
                self.thumbs.clear();
            }
            Act::LoadLast => {
                let ok = self.session.as_mut().map(|s| s.load_state(&self.paths, Some(0))).unwrap_or(false);
                if ok {
                    self.resume();
                    self.toast("Loaded");
                } else {
                    self.toast("No save yet");
                }
            }
            Act::Slots(gid) => self.push(Overlay::Slots(gid), id("slot:1")),
            Act::Reset => {
                if let Some(s) = self.session.as_mut() {
                    s.core.reset();
                }
                self.resume();
            }
            Act::FastLatch => {
                self.fast_latch = !self.fast_latch;
                self.resume();
                self.toast(if self.fast_latch { "Fast forward on" } else { "Fast forward off" });
            }
            Act::GameSettings => self.push(Overlay::Settings, 0),
            Act::QuitGame => self.quit_game(),
            Act::Slot(gid, n) => self.push(Overlay::SlotMenu(gid, n), id("sm:0")),
            Act::SlotSave(n) => {
                let ok = self.session.as_mut().map(|s| s.save_state(&self.paths, Some(n))).unwrap_or(false);
                self.thumbs.clear();
                self.layers.pop();
                self.toast(if ok { format!("Saved to slot {n}") } else { "That didn't save".into() });
            }
            Act::SlotLoad(gid, n) => {
                if self.session.as_ref().map(|s| s.game.id != gid).unwrap_or(true) {
                    self.play(&gid, true);
                }
                let ok = self.session.as_mut().map(|s| s.load_state(&self.paths, Some(n))).unwrap_or(false);
                self.resume();
                self.toast(if ok { format!("Loaded slot {n}") } else { "That save didn't load".into() });
            }
            Act::SlotDelete(gid, n) => {
                self.push(
                    Overlay::Confirm {
                        title: format!("Empty slot {n}?"),
                        body: "That save state goes for good. In-game saves aren't touched.".into(),
                        yes: "Empty it".into(),
                        act: Act::SlotDeleteNow(gid, n),
                        danger: true,
                    },
                    id("c:no"),
                );
            }
            Act::SlotDeleteNow(gid, n) => {
                let f = files::state_file(&self.paths, &gid, Some(n));
                let _ = std::fs::remove_file(&f);
                let _ = std::fs::remove_file(f.with_extension("png"));
                self.thumbs.clear();
                // close the slot's menu, back to the slots
                if matches!(self.layers.last(), Some(Layer { ov: Overlay::SlotMenu(..), .. })) {
                    self.layers.pop();
                }
                self.toast(format!("Slot {n} is empty again"));
            }
            Act::Favourite(gid) => {
                let on = self.lib.toggle_favourite(&gid);
                self.toast(if on { "Added to Favorites" } else { "Taken out of Favorites" });
                self.layers.pop();
            }
            Act::Remove(gid) => {
                let title = self.lib.find(&gid).and_then(|i| self.lib.by_index(i)).map(|g| g.title.clone()).unwrap_or_default();
                self.push(
                    Overlay::Confirm {
                        title: format!("Take {title} off the TV?"),
                        body: "Your saves stay, so it picks up again if you add it back.".into(),
                        yes: "Take it off".into(),
                        act: Act::RemoveNow(gid),
                        danger: true,
                    },
                    id("c:no"),
                );
            }
            Act::RemoveNow(gid) => {
                self.lib.remove(&gid, &self.paths);
                self.layers.clear();
                self.toast("Taken off the TV");
            }
            Act::ConfirmYes => {
                let act = match self.layers.last() {
                    Some(Layer { ov: Overlay::Confirm { act, .. }, .. }) => act.clone(),
                    _ => Act::Close,
                };
                self.layers.pop();
                self.activate(act);
            }
            Act::Close => {
                self.layers.pop();
            }
            Act::QuitApp => self.quit = true,
        }
    }

    fn row_action(&mut self, key: &str) {
        match key {
            "reset_look" => {
                self.s.reset_look();
                self.settings_dirty = Some(Instant::now());
                self.toast("Back to gold on charcoal");
            }
            "reset_controls" => {
                self.s.reset_controls();
                self.settings_dirty = Some(Instant::now());
                self.toast("Buttons are back to how they started");
            }
            "reset_core" => {
                self.core_vals.clear();
                self.core_dirty = Some(Instant::now());
                if self.session.is_some() {
                    for (k, v) in self.core_preset().into_iter().chain(self.core_defaults.clone()) {
                        retro::host().options.set(&k, &v);
                    }
                }
                self.toast("cBoy's settings are back to how they started");
            }
            "rescan" => {
                self.lib.scan(&self.paths);
                let n = self.lib.games.len();
                self.toast(format!("{n} game{} in your library", if n == 1 { "" } else { "s" }));
            }
            "add_games" => self.switch_page(PAGE_ADD, false),
            "quit_app" => self.push(
                Overlay::Confirm { title: "Leave Continuum?".into(), body: "Your games and saves stay right here.".into(), yes: "Leave".into(), act: Act::QuitApp, danger: false },
                id("c:no"),
            ),
            _ => {}
        }
    }

    // ------------------------------------------------------------------------------------
    // drawing

    fn update_swirl(&mut self, dt: f64, moving: bool) {
        let key = format!("{}|{}|{}", self.s.accent, self.s.swirl_source, self.s.swirl_own);
        if key != self.palette_key {
            self.palette = Palette::new(&self.s.swirl_colours());
            self.palette_key = key;
            self.swirl_drawn = -1.0;
        }
        let t = if moving { self.swirl_clock + dt * self.s.swirl_speed } else { self.swirl_clock };
        self.swirl_clock = t;
        let shown = if moving { t } else { 37.0 + t * 0.0 };
        // 30 updates a second is plenty for something this slow
        if self.swirl_drawn >= 0.0 && (shown - self.swirl_drawn).abs() < 1.0 / 30.0 * self.s.swirl_speed.max(0.25) && self.swirl_tex.is_some() {
            return;
        }
        self.swirl_drawn = shown;
        let base = swirl::hex_rgb(&self.s.background);
        let k = self.s.swirl_strength.min(1.0);
        let mut field = std::mem::take(&mut self.swirl_buf);
        // The phone's swirl is laid out on a tall screen, measured in screen widths. A TV is
        // wide, so the same field is worked out for a screen standing on its side (measured in
        // the TV's height) and turned a quarter: the clouds keep the size and spacing they have
        // on the phone, instead of each one spreading over half the screen.
        let (fw, fh) = swirl::field(shown, VW as f64 / VH as f64, &self.palette, k, base, &mut field);
        let (w, h) = (fh, fw);
        let mut buf = vec![255u8; (w * h * 4) as usize];
        for i in 0..fw as usize {
            for j in 0..fh as usize {
                let s = (j * fw as usize + i) * 4;
                let d = (i * w as usize + j) * 4;
                buf[d..d + 3].copy_from_slice(&field[s..s + 3]);
            }
        }
        self.swirl_buf = field;
        if w > 0 {
            match self.swirl_tex.as_mut() {
                Some(tex) => self.d.update_texture(tex, w, h, &buf),
                None => self.swirl_tex = Some(self.d.texture_rgba(w, h, &buf, crate::gl::LINEAR)),
            }
        }
    }

    fn draw_background(&mut self, dt: f64, allow_motion: bool) {
        let th_bg = ui::colour_of(&self.s.background);
        self.d.begin(th_bg);
        match self.s.background_style.as_str() {
            "swirl" => {
                self.update_swirl(dt, allow_motion && self.s.background_moves);
                if let Some(t) = &self.swirl_tex {
                    let o = swirl::OVER as f32;
                    self.d.swirl(t, -o * VW, -o * VH, (1.0 + 2.0 * o) * VW, (1.0 + 2.0 * o) * VH);
                }
            }
            "glow" => {
                let t = if allow_motion && self.s.background_moves { self.start.elapsed().as_secs_f64() } else { 12.0 };
                let tau = std::f64::consts::TAU;
                let breath = 0.5 + 0.5 * (t * tau / 7.0).sin();
                let (a, b) = ui::pair_of(&self.s.accent);
                let k = self.s.swirl_strength.max(0.2) as f32 / 0.7;
                let (w, h) = (VW as f64, VH as f64);
                self.d.radial((w * (0.72 + 0.05 * (t * tau / 19.0).sin())) as f32, (h * (0.22 + 0.015 * (t * tau / 23.0).sin())) as f32, (w * (0.66 + 0.04 * breath)) as f32, (w * (0.66 + 0.04 * breath) * 0.8) as f32, with_alpha(b, ((0.15 + 0.05 * breath) as f32) * k));
                self.d.radial((w * (0.25 - 0.06 * (t * tau / 29.0).sin())) as f32, (0.06 * h) as f32, (0.55 * w) as f32, (0.55 * w * 0.6) as f32, with_alpha(a, ((0.04 + 0.02 * (1.0 - breath)) as f32) * k));
                if allow_motion && self.s.background_moves {
                    let p = (t / 14.0).fract();
                    self.d.radial((0.5 * w) as f32, (1.03 * h) as f32, (0.75 * w) as f32, (0.75 * w * 0.3) as f32, with_alpha(b, 0.10 * k));
                    self.d.radial((w * (-0.25 + 1.5 * p)) as f32, h as f32, (0.42 * w) as f32, (0.42 * w * 0.36) as f32, with_alpha(a, (0.20 * (p * std::f64::consts::PI).sin()) as f32 * k));
                }
            }
            _ => {}
        }
    }

    fn draw_frame(&mut self, dt: f32) {
        let t = self.start.elapsed().as_secs_f64();
        let playing = self.session.is_some();
        if playing {
            // the game, with anything over it
            let bd = self.s.game_backdrop.clone();
            match bd.as_str() {
                "swirl" => self.draw_background(dt as f64, false),
                "dark" => self.d.begin(ui::colour_of(&self.s.background)),
                _ => self.d.begin([0.0, 0.0, 0.0, 1.0]),
            }
            self.draw_game();
            if !self.layers.is_empty() {
                self.d.rect(0.0, 0.0, VW, VH, 0.0, &Shape::fill([0.0, 0.0, 0.0, 0.55]));
            }
        } else {
            self.draw_background(dt as f64, true);
        }
        let th = Theme::new(&self.s, t);
        self.items.clear();
        self.acts.clear();
        if !playing {
            self.draw_page(&th, dt);
        }
        if !self.layers.is_empty() {
            // only the top sheet takes the remote
            self.items.clear();
            self.acts.clear();
            self.draw_layer(&th, dt);
        }
        // keep the focus on something that exists
        let f = self.cur_focus();
        if !self.items.is_empty() && !self.items.iter().any(|i| i.id == f) {
            let first = self.default_focus();
            self.set_focus_raw(first);
        }
        self.scroll_to_focus();
        self.draw_toast(&th);
        if playing && self.layers.is_empty() && self.s.show_fps {
            if let Some(s) = &self.session {
                let txt = format!("{:.0} fps", s.fps_shown);
                self.d.rect(VW - 190.0, 30.0, 150.0, 50.0, 25.0, &Shape::fill([0.0, 0.0, 0.0, 0.5]));
                self.d.text_center(&txt, VW - 115.0, 41.0, 26.0, MEDIUM, white(0.9));
            }
        }
        self.d.end();
    }

    fn set_focus_raw(&mut self, f: u64) {
        match self.layers.last_mut() {
            Some(l) => l.focus = f,
            None => self.focus[self.page] = f,
        }
    }

    fn default_focus(&self) -> u64 {
        // the first thing in the content, not the top bar or the tabs
        self.items.iter().find(|i| i.group == 0).or(self.items.first()).map(|i| i.id).unwrap_or(0)
    }

    fn scroll_to_focus(&mut self) {
        let f = self.cur_focus();
        let it = match self.items.iter().find(|i| i.id == f) {
            Some(i) => *i,
            None => return,
        };
        if it.group != 0 {
            return;
        }
        let (top, bottom) = if self.layers.is_empty() { (150.0, VH - 170.0) } else { (130.0, VH - 110.0) };
        let (scroll, anim) = match self.layers.last_mut() {
            Some(l) => (&mut l.scroll, &mut l.scroll_anim),
            None => (&mut self.scroll[self.page], &mut self.scroll_anim[self.page]),
        };
        // positions in the items are on screen with the drawn scroll; aim the scroll target
        let on_screen_y = it.r.y;
        let drawn = *anim;
        let content_y = on_screen_y + drawn;
        if content_y - *scroll < top {
            *scroll = (content_y - top).max(0.0);
        } else if content_y + it.r.h - *scroll > bottom {
            *scroll = content_y + it.r.h - bottom;
        }
        if *scroll < 0.0 {
            *scroll = 0.0;
        }
        let k = 0.22;
        *anim += (*scroll - *anim) * k;
        if (*anim - *scroll).abs() < 0.5 {
            *anim = *scroll;
        }
    }

    fn draw_game(&mut self) {
        let (sess, tex) = match (&self.session, &self.game_tex) {
            (Some(s), Some(t)) => (s, t),
            _ => return,
        };
        let h = retro::host();
        let (fw, fh) = (h.frame_w.max(1), h.frame_h.max(1));
        let g = sess.core.av.geometry;
        let aspect = if g.aspect_ratio > 0.0 { g.aspect_ratio } else { fw as f32 / fh as f32 };
        let (w, hh) = match self.s.scale.as_str() {
            "stretch" => (VW, VH),
            "integer" => {
                // whole multiples of the real pixels on the TV's 1080 lines
                let scale = self.d.scale;
                let k = ((VH * scale) / fh as f32).floor().min(((VW * scale) / fw as f32).floor()).max(1.0);
                (fw as f32 * k / scale, fh as f32 * k / scale)
            }
            _ => {
                let hh = VH;
                let w = (hh * aspect).min(VW);
                (w, w / aspect)
            }
        };
        let x = (VW - w) * 0.5;
        let y = (VH - hh) * 0.5;
        let sharp = self.s.sharp;
        let grid = self.s.lcd_grid as f32;
        self.d.game(tex, fw, fh, x, y, w, hh, sharp, grid);
    }

    fn draw_toast(&mut self, th: &Theme) {
        let (msg, at) = match &self.toast {
            Some(t) => t.clone(),
            None => return,
        };
        let age = at.elapsed().as_secs_f32();
        if age > 2.8 {
            self.toast = None;
            return;
        }
        let a = (age / 0.2).min(1.0) * ((2.8 - age) / 0.3).min(1.0);
        let w = self.d.measure(&msg, 28.0, MEDIUM, 0.0) + 70.0;
        let x = (VW - w) * 0.5;
        let y = 52.0 - (1.0 - a) * 20.0;
        self.d.glow(x, y + 6.0, w, 70.0, 35.0, 14.0, [0.0, 0.0, 0.0, 0.5 * a]);
        self.d.rect(x, y, w, 70.0, 35.0, &Shape::fill([0.09, 0.09, 0.11, 0.92 * a]).border(1.0, with_alpha(th.first, 0.5 * a)));
        self.d.text_center(&msg, VW * 0.5, y + 20.0, 28.0, MEDIUM, white(a));
    }

    // ---- pages ----

    fn draw_page(&mut self, th: &Theme, dt: f32) {
        let page = self.page;
        let scroll = self.scroll_anim[page];
        let rise = if self.s.page_fades { (1.0 - (self.page_started.elapsed().as_secs_f32() / 0.35).min(1.0)).powi(2) * 18.0 } else { 0.0 };
        let off = -scroll + rise;
        let focus = self.focus[page];
        let App { d, items, acts, anim, images, lib, s, filter, hscroll, upload, qr_tex, core_defs, core_hidden, core_vals, core_defaults, core_version, open_cards, paths, .. } = self;
        let mut cx = Ctx { d, th, focus, items, anim, dt, group: 0, images };
        // content, clipped below the top bar
        cx.d.push_clip(0.0, 120.0, VW, VH - 120.0);
        match page {
            0 => page_home(&mut cx, acts, lib, s, off, hscroll),
            1 => page_library(&mut cx, acts, lib, s, *filter, off),
            2 => page_favorites(&mut cx, acts, lib, s, off),
            3 => {
                let cards = settings_page::cards(s, core_defs, core_hidden, core_version, false);
                page_settings(&mut cx, acts, s, core_vals, core_defaults, &cards, open_cards, off, 80.0, VW - 160.0, 150.0);
            }
            _ => page_add(&mut cx, acts, upload.as_ref(), qr_tex, off, paths),
        }
        cx.d.pop_clip();
        // top bar
        cx.group = 1;
        cx.wordmark(80.0, 46.0);
        let mut right = VW - 80.0;
        if page != PAGE_ADD {
            let w = cx.d.measure("+  Add games", 24.0, MEDIUM, 0.0) + 48.0;
            right -= w;
            cx.bead("top:add", right, 34.0, "+  Add games");
            acts.insert(id("top:add"), Act::AddGames);
            right -= 24.0;
        }
        if page < 3 {
            let hint = "OK to play  ·  Hold OK for options  ·  Green to favourite";
            let w = cx.d.measure(hint, 21.0, REGULAR, 0.0);
            cx.d.text(hint, right - w - 10.0, 53.0, 21.0, REGULAR, white(0.45), 0.0);
        }
        // tab bar
        cx.group = 2;
        let tw = 230.0;
        let bw = tw * 4.0 + 20.0;
        let bx = (VW - bw) * 0.5;
        let by = VH - 118.0;
        cx.d.glow(bx, by + 8.0, bw, 88.0, 44.0, 18.0, [0.0, 0.0, 0.0, 0.45]);
        cx.d.rect(bx, by, bw, 88.0, 44.0, &Shape::fill([th.bg[0], th.bg[1], th.bg[2], 0.72]).border(1.0, white(0.10)));
        for (i, name) in TAB_NAMES.iter().enumerate() {
            let key = format!("tab:{i}");
            let iid = id(&key);
            let r = Rect::new(bx + 10.0 + i as f32 * tw, by + 10.0, tw, 68.0);
            cx.add(iid, r, Kind::Normal);
            acts.insert(iid, Act::Tab(i));
            let amt = cx.focus_amt(iid);
            if i == page {
                cx.d.glow(r.x, r.y, r.w, r.h, 34.0, 10.0, with_alpha(th.second, 0.6));
                cx.d.rect(r.x, r.y, r.w, r.h, 34.0, &th.accent_fill());
                cx.d.text_center(name, r.cx(), r.y + 19.0, 30.0, BOLD, th.on_accent);
            } else {
                cx.d.text_center(name, r.cx(), r.y + 19.0, 30.0, MEDIUM, white(0.6 + 0.4 * amt));
            }
            if amt > 0.01 {
                cx.d.rect(r.x - 4.0, r.y - 4.0, r.w + 8.0, r.h + 8.0, 38.0, &Shape::fill([0.0; 4]).border(3.0, white(0.9 * amt)));
            }
        }
    }

    // ---- sheets ----

    fn draw_layer(&mut self, th: &Theme, dt: f32) {
        let li = self.layers.len() - 1;
        let focus = self.layers[li].focus;
        let off = -self.layers[li].scroll_anim;
        let in_game = self.session.is_some();
        let slots = self.s.state_slots;
        let App { d, items, acts, anim, images, lib, s, session, layers, paths, thumbs, core_defs, core_hidden, core_vals, core_defaults, core_version, open_cards, fast_latch, .. } = self;
        let mut cx = Ctx { d, th, focus, items, anim, dt, group: 0, images };
        let layer = &layers[li];
        match &layer.ov {
            Overlay::Pause => {
                let sess = match session.as_ref() {
                    Some(s) => s,
                    None => return,
                };
                let g = sess.game.clone();
                let r = Rect::new(360.0, 120.0, 1200.0, 840.0);
                sheet(&mut cx, r, th);
                let cover = Rect::new(r.x + 60.0, r.y + 56.0, 150.0, 150.0);
                if th.hero_glow {
                    cx.d.radial(cover.cx(), cover.cy(), 200.0, 200.0, with_alpha(th.second, 0.45));
                }
                cx.cover(&g, cover, 18.0, 1.0);
                cx.d.text("PAUSED", r.x + 250.0, r.y + 66.0, 22.0, BOLD, th.first, 2.4);
                cx.d.text_fit(&g.title, r.x + 250.0, r.y + 100.0, 44.0, BOLD, white(1.0), r.w - 320.0);
                let meta = if g.tags.is_empty() { g.system.name().to_string() } else { format!("{} · {}", g.system.name(), g.tags) };
                cx.d.text_fit(&meta, r.x + 250.0, r.y + 156.0, 24.0, REGULAR, white(0.55), r.w - 320.0);
                let pr = cx.primary("p:resume", r.x + 60.0, r.y + 250.0, "Resume", r.w - 120.0);
                acts.insert(id("p:resume"), Act::Resume);
                let quick = files::state_file(paths, &g.id, Some(0));
                let quick_age = if quick.exists() { files::ago(files::mtime(&quick)) } else { String::new() };
                let fast_label = if *fast_latch { "Fast forward: on" } else { "Fast forward" };
                let tiles: Vec<(&str, String, Act)> = vec![
                    ("p:save", "Save".to_string(), Act::SaveState),
                    ("p:load", if quick_age.is_empty() { "Load".to_string() } else { format!("Load ({quick_age})") }, Act::LoadLast),
                    ("p:slots", "Save slots".to_string(), Act::Slots(g.id.clone())),
                    ("p:reset", "Restart".to_string(), Act::Reset),
                    ("p:fast", fast_label.to_string(), Act::FastLatch),
                    ("p:settings", "Settings".to_string(), Act::GameSettings),
                ];
                let (tw, thh, gap) = (346.0, 110.0, 21.0);
                for (i, (k, label, act)) in tiles.into_iter().enumerate() {
                    let col = (i % 3) as f32;
                    let row = (i / 3) as f32;
                    let tr = Rect::new(r.x + 60.0 + col * (tw + gap), pr.y + pr.h + 40.0 + row * (thh + gap), tw, thh);
                    tile(&mut cx, k, tr, &label);
                    acts.insert(id(k), act);
                }
                let qy = pr.y + pr.h + 40.0 + 2.0 * (thh + gap) + 20.0;
                let qr = Rect::new(r.x + 60.0, qy, r.w - 120.0, 80.0);
                danger_bar(&mut cx, "p:quit", qr, "Quit to library", th);
                acts.insert(id("p:quit"), Act::QuitGame);
            }
            Overlay::GameMenu(gid) => {
                let g = match lib.find(gid).and_then(|i| lib.by_index(i)) {
                    Some(g) => g.clone(),
                    None => return,
                };
                let r = Rect::new(460.0, 170.0, 1000.0, 740.0);
                sheet(&mut cx, r, th);
                let cover = Rect::new(r.x + 60.0, r.y + 60.0, 200.0, 200.0);
                if th.hero_glow {
                    cx.d.radial(cover.cx(), cover.cy(), 260.0, 260.0, with_alpha(th.second, 0.4));
                }
                cx.cover(&g, cover, 22.0, 1.0);
                let lines = cx.d.wrap(&g.title, 42.0, BOLD, r.w - 380.0);
                let mut y = r.y + 70.0;
                for l in lines.iter().take(2) {
                    cx.d.text(l, r.x + 300.0, y, 42.0, BOLD, white(1.0), 0.0);
                    y += 50.0;
                }
                let played = if g.played > 0 { format!("Played {}", files::ago(g.played)) } else { "Not played yet".to_string() };
                cx.d.text_fit(&format!("{} · {played}", g.system.name()), r.x + 300.0, y + 6.0, 24.0, REGULAR, white(0.55), r.w - 360.0);
                let auto = files::state_file(paths, &g.id, None).exists();
                let label = if auto { "Resume" } else { "Play" };
                let pr = cx.primary("gm:play", r.x + 60.0, r.y + 300.0, label, r.w - 120.0);
                acts.insert(id("gm:play"), Act::Play(g.id.clone(), false));
                let mut y = pr.y + pr.h + 30.0;
                let mut rows: Vec<(&str, String, Act, bool)> = Vec::new();
                if auto {
                    rows.push(("gm:fresh", "Start from the title screen".into(), Act::Play(g.id.clone(), true), false));
                }
                rows.push(("gm:fav", if g.favourite { "Take out of Favorites".into() } else { "Add to Favorites".into() }, Act::Favourite(g.id.clone()), false));
                rows.push(("gm:slots", "Save slots".into(), Act::Slots(g.id.clone()), false));
                rows.push(("gm:remove", "Take it off the TV".into(), Act::Remove(g.id.clone()), true));
                for (k, label, act, danger) in rows {
                    let rr = Rect::new(r.x + 60.0, y, r.w - 120.0, 70.0);
                    cx.action_row(k, rr, &label, "", danger);
                    acts.insert(id(k), act);
                    y += 78.0;
                }
            }
            Overlay::Slots(gid) => {
                let r = Rect::new(310.0, 90.0, 1300.0, 900.0);
                sheet(&mut cx, r, th);
                cx.d.text("SAVE SLOTS", r.x + 60.0, r.y + 50.0, 22.0, BOLD, th.first, 2.4);
                let title = lib.find(gid).and_then(|i| lib.by_index(i)).map(|g| g.title.clone()).unwrap_or_default();
                cx.d.text_fit(&title, r.x + 60.0, r.y + 84.0, 40.0, BOLD, white(1.0), r.w - 120.0);
                cx.d.push_clip(r.x, r.y + 150.0, r.w, r.h - 170.0);
                let (cw, chh, gap) = (380.0, 300.0, 30.0);
                for n in 1..=slots {
                    let i = (n - 1) as f32;
                    let col = (i as u32 % 3) as f32;
                    let row = (i as u32 / 3) as f32;
                    let cr = Rect::new(r.x + 60.0 + col * (cw + gap), r.y + 170.0 + row * (chh + gap) + off, cw, chh);
                    let f = files::state_file(paths, gid, Some(n));
                    let exists = f.exists();
                    let key = format!("slot:{n}");
                    let iid = id(&key);
                    cx.add(iid, cr, Kind::Normal);
                    acts.insert(iid, Act::Slot(gid.clone(), n));
                    let amt = cx.focus_amt(iid);
                    cx.d.rect(cr.x, cr.y, cr.w, cr.h, 20.0, &Shape::fill(white(0.06 + 0.05 * amt)).border(1.0, white(0.1)));
                    let pic = Rect::new(cr.x + 20.0, cr.y + 20.0, cr.w - 40.0, cr.h - 110.0);
                    if exists {
                        let png = f.with_extension("png");
                        let mt = files::mtime(&png);
                        let pk = png.to_string_lossy().into_owned();
                        let stale = thumbs.get(&pk).map(|(m, _)| *m != mt).unwrap_or(true);
                        if stale {
                            if let Some(old) = thumbs.remove(&pk) {
                                cx.d.delete_texture(&old.1);
                            }
                            if let Ok(b) = std::fs::read(&png) {
                                if let Some(t) = texture_png(cx.d, &b) {
                                    thumbs.insert(pk.clone(), (mt, t));
                                }
                            }
                        }
                        if let Some((_, t)) = thumbs.get(&pk) {
                            let a = t.w as f32 / t.h as f32;
                            let (pw, ph) = if pic.w / pic.h > a { (pic.h * a, pic.h) } else { (pic.w, pic.w / a) };
                            cx.d.image(t, pic.cx() - pw * 0.5, pic.cy() - ph * 0.5, pw, ph, 10.0, [1.0; 4], false);
                        }
                        cx.d.text(&format!("Slot {n}"), cr.x + 24.0, cr.y + cr.h - 80.0, 28.0, BOLD, white(1.0), 0.0);
                        cx.d.text(&files::ago(files::mtime(&f)), cr.x + 24.0, cr.y + cr.h - 44.0, 22.0, REGULAR, white(0.55), 0.0);
                    } else {
                        cx.d.rect(pic.x, pic.y, pic.w, pic.h, 10.0, &Shape::fill(white(0.04)).border(1.0, white(0.08)));
                        cx.d.text_center("Empty", pic.cx(), pic.cy() - 14.0, 28.0, MEDIUM, white(0.35));
                        cx.d.text(&format!("Slot {n}"), cr.x + 24.0, cr.y + cr.h - 80.0, 28.0, BOLD, white(0.8), 0.0);
                    }
                    cx.focus_ring(cr, 20.0, amt);
                }
                cx.d.pop_clip();
            }
            Overlay::SlotMenu(gid, n) => {
                let exists = files::state_file(paths, gid, Some(*n)).exists();
                let r = Rect::new(610.0, 300.0, 700.0, 480.0);
                sheet(&mut cx, r, th);
                cx.d.text(&format!("Slot {n}"), r.x + 50.0, r.y + 46.0, 40.0, BOLD, white(1.0), 0.0);
                let mut y = r.y + 130.0;
                let mut rows: Vec<(&str, &str, Act, bool)> = Vec::new();
                if in_game {
                    rows.push(("sm:0", if exists { "Save over it" } else { "Save here" }, Act::SlotSave(*n), false));
                }
                if exists {
                    rows.push(("sm:1", "Load it", Act::SlotLoad(gid.clone(), *n), false));
                    rows.push(("sm:2", "Empty this slot", Act::SlotDelete(gid.clone(), *n), true));
                }
                if rows.is_empty() {
                    cx.d.text("Nothing saved here yet. Save from the game's menu.", r.x + 50.0, y, 26.0, REGULAR, white(0.6), 0.0);
                    y += 60.0;
                }
                for (k, label, act, danger) in rows {
                    cx.action_row(k, Rect::new(r.x + 40.0, y, r.w - 80.0, 76.0), label, "", danger);
                    acts.insert(id(k), act);
                    y += 86.0;
                }
                cx.action_row("sm:close", Rect::new(r.x + 40.0, y, r.w - 80.0, 76.0), "Never mind", "", false);
                acts.insert(id("sm:close"), Act::Close);
            }
            Overlay::Confirm { title, body, yes, danger, .. } => {
                let r = Rect::new(560.0, 320.0, 800.0, 420.0);
                sheet(&mut cx, r, th);
                let lines = cx.d.wrap(title, 40.0, BOLD, r.w - 100.0);
                let mut y = r.y + 50.0;
                for l in lines.iter().take(2) {
                    cx.d.text(l, r.x + 50.0, y, 40.0, BOLD, white(1.0), 0.0);
                    y += 50.0;
                }
                for l in cx.d.wrap(body, 26.0, REGULAR, r.w - 100.0).iter().take(3) {
                    cx.d.text(l, r.x + 50.0, y + 10.0, 26.0, REGULAR, white(0.6), 0.0);
                    y += 36.0;
                }
                let by = r.y + r.h - 120.0;
                let yes_r = cx.button("c:yes", r.x + 50.0, by, yes, *danger);
                acts.insert(id("c:yes"), Act::ConfirmYes);
                cx.button("c:no", yes_r.x + yes_r.w + 24.0, by, "Not now", false);
                acts.insert(id("c:no"), Act::Close);
            }
            Overlay::Settings => {
                let r = Rect::new(200.0, 60.0, 1520.0, 960.0);
                sheet(&mut cx, r, th);
                cx.d.text("SETTINGS", r.x + 60.0, r.y + 44.0, 22.0, BOLD, th.first, 2.4);
                cx.d.push_clip(r.x, r.y + 90.0, r.w, r.h - 100.0);
                let cards = settings_page::cards(s, core_defs, core_hidden, core_version, in_game);
                page_settings(&mut cx, acts, s, core_vals, core_defaults, &cards, open_cards, off, r.x + 40.0, r.w - 80.0, r.y + 100.0);
                cx.d.pop_clip();
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// page and sheet drawing (free functions, so they can borrow the app in pieces)

fn sheet(cx: &mut Ctx, r: Rect, th: &Theme) {
    cx.d.glow(r.x, r.y + 14.0, r.w, r.h, 34.0, 30.0, [0.0, 0.0, 0.0, 0.55]);
    cx.d.rect(r.x, r.y, r.w, r.h, 34.0, &Shape::fill([0.075, 0.075, 0.09, 0.96]).border(1.0, white(0.1)));
    cx.d.radial(r.cx(), r.y, r.w * 0.5, 360.0, with_alpha(th.first, 0.14));
}

fn tile(cx: &mut Ctx, key: &str, r: Rect, label: &str) {
    let iid = id(key);
    cx.add(iid, r, Kind::Normal);
    let amt = cx.focus_amt(iid);
    let rr = if cx.th.grow { r.grow(1.0 + 0.04 * amt) } else { r };
    cx.d.rect(rr.x, rr.y, rr.w, rr.h, 18.0, &Shape::fill(white(0.07 + 0.06 * amt)).border(1.0, white(0.09)));
    cx.focus_ring(rr, 18.0, amt);
    cx.d.text_center(label, rr.cx(), rr.cy() - 16.0, 30.0, MEDIUM, white(0.92 + 0.08 * amt));
}

fn danger_bar(cx: &mut Ctx, key: &str, r: Rect, label: &str, th: &Theme) {
    let iid = id(key);
    cx.add(iid, r, Kind::Normal);
    let amt = cx.focus_amt(iid);
    cx.d.rect(r.x, r.y, r.w, r.h, 18.0, &Shape::fill(with_alpha(th.danger, 0.08 + 0.2 * amt)).border(1.5, with_alpha(th.danger, 0.45 + 0.5 * amt)));
    if amt > 0.01 {
        cx.d.rect(r.x - 5.0, r.y - 5.0, r.w + 10.0, r.h + 10.0, 22.0, &Shape::fill([0.0; 4]).border(3.0, white(0.85 * amt)));
    }
    cx.d.text_center(label, r.cx(), r.y + (r.h - 30.0) * 0.5, 30.0, MEDIUM, th.danger);
}

fn sorted<'a>(games: impl Iterator<Item = &'a Game>, sort: &str) -> Vec<&'a Game> {
    let mut v: Vec<&Game> = games.collect();
    match sort {
        "added" => v.sort_by(|a, b| b.added.cmp(&a.added)),
        "played" => v.sort_by(|a, b| b.played.cmp(&a.played).then(a.title.cmp(&b.title))),
        _ => {}
    }
    v
}

fn card_width(s: &Settings) -> f32 {
    match s.card_size.as_str() {
        "s" => 210.0,
        "l" => 330.0,
        _ => 260.0,
    }
}

fn meta_line(g: &Game) -> String {
    if g.played > 0 {
        format!("Played {}", files::ago(g.played))
    } else {
        g.system.name().to_string()
    }
}

/// A grid of game cards from (x, y). Returns the y below it.
fn grid(cx: &mut Ctx, acts: &mut HashMap<u64, Act>, games: &[&Game], s: &Settings, prefix: &str, x: f32, y: f32, width: f32) -> f32 {
    let w = card_width(s);
    let gap = (w * 0.12).max(26.0);
    let cols = (((width + gap) / (w + gap)).floor() as usize).max(1);
    let text_h = if s.hide_titles { 30.0 } else { 110.0 };
    let row_h = w + text_h;
    for (i, g) in games.iter().enumerate() {
        let (c, r) = ((i % cols) as f32, (i / cols) as f32);
        let gx = x + c * (w + gap);
        let gy = y + r * (row_h + 10.0);
        if gy > VH + 50.0 || gy + row_h < 60.0 {
            // off screen: still reachable with the remote
            let key = format!("{prefix}:{}", g.id);
            cx.add(id(&key), Rect::new(gx, gy, w, w), Kind::Normal);
            acts.insert(id(&key), Act::Play(g.id.clone(), false));
            continue;
        }
        let key = format!("{prefix}:{}", g.id);
        cx.game_card(&key, g, gx, gy, w, &meta_line(g));
        acts.insert(id(&key), Act::Play(g.id.clone(), false));
    }
    let rows = games.len().div_ceil(cols);
    y + rows as f32 * (row_h + 10.0)
}

fn tray(cx: &mut Ctx, acts: &mut HashMap<u64, Act>, lib: &Library, picked: Option<Option<System>>, x: f32, y: f32) -> f32 {
    cx.section_title("SYSTEMS", x, y);
    let mut tx = x;
    let ty = y + 50.0;
    let all: Vec<Option<System>> = std::iter::once(None).chain(System::ALL.iter().map(|s| Some(*s))).collect();
    for sys in all {
        let count = match sys {
            None => lib.games.len(),
            Some(s) => lib.games.iter().filter(|g| g.system == s).count(),
        };
        let key = format!("tray:{}", sys.map(|s| s.short()).unwrap_or("all"));
        let is_picked = picked.map(|p| p == sys).unwrap_or(false);
        cx.console_tile(&key, sys, count, tx, ty, is_picked);
        acts.insert(id(&key), Act::Filter(sys));
        tx += 224.0;
    }
    ty + 240.0
}

/// A sideways row of games (Home's shelves), scrolled so the picked one shows.
fn shelf(cx: &mut Ctx, acts: &mut HashMap<u64, Act>, hscroll: &mut HashMap<String, f32>, title: &str, games: &[&Game], s: &Settings, y: f32) -> f32 {
    if games.is_empty() {
        return y;
    }
    cx.heading(title, 80.0, y);
    let w = card_width(s) * 0.92;
    let gap = 34.0;
    let key = title.to_string();
    let focused_idx = games.iter().position(|g| cx.focus == id(&format!("shelf:{title}:{}", g.id)));
    let target = match focused_idx {
        Some(i) => {
            let right = 80.0 + (i as f32 + 1.0) * (w + gap);
            (right - (VW - 120.0)).max(0.0)
        }
        None => *hscroll.get(&key).unwrap_or(&0.0),
    };
    let cur = hscroll.entry(key).or_insert(0.0);
    *cur += (target - *cur) * 0.25;
    let off = *cur;
    let cy = y + 62.0;
    for (i, g) in games.iter().enumerate() {
        let gx = 80.0 + i as f32 * (w + gap) - off;
        let k = format!("shelf:{title}:{}", g.id);
        if gx > VW + 40.0 || gx + w < -40.0 {
            cx.add(id(&k), Rect::new(gx, cy, w, w), Kind::Normal);
        } else {
            cx.game_card(&k, g, gx, cy, w, &meta_line(g));
        }
        acts.insert(id(&k), Act::Play(g.id.clone(), false));
    }
    cy + w + if s.hide_titles { 50.0 } else { 120.0 }
}

fn page_home(cx: &mut Ctx, acts: &mut HashMap<u64, Act>, lib: &Library, s: &Settings, off: f32, hscroll: &mut HashMap<String, f32>) {
    let th = cx.th;
    if lib.games.is_empty() {
        let y = 260.0 + off;
        cx.d.radial(VW * 0.5, y + 120.0, 700.0, 380.0, with_alpha(th.second, 0.18));
        cx.d.text_center("Add your games", VW * 0.5, y, 76.0, BOLD, white(1.0));
        cx.d.text_center("Send Game Boy, Color and Advance games from your phone.", VW * 0.5, y + 110.0, 32.0, REGULAR, white(0.6));
        cx.d.text_center("They show up here as soon as they land.", VW * 0.5, y + 156.0, 32.0, REGULAR, white(0.6));
        let w = 420.0;
        cx.primary("home:add", (VW - w) * 0.5, y + 250.0, "Add games", w);
        acts.insert(id("home:add"), Act::AddGames);
        return;
    }
    // the hero: the last game played, or the newest
    let last = lib.games.iter().filter(|g| g.played > 0).max_by_key(|g| g.played);
    let (hero, eyebrow) = match last {
        Some(g) => (g, "LAST PLAYED"),
        None => (lib.games.iter().max_by_key(|g| g.added).unwrap(), "RECENTLY ADDED"),
    };
    let t = th.t;
    let float = if th.floats { (4.0 * (t * std::f64::consts::TAU / 4.6).sin()) as f32 } else { 0.0 };
    let cover = Rect::new(150.0, 170.0 + off + float, 400.0, 400.0);
    if th.hero_glow {
        let breath = (0.5 + 0.5 * (t * std::f64::consts::TAU / 6.4).sin()) as f32;
        let sc = 0.97 + 0.07 * breath;
        cx.d.radial(cover.cx(), cover.cy(), 360.0 * sc, 360.0 * sc, with_alpha(th.second, 0.55 * (0.82 + 0.18 * breath)));
    }
    let radius = th.corner * 0.18 * cover.w;
    if let Some(gc) = th.cover_glow(hero.system) {
        cx.d.glow_ring(cover.x, cover.y, cover.w, cover.h, radius, 5.0, 7.0, with_alpha(gc, 0.75));
    }
    cx.cover(hero, cover, radius, 1.0);
    if let Some(gc) = th.cover_glow(hero.system) {
        cx.d.rect(cover.x, cover.y, cover.w, cover.h, radius, &Shape::fill([0.0; 4]).border(2.0, with_alpha(gc, 0.95)));
    }
    let tx = 650.0;
    let mut y = 200.0 + off;
    cx.d.text(eyebrow, tx, y, 26.0, MEDIUM, th.first, 1.2);
    y += 48.0;
    for l in cx.d.wrap(&hero.title, 64.0, BOLD, VW - tx - 120.0).iter().take(2) {
        cx.d.text(l, tx, y, 64.0, BOLD, white(1.0), 0.0);
        y += 74.0;
    }
    let meta = if hero.played > 0 { format!("{} · Played {}", hero.system.name(), files::ago(hero.played)) } else { hero.system.name().to_string() };
    cx.d.text(&meta, tx, y + 4.0, 28.0, REGULAR, white(0.55), 0.0);
    y += 80.0;
    let label = if hero.played > 0 { "Resume" } else { "Play" };
    let pr = cx.primary("home:play", tx, y, label, 260.0);
    acts.insert(id("home:play"), Act::Play(hero.id.clone(), false));
    let br = cx.button("home:menu", pr.x + pr.w + 28.0, y, "Game options", false);
    acts.insert(id("home:menu"), Act::GameMenu(hero.id.clone()));
    let _ = br;
    let mut y = 640.0 + off;
    y = tray(cx, acts, lib, None, 80.0, y) + 30.0;
    let recent: Vec<&Game> = {
        let mut v: Vec<&Game> = lib.games.iter().filter(|g| g.played > 0).collect();
        v.sort_by(|a, b| b.played.cmp(&a.played));
        v.into_iter().take(12).collect()
    };
    y = shelf(cx, acts, hscroll, "Jump Back In", &recent, s, y);
    let added: Vec<&Game> = {
        let mut v: Vec<&Game> = lib.games.iter().collect();
        v.sort_by(|a, b| b.added.cmp(&a.added));
        v.into_iter().take(12).collect()
    };
    y = shelf(cx, acts, hscroll, "Recently Added", &added, s, y);
    if s.show_home_shelves {
        for sys in System::ALL {
            let v: Vec<&Game> = lib.games.iter().filter(|g| g.system == sys).take(20).collect();
            y = shelf(cx, acts, hscroll, sys.name(), &v, s, y);
        }
    }
    let _ = y;
}

fn page_library(cx: &mut Ctx, acts: &mut HashMap<u64, Act>, lib: &Library, s: &Settings, filter: Option<System>, off: f32) {
    let y0 = 150.0 + off;
    cx.d.text("Library", 80.0, y0, 60.0, BOLD, white(1.0), 0.0);
    let n = lib.games.len();
    cx.d.text(&format!("{n} game{}", if n == 1 { "" } else { "s" }), 82.0, y0 + 74.0, 26.0, REGULAR, white(0.55), 0.0);
    // header buttons
    let sort = match s.library_sort.as_str() {
        "added" => "Recently added",
        "played" => "Last played",
        _ => "Name",
    };
    let layout = if s.library_layout == "console" { "By console" } else { "Grid" };
    let labels = [("lib:shuffle", "Shuffle".to_string(), Act::Shuffle), ("lib:sort", format!("Order: {sort}"), Act::Sort), ("lib:layout", layout.to_string(), Act::Layout)];
    let mut x = VW - 80.0;
    for (k, l, a) in labels.iter().rev() {
        let w = cx.d.measure(l, 24.0, MEDIUM, 0.0) + 48.0;
        x -= w;
        cx.bead(k, x, y0 + 14.0, l);
        acts.insert(id(k), a.clone());
        x -= 18.0;
    }
    if n == 0 {
        cx.d.text("No games yet.", 80.0, y0 + 180.0, 36.0, MEDIUM, white(0.8), 0.0);
        cx.primary("lib:add", 80.0, y0 + 250.0, "Add games", 360.0);
        acts.insert(id("lib:add"), Act::AddGames);
        return;
    }
    let mut y = tray(cx, acts, lib, Some(filter), 80.0, y0 + 140.0) + 20.0;
    let width = VW - 160.0;
    if s.library_layout == "console" && filter.is_none() {
        for sys in System::ALL {
            let games = sorted(lib.games.iter().filter(|g| g.system == sys), &s.library_sort);
            if games.is_empty() {
                continue;
            }
            // the console's heading bar
            cx.d.rect(80.0, y, width, 84.0, 20.0, &Shape::fill([0.09, 0.09, 0.10, 0.85]).border(1.0, white(0.08)));
            if let Some(t) = cx.images.console(sys) {
                cx.d.image(t, 96.0, y + 4.0, 76.0, 76.0, 0.0, [1.0; 4], false);
            }
            let nw = cx.d.text(sys.name(), 190.0, y + 24.0, 34.0, BOLD, white(1.0), 0.0);
            let c = games.len().to_string();
            let bw = cx.d.measure(&c, 20.0, BOLD, 0.0) + 22.0;
            cx.d.rect(210.0 + nw, y + 26.0, bw, 32.0, 16.0, &Shape::fill(draw::rgba(0x545454, 0.9)));
            cx.d.text_center(&c, 210.0 + nw + bw * 0.5, y + 32.0, 20.0, BOLD, white(1.0));
            y += 120.0;
            y = grid(cx, acts, &games, s, &format!("lib:{}", sys.short()), 80.0, y, width) + 30.0;
        }
    } else {
        let games = sorted(lib.games.iter().filter(|g| filter.map(|f| g.system == f).unwrap_or(true)), &s.library_sort);
        grid(cx, acts, &games, s, "lib", 80.0, y, width);
    }
}

fn page_favorites(cx: &mut Ctx, acts: &mut HashMap<u64, Act>, lib: &Library, s: &Settings, off: f32) {
    let y0 = 150.0 + off;
    cx.d.text("Favorites", 80.0, y0, 60.0, BOLD, white(1.0), 0.0);
    let games = sorted(lib.games.iter().filter(|g| g.favourite), &s.library_sort);
    if games.is_empty() {
        cx.d.text("Nothing here yet.", 80.0, y0 + 110.0, 34.0, MEDIUM, white(0.8), 0.0);
        cx.d.text("Press the green button on a game, or hold OK and pick Add to Favorites.", 80.0, y0 + 160.0, 28.0, REGULAR, white(0.55), 0.0);
        return;
    }
    cx.d.text(&format!("{} game{}", games.len(), if games.len() == 1 { "" } else { "s" }), 82.0, y0 + 74.0, 26.0, REGULAR, white(0.55), 0.0);
    grid(cx, acts, &games, s, "fav", 80.0, y0 + 140.0, VW - 160.0);
}

#[allow(clippy::too_many_arguments)]
fn page_settings(
    cx: &mut Ctx,
    acts: &mut HashMap<u64, Act>,
    s: &Settings,
    core_vals: &HashMap<String, String>,
    core_defaults: &HashMap<String, String>,
    cards: &[settings_page::Card],
    open: &HashSet<&'static str>,
    off: f32,
    x: f32,
    width: f32,
    top: f32,
) {
    let mut y = top + off;
    if top > 140.0 && top < 160.0 {
        cx.d.text("Settings", x, y, 60.0, BOLD, white(1.0), 0.0);
        y += 110.0;
    }
    for card in cards {
        let is_open = open.contains(card.key);
        let key = format!("fold:{}", card.key);
        cx.fold_head(&key, Rect::new(x, y, width, 104.0), card.title, &card.subtitle, is_open);
        acts.insert(id(&key), Act::Fold(card.key));
        y += 104.0;
        if is_open {
            y += 10.0;
            for row in &card.rows {
                let r = Rect::new(x + 20.0, y, width - 40.0, 80.0);
                match row {
                    Row::Heading(t) => {
                        cx.d.text(&t.to_uppercase(), x + 44.0, y + 30.0, 22.0, BOLD, cx.th.first, 1.6);
                        y += 72.0;
                        continue;
                    }
                    Row::Note(t) => {
                        let lines = cx.d.wrap(t, 24.0, REGULAR, width - 100.0);
                        for l in &lines {
                            cx.d.text(l, x + 44.0, y + 8.0, 24.0, REGULAR, white(0.5), 0.0);
                            y += 34.0;
                        }
                        y += 16.0;
                        continue;
                    }
                    Row::Toggle { key, label } => {
                        let on = settings_page::get(s, core_vals, core_defaults, key) == "true";
                        let k = format!("set:{key}");
                        cx.toggle_row(&k, r, label, on);
                        acts.insert(id(&k), Act::Row(row.clone()));
                    }
                    Row::Choice { key, label, values, swatch } => {
                        let cur = settings_page::get(s, core_vals, core_defaults, key);
                        let shown = values.iter().find(|(v, _)| *v == cur).map(|(_, l)| l.clone()).unwrap_or(cur.clone());
                        let sw = if *swatch { Some(ui::pair_of(&cur)) } else { None };
                        let k = format!("set:{key}");
                        cx.choice_row(&k, r, label, &shown, sw);
                        acts.insert(id(&k), Act::Row(row.clone()));
                    }
                    Row::Slider { key, label, min, max, .. } => {
                        let v: f64 = settings_page::get(s, core_vals, core_defaults, key).parse().unwrap_or(*min);
                        let frac = ((v - min) / (max - min)) as f32;
                        let k = format!("set:{key}");
                        cx.slider_row(&k, r, label, frac, &settings_page::slider_text(key, v));
                        acts.insert(id(&k), Act::Row(row.clone()));
                    }
                    Row::Action { key, label, hint, danger } => {
                        let k = format!("set:{key}");
                        cx.action_row(&k, r, label, hint, *danger);
                        acts.insert(id(&k), Act::Row(row.clone()));
                    }
                }
                y += 88.0;
            }
            y += 14.0;
        }
        y += 18.0;
    }
}

fn page_add(cx: &mut Ctx, acts: &mut HashMap<u64, Act>, upload: Option<&Upload>, qr_tex: &mut Option<(String, Texture)>, off: f32, paths: &Paths) {
    let th = cx.th;
    let y0 = 150.0 + off;
    cx.d.text("Add games", 80.0, y0, 60.0, BOLD, white(1.0), 0.0);
    let u = match upload {
        Some(u) => u,
        None => {
            cx.d.text("The transfer page isn't open. Press Back and try again.", 80.0, y0 + 120.0, 30.0, REGULAR, white(0.6), 0.0);
            return;
        }
    };
    // the QR code
    if qr_tex.as_ref().map(|(k, _)| *k != u.url).unwrap_or(true) {
        if let Some((n, m)) = upload::qr(&u.url) {
            let border = 2;
            let size = n + border * 2;
            let mut px = vec![255u8; size * size * 4];
            for yy in 0..n {
                for xx in 0..n {
                    if m[yy * n + xx] {
                        let o = ((yy + border) * size + xx + border) * 4;
                        px[o] = 0;
                        px[o + 1] = 0;
                        px[o + 2] = 0;
                    }
                }
            }
            let t = cx.d.texture_rgba(size as u32, size as u32, &px, crate::gl::NEAREST);
            *qr_tex = Some((u.url.clone(), t));
        }
    }
    let card = Rect::new(80.0, y0 + 110.0, 560.0, 560.0);
    cx.d.glow(card.x, card.y + 10.0, card.w, card.h, 30.0, 26.0, with_alpha(th.second, 0.35));
    cx.d.rect(card.x, card.y, card.w, card.h, 30.0, &Shape::fill(white(1.0)));
    if let Some((_, t)) = qr_tex.as_ref() {
        cx.d.image(t, card.x + 30.0, card.y + 30.0, card.w - 60.0, card.h - 60.0, 0.0, [1.0; 4], false);
    }
    let tx = 720.0;
    let mut y = y0 + 130.0;
    let steps = [
        "Point your phone's camera at the code.",
        "Tap Pick games and choose your Game Boy games (a .zip works too).",
        "They show up in your library as they land.",
    ];
    for (i, st) in steps.iter().enumerate() {
        cx.d.rect(tx, y - 4.0, 52.0, 52.0, 26.0, &th.accent_fill());
        cx.d.text_center(&(i + 1).to_string(), tx + 26.0, y + 6.0, 28.0, BOLD, th.on_accent);
        for (j, l) in cx.d.wrap(st, 32.0, REGULAR, VW - tx - 200.0).iter().enumerate() {
            cx.d.text(l, tx + 80.0, y + 4.0 + j as f32 * 42.0, 32.0, REGULAR, white(0.92), 0.0);
        }
        y += 100.0;
    }
    cx.d.text("Or type this on the phone:", tx, y + 10.0, 26.0, REGULAR, white(0.55), 0.0);
    cx.d.text(&u.url, tx, y + 50.0, 38.0, MEDIUM, th.first, 0.0);
    cx.d.text("Your phone needs to be on the same Wi-Fi as the TV.", tx, y + 110.0, 24.0, REGULAR, white(0.45), 0.0);
    y += 180.0;
    let files_now: Vec<upload::Received> = u.shared.lock().unwrap().files.clone();
    if !files_now.is_empty() {
        cx.section_title("ARRIVING", tx, y);
        y += 46.0;
        for f in files_now.iter().rev().take(5) {
            let (status, c) = if let Some(e) = &f.error {
                (format!("didn't arrive: {e}"), th.danger)
            } else if f.finished {
                ("in your library".to_string(), [0.61, 0.88, 0.36, 1.0])
            } else {
                (format!("{}%", f.done * 100 / f.total.max(1)), white(0.6))
            };
            cx.d.text_fit(&f.name, tx, y, 26.0, MEDIUM, white(0.9), 700.0);
            cx.d.text(&status, tx + 740.0, y, 26.0, REGULAR, c, 0.0);
            let frac = f.done as f32 / f.total.max(1) as f32;
            cx.d.rect(tx, y + 38.0, 900.0, 6.0, 3.0, &Shape::fill(white(0.12)));
            cx.d.rect(tx, y + 38.0, (900.0 * frac).max(6.0), 6.0, 3.0, &th.accent_fill());
            y += 66.0;
        }
    }
    let done = cx.primary("add:done", 80.0, card.y + card.h + 40.0, "Done", 560.0);
    acts.insert(id("add:done"), Act::Tab(0));
    let _ = (done, paths);
}
