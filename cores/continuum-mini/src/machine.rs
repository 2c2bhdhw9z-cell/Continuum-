//! Pokémon Mini memory map and the peripherals clocked from the 4 MHz system clock.

use crate::{
    audio::Audio,
    eeprom::Eeprom,
    link::Infrared,
    minx::{Bus, Minx},
    prc::Prc,
};
use serde::{Deserialize, Serialize};

pub const CLOCK: u32 = 4_000_000;
pub const FPS: f64 = 72.0;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Machine {
    pub cpu: Minx,
    pub ram: Vec<u8>,
    pub regs: Vec<u8>,
    pub bios: Vec<u8>,
    pub cart: Vec<u8>,
    pub prc: Prc,
    pub audio: Audio,
    pub eeprom: Eeprom,
    pub infrared: Infrared,
    pub buttons: u8,
    previous_buttons: u8,
    pub rumble: bool,
    pub shock: bool,
    pub cycles: u64,
    second_phase: u32,
    hz256_phase: u32,
    timer_phase: [u32; 6],
}

struct Memory<'a> {
    ram: &'a mut Vec<u8>,
    regs: &'a mut Vec<u8>,
    bios: &'a Vec<u8>,
    cart: &'a Vec<u8>,
    prc: &'a mut Prc,
    eeprom: &'a mut Eeprom,
    infrared: &'a mut Infrared,
    rumble: &'a mut bool,
}
impl Bus for Memory<'_> {
    fn read(&mut self, a: u32) -> u8 {
        match a {
            0..=0xfff => self.bios.get(a as usize).copied().unwrap_or(0xff),
            0x1000..=0x1fff => self.ram[(a as usize - 0x1000) & 0xfff],
            0x2000..=0x20ff => {
                let r = a as usize & 0xff;
                if (0x80..=0x8a).contains(&r) {
                    return self.prc.read_reg(r as u8);
                }
                if r == 0x52 {
                    return self.regs[0x52];
                };
                if r == 0x61 {
                    let mut v = self.regs[r] | 0x40;
                    if self.eeprom.sda() {
                        v |= 4
                    } else {
                        v &= !4
                    };
                    if self.infrared.clock() {
                        v |= 0x81
                    } else {
                        v &= !0x81
                    };
                    return v;
                }
                self.regs[r]
            }
            0x2100..=0x1fffff => self.cart.get(a as usize).copied().unwrap_or(0xff),
            _ => 0xff,
        }
    }
    fn write(&mut self, a: u32, v: u8) {
        match a {
            0x1000..=0x1fff => self.ram[(a as usize - 0x1000) & 0xfff] = v,
            0x2000..=0x20ff => {
                let r = a as usize & 0xff;
                if (0x80..=0x8a).contains(&r) || r == 0xfe || r == 0xff {
                    self.prc.write_reg(r as u8, v);
                }
                match r {
                    0x09..=0x0b | 0x36 | 0x37 | 0x3e | 0x3f | 0x4e | 0x4f | 0x52 => return,
                    0x27..=0x2a => self.regs[r] &= !v,
                    0x08 => {
                        self.regs[r] = v & !2;
                        if v & 2 != 0 {
                            self.regs[9] = 0;
                            self.regs[10] = 0;
                            self.regs[11] = 0
                        }
                    }
                    0x61 => {
                        let dir = self.regs[0x60];
                        self.eeprom.pins(
                            if dir & 4 != 0 { v & 4 != 0 } else { true },
                            if dir & 8 != 0 { v & 8 != 0 } else { false },
                        );
                        self.infrared.write(dir & 0x20 != 0, v & 2 != 0);
                        *self.rumble = dir & 0x10 != 0 && v & 0x10 != 0;
                        self.regs[r] = v
                    }
                    _ => self.regs[r] = v,
                }
            }
            _ => {}
        }
    }
}

