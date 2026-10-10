//! Where the two screens of a DS or a 3DS go, and where a finger on the glass lands in the guest.
//!
//! Pure arithmetic, no GPU. The renderer turns a [`ScreenPlacement`] list into the instanced
//! `frame_blit.wgsl` uniforms, and the touch mapping walks the SAME list, so what is drawn and
//! what a tap hits cannot disagree. Everything is in fractions:
//!
//! - `dest` is a fraction of the target view (the phone's Metal view or the TV), origin top left.
//! - `src` is a fraction of the core's ONE framebuffer, origin top left.
//!
//! Both cores hand over one framebuffer with the two screens stacked inside it, and neither core's
//! own layout option is answered by this host, so the stacked default is what arrives:
//!
//! - melonDS: 256x192 over 256x192, a 256x384 frame. Touch screen is the lower half.
//! - Azahar: 400x240 over 320x240 centred, a 400x480 frame. Touch screen is `x 40..360` of the
//!   lower half. The widths differ, which is why every layout below is computed from each screen's
//!   own pixel size rather than from halves of the frame.
//!
//! Upscaled internal resolutions keep the same proportions, so fractions survive them.
//!
//! This is engine logic on purpose. Android gets the same layouts and the same touch maths by
//! calling the same functions.

use super::SkinHole;
use super::ScaleMode;

/// A rectangle as fractions, origin top left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FracRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl FracRect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub const WHOLE: FracRect = FracRect::new(0.0, 0.0, 1.0, 1.0);

    pub fn is_empty(&self) -> bool {
        !(self.w > 0.0 && self.h > 0.0)
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }

    pub fn centre(&self) -> (f32, f32) {
        (self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    /// The overlap of two rects, or `None` when they do not overlap by any area.
    pub fn intersect(&self, other: &FracRect) -> Option<FracRect> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = (self.x + self.w).min(other.x + other.w);
        let y1 = (self.y + self.h).min(other.y + other.h);
        if x1 - x0 > 1e-6 && y1 - y0 > 1e-6 {
            Some(FracRect::new(x0, y0, x1 - x0, y1 - y0))
        } else {
            None
        }
    }
}

/// One of the two guest screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestScreen {
    Top,
    /// The touch screen on both the DS and the 3DS.
    Bottom,
}

impl GuestScreen {
    fn other(self) -> Self {
        match self {
            GuestScreen::Top => GuestScreen::Bottom,
            GuestScreen::Bottom => GuestScreen::Top,
        }
    }
}

/// Where each guest screen sits inside the core's single framebuffer, as fractions of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DualScreenGeometry {
    pub top: FracRect,
    pub bottom: FracRect,
}

impl DualScreenGeometry {
    /// melonDS, 256x384: two 256x192 screens stacked.
    pub const NINTENDO_DS: DualScreenGeometry = DualScreenGeometry {
        top: FracRect::new(0.0, 0.0, 1.0, 0.5),
        bottom: FracRect::new(0.0, 0.5, 1.0, 0.5),
    };

    /// Azahar, 400x480: the 400x240 top screen over the 320x240 bottom screen, centred, so the
    /// bottom one starts 40 pixels in.
    pub const NINTENDO_3DS: DualScreenGeometry = DualScreenGeometry {
        top: FracRect::new(0.0, 0.0, 1.0, 0.5),
        bottom: FracRect::new(0.1, 0.5, 0.8, 0.5),
    };

    /// The geometry for a system id, or `None` for every one-screen system.
    ///
    /// Takes the ids the app's catalogue and the core descriptors already use.
    pub fn for_system(system: &str) -> Option<Self> {
        match system {
            "ds" | "nds" => Some(Self::NINTENDO_DS),
            "n3ds" | "3ds" => Some(Self::NINTENDO_3DS),
            _ => None,
        }
    }

    pub fn src(&self, screen: GuestScreen) -> FracRect {
        match screen {
            GuestScreen::Top => self.top,
            GuestScreen::Bottom => self.bottom,
        }
    }

    /// A screen's size in framebuffer pixels.
    fn pixels(&self, screen: GuestScreen, fb_w: f32, fb_h: f32) -> (f32, f32) {
        let src = self.src(screen);
        ((src.w * fb_w).max(1.0), (src.h * fb_h).max(1.0))
    }
}

/// The arrangements a user can pick for a two-screen system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DualLayout {
    /// Top over bottom, the hardware's own arrangement. The default, and drawn exactly as the
    /// single-quad path draws the whole frame.
    #[default]
    Stacked,
    /// Top on the left, bottom on the right, each at full size.
    SideBySide,
    /// The top screen big, the bottom one at [`SMALL_SCREEN_SCALE`].
    BigTop,
    /// The bottom screen big, the top one at [`SMALL_SCREEN_SCALE`].
    BigBottom,
    /// Only the top screen.
    TopOnly,
    /// Only the bottom screen.
    BottomOnly,
}

impl DualLayout {
    pub const ALL: [DualLayout; 6] = [
        DualLayout::Stacked,
        DualLayout::SideBySide,
        DualLayout::BigTop,
        DualLayout::BigBottom,
        DualLayout::TopOnly,
        DualLayout::BottomOnly,
    ];
}

/// How big the small screen is in the big + small layouts, relative to its own pixels.
pub const SMALL_SCREEN_SCALE: f32 = 0.5;

