//! A high-level BIOS: the SWI calls games use, done in Rust, so no BIOS file is needed. The IRQ
//! path stays low-level (the mini BIOS in `bus.rs` dispatches to the game's handler).
//! IntrWait re-runs its own SWI after each interrupt until the wanted one has come.

use super::bus::Bus;
use super::cpu::{Cpu, MODE_SVC, MODE_SYS};

fn rd32(bus: &Bus, a: u32) -> u32 {
    bus.peek32(a)
}

fn wr32(bus: &mut Bus, a: u32, v: u32) {
    bus.write32(a, v);
}

pub fn swi(cpu: &mut Cpu, bus: &mut Bus, n: u32) {
    bus.bios_latch = 0xE3A0_2004;
    bus.idle(3);
    let r = &mut cpu.r;
    match n {
        0x00 => {
            // SoftReset
            for a in 0x0300_7E00u32..0x0300_8000 {
                bus.write8(a, 0);
            }
            let to_ram = bus.peek8(0x0300_7FFA) != 0;
            let fresh = Cpu::default();
            *cpu = fresh;
            cpu.pc = if to_ram { 0x0200_0000 } else { 0x0800_0000 };
        }
        0x01 => register_ram_reset(bus, r[0]),
        0x02 | 0x03 => bus.halted = true,
        0x04 | 0x05 => {
            if n == 0x05 {
                r[0] = 1;
                r[1] = 1;
            }
            intr_wait(cpu, bus);
        }
        0x06 | 0x07 => {
            let (num, den) = if n == 6 { (r[0] as i32, r[1] as i32) } else { (r[1] as i32, r[0] as i32) };
            if den != 0 {
                let q = num.wrapping_div(den);
                r[0] = q as u32;
                r[1] = num.wrapping_rem(den) as u32;
                r[3] = q.unsigned_abs();
            }
        }
        0x08 => r[0] = (r[0] as f64).sqrt() as u32,
        0x09 => r[0] = arctan(r[0] as i32) as u32,
        0x0A => {
            let (x, y) = (r[0] as i16 as f64, r[1] as i16 as f64);
            let a = y.atan2(x);
            let a = if a < 0.0 { a + std::f64::consts::TAU } else { a };
            r[0] = ((a / std::f64::consts::TAU * 65536.0) as u32) & 0xFFFF;
        }
        0x0B => cpu_set(bus, r[0], r[1], r[2]),
        0x0C => cpu_fast_set(bus, r[0], r[1], r[2]),
        0x0D => r[0] = 0xBAAE_187F,
        0x0E => bg_affine_set(bus, r[0], r[1], r[2]),
        0x0F => obj_affine_set(bus, r[0], r[1], r[2], r[3]),
        0x10 => bit_unpack(bus, r[0], r[1], r[2]),
        0x11 | 0x12 => {
            let out = lz77(bus, r[0]);
            write_out(bus, r[1], &out, n == 0x12);
        }
        0x13 => {
            let out = huffman(bus, r[0]);
            write_out(bus, r[1], &out, false);
        }
        0x14 | 0x15 => {
            let out = rle(bus, r[0]);
            write_out(bus, r[1], &out, n == 0x15);
        }
        0x16 | 0x17 | 0x18 => {
            let out = diff(bus, r[0], n == 0x18);
            write_out(bus, r[1], &out, n == 0x17);
        }
        _ => {}
    }
    let _ = (MODE_SVC, MODE_SYS);
}

fn intr_wait(cpu: &mut Cpu, bus: &mut Bus) {
    // The BIOS's own copy of acknowledged interrupts lives at $03007FF8 (IWRAM end - 8).
    bus.io[0x208] = 1; // IME
    let flags = bus.peek16(0x0300_7FF8) as u16;
    let want = cpu.r[1] as u16;
    if cpu.r[0] != 0 {
        // Discard old flags on the first call, then wait for a new one.
        bus.write16(0x0300_7FF8, flags & !want);
        cpu.r[0] = 0;
    } else if flags & want != 0 {
        bus.write16(0x0300_7FF8, flags & !want);
        return;
    }
    // Not yet: halt and run this SWI again after the interrupt returns.
    bus.halted = true;
    cpu.pc = cpu.pc.wrapping_sub(if cpu.thumb() { 2 } else { 4 });
    cpu.cpsr &= !0x80;
}

fn arctan(x: i32) -> i32 {
    let a = -((x * x) >> 14);
    let mut b = ((0xA9 * a) >> 14) + 0x390;
    for k in [0x91C, 0xFB6, 0x16AA, 0x2081, 0x3651, 0xA2F9] {
        b = ((b * a) >> 14) + k;
    }
    (x * b) >> 16
}

