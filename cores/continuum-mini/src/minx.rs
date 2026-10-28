//! Cycle-accounted MINX interpreter. The decoder covers all 256 primary bytes and both extension
//! pages; undefined encodings still consume their measured bus time instead of hanging the machine.

use serde::{Deserialize, Serialize};

pub const FLAG_I: u8 = 0x80;
pub const FLAG_D: u8 = 0x40;
pub const FLAG_S: u8 = 0x08;
pub const FLAG_O: u8 = 0x04;
pub const FLAG_C: u8 = 0x02;
pub const FLAG_Z: u8 = 0x01;

pub trait Bus {
    fn read(&mut self, address: u32) -> u8;
    fn write(&mut self, address: u32, value: u8);
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Minx {
    pub pc: u16,
    pub sp: u16,
    pub ba: u16,
    pub hl: u16,
    pub x: u16,
    pub y: u16,
    pub u: u8,
    pub v: u8,
    pub f: u8,
    pub e: u8,
    pub n: u8,
    pub i: u8,
    pub xi: u8,
    pub yi: u8,
    pub halted: bool,
    pub irq: Option<u8>,
    pub cycles: u64,
}

impl Minx {
    pub fn reset(&mut self, bus: &mut impl Bus, direct: bool) {
        *self = Self::default();
        self.sp = 0x2000;
        self.pc = if direct { 0x2102 } else { self.read16(bus, 0) };
        if direct {
            self.u = 1;
            self.v = 0;
        }
    }

    pub fn address(&self) -> u32 {
        if self.pc & 0x8000 != 0 {
            (self.v as u32) << 15 | (self.pc & 0x7fff) as u32
        } else {
            self.pc as u32
        }
    }
    fn fetch(&mut self, bus: &mut impl Bus) -> u8 {
        let v = bus.read(self.address());
        self.pc = self.pc.wrapping_add(1);
        v
    }
    fn fetch16(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.fetch(bus);
        let hi = self.fetch(bus);
        u16::from_le_bytes([lo, hi])
    }
    fn read16(&self, bus: &mut impl Bus, a: u32) -> u16 {
        u16::from_le_bytes([bus.read(a), bus.read(a.wrapping_add(1))])
    }
    fn write16(&self, bus: &mut impl Bus, a: u32, v: u16) {
        let b = v.to_le_bytes();
        bus.write(a, b[0]);
        bus.write(a + 1, b[1]);
    }
    fn a(&self) -> u8 {
        self.ba as u8
    }
    fn b(&self) -> u8 {
        (self.ba >> 8) as u8
    }
    fn l(&self) -> u8 {
        self.hl as u8
    }
    fn h(&self) -> u8 {
        (self.hl >> 8) as u8
    }
    fn set_a(&mut self, v: u8) {
        self.ba = (self.ba & 0xff00) | v as u16
    }
    fn set_b(&mut self, v: u8) {
        self.ba = (self.ba & 0x00ff) | (v as u16) << 8
    }
    fn set_l(&mut self, v: u8) {
        self.hl = (self.hl & 0xff00) | v as u16
    }
    fn set_h(&mut self, v: u8) {
        self.hl = (self.hl & 0x00ff) | (v as u16) << 8
    }
    fn addr_ihl(&self) -> u32 {
        (self.i as u32) << 16 | self.hl as u32
    }
    fn addr_x(&self) -> u32 {
        (self.xi as u32) << 16 | self.x as u32
    }
    fn addr_y(&self) -> u32 {
        (self.yi as u32) << 16 | self.y as u32
    }
    fn addr_n(&mut self, bus: &mut impl Bus) -> u32 {
        (self.i as u32) << 16 | (self.n as u32) << 8 | self.fetch(bus) as u32
    }
    fn addr_i16(&mut self, bus: &mut impl Bus) -> u32 {
        (self.i as u32) << 16 | self.fetch16(bus) as u32
    }

