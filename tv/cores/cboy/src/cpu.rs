//! The SM83 CPU: every opcode and every CB opcode, with each memory access and internal delay
//! costing one M-cycle on the bus, interrupts (five M-cycles to dispatch), EI's one-instruction
//! delay, HALT and the HALT bug.

use serde::{Deserialize, Serialize};

use crate::bus::Bus;

const Z: u8 = 0x80;
const N: u8 = 0x40;
const H: u8 = 0x20;
const C: u8 = 0x10;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cpu {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
    pub ime: bool,
    /// EI takes effect after the instruction that follows it.
    ei_pending: bool,
    pub halted: bool,
    halt_bug: bool,
    #[serde(default)]
    ei_now: bool,
    /// An illegal opcode locks the CPU, as on hardware.
    pub locked: bool,
    /// Set when `LD B,B` runs (Mooneye's "test finished" breakpoint).
    #[serde(skip)]
    pub ld_b_b: bool,
}

impl Cpu {
    /// Everything that decides what the CPU does next (for spotting a loop that repeats itself
    /// exactly, gb::WaitLoop).
    pub fn same_state(&self, o: &Cpu) -> bool {
        (self.a, self.f, self.b, self.c, self.d, self.e, self.h, self.l, self.sp, self.pc)
            == (o.a, o.f, o.b, o.c, o.d, o.e, o.h, o.l, o.sp, o.pc)
            && (self.ime, self.ei_pending, self.halted, self.halt_bug, self.ei_now, self.locked)
                == (o.ime, o.ei_pending, o.halted, o.halt_bug, o.ei_now, o.locked)
    }

    /// Not waiting on anything: running instructions one after another.
    pub fn plain(&self) -> bool {
        !self.halted && !self.locked && !self.ei_pending && !self.halt_bug && !self.ld_b_b
    }
}

impl Default for Cpu {
    /// The DMG's registers as its boot ROM leaves them.
    fn default() -> Self {
        Cpu {
            a: 0x01,
            f: 0xB0,
            b: 0x00,
            c: 0x13,
            d: 0x00,
            e: 0xD8,
            h: 0x01,
            l: 0x4D,
            sp: 0xFFFE,
            pc: 0x0100,
            ime: false,
            ei_pending: false,
            halted: false,
            halt_bug: false,
            ei_now: false,
            locked: false,
            ld_b_b: false,
        }
    }
}

impl Cpu {
    /// The CGB's registers as its boot ROM leaves them for a colour cartridge.
    pub fn cgb() -> Cpu {
        Cpu { a: 0x11, f: 0x80, b: 0, c: 0, d: 0xFF, e: 0x56, h: 0, l: 0x0D, ..Cpu::default() }
    }

    pub fn bc(&self) -> u16 { u16::from(self.b) << 8 | u16::from(self.c) }
    pub fn de(&self) -> u16 { u16::from(self.d) << 8 | u16::from(self.e) }
    pub fn hl(&self) -> u16 { u16::from(self.h) << 8 | u16::from(self.l) }
    fn set_bc(&mut self, v: u16) { self.b = (v >> 8) as u8; self.c = v as u8; }
    fn set_de(&mut self, v: u16) { self.d = (v >> 8) as u8; self.e = v as u8; }
    fn set_hl(&mut self, v: u16) { self.h = (v >> 8) as u8; self.l = v as u8; }
    fn flag(&self, f: u8) -> bool { self.f & f != 0 }
    fn set_flags(&mut self, z: bool, n: bool, h: bool, c: bool) {
        self.f = (if z { Z } else { 0 }) | (if n { N } else { 0 }) | (if h { H } else { 0 })
            | (if c { C } else { 0 });
    }

    fn fetch(&mut self, bus: &mut Bus) -> u8 {
        let v = bus.read(self.pc);
        if self.halt_bug {
            self.halt_bug = false;
        } else {
            self.pc = self.pc.wrapping_add(1);
        }
        v
    }

