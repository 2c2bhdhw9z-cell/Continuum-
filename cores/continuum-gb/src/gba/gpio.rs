//! Game Boy Advance cartridge extras: the GPIO port at $080000C4-$080000C9 (real-time clock,
//! solar sensor, gyro sensor, rumble motor) and the two-axis tilt sensor mapped at $0E008000.
//! Written from the public descriptions of these parts.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum Hardware {
    #[default]
    None,
    Rtc,
    Solar,
    Gyro,
    Rumble,
    Tilt,
}

/// What a game code (ROM $AC-$AE) is known to carry.
pub fn detect(rom: &[u8]) -> Hardware {
    if rom.len() < 0xB0 {
        return Hardware::None;
    }
    let code = &rom[0xAC..0xAF];
    match code {
        b"AXV" | b"AXP" | b"BPE" | b"BLJ" | b"BLV" | b"BKA" | b"BR4" | b"BRK" | b"A9L" => Hardware::Rtc,
        b"U3I" | b"U32" | b"U33" => Hardware::Solar,
        b"RZW" => Hardware::Gyro,
        b"V49" => Hardware::Rumble,
        b"KYG" | b"KHP" => Hardware::Tilt,
        _ => Hardware::None,
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Gpio {
    pub hw: Hardware,
    data: u8,
    dir: u8,
    readable: bool,
    // Clock (S-3511 style): 3-wire serial, SCK bit 0, SIO bit 1, CS bit 2.
    rtc_active: bool,
    rtc_bits: u8,
    rtc_shift: u8,
    rtc_cmd: Option<u8>,
    rtc_out: Vec<u8>,
    rtc_out_bit: u8,
    rtc_in: Vec<u8>,
    rtc_status: u8,
    rtc_sio: bool,
    // Solar: bit 0 clock, bit 1 reset, bit 3 flag.
    solar_count: u8,
    // Gyro: bit 0 start, bit 1 clock, bit 2 data, bit 3 motor.
    gyro_sample: u16,
    gyro_bits: u8,
    gyro_out: bool,
    // Tilt.
    tilt_armed: u8,
    tilt_x: u16,
    tilt_y: u16,
    tilt_ready: bool,
    /// Inputs from the frontend, not saved.
    #[serde(skip)]
    pub light: u8,
    #[serde(skip)]
    pub gyro_rate: f32,
    #[serde(skip)]
    pub tilt: (f32, f32),
    #[serde(skip)]
    pub motor: bool,
    /// Seconds to add to the phone's clock (core option), and the clock source.
    #[serde(skip)]
    pub clock_offset: i64,
    /// Count the cartridge clock from the game's own running time instead of the phone's clock
    /// (core option "Advance cartridge clock").
    #[serde(skip)]
    pub game_clock: bool,
    /// Seconds this machine has run, fed by the bus for the game-time clock.
    #[serde(skip)]
    pub run_secs: u64,
    /// The phone's time, and the run time, the game-time clock is measured from. Taken afresh at
    /// each start and after a save state is loaded, so game time begins at the phone's time.
    #[serde(skip)]
    clock_anchor: Option<(i64, u64)>,
}

fn bcd(v: u32) -> u8 {
    ((v / 10) << 4 | (v % 10)) as u8
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Local date and time as (year 0-99, month, day, weekday 0-6, hour, minute, second).
pub fn local_time(offset: i64) -> [u32; 7] {
    time_at(now_unix() + offset)
}

/// The same for a chosen moment, so a clock counted from play time converts the same way.
pub fn time_at(unix: i64) -> [u32; 7] {
    let t = unix as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut tm) };
    [
        (tm.tm_year % 100) as u32,
        (tm.tm_mon + 1) as u32,
        tm.tm_mday as u32,
        tm.tm_wday as u32,
        tm.tm_hour as u32,
        tm.tm_min as u32,
        tm.tm_sec as u32,
    ]
}

impl Gpio {
    pub fn new(hw: Hardware) -> Gpio {
        Gpio { hw, rtc_status: 0x40, light: 128, ..Gpio::default() }
    }

    pub fn has_port(&self) -> bool {
        matches!(self.hw, Hardware::Rtc | Hardware::Solar | Hardware::Gyro | Hardware::Rumble)
    }

    /// A 16-bit read of $080000C4-$080000C9, when the port is readable.
    pub fn read(&self, addr: u32) -> Option<u16> {
        if !self.has_port() || !self.readable {
            return None;
        }
        match addr & 0xF {
            0x4 => Some(u16::from(self.pins_in())),
            0x6 => Some(u16::from(self.dir)),
            0x8 => Some(u16::from(self.readable)),
            _ => None,
        }
    }

    fn pins_in(&self) -> u8 {
        let mut v = self.data & self.dir;
        let input = !self.dir & 0xF;
        let dev = match self.hw {
            Hardware::Rtc => u8::from(self.rtc_sio) << 1,
            Hardware::Solar => {
                let threshold = 0xE8u16.saturating_sub(u16::from(self.light) * 0x98 / 255) as u8;
                u8::from(self.solar_count >= threshold) << 3
            }
            Hardware::Gyro => u8::from(self.gyro_out) << 2,
            _ => 0,
        };
        v |= dev & input;
        v
    }

    pub fn write(&mut self, addr: u32, v: u16) -> bool {
        if !self.has_port() {
            return false;
        }
        match addr & 0xF {
            0x4 => {
                let old = self.data;
                self.data = (v as u8) & 0xF;
                self.pins_changed(old);
                true
            }
            0x6 => {
                self.dir = (v as u8) & 0xF;
                true
            }
            0x8 => {
                self.readable = v & 1 != 0;
                true
            }
            _ => false,
        }
    }

    fn pins_changed(&mut self, old: u8) {
        let d = self.data;
        let rising = |bit: u8| d & bit != 0 && old & bit == 0;
        let falling = |bit: u8| d & bit == 0 && old & bit != 0;
        match self.hw {
            Hardware::Rtc => self.rtc_pins(old),
            Hardware::Solar => {
                if d & 2 != 0 {
                    self.solar_count = 0;
                } else if rising(1) {
                    self.solar_count = self.solar_count.saturating_add(1);
                }
            }
            Hardware::Gyro => {
                self.motor = d & 8 != 0;
                if rising(1) {
                    // Centre $6C0, about $200 for a fast turn, 12 bits.
                    let s = (0x6C0 as f32 + self.gyro_rate.clamp(-1.0, 1.0) * 0x200 as f32) as i32;
                    self.gyro_sample = (s.clamp(0, 0xFFF) as u16) << 2;
                    self.gyro_bits = 16;
                }
                if falling(2) && self.gyro_bits > 0 {
                    self.gyro_out = self.gyro_sample & 0x8000 != 0;
                    self.gyro_sample <<= 1;
                    self.gyro_bits -= 1;
                }
            }
            Hardware::Rumble => self.motor = d & 8 != 0,
            _ => {}
        }
    }

    fn rtc_pins(&mut self, old: u8) {
        let d = self.data;
        let cs = d & 4 != 0;
        if !cs {
            self.rtc_active = false;
            self.rtc_cmd = None;
            self.rtc_bits = 0;
            self.rtc_shift = 0;
            self.rtc_out.clear();
            self.rtc_in.clear();
            return;
        }
        if !self.rtc_active {
            self.rtc_active = true;
            self.rtc_bits = 0;
            self.rtc_shift = 0;
            return;
        }
        let rising = d & 1 != 0 && old & 1 == 0;
        if !rising {
            return;
        }
        let sio = d & 2 != 0;
        match self.rtc_cmd {
            None => {
                self.rtc_shift = self.rtc_shift << 1 | u8::from(sio);
                self.rtc_bits += 1;
                if self.rtc_bits == 8 {
                    let mut c = self.rtc_shift;
                    // Sent either way round by different games: the fixed nibble says which.
                    if c >> 4 != 6 {
                        c = c.reverse_bits();
                    }
                    self.rtc_bits = 0;
                    self.rtc_shift = 0;
                    if c >> 4 == 6 {
                        self.rtc_command(c);
                    }
                }
            }
            Some(c) if c & 1 != 0 => {
                // Reading: one bit out per clock, LSB first.
                let byte = self.rtc_out.get(usize::from(self.rtc_out_bit / 8)).copied().unwrap_or(0xFF);
                self.rtc_sio = byte >> (self.rtc_out_bit % 8) & 1 != 0;
                self.rtc_out_bit = self.rtc_out_bit.saturating_add(1);
            }
            Some(c) => {
                self.rtc_shift |= u8::from(sio) << self.rtc_bits;
                self.rtc_bits += 1;
                if self.rtc_bits == 8 {
                    self.rtc_in.push(self.rtc_shift);
                    self.rtc_bits = 0;
                    self.rtc_shift = 0;
                    if (c >> 1) & 7 == 1 && !self.rtc_in.is_empty() {
                        self.rtc_status = self.rtc_in[0] & 0x6A;
                    }
                }
            }
        }
    }

    /// The time the cartridge's clock reads: the phone's, or one that started at the phone's time
    /// and since then has only moved while the game was running.
    fn clock_now(&mut self) -> [u32; 7] {
        if !self.game_clock {
            return local_time(self.clock_offset);
        }
        let (base, from) = *self.clock_anchor.get_or_insert((now_unix(), self.run_secs));
        time_at(base + (self.run_secs.saturating_sub(from)) as i64 + self.clock_offset)
    }

    fn rtc_command(&mut self, c: u8) {
        let reg = (c >> 1) & 7;
        self.rtc_cmd = Some(c);
        self.rtc_out.clear();
        self.rtc_out_bit = 0;
        if c & 1 == 0 {
            if reg == 0 {
                self.rtc_status = 0;
            }
            return;
        }
        let t = self.clock_now();
        let hour = |h: u32, status: u8| -> u8 {
            if status & 0x40 != 0 {
                bcd(h) | if h >= 12 { 0x80 } else { 0 }
            } else {
                bcd(h % 12) | if h >= 12 { 0x80 } else { 0 }
            }
        };
        self.rtc_out = match reg {
            1 => vec![self.rtc_status],
            2 => vec![bcd(t[0]), bcd(t[1]), bcd(t[2]), bcd(t[3]), hour(t[4], self.rtc_status), bcd(t[5]), bcd(t[6])],
            3 => vec![hour(t[4], self.rtc_status), bcd(t[5]), bcd(t[6])],
            _ => vec![],
        };
    }

    /// Tilt sensor reads ($0E008200-$0E008500); None when the address is not the sensor's.
    pub fn tilt_read(&self, addr: u32) -> Option<u8> {
        if self.hw != Hardware::Tilt {
            return None;
        }
        match addr & 0xFFFF {
            0x8200 => Some(self.tilt_x as u8),
            0x8300 => Some(((self.tilt_x >> 8) as u8 & 0xF) | if self.tilt_ready { 0x80 } else { 0 }),
            0x8400 => Some(self.tilt_y as u8),
            0x8500 => Some((self.tilt_y >> 8) as u8 & 0xF),
            _ => None,
        }
    }

    pub fn tilt_write(&mut self, addr: u32, v: u8) -> bool {
        if self.hw != Hardware::Tilt {
            return false;
        }
        match (addr & 0xFFFF, v) {
            (0x8000, 0x55) => self.tilt_armed = 1,
            (0x8100, 0xAA) if self.tilt_armed == 1 => {
                let axis = |t: f32| (0x3A0 as f32 + t.clamp(-1.0, 1.0) * 0xE0 as f32) as u16;
                self.tilt_x = axis(self.tilt.0);
                self.tilt_y = axis(self.tilt.1);
                self.tilt_ready = true;
                self.tilt_armed = 0;
            }
            (0x8000 | 0x8100, _) => self.tilt_armed = 0,
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rtc_send(g: &mut Gpio, byte: u8, msb_first: bool) {
        for i in 0..8 {
            let bit = if msb_first { byte >> (7 - i) & 1 } else { byte >> i & 1 };
            g.write(0x080000C4, u16::from(4 | bit << 1));
            g.write(0x080000C4, u16::from(5 | bit << 1));
        }
    }

    #[test]
    fn rtc_status_and_time_read() {
        let mut g = Gpio::new(Hardware::Rtc);
        g.write(0x080000C8, 1);
        g.write(0x080000C6, 7);
        g.write(0x080000C4, 1);
        g.write(0x080000C4, 5);
        rtc_send(&mut g, 0x63, true); // status read (reg 1)
        g.write(0x080000C6, 5);
        let mut v = 0u8;
        for i in 0..8 {
            g.write(0x080000C4, 4);
            g.write(0x080000C4, 5);
            v |= ((g.read(0x080000C4).unwrap() >> 1) as u8 & 1) << i;
        }
        assert_eq!(v, 0x40);
        let t = local_time(0);
        assert!(t[1] >= 1 && t[1] <= 12 && t[6] < 61);
    }

    #[test]
    fn solar_flag_needs_more_clocks_in_the_dark() {
        let count = |light: u8| {
            let mut g = Gpio::new(Hardware::Solar);
            g.light = light;
            g.write(0x080000C8, 1);
            g.write(0x080000C6, 7);
            g.write(0x080000C4, 2);
            g.write(0x080000C4, 0);
            let mut n = 0;
            while g.read(0x080000C4).unwrap() & 8 == 0 && n < 300 {
                g.write(0x080000C4, 1);
                g.write(0x080000C4, 0);
                n += 1;
            }
            n
        };
        assert!(count(0) > count(255));
    }

    /// Seconds into the day, so two clock readings can be compared across midnight.
    fn gap(later: [u32; 7], earlier: [u32; 7]) -> i64 {
        let secs = |t: [u32; 7]| i64::from(t[4]) * 3600 + i64::from(t[5]) * 60 + i64::from(t[6]);
        (secs(later) - secs(earlier)).rem_euclid(86_400)
    }

    #[test]
    fn the_cartridge_clock_can_count_play_time_instead_of_the_phone() {
        // Phone clock (the default): the phone's time, however long the game has been running.
        let mut phone = Gpio::new(Hardware::Rtc);
        phone.run_secs = 7 * 3600;
        assert!(gap(phone.clock_now(), local_time(0)) <= 1);
        // Game time: starts at the phone's time, then moves only with the game's running time.
        let mut game = Gpio::new(Hardware::Rtc);
        game.game_clock = true;
        let start = game.clock_now();
        assert!(gap(start, local_time(0)) <= 1);
        game.run_secs = 3661; // an hour, a minute and a second of play
        let later = game.clock_now();
        assert_eq!(gap(later, start), 3661);
        // The clock offset setting still moves it.
        game.clock_offset = 3600;
        assert_eq!(gap(game.clock_now(), later), 3600);
    }

    #[test]
    fn tilt_and_gyro_follow_the_input() {
        let mut g = Gpio::new(Hardware::Tilt);
        g.tilt = (1.0, 0.0);
        g.tilt_write(0x0E008000, 0x55);
        g.tilt_write(0x0E008100, 0xAA);
        let x = u16::from(g.tilt_read(0x0E008200).unwrap()) | u16::from(g.tilt_read(0x0E008300).unwrap() & 0xF) << 8;
        assert_eq!(x, 0x3A0 + 0xE0);
        let mut y = Gpio::new(Hardware::Gyro);
        y.gyro_rate = 0.0;
        y.write(0x080000C8, 1);
        y.write(0x080000C6, 0xB);
        y.write(0x080000C4, 1);
        let mut s = 0u16;
        for _ in 0..16 {
            y.write(0x080000C4, 3);
            y.write(0x080000C4, 1);
            s = s << 1 | (y.read(0x080000C4).unwrap() >> 2 & 1);
        }
        assert_eq!(s >> 2, 0x6C0);
    }
}