    fn flags8(&mut self, value: u8, carry: bool, overflow: bool) {
        self.f = (self.f & !(FLAG_S | FLAG_O | FLAG_C | FLAG_Z))
            | if value & 0x80 != 0 { FLAG_S } else { 0 }
            | if overflow { FLAG_O } else { 0 }
            | if carry { FLAG_C } else { 0 }
            | if value == 0 { FLAG_Z } else { 0 };
    }
    fn add8(&mut self, a: u8, b: u8, c: bool) -> u8 {
        let ci = (c && (self.f & FLAG_C != 0)) as u16;
        let r = a as u16 + b as u16 + ci;
        let v = r as u8;
        self.flags8(v, r > 255, (!(a ^ b) & (a ^ v) & 0x80) != 0);
        v
    }
    fn sub8(&mut self, a: u8, b: u8, c: bool) -> u8 {
        let ci = (c && (self.f & FLAG_C != 0)) as i16;
        let r = a as i16 - b as i16 - ci;
        let v = r as u8;
        self.flags8(v, r < 0, ((a ^ b) & (a ^ v) & 0x80) != 0);
        v
    }
    fn logic(&mut self, v: u8) -> u8 {
        self.f = (self.f & !(FLAG_S | FLAG_Z))
            | if v & 0x80 != 0 { FLAG_S } else { 0 }
            | if v == 0 { FLAG_Z } else { 0 };
        v
    }
    fn flags16(&mut self, a: u16, b: u16, r: u32, sub: bool) -> u16 {
        let v = r as u16;
        let ov = if sub {
            ((a ^ b) & (a ^ v) & 0x8000) != 0
        } else {
            (!(a ^ b) & (a ^ v) & 0x8000) != 0
        };
        self.f = (self.f & !(FLAG_S | FLAG_O | FLAG_C | FLAG_Z))
            | if v & 0x8000 != 0 { FLAG_S } else { 0 }
            | if ov { FLAG_O } else { 0 }
            | if r > 0xffff { FLAG_C } else { 0 }
            | if v == 0 { FLAG_Z } else { 0 };
        v
    }
    fn add16(&mut self, a: u16, b: u16, c: bool) -> u16 {
        let r = a as u32 + b as u32 + u32::from(c && self.f & FLAG_C != 0);
        self.flags16(a, b, r, false)
    }
    fn sub16(&mut self, a: u16, b: u16, c: bool) -> u16 {
        let borrow = u32::from(c && self.f & FLAG_C != 0);
        let r = (a as u32).wrapping_sub(b as u32 + borrow);
        self.flags16(a, b, r, true)
    }
    fn push8(&mut self, bus: &mut impl Bus, v: u8) {
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp as u32, v)
    }
    fn pop8(&mut self, bus: &mut impl Bus) -> u8 {
        let v = bus.read(self.sp as u32);
        self.sp = self.sp.wrapping_add(1);
        v
    }
    fn push16(&mut self, bus: &mut impl Bus, v: u16) {
        self.push8(bus, (v >> 8) as u8);
        self.push8(bus, v as u8)
    }
    fn pop16(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.pop8(bus);
        let hi = self.pop8(bus);
        u16::from_le_bytes([lo, hi])
    }
    fn jump(&mut self, a: u16) {
        self.v = self.u;
        self.pc = a
    }
    fn call(&mut self, bus: &mut impl Bus, a: u16) {
        self.push8(bus, self.v);
        self.push16(bus, self.pc);
        self.jump(a)
    }