/// Everything a user chooses about the two screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DualScreenConfig {
    pub layout: DualLayout,
    /// The one-tap swap. Exchanges which guest screen goes in which place: in Stacked the bottom
    /// screen is drawn on top, in the big + small layouts the other screen becomes the big one,
    /// in the one-screen layouts the other screen is shown, and in a skin the two holes trade
    /// pictures.
    pub swapped: bool,
    /// The big + small layouts put the small screen BESIDE the big one in landscape and BELOW it
    /// in portrait. The host says which it is, because only the host knows the window.
    pub landscape: bool,
    /// With an external display connected: the touch screen stays on the phone and the TV shows
    /// the other screen. Off: the TV shows the whole layout and the phone shows only controls.
    pub touch_on_phone: bool,
}

impl DualScreenConfig {
    /// True for the configuration whose drawing must stay byte-identical to the single quad.
    pub fn is_hardware_default(&self) -> bool {
        self.layout == DualLayout::Stacked && !self.swapped
    }

    /// Which guest screen goes in the first slot (above or left). Swap exchanges it.
    fn first(&self) -> GuestScreen {
        if self.swapped {
            GuestScreen::Bottom
        } else {
            GuestScreen::Top
        }
    }

    /// Which guest screen the external display shows while the touch screen stays on the phone.
    /// The top screen, or the bottom one when swapped.
    pub fn external_screen(&self) -> GuestScreen {
        self.first()
    }
}

/// One instance of the composite pass: a crop of the framebuffer drawn into a rect of the view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenPlacement {
    /// Fraction of the target view, origin top left.
    pub dest: FracRect,
    /// Fraction of the framebuffer, origin top left.
    pub src: FracRect,
}

/// Which surface a plan is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetRole {
    /// The phone with no external display: everything.
    Phone,
    /// The TV.
    External,
    /// The phone while a TV is connected: only the touch screen, or nothing.
    PhoneCompanion,
}

/// Places screens in a box of `w x h` pixels at the origin, then fits that box into the target.
struct Arrangement {
    /// `(screen, x, y, w, h)` in arrangement pixels.
    items: Vec<(GuestScreen, f32, f32, f32, f32)>,
    width: f32,
    height: f32,
}

fn arrange(geometry: &DualScreenGeometry, config: &DualScreenConfig, fb_w: f32, fb_h: f32) -> Arrangement {
    let a = config.first();
    let b = a.other();
    let (aw, ah) = geometry.pixels(a, fb_w, fb_h);
    let (bw, bh) = geometry.pixels(b, fb_w, fb_h);

    let single = |screen: GuestScreen| {
        let (w, h) = geometry.pixels(screen, fb_w, fb_h);
        Arrangement {
            items: vec![(screen, 0.0, 0.0, w, h)],
            width: w,
            height: h,
        }
    };

    match config.layout {
        DualLayout::Stacked => {
            let width = aw.max(bw);
            Arrangement {
                items: vec![
                    (a, (width - aw) * 0.5, 0.0, aw, ah),
                    (b, (width - bw) * 0.5, ah, bw, bh),
                ],
                width,
                height: ah + bh,
            }
        }
        DualLayout::SideBySide => side_by_side(a, (aw, ah), b, (bw, bh)),
        DualLayout::BigTop | DualLayout::BigBottom => {
            // The big screen by identity. Swap exchanges identities everywhere, so it also
            // exchanges which screen is big.
            let big = match (config.layout, config.swapped) {
                (DualLayout::BigTop, false) | (DualLayout::BigBottom, true) => GuestScreen::Top,
                _ => GuestScreen::Bottom,
            };
            let scale = |screen: GuestScreen, w: f32, h: f32| {
                if screen == big {
                    (w, h)
                } else {
                    (w * SMALL_SCREEN_SCALE, h * SMALL_SCREEN_SCALE)
                }
            };
            let (aw, ah) = scale(a, aw, ah);
            let (bw, bh) = scale(b, bw, bh);
            if config.landscape {
                side_by_side(a, (aw, ah), b, (bw, bh))
            } else {
                let width = aw.max(bw);
                Arrangement {
                    items: vec![
                        (a, (width - aw) * 0.5, 0.0, aw, ah),
                        (b, (width - bw) * 0.5, ah, bw, bh),
                    ],
                    width,
                    height: ah + bh,
                }
            }
        }
        DualLayout::TopOnly => single(if config.swapped { GuestScreen::Bottom } else { GuestScreen::Top }),
        DualLayout::BottomOnly => single(if config.swapped { GuestScreen::Top } else { GuestScreen::Bottom }),
    }
}

fn side_by_side(a: GuestScreen, (aw, ah): (f32, f32), b: GuestScreen, (bw, bh): (f32, f32)) -> Arrangement {
    let height = ah.max(bh);
    Arrangement {
        items: vec![
            (a, 0.0, (height - ah) * 0.5, aw, ah),
            (b, aw, (height - bh) * 0.5, bw, bh),
        ],
        width: aw + bw,
        height,
    }
}

/// Scale factors `(kx, ky)` and the top-left offset that fit a `w x h` box into the target.
fn fit_box(w: f32, h: f32, target_w: f32, target_h: f32, mode: ScaleMode) -> (f32, f32, f32, f32) {
    if w <= 0.0 || h <= 0.0 || target_w <= 0.0 || target_h <= 0.0 {
        return (1.0, 1.0, 0.0, 0.0);
    }
    let fit = (target_w / w).min(target_h / h);
    let (kx, ky) = match mode {
        ScaleMode::Stretch => (target_w / w, target_h / h),
        ScaleMode::AspectFit => (fit, fit),
        ScaleMode::IntegerScale => {
            let whole = fit.floor();
            if whole >= 1.0 {
                (whole, whole)
            } else {
                (fit, fit)
            }
        }
    };
    let ox = (target_w - w * kx) * 0.5;
    let oy = (target_h - h * ky) * 0.5;
    (kx, ky, ox, oy)
}

