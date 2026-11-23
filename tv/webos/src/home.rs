//! The TV's Home screen as a native program (owner, 22 November 2026: "a lightweight native
//! launcher"). The Kiro launcher's look and everything it does, without a web engine:
//! - the clock and the date;
//! - the top bar: Inputs, Backgrounds, Upload, Settings, App Store, Homebrew, LG Home;
//! - the Play row: the HDMI inputs, named by the device on them (green for an Xbox; HDMI 2 is
//!   the soundbar's ARC and stays hidden), and AirPlay;
//! - the apps the owner wants, in his order: Netflix, YouTube AdFree, Apple Music, Continuum,
//!   Tubi, Pluto TV;
//! - the background pictures: none, one, or shuffled every few minutes, the screen saver switch,
//!   the red button twice deletes a picture, and the phone upload page.
//!
//! The pictures and their settings stay with kiro-bg.sh (and its `bg.conf`, `app/bg/`, the screen
//! saver's Slideshow.qml and upload.js), so the screen saver and the upload page work as before.
//!
//! SAM starts it as com.webos.app.home through `home-standin-native.sh`, the same RAM-only stand-in
//! way as the web launcher: `continuum-tv --home '<SAM's launch JSON>'`.

use std::collections::HashMap;
use std::ffi::{c_void, CString};
use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use ab_glyph::FontArc;

use crate::draw::{rgb, rgba, white, Draw, Rgba, Shape, Texture, LIGHT, MEDIUM, REGULAR};
use crate::sdl::{self, Event, Sdl};

const K: &str = "/media/developer/kiro";
const H: &str = "/media/developer/kiro/home";
/// The apps on the Home screen, in this order (owner, 22 November 2026: "Netflix, YouTube
/// adfree, Apple Music, continuum, tubi, and Pluto are all I want on the launcher").
const APPS: [&str; 6] = ["netflix", "youtube.leanback.v4", "com.apple.applemusic", "dev.continuum.tv", "com.tubitv.ott.tubi", "com.plutotv.app"];
const INTERVALS: [u32; 5] = [1, 5, 15, 30, 60];
const COLS: usize = 6;
const AIRPLAY_ICON: &str = "/usr/palm/applications/airplay/AirPlay_Icon-115x115.png";

// ------------------------------------------------------------------------------------------
// the TV's services, through the same small shell helpers the web launcher used

/// One Luna call and its reply (lib.sh's `lsend`, which keeps luna-send's input open so the
/// reply isn't dropped).
fn luna(uri: &str, payload: &str) -> Option<serde_json::Value> {
    let out = Command::new("/bin/sh")
        .arg("-c")
        .arg(format!(". {K}/lib.sh; lsend -n 1 -w 10000 \"$1\" \"$2\""))
        .arg("sh")
        .arg(uri)
        .arg(payload)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(text.trim()).ok().or_else(|| text.lines().rev().find_map(|l| serde_json::from_str(l.trim()).ok()))
}

/// kiro-bg.sh with its arguments; its answer (JSON, or the upload page's address).
fn bg_sh(args: &[&str]) -> String {
    Command::new("/bin/sh")
        .arg(format!("{H}/kiro-bg.sh"))
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// The background settings and pictures (kiro-bg.sh json).
#[derive(Clone, Default, PartialEq)]
struct Bgs {
    mode: String,
    current: String,
    interval: u32,
    saver: String,
    files: Vec<String>,
}

fn parse_bgs(text: &str) -> Option<Bgs> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    if v.get("returnValue").and_then(|r| r.as_bool()) != Some(true) {
        return None;
    }
    let s = |k: &str, d: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or(d).to_string();
    Some(Bgs {
        mode: s("mode", "none"),
        current: s("current", ""),
        interval: v.get("interval").and_then(|x| x.as_u64()).unwrap_or(15) as u32,
        saver: s("saver", "off"),
        files: v.get("files").and_then(|f| f.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
    })
}

fn bg_error(text: &str) -> String {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get("errorText").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or_else(|| "no answer".into())
}

// ------------------------------------------------------------------------------------------
// pictures, read away from the drawing thread

fn read_picture(path: &str) -> Option<(u32, u32, Vec<u8>)> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return crate::files::read_png(&bytes);
    }
    let mut dec = jpeg_decoder::Decoder::new(&bytes[..]);
    let px = dec.decode().ok()?;
    let info = dec.info()?;
    let (w, h) = (u32::from(info.width), u32::from(info.height));
    let rgba: Vec<u8> = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => px.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        jpeg_decoder::PixelFormat::L8 => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        _ => return None,
    };
    Some((w, h, rgba))
}

// ------------------------------------------------------------------------------------------
// what the worker threads send back

enum Msg {
    Apps(Vec<AppTile>),
    Inputs(Vec<InputTile>),
    Bg(Bgs, bool),
    Picture(String, Option<(u32, u32, Vec<u8>)>),
    Icon(String, Option<(u32, u32, Vec<u8>)>),
    Upload(Option<String>),
    Say(String),
    Relaunch,
    Close,
}

#[derive(Clone)]
struct AppTile {
    id: String,
    title: String,
    color: Rgba,
    icon: String,
}

#[derive(Clone)]
struct InputTile {
    id: String,
    /// The small name under the tile.
    title: String,
    /// The big text on the tile.
    big: String,
    color: Rgba,
}

fn hex_color(s: &str) -> Option<Rgba> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    u32::from_str_radix(s, 16).ok().map(rgb)
}

fn load_apps(tx: &Sender<Msg>) {
    let Some(v) = luna("luna://com.webos.applicationManager/listLaunchPoints", "{}") else { return };
    let Some(lps) = v.get("launchPoints").and_then(|l| l.as_array()) else { return };
    let mut tiles = Vec::new();
    for id in APPS {
        let Some(lp) = lps.iter().find(|lp| lp.get("id").and_then(|x| x.as_str()) == Some(id)) else { continue };
        let s = |k: &str| lp.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let icon = [s("largeIcon"), s("mediumLargeIcon"), s("icon")].into_iter().find(|p| !p.is_empty() && std::path::Path::new(p).exists()).unwrap_or_default();
        tiles.push(AppTile {
            id: id.to_string(),
            title: s("title"),
            color: hex_color(&s("iconColor")).or_else(|| hex_color(&s("bgColor"))).unwrap_or(rgb(0x1c2333)),
            icon,
        });
    }
    let _ = tx.send(Msg::Apps(tiles));
}