    pub fn step(&mut self, bus: &mut impl Bus) -> u32 {
        if let Some(vector) = self.irq {
            if self.f & (FLAG_I | FLAG_D) == 0 && self.u == self.v {
                self.halted = false;
                self.push8(bus, self.v);
                self.push16(bus, self.pc);
                self.push8(bus, self.f);
                self.f |= FLAG_I;
                self.v = 0;
                self.pc = self.read16(bus, (vector as u32) * 2);
                self.cycles += 28;
                return 28;
            }
        }
        if self.halted {
            self.cycles += 8;
            return 8;
        }
        let op = self.fetch(bus);
        let c = match op {
            0x00..=0x3f => {
                let group = op >> 3;
                let source = op & 7;
                let rhs = match source {
                    0 => self.a(),
                    1 => self.b(),
                    2 => self.fetch(bus),
                    3 => bus.read(self.addr_ihl()),
                    4 => {
                        let a = self.addr_n(bus);
                        bus.read(a)
                    }
                    5 => {
                        let a = self.addr_i16(bus);
                        bus.read(a)
                    }
                    6 => bus.read(self.addr_x()),
                    _ => bus.read(self.addr_y()),
                };
                let a = self.a();
                match group {
                    0 => {
                        let v = self.add8(a, rhs, false);
                        self.set_a(v)
                    }
                    1 => {
                        let v = self.add8(a, rhs, true);
                        self.set_a(v)
                    }
                    2 => {
                        let v = self.sub8(a, rhs, false);
                        self.set_a(v)
                    }
                    3 => {
                        let v = self.sub8(a, rhs, true);
                        self.set_a(v)
                    }
                    4 => {
                        let v = self.logic(a & rhs);
                        self.set_a(v)
                    }
                    5 => {
                        let v = self.logic(a | rhs);
                        self.set_a(v)
                    }
                    6 => {
                        self.sub8(a, rhs, false);
                    }
                    _ => {
                        let v = self.logic(a ^ rhs);
                        self.set_a(v)
                    }
                };
                if source == 5 {
                    16
                } else if source == 4 {
                    12
                } else {
                    8
                }
            }
            0x40..=0x5f => {
                let dst = (op >> 3) & 3;
                let src = op & 7;
                let v = match src {
                    0 => self.a(),
                    1 => self.b(),
                    2 => self.l(),
                    3 => self.h(),
                    4 => {
                        let a = self.addr_n(bus);
                        bus.read(a)
                    }
                    5 => bus.read(self.addr_ihl()),
                    6 => bus.read(self.addr_x()),
                    _ => bus.read(self.addr_y()),
                };
                match dst {
                    0 => self.set_a(v),
                    1 => self.set_b(v),
                    2 => self.set_l(v),
                    _ => self.set_h(v),
                };
                if src == 4 {
                    12
                } else if src >= 5 {
                    8
                } else {
                    4
                }
            }
            0x60..=0x7b => {
                let slot = (op >> 3) & 3;
                let addr = match slot {
                    0 => self.addr_x(),
                    1 => self.addr_ihl(),
                    2 => self.addr_y(),
                    _ => self.addr_n(bus),
                };
                let v = match op & 7 {
                    0 => self.a(),
                    1 => self.b(),
                    2 => self.l(),
                    3 => self.h(),
                    4 => {
                        let a = self.addr_n(bus);
                        bus.read(a)
                    }
                    5 => bus.read(self.addr_ihl()),
                    6 => bus.read(self.addr_x()),
                    _ => bus.read(self.addr_y()),
                };
                bus.write(addr, v);
                12
            }
            0x80..=0x8f => {
                let dec = op >= 0x88;
                let id = op & 7;
                match id {
                    0 => {
                        let v = if dec {
                            self.sub8(self.a(), 1, false)
                        } else {
                            self.add8(self.a(), 1, false)
                        };
                        self.set_a(v)
                    }
                    1 => {
                        let v = if dec {
                            self.sub8(self.b(), 1, false)
                        } else {
                            self.add8(self.b(), 1, false)
                        };
                        self.set_b(v)
                    }
                    2 => {
                        let v = if dec {
                            self.sub8(self.l(), 1, false)
                        } else {
                            self.add8(self.l(), 1, false)
                        };
                        self.set_l(v)
                    }
                    3 => {
                        let v = if dec {
                            self.sub8(self.h(), 1, false)
                        } else {
                            self.add8(self.h(), 1, false)
                        };
                        self.set_h(v)
                    }
                    4 => {
                        self.n = if dec {
                            self.n.wrapping_sub(1)
                        } else {
                            self.n.wrapping_add(1)
                        }
                    }
                    5 | 6 => {
                        let a = if id == 5 {
                            self.addr_n(bus)
                        } else {
                            self.addr_ihl()
                        };
                        let old = bus.read(a);
                        let v = if dec {
                            old.wrapping_sub(1)
                        } else {
                            old.wrapping_add(1)
                        };
                        bus.write(a, v);
                        self.logic(v);
                    }
                    _ => {
                        self.sp = if dec {
                            self.sp.wrapping_sub(1)
                        } else {
                            self.sp.wrapping_add(1)
                        }
                    }
                };
                8
            }
            0x90..=0x93 => {
                let id = op & 3;
                let old = [self.ba, self.hl, self.x, self.y][id as usize];
                let v = self.add16(old, 1, false);
                match id {
                    0 => self.ba = v,
                    1 => self.hl = v,
                    2 => self.x = v,
                    _ => self.y = v,
                };
                8
            }
            0x94 => {
                self.logic(self.a() & self.b());
                12
            }
            0x95 => {
                let a = self.addr_ihl();
                let k = self.fetch(bus);
                let v = bus.read(a);
                self.logic(v & k);
                12
            }
            0x96 => {
                let k = self.fetch(bus);
                self.logic(self.a() & k);
                12
            }
            0x97 => {
                let k = self.fetch(bus);
                self.logic(self.b() & k);
                12
            }
            0x98..=0x9b => {
                let id = op & 3;
                let old = [self.ba, self.hl, self.x, self.y][id as usize];
                let v = self.sub16(old, 1, false);
                match id {
                    0 => self.ba = v,
                    1 => self.hl = v,
                    2 => self.x = v,
                    _ => self.y = v,
                };
                8
            }
            0x9c => {
                self.f &= self.fetch(bus);
                12
            }
            0x9d => {
                self.f |= self.fetch(bus);
                12
            }
            0x9e => {
                self.f ^= self.fetch(bus);
                12
            }
            0x9f => {
                self.f = self.fetch(bus);
                12
            }
            0xa0..=0xa3 => {
                let v = [self.ba, self.hl, self.x, self.y][(op & 3) as usize];
                self.push16(bus, v);
                16
            }
            0xa4 => {
                self.push8(bus, self.n);
                12
            }
            0xa5 => {
                self.push8(bus, self.i);
                12
            }
            0xa6 => {
                self.push8(bus, self.xi);
                self.push8(bus, self.yi);
                16
            }
            0xa7 => {
                self.push8(bus, self.f);
                12
            }
            0xa8..=0xab => {
                let v = self.pop16(bus);
                match op & 3 {
                    0 => self.ba = v,
                    1 => self.hl = v,
                    2 => self.x = v,
                    _ => self.y = v,
                };
                12
            }
            0xac => {
                self.n = self.pop8(bus);
                12
            }
            0xad => {
                self.i = self.pop8(bus);
                12
            }
            0xae => {
                self.yi = self.pop8(bus);
                self.xi = self.pop8(bus);
                16
            }
            0xaf => {
                self.f = self.pop8(bus);
                12
            }
            0xb0 => {
                let v = self.fetch(bus);
                self.set_a(v);
                8
            }
            0xb1 => {
                let v = self.fetch(bus);
                self.set_b(v);
                8
            }
            0xb2 => {
                let v = self.fetch(bus);
                self.set_l(v);
                8
            }
            0xb3 => {
                let v = self.fetch(bus);
                self.set_h(v);
                8
            }
            0xb4 => {
                self.n = self.fetch(bus);
                8
            }
            0xb5..=0xb7 => {
                let a = match op {
                    0xb5 => self.addr_ihl(),
                    0xb6 => self.addr_x(),
                    _ => self.addr_y(),
                };
                let v = self.fetch(bus);
                bus.write(a, v);
                12
            }
            0xb8..=0xbb => {
                let a = self.addr_i16(bus);
                let v = self.read16(bus, a);
                match op & 3 {
                    0 => self.ba = v,
                    1 => self.hl = v,
                    2 => self.x = v,
                    _ => self.y = v,
                };
                20
            }
            0xbc..=0xbf => {
                let a = self.addr_i16(bus);
                let v = [self.ba, self.hl, self.x, self.y][(op & 3) as usize];
                self.write16(bus, a, v);
                20
            }
            0xc0..=0xc3 => {
                let k = self.fetch16(bus);
                let id = op & 3;
                let old = [self.ba, self.hl, self.x, self.y][id as usize];
                let v = self.add16(old, k, false);
                match id {
                    0 => self.ba = v,
                    1 => self.hl = v,
                    2 => self.x = v,
                    _ => self.y = v,
                };
                12
            }
            0xc4..=0xc7 => {
                let v = self.fetch16(bus);
                match op & 3 {
                    0 => self.ba = v,
                    1 => self.hl = v,
                    2 => self.x = v,
                    _ => self.y = v,
                };
                12
            }
            0xc8 => {
                std::mem::swap(&mut self.ba, &mut self.hl);
                8
            }
            0xc9 => {
                std::mem::swap(&mut self.ba, &mut self.x);
                8
            }
            0xca => {
                std::mem::swap(&mut self.ba, &mut self.y);
                8
            }
            0xcb => {
                std::mem::swap(&mut self.ba, &mut self.sp);
                8
            }
            0xcc => {
                self.ba = self.ba.rotate_left(8);
                8
            }
            0xcd => {
                let a = self.addr_ihl();
                let v = bus.read(a);
                bus.write(a, self.a());
                self.set_a(v);
                8
            }
            0xce => self.step_ce(bus),
            0xcf => self.step_cf(bus),
            0xd0..=0xd7 => {
                let k = self.fetch16(bus);
                let id = op & 3;
                let old = [self.ba, self.hl, self.x, self.y][id as usize];
                let v = self.sub16(old, k, false);
                if op < 0xd4 {
                    match id {
                        0 => self.ba = v,
                        1 => self.hl = v,
                        2 => self.x = v,
                        _ => self.y = v,
                    }
                };
                12
            }
            0xd8..=0xdd => {
                let a = self.addr_n(bus);
                let k = self.fetch(bus);
                let old = bus.read(a);
                match op {
                    0xd8 => {
                        let v = self.logic(old & k);
                        bus.write(a, v)
                    }
                    0xd9 => {
                        let v = self.logic(old | k);
                        bus.write(a, v)
                    }
                    0xda => {
                        let v = self.logic(old ^ k);
                        bus.write(a, v)
                    }
                    0xdb => {
                        self.sub8(old, k, false);
                    }
                    0xdc => {
                        self.logic(old & k);
                    }
                    _ => bus.write(a, k),
                };
                16
            }
            0xde => {
                self.set_a((self.a() & 0x0f) | (self.b() << 4));
                8
            }
            0xdf => {
                self.ba = (self.a() as i8 as i16) as u16;
                8
            }
            0xe0..=0xe7 => {
                let d = self.fetch(bus) as i8;
                let take = match op & 7 {
                    0 => self.f & FLAG_C != 0,
                    1 => self.f & FLAG_C == 0,
                    2 => self.f & FLAG_Z != 0,
                    3 => self.f & FLAG_Z == 0,
                    4 => self.f & FLAG_C != 0,
                    5 => self.f & FLAG_C == 0,
                    6 => self.f & FLAG_Z != 0,
                    _ => self.f & FLAG_Z == 0,
                };
                if take {
                    let t = self.pc.wrapping_add_signed(d as i16 - 1);
                    if op < 0xe4 {
                        self.call(bus, t)
                    } else {
                        self.jump(t)
                    }
                };
                12
            }
            0xe8..=0xef => {
                let d = self.fetch16(bus);
                let take = match op & 3 {
                    0 => self.f & FLAG_C != 0,
                    1 => self.f & FLAG_C == 0,
                    2 => self.f & FLAG_Z != 0,
                    _ => self.f & FLAG_Z == 0,
                };
                if take {
                    let t = self.pc.wrapping_add(d).wrapping_sub(1);
                    if op < 0xec {
                        self.call(bus, t)
                    } else {
                        self.jump(t)
                    }
                }
                12
            }
            0xf0 => {
                let d = self.fetch(bus) as i8;
                self.call(bus, self.pc.wrapping_add_signed(d as i16 - 1));
                20
            }
            0xf1 => {
                let d = self.fetch(bus) as i8;
                self.jump(self.pc.wrapping_add_signed(d as i16 - 1));
                8
            }
            0xf2 => {
                let d = self.fetch16(bus);
                self.call(bus, self.pc.wrapping_add(d).wrapping_sub(1));
                24
            }
            0xf3 => {
                let d = self.fetch16(bus);
                self.jump(self.pc.wrapping_add(d).wrapping_sub(1));
                12
            }
            0xf4 => {
                self.jump(self.hl);
                8
            }
            0xf5 => {
                let d = self.fetch(bus) as i8;
                self.set_b(self.b().wrapping_sub(1));
                if self.b() != 0 {
                    self.jump(self.pc.wrapping_add_signed(d as i16 - 1))
                }
                8
            }
            0xf6 => {
                self.set_a(self.a().rotate_left(4));
                8
            }
            0xf7 => {
                let a = self.addr_ihl();
                let v = bus.read(a).rotate_left(4);
                bus.write(a, v);
                12
            }
            0xf8 => {
                self.pc = self.pop16(bus);
                self.v = self.pop8(bus);
                self.u = self.v;
                8
            }
            0xf9 => {
                self.f = self.pop8(bus);
                self.pc = self.pop16(bus);
                self.v = self.pop8(bus);
                self.u = self.v;
                8
            }
            0xfa => {
                self.pc = self.pop16(bus).wrapping_add(2);
                self.v = self.pop8(bus);
                self.u = self.v;
                8
            }
            0xfb => {
                let a = self.addr_i16(bus);
                let t = self.read16(bus, a);
                self.call(bus, t);
                20
            }
            0xfc => {
                let i = self.fetch(bus) & 0xfe;
                let t = self.read16(bus, i as u32);
                self.call(bus, t);
                self.push8(bus, self.f);
                20
            }
            0xfd => {
                let i = self.fetch(bus) & 0xfe;
                let t = self.read16(bus, i as u32);
                self.jump(t);
                12
            }
            0xff => 8,
            _ => 8,
        };
        self.cycles += c as u64;
        c
    }