fn fitted(
    geometry: &DualScreenGeometry,
    arrangement: Arrangement,
    target_w: f32,
    target_h: f32,
    mode: ScaleMode,
) -> Vec<ScreenPlacement> {
    let (kx, ky, ox, oy) = fit_box(arrangement.width, arrangement.height, target_w, target_h, mode);
    arrangement
        .items
        .into_iter()
        .map(|(screen, x, y, w, h)| ScreenPlacement {
            dest: FracRect::new(
                (ox + x * kx) / target_w,
                (oy + y * ky) / target_h,
                (w * kx) / target_w,
                (h * ky) / target_h,
            ),
            src: geometry.src(screen),
        })
        .collect()
}

/// The placements for a two-screen system on one target, in pixels of that target.
///
/// `target_w`/`target_h` only matter as a shape, so points and pixels give the same answer.
#[allow(clippy::too_many_arguments)]
pub fn dual_placements(
    geometry: &DualScreenGeometry,
    config: &DualScreenConfig,
    role: TargetRole,
    fb_w: f32,
    fb_h: f32,
    target_w: f32,
    target_h: f32,
    mode: ScaleMode,
) -> Vec<ScreenPlacement> {
    let fb_w = fb_w.max(1.0);
    let fb_h = fb_h.max(1.0);
    let target_w = target_w.max(1.0);
    let target_h = target_h.max(1.0);
    let effective = match role {
        TargetRole::Phone => *config,
        TargetRole::External if config.touch_on_phone => single_screen_config(config, config.external_screen()),
        TargetRole::External => *config,
        TargetRole::PhoneCompanion if config.touch_on_phone => {
            single_screen_config(config, config.external_screen().other())
        }
        TargetRole::PhoneCompanion => return Vec::new(),
    };
    fitted(geometry, arrange(geometry, &effective, fb_w, fb_h), target_w, target_h, mode)
}

/// A config that shows exactly one named screen.
fn single_screen_config(config: &DualScreenConfig, screen: GuestScreen) -> DualScreenConfig {
    DualScreenConfig {
        layout: DualLayout::TopOnly,
        swapped: screen == GuestScreen::Bottom,
        ..*config
    }
}

/// The width over height of the arrangement the phone shows, so the host can size the picture
/// area to it the way it sizes it to a one-screen core's aspect.
pub fn arrangement_aspect(
    geometry: &DualScreenGeometry,
    config: &DualScreenConfig,
    role: TargetRole,
    fb_w: f32,
    fb_h: f32,
) -> Option<f32> {
    let effective = match role {
        TargetRole::Phone => *config,
        TargetRole::External if config.touch_on_phone => single_screen_config(config, config.external_screen()),
        TargetRole::External => *config,
        TargetRole::PhoneCompanion if config.touch_on_phone => {
            single_screen_config(config, config.external_screen().other())
        }
        TargetRole::PhoneCompanion => return None,
    };
    let arrangement = arrange(geometry, &effective, fb_w.max(1.0), fb_h.max(1.0));
    (arrangement.height > 0.0).then(|| arrangement.width / arrangement.height)
}

/// The one-quad placement every one-screen system uses, from the renderer's clip-space fit.
///
/// `fitted` is the clip-space half-extent `compute_scale` returns, centred.
pub fn single_placement(fitted: [f32; 2]) -> ScreenPlacement {
    let [sx, sy] = fitted;
    ScreenPlacement {
        dest: FracRect::new(0.5 - sx * 0.5, 0.5 - sy * 0.5, sx, sy),
        src: FracRect::WHOLE,
    }
}

/// A skin hole's crop as a fraction of the framebuffer. An uncropped hole is the whole frame.
pub fn hole_src(hole: &SkinHole, fb_w: f32, fb_h: f32) -> FracRect {
    if hole.src_w <= 0.0 || hole.src_h <= 0.0 || fb_w <= 0.0 || fb_h <= 0.0 {
        return FracRect::WHOLE;
    }
    FracRect::new(hole.src_x / fb_w, hole.src_y / fb_h, hole.src_w / fb_w, hole.src_h / fb_h)
}

/// Which guest screen a cropped hole shows, by where its crop's centre falls.
fn hole_screen(hole: &SkinHole, geometry: &DualScreenGeometry, fb_w: f32, fb_h: f32) -> Option<GuestScreen> {
    if hole.src_w <= 0.0 || hole.src_h <= 0.0 {
        return None;
    }
    let (cx, cy) = hole_src(hole, fb_w, fb_h).centre();
    if geometry.bottom.contains(cx, cy) {
        Some(GuestScreen::Bottom)
    } else if geometry.top.contains(cx, cy) {
        Some(GuestScreen::Top)
    } else if cy >= 0.5 {
        Some(GuestScreen::Bottom)
    } else {
        Some(GuestScreen::Top)
    }
}

