//! The 7800's colours, made by our own model of its colour circuit: 16 hues and 16 brightnesses.
//! Hue 0 is grey; hues 1 to 15 step round the colour wheel from a fixed start, each by the same
//! delay-line phase (about 25.7 degrees, which the console's colour adjustment and its warming up
//! move between about 24.7 and 27.7 degrees, as Atari's service notes and MAME's palette notes
//! describe). The brightness is linear. Turned into RGB the NTSC way (YIQ).
//!
//! The model's start, step and colour strength were fitted to the colours MAME 0.289 shows (its
//! table was measured from real consoles): within about 6 levels of 255 per channel, on average,
//! for the NTSC console and the PAL one alike.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Atari,
    Cool,
    Warm,
    Hot,
    Pal,
    Grey,
}

/// 256 colours as 0x00RRGGBB, with the colour strength and brightness the settings give (percent).
pub fn build(preset: Preset, saturation: u8, brightness: u8) -> [u32; 256] {
    let (start, step, sat) = match preset {
        Preset::Atari => (-42.25, 25.8, 0.215),
        Preset::Cool => (-42.25, 25.2, 0.215),
        Preset::Warm => (-42.25, 26.7, 0.215),
        Preset::Hot => (-42.25, 27.7, 0.215),
        Preset::Pal => (-66.0, 25.5, 0.21),
        Preset::Grey => (0.0, 0.0, 0.0),
    };
    let sat = sat * saturation as f64 / 100.0;
    let bright = brightness as f64 / 100.0;
    let mut out = [0u32; 256];
    for (c, o) in out.iter_mut().enumerate() {
        let hue = c >> 4;
        let y = (c & 15) as f64 / 15.0 * bright;
        let (i, q) = if hue == 0 || preset == Preset::Grey {
            (0.0, 0.0)
        } else {
            let a = (start + (hue - 1) as f64 * step).to_radians();
            (sat * a.cos(), sat * a.sin())
        };
        let r = y + 0.956 * i + 0.621 * q;
        let g = y - 0.272 * i - 0.647 * q;
        let b = y - 1.106 * i + 1.703 * q;
        let to8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
        *o = (to8(r) << 16) | (to8(g) << 8) | to8(b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grey_and_the_ends_of_the_scale() {
        let p = build(Preset::Atari, 100, 100);
        assert_eq!(p[0x00], 0x000000);
        assert_eq!(p[0x0F], 0xFFFFFF);
        assert_eq!(p[0x07], 0x777777);
        let g = build(Preset::Grey, 100, 100);
        for (c, &rgb) in g.iter().enumerate() {
            assert_eq!(rgb, (rgb & 0xFF) * 0x010101, "colour {c:02x} is grey");
        }
    }

    #[test]
    fn the_presets_and_adjustments_differ() {
        let a = build(Preset::Atari, 100, 100);
        for p in [Preset::Cool, Preset::Warm, Preset::Hot, Preset::Pal, Preset::Grey] {
            assert_ne!(a, build(p, 100, 100), "{p:?}");
        }
        assert_ne!(a, build(Preset::Atari, 150, 100));
        assert_ne!(a, build(Preset::Atari, 100, 80));
    }
}