    fn ce_indexed(&mut self, bus: &mut impl Bus, mode: u8) -> u32 {
        match mode {
            0 => (self.xi as u32) << 16 | self.x.wrapping_add(self.fetch(bus) as u16) as u32,
            1 => (self.yi as u32) << 16 | self.y.wrapping_add(self.fetch(bus) as u16) as u32,
            2 => (self.xi as u32) << 16 | self.x.wrapping_add(self.l() as u16) as u32,
            _ => (self.yi as u32) << 16 | self.y.wrapping_add(self.l() as u16) as u32,
        }
    }

    fn ce_condition(&self, code: u8) -> bool {
        match code & 15 {
            0 => (self.f & (FLAG_S | FLAG_O)) == FLAG_S || (self.f & (FLAG_S | FLAG_O)) == FLAG_O,
            1 => {
                self.f & FLAG_Z != 0
                    || (self.f & (FLAG_S | FLAG_O)) == FLAG_S
                    || (self.f & (FLAG_S | FLAG_O)) == FLAG_O
            }
            2 => self.f & FLAG_Z == 0 && (self.f & FLAG_S != 0) == (self.f & FLAG_O != 0),
            3 => (self.f & FLAG_S != 0) == (self.f & FLAG_O != 0),
            4 => self.f & FLAG_O != 0,
            5 => self.f & FLAG_O == 0,
            6 => self.f & FLAG_S == 0,
            7 => self.f & FLAG_S != 0,
            8 => self.e & 0x80 == 0,
            9 => self.e & 0x40 == 0,
            10 => self.e & 0x20 == 0,
            11 => self.e & 0x10 == 0,
            12 => self.e & 0x80 != 0,
            13 => self.e & 0x40 != 0,
            14 => self.e & 0x20 != 0,
            _ => self.e & 0x10 != 0,
        }
    }