/// The skin holes with the top and bottom pictures exchanged, for the swap.
///
/// Only cropped holes can be exchanged: an uncropped hole already shows the whole frame. The
/// first top hole trades crops with the first bottom hole; the destinations (the holes) stay
/// where the skin drew them. Returns the holes unchanged when there is no such pair.
pub fn swap_skin_holes(holes: &[SkinHole], geometry: &DualScreenGeometry, fb_w: f32, fb_h: f32) -> Vec<SkinHole> {
    let mut out = holes.to_vec();
    let top = holes.iter().position(|h| hole_screen(h, geometry, fb_w, fb_h) == Some(GuestScreen::Top));
    let bottom = holes.iter().position(|h| hole_screen(h, geometry, fb_w, fb_h) == Some(GuestScreen::Bottom));
    if let (Some(t), Some(b)) = (top, bottom) {
        let (ts, bs) = (holes[t], holes[b]);
        out[t].src_x = bs.src_x;
        out[t].src_y = bs.src_y;
        out[t].src_w = bs.src_w;
        out[t].src_h = bs.src_h;
        out[b].src_x = ts.src_x;
        out[b].src_y = ts.src_y;
        out[b].src_w = ts.src_w;
        out[b].src_h = ts.src_h;
    }
    out
}

/// Whether a skin has a top hole and a bottom hole to swap between.
pub fn skin_can_swap(holes: &[SkinHole], geometry: &DualScreenGeometry, fb_w: f32, fb_h: f32) -> bool {
    let has = |s| holes.iter().any(|h| hole_screen(h, geometry, fb_w, fb_h) == Some(s));
    has(GuestScreen::Top) && has(GuestScreen::Bottom)
}

/// Skin holes as placements, so touch can walk them like any other layout.
pub fn hole_placements(holes: &[SkinHole], fb_w: f32, fb_h: f32) -> Vec<ScreenPlacement> {
    holes
        .iter()
        .map(|hole| ScreenPlacement {
            dest: FracRect::new(hole.dest_x, hole.dest_y, hole.dest_w, hole.dest_h),
            src: hole_src(hole, fb_w, fb_h),
        })
        .collect()
}

/// The holes that show (part of) the touch screen, for the phone while a TV has the rest.
pub fn touch_holes(holes: &[SkinHole], geometry: &DualScreenGeometry, fb_w: f32, fb_h: f32) -> Vec<SkinHole> {
    holes
        .iter()
        .copied()
        .filter(|h| hole_src(h, fb_w, fb_h).intersect(&geometry.bottom).is_some() && h.src_w > 0.0)
        .collect()
}

/// Where the touch screen was drawn, and which crop of the framebuffer that rect shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TouchRegion {
    /// Fraction of the view.
    pub dest: FracRect,
    /// Fraction of the framebuffer. Always inside the guest touch screen.
    pub src: FracRect,
}

/// The part of each placement that shows the guest touch screen, first match wins.
///
/// Works for every kind of placement: a layout slot showing exactly the bottom screen, a skin hole
/// whose crop includes margins around it, and the single quad that shows the whole stacked frame.
/// In each case it is the intersection of the placement's crop with the guest touch screen,
/// carried back into the view.
pub fn touch_region(placements: &[ScreenPlacement], geometry: &DualScreenGeometry) -> Option<TouchRegion> {
    placements.iter().find_map(|placement| {
        if placement.dest.is_empty() || placement.src.is_empty() {
            return None;
        }
        let src = placement.src.intersect(&geometry.bottom)?;
        let sx = placement.dest.w / placement.src.w;
        let sy = placement.dest.h / placement.src.h;
        Some(TouchRegion {
            dest: FracRect::new(
                placement.dest.x + (src.x - placement.src.x) * sx,
                placement.dest.y + (src.y - placement.src.y) * sy,
                src.w * sx,
                src.h * sy,
            ),
            src,
        })
    })
}