    fn fetch16(&mut self, bus: &mut Bus) -> u16 {
        let lo = self.fetch(bus);
        let hi = self.fetch(bus);
        u16::from(hi) << 8 | u16::from(lo)
    }

    fn push(&mut self, bus: &mut Bus, v: u16) {
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, (v >> 8) as u8);
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, v as u8);
    }

    fn pop(&mut self, bus: &mut Bus) -> u16 {
        let lo = bus.read(self.sp);
        self.sp = self.sp.wrapping_add(1);
        let hi = bus.read(self.sp);
        self.sp = self.sp.wrapping_add(1);
        u16::from(hi) << 8 | u16::from(lo)
    }

    fn reg(&mut self, bus: &mut Bus, i: u8) -> u8 {
        match i {
            0 => self.b,
            1 => self.c,
            2 => self.d,
            3 => self.e,
            4 => self.h,
            5 => self.l,
            6 => bus.read(self.hl()),
            _ => self.a,
        }
    }

    fn set_reg(&mut self, bus: &mut Bus, i: u8, v: u8) {
        match i {
            0 => self.b = v,
            1 => self.c = v,
            2 => self.d = v,
            3 => self.e = v,
            4 => self.h = v,
            5 => self.l = v,
            6 => bus.write(self.hl(), v),
            _ => self.a = v,
        }
    }

    fn rp(&self, i: u8) -> u16 {
        match i {
            0 => self.bc(),
            1 => self.de(),
            2 => self.hl(),
            _ => self.sp,
        }
    }

    fn set_rp(&mut self, i: u8, v: u16) {
        match i {
            0 => self.set_bc(v),
            1 => self.set_de(v),
            2 => self.set_hl(v),
            _ => self.sp = v,
        }
    }

    fn cond(&self, i: u8) -> bool {
        match i {
            0 => !self.flag(Z),
            1 => self.flag(Z),
            2 => !self.flag(C),
            _ => self.flag(C),
        }
    }

    fn alu(&mut self, op: u8, v: u8) {
        let a = self.a;
        let carry = u8::from(self.flag(C));
        match op {
            0 | 1 => {
                let c = if op == 1 { carry } else { 0 };
                let r = u16::from(a) + u16::from(v) + u16::from(c);
                self.a = r as u8;
                self.set_flags(self.a == 0, false, (a & 0xF) + (v & 0xF) + c > 0xF, r > 0xFF);
            }
            2 | 3 | 7 => {
                let c = if op == 3 { carry } else { 0 };
                let r = i16::from(a) - i16::from(v) - i16::from(c);
                let res = r as u8;
                self.set_flags(res == 0, true, (a & 0xF) < (v & 0xF) + c, r < 0);
                if op != 7 {
                    self.a = res;
                }
            }
            4 => {
                self.a &= v;
                self.set_flags(self.a == 0, false, true, false);
            }
            5 => {
                self.a ^= v;
                self.set_flags(self.a == 0, false, false, false);
            }
            _ => {
                self.a |= v;
                self.set_flags(self.a == 0, false, false, false);
            }
        }
    }

    fn add_sp_e(&mut self, e: u8) -> u16 {
        let sp = self.sp;
        let ev = i16::from(e as i8) as u16;
        let low = (sp & 0xFF) + u16::from(e);
        self.set_flags(false, false, (sp & 0xF) + u16::from(e & 0xF) > 0xF, low > 0xFF);
        sp.wrapping_add(ev)
    }

    /// Dispatches a pending interrupt, if any. Returns true when one was taken.
    fn interrupt(&mut self, bus: &mut Bus) -> bool {
        let pending = bus.ie & bus.iflag & 0x1F;
        if pending == 0 {
            return false;
        }
        self.halted = false;
        if !self.ime {
            return false;
        }
        self.ime = false;
        bus.tick();
        bus.tick();
        let pc = self.pc;
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, (pc >> 8) as u8);
        // IE is read after the high byte is pushed: a push that overwrote IE can cancel it.
        let pending = bus.ie & bus.iflag & 0x1F;
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, pc as u8);
        if pending == 0 {
            self.pc = 0;
        } else {
            let bit = pending.trailing_zeros() as u8;
            bus.iflag &= !(1 << bit);
            bus.irq_taken[bit as usize] = bus.irq_taken[bit as usize].wrapping_add(1);
            self.pc = 0x40 + u16::from(bit) * 8;
        }
        bus.tick();
        true
    }

    /// One instruction (or one M-cycle of HALT, or an interrupt dispatch).
    pub fn step(&mut self, bus: &mut Bus) {
        if self.locked {
            bus.tick();
            return;
        }
        if self.halted {
            if bus.ie & bus.iflag & 0x1F == 0 {
                bus.tick();
                return;
            }
            self.halted = false;
        }
        if self.interrupt(bus) {
            return;
        }
        // EI's effect lands after the instruction that follows it (that instruction cannot be
        // interrupted); a DI in that slot cancels it.
        let ei = self.ei_pending;
        self.ei_pending = false;
        self.ei_now = ei;
        bus.trace_pc = self.pc;
        bus.trace_bank = bus.cart.rom_bank;
        let op = self.fetch(bus);
        self.execute(bus, op);
        self.ei_now = false;
        if ei && op != 0xF3 {
            self.ime = true;
        }
    }

    fn execute(&mut self, bus: &mut Bus, op: u8) {
        let x = op >> 6;
        let y = (op >> 3) & 7;
        let z = op & 7;
        match op {
            0x00 => {}
            0x08 => {
                let a = self.fetch16(bus);
                bus.write(a, self.sp as u8);
                bus.write(a.wrapping_add(1), (self.sp >> 8) as u8);
            }
            0x10 => {
                self.fetch(bus); // STOP's second byte; low-power stop itself is not modelled
                bus.stop();
            }
            0x18 => {
                let e = self.fetch(bus) as i8;
                bus.tick();
                self.pc = self.pc.wrapping_add(e as u16);
            }
            0x20 | 0x28 | 0x30 | 0x38 => {
                let e = self.fetch(bus) as i8;
                if self.cond(y - 4) {
                    bus.tick();
                    self.pc = self.pc.wrapping_add(e as u16);
                }
            }
            _ if x == 0 && z == 1 => {
                let p = y >> 1;
                if y & 1 == 0 {
                    let v = self.fetch16(bus);
                    self.set_rp(p, v);
                } else {
                    let hl = self.hl();
                    let v = self.rp(p);
                    let r = u32::from(hl) + u32::from(v);
                    let z = self.flag(Z);
                    self.set_flags(z, false, (hl & 0xFFF) + (v & 0xFFF) > 0xFFF, r > 0xFFFF);
                    self.set_hl(r as u16);
                    bus.tick();
                }
            }
            _ if x == 0 && z == 2 => {
                let addr = match y >> 1 {
                    0 => self.bc(),
                    1 => self.de(),
                    _ => self.hl(),
                };
                if y & 1 == 0 {
                    bus.write(addr, self.a);
                } else {
                    self.a = bus.read(addr);
                }
                match y >> 1 {
                    2 => self.set_hl(addr.wrapping_add(1)),
                    3 => self.set_hl(addr.wrapping_sub(1)),
                    _ => {}
                }
            }
            _ if x == 0 && z == 3 => {
                let p = y >> 1;
                let v = self.rp(p);
                self.set_rp(p, if y & 1 == 0 { v.wrapping_add(1) } else { v.wrapping_sub(1) });
                bus.tick();
            }
            _ if x == 0 && (z == 4 || z == 5) => {
                let v = self.reg(bus, y);
                let c = self.flag(C);
                let r = if z == 4 {
                    let r = v.wrapping_add(1);
                    self.set_flags(r == 0, false, v & 0xF == 0xF, c);
                    r
                } else {
                    let r = v.wrapping_sub(1);
                    self.set_flags(r == 0, true, v & 0xF == 0, c);
                    r
                };
                self.set_reg(bus, y, r);
            }
            _ if x == 0 && z == 6 => {
                let v = self.fetch(bus);
                self.set_reg(bus, y, v);
            }
            0x07 => {
                let c = self.a >> 7;
                self.a = self.a << 1 | c;
                self.set_flags(false, false, false, c != 0);
            }
            0x0F => {
                let c = self.a & 1;
                self.a = self.a >> 1 | c << 7;
                self.set_flags(false, false, false, c != 0);
            }
            0x17 => {
                let c = self.a >> 7;
                self.a = self.a << 1 | u8::from(self.flag(C));
                self.set_flags(false, false, false, c != 0);
            }
            0x1F => {
                let c = self.a & 1;
                self.a = self.a >> 1 | u8::from(self.flag(C)) << 7;
                self.set_flags(false, false, false, c != 0);
            }
            0x27 => {
                let mut a = self.a;
                let mut carry = self.flag(C);
                if !self.flag(N) {
                    if carry || a > 0x99 {
                        a = a.wrapping_add(0x60);
                        carry = true;
                    }
                    if self.flag(H) || a & 0x0F > 0x09 {
                        a = a.wrapping_add(0x06);
                    }
                } else {
                    if carry {
                        a = a.wrapping_sub(0x60);
                    }
                    if self.flag(H) {
                        a = a.wrapping_sub(0x06);
                    }
                }
                self.a = a;
                let n = self.flag(N);
                self.set_flags(a == 0, n, false, carry);
            }
            0x2F => {
                self.a = !self.a;
                self.f |= N | H;
            }
            0x37 => self.f = (self.f & Z) | C,
            0x3F => self.f = (self.f & Z) | if self.flag(C) { 0 } else { C },
            0x76 => {
                if self.ei_now && bus.ie & bus.iflag & 0x1F != 0 {
                    // EI then HALT with an interrupt already waiting: IME comes on, the interrupt
                    // is taken, and it returns to the HALT, which then halts (SameSuite
                    // ei_delay_halt). Not the HALT bug.
                    self.pc = self.pc.wrapping_sub(1);
                } else if !self.ime && bus.ie & bus.iflag & 0x1F != 0 {
                    self.halt_bug = true;
                } else {
                    self.halted = true;
                }
            }
            _ if x == 1 => {
                if op == 0x40 {
                    self.ld_b_b = true;
                }
                let v = self.reg(bus, z);
                self.set_reg(bus, y, v);
            }
            _ if x == 2 => {
                let v = self.reg(bus, z);
                self.alu(y, v);
            }
            0xC0 | 0xC8 | 0xD0 | 0xD8 => {
                bus.tick();
                if self.cond(y) {
                    self.pc = self.pop(bus);
                    bus.tick();
                }
            }
            0xC9 | 0xD9 => {
                self.pc = self.pop(bus);
                bus.tick();
                if op == 0xD9 {
                    self.ime = true;
                }
            }
            0xC1 | 0xD1 | 0xE1 | 0xF1 => {
                let v = self.pop(bus);
                match y >> 1 {
                    0 => self.set_bc(v),
                    1 => self.set_de(v),
                    2 => self.set_hl(v),
                    _ => {
                        self.a = (v >> 8) as u8;
                        self.f = v as u8 & 0xF0;
                    }
                }
            }
            0xC5 | 0xD5 | 0xE5 | 0xF5 => {
                let v = match y >> 1 {
                    0 => self.bc(),
                    1 => self.de(),
                    2 => self.hl(),
                    _ => u16::from(self.a) << 8 | u16::from(self.f),
                };
                bus.tick();
                self.push(bus, v);
            }
            0xC2 | 0xCA | 0xD2 | 0xDA => {
                let a = self.fetch16(bus);
                if self.cond(y) {
                    bus.tick();
                    self.pc = a;
                }
            }
            0xC3 => {
                let a = self.fetch16(bus);
                bus.tick();
                self.pc = a;
            }
            0xE9 => self.pc = self.hl(),
            0xC4 | 0xCC | 0xD4 | 0xDC => {
                let a = self.fetch16(bus);
                if self.cond(y) {
                    bus.tick();
                    let pc = self.pc;
                    self.push(bus, pc);
                    self.pc = a;
                }
            }
            0xCD => {
                let a = self.fetch16(bus);
                bus.tick();
                let pc = self.pc;
                self.push(bus, pc);
                self.pc = a;
            }
            _ if x == 3 && z == 6 => {
                let v = self.fetch(bus);
                self.alu(y, v);
            }
            _ if x == 3 && z == 7 => {
                bus.tick();
                let pc = self.pc;
                self.push(bus, pc);
                self.pc = u16::from(y) * 8;
            }
            0xCB => {
                let cb = self.fetch(bus);
                self.execute_cb(bus, cb);
            }
            0xE0 => {
                let a = 0xFF00 | u16::from(self.fetch(bus));
                bus.write(a, self.a);
            }
            0xF0 => {
                let a = 0xFF00 | u16::from(self.fetch(bus));
                self.a = bus.read(a);
            }
            0xE2 => bus.write(0xFF00 | u16::from(self.c), self.a),
            0xF2 => self.a = bus.read(0xFF00 | u16::from(self.c)),
            0xEA => {
                let a = self.fetch16(bus);
                bus.write(a, self.a);
            }
            0xFA => {
                let a = self.fetch16(bus);
                self.a = bus.read(a);
            }
            0xE8 => {
                let e = self.fetch(bus);
                self.sp = self.add_sp_e(e);
                bus.tick();
                bus.tick();
            }
            0xF8 => {
                let e = self.fetch(bus);
                let v = self.add_sp_e(e);
                self.set_hl(v);
                bus.tick();
            }
            0xF9 => {
                self.sp = self.hl();
                bus.tick();
            }
            0xF3 => {
                self.ime = false;
                self.ei_pending = false;
            }
            0xFB => self.ei_pending = true,
            _ => {
                // D3 DB DD E3 E4 EB EC ED F4 FC FD: the real CPU locks up.
                self.locked = true;
                let at = self.pc.wrapping_sub(1);
                bus.log(format!("CPU locked: illegal opcode {op:02X} at {at:04X} (ROM bank {}, SP {:04X})",
                    bus.cart.rom_bank, self.sp));
            }
        }
    }

    fn execute_cb(&mut self, bus: &mut Bus, op: u8) {
        let x = op >> 6;
        let y = (op >> 3) & 7;
        let z = op & 7;
        let v = self.reg(bus, z);
        match x {
            0 => {
                let c_in = u8::from(self.flag(C));
                let (r, c) = match y {
                    0 => (v.rotate_left(1), v >> 7),
                    1 => (v.rotate_right(1), v & 1),
                    2 => (v << 1 | c_in, v >> 7),
                    3 => (v >> 1 | c_in << 7, v & 1),
                    4 => (v << 1, v >> 7),
                    5 => ((v as i8 >> 1) as u8, v & 1),
                    6 => (v.rotate_left(4), 0),
                    _ => (v >> 1, v & 1),
                };
                self.set_flags(r == 0, false, false, c != 0);
                self.set_reg(bus, z, r);
            }
            1 => {
                let c = self.flag(C);
                self.set_flags(v & (1 << y) == 0, false, true, c);
            }
            2 => self.set_reg(bus, z, v & !(1 << y)),
            _ => self.set_reg(bus, z, v | (1 << y)),
        }
    }
}