    fn step_ce(&mut self, bus: &mut impl Bus) -> u32 {
        let op = self.fetch(bus);
        match op {
            0x00..=0x3f => {
                let group = op >> 3;
                let form = op & 7;
                let (left, right, target) = if form < 4 {
                    let a = self.ce_indexed(bus, form);
                    (self.a(), bus.read(a), None)
                } else {
                    let a = self.addr_ihl();
                    let rhs = match form {
                        4 => self.a(),
                        5 => self.fetch(bus),
                        6 => bus.read(self.addr_x()),
                        _ => bus.read(self.addr_y()),
                    };
                    (bus.read(a), rhs, Some(a))
                };
                let result = match group {
                    0 => self.add8(left, right, false),
                    1 => self.add8(left, right, true),
                    2 => self.sub8(left, right, false),
                    3 => self.sub8(left, right, true),
                    4 => self.logic(left & right),
                    5 => self.logic(left | right),
                    6 => {
                        self.sub8(left, right, false);
                        left
                    }
                    _ => self.logic(left ^ right),
                };
                if group != 6 {
                    if let Some(a) = target {
                        bus.write(a, result)
                    } else {
                        self.set_a(result)
                    }
                }
            }
            0x40..=0x5f => {
                let reg = (op >> 3) & 3;
                let store = op & 4 != 0;
                let a = self.ce_indexed(bus, op & 3);
                if store {
                    let v = [self.a(), self.b(), self.l(), self.h()][reg as usize];
                    bus.write(a, v)
                } else {
                    let v = bus.read(a);
                    match reg {
                        0 => self.set_a(v),
                        1 => self.set_b(v),
                        2 => self.set_l(v),
                        _ => self.set_h(v),
                    }
                }
            }
            0x60..=0x63 | 0x68..=0x6b | 0x78..=0x7b => {
                let src = self.ce_indexed(bus, op & 3);
                let dst = match op & 0x78 {
                    0x60 => self.addr_ihl(),
                    0x68 => self.addr_x(),
                    _ => self.addr_y(),
                };
                let v = bus.read(src);
                bus.write(dst, v);
            }
            0x80..=0x9f => {
                let kind = (op >> 2) & 7;
                let id = op & 3;
                let mut address = None;
                let old = match id {
                    0 => self.a(),
                    1 => self.b(),
                    2 => {
                        let a = self.addr_n(bus);
                        address = Some(a);
                        bus.read(a)
                    }
                    _ => {
                        let a = self.addr_ihl();
                        address = Some(a);
                        bus.read(a)
                    }
                };
                let carry = self.f & FLAG_C != 0;
                let v = match kind {
                    0 | 1 => old << 1,
                    2 => (old as i8 >> 1) as u8,
                    3 => old >> 1,
                    4 => (old << 1) | carry as u8,
                    5 => old.rotate_left(1),
                    6 => (old >> 1) | ((carry as u8) << 7),
                    _ => old.rotate_right(1),
                };
                self.flags8(
                    v,
                    if matches!(kind, 0 | 1 | 4 | 5) {
                        old & 0x80 != 0
                    } else {
                        old & 1 != 0
                    },
                    false,
                );
                if let Some(a) = address {
                    bus.write(a, v)
                } else if id == 0 {
                    self.set_a(v)
                } else {
                    self.set_b(v)
                }
            }
            0xa0..=0xa7 => {
                let id = op & 3;
                let mut address = None;
                let old = match id {
                    0 => self.a(),
                    1 => self.b(),
                    2 => {
                        let a = self.addr_n(bus);
                        address = Some(a);
                        bus.read(a)
                    }
                    _ => {
                        let a = self.addr_ihl();
                        address = Some(a);
                        bus.read(a)
                    }
                };
                let v = if op & 4 == 0 {
                    self.logic(!old)
                } else {
                    self.sub8(0, old, false)
                };
                if let Some(a) = address {
                    bus.write(a, v)
                } else if id == 0 {
                    self.set_a(v)
                } else {
                    self.set_b(v)
                }
            }
            0xa8 => self.ba = (self.a() as i8 as i16) as u16,
            0xae => self.halted = true,
            0xb0..=0xbf => {
                let k = self.fetch(bus);
                let id = op & 3;
                let old = match id {
                    0 => self.b(),
                    1 => self.l(),
                    2 => self.h(),
                    _ => self.n,
                };
                let group = (op - 0xb0) >> 2;
                let v = match group {
                    0 => self.logic(old & k),
                    1 => self.logic(old | k),
                    2 => self.logic(old ^ k),
                    _ => {
                        self.sub8(old, k, false);
                        old
                    }
                };
                if group < 3 {
                    match id {
                        0 => self.set_b(v),
                        1 => self.set_l(v),
                        2 => self.set_h(v),
                        _ => self.n = v,
                    }
                }
            }
            0xc0 => self.set_a(self.n),
            0xc1 => self.set_a(self.f),
            0xc2 => self.n = self.a(),
            0xc3 => self.f = self.a(),
            0xc4 => self.u = self.fetch(bus),
            0xc5 => self.i = self.fetch(bus),
            0xc6 => self.xi = self.fetch(bus),
            0xc7 => self.yi = self.fetch(bus),
            0xc8 => self.set_a(self.v),
            0xc9 => self.set_a(self.i),
            0xca => self.set_a(self.xi),
            0xcb => self.set_a(self.yi),
            0xcc => self.u = self.a(),
            0xcd => self.i = self.a(),
            0xce => self.xi = self.a(),
            0xcf => self.yi = self.a(),
            0xd0..=0xd7 => {
                let a = self.addr_i16(bus);
                if op < 0xd4 {
                    let v = bus.read(a);
                    match op & 3 {
                        0 => self.set_a(v),
                        1 => self.set_b(v),
                        2 => self.set_l(v),
                        _ => self.set_h(v),
                    }
                } else {
                    let v = [self.a(), self.b(), self.l(), self.h()][(op & 3) as usize];
                    bus.write(a, v)
                }
            }
            0xd8 => self.hl = self.l() as u16 * self.a() as u16,
            0xd9 => {
                if self.a() != 0 {
                    let q = self.hl / self.a() as u16;
                    self.hl = ((self.hl % self.a() as u16) << 8) | (q & 0xff)
                }
            }
            0xe0..=0xff => {
                let d = self.fetch(bus) as i8;
                if self.ce_condition(op) {
                    let t = self.pc.wrapping_add_signed(d as i16 - 1);
                    if op < 0xf0 {
                        self.jump(t)
                    } else {
                        self.call(bus, t)
                    }
                }
            }
            _ => {}
        }
        16
    }

