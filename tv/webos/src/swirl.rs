//! The New look's swirling background, the same maths as the iPhone app
//! (native/ios/NewLookMotion.swift, NewLookSwirlField): colour clouds round a slowly turning oval,
//! two whirlpools stirring them, mixed in Oklab so the colours never turn grey.

pub const OVER: f64 = 0.08;

fn linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn gamma(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

pub fn oklab(rgb: [f64; 3]) -> [f64; 3] {
    let (r, g, b) = (linear(rgb[0]), linear(rgb[1]), linear(rgb[2]));
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

pub fn from_oklab(lab: [f64; 3]) -> [f64; 3] {
    let l0 = lab[0] + 0.3963377774 * lab[1] + 0.2158037573 * lab[2];
    let m0 = lab[0] - 0.1055613458 * lab[1] - 0.0638541728 * lab[2];
    let s0 = lab[0] - 0.0894841775 * lab[1] - 1.2914855480 * lab[2];
    let (l, m, s) = (l0 * l0 * l0, m0 * m0 * m0, s0 * s0 * s0);
    [
        gamma(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
        gamma(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
        gamma(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
    ]
}

pub struct Palette {
    lightness: Vec<f64>,
    chroma: Vec<f64>,
    hue: Vec<f64>,
    ring: Vec<usize>,
}

impl Palette {
    pub fn new(colours: &[[f64; 3]]) -> Palette {
        let labs: Vec<[f64; 3]> = colours.iter().map(|c| oklab(*c)).collect();
        let lightness: Vec<f64> = labs.iter().map(|l| l[0]).collect();
        let chroma: Vec<f64> = labs.iter().map(|l| (l[1] * l[1] + l[2] * l[2]).sqrt()).collect();
        let raw: Vec<f64> = labs.iter().map(|l| l[2].atan2(l[1])).collect();
        let n = raw.len();
        if n == 0 {
            return Palette { lightness, chroma, hue: Vec::new(), ring: Vec::new() };
        }
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|a, b| raw[*a].partial_cmp(&raw[*b]).unwrap());
        let (mut cut, mut widest) = (0, -1.0);
        for k in 0..n {
            let mut gap = raw[order[(k + 1) % n]] - raw[order[k]];
            if k == n - 1 {
                gap += 2.0 * std::f64::consts::PI;
            }
            if gap > widest {
                widest = gap;
                cut = k;
            }
        }
        let start = raw[order[(cut + 1) % n]];
        let two_pi = 2.0 * std::f64::consts::PI;
        let hue: Vec<f64> = raw
            .iter()
            .map(|v| {
                let d = (v - start) % two_pi;
                start + if d < 0.0 { d + two_pi } else { d }
            })
            .collect();
        let mut wheel: Vec<usize> = (0..n).collect();
        wheel.sort_by(|a, b| hue[*a].partial_cmp(&hue[*b]).unwrap());
        let mut ring = wheel.clone();
        ring.extend(wheel.iter().rev());
        Palette { lightness, chroma, hue, ring }
    }
}

/// Everything one frame of the field needs apart from the per-point maths, for the graphics
/// chip's version of it (draw.rs FS_FIELD).
pub struct FieldParams {
    pub t: f32,
    pub mix: f32,
    pub cols: u32,
    pub rows: u32,
    pub whirls: [[f32; 3]; 2],
    pub clouds: Vec<[f32; 3]>,
    pub pal: Vec<[f32; 3]>,
}

pub fn params(time: f64, tall: f64, p: &Palette, k: f64) -> Option<FieldParams> {
    let t = time * 1.5;
    if p.lightness.is_empty() || tall <= 0.0 {
        return None;
    }
    let cols = 40usize;
    let rows = ((cols as f64 * tall).round() as usize).max(1);
    let (spin, wobble) = (0.08, 0.13);
    let mut whirls = [[0f32; 3]; 2];
    for (v, w) in whirls.iter_mut().enumerate() {
        let fv = v as f64;
        *w = [
            (0.5 + 0.28 * (t * 0.051 + fv * 2.6).sin()) as f32,
            (tall * (0.5 + 0.28 * (t * 0.039 + fv * 1.9 + 0.8).sin())) as f32,
            ((if v == 0 { 3.0 } else { -3.0 }) * (0.75 + 0.25 * (t * 0.11 + fv).sin())) as f32,
        ];
    }
    let cx = 0.5 + 0.05 * (t * 0.07).sin();
    let cy = tall * (0.5 + 0.04 * (t * 0.053 + 1.0).sin());
    let m = p.ring.len();
    let clouds = p
        .ring
        .iter()
        .enumerate()
        .map(|(s, &colour)| {
            let fs = s as f64;
            let angle = t * spin + 2.0 * std::f64::consts::PI * fs / m as f64;
            let reach = 1.0 + wobble / 0.3 * (t * (0.13 + 0.02 * fs) + fs * 2.1).sin();
            [(cx + 0.30 * reach * angle.cos()) as f32, (cy + 0.31 * tall * reach * angle.sin()) as f32, colour as f32]
        })
        .collect();
    let pal = (0..p.lightness.len()).map(|c| [p.lightness[c] as f32, p.chroma[c] as f32, p.hue[c] as f32]).collect();
    Some(FieldParams { t: t as f32, mix: (0.35 + 0.6 * k) as f32, cols: cols as u32, rows: rows as u32, whirls, clouds, pal })
}

/// One frame of the swirl as a small RGBA grid (`width` x `height`) covering the screen and
/// `OVER` past each edge. `tall` is height / width.
pub fn field(time: f64, tall: f64, p: &Palette, k: f64, base: [f64; 3], out: &mut Vec<u8>) -> (u32, u32) {
    let t = time * 1.5;
    let n = p.lightness.len();
    if n == 0 || tall <= 0.0 {
        return (0, 0);
    }
    let cols = 40usize;
    let rows = ((cols as f64 * tall).round() as usize).max(1);
    let (sigma, sharp, spin, wobble) = (0.22, 1.8, 0.08, 0.13);
    let mix = 0.35 + 0.6 * k;
    let whirls: Vec<(f64, f64, f64)> = (0..2)
        .map(|v| {
            let fv = v as f64;
            (
                0.5 + 0.28 * (t * 0.051 + fv * 2.6).sin(),
                tall * (0.5 + 0.28 * (t * 0.039 + fv * 1.9 + 0.8).sin()),
                (if v == 0 { 3.0 } else { -3.0 }) * (0.75 + 0.25 * (t * 0.11 + fv).sin()),
            )
        })
        .collect();
    let cx = 0.5 + 0.05 * (t * 0.07).sin();
    let cy = tall * (0.5 + 0.04 * (t * 0.053 + 1.0).sin());
    let m = p.ring.len();
    let clouds: Vec<(f64, f64, usize)> = p
        .ring
        .iter()
        .enumerate()
        .map(|(s, &colour)| {
            let fs = s as f64;
            let angle = t * spin + 2.0 * std::f64::consts::PI * fs / m as f64;
            let reach = 1.0 + wobble / 0.3 * (t * (0.13 + 0.02 * fs) + fs * 2.1).sin();
            (cx + 0.30 * reach * angle.cos(), cy + 0.31 * tall * reach * angle.sin(), colour)
        })
        .collect();
    let falloff = 1.0 / (2.0 * sigma * sigma);
    let (width, height) = (cols + 1, rows + 1);
    out.clear();
    out.resize(width * height * 4, 255);
    let mut weight = vec![0.0f64; n];
    for j in 0..height {
        let y0 = (-OVER + (1.0 + 2.0 * OVER) * j as f64 / rows as f64) * tall;
        for i in 0..width {
            let mut qx = -OVER + (1.0 + 2.0 * OVER) * i as f64 / cols as f64;
            let mut qy = y0;
            for w in &whirls {
                let (dx, dy) = (qx - w.0, qy - w.1);
                let a = w.2 * (-(dx * dx + dy * dy) / 0.12).exp();
                let (sa, ca) = a.sin_cos();
                qx = w.0 + dx * ca - dy * sa;
                qy = w.1 + dx * sa + dy * ca;
            }
            let px = qx + 0.05 * (qy * 3.1 + t * 0.37).sin() + 0.025 * (qy * 5.3 - t * 0.29 + 1.3).sin();
            let py = qy + 0.05 * (qx * 2.7 - t * 0.31 + 2.1).sin() + 0.025 * (qx * 4.9 + t * 0.23).sin();
            weight.iter_mut().for_each(|w| *w = 0.0);
            for c in &clouds {
                let (dx, dy) = (px - c.0, py - c.1);
                weight[c.2] += (-(dx * dx + dy * dy) * falloff).exp();
            }
            let (mut total, mut l, mut chroma, mut hue) = (0.0, 0.0, 0.0, 0.0);
            for c in 0..n {
                let v = (weight[c] + 1e-4).powf(sharp);
                total += v;
                l += v * p.lightness[c];
                chroma += v * p.chroma[c];
                hue += v * p.hue[c];
            }
            l /= total;
            chroma /= total;
            hue /= total;
            let rgb = from_oklab([l, chroma * hue.cos(), chroma * hue.sin()]);
            let o = (j * width + i) * 4;
            for ch in 0..3 {
                let v = base[ch] * (1.0 - mix) + rgb[ch] * mix;
                out[o + ch] = (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            }
        }
    }
    (width as u32, height as u32)
}

// ---- colour helpers shared with the theme ----

pub fn hex_rgb(hex: &str) -> [f64; 3] {
    let h = hex.trim_start_matches('#');
    let v = u32::from_str_radix(h.get(0..6).unwrap_or("000000"), 16).unwrap_or(0);
    [((v >> 16) & 255) as f64 / 255.0, ((v >> 8) & 255) as f64 / 255.0, (v & 255) as f64 / 255.0]
}

pub fn rgb_hex(c: [f64; 3]) -> String {
    let b = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    format!("{:02X}{:02X}{:02X}", b(c[0]), b(c[1]), b(c[2]))
}

pub fn hsb(c: [f64; 3]) -> (f64, f64, f64) {
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    let d = mx - mn;
    let s = if mx > 0.0 { d / mx } else { 0.0 };
    let h = if d == 0.0 {
        0.0
    } else if mx == c[0] {
        (((c[1] - c[2]) / d) / 6.0 + 1.0).fract()
    } else if mx == c[1] {
        ((c[2] - c[0]) / d + 2.0) / 6.0
    } else {
        ((c[0] - c[1]) / d + 4.0) / 6.0
    };
    (h, s, mx)
}

pub fn from_hsb(h: f64, s: f64, v: f64) -> [f64; 3] {
    let hh = (h.rem_euclid(1.0)) * 6.0;
    let sector = hh.floor() as i32 % 6;
    let f = hh - hh.floor();
    let (p, q, u) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match sector {
        0 => [v, u, p],
        1 => [q, v, p],
        2 => [p, v, u],
        3 => [p, q, v],
        4 => [u, p, v],
        _ => [v, p, q],
    }
}

pub fn shifted_hue(hex: &str, by: f64) -> String {
    let (h, s, b) = hsb(hex_rgb(hex));
    rgb_hex(from_hsb((h + by).rem_euclid(1.0), s.max(0.35), b))
}
