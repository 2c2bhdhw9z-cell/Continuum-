//! The remote and controllers: menu moves (with repeat when held) and, in a game, the buttons
//! the player mapped.

use crate::config::{Action, Settings};
use crate::retro;
use crate::sdl::{self, Event, Sdl};
use std::collections::HashSet;
use std::ffi::c_void;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    Ok,
    Back,
    Options,
    Favourite,
    TabPrev,
    TabNext,
}

/// The remote's key names (config.rs REMOTE_KEYS) from SDL scancodes (webosbrew SDL).
pub fn remote_key(scancode: i32) -> Option<&'static str> {
    Some(match scancode {
        82 => "up",
        81 => "down",
        80 => "left",
        79 => "right",
        40 | 88 => "ok",
        482 | 41 => "back",
        486 => "red",
        487 => "green",
        488 => "yellow",
        489 => "blue",
        30 => "1",
        31 => "2",
        32 => "3",
        33 => "4",
        34 => "5",
        35 => "6",
        36 => "7",
        37 => "8",
        38 => "9",
        39 => "0",
        261 => "play",
        260 => "pause",
        286 => "ff",
        285 => "rew",
        480 => "chup",
        481 => "chdown",
        505 => "exit",
        _ => return None,
    })
}

pub struct Pad {
    ctrl: *mut c_void,
    pub id: i32,
    pub name: String,
    pub buttons: [bool; 21],
    pub axes: [i16; 6],
}

pub struct Input {
    pub held: HashSet<&'static str>,
    pub pads: Vec<Pad>,
    pub nav: Vec<Nav>,
    repeat: Option<(Nav, Instant)>,
    ok_down: Option<Instant>,
    ok_hold_fired: bool,
    /// Pressed this frame (for actions that fire once: menu, quick save and load).
    pub pressed: Vec<Action>,
    turbo_tick: u32,
    pub last_used_pad: bool,
}

const AXIS_LEFTX: usize = 0;
const AXIS_LEFTY: usize = 1;
const AXIS_RIGHTX: usize = 2;
const AXIS_RIGHTY: usize = 3;
const AXIS_LT: usize = 4;
const AXIS_RT: usize = 5;

impl Input {
    pub fn new() -> Input {
        Input {
            held: HashSet::new(),
            pads: Vec::new(),
            nav: Vec::new(),
            repeat: None,
            ok_down: None,
            ok_hold_fired: false,
            pressed: Vec::new(),
            turbo_tick: 0,
            last_used_pad: false,
        }
    }

    pub fn open_pads(&mut self, sdl: &Sdl) {
        let n = unsafe { (sdl.NumJoysticks)() };
        for i in 0..n {
            self.add_pad(sdl, i);
        }
    }

    fn add_pad(&mut self, sdl: &Sdl, index: i32) {
        unsafe {
            if (sdl.IsGameController)(index) == 0 {
                return;
            }
            let c = (sdl.GameControllerOpen)(index);
            if c.is_null() {
                return;
            }
            let id = (sdl.JoystickInstanceID)((sdl.GameControllerGetJoystick)(c));
            if self.pads.iter().any(|p| p.id == id) {
                return;
            }
            let np = (sdl.GameControllerName)(c);
            let name = if np.is_null() { "Controller".into() } else { std::ffi::CStr::from_ptr(np).to_string_lossy().into_owned() };
            crate::logln!("controller connected: {name}");
            self.pads.push(Pad { ctrl: c, id, name, buttons: [false; 21], axes: [0; 6] });
        }
    }

    pub fn rumble(&self, sdl: &Sdl, strength: u16) {
        for p in &self.pads {
            unsafe { (sdl.GameControllerRumble)(p.ctrl, strength, strength, if strength > 0 { 5000 } else { 0 }) };
        }
    }