fn register_ram_reset(bus: &mut Bus, f: u32) {
    if f & 1 != 0 { bus.ewram.iter_mut().for_each(|b| *b = 0) }
    if f & 2 != 0 { bus.iwram[..0x7E00].iter_mut().for_each(|b| *b = 0) }
    if f & 4 != 0 { bus.pal.iter_mut().for_each(|b| *b = 0) }
    if f & 8 != 0 { bus.vram.iter_mut().for_each(|b| *b = 0) }
    if f & 16 != 0 { bus.oam.iter_mut().for_each(|b| *b = 0) }
    if f & 0x80 != 0 {
        for a in (0x0400_0000u32..0x0400_0060).step_by(2) {
            bus.write16(a, 0);
        }
    }
}

fn cpu_set(bus: &mut Bus, src: u32, dst: u32, ctl: u32) {
    let count = ctl & 0x1F_FFFF;
    let fixed = ctl & (1 << 24) != 0;
    let word = ctl & (1 << 26) != 0;
    let step = if word { 4 } else { 2 };
    let (mut s, mut d) = (src, dst);
    for _ in 0..count {
        if word {
            let v = bus.read32(s & !3);
            bus.write32(d & !3, v);
        } else {
            let v = bus.read16(s & !1);
            bus.write16(d & !1, v as u16);
        }
        if !fixed {
            s = s.wrapping_add(step);
        }
        d = d.wrapping_add(step);
    }
}

fn cpu_fast_set(bus: &mut Bus, src: u32, dst: u32, ctl: u32) {
    let count = (ctl & 0x1F_FFFF).div_ceil(8) * 8;
    let fixed = ctl & (1 << 24) != 0;
    let (mut s, mut d) = (src & !3, dst & !3);
    let fill = bus.read32(s);
    for _ in 0..count {
        let v = if fixed { fill } else { bus.read32(s) };
        bus.write32(d, v);
        s = s.wrapping_add(4);
        d = d.wrapping_add(4);
    }
}

fn sin_cos(angle: u32) -> (i32, i32) {
    let t = f64::from((angle >> 8) & 0xFF) * std::f64::consts::TAU / 256.0;
    ((t.sin() * 16384.0) as i32, (t.cos() * 16384.0) as i32)
}

fn bg_affine_set(bus: &mut Bus, mut src: u32, mut dst: u32, n: u32) {
    for _ in 0..n {
        let ox = rd32(bus, src) as i32;
        let oy = rd32(bus, src + 4) as i32;
        let dx = i32::from(bus.peek16(src + 8) as i16);
        let dy = i32::from(bus.peek16(src + 10) as i16);
        let sx = i32::from(bus.peek16(src + 12) as i16);
        let sy = i32::from(bus.peek16(src + 14) as i16);
        let (sin, cos) = sin_cos(bus.peek16(src + 16));
        let pa = (sx * cos) >> 14;
        let pb = -(sx * sin) >> 14;
        let pc = (sy * sin) >> 14;
        let pd = (sy * cos) >> 14;
        bus.write16(dst, pa as u16);
        bus.write16(dst + 2, pb as u16);
        bus.write16(dst + 4, pc as u16);
        bus.write16(dst + 6, pd as u16);
        wr32(bus, dst + 8, (ox - (pa * dx + pb * dy)) as u32);
        wr32(bus, dst + 12, (oy - (pc * dx + pd * dy)) as u32);
        src += 20;
        dst += 16;
    }
}

fn obj_affine_set(bus: &mut Bus, mut src: u32, mut dst: u32, n: u32, stride: u32) {
    for _ in 0..n {
        let sx = i32::from(bus.peek16(src) as i16);
        let sy = i32::from(bus.peek16(src + 2) as i16);
        let (sin, cos) = sin_cos(bus.peek16(src + 4));
        let vals = [(sx * cos) >> 14, -(sx * sin) >> 14, (sy * sin) >> 14, (sy * cos) >> 14];
        for v in vals {
            bus.write16(dst, v as u16);
            dst = dst.wrapping_add(stride);
        }
        src += 8;
    }
}