fn load_inputs(tx: &Sender<Msg>) {
    let Some(v) = luna("luna://com.webos.service.eim/getAllInputStatus", "{}") else { return };
    let Some(devs) = v.get("devices").and_then(|d| d.as_array()) else { return };
    let mut tiles = Vec::new();
    for d in devs {
        let id = d.get("appId").and_then(|x| x.as_str()).unwrap_or("");
        // HDMI 2 is the soundbar's ARC connection: opening it shows nothing
        if !id.starts_with("com.webos.app.hdmi") || id == "com.webos.app.hdmi2" {
            continue;
        }
        let port = format!("HDMI {}", d.get("port").map(|p| p.to_string().trim_matches('"').to_string()).unwrap_or_default());
        let osd = d.get("subList").and_then(|s| s.get(0)).and_then(|s| s.get("osdName")).and_then(|n| n.as_str()).unwrap_or("");
        let label = d.get("label").and_then(|x| x.as_str()).unwrap_or("");
        let name = if !osd.is_empty() { osd.to_string() } else if !label.is_empty() { label.to_string() } else { port.clone() };
        let xbox = name.to_lowercase().contains("xbox");
        tiles.push(InputTile {
            id: id.to_string(),
            title: if name == port { "Input".into() } else { port },
            big: if xbox { "Xbox".into() } else { name },
            color: if xbox { rgb(0x107c10) } else { rgb(0x2a3245) },
        });
    }
    let _ = tx.send(Msg::Inputs(tiles));
}

fn spawn(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(f);
}

