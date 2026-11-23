//! The New look's pieces for a TV: glass cards, the gold button with its running light, console
//! tiles, game cards, settings rows. Moving round them with the remote picks the nearest thing in
//! the direction pressed.

use crate::config::Settings;
use crate::draw::{rgba, white, with_alpha, Draw, Rgba, Shape, BOLD, LIGHT, MEDIUM, REGULAR};
use crate::library::{Game, System};
use crate::swirl::hex_rgb;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn cx(&self) -> f32 {
        self.x + self.w * 0.5
    }
    pub fn cy(&self) -> f32 {
        self.y + self.h * 0.5
    }
    pub fn grow(&self, k: f32) -> Rect {
        let (dw, dh) = (self.w * (k - 1.0) * 0.5, self.h * (k - 1.0) * 0.5);
        Rect { x: self.x - dw, y: self.y - dh, w: self.w + dw * 2.0, h: self.h + dh * 2.0 }
    }
}

pub fn id(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn c4(c: [f64; 3]) -> Rgba {
    [c[0] as f32, c[1] as f32, c[2] as f32, 1.0]
}

/// Colours and switches for one frame, from the settings.
pub struct Theme {
    pub accent: Vec<Rgba>,
    pub first: Rgba,
    pub second: Rgba,
    pub on_accent: Rgba,
    pub bg: Rgba,
    pub light: bool,
    pub light_angle: f32,
    pub ring_mode: f32,
    pub ring: Rgba,
    pub ring_angle: f32,
    pub glow_mode: String,
    pub glow: Rgba,
    pub hero_glow: bool,
    pub floats: bool,
    pub corner: f32,
    pub hide_titles: bool,
    pub pulse: bool,
    pub grow: bool,
    pub t: f64,
    pub danger: Rgba,
}

impl Theme {
    pub fn new(s: &Settings, t: f64) -> Theme {
        let accent: Vec<Rgba> = s.accent_stops().iter().map(|h| c4(hex_rgb(h))).collect();
        let first = accent[0];
        let second = if accent.len() > 1 { accent[1] } else { first };
        let mid = [(first[0] + second[0]) * 0.5, (first[1] + second[1]) * 0.5, (first[2] + second[2]) * 0.5];
        let lum = 0.2126 * mid[0] + 0.7152 * mid[1] + 0.0722 * mid[2];
        let period = 2.4 / s.light_speed.max(0.1);
        let light_angle = ((t / period).fract() * std::f64::consts::TAU) as f32;
        let ring_angle = if s.ring_spins { ((t / 3.5).fract() * std::f64::consts::TAU) as f32 } else { 0.0 };
        let ring_mode = match s.ring_mode.as_str() {
            "accent" => 1.0,
            "custom" => 2.0,
            _ => 0.0,
        };
        Theme {
            on_accent: if lum > 0.6 { [0.0, 0.0, 0.0, 1.0] } else { [1.0; 4] },
            bg: c4(hex_rgb(&s.background)),
            light: s.button_light,
            light_angle,
            ring_mode,
            ring: c4(hex_rgb(&s.ring)),
            ring_angle,
            glow_mode: s.glow_mode.clone(),
            glow: c4(hex_rgb(&s.glow)),
            hero_glow: s.hero_glow,
            floats: s.box_floats,
            corner: s.cover_corner as f32,
            hide_titles: s.hide_titles,
            pulse: s.focus_pulse,
            grow: s.focus_grow,
            t,
            danger: [0.93, 0.16, 0.29, 1.0],
            accent,
            first,
            second,
        }
    }

    pub fn accent_fill(&self) -> Shape {
        Shape::gradient(self.first, self.second)
    }

    /// The glow colour round a game's cover.
    pub fn cover_glow(&self, sys: System) -> Option<Rgba> {
        match self.glow_mode.as_str() {
            "off" => None,
            "accent" => Some(self.second),
            "custom" => Some(self.glow),
            _ => Some(sys.colour()),
        }
    }

    /// 0..1, breathing slowly (the picked tile's glow).
    pub fn pulse_amt(&self) -> f32 {
        if self.pulse {
            (0.55 + 0.45 * (0.5 + 0.5 * (self.t * std::f64::consts::TAU / 3.0).sin())) as f32
        } else {
            1.0
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Normal,
    /// Left and right change its value instead of moving away.
    Adjust,
}

#[derive(Clone, Copy)]
pub struct Item {
    pub id: u64,
    pub r: Rect,
    pub kind: Kind,
    /// a group: moving up/down prefers items in the same group's column layout
    pub group: u32,
}

/// Picks the item to move to from `from` in direction (dx, dy).
pub fn navigate(items: &[Item], from: u64, dx: f32, dy: f32) -> Option<u64> {
    let cur = items.iter().find(|i| i.id == from)?;
    let (cx, cy) = (cur.r.cx(), cur.r.cy());
    let mut best: Option<(f32, u64)> = None;
    for it in items {
        if it.id == from {
            continue;
        }
        let (ix, iy) = (it.r.cx(), it.r.cy());
        let (vx, vy) = (ix - cx, iy - cy);
        let along = vx * dx + vy * dy;
        if along <= 1.0 {
            continue;
        }
        // must actually be past the edge in that direction
        let past = if dx > 0.0 {
            it.r.x + it.r.w * 0.5 > cur.r.x + cur.r.w * 0.5 + 1.0 && it.r.x >= cur.r.x + cur.r.w * 0.5
        } else if dx < 0.0 {
            it.r.x + it.r.w <= cur.r.x + cur.r.w * 0.5
        } else if dy > 0.0 {
            it.r.y >= cur.r.y + cur.r.h * 0.5
        } else {
            it.r.y + it.r.h <= cur.r.y + cur.r.h * 0.5
        };
        if !past {
            continue;
        }
        let across = (vx * dy - vy * dx).abs();
        // overlap along the other axis counts as perfectly in line
        let overlap = if dx != 0.0 {
            (cur.r.y + cur.r.h).min(it.r.y + it.r.h) - cur.r.y.max(it.r.y)
        } else {
            (cur.r.x + cur.r.w).min(it.r.x + it.r.w) - cur.r.x.max(it.r.x)
        };
        let across = if overlap > 0.0 { across * 0.15 } else { across };
        let score = along + across * 2.2 + if it.group != cur.group { 40.0 } else { 0.0 };
        if best.map(|b| score < b.0).unwrap_or(true) {
            best = Some((score, it.id));
        }
    }
    best.map(|b| b.1)
}

/// The per-frame drawing context.
pub struct Ctx<'a> {
    pub d: &'a mut Draw,
    pub th: &'a Theme,
    pub focus: u64,
    pub items: &'a mut Vec<Item>,
    pub anim: &'a mut HashMap<u64, f32>,
    pub dt: f32,
    pub group: u32,
    pub images: &'a crate::app::Images,
}

impl<'a> Ctx<'a> {
    pub fn add(&mut self, id: u64, r: Rect, kind: Kind) {
        self.items.push(Item { id, r, kind, group: self.group });
    }

    /// 0..1, easing in when the item is focused.
    pub fn focus_amt(&mut self, id: u64) -> f32 {
        let target = if id == self.focus { 1.0 } else { 0.0 };
        let v = self.anim.entry(id).or_insert(0.0);
        let k = (self.dt * 14.0).min(1.0);
        *v += (target - *v) * k;
        if (*v - target).abs() < 0.01 {
            *v = target;
        }
        *v
    }

    pub fn is_focus(&self, id: u64) -> bool {
        self.focus == id
    }

    // ---- glass ----

    pub fn card(&mut self, r: Rect, radius: f32) {
        self.d.rect(r.x, r.y, r.w, r.h, radius, &Shape::fill(white(0.055)).border(1.0, white(0.09)));
    }

    pub fn glass(&mut self, r: Rect, radius: f32, strength: f32) {
        self.d.rect(r.x, r.y, r.w, r.h, radius, &Shape::fill([0.10, 0.10, 0.12, 0.55 * strength]).border(1.0, white(0.12)));
    }

    /// The focus ring for anything picked: accent edge, a breathing glow, and a small lift.
    pub fn focus_ring(&mut self, r: Rect, radius: f32, amt: f32) {
        if amt <= 0.01 {
            return;
        }
        let p = self.th.pulse_amt();
        self.d.glow_ring(r.x, r.y, r.w, r.h, radius, 6.0, 9.0, with_alpha(self.th.second, 0.85 * p * amt));
        self.d.rect(r.x, r.y, r.w, r.h, radius, &Shape::fill([0.0; 4]).border(3.0, with_alpha(self.th.first, amt)));
    }

    // ---- text shortcuts ----

    pub fn label(&mut self, s: &str, x: f32, y: f32, size: f32, font: usize, c: Rgba) -> f32 {
        self.d.text(s, x, y, size, font, c, 0.0)
    }

    // ---- buttons ----

    /// The gold button with the light running round it.
    pub fn primary(&mut self, key: &str, x: f32, y: f32, label: &str, min_w: f32) -> Rect {
        let iid = id(key);
        let tw = self.d.measure(label, 30.0, MEDIUM, 0.0);
        let w = (tw + 88.0).max(min_w);
        let r = Rect::new(x, y, w, 76.0);
        self.add(iid, r, Kind::Normal);
        let amt = self.focus_amt(iid);
        let rr = if self.th.grow { r.grow(1.0 + 0.05 * amt) } else { r };
        // shadow in the accent
        self.d.glow(rr.x, rr.y + 5.0, rr.w, rr.h, rr.h * 0.5, 14.0, with_alpha(self.th.second, 0.35 + 0.25 * amt));
        let mut s = self.th.accent_fill();
        if self.th.light {
            s = s.light(self.th.light_angle, 2.4, self.th.first, self.th.second);
        }
        self.d.rect(rr.x, rr.y, rr.w, rr.h, rr.h * 0.5, &s);
        // top light
        self.d.rect(rr.x + 2.0, rr.y + 2.0, rr.w - 4.0, rr.h * 0.5, rr.h * 0.5, &Shape::vertical(white(0.22), white(0.0)));
        if amt > 0.01 {
            self.d.glow_ring(rr.x, rr.y, rr.w, rr.h, rr.h * 0.5, 4.0, 10.0, with_alpha([1.0; 4], 0.55 * amt * self.th.pulse_amt()));
            self.d.rect(rr.x - 5.0, rr.y - 5.0, rr.w + 10.0, rr.h + 10.0, (rr.h + 10.0) * 0.5, &Shape::fill([0.0; 4]).border(3.0, white(0.9 * amt)));
        }
        let on = self.th.on_accent;
        self.d.text_center(label, rr.cx(), rr.y + (rr.h - 30.0) * 0.5, 30.0, MEDIUM, on);
        r
    }

    /// A glass button.
    pub fn button(&mut self, key: &str, x: f32, y: f32, label: &str, danger: bool) -> Rect {
        let iid = id(key);
        let tw = self.d.measure(label, 28.0, MEDIUM, 0.0);
        let r = Rect::new(x, y, tw + 76.0, 76.0);
        self.add(iid, r, Kind::Normal);
        let amt = self.focus_amt(iid);
        let rr = if self.th.grow { r.grow(1.0 + 0.04 * amt) } else { r };
        let col = if danger { self.th.danger } else { white(1.0) };
        let fill = if danger { with_alpha(self.th.danger, 0.10 + 0.15 * amt) } else { white(0.08 + 0.10 * amt) };
        let border = if danger { with_alpha(self.th.danger, 0.45) } else { white(0.14) };
        self.d.rect(rr.x, rr.y, rr.w, rr.h, rr.h * 0.5, &Shape::fill(fill).border(1.0, border));
        self.focus_ring(rr, rr.h * 0.5, amt);
        self.d.text_center(label, rr.cx(), rr.y + (rr.h - 28.0) * 0.5, 28.0, MEDIUM, col);
        r
    }

    /// A small round glass button with a short label (top bar, library header).
    pub fn bead(&mut self, key: &str, x: f32, y: f32, label: &str) -> Rect {
        let iid = id(key);
        let tw = self.d.measure(label, 24.0, MEDIUM, 0.0);
        let r = Rect::new(x, y, (tw + 48.0).max(60.0), 60.0);
        self.add(iid, r, Kind::Normal);
        let amt = self.focus_amt(iid);
        self.d.rect(r.x, r.y, r.w, r.h, 30.0, &Shape::fill(white(0.10 + 0.12 * amt)).border(1.0, white(0.14)));
        self.focus_ring(r, 30.0, amt);
        self.d.text_center(label, r.cx(), r.y + 18.0, 24.0, MEDIUM, white(0.95));
        r
    }

    // ---- the CONTINUUM name and its ring ----

    pub fn wordmark(&mut self, x: f32, y: f32) {
        let w = self.d.text("CONTINUUM", x, y, 34.0, LIGHT, white(1.0), 3.6);
        let (c1, c2) = if self.th.ring_mode > 1.5 { (self.th.ring, self.th.ring) } else { (self.th.first, self.th.second) };
        self.d.ring(x + w + 34.0, y + 17.0, 18.0, 5.0, self.th.ring_angle, self.th.ring_mode, c1, c2);
    }

    // ---- covers and game cards ----

    /// A cover for a game. With no box art on the TV, each cover is drawn: the console's colour,
    /// a sheen, and the game's name.
    pub fn cover(&mut self, g: &Game, r: Rect, radius: f32, dim: f32) {
        let c = g.system.colour();
        let top = [c[0] * 0.78, c[1] * 0.78, c[2] * 0.78, dim];
        let bottom = [c[0] * 0.22, c[1] * 0.22, c[2] * 0.26, dim];
        self.d.rect(r.x, r.y, r.w, r.h, radius, &Shape::vertical(top, bottom));
        self.d.rect(r.x, r.y, r.w, r.h * 0.55, radius, &Shape::gradient(white(0.16 * dim), white(0.0)));
        if let Some(t) = self.images.console(g.system) {
            let s = r.w * 0.42;
            self.d.image(t, r.x + r.w - s - r.w * 0.06, r.y + r.h - s - r.h * 0.04, s, s, 0.0, [1.0, 1.0, 1.0, 0.85 * dim], false);
        }
        let size = (r.w * 0.13).clamp(16.0, 44.0);
        let lines = self.d.wrap(&g.title, size, BOLD, r.w * 0.84);
        let mut y = r.y + r.w * 0.09;
        for l in lines.iter().take(3) {
            self.d.text_fit(l, r.x + r.w * 0.08, y, size, BOLD, white(0.96 * dim), r.w * 0.84);
            y += size * 1.12;
        }
        let tag = g.system.short();
        self.d.text(tag, r.x + r.w * 0.08, r.y + r.h - size * 0.9 - r.h * 0.06, size * 0.62, BOLD, white(0.7 * dim), 1.5);
    }

    /// A game in a grid: its cover in a glowing frame, its name, one grey line.
    pub fn game_card(&mut self, key: &str, g: &Game, x: f32, y: f32, w: f32, meta: &str) -> Rect {
        let iid = id(key);
        let cover = Rect::new(x, y, w, w);
        self.add(iid, cover, Kind::Normal);
        let amt = self.focus_amt(iid);
        let r = if self.th.grow { cover.grow(1.0 + 0.06 * amt) } else { cover };
        let radius = self.th.corner * 0.18 * w.max(40.0);
        match self.th.cover_glow(g.system) {
            Some(gc) => {
                self.d.glow_ring(r.x, r.y, r.w, r.h, radius, 5.0, 7.0, with_alpha(gc, 0.75));
                self.cover(g, r, radius, 1.0);
                self.d.rect(r.x, r.y, r.w, r.h, radius, &Shape::fill([0.0; 4]).border(2.0, with_alpha(gc, 0.95)));
            }
            None => {
                self.cover(g, r, radius, 1.0);
                self.d.rect(r.x, r.y, r.w, r.h, radius, &Shape::fill([0.0; 4]).border(1.0, white(0.14)));
            }
        }
        self.focus_ring(r.grow(1.0 + 8.0 / r.w), radius + 4.0, amt);
        if g.favourite {
            self.d.text("★", r.x + r.w - 40.0, r.y + 10.0, 30.0, BOLD, self.th.first, 0.0);
        }
        if !self.th.hide_titles {
            let ty = y + w + 16.0 + 8.0 * amt;
            self.d.text_fit(&g.title, x, ty, 26.0, MEDIUM, white(1.0), w);
            self.d.text_fit(meta, x, ty + 34.0, 21.0, REGULAR, white(0.5), w);
        }
        cover
    }

    /// A console in the SYSTEMS tray.
    pub fn console_tile(&mut self, key: &str, sys: Option<System>, count: usize, x: f32, y: f32, picked: bool) -> Rect {
        let iid = id(key);
        let r = Rect::new(x, y, 200.0, 220.0);
        self.add(iid, r, Kind::Normal);
        let amt = self.focus_amt(iid);
        let rr = if self.th.grow { r.grow(1.0 + 0.05 * amt) } else { r };
        let fill = if picked { with_alpha(self.th.first, 0.07) } else { white(0.055) };
        let mut s = Shape::fill(fill).border(1.0, white(0.09));
        if picked {
            self.d.glow_ring(rr.x, rr.y, rr.w, rr.h, 22.0, 5.0, 8.0, with_alpha(self.th.second, 0.85 * self.th.pulse_amt()));
            s = s.border(2.5, self.th.first);
            if self.th.light {
                s = s.light(self.th.light_angle, 3.0, self.th.first, self.th.second);
            }
        }
        self.d.rect(rr.x, rr.y, rr.w, rr.h, 22.0, &s);
        if !picked {
            self.focus_ring(rr, 22.0, amt);
        } else if amt > 0.01 {
            self.d.rect(rr.x - 5.0, rr.y - 5.0, rr.w + 10.0, rr.h + 10.0, 26.0, &Shape::fill([0.0; 4]).border(3.0, white(0.85 * amt)));
        }
        let pic = 140.0;
        match sys.and_then(|s| self.images.console(s)) {
            Some(t) => self.d.image(t, rr.cx() - pic * 0.5, rr.y + 14.0, pic, pic, 0.0, [1.0; 4], false),
            None => {
                // "All": the three consoles' colours
                for (i, s) in System::ALL.iter().enumerate() {
                    let c = s.colour();
                    self.d.rect(rr.cx() - 60.0 + i as f32 * 36.0, rr.y + 40.0 + i as f32 * 10.0, 56.0, 76.0, 10.0, &Shape::vertical(c, with_alpha(c, 0.4)));
                }
            }
        }
        let name = match sys {
            Some(System::Gb) => "GAME BOY",
            Some(System::Gbc) => "COLOR",
            Some(System::Gba) => "ADVANCE",
            None => "ALL",
        };
        self.d.text_center(name, rr.cx(), rr.y + 160.0, 24.0, BOLD, white(0.95));
        // count badge
        let n = count.to_string();
        let bw = self.d.measure(&n, 20.0, BOLD, 0.0) + 22.0;
        self.d.rect(rr.x + rr.w - bw - 10.0, rr.y + 10.0, bw, 32.0, 16.0, &Shape::fill(rgba(0x545454, 0.9)));
        self.d.text_center(&n, rr.x + rr.w - bw * 0.5 - 10.0, rr.y + 16.0, 20.0, BOLD, white(1.0));
        r
    }

    pub fn section_title(&mut self, s: &str, x: f32, y: f32) {
        self.d.text(s, x, y, 26.0, MEDIUM, white(0.6), 1.6);
    }

    pub fn heading(&mut self, s: &str, x: f32, y: f32) {
        self.d.text(s, x, y, 36.0, BOLD, white(1.0), 0.0);
    }

    // ---- settings rows ----

    fn row_base(&mut self, key: &str, r: Rect, kind: Kind) -> f32 {
        let iid = id(key);
        self.add(iid, r, kind);
        let amt = self.focus_amt(iid);
        if amt > 0.01 {
            self.d.rect(r.x, r.y, r.w, r.h, 16.0, &Shape::fill(white(0.07 * amt)));
            self.d.rect(r.x, r.y, r.w, r.h, 16.0, &Shape::fill([0.0; 4]).border(2.5, with_alpha(self.th.first, amt)));
        }
        amt
    }

    pub fn toggle_row(&mut self, key: &str, r: Rect, label: &str, on: bool) {
        self.row_base(key, r, Kind::Normal);
        self.d.text_fit(label, r.x + 24.0, r.y + (r.h - 28.0) * 0.5, 28.0, REGULAR, white(0.95), r.w - 180.0);
        let (tw, thh) = (86.0, 50.0);
        let tx = r.x + r.w - tw - 24.0;
        let ty = r.y + (r.h - thh) * 0.5;
        if on {
            self.d.rect(tx, ty, tw, thh, thh * 0.5, &self.th.accent_fill());
        } else {
            self.d.rect(tx, ty, tw, thh, thh * 0.5, &Shape::fill(white(0.16)));
        }
        let kx = if on { tx + tw - thh + 4.0 } else { tx + 4.0 };
        self.d.glow(kx, ty + 6.0, thh - 8.0, thh - 8.0, (thh - 8.0) * 0.5, 4.0, [0.0, 0.0, 0.0, 0.35]);
        self.d.rect(kx, ty + 4.0, thh - 8.0, thh - 8.0, (thh - 8.0) * 0.5, &Shape::fill(white(1.0)));
    }

    /// A choice: left and right step through the values. `swatch` draws a colour chip.
    pub fn choice_row(&mut self, key: &str, r: Rect, label: &str, value: &str, swatch: Option<(Rgba, Rgba)>) {
        let amt = self.row_base(key, r, Kind::Adjust);
        self.d.text_fit(label, r.x + 24.0, r.y + (r.h - 28.0) * 0.5, 28.0, REGULAR, white(0.95), r.w * 0.55);
        let vw = self.d.measure(value, 28.0, MEDIUM, 0.0);
        let right = r.x + r.w - 24.0;
        let arrows = amt > 0.01;
        let vx = right - vw - if arrows { 40.0 } else { 0.0 };
        let mut sx = vx;
        if let Some((a, b)) = swatch {
            sx -= 56.0;
            self.d.rect(sx, r.y + (r.h - 40.0) * 0.5, 40.0, 40.0, 20.0, &Shape::gradient(a, b).border(1.0, white(0.3)));
        }
        if arrows {
            self.d.text("‹", sx - 34.0, r.y + (r.h - 34.0) * 0.5 - 2.0, 34.0, BOLD, with_alpha(self.th.first, amt), 0.0);
            self.d.text("›", right - 20.0, r.y + (r.h - 34.0) * 0.5 - 2.0, 34.0, BOLD, with_alpha(self.th.first, amt), 0.0);
        }
        self.d.text(value, vx, r.y + (r.h - 28.0) * 0.5, 28.0, MEDIUM, self.th.first, 0.0);
    }

    pub fn slider_row(&mut self, key: &str, r: Rect, label: &str, frac: f32, value: &str) {
        self.row_base(key, r, Kind::Adjust);
        self.d.text_fit(label, r.x + 24.0, r.y + (r.h - 28.0) * 0.5, 28.0, REGULAR, white(0.95), r.w * 0.4);
        let bw = r.w * 0.34;
        let bx = r.x + r.w - bw - 150.0;
        let by = r.y + r.h * 0.5 - 5.0;
        self.d.rect(bx, by, bw, 10.0, 5.0, &Shape::fill(white(0.16)));
        self.d.rect(bx, by, (bw * frac.clamp(0.0, 1.0)).max(10.0), 10.0, 5.0, &self.th.accent_fill());
        let kx = bx + bw * frac.clamp(0.0, 1.0) - 15.0;
        self.d.rect(kx, by - 10.0, 30.0, 30.0, 15.0, &Shape::fill(white(1.0)));
        self.d.text(value, r.x + r.w - 124.0, r.y + (r.h - 28.0) * 0.5, 28.0, MEDIUM, self.th.first, 0.0);
    }

    pub fn action_row(&mut self, key: &str, r: Rect, label: &str, hint: &str, danger: bool) {
        self.row_base(key, r, Kind::Normal);
        let c = if danger { self.th.danger } else { white(0.95) };
        self.d.text_fit(label, r.x + 24.0, r.y + (r.h - 28.0) * 0.5, 28.0, MEDIUM, c, r.w * 0.6);
        if !hint.is_empty() {
            let w = self.d.measure(hint, 24.0, REGULAR, 0.0);
            self.d.text(hint, r.x + r.w - w - 24.0, r.y + (r.h - 24.0) * 0.5, 24.0, REGULAR, white(0.5), 0.0);
        }
    }

    /// A foldable card's heading bar. Returns its height.
    pub fn fold_head(&mut self, key: &str, r: Rect, title: &str, subtitle: &str, open: bool) {
        let iid = id(key);
        self.add(iid, r, Kind::Normal);
        let amt = self.focus_amt(iid);
        self.d.rect(r.x, r.y, r.w, r.h, 20.0, &Shape::fill(white(0.055 + 0.05 * amt)).border(1.0, white(0.09)));
        if amt > 0.01 {
            self.focus_ring(r, 20.0, amt);
        }
        // icon tile in the accent
        self.d.rect(r.x + 22.0, r.y + (r.h - 52.0) * 0.5, 52.0, 52.0, 14.0, &Shape::fill(with_alpha(self.th.first, 0.16)));
        let initial: String = title.chars().take(1).collect();
        self.d.text_center(&initial, r.x + 48.0, r.y + (r.h - 30.0) * 0.5, 30.0, BOLD, self.th.first);
        self.d.text(title, r.x + 96.0, r.y + if subtitle.is_empty() { (r.h - 30.0) * 0.5 } else { r.h * 0.5 - 34.0 }, 30.0, BOLD, white(1.0), 0.0);
        if !subtitle.is_empty() {
            self.d.text_fit(subtitle, r.x + 96.0, r.y + r.h * 0.5 + 4.0, 22.0, REGULAR, white(0.5), r.w - 200.0);
        }
        // the TV's font has no down-pointing chevron: minus when open, plus when closed
        let sign = if open { "−" } else { "+" };
        self.d.text_center(sign, r.x + r.w - 46.0, r.y + (r.h - 40.0) * 0.5, 40.0, BOLD, white(0.6));
    }
}

/// Short hints: which remote keys do what here.
#[allow(dead_code)]
pub fn hints(d: &mut Draw, items: &[(&str, &str)], y: f32) {
    let mut x = 80.0;
    for (k, v) in items {
        let kw = d.measure(k, 20.0, BOLD, 0.0) + 22.0;
        d.rect(x, y, kw, 34.0, 10.0, &Shape::fill(white(0.12)).border(1.0, white(0.16)));
        d.text_center(k, x + kw * 0.5, y + 7.0, 20.0, BOLD, white(0.9));
        x += kw + 10.0;
        let w = d.text(v, x, y + 5.0, 22.0, REGULAR, white(0.6), 0.0);
        x += w + 32.0;
    }
}

pub fn colour_of(hex: &str) -> Rgba {
    c4(hex_rgb(hex))
}

pub fn pair_of(hex: &str) -> (Rgba, Rgba) {
    let st = crate::config::stops(hex);
    let a = colour_of(&st[0]);
    let b = if st.len() > 1 { colour_of(&st[1]) } else { a };
    (a, b)
}