    /// Handle one SDL event. Returns true if it was input.
    pub fn event(&mut self, sdl: &Sdl, e: &Event, s: &Settings, in_game: bool) -> bool {
        match e.kind() {
            sdl::KEYDOWN | sdl::KEYUP => {
                let (sc, _kc, rep) = e.key();
                let down = e.kind() == sdl::KEYDOWN;
                let key = match remote_key(sc) {
                    Some(k) => k,
                    None => {
                        if down && !rep {
                            crate::logln!("key {sc} ({}) has no use", sdl.scancode_name(sc));
                        }
                        return true;
                    }
                };
                self.last_used_pad = false;
                if down {
                    self.held.insert(key);
                } else {
                    self.held.remove(key);
                }
                if in_game {
                    if down && !rep {
                        if let Some(a) = s.remote.get(key) {
                            self.pressed.push(*a);
                        }
                    }
                    return true;
                }
                // menus
                let nav = match key {
                    "up" => Some(Nav::Up),
                    "down" => Some(Nav::Down),
                    "left" => Some(Nav::Left),
                    "right" => Some(Nav::Right),
                    "back" | "exit" => Some(Nav::Back),
                    "yellow" | "blue" => Some(Nav::Options),
                    "green" => Some(Nav::Favourite),
                    "chup" => Some(Nav::TabPrev),
                    "chdown" => Some(Nav::TabNext),
                    "ok" | "play" => {
                        // OK fires on release; held, it opens the options instead.
                        if down && !rep {
                            self.ok_down = Some(Instant::now());
                            self.ok_hold_fired = false;
                        } else if !down {
                            if self.ok_down.take().is_some() && !self.ok_hold_fired {
                                self.nav.push(Nav::Ok);
                            }
                        }
                        None
                    }
                    _ => None,
                };
                if let Some(n) = nav {
                    if down {
                        self.nav.push(n);
                    }
                }
                true
            }
            sdl::CONTROLLERDEVICEADDED => {
                self.add_pad(sdl, e.cdevice());
                true
            }
            sdl::CONTROLLERDEVICEREMOVED => {
                let id = e.cdevice();
                if let Some(i) = self.pads.iter().position(|p| p.id == id) {
                    let p = self.pads.remove(i);
                    unsafe { (sdl.GameControllerClose)(p.ctrl) };
                    crate::logln!("controller gone: {}", p.name);
                }
                true
            }
            sdl::CONTROLLERBUTTONDOWN | sdl::CONTROLLERBUTTONUP => {
                let (id, b) = e.cbutton();
                let down = e.kind() == sdl::CONTROLLERBUTTONDOWN;
                self.last_used_pad = true;
                if let Some(p) = self.pads.iter_mut().find(|p| p.id == id) {
                    if (b as usize) < 21 {
                        p.buttons[b as usize] = down;
                    }
                }
                if down {
                    if in_game {
                        if let Some(a) = s.pad.get(&b) {
                            self.pressed.push(*a);
                        }
                        if s.menu_combo && self.pad_held(4) && self.pad_held(6) {
                            self.pressed.push(Action::Menu);
                        }
                    } else {
                        let n = match b {
                            0 => Some(Nav::Ok),
                            1 => Some(Nav::Back),
                            2 => Some(Nav::Favourite),
                            3 => Some(Nav::Options),
                            6 => Some(Nav::Options),
                            9 => Some(Nav::TabPrev),
                            10 => Some(Nav::TabNext),
                            11 => Some(Nav::Up),
                            12 => Some(Nav::Down),
                            13 => Some(Nav::Left),
                            14 => Some(Nav::Right),
                            _ => None,
                        };
                        if let Some(n) = n {
                            self.nav.push(n);
                            if matches!(n, Nav::Up | Nav::Down | Nav::Left | Nav::Right) {
                                self.repeat = Some((n, Instant::now() + Duration::from_millis(380)));
                            }
                        }
                    }
                } else if let Some((n, _)) = self.repeat {
                    let held_dir = match n {
                        Nav::Up => 11,
                        Nav::Down => 12,
                        Nav::Left => 13,
                        Nav::Right => 14,
                        _ => 255,
                    };
                    if held_dir == b {
                        self.repeat = None;
                    }
                }
                true
            }
            sdl::CONTROLLERAXISMOTION => {
                let (id, axis, v) = e.caxis();
                if let Some(p) = self.pads.iter_mut().find(|p| p.id == id) {
                    if (axis as usize) < 6 {
                        let old = p.axes[axis as usize];
                        p.axes[axis as usize] = v;
                        // triggers as buttons
                        if (axis as usize) == AXIS_LT || (axis as usize) == AXIS_RT {
                            let code = if axis as usize == AXIS_LT { 100u8 } else { 101u8 };
                            if old < 16000 && v >= 16000 {
                                self.last_used_pad = true;
                                if in_game {
                                    if let Some(a) = s.pad.get(&code) {
                                        self.pressed.push(*a);
                                    }
                                }
                            }
                        }
                        // the left stick moves through menus
                        if !in_game && (axis as usize == AXIS_LEFTX || axis as usize == AXIS_LEFTY) {
                            let was = old.unsigned_abs() > 20000;
                            let now = v.unsigned_abs() > 20000;
                            if now && !was {
                                let n = match (axis as usize, v > 0) {
                                    (AXIS_LEFTX, true) => Nav::Right,
                                    (AXIS_LEFTX, false) => Nav::Left,
                                    (_, true) => Nav::Down,
                                    (_, false) => Nav::Up,
                                };
                                self.nav.push(n);
                                self.repeat = Some((n, Instant::now() + Duration::from_millis(380)));
                            } else if !now && was {
                                self.repeat = None;
                            }
                        }
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn pad_held(&self, b: usize) -> bool {
        self.pads.iter().any(|p| p.buttons.get(b).copied().unwrap_or(false))
    }

    /// Repeats for held directions on a controller, and OK held down.
    pub fn tick(&mut self) {
        if let Some((n, at)) = self.repeat {
            if Instant::now() >= at {
                self.nav.push(n);
                self.repeat = Some((n, Instant::now() + Duration::from_millis(110)));
            }
        }
        if let Some(t) = self.ok_down {
            if !self.ok_hold_fired && t.elapsed() > Duration::from_millis(650) {
                self.ok_hold_fired = true;
                self.nav.push(Nav::Options);
            }
        }
    }

    pub fn clear(&mut self) {
        self.nav.clear();
        self.pressed.clear();
        self.repeat = None;
        self.ok_down = None;
    }

    /// Everything held right now, for the game: (buttons for player 1, left stick, fast forward held)
    pub fn game_state(&mut self, s: &Settings) -> (u16, [i16; 4], bool) {
        self.turbo_tick = self.turbo_tick.wrapping_add(1);
        let turbo_on = (self.turbo_tick / s.turbo_rate.max(1)) % 2 == 0;
        let mut mask: u16 = 0;
        let mut fast = false;
        let apply = |a: Action, mask: &mut u16, fast: &mut bool| {
            let bit = |id: u32| 1u16 << id;
            match a {
                Action::Up => *mask |= bit(retro::UP),
                Action::Down => *mask |= bit(retro::DOWN),
                Action::Left => *mask |= bit(retro::LEFT),
                Action::Right => *mask |= bit(retro::RIGHT),
                Action::A => *mask |= bit(retro::A),
                Action::B => *mask |= bit(retro::B),
                Action::Start => *mask |= bit(retro::START),
                Action::Select => *mask |= bit(retro::SELECT),
                Action::L => *mask |= bit(retro::L),
                Action::R => *mask |= bit(retro::R),
                Action::TurboA => {
                    if turbo_on {
                        *mask |= bit(retro::A)
                    }
                }
                Action::TurboB => {
                    if turbo_on {
                        *mask |= bit(retro::B)
                    }
                }
                Action::FastForward => *fast = true,
                _ => {}
            }
        };
        for k in self.held.iter() {
            match *k {
                "up" => apply(Action::Up, &mut mask, &mut fast),
                "down" => apply(Action::Down, &mut mask, &mut fast),
                "left" => apply(Action::Left, &mut mask, &mut fast),
                "right" => apply(Action::Right, &mut mask, &mut fast),
                other => {
                    if let Some(a) = s.remote.get(other) {
                        apply(*a, &mut mask, &mut fast);
                    }
                }
            }
        }
        let mut stick = [0i16; 4];
        for p in &self.pads {
            for (b, down) in p.buttons.iter().enumerate() {
                if *down {
                    if let Some(a) = s.pad.get(&(b as u8)) {
                        apply(*a, &mut mask, &mut fast);
                    }
                }
            }
            for (code, axis) in [(100u8, AXIS_LT), (101u8, AXIS_RT)] {
                if p.axes[axis] >= 16000 {
                    if let Some(a) = s.pad.get(&code) {
                        apply(*a, &mut mask, &mut fast);
                    }
                }
            }
            let (lx, ly) = (p.axes[AXIS_LEFTX], p.axes[AXIS_LEFTY]);
            if stick[0] == 0 && stick[1] == 0 {
                stick = [lx, ly, p.axes[AXIS_RIGHTX], p.axes[AXIS_RIGHTY]];
            }
            if s.stick_moves {
                let dz = 14000;
                if lx > dz {
                    mask |= 1 << retro::RIGHT;
                }
                if lx < -dz {
                    mask |= 1 << retro::LEFT;
                }
                if ly > dz {
                    mask |= 1 << retro::DOWN;
                }
                if ly < -dz {
                    mask |= 1 << retro::UP;
                }
            }
        }
        (mask, stick, fast)
    }
}
