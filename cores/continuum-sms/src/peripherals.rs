//! What else plugs into a Master System's two controller ports: the Light Phaser, the Paddle
//! Control (HPD-200) and the Sports Pad.
//!
//! Written from SMS Power's "Light Phaser" and "Paddle" pages and JoppyFurr's "Sports Pad
//! Investigation" (SMS Power forums, October 2025: the relative mode the console drives with TH,
//! four nibbles X high, X low, Y high, Y low, movement sent reversed; and the absolute mode that
//! cycles five nibbles by itself, TL and TR saying which).
//!
//! Each port's six lines are returned as the pins read: bit 0 up, 1 down, 2 left, 3 right, 4 TL
//! (button 1), 5 TR (button 2); 1 is high (nothing pressed). The Light Phaser's light sensor is TH,
//! which the bus answers itself (it needs the picture and the beam).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Device {
    #[default]
    Pad,
    Phaser,
    Paddle,
    SportsPad,
}

/// One frame of a player's controls, from the app.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controls {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub b1: bool,
    pub b2: bool,
    /// The left stick, -1 to 1 each way (right and down positive).
    pub stick: (f32, f32),
    /// A finger on the picture, in the console's picture pixels, while it touches.
    pub touch: Option<(f32, f32)>,
}

/// Lines above and below the aim the sensor sees, and how far right of the aim the beam keeps it
/// lit (long enough that a game's polling loop, 30 or so cycles, always sees it).
pub const PHASER_LINES: i32 = 4;
pub const PHASER_WIDTH: i32 = 48;
/// The Sports Pad starts a new reading when TH has not changed for this long (CPU cycles): a
/// reading's four steps come a few tens of microseconds apart.
const SPORTS_TIMEOUT: u64 = 1000;
/// The absolute mode's nibbles change about every 48 microseconds (five in 238).
const SPORTS_STEP: u64 = 170;
/// The paddle's clock flips TR at about 8 kHz.
const PADDLE_HALF: u64 = 224;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Port {
    /// What is plugged in, and how fast the stick turns it (1-10): settings, not saved.
    #[serde(skip)]
    pub device: Device,
    #[serde(skip, default = "five")]
    pub speed: u8,
    #[serde(skip)]
    pub b1: bool,
    #[serde(skip)]
    pub b2: bool,
    /// Light Phaser: where it points, in picture pixels, and whether that is on the picture.
    pub aim: (f32, f32),
    pub aim_on: bool,
    /// Paddle: the knob, 0 (fully anticlockwise) to 255.
    pub paddle: f32,
    /// Sports Pad: the ball's position (it wraps), and where it was at the last reading.
    pub ball: (f32, f32),
    sent: (i32, i32),
    nibbles: [u8; 4],
    index: u8,
    last_strobe: u64,
    th_level: bool,
}

fn five() -> u8 {
    5
}

impl Default for Port {
    fn default() -> Port {
        Port {
            device: Device::Pad,
            speed: 5,
            b1: false,
            b2: false,
            aim: (128.0, 96.0),
            aim_on: true,
            paddle: 128.0,
            ball: (0.0, 0.0),
            sent: (0, 0),
            nibbles: [0; 4],
            index: 0,
            last_strobe: 0,
            th_level: true,
        }
    }
}

impl Port {
    /// Takes a frame's controls. `lines` is the picture's height.
    pub fn frame(&mut self, c: &Controls, lines: f32) {
        let k = f32::from(self.speed.clamp(1, 10)) / 5.0;
        let dpad = (f32::from(i8::from(c.right) - i8::from(c.left)), f32::from(i8::from(c.down) - i8::from(c.up)));
        let dead = |v: f32| if v.abs() < 0.15 { 0.0 } else { v };
        let stick = (dead(c.stick.0), dead(c.stick.1));
        self.b1 = c.b1;
        self.b2 = c.b2;
        match self.device {
            Device::Pad => {}
            Device::Phaser => {
                if let Some((x, y)) = c.touch {
                    // A tap aims there and pulls the trigger.
                    self.aim = (x, y);
                    self.aim_on = (0.0..256.0).contains(&x) && (0.0..lines).contains(&y);
                    self.b1 = true;
                } else {
                    let step = |s: f32, d: f32| (s * 6.0 + d * 3.0) * k;
                    self.aim.0 = (self.aim.0 + step(stick.0, dpad.0)).clamp(0.0, 255.0);
                    self.aim.1 = (self.aim.1 + step(stick.1, dpad.1)).clamp(0.0, lines - 1.0);
                    self.aim_on = true;
                }
            }
            Device::Paddle => {
                self.paddle = (self.paddle + (stick.0 * 4.0 + dpad.0 * 3.0) * k).clamp(0.0, 255.0);
            }
            Device::SportsPad => {
                self.ball.0 += (stick.0 * 4.0 + dpad.0 * 2.0) * k;
                self.ball.1 += (stick.1 * 4.0 + dpad.1 * 2.0) * k;
                // Kept small (the pad's own counters wrap at 256), moving the last reading along by
                // the same whole number of turns so the next movement is still right.
                for (b, sent) in [(&mut self.ball.0, &mut self.sent.0), (&mut self.ball.1, &mut self.sent.1)] {
                    if b.abs() > 65536.0 {
                        let turns = (*b / 256.0).trunc() * 256.0;
                        *b -= turns;
                        *sent -= turns as i32;
                    }
                }
            }
        }
    }

    fn buttons(&self) -> u8 {
        u8::from(!self.b1) << 4 | u8::from(!self.b2) << 5
    }