/// Continuum (a game) runs ahead of LG's background services (owner's tips, 22 November 2026:
/// "prioritize foreground workloads"). The app itself isn't allowed to raise its own priority, so
/// Home, which runs as root, does it: every few seconds, every thread of a running continuum-tv
/// gets nice -10 (the TV's processor is already at its top speed with the performance governor,
/// measured; this only keeps the background from cutting in).
fn boost_continuum() {
    loop {
        if let Ok(dir) = std::fs::read_dir("/proc") {
            for e in dir.flatten() {
                let name = e.file_name();
                let Some(pid) = name.to_str().filter(|s| s.bytes().all(|b| b.is_ascii_digit())) else { continue };
                let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
                if comm.trim() != "continuum-tv" {
                    continue;
                }
                if let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) {
                    for t in tasks.flatten() {
                        let Some(tid) = t.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
                        unsafe {
                            *libc::__errno_location() = 0;
                            let now = libc::getpriority(libc::PRIO_PROCESS, tid);
                            if now != -10 {
                                libc::setpriority(libc::PRIO_PROCESS, tid, -10);
                            }
                        }
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}

// ------------------------------------------------------------------------------------------
// SAM: registered as com.webos.app.home, so the Home key sends "relaunch"

/// `luna-send -i -a com.webos.app.home .../registerApp`, kept running (its input held open so it
/// doesn't drop out), its events passed on.
fn register_with_sam(tx: Sender<Msg>) -> Option<(Child, ChildStdin)> {
    let mut child = Command::new("luna-send")
        .args(["-i", "-a", "com.webos.app.home", "luna://com.webos.applicationManager/registerApp", "{}"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdin = child.stdin.take()?;
    let out = child.stdout.take()?;
    std::thread::spawn(move || {
        let mut registered = false;
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            crate::logln!("SAM: {}", line.chars().take(200).collect::<String>());
            if line.contains("\"event\"") && line.contains("relaunch") {
                let _ = tx.send(Msg::Relaunch);
            } else if line.contains("\"event\"") && line.contains("close") {
                let _ = tx.send(Msg::Close);
            } else if line.contains("registered") {
                registered = true;
            }
        }
        crate::logln!("SAM registration ended");
        // SAM restarted (some TVs do that on standby) and no longer knows this as Home: close,
        // so the next Home launch starts afresh (what home-standin.sh does for the web one).
        if registered {
            let _ = tx.send(Msg::Close);
        }
    });
    Some((child, stdin))
}

// ------------------------------------------------------------------------------------------
// fonts, mapped from the TV's own files (only the pages for the letters drawn are read in)

fn map_font(path: &str) -> Option<FontArc> {
    let c = CString::new(path).ok()?;
    unsafe {
        let fd = libc::open(c.as_ptr(), libc::O_RDONLY);
        if fd < 0 {
            return None;
        }
        let mut st: libc::stat = std::mem::zeroed();
        if libc::fstat(fd, &mut st) != 0 || st.st_size <= 0 {
            libc::close(fd);
            return None;
        }
        let len = st.st_size as usize;
        let p = libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ, libc::MAP_PRIVATE, fd, 0);
        libc::close(fd);
        if p == libc::MAP_FAILED {
            return None;
        }
        let bytes: &'static [u8] = std::slice::from_raw_parts(p as *const u8, len);
        ab_glyph::FontRef::try_from_slice(bytes).ok().map(FontArc::new)
    }
}

fn fonts() -> Result<Vec<FontArc>, String> {
    let dirs = ["/usr/share/fonts", "/usr/share/fonts/truetype/dejavu"];
    let names = [
        ["LG_Smart_UI-Light.ttf", "DejaVuSans-ExtraLight.ttf"],
        ["LG_Smart_UI-Regular.ttf", "DejaVuSans.ttf"],
        ["LG_Smart_UI-SemiBold.ttf", "DejaVuSans-Bold.ttf"],
    ];
    let mut out: Vec<FontArc> = Vec::new();
    for choice in names {
        let f = dirs.iter().flat_map(|d| choice.iter().map(move |n| format!("{d}/{n}"))).find_map(|p| map_font(&p));
        match f.or_else(|| out.last().cloned()) {
            Some(f) => out.push(f),
            None => return Err("no fonts found".into()),
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------
// the clock (the TV's own time zone, as the web launcher's Date did)

fn now_local() -> libc::tm {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    }
}

const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

fn clock_text() -> (String, String) {
    let tm = now_local();
    let h = tm.tm_hour;
    let time = format!("{}:{:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, tm.tm_min, if h < 12 { "AM" } else { "PM" });
    let date = format!("{}, {} {}", DAYS[tm.tm_wday.clamp(0, 6) as usize], MONTHS[tm.tm_mon.clamp(0, 11) as usize], tm.tm_mday);
    (time, date)
}

// ------------------------------------------------------------------------------------------
// the screen

#[derive(Clone, PartialEq)]
enum Act {
    Launch(String),
    LgHome,
    AirPlay,
    Panel,
    PanelDone,
    Upload,
    BgNone,
    BgPick(String),
    Shuffle,
    Interval,
    Saver,
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Home,
    Panel,
    Dialog,
}

/// One thing the remote can land on, where it was last drawn.
#[derive(Clone)]
#[allow(dead_code)]
struct Spot {
    act: Act,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

struct Fade {
    tex: Texture,
    name: String,
    born: Instant,
}

struct Home {
    sdl: Sdl,
    win: *mut c_void,
    d: Draw,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    view: View,
    dialog_from: View,
    apps: Vec<AppTile>,
    inputs: Vec<InputTile>,
    icons: HashMap<String, Option<Texture>>,
    thumbs: HashMap<String, Option<Texture>>,
    bgs: Bgs,
    /// The picture on screen and the one fading out under it.
    pic: Option<Fade>,
    old_pic: Option<Fade>,
    /// The picture asked for and not yet read.
    loading: Option<String>,
    shuffle_bag: Vec<String>,
    next_shuffle: Option<Instant>,
    /// What the remote is on: (row, column), kept by what it is when the screen changes.
    focus: (usize, usize),
    focus_act: Option<Act>,
    home_focus: Option<Act>,
    rows: Vec<Vec<Spot>>,
    panel_scroll: f32,
    red_armed: Option<String>,
    say: Option<(String, Instant)>,
    upload_url: String,
    upload_before: usize,
    upload_poll: Instant,
    front: bool,
    quit: bool,
    busy_until: Instant,
    /// Something changed that the screen hasn't shown yet.
    dirty: bool,
    /// The remote has moved the focus: until then it sits on the first app (as the web one did).
    user_moved: bool,
    _sam: Option<(Child, ChildStdin)>,
}

fn rand_u32() -> u32 {
    let mut b = [0u8; 4];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut b);
    }
    u32::from_le_bytes(b)
}

impl Home {
    fn refresh(&mut self) {
        let tx = self.tx.clone();
        spawn(move || load_apps(&tx));
        let tx = self.tx.clone();
        spawn(move || load_inputs(&tx));
        let tx = self.tx.clone();
        spawn(move || {
            if let Some(b) = parse_bgs(&bg_sh(&["json"])) {
                let _ = tx.send(Msg::Bg(b, false));
            }
        });
    }

    fn say(&mut self, s: impl Into<String>) {
        self.say = Some((s.into(), Instant::now()));
        self.dirty = true;
    }

    fn bg_set(&self, key: &'static str, value: String, change_now: bool) {
        let tx = self.tx.clone();
        spawn(move || {
            let r = bg_sh(&["set", key, &value]);
            match parse_bgs(&r) {
                Some(b) => {
                    let _ = tx.send(Msg::Bg(b, change_now));
                }
                None => {
                    let _ = tx.send(Msg::Say(format!("Could not save that: {}", bg_error(&r))));
                }
            }
        });
    }

    // ---- pictures ----

    fn want_picture(&mut self, name: &str) {
        if name.is_empty() {
            if let Some(p) = self.pic.take() {
                self.fade_out(p);
            }
            return;
        }
        if self.pic.as_ref().map(|p| p.name == name).unwrap_or(false) || self.loading.as_deref() == Some(name) {
            return;
        }
        self.loading = Some(name.to_string());
        let (tx, n) = (self.tx.clone(), name.to_string());
        spawn(move || {
            let px = read_picture(&format!("{H}/app/bg/{n}"));
            let _ = tx.send(Msg::Picture(n, px));
        });
    }

    fn fade_out(&mut self, mut p: Fade) {
        if let Some(o) = self.old_pic.take() {
            self.d.delete_texture(&o.tex);
        }
        p.born = Instant::now();
        self.old_pic = Some(p);
    }

    fn shuffle_next(&mut self) {
        if self.shuffle_bag.is_empty() {
            let mut bag = self.bgs.files.clone();
            for i in (1..bag.len()).rev() {
                let j = rand_u32() as usize % (i + 1);
                bag.swap(i, j);
            }
            self.shuffle_bag = bag;
        }
        let mut n = self.shuffle_bag.pop().unwrap_or_default();
        let shown = self.pic.as_ref().map(|p| p.name.clone()).unwrap_or_default();
        if n == shown && !self.shuffle_bag.is_empty() {
            n = self.shuffle_bag.pop().unwrap_or_default();
        }
        self.want_picture(&n);
    }

    /// The background as the settings say. `change_now`: a new picture straight away (a choice
    /// in the panel); otherwise the one on screen stays.
    fn apply_bg(&mut self, change_now: bool) {
        self.next_shuffle = None;
        let files = self.bgs.files.clone();
        if self.bgs.mode == "none" || files.is_empty() {
            self.want_picture("");
            return;
        }
        let shown = self.pic.as_ref().map(|p| p.name.clone()).unwrap_or_default();
        if self.bgs.mode == "shuffle" && files.len() > 1 {
            if change_now || !files.contains(&shown) {
                self.shuffle_next();
            }
            self.next_shuffle = Some(Instant::now() + Duration::from_secs(u64::from(self.bgs.interval.max(1)) * 60));
            return;
        }
        let one = if files.contains(&self.bgs.current) { self.bgs.current.clone() } else { files[0].clone() };
        self.want_picture(&one);
    }

    fn set_bgs(&mut self, b: Bgs, change_now: bool) {
        // thumbnails of pictures that are gone are let go
        let gone: Vec<String> = self.thumbs.keys().filter(|k| !b.files.contains(k)).cloned().collect();
        for k in gone {
            if let Some(Some(t)) = self.thumbs.remove(&k) {
                self.d.delete_texture(&t);
            }
        }
        self.bgs = b;
        self.shuffle_bag.clear();
        self.apply_bg(change_now);
    }

    fn icon(&mut self, path: &str) -> Option<&Texture> {
        if path.is_empty() {
            return None;
        }
        if !self.icons.contains_key(path) {
            self.icons.insert(path.to_string(), None);
            let (tx, p) = (self.tx.clone(), path.to_string());
            spawn(move || {
                let px = read_picture(&p);
                let _ = tx.send(Msg::Icon(p, px));
            });
        }
        self.icons.get(path).and_then(|t| t.as_ref())
    }

    fn thumb(&mut self, name: &str) -> Option<&Texture> {
        if !self.thumbs.contains_key(name) {
            self.thumbs.insert(name.to_string(), None);
            let (tx, n) = (self.tx.clone(), name.to_string());
            spawn(move || {
                let px = read_picture(&format!("{H}/app/bg/thumbs/{n}")).or_else(|| read_picture(&format!("{H}/app/bg/{n}")));
                let _ = tx.send(Msg::Icon(format!("thumb:{n}"), px));
            });
        }
        self.thumbs.get(name).and_then(|t| t.as_ref())
    }

    fn messages(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            self.dirty = true;
            match m {
                Msg::Apps(a) => self.apps = a,
                Msg::Inputs(i) => self.inputs = i,
                Msg::Bg(b, now) => {
                    if b != self.bgs || now {
                        self.set_bgs(b, now);
                    }
                }
                Msg::Picture(name, px) => {
                    if self.loading.as_deref() != Some(name.as_str()) {
                        continue;
                    }
                    self.loading = None;
                    if let Some((w, h, px)) = px {
                        let tex = self.d.texture_rgba(w, h, &px, crate::gl::LINEAR);
                        if let Some(p) = self.pic.take() {
                            self.fade_out(p);
                        }
                        self.pic = Some(Fade { tex, name, born: Instant::now() });
                    }
                }
                Msg::Icon(key, px) => {
                    let tex = px.map(|(w, h, px)| self.d.texture_rgba(w, h, &px, crate::gl::LINEAR));
                    if let Some(n) = key.strip_prefix("thumb:") {
                        if self.thumbs.contains_key(n) {
                            self.thumbs.insert(n.to_string(), tex);
                        } else if let Some(t) = tex {
                            self.d.delete_texture(&t);
                        }
                    } else {
                        self.icons.insert(key, tex);
                    }
                }
                Msg::Upload(url) => match url {
                    Some(u) => {
                        self.upload_url = u;
                        self.upload_before = self.bgs.files.len();
                        self.dialog_from = self.view;
                        self.view = View::Dialog;
                        self.upload_poll = Instant::now();
                    }
                    None => self.say("Could not start the upload page"),
                },
                Msg::Say(s) => self.say(s),
                Msg::Relaunch => {
                    crate::logln!("Home key: to the front");
                    self.front = true;
                    unsafe {
                        (self.sdl.ShowWindow)(self.win);
                        (self.sdl.RestoreWindow)(self.win);
                        (self.sdl.RaiseWindow)(self.win);
                    }
                    if self.view == View::Panel {
                        self.view = View::Home;
                    }
                    self.refresh();
                }
                Msg::Close => {
                    crate::logln!("closed by SAM");
                    self.quit = true;
                }
            }
        }
    }

    // ---- the remote ----

    fn activate(&mut self, act: Act) {
        let now = Instant::now();
        if now < self.busy_until {
            return;
        }
        self.busy_until = now + Duration::from_millis(500);
        match act {
            Act::Launch(id) => {
                let tx = self.tx.clone();
                spawn(move || {
                    let r = luna("luna://com.webos.applicationManager/launch", &format!("{{\"id\":\"{id}\"}}"));
                    if r.as_ref().and_then(|v| v.get("returnValue")).and_then(|x| x.as_bool()) != Some(true) {
                        let why = r.and_then(|v| v.get("errorText").and_then(|e| e.as_str()).map(String::from)).unwrap_or_else(|| "no answer".into());
                        let _ = tx.send(Msg::Say(format!("Could not open {id}: {why}")));
                    }
                });
            }
            Act::LgHome => {
                // LG's Home until the next reboot; this launcher comes back at boot
                let _ = Command::new("setsid")
                    .args(["sh", &format!("{H}/kiro-home-native.sh"), "off"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
            }
            Act::AirPlay => {
                let tx = self.tx.clone();
                spawn(move || {
                    let r = luna("luna://airplay.service/showAirplayPage", "{}");
                    if r.as_ref().and_then(|v| v.get("returnValue")).and_then(|x| x.as_bool()) != Some(true) {
                        let _ = tx.send(Msg::Say("Could not open AirPlay".into()));
                    }
                });
            }
            Act::Panel => {
                self.home_focus = self.focus_act.clone();
                self.view = View::Panel;
                self.panel_scroll = 0.0;
                self.focus_act = Some(if self.bgs.mode == "one" && !self.bgs.current.is_empty() { Act::BgPick(self.bgs.current.clone()) } else { Act::BgNone });
            }
            Act::PanelDone => {
                self.view = View::Home;
                self.focus_act = self.home_focus.clone();
            }
            Act::Upload => {
                let tx = self.tx.clone();
                spawn(move || {
                    let u = bg_sh(&["upload"]);
                    let _ = tx.send(Msg::Upload((!u.is_empty() && !u.starts_with('{')).then_some(u)));
                });
            }
            Act::BgNone => self.bg_set("mode", "none".into(), true),
            Act::BgPick(n) => self.bg_set("current", n, true),
            Act::Shuffle => {
                if self.bgs.mode != "shuffle" && self.bgs.files.len() < 2 {
                    self.say("Add at least 2 pictures to shuffle");
                    return;
                }
                let v = if self.bgs.mode == "shuffle" { "one" } else { "shuffle" };
                self.bg_set("mode", v.into(), true);
            }
            Act::Interval => {
                let i = INTERVALS.iter().position(|&x| x == self.bgs.interval).map(|i| (i + 1) % INTERVALS.len()).unwrap_or(2);
                self.bg_set("interval", INTERVALS[i].to_string(), false);
            }
            Act::Saver => {
                if self.bgs.saver != "on" && self.bgs.files.is_empty() {
                    self.say("Add a picture first");
                    return;
                }
                let v = if self.bgs.saver == "on" { "off" } else { "on" };
                self.bg_set("saver", v.into(), false);
            }
        }
    }

    fn close_dialog(&mut self) {
        self.view = self.dialog_from;
        let tx = self.tx.clone();
        spawn(move || {
            if let Some(b) = parse_bgs(&bg_sh(&["json"])) {
                let _ = tx.send(Msg::Bg(b, false));
            }
        });
    }

    fn red(&mut self) {
        if self.view != View::Panel {
            return;
        }
        let Some(Act::BgPick(n)) = self.focus_act.clone() else { return };
        if self.red_armed.as_deref() != Some(n.as_str()) {
            self.red_armed = Some(n);
            self.say("Press the red button again to delete this picture");
            return;
        }
        self.red_armed = None;
        let tx = self.tx.clone();
        spawn(move || {
            let r = bg_sh(&["delete", &n]);
            match parse_bgs(&r) {
                Some(b) => {
                    let _ = tx.send(Msg::Say("Picture deleted".into()));
                    let _ = tx.send(Msg::Bg(b, true));
                }
                None => {
                    let _ = tx.send(Msg::Say(format!("Could not delete it: {}", bg_error(&r))));
                }
            }
        });
    }

    fn mv(&mut self, dr: i32, dc: i32) {
        self.red_armed = None;
        self.user_moved = true;
        if self.rows.is_empty() {
            return;
        }
        let (r, c) = self.focus;
        let r = r.min(self.rows.len() - 1);
        let c = c.min(self.rows[r].len().saturating_sub(1));
        if dc != 0 {
            let row = &self.rows[r];
            let nc = (c as i32 + dc).clamp(0, row.len() as i32 - 1) as usize;
            self.focus = (r, nc);
        } else {
            let nr = r as i32 + dr;
            if nr < 0 || nr >= self.rows.len() as i32 {
                return;
            }
            let cur = &self.rows[r][c];
            let x = cur.x + cur.w * 0.5;
            let row = &self.rows[nr as usize];
            let mut best = 0;
            let mut bd = f32::MAX;
            for (i, s) in row.iter().enumerate() {
                let dd = (s.x + s.w * 0.5 - x).abs();
                if dd < bd {
                    bd = dd;
                    best = i;
                }
            }
            self.focus = (nr as usize, best);
        }
        self.focus_act = Some(self.rows[self.focus.0][self.focus.1].act.clone());
    }

    fn key(&mut self, sc: i32) {
        self.dirty = true;
        let Some(k) = crate::input::remote_key(sc) else { return };
        if self.view == View::Dialog {
            if k == "ok" || k == "back" {
                self.close_dialog();
            }
            return;
        }
        match k {
            "left" => self.mv(0, -1),
            "right" => self.mv(0, 1),
            "up" => self.mv(-1, 0),
            "down" => self.mv(1, 0),
            "ok" => {
                if let Some(a) = self.focus_act.clone() {
                    self.activate(a);
                }
            }
            "red" => self.red(),
            // Back on Home does nothing; in the panel it closes it
            "back" if self.view == View::Panel => self.activate(Act::PanelDone),
            _ => {}
        }
    }

    fn events(&mut self) {
        let mut e = Event::new();
        while unsafe { (self.sdl.PollEvent)(&mut e) } != 0 {
            match e.kind() {
                sdl::QUIT | sdl::APP_TERMINATING => self.quit = true,
                sdl::KEYDOWN => {
                    let (sc, _, rep) = e.key();
                    if !rep || matches!(crate::input::remote_key(sc), Some("up" | "down" | "left" | "right")) {
                        self.key(sc);
                    }
                }
                sdl::WINDOWEVENT => match e.window_event() {
                    sdl::WINDOWEVENT_FOCUS_LOST | sdl::WINDOWEVENT_HIDDEN => self.front = false,
                    sdl::WINDOWEVENT_FOCUS_GAINED | sdl::WINDOWEVENT_SHOWN => {
                        if !self.front {
                            self.front = true;
                            self.refresh();
                        }
                    }
                    _ => {}
                },
                sdl::APP_WILLENTERBACKGROUND | sdl::APP_DIDENTERBACKGROUND => self.front = false,
                sdl::APP_DIDENTERFOREGROUND => {
                    if !self.front {
                        self.front = true;
                        self.refresh();
                    }
                }
                _ => {}
            }
        }
    }

    // ---- drawing ----

    /// A tile: the coloured card, its icon or big name, the name under it, and the focus ring.
    #[allow(clippy::too_many_arguments)]
    fn tile(&mut self, x: f32, y: f32, color: Rgba, icon: Option<&str>, big: &str, small: &str, name: &str, focused: bool, selected: bool, pic: Option<String>) {
        let (w, h) = (250.0, 140.0);
        let k = if focused { 1.08 } else { 1.0 };
        let (tw, th) = (w * k, h * k);
        let (tx, ty) = (x - (tw - w) * 0.5, y - (th - h) * 0.5);
        if focused || selected {
            let ring = if focused { white(1.0) } else { rgb(0x4caf50) };
            self.d.rect(tx - 6.0 * k, ty - 6.0 * k, tw + 12.0 * k, th + 12.0 * k, 19.0 * k, &Shape::fill([0.0; 4]).border(4.0, ring));
        }
        self.d.rect(tx, ty, tw, th, 14.0 * k, &Shape::fill(color));
        let mut drew = false;
        if let Some(n) = pic {
            let t = self.thumb(&n).map(|t| (t.id, t.w, t.h));
            if let Some((id, tw0, th0)) = t {
                let tex = Texture { id, w: tw0, h: th0 };
                self.d.image(&tex, tx, ty, tw, th, 14.0 * k, white(1.0), false);
                std::mem::forget(tex);
                drew = true;
            }
        } else if let Some(p) = icon {
            let t = self.icon(p).map(|t| (t.id, t.w, t.h));
            if let Some((id, iw, ih)) = t {
                let tex = Texture { id, w: iw, h: ih };
                // as big as it is, up to 130 across (the web launcher's max-width/max-height)
                let s = (130.0 / iw.max(ih).max(1) as f32).min(1.0) * k;
                let (dw, dh) = (iw as f32 * s, ih as f32 * s);
                self.d.image(&tex, tx + (tw - dw) * 0.5, ty + (th - dh) * 0.5, dw, dh, 0.0, white(1.0), false);
                std::mem::forget(tex);
                drew = true;
            }
        }
        if !drew {
            let size = 34.0 * k;
            let lines = self.d.wrap(big, size, MEDIUM, tw - 28.0);
            let n = lines.len().min(2) as f32;
            let top = ty + th * 0.5 - n * size * 0.62;
            for (i, l) in lines.iter().take(2).enumerate() {
                self.d.text_center(l, tx + tw * 0.5, top + i as f32 * size * 1.2, size, MEDIUM, white(1.0));
            }
            if !small.is_empty() {
                self.d.text_center(small, tx + tw * 0.5, ty + th - 10.0 - 19.0, 19.0, REGULAR, white(0.8));
            }
        }
        let a = if focused { 1.0 } else { 0.6 };
        self.d.text_fit(name, x, y + h + 12.0, 22.0, REGULAR, white(a), w);
    }

    fn pill(&mut self, x: f32, y: f32, title: &str, focused: bool) -> f32 {
        let w = self.d.measure(title, 25.0, REGULAR, 0.0) + 60.0;
        let h = 25.0 + 28.0;
        let k = if focused { 1.07 } else { 1.0 };
        let (pw, ph) = (w * k, h * k);
        let (px, py) = (x - (pw - w) * 0.5, y - (ph - h) * 0.5);
        self.d.rect(px, py, pw, ph, ph * 0.5, &Shape::fill(if focused { white(1.0) } else { white(0.10) }));
        let c = if focused { rgb(0x0b0f17) } else { rgb(0xeef1f6) };
        self.d.text_center(title, px + pw * 0.5, py + (ph - 25.0 * k) * 0.5, 25.0 * k, REGULAR, c);
        w
    }

    fn is_focus(&self, a: &Act) -> bool {
        self.focus_act.as_ref() == Some(a)
    }

    fn draw_background(&mut self) {
        self.d.begin(rgb(0x0b0f17));
        // radial-gradient(ellipse at 15% 0%, #1f2c47 0%, #0b0f17 62%)
        self.d.radial(1920.0 * 0.15, 0.0, 1650.0, 1150.0, rgb(0x1f2c47));
        let fade = |born: Instant| (born.elapsed().as_secs_f32() / 1.5).clamp(0.0, 1.0);
        let mut drop_old = false;
        if let Some(o) = &self.old_pic {
            let a = 1.0 - fade(o.born);
            if a <= 0.0 {
                drop_old = true;
            } else {
                let t = Texture { id: o.tex.id, w: o.tex.w, h: o.tex.h };
                self.d.image(&t, 0.0, 0.0, 1920.0, 1080.0, 0.0, white(a), false);
                std::mem::forget(t);
            }
        }
        if drop_old {
            if let Some(o) = self.old_pic.take() {
                self.d.delete_texture(&o.tex);
            }
        }
        let mut tint = 0.0;
        if let Some(p) = &self.pic {
            let a = fade(p.born);
            let t = Texture { id: p.tex.id, w: p.tex.w, h: p.tex.h };
            self.d.image(&t, 0.0, 0.0, 1920.0, 1080.0, 0.0, white(a), false);
            std::mem::forget(t);
            tint = a;
        } else if let Some(o) = &self.old_pic {
            tint = 1.0 - fade(o.born);
        }
        if tint > 0.0 {
            self.d.rect(0.0, 0.0, 1920.0, 1080.0, 0.0, &Shape::vertical(rgba(0x06090f, 0.30 * tint), rgba(0x06090f, 0.62 * tint)));
        }
    }

    fn draw_home(&mut self) -> Vec<Vec<Spot>> {
        let mut rows = Vec::new();
        // the clock
        let (time, date) = clock_text();
        self.d.text(&time, 125.0, 95.0 - 52.0, 76.0, LIGHT, rgb(0xeef1f6), 1.0);
        self.d.text(&date, 125.0, 95.0 + 32.0, 27.0, REGULAR, rgba(0xeef1f6, 0.75), 0.0);
        // the top bar, right-aligned
        let bar: [(&str, Act); 7] = [
            ("Inputs", Act::Launch("com.webos.app.homeconnect".into())),
            ("Backgrounds", Act::Panel),
            ("Upload", Act::Upload),
            ("Settings", Act::Launch("com.palm.app.settings".into())),
            ("App Store", Act::Launch("com.webos.app.discovery".into())),
            ("Homebrew", Act::Launch("org.webosbrew.hbchannel".into())),
            ("LG Home", Act::LgHome),
        ];
        let widths: Vec<f32> = bar.iter().map(|(t, _)| self.d.measure(t, 25.0, REGULAR, 0.0) + 60.0).collect();
        let total: f32 = widths.iter().sum::<f32>() + 18.0 * (bar.len() as f32 - 1.0);
        let mut x = 1920.0 - 125.0 - total;
        let y = 95.0 - 26.5;
        let mut row = Vec::new();
        for ((t, a), w) in bar.iter().zip(widths) {
            let f = self.is_focus(a);
            self.pill(x, y, t, f);
            row.push(Spot { act: a.clone(), x, y, w, h: 53.0 });
            x += w + 18.0;
        }
        rows.push(row);
        // Play
        let mut y = 190.0 + 18.0;
        self.d.text("Play", 125.0, y, 27.0, REGULAR, white(0.7), 0.0);
        y += 27.0 + 16.0 + 4.0;
        let mut row = Vec::new();
        let inputs = self.inputs.clone();
        let mut x = 125.0;
        for i in &inputs {
            let a = Act::Launch(i.id.clone());
            let f = self.is_focus(&a);
            self.tile(x, y, i.color, None, &i.big, "", &i.title, f, false, None);
            row.push(Spot { act: a, x, y, w: 250.0, h: 140.0 });
            x += 250.0 + 34.0;
        }
        let f = self.is_focus(&Act::AirPlay);
        self.tile(x, y, rgb(0x1c1c1e), Some(AIRPLAY_ICON), "AirPlay", "", "AirPlay", f, false, None);
        row.push(Spot { act: Act::AirPlay, x, y, w: 250.0, h: 140.0 });
        rows.push(row);
        // Apps
        y += 140.0 + 12.0 + 22.0 + 18.0 + 8.0;
        self.d.text("Apps", 125.0, y, 27.0, REGULAR, white(0.7), 0.0);
        y += 27.0 + 16.0 + 4.0;
        let apps = self.apps.clone();
        let mut row = Vec::new();
        for (i, a) in apps.iter().enumerate() {
            if i > 0 && i % COLS == 0 {
                rows.push(std::mem::take(&mut row));
                y += 140.0 + 12.0 + 22.0 + 26.0;
            }
            let x = 125.0 + (i % COLS) as f32 * (250.0 + 34.0);
            let act = Act::Launch(a.id.clone());
            let f = self.is_focus(&act);
            let icon = (!a.icon.is_empty()).then_some(a.icon.as_str());
            self.tile(x, y, a.color, icon, &a.title, "", &a.title, f, false, None);
            row.push(Spot { act, x, y, w: 250.0, h: 140.0 });
        }
        if !row.is_empty() {
            rows.push(row);
        }
        rows
    }

    fn draw_panel(&mut self) -> Vec<Vec<Spot>> {
        let mut rows = Vec::new();
        self.d.flush_text();
        self.d.rect(0.0, 0.0, 1920.0, 1080.0, 0.0, &Shape::fill(rgba(0x06090f, 0.94)));
        self.d.text("Backgrounds", 125.0, 60.0, 48.0, LIGHT, rgb(0xeef1f6), 0.0);
        let every = |n: u32| if n < 60 { format!("{n} min") } else { "1 hour".into() };
        let pills = [
            (format!("Shuffle: {}", if self.bgs.mode == "shuffle" { "On" } else { "Off" }), Act::Shuffle),
            (format!("Change every {}", every(self.bgs.interval)), Act::Interval),
            (format!("Screensaver: {}", if self.bgs.saver == "on" { "On" } else { "Off" }), Act::Saver),
            ("Add pictures".to_string(), Act::Upload),
            ("Done".to_string(), Act::PanelDone),
        ];
        let mut x = 125.0;
        let y = 60.0 + 48.0 + 26.0;
        let mut row = Vec::new();
        for (t, a) in pills.iter() {
            let f = self.is_focus(a);
            let w = self.pill(x, y, t, f);
            row.push(Spot { act: a.clone(), x, y, w, h: 53.0 });
            x += w + 18.0;
        }
        rows.push(row);
        // the pictures, scrolled so the focused one shows
        let top = 60.0 + 182.0 + 18.0;
        let bottom = 1080.0 - 40.0 - 50.0;
        let mut items: Vec<(Act, String, String, bool, Option<String>)> =
            vec![(Act::BgNone, "None".into(), if self.bgs.mode == "none" { "In use".into() } else { "No picture".into() }, self.bgs.mode == "none", None)];
        for (i, n) in self.bgs.files.clone().iter().enumerate() {
            let in_use = self.bgs.mode == "one" && self.bgs.current == *n;
            let title = if in_use { "In use".to_string() } else if self.bgs.mode == "shuffle" { "Shuffling".to_string() } else { format!("Picture {}", i + 1) };
            items.push((Act::BgPick(n.clone()), format!("Picture {}", i + 1), title, in_use, Some(n.clone())));
        }
        let pitch = 140.0 + 12.0 + 22.0 + 26.0;
        if let Some(fi) = items.iter().position(|it| self.is_focus(&it.0)) {
            let ry = (fi / COLS) as f32 * pitch;
            if ry - self.panel_scroll + 140.0 + 60.0 > bottom - top {
                self.panel_scroll = ry + 140.0 + 60.0 - (bottom - top);
            } else if ry < self.panel_scroll {
                self.panel_scroll = ry;
            }
        }
        self.d.push_clip(100.0, top - 24.0, 1720.0, bottom - top + 24.0);
        let mut row = Vec::new();
        for (i, (a, big, title, sel, pic)) in items.iter().enumerate() {
            if i > 0 && i % COLS == 0 {
                rows.push(std::mem::take(&mut row));
            }
            let x = 125.0 + (i % COLS) as f32 * (250.0 + 34.0);
            let y = top + (i / COLS) as f32 * pitch - self.panel_scroll;
            let f = self.is_focus(a);
            if y > -200.0 && y < 1100.0 {
                self.tile(x, y, rgb(0x1c2333), None, big, "", title, f, *sel, pic.clone());
            }
            row.push(Spot { act: a.clone(), x, y, w: 250.0, h: 140.0 });
        }
        if !row.is_empty() {
            rows.push(row);
        }
        self.d.flush_text();
        self.d.pop_clip();
        self.d.text("OK: use this picture  ·  Red button twice: delete it  ·  Back: close", 125.0, 1080.0 - 40.0 - 26.0, 22.0, REGULAR, white(0.6), 0.0);
        rows
    }

    fn draw_dialog(&mut self) {
        self.d.flush_text();
        self.d.rect(0.0, 0.0, 1920.0, 1080.0, 0.0, &Shape::fill(rgba(0x06090f, 0.94)));
        let cx = 960.0;
        let mut y = 260.0;
        self.d.text_center("Add pictures or games from your phone", cx, y, 48.0, LIGHT, rgb(0xeef1f6));
        y += 48.0 + 26.0 + 12.0;
        self.d.text_center("On a phone connected to the same Wi-Fi as this TV, open this address in the web browser:", cx, y, 30.0, REGULAR, white(0.8));
        y += 30.0 + 36.0 + 12.0;
        let url = self.upload_url.clone();
        self.d.text_center(&url, cx, y, 68.0, MEDIUM, rgb(0xeef1f6));
        y += 68.0 + 36.0;
        let added = self.bgs.files.len().saturating_sub(self.upload_before);
        let st = if added == 0 { "Waiting for pictures...".to_string() } else { format!("{added} new picture{} received", if added == 1 { "" } else { "s" }) };
        self.d.text_center(&st, cx, y, 30.0, REGULAR, white(0.8));
        y += 30.0 + 40.0 + 12.0;
        self.d.text_center("Press OK or Back on the remote when you're done.", cx, y, 24.0, REGULAR, white(0.55));
    }

    fn draw(&mut self) {
        self.draw_background();
        let home_rows = self.draw_home();
        let rows = match self.view {
            View::Home => home_rows,
            View::Panel => self.draw_panel(),
            View::Dialog => {
                self.draw_dialog();
                Vec::new()
            }
        };
        if self.view != View::Dialog {
            self.rows = rows;
            // keep the focus on the same thing; the first app otherwise
            let found = self.focus_act.as_ref().and_then(|a| {
                self.rows.iter().enumerate().find_map(|(r, row)| row.iter().position(|s| &s.act == a).map(|c| (r, c)))
            });
            let first_app = (self.view == View::Home && !self.user_moved).then(|| self.rows.get(2).and_then(|r| r.first()).map(|s| s.act.clone())).flatten();
            if let Some(a) = first_app {
                self.focus = (2, 0);
                self.focus_act = Some(a);
                return self.draw_say();
            }
            match found {
                Some(p) => self.focus = p,
                None => {
                    let first = if self.view == View::Home { self.rows.len().saturating_sub(1).min(2) } else { self.rows.len().min(2).saturating_sub(1) };
                    if let Some(s) = self.rows.get(first).and_then(|r| r.first()) {
                        self.focus = (first, 0);
                        self.focus_act = Some(s.act.clone());
                    }
                }
            }
        }
        self.draw_say();
    }

    /// The short message at the bottom right, then the frame is finished.
    fn draw_say(&mut self) {
        if let Some((s, t)) = &self.say {
            if t.elapsed() < Duration::from_secs(5) {
                let s = s.clone();
                let w = self.d.measure(&s, 22.0, REGULAR, 0.0);
                self.d.text(&s, 1920.0 - 125.0 - w, 1080.0 - 34.0 - 22.0, 22.0, REGULAR, rgb(0xff8a80), 0.0);
            } else {
                self.say = None;
            }
        }
        self.d.end();
    }

    fn run(&mut self) {
        let mut last_clock = String::new();
        let mut last_draw = Instant::now() - Duration::from_secs(1);
        while !self.quit {
            self.events();
            self.messages();
            if let Some(t) = self.next_shuffle {
                if Instant::now() >= t && self.front {
                    self.shuffle_next();
                    self.next_shuffle = Some(Instant::now() + Duration::from_secs(u64::from(self.bgs.interval.max(1)) * 60));
                }
            }
            if self.view == View::Dialog && self.upload_poll.elapsed() > Duration::from_secs(4) {
                self.upload_poll = Instant::now();
                let tx = self.tx.clone();
                spawn(move || {
                    if let Some(b) = parse_bgs(&bg_sh(&["json"])) {
                        let _ = tx.send(Msg::Bg(b, false));
                    }
                });
            }
            if !self.front {
                // behind an app: nothing to draw, nearly nothing to do
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            // Only the clock, a fade, a message or the remote change the picture: draw then,
            // otherwise rest (the last frame stays on screen).
            let fading = self.pic.as_ref().map(|p| p.born.elapsed().as_secs_f32() < 1.6).unwrap_or(false) || self.old_pic.is_some();
            let clock = clock_text().0;
            if clock != last_clock {
                last_clock = clock;
                self.dirty = true;
            }
            if self.say.as_ref().map(|(_, t)| t.elapsed() > Duration::from_secs(5)).unwrap_or(false) {
                self.dirty = true;
            }
            if fading || self.dirty || last_draw.elapsed() > Duration::from_secs(5) {
                self.dirty = false;
                self.draw();
                unsafe { (self.sdl.GL_SwapWindow)(self.win) };
                last_draw = Instant::now();
            } else {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        crate::logln!("Home closed");
    }

}

/// `continuum-tv --home [SAM's launch JSON]`.
pub fn run(arg: Option<&str>) {
    crate::log::open("/tmp/kiro-home-native.log");
    crate::logln!("Kiro Home (native) starting: {}", arg.unwrap_or("").chars().take(160).collect::<String>());
    // the window belongs to Home, so SAM and the surface manager treat it as Home
    if std::env::var_os("APPID").is_none() {
        std::env::set_var("APPID", "com.webos.app.home");
    }
    if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
        std::env::set_var("XDG_RUNTIME_DIR", "/tmp/xdg");
    }
    let (tx, rx) = channel();
    spawn(boost_continuum);
    let sam = register_with_sam(tx.clone());
    if sam.is_none() {
        crate::logln!("couldn't register with SAM");
    }
    // Preloaded by SAM at boot: wait for the Home key before opening a window.
    if arg.map(|a| a.contains("\"reason\":\"preload\"") || a.contains("\"reason\": \"preload\"")).unwrap_or(false) {
        crate::logln!("preloaded: waiting for the Home key");
        loop {
            match rx.recv() {
                Ok(Msg::Relaunch) => break,
                Ok(Msg::Close) | Err(_) => return,
                _ => {}
            }
        }
    }
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_default();
    let sdl = match Sdl::open(&exe_dir.join("lib/libSDL2-2.0.so.0").to_string_lossy()).or_else(|_| Sdl::open("libSDL2-2.0.so.0")) {
        Ok(s) => s,
        Err(e) => {
            crate::logln!("no SDL: {e}");
            std::process::exit(1);
        }
    };
    // Back reaches the launcher (it closes the panel); Home and Exit stay the TV's own.
    sdl.hint("SDL_WEBOS_ACCESS_POLICY_KEYS_BACK", "true");
    sdl.hint("SDL_WEBOS_CURSOR_SLEEP_TIME", "1000");
    unsafe {
        if (sdl.Init)(sdl::INIT_VIDEO | sdl::INIT_EVENTS | sdl::INIT_TIMER) != 0 {
            crate::logln!("SDL didn't start: {}", sdl.error());
            std::process::exit(1);
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
    let title = CString::new("Home").unwrap();
    let win = unsafe { (sdl.CreateWindow)(title.as_ptr(), 0, 0, 1920, 1080, sdl::WINDOW_OPENGL | sdl::WINDOW_SHOWN | sdl::WINDOW_FULLSCREEN) };
    if win.is_null() {
        crate::logln!("no window: {}", sdl.error());
        std::process::exit(1);
    }
    let ctx = unsafe { (sdl.GL_CreateContext)(win) };
    if ctx.is_null() {
        crate::logln!("no OpenGL: {}", sdl.error());
        std::process::exit(1);
    }
    let (mut dw, mut dh) = (0, 0);
    unsafe {
        (sdl.GL_MakeCurrent)(win, ctx);
        (sdl.GL_SetSwapInterval)(1);
        (sdl.ShowCursor)(0);
        (sdl.GL_GetDrawableSize)(win, &mut dw, &mut dh);
    }
    if dw <= 0 {
        (dw, dh) = (1920, 1080);
    }
    let getp = |name: &str| -> *mut c_void {
        let c = CString::new(name).unwrap();
        unsafe { (sdl.GL_GetProcAddress)(c.as_ptr()) }
    };
    let gl = match crate::gl::Gl::load(&getp) {
        Ok(g) => g,
        Err(e) => {
            crate::logln!("no GL functions: {e}");
            std::process::exit(1);
        }
    };
    let fonts = match fonts() {
        Ok(f) => f,
        Err(e) => {
            crate::logln!("{e}");
            std::process::exit(1);
        }
    };
    let d = match Draw::new(gl, fonts, dw, dh) {
        Ok(d) => d,
        Err(e) => {
            crate::logln!("drawing didn't start: {e}");
            std::process::exit(1);
        }
    };
    crate::logln!("window {dw}x{dh}");
    let mut home = Home {
        sdl,
        win,
        d,
        tx,
        rx,
        view: View::Home,
        dialog_from: View::Home,
        apps: Vec::new(),
        inputs: Vec::new(),
        icons: HashMap::new(),
        thumbs: HashMap::new(),
        bgs: Bgs { mode: "none".into(), interval: 15, saver: "off".into(), ..Default::default() },
        pic: None,
        old_pic: None,
        loading: None,
        shuffle_bag: Vec::new(),
        next_shuffle: None,
        focus: (2, 0),
        focus_act: None,
        home_focus: None,
        rows: Vec::new(),
        panel_scroll: 0.0,
        red_armed: None,
        say: None,
        upload_url: String::new(),
        upload_before: 0,
        upload_poll: Instant::now(),
        front: true,
        quit: false,
        busy_until: Instant::now(),
        dirty: true,
        user_moved: false,
        _sam: sam,
    };
    home.refresh();
    home.run();
}