fn bit_unpack(bus: &mut Bus, src: u32, dst: u32, info: u32) {
    let len = bus.peek16(info);
    let sw = bus.peek8(info + 2);
    let dw = bus.peek8(info + 3);
    let off = rd32(bus, info + 4);
    let zero = off & (1 << 31) != 0;
    let off = off & 0x7FFF_FFFF;
    let (mut out, mut bits, mut d) = (0u32, 0u32, dst);
    for i in 0..len {
        let byte = bus.peek8(src + i);
        let mut b = 0;
        while b < 8 {
            let v = (byte >> b) & ((1 << sw) - 1);
            let v = if v != 0 || zero { v + off } else { v };
            out |= (v & ((1u64 << dw) - 1) as u32) << bits;
            bits += dw;
            if bits >= 32 {
                wr32(bus, d, out);
                d += 4;
                out = 0;
                bits = 0;
            }
            b += sw;
        }
    }
}

fn write_out(bus: &mut Bus, dst: u32, out: &[u8], halfwords: bool) {
    if halfwords {
        for (i, pair) in out.chunks(2).enumerate() {
            let v = u16::from(pair[0]) | u16::from(*pair.get(1).unwrap_or(&0)) << 8;
            bus.write16(dst + i as u32 * 2, v);
        }
    } else {
        for (i, b) in out.iter().enumerate() {
            bus.write8(dst + i as u32, *b);
        }
    }
}

fn lz77(bus: &Bus, src: u32) -> Vec<u8> {
    let size = (rd32(bus, src) >> 8) as usize;
    let mut out = Vec::with_capacity(size);
    let mut s = src + 4;
    while out.len() < size {
        let flags = bus.peek8(s) as u8;
        s += 1;
        for bit in (0..8).rev() {
            if out.len() >= size {
                break;
            }
            if flags & (1 << bit) != 0 {
                let b0 = bus.peek8(s) as usize;
                let b1 = bus.peek8(s + 1) as usize;
                s += 2;
                let disp = ((b0 & 0xF) << 8 | b1) + 1;
                let len = (b0 >> 4) + 3;
                for _ in 0..len {
                    let v = if disp <= out.len() { out[out.len() - disp] } else { 0 };
                    out.push(v);
                }
            } else {
                out.push(bus.peek8(s) as u8);
                s += 1;
            }
        }
    }
    out.truncate(size);
    out
}

fn rle(bus: &Bus, src: u32) -> Vec<u8> {
    let size = (rd32(bus, src) >> 8) as usize;
    let mut out = Vec::with_capacity(size);
    let mut s = src + 4;
    while out.len() < size {
        let f = bus.peek8(s) as usize;
        s += 1;
        if f & 0x80 != 0 {
            let v = bus.peek8(s) as u8;
            s += 1;
            for _ in 0..(f & 0x7F) + 3 {
                out.push(v);
            }
        } else {
            for _ in 0..(f & 0x7F) + 1 {
                out.push(bus.peek8(s) as u8);
                s += 1;
            }
        }
    }
    out.truncate(size);
    out
}

fn huffman(bus: &Bus, src: u32) -> Vec<u8> {
    let header = rd32(bus, src);
    let bits = header & 0xF;
    let size = (header >> 8) as usize;
    let tree_size = bus.peek8(src + 4);
    let root = src + 5;
    let mut data = src + 4 + (tree_size + 1) * 2;
    let mut out = Vec::with_capacity(size);
    let mut acc = 0u32;
    let mut acc_bits = 0;
    let mut node = root;
    let mut node_val = bus.peek8(node);
    while out.len() < size {
        let word = rd32(bus, data);
        data += 4;
        for i in (0..32).rev() {
            let dir = (word >> i) & 1;
            let child = (node & !1) + (node_val & 0x3F) * 2 + 2 + dir;
            let leaf = node_val & (0x80 >> dir) != 0;
            if leaf {
                let v = bus.peek8(child);
                acc |= (v & ((1 << bits) - 1)) << acc_bits;
                acc_bits += bits;
                if acc_bits == 32 {
                    out.extend_from_slice(&acc.to_le_bytes());
                    acc = 0;
                    acc_bits = 0;
                }
                node = root;
            } else {
                node = child;
            }
            node_val = bus.peek8(node);
            if out.len() >= size {
                break;
            }
        }
    }
    out.truncate(size);
    out
}

fn diff(bus: &Bus, src: u32, sixteen: bool) -> Vec<u8> {
    let size = (rd32(bus, src) >> 8) as usize;
    let mut out = Vec::with_capacity(size);
    if sixteen {
        let mut prev = 0u16;
        for i in (0..size).step_by(2) {
            prev = prev.wrapping_add(bus.peek16(src + 4 + i as u32) as u16);
            out.extend_from_slice(&prev.to_le_bytes());
        }
    } else {
        let mut prev = 0u8;
        for i in 0..size {
            prev = prev.wrapping_add(bus.peek8(src + 4 + i as u32) as u8);
            out.push(prev);
        }
    }
    out
}