    /// The six lines at CPU time `t`. `th_out` is what the console drives on TH when it does (an
    /// export console with the pin set to output).
    pub fn lines(&self, t: u64, th_out: Option<bool>) -> u8 {
        match self.device {
            Device::Pad => 0x3F,
            Device::Phaser => 0x0F | self.buttons() | 0x20,
            Device::Paddle => {
                // An export console picks the nibble with TH; the Japanese paddle's own clock
                // flips between them, saying which on TR.
                let high = th_out.unwrap_or((t / PADDLE_HALF) & 1 != 0);
                let pos = self.paddle as u8;
                let nibble = if high { pos >> 4 } else { pos & 15 };
                nibble | u8::from(!self.b1) << 4 | u8::from(high) << 5
            }
            Device::SportsPad => {
                if th_out.is_some() {
                    let i = usize::from(self.index.max(1) - 1);
                    self.nibbles[i] | self.buttons()
                } else {
                    let (x, y) = (self.ball.0 as i32 as u8, self.ball.1 as i32 as u8);
                    match (t / SPORTS_STEP) % 5 {
                        0 => 0x04 | u8::from(!self.b2) << 1 | u8::from(!self.b1) | 0x30,
                        1 => x >> 4,
                        2 => (x & 15) | 0x10,
                        3 => y >> 4,
                        _ => (y & 15) | 0x10,
                    }
                }
            }
        }
    }

    /// The console drove TH to `level` at time `t` (export consoles). The Sports Pad's relative
    /// mode moves to its next nibble on each change; the first of a reading takes the movement.
    pub fn th_driven(&mut self, level: bool, t: u64) {
        if level == self.th_level {
            return;
        }
        self.th_level = level;
        if self.device != Device::SportsPad {
            return;
        }
        if self.index >= 4 || t.saturating_sub(self.last_strobe) > SPORTS_TIMEOUT {
            self.index = 0;
        }
        if self.index == 0 {
            let now = (self.ball.0 as i32, self.ball.1 as i32);
            // Sent reversed: games negate it.
            let dx = (self.sent.0 - now.0).clamp(-128, 127) as i8 as u8;
            let dy = (self.sent.1 - now.1).clamp(-128, 127) as i8 as u8;
            self.sent = now;
            self.nibbles = [dx >> 4, dx & 15, dy >> 4, dy & 15];
        }
        self.index += 1;
        self.last_strobe = t;
    }
}

/// Is the picture bright enough at this pixel for the sensor to see it?
pub fn bright(rgb: u32) -> bool {
    let (r, g, b) = ((rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF);
    r * 3 + g * 6 + b >= 960
}

/// The crosshair: a cross or a square outline, 11 pixels across, at (x, y) in `px` (w wide).
pub fn draw_crosshair(px: &mut [u32], w: usize, h: usize, x: i32, y: i32, square: bool, colour: u32) {
    let mut put = |dx: i32, dy: i32| {
        let (cx, cy) = (x + dx, y + dy);
        if cx >= 0 && cy >= 0 && (cx as usize) < w && (cy as usize) < h {
            px[cy as usize * w + cx as usize] = colour;
        }
    };
    for d in -5..=5 {
        if square {
            put(d, -5);
            put(d, 5);
            put(-5, d);
            put(5, d);
        } else if d != 0 {
            put(d, 0);
            put(0, d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_paddle_gives_both_nibbles_with_tr_saying_which() {
        let p = Port { device: Device::Paddle, paddle: 0xA7 as f32, ..Port::default() };
        assert_eq!(p.lines(0, None), 0x07 | 0x10);
        assert_eq!(p.lines(PADDLE_HALF, None), 0x0A | 0x10 | 0x20);
        assert_eq!(p.lines(0, Some(true)), 0x0A | 0x10 | 0x20, "export: TH picks the high nibble");
    }

    #[test]
    fn the_sports_pad_sends_its_movement_reversed_one_nibble_per_th_change() {
        let mut p = Port { device: Device::SportsPad, ..Port::default() };
        p.frame(&Controls { stick: (1.0, 0.0), ..Controls::default() }, 192.0);
        p.frame(&Controls { stick: (1.0, -1.0), ..Controls::default() }, 192.0);
        let (mx, my) = (p.ball.0 as i32, p.ball.1 as i32);
        assert!(mx > 0 && my < 0);
        let mut got = Vec::new();
        for (i, level) in [false, true, false, true].into_iter().enumerate() {
            p.th_driven(level, 100 + i as u64 * 200);
            got.push(p.lines(0, Some(level)) & 15);
        }
        let dx = (got[0] << 4 | got[1]) as i8;
        let dy = (got[2] << 4 | got[3]) as i8;
        assert_eq!((-i32::from(dx), -i32::from(dy)), (mx, my));
        // Read again straight away: nothing moved since.
        for (i, level) in [false, true].into_iter().enumerate() {
            p.th_driven(level, 2000 + i as u64 * 200);
        }
        assert_eq!(p.lines(0, Some(true)) & 15, 0);
    }

    #[test]
    fn the_sports_pad_on_its_own_cycles_five_nibbles() {
        let p = Port { device: Device::SportsPad, ball: (0x5C as f32, 0x31 as f32), b1: true, ..Port::default() };
        let seen: Vec<u8> = (0..5).map(|i| p.lines(i * SPORTS_STEP, None)).collect();
        assert_eq!(seen[0] & 0x30, 0x30, "TL and TR high: the buttons nibble");
        assert_eq!(seen[0] & 1, 0, "button 1 held");
        assert_eq!([seen[1] & 15, seen[2] & 15, seen[3] & 15, seen[4] & 15], [5, 0xC, 3, 1]);
        assert_eq!([seen[1] & 0x30, seen[2] & 0x30], [0x00, 0x10]);
    }
}