impl Machine {
    pub fn new(mut cart: Vec<u8>, bios: Option<Vec<u8>>, real_bios: bool) -> Result<Self, String> {
        if cart.len() <= 0x2102 || cart.len() > 0x20_0000 {
            return Err("This is not a complete Pokémon Mini game file.".into());
        }
        if cart.len() < 0x20_0000 {
            cart.resize(0x20_0000, 0xff)
        }
        let use_bios = real_bios && bios.as_ref().is_some_and(|b| b.len() == 0x1000);
        let mut internal_bios = vec![0xff; 4096];
        if !use_bios {
            for vector in 0..0x4d {
                internal_bios[vector * 2..vector * 2 + 2].copy_from_slice(&0x0100u16.to_le_bytes());
            }
            internal_bios[0x100] = 0xf8;
        }
        let mut m = Self {
            cpu: Minx::default(),
            ram: vec![0; 4096],
            regs: vec![0; 256],
            bios: if use_bios {
                bios.unwrap()
            } else {
                internal_bios
            },
            cart,
            prc: Prc::default(),
            audio: Audio::default(),
            eeprom: Eeprom::default(),
            infrared: Infrared::default(),
            buttons: 0xff,
            previous_buttons: 0xff,
            rumble: false,
            shock: false,
            cycles: 0,
            second_phase: 0,
            hz256_phase: 0,
            timer_phase: [0; 6],
        };
        if let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            m.eeprom.set_rtc_unix(now.as_secs());
        }
        m.regs[0x52] = 0xff;
        m.regs[0x81] = 7;
        let direct = !use_bios;
        let mut cpu = std::mem::take(&mut m.cpu);
        {
            let mut mem = m.memory();
            cpu.reset(&mut mem, direct)
        }
        m.cpu = cpu;
        Ok(m)
    }
    fn memory(&mut self) -> Memory<'_> {
        Memory {
            ram: &mut self.ram,
            regs: &mut self.regs,
            bios: &self.bios,
            cart: &self.cart,
            prc: &mut self.prc,
            eeprom: &mut self.eeprom,
            infrared: &mut self.infrared,
            rumble: &mut self.rumble,
        }
    }
    pub fn reset(&mut self, real_bios: bool) {
        self.ram.fill(0);
        self.regs.fill(0);
        self.regs[0x52] = 0xff;
        self.regs[0x81] = 7;
        let direct =
            !(real_bios && self.bios.len() == 0x1000 && self.bios.iter().any(|&b| b != 0xff));
        let mut cpu = std::mem::take(&mut self.cpu);
        {
            let mut mem = self.memory();
            cpu.reset(&mut mem, direct)
        }
        self.cpu = cpu;
    }
    pub fn set_buttons(&mut self, buttons: u8, shock: bool) {
        let pressed = self.previous_buttons & !buttons;
        self.regs[0x29] |= pressed;
        self.regs[0x52] = buttons;
        if shock && !self.shock {
            self.regs[0x2a] |= 0x40
        }
        self.previous_buttons = buttons;
        self.buttons = buttons;
        self.shock = shock;
        self.update_irq();
    }
    fn update_irq(&mut self) {
        let groups = [
            (self.regs[0x23] & self.regs[0x27], self.regs[0x20]),
            (self.regs[0x24] & self.regs[0x28], self.regs[0x21]),
            (self.regs[0x25] & self.regs[0x29], self.regs[0x21]),
            (self.regs[0x26] & self.regs[0x2a], self.regs[0x22]),
        ];
        let mut best = None;
        for (g, (flags, pri)) in groups.into_iter().enumerate() {
            if flags == 0 {
                continue;
            }
            let p = match g {
                0 => (pri >> 6) & 3,
                1 => (pri >> 4) & 3,
                2 => (pri >> 2) & 3,
                _ => pri & 3,
            };
            if p == 0 {
                continue;
            }
            let bit = flags.trailing_zeros() as u8;
            let vector = match g {
                0 => 8 - bit,
                1 => 14 - bit,
                2 => 28 - bit,
                _ => 16 - bit,
            };
            if best.is_none_or(|(bp, _)| p >= bp) {
                best = Some((p, vector))
            }
        }
        self.cpu.irq = best.map(|x| x.1)
    }
    pub fn read(&mut self, a: u32) -> u8 {
        self.memory().read(a)
    }
    pub fn write(&mut self, a: u32, v: u8) {
        self.memory().write(a, v);
        self.sync_audio_regs();
        self.update_irq()
    }
    fn sync_audio_regs(&mut self) {
        self.audio.enabled = self.regs[0x70] & 3 == 0;
        self.audio.volume = self.regs[0x71] & 3;
        self.audio.preset = u16::from_le_bytes([self.regs[0x4a], self.regs[0x4b]]);
        self.audio.pivot = u16::from_le_bytes([self.regs[0x4c], self.regs[0x4d]]);
        self.audio.timer = u16::from_le_bytes([self.regs[0x4e], self.regs[0x4f]])
    }
    fn clock_timers(&mut self, c: u32) {
        self.second_phase += c;
        if self.regs[8] & 1 != 0 {
            while self.second_phase >= CLOCK {
                self.second_phase -= CLOCK;
                let mut n = u32::from_le_bytes([self.regs[9], self.regs[10], self.regs[11], 0]) + 1;
                self.regs[9] = n as u8;
                n >>= 8;
                self.regs[10] = n as u8;
                n >>= 8;
                self.regs[11] = n as u8
            }
        }
        self.hz256_phase += c * 256;
        while self.hz256_phase >= CLOCK {
            self.hz256_phase -= CLOCK;
            if self.regs[0x40] & 1 != 0 {
                self.regs[0x41] = self.regs[0x41].wrapping_add(1);
                let n = self.regs[0x41];
                if n & 7 == 0 {
                    self.regs[0x28] |= 0x20
                }
                if n & 31 == 0 {
                    self.regs[0x28] |= 0x10
                }
                if n & 127 == 0 {
                    self.regs[0x28] |= 8
                }
                if n == 0 {
                    self.regs[0x28] |= 4
                }
            }
        }
        let ctl = [0x18, 0x18, 0x1a, 0x1a, 0x1c, 0x1c];
        let cnt = [0x36, 0x37, 0x3e, 0x3f, 0x4e, 0x4f];
        let pre = [0x32, 0x33, 0x3a, 0x3b, 0x4a, 0x4b];
        let irq = [0x04, 0x08, 0x10, 0x20, 0x01, 0x02];
        let div = [2, 8, 32, 64, 128, 256, 1024, 4096];
        for k in 0..6 {
            let shift = if k & 1 == 0 { 0 } else { 4 };
            if self.regs[ctl[k]] & (8 << shift) == 0 {
                continue;
            }
            let d = div[((self.regs[ctl[k]] >> shift) & 7) as usize];
            self.timer_phase[k] += c;
            while self.timer_phase[k] >= d {
                self.timer_phase[k] -= d;
                let (v, under) = self.regs[cnt[k]].overflowing_sub(1);
                self.regs[cnt[k]] = if under { self.regs[pre[k]] } else { v };
                if under {
                    self.regs[0x27] |= irq[k]
                }
            }
        }
        self.sync_audio_regs();
        self.update_irq()
    }
    pub fn run_frame(
        &mut self,
        rate: u32,
        piezo: bool,
        lowpass: u8,
        gain: u8,
        audio: &mut Vec<i16>,
    ) {
        let target = self.cycles + CLOCK as u64 / 72;
        while self.cycles < target {
            let mut cpu = std::mem::take(&mut self.cpu);
            let c = {
                let mut mem = self.memory();
                cpu.step(&mut mem)
            };
            self.cpu = cpu;
            self.cycles += c as u64;
            self.clock_timers(c);
            self.audio.render(c, rate, piezo, lowpass, gain, audio)
        }
        let bios = &self.bios;
        let cart = &self.cart;
        let read = |a: u32| match a {
            0..=0xfff => bios.get(a as usize).copied().unwrap_or(0xff),
            0x2100..=0x1fffff => cart.get(a as usize).copied().unwrap_or(0xff),
            _ => 0xff,
        };
        let (draw, copy) = self.prc.tick((CLOCK / 72) as u32, &mut self.ram, &read);
        if draw {
            self.regs[0x27] |= 0x40
        }
        if copy {
            self.regs[0x27] |= 0x80
        }
        self.update_irq()
    }
}