/// A point in the view, as a fraction, to a point in the framebuffer, as a fraction.
///
/// `None` when the point is not on the touch screen and `clamp` is false. With `clamp`, a finger
/// that has slid a little off the edge reads as the edge, which is what a drag that started on
/// the screen wants.
pub fn map_touch(region: &TouchRegion, view_x: f32, view_y: f32, clamp: bool) -> Option<(f32, f32)> {
    let dest = region.dest;
    // A NaN coordinate passes no range check and survives `clamp`, so it would come out as a NaN
    // guest point; there is no edge to read it as.
    if dest.is_empty() || !view_x.is_finite() || !view_y.is_finite() {
        return None;
    }
    let mut u = (view_x - dest.x) / dest.w;
    let mut v = (view_y - dest.y) / dest.h;
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        if !clamp {
            return None;
        }
        u = u.clamp(0.0, 1.0);
        v = v.clamp(0.0, 1.0);
    }
    Some((region.src.x + u * region.src.w, region.src.y + v * region.src.h))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-4;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < EPS
    }

    fn rect_close(a: FracRect, b: FracRect) -> bool {
        close(a.x, b.x) && close(a.y, b.y) && close(a.w, b.w) && close(a.h, b.h)
    }

    fn cfg(layout: DualLayout, swapped: bool) -> DualScreenConfig {
        DualScreenConfig { layout, swapped, landscape: false, touch_on_phone: true }
    }

    fn ds(layout: DualLayout, swapped: bool, tw: f32, th: f32) -> Vec<ScreenPlacement> {
        dual_placements(&DualScreenGeometry::NINTENDO_DS, &cfg(layout, swapped), TargetRole::Phone,
                        256.0, 384.0, tw, th, ScaleMode::AspectFit)
    }

    fn n3ds(layout: DualLayout, swapped: bool, tw: f32, th: f32) -> Vec<ScreenPlacement> {
        dual_placements(&DualScreenGeometry::NINTENDO_3DS, &cfg(layout, swapped), TargetRole::Phone,
                        400.0, 480.0, tw, th, ScaleMode::AspectFit)
    }

    #[test]
    fn ds_stacked_fills_a_view_of_its_own_shape_exactly_like_the_single_quad() {
        let p = ds(DualLayout::Stacked, false, 256.0, 384.0);
        assert_eq!(p.len(), 2);
        assert!(rect_close(p[0].dest, FracRect::new(0.0, 0.0, 1.0, 0.5)));
        assert!(rect_close(p[1].dest, FracRect::new(0.0, 0.5, 1.0, 0.5)));
        // Each slot samples exactly the screen it shows, so together they are the whole frame.
        assert_eq!(p[0].src, DualScreenGeometry::NINTENDO_DS.top);
        assert_eq!(p[1].src, DualScreenGeometry::NINTENDO_DS.bottom);
    }

    #[test]
    fn ds_stacked_swapped_puts_the_touch_screen_on_top() {
        let p = ds(DualLayout::Stacked, true, 256.0, 384.0);
        assert_eq!(p[0].src, DualScreenGeometry::NINTENDO_DS.bottom);
        assert!(close(p[0].dest.y, 0.0));
        assert_eq!(p[1].src, DualScreenGeometry::NINTENDO_DS.top);
        assert!(close(p[1].dest.y, 0.5));
    }

    #[test]
    fn ds_side_by_side_on_a_wide_view() {
        // 512x192 arrangement in a 1024x384 view: exactly 2x, no letterbox.
        let p = ds(DualLayout::SideBySide, false, 1024.0, 384.0);
        assert!(rect_close(p[0].dest, FracRect::new(0.0, 0.0, 0.5, 1.0)));
        assert!(rect_close(p[1].dest, FracRect::new(0.5, 0.0, 0.5, 1.0)));
        assert_eq!(p[0].src, DualScreenGeometry::NINTENDO_DS.top);
    }

    #[test]
    fn ds_big_top_portrait_puts_a_half_size_bottom_below() {
        // 256 x (192 + 96) arrangement in a 256x288 view.
        let p = ds(DualLayout::BigTop, false, 256.0, 288.0);
        assert!(rect_close(p[0].dest, FracRect::new(0.0, 0.0, 1.0, 192.0 / 288.0)));
        assert!(rect_close(p[1].dest, FracRect::new(0.25, 192.0 / 288.0, 0.5, 96.0 / 288.0)));
        assert_eq!(p[0].src, DualScreenGeometry::NINTENDO_DS.top);
        assert_eq!(p[1].src, DualScreenGeometry::NINTENDO_DS.bottom);
    }

    #[test]
    fn swap_makes_the_other_screen_big() {
        let normal = ds(DualLayout::BigTop, false, 256.0, 288.0);
        let swapped = ds(DualLayout::BigTop, true, 256.0, 288.0);
        // The big slot (first, full width) now shows the bottom screen.
        assert!(close(swapped[0].dest.w, 1.0));
        assert_eq!(swapped[0].src, DualScreenGeometry::NINTENDO_DS.bottom);
        assert_eq!(swapped[1].src, DualScreenGeometry::NINTENDO_DS.top);
        assert!(close(swapped[1].dest.w, normal[1].dest.w));
    }

    #[test]
    fn ds_big_bottom_puts_the_small_top_screen_above() {
        let p = ds(DualLayout::BigBottom, false, 256.0, 288.0);
        assert_eq!(p[0].src, DualScreenGeometry::NINTENDO_DS.top);
        assert!(rect_close(p[0].dest, FracRect::new(0.25, 0.0, 0.5, 96.0 / 288.0)));
        assert_eq!(p[1].src, DualScreenGeometry::NINTENDO_DS.bottom);
        assert!(rect_close(p[1].dest, FracRect::new(0.0, 96.0 / 288.0, 1.0, 192.0 / 288.0)));
    }

    #[test]
    fn big_small_goes_side_by_side_in_landscape() {
        let config = DualScreenConfig { layout: DualLayout::BigTop, swapped: false, landscape: true, touch_on_phone: true };
        // 256 + 128 wide, 192 tall.
        let p = dual_placements(&DualScreenGeometry::NINTENDO_DS, &config, TargetRole::Phone,
                                256.0, 384.0, 384.0, 192.0, ScaleMode::AspectFit);
        assert!(rect_close(p[0].dest, FracRect::new(0.0, 0.0, 256.0 / 384.0, 1.0)));
        assert!(rect_close(p[1].dest, FracRect::new(256.0 / 384.0, 0.25, 128.0 / 384.0, 0.5)));
    }

    #[test]
    fn top_only_and_bottom_only_fill_the_view() {
        let top = ds(DualLayout::TopOnly, false, 256.0, 192.0);
        assert_eq!(top.len(), 1);
        assert!(rect_close(top[0].dest, FracRect::WHOLE));
        assert_eq!(top[0].src, DualScreenGeometry::NINTENDO_DS.top);
        let bottom = ds(DualLayout::BottomOnly, false, 256.0, 192.0);
        assert_eq!(bottom[0].src, DualScreenGeometry::NINTENDO_DS.bottom);
        // Swap flips the one-screen layouts too.
        assert_eq!(ds(DualLayout::TopOnly, true, 256.0, 192.0)[0].src, DualScreenGeometry::NINTENDO_DS.bottom);
        assert_eq!(ds(DualLayout::BottomOnly, true, 256.0, 192.0)[0].src, DualScreenGeometry::NINTENDO_DS.top);
    }

    #[test]
    fn n3ds_stacked_keeps_the_narrower_bottom_screen_centred() {
        // 400x480 arrangement in a 400x480 view: the bottom 320 is 40 in from each side, which is
        // exactly where it sits in the frame, so Stacked draws what the single quad draws.
        let p = n3ds(DualLayout::Stacked, false, 400.0, 480.0);
        assert!(rect_close(p[0].dest, FracRect::new(0.0, 0.0, 1.0, 0.5)));
        assert!(rect_close(p[1].dest, FracRect::new(0.1, 0.5, 0.8, 0.5)));
        assert_eq!(p[1].src, DualScreenGeometry::NINTENDO_3DS.bottom);
    }

    #[test]
    fn n3ds_side_by_side_uses_each_screens_own_width() {
        // 400 + 320 = 720 wide, 240 tall. In a 720x240 view the bottom screen is 320/720 wide.
        let p = n3ds(DualLayout::SideBySide, false, 720.0, 240.0);
        assert!(rect_close(p[0].dest, FracRect::new(0.0, 0.0, 400.0 / 720.0, 1.0)));
        assert!(rect_close(p[1].dest, FracRect::new(400.0 / 720.0, 0.0, 320.0 / 720.0, 1.0)));
    }

    #[test]
    fn n3ds_big_bottom_is_320_wide_and_the_small_top_is_200() {
        // Big bottom 320x240, small top 200x120 above it: 320 x 360.
        let p = n3ds(DualLayout::BigBottom, false, 320.0, 360.0);
        assert!(rect_close(p[0].dest, FracRect::new(60.0 / 320.0, 0.0, 200.0 / 320.0, 120.0 / 360.0)));
        assert!(rect_close(p[1].dest, FracRect::new(0.0, 120.0 / 360.0, 1.0, 240.0 / 360.0)));
    }

    #[test]
    fn letterbox_is_centred() {
        // DS stacked (2:3) in a square view: 2/3 wide, centred.
        let p = ds(DualLayout::Stacked, false, 300.0, 300.0);
        let w: f32 = 256.0 / 384.0;
        assert!(close(p[0].dest.x, (1.0 - w) / 2.0));
        assert!(close(p[0].dest.w, w));
    }

    #[test]
    fn stretch_fills_and_integer_scale_snaps() {
        let geometry = DualScreenGeometry::NINTENDO_DS;
        let c = cfg(DualLayout::Stacked, false);
        let stretched = dual_placements(&geometry, &c, TargetRole::Phone, 256.0, 384.0, 1000.0, 500.0, ScaleMode::Stretch);
        assert!(rect_close(stretched[0].dest, FracRect::new(0.0, 0.0, 1.0, 0.5)));
        let integer = dual_placements(&geometry, &c, TargetRole::Phone, 256.0, 384.0, 600.0, 900.0, ScaleMode::IntegerScale);
        // 600/256 = 2.34, 900/384 = 2.34 -> 2x: 512 of 600 wide.
        assert!(close(integer[0].dest.w, 512.0 / 600.0));
    }

    #[test]
    fn every_layout_fits_the_uniform_array_and_stays_inside_the_view() {
        for layout in DualLayout::ALL {
            for swapped in [false, true] {
                for landscape in [false, true] {
                    let c = DualScreenConfig { layout, swapped, landscape, touch_on_phone: true };
                    for geometry in [DualScreenGeometry::NINTENDO_DS, DualScreenGeometry::NINTENDO_3DS] {
                        let p = dual_placements(&geometry, &c, TargetRole::Phone, 400.0, 480.0, 390.0, 600.0, ScaleMode::AspectFit);
                        assert!(!p.is_empty() && p.len() <= 4);
                        for placement in p {
                            assert!(placement.dest.x >= -EPS && placement.dest.y >= -EPS);
                            assert!(placement.dest.x + placement.dest.w <= 1.0 + EPS);
                            assert!(placement.dest.y + placement.dest.h <= 1.0 + EPS);
                        }
                    }
                }
            }
        }
    }

    // ---------------------------------------------------------------- touch

    fn touch(placements: &[ScreenPlacement], geometry: &DualScreenGeometry, x: f32, y: f32) -> Option<(f32, f32)> {
        let region = touch_region(placements, geometry)?;
        map_touch(&region, x, y, false)
    }

    #[test]
    fn ds_stacked_touch_matches_the_old_host_mapping() {
        let g = DualScreenGeometry::NINTENDO_DS;
        let p = ds(DualLayout::Stacked, false, 256.0, 384.0);
        // Top-left of the touch screen is (0, 0.5) of the frame; centre of it is (0.5, 0.75).
        let (x, y) = touch(&p, &g, 0.0, 0.5).unwrap();
        assert!(close(x, 0.0) && close(y, 0.5));
        let (x, y) = touch(&p, &g, 0.5, 0.75).unwrap();
        assert!(close(x, 0.5) && close(y, 0.75));
        // The top screen is not a digitiser.
        assert!(touch(&p, &g, 0.5, 0.25).is_none());
    }

    #[test]
    fn ds_swapped_touch_follows_the_screen_to_the_top() {
        let g = DualScreenGeometry::NINTENDO_DS;
        let p = ds(DualLayout::Stacked, true, 256.0, 384.0);
        // Now the TOP half of the view is the touch screen, and its centre is still the centre of
        // the guest touch screen.
        let (x, y) = touch(&p, &g, 0.5, 0.25).unwrap();
        assert!(close(x, 0.5) && close(y, 0.75));
        assert!(touch(&p, &g, 0.5, 0.75).is_none());
    }

    #[test]
    fn side_by_side_touch_lands_on_the_right_guest_pixel() {
        let g = DualScreenGeometry::NINTENDO_DS;
        let p = ds(DualLayout::SideBySide, false, 1024.0, 384.0);
        // A tap 3/4 across and halfway down is the centre of the bottom screen.
        let (x, y) = touch(&p, &g, 0.75, 0.5).unwrap();
        assert!(close(x, 0.5) && close(y, 0.75));
        // Guest pixel (64, 48) of the bottom screen: view x = 0.5 + 64/512, y = 48/192.
        let (x, y) = touch(&p, &g, 0.5 + 64.0 / 512.0, 48.0 / 192.0).unwrap();
        assert!(close(x * 256.0, 64.0), "x px {}", x * 256.0);
        assert!(close(y * 384.0, 192.0 + 48.0), "y px {}", y * 384.0);
    }

    #[test]
    fn n3ds_touch_maps_into_the_320_wide_bottom_screen() {
        let g = DualScreenGeometry::NINTENDO_3DS;
        // Stacked: the left 40 px of the lower half are not the touch screen.
        let p = n3ds(DualLayout::Stacked, false, 400.0, 480.0);
        assert!(touch(&p, &g, 0.05, 0.75).is_none());
        let (x, y) = touch(&p, &g, 0.5, 0.75).unwrap();
        assert!(close(x, 0.5) && close(y, 0.75));
        // Bottom-screen pixel (0, 0) is framebuffer pixel (40, 240).
        let (x, y) = touch(&p, &g, 0.1, 0.5).unwrap();
        assert!(close(x * 400.0, 40.0) && close(y * 480.0, 240.0));

        // Side by side in a 720x240 view: bottom-screen pixel (160, 120) is view (560, 120).
        let p = n3ds(DualLayout::SideBySide, false, 720.0, 240.0);
        let (x, y) = touch(&p, &g, 560.0 / 720.0, 0.5).unwrap();
        assert!(close(x * 400.0, 40.0 + 160.0), "x px {}", x * 400.0);
        assert!(close(y * 480.0, 240.0 + 120.0), "y px {}", y * 480.0);

        // Bottom only fills the view with the 320x240 screen.
        let p = n3ds(DualLayout::BottomOnly, false, 320.0, 240.0);
        let (x, y) = touch(&p, &g, 0.0, 0.0).unwrap();
        assert!(close(x * 400.0, 40.0) && close(y * 480.0, 240.0));
        let (x, y) = touch(&p, &g, 1.0, 1.0).unwrap();
        assert!(close(x * 400.0, 360.0) && close(y * 480.0, 480.0));

        // Top only has no touch screen at all.
        let p = n3ds(DualLayout::TopOnly, false, 400.0, 240.0);
        assert!(touch_region(&p, &g).is_none());
    }

    #[test]
    fn n3ds_big_bottom_touch() {
        let g = DualScreenGeometry::NINTENDO_3DS;
        let p = n3ds(DualLayout::BigBottom, false, 320.0, 360.0);
        // Centre of the big bottom screen: view (160, 240) of 320x360.
        let (x, y) = touch(&p, &g, 0.5, 240.0 / 360.0).unwrap();
        assert!(close(x, 0.5) && close(y, 0.75));
        // The small top screen is not a digitiser.
        assert!(touch(&p, &g, 0.5, 60.0 / 360.0).is_none());
    }

    #[test]
    fn clamped_touch_reads_the_edge() {
        let g = DualScreenGeometry::NINTENDO_DS;
        let p = ds(DualLayout::Stacked, false, 256.0, 384.0);
        let region = touch_region(&p, &g).unwrap();
        assert!(map_touch(&region, 1.2, 0.75, false).is_none());
        let (x, _) = map_touch(&region, 1.2, 0.75, true).unwrap();
        assert!(close(x, 1.0));
        assert!(map_touch(&region, f32::NAN, 0.75, true).is_none());
        assert!(map_touch(&region, 0.5, f32::INFINITY, true).is_none());
    }

    #[test]
    fn the_single_quad_maps_touch_through_the_whole_frame() {
        // The default path draws the whole stacked frame in one quad; the touch region is its
        // lower half, letterbox included.
        let g = DualScreenGeometry::NINTENDO_DS;
        let p = [single_placement([0.5, 1.0])];
        let region = touch_region(&p, &g).unwrap();
        assert!(rect_close(region.dest, FracRect::new(0.25, 0.5, 0.5, 0.5)));
        let (x, y) = map_touch(&region, 0.5, 0.75, false).unwrap();
        assert!(close(x, 0.5) && close(y, 0.75));
    }

    // ---------------------------------------------------------------- skins

    fn hole(dest_y: f32, src_y: f32, src_x: f32, src_w: f32) -> SkinHole {
        SkinHole { dest_x: 0.1, dest_y, dest_w: 0.8, dest_h: 0.3, src_x, src_y, src_w, src_h: 240.0 }
    }

    #[test]
    fn swap_exchanges_which_picture_goes_in_which_hole() {
        let g = DualScreenGeometry::NINTENDO_3DS;
        let holes = [hole(0.05, 0.0, 0.0, 400.0), hole(0.4, 240.0, 40.0, 320.0)];
        assert!(skin_can_swap(&holes, &g, 400.0, 480.0));
        let swapped = swap_skin_holes(&holes, &g, 400.0, 480.0);
        // Holes stay where the skin drew them; the crops trade places.
        assert_eq!(swapped[0].dest_y, 0.05);
        assert_eq!(swapped[0].src_y, 240.0);
        assert_eq!(swapped[0].src_x, 40.0);
        assert_eq!(swapped[1].src_y, 0.0);
        assert_eq!(swapped[1].src_w, 400.0);
        // And touch follows the bottom picture into the upper hole.
        let p = hole_placements(&swapped, 400.0, 480.0);
        let region = touch_region(&p, &g).unwrap();
        assert!(close(region.dest.y, 0.05));
        let (x, y) = map_touch(&region, 0.5, 0.2, false).unwrap();
        assert!(close(x, 0.5) && close(y, 0.75));
    }

    #[test]
    fn a_skin_hole_with_margins_maps_only_its_touch_screen_part() {
        // A 3DS bottom hole cropping the full 400 width of the lower half: the touch screen is the
        // middle 80% of that hole.
        let g = DualScreenGeometry::NINTENDO_3DS;
        let holes = [hole(0.05, 0.0, 0.0, 400.0), hole(0.4, 240.0, 0.0, 400.0)];
        let p = hole_placements(&holes, 400.0, 480.0);
        let region = touch_region(&p, &g).unwrap();
        assert!(rect_close(region.dest, FracRect::new(0.1 + 0.08, 0.4, 0.64, 0.3)));
    }

    #[test]
    fn an_uncropped_skin_cannot_swap_and_keeps_its_holes() {
        let g = DualScreenGeometry::NINTENDO_DS;
        let one = [SkinHole { dest_x: 0.0, dest_y: 0.0, dest_w: 1.0, dest_h: 0.5, src_x: 0.0, src_y: 0.0, src_w: 0.0, src_h: 0.0 }];
        assert!(!skin_can_swap(&one, &g, 256.0, 384.0));
        assert_eq!(swap_skin_holes(&one, &g, 256.0, 384.0), one.to_vec());
        // Its touch screen is the lower half of the one hole.
        let region = touch_region(&hole_placements(&one, 256.0, 384.0), &g).unwrap();
        assert!(rect_close(region.dest, FracRect::new(0.0, 0.25, 1.0, 0.25)));
    }

    // ---------------------------------------------------------------- external display

    #[test]
    fn with_a_tv_the_touch_screen_stays_on_the_phone() {
        let g = DualScreenGeometry::NINTENDO_DS;
        let c = cfg(DualLayout::Stacked, false);
        let tv = dual_placements(&g, &c, TargetRole::External, 256.0, 384.0, 1920.0, 1080.0, ScaleMode::AspectFit);
        assert_eq!(tv.len(), 1);
        assert_eq!(tv[0].src, g.top);
        let phone = dual_placements(&g, &c, TargetRole::PhoneCompanion, 256.0, 384.0, 390.0, 292.0, ScaleMode::AspectFit);
        assert_eq!(phone.len(), 1);
        assert_eq!(phone[0].src, g.bottom);
        assert!(touch_region(&phone, &g).is_some());
        // Swap trades them.
        let s = cfg(DualLayout::Stacked, true);
        let tv = dual_placements(&g, &s, TargetRole::External, 256.0, 384.0, 1920.0, 1080.0, ScaleMode::AspectFit);
        assert_eq!(tv[0].src, g.bottom);
    }

    #[test]
    fn with_a_tv_and_touch_on_phone_off_the_tv_gets_everything() {
        let g = DualScreenGeometry::NINTENDO_DS;
        let c = DualScreenConfig { layout: DualLayout::SideBySide, swapped: false, landscape: false, touch_on_phone: false };
        let tv = dual_placements(&g, &c, TargetRole::External, 256.0, 384.0, 1920.0, 1080.0, ScaleMode::AspectFit);
        assert_eq!(tv.len(), 2);
        let phone = dual_placements(&g, &c, TargetRole::PhoneCompanion, 256.0, 384.0, 390.0, 292.0, ScaleMode::AspectFit);
        assert!(phone.is_empty());
    }

    #[test]
    fn arrangement_aspects() {
        let g = DualScreenGeometry::NINTENDO_3DS;
        let a = |layout| arrangement_aspect(&g, &cfg(layout, false), TargetRole::Phone, 400.0, 480.0).unwrap();
        assert!(close(a(DualLayout::Stacked), 400.0 / 480.0));
        assert!(close(a(DualLayout::SideBySide), 720.0 / 240.0));
        assert!(close(a(DualLayout::TopOnly), 400.0 / 240.0));
        assert!(close(a(DualLayout::BottomOnly), 320.0 / 240.0));
        let phone = arrangement_aspect(&g, &cfg(DualLayout::Stacked, false), TargetRole::PhoneCompanion, 400.0, 480.0).unwrap();
        assert!(close(phone, 320.0 / 240.0));
    }

    #[test]
    fn systems_with_two_screens_are_named() {
        assert_eq!(DualScreenGeometry::for_system("ds"), Some(DualScreenGeometry::NINTENDO_DS));
        assert_eq!(DualScreenGeometry::for_system("n3ds"), Some(DualScreenGeometry::NINTENDO_3DS));
        assert_eq!(DualScreenGeometry::for_system("gba"), None);
    }
}