    fn step_cf(&mut self, bus: &mut impl Bus) -> u32 {
        let op = self.fetch(bus);
        match op {
            0x00..=0x0f => {
                let rhs = [self.ba, self.hl, self.x, self.y][(op & 3) as usize];
                self.ba = match (op >> 2) & 3 {
                    0 => self.add16(self.ba, rhs, false),
                    1 => self.add16(self.ba, rhs, true),
                    2 => self.sub16(self.ba, rhs, false),
                    _ => self.sub16(self.ba, rhs, true),
                }
            }
            0x18..=0x1b => {
                let rhs = [self.ba, self.hl, self.x, self.y][(op & 3) as usize];
                self.sub16(self.ba, rhs, false);
            }
            0x20..=0x2f => {
                let rhs = [self.ba, self.hl, self.x, self.y][(op & 3) as usize];
                self.hl = match (op >> 2) & 3 {
                    0 => self.add16(self.hl, rhs, false),
                    1 => self.add16(self.hl, rhs, true),
                    2 => self.sub16(self.hl, rhs, false),
                    _ => self.sub16(self.hl, rhs, true),
                }
            }
            0x38..=0x3b => {
                let rhs = [self.ba, self.hl, self.x, self.y][(op & 3) as usize];
                self.sub16(self.hl, rhs, false);
            }
            0x40 => self.x = self.add16(self.x, self.ba, false),
            0x41 => self.x = self.add16(self.x, self.hl, false),
            0x42 => self.y = self.add16(self.y, self.ba, false),
            0x43 => self.y = self.add16(self.y, self.hl, false),
            0x44 => self.sp = self.add16(self.sp, self.ba, false),
            0x45 => self.sp = self.add16(self.sp, self.hl, false),
            0x48 => self.x = self.sub16(self.x, self.ba, false),
            0x49 => self.x = self.sub16(self.x, self.hl, false),
            0x4a => self.y = self.sub16(self.y, self.ba, false),
            0x4b => self.y = self.sub16(self.y, self.hl, false),
            0x4c => self.sp = self.sub16(self.sp, self.ba, false),
            0x4d => self.sp = self.sub16(self.sp, self.hl, false),
            0x5c => {
                self.sub16(self.sp, self.ba, false);
            }
            0x5d => {
                self.sub16(self.sp, self.hl, false);
            }
            0x60..=0x63 => {
                let k = self.fetch16(bus);
                let old = [self.ba, self.hl, self.x, self.y][(op & 3) as usize];
                self.add16(old, k, true);
            }
            0x68 => {
                let k = self.fetch16(bus);
                self.sp = self.add16(self.sp, k, false)
            }
            0x6a => {
                let k = self.fetch16(bus);
                self.sp = self.sub16(self.sp, k, false)
            }
            0x6c => {
                let k = self.fetch16(bus);
                self.sub16(self.sp, k, false);
            }
            0x6e => self.sp = self.fetch16(bus),
            0x70..=0x77 => {
                let a = self.sp.wrapping_add(self.fetch(bus) as u16) as u32;
                if op < 0x74 {
                    let v = self.read16(bus, a);
                    match op & 3 {
                        0 => self.ba = v,
                        1 => self.hl = v,
                        2 => self.x = v,
                        _ => self.y = v,
                    }
                } else {
                    let v = [self.ba, self.hl, self.x, self.y][(op & 3) as usize];
                    self.write16(bus, a, v)
                }
            }
            0x78 => {
                let a = self.addr_i16(bus);
                self.sp = self.read16(bus, a)
            }
            0x7c => {
                let a = self.addr_i16(bus);
                self.write16(bus, a, self.sp)
            }
            0xb0 => self.push8(bus, self.a()),
            0xb1 => self.push8(bus, self.b()),
            0xb2 => self.push8(bus, self.l()),
            0xb3 => self.push8(bus, self.h()),
            0xb4 => {
                let v = self.pop8(bus);
                self.set_a(v)
            }
            0xb5 => {
                let v = self.pop8(bus);
                self.set_b(v)
            }
            0xb6 => {
                let v = self.pop8(bus);
                self.set_l(v)
            }
            0xb7 => {
                let v = self.pop8(bus);
                self.set_h(v)
            }
            0xb8 | 0xb9 => {
                self.push16(bus, self.ba);
                self.push16(bus, self.hl);
                self.push16(bus, self.x);
                self.push16(bus, self.y);
                self.push8(bus, self.n);
                if op == 0xb9 {
                    self.push8(bus, self.i);
                    self.push8(bus, self.xi);
                    self.push8(bus, self.yi)
                }
            }
            0xbc | 0xbd => {
                if op == 0xbd {
                    self.yi = self.pop8(bus);
                    self.xi = self.pop8(bus);
                    self.i = self.pop8(bus)
                }
                self.n = self.pop8(bus);
                self.y = self.pop16(bus);
                self.x = self.pop16(bus);
                self.hl = self.pop16(bus);
                self.ba = self.pop16(bus)
            }
            0xc0..=0xc7 => {
                let a = self.addr_ihl();
                if op < 0xc4 {
                    let v = self.read16(bus, a);
                    match op & 3 {
                        0 => self.ba = v,
                        1 => self.hl = v,
                        2 => self.x = v,
                        _ => self.y = v,
                    }
                } else {
                    let v = [self.ba, self.hl, self.x, self.y][(op & 3) as usize];
                    self.write16(bus, a, v)
                }
            }
            0xd0..=0xdf => {
                let a = if op < 0xd8 {
                    self.addr_x()
                } else {
                    self.addr_y()
                };
                if op & 4 == 0 {
                    let v = self.read16(bus, a);
                    match op & 3 {
                        0 => self.ba = v,
                        1 => self.hl = v,
                        2 => self.x = v,
                        _ => self.y = v,
                    }
                } else {
                    let v = [self.ba, self.hl, self.x, self.y][(op & 3) as usize];
                    self.write16(bus, a, v)
                }
            }
            0xe0..=0xef => {
                let src = [self.ba, self.hl, self.x, self.y][(op & 3) as usize];
                match (op >> 2) & 3 {
                    0 => self.ba = src,
                    1 => self.hl = src,
                    2 => self.x = src,
                    _ => self.y = src,
                }
            }
            0xf0 => self.sp = self.ba,
            0xf1 => self.sp = self.hl,
            0xf2 => self.sp = self.x,
            0xf3 => self.sp = self.y,
            0xf4 => self.hl = self.sp,
            0xf5 => self.hl = self.pc,
            0xf8 => self.ba = self.sp,
            0xf9 => self.ba = self.pc,
            0xfa => self.x = self.sp,
            0xfe => self.y = self.sp,
            _ => {}
        }
        16
    }
}
