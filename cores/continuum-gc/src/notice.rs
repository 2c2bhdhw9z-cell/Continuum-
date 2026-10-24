//! A text screen the core shows instead of a black picture when a game has shown nothing for a
//! while: what it is doing, in a few lines a tester can screenshot. Our own 5x7 pixel font,
//! drawn at twice its size.

/// Rows of a glyph, top first; bit 4 is the leftmost pixel.
fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        ':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        ',' => [0x00, 0x00, 0x00, 0x00, 0x0C, 0x04, 0x08],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '/' => [0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        '\'' => [0x0C, 0x04, 0x08, 0x00, 0x00, 0x00, 0x00],
        '?' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x00, 0x04],
        '!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x00, 0x04],
        '%' => [0x18, 0x19, 0x02, 0x04, 0x08, 0x13, 0x03],
        '=' => [0x00, 0x00, 0x1F, 0x00, 0x1F, 0x00, 0x00],
        '_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F],
        '[' => [0x0E, 0x08, 0x08, 0x08, 0x08, 0x08, 0x0E],
        ']' => [0x0E, 0x02, 0x02, 0x02, 0x02, 0x02, 0x0E],
        _ => [0; 7],
    }
}

/// Pixels a character takes across (with its gap) and a line down, at twice the font's size.
pub const CHAR_W: usize = 12;
pub const LINE_H: usize = 18;

/// Draws `lines` in white on black over the whole `w` x `h` frame (0RGB pixels), from a margin
/// at the top left; text past the right edge is cut off.
pub fn draw(frame: &mut [u32], w: usize, h: usize, lines: &[String]) {
    frame.iter_mut().for_each(|p| *p = 0);
    let (left, top) = (16, 16);
    for (row, line) in lines.iter().enumerate() {
        let y0 = top + row * LINE_H;
        if y0 + 14 > h {
            break;
        }
        for (col, c) in line.chars().enumerate() {
            let x0 = left + col * CHAR_W;
            if x0 + 10 > w {
                break;
            }
            for (gy, bits) in glyph(c).iter().enumerate() {
                for gx in 0..5 {
                    if bits & (0x10 >> gx) != 0 {
                        for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                            frame[(y0 + 2 * gy + dy) * w + x0 + 2 * gx + dx] = 0x00FF_FFFF;
                        }
                    }
                }
            }
        }
    }
}

/// Draws `lines` in white on a darkened panel in the top-left corner of the `w` x `h` picture,
/// over what is there (the speed read-out). The font is drawn at its own size (one pixel per
/// font pixel) on pictures narrower than 600 pixels, else at twice.
pub fn draw_panel(frame: &mut [u32], w: usize, h: usize, lines: &[String]) {
    let scale = if w >= 600 { 2 } else { 1 };
    let (cw, lh, pad) = (6 * scale, 9 * scale, 2 * scale);
    let cols = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let pw = (cols * cw + 2 * pad).min(w);
    let ph = (lines.len() * lh + 2 * pad).min(h);
    if frame.len() < w * h {
        return;
    }
    for y in 0..ph {
        for p in &mut frame[y * w..y * w + pw] {
            // A quarter of the colour left: the text stands out, the game still shows.
            *p = (*p >> 2) & 0x003F_3F3F;
        }
    }
    for (row, line) in lines.iter().enumerate() {
        let y0 = pad + row * lh;
        if y0 + 7 * scale > ph {
            break;
        }
        for (col, c) in line.chars().enumerate() {
            let x0 = pad + col * cw;
            if x0 + 5 * scale > pw {
                break;
            }
            for (gy, bits) in glyph(c).iter().enumerate() {
                for gx in 0..5 {
                    if bits & (0x10 >> gx) != 0 {
                        for dy in 0..scale {
                            for dx in 0..scale {
                                frame[(y0 + scale * gy + dy) * w + x0 + scale * gx + dx] = 0x00FF_FFFF;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Splits `text` into lines of at most `width` characters, at spaces where it can.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut word = word.to_string();
        while word.chars().count() > width {
            let head: String = word.chars().take(width).collect();
            word = word.chars().skip(width).collect();
            if !line.is_empty() {
                out.push(std::mem::take(&mut line));
            }
            out.push(head);
        }
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_fits_five_columns_and_text_lands_where_expected() {
        for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789:.,-/()'?!%=_[]".chars() {
            assert!(glyph(c).iter().all(|r| r & !0x1F == 0), "{c}");
            assert!(glyph(c).iter().any(|r| *r != 0), "{c} is blank");
        }
        let mut f = vec![0x123456u32; 640 * 480];
        draw(&mut f, 640, 480, &["I".to_string()]);
        // 'I' row 0 is 01110: pixels 1-3 of the glyph, doubled, from (16, 16).
        assert_eq!(f[16 * 640 + 16], 0);
        assert_eq!(f[16 * 640 + 18], 0x00FF_FFFF);
        assert_eq!(f[17 * 640 + 23], 0x00FF_FFFF);
        assert_eq!(f[16 * 640 + 24], 0);
        assert_eq!(f[400 * 640 + 400], 0, "the rest is black");
    }

    #[test]
    fn the_panel_darkens_its_corner_and_leaves_the_rest() {
        let mut f = vec![0x0080_8080u32; 640 * 480];
        draw_panel(&mut f, 640, 480, &["I".to_string()]);
        assert_eq!(f[0], 0x0020_2020, "darkened");
        assert_eq!(f[4 * 640 + 4 + 2], 0x00FF_FFFF, "the I's top row");
        assert_eq!(f[300 * 640 + 300], 0x0080_8080, "the picture elsewhere");
        // Small pictures get the font at its own size.
        let mut s = vec![0u32; 320 * 240];
        draw_panel(&mut s, 320, 240, &["I".to_string()]);
        assert_eq!(s[2 * 320 + 2 + 1], 0x00FF_FFFF);
    }

    #[test]
    fn long_text_wraps_at_spaces() {
        let lines = wrap("one two three four five", 9);
        assert_eq!(lines, vec!["one two", "three", "four five"]);
        assert_eq!(wrap("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
    }
}
