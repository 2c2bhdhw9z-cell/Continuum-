//! Address translation: block address translation (BATs) first, then segments and the hashed
//! page table, as the PowerPC operating environment architecture (book III) describes.
//!
//! Games mostly run on BATs alone; some (and the Wii's IOS-facing code) use the page table, which
//! is why it is here from the start. A small cache (`Tlb`) remembers recent page translations;
//! it is flushed whenever anything it depends on changes (BATs, segment registers, SDR1, MSR,
//! `tlbie`).

use super::{Access, Cpu, HID4_SBE, MSR_PR};
use crate::bus::Bus;

const TLB_SETS: usize = 1024;

#[derive(Clone, Copy, Default)]
struct Entry {
    /// Effective page number + 1 (0 means empty).
    tag: u32,
    /// The flush generation this entry belongs to; older generations are stale.
    gen: u32,
    /// Physical page base.
    page: u32,
}

/// Remembered translations (block-mapped and page-table alike), one table per kind of access (a
/// page may be readable but not writable, and the C bit must be set on the first write).
/// Flushing only bumps a generation number, so it costs nothing however often it happens.
#[derive(Clone)]
pub struct Tlb {
    pub(crate) gen: u32,
    /// The generation `win` was worked out for (0: never).
    win_gen: u32,
    /// See [`data_window`].
    win: u32,
    read: Vec<Entry>,
    write: Vec<Entry>,
    fetch: Vec<Entry>,
}

impl Default for Tlb {
    fn default() -> Self {
        Tlb { gen: 1, win_gen: 0, win: 0, read: vec![Entry::default(); TLB_SETS], write: vec![Entry::default(); TLB_SETS], fetch: vec![Entry::default(); TLB_SETS] }
    }
}

impl Tlb {
    pub fn flush_all(&mut self) {
        self.gen = self.gen.wrapping_add(1);
        if self.gen == 0 {
            for t in [&mut self.read, &mut self.write, &mut self.fetch] {
                t.iter_mut().for_each(|e| *e = Entry::default());
            }
            self.gen = 1;
            self.win_gen = 0;
        }
    }
    /// The MSR changed (exception, rfi, mtmsr): translations depend only on the privilege
    /// level (PR), so they are dropped only when that changed. (Whether translation is on at
    /// all, IR/DR, is checked before the cache is used.)
    pub fn flush_mode(&mut self, old_msr: u32, new_msr: u32) {
        if (old_msr ^ new_msr) & MSR_PR != 0 {
            self.flush_all();
        }
    }
    /// `tlbie`: drop one effective page.
    pub fn flush_page(&mut self, ea: u32) {
        let set = ((ea >> 12) as usize) & (TLB_SETS - 1);
        for t in [&mut self.read, &mut self.write, &mut self.fetch] {
            t[set] = Entry::default();
        }
    }
    fn table(&mut self, access: Access) -> &mut Vec<Entry> {
        match access {
            Access::Read => &mut self.read,
            Access::Write => &mut self.write,
            Access::Fetch => &mut self.fetch,
        }
    }
}

/// The data window: with data translation on, an effective address from 0x80000000 up to
/// 0x80000000 + this value is in main memory at (address - 0x80000000), for reads and writes
/// alike, and any access of up to 8 bytes starting there stays inside main memory. 0 when there
/// is no such window.
///
/// It is exactly what `translate` answers there: the MMU uses the first BAT that matches, and
/// the window is only set when the first data BAT is valid at both privilege levels, starts at
/// 0x80000000, maps it to physical 0 and allows reading and writing (as every game's set-up
/// does). It depends only on that BAT, and every BAT change bumps the generation, so it is
/// worked out again whenever the BATs change.
#[inline(always)]
pub fn data_window(cpu: &mut Cpu) -> u32 {
    if cpu.tlb.win_gen != cpu.tlb.gen {
        work_out_window(cpu);
    }
    cpu.tlb.win
}

#[cold]
#[inline(never)]
fn work_out_window(cpu: &mut Cpu) {
    let [u, l] = cpu.dbat[0];
    let bl = (u >> 2) & 0x7FF;
    let mask = !(bl << 17) & 0xFFFE_0000;
    let ok = u & 3 == 3 && bl & (bl + 1) == 0 && u & mask == 0x8000_0000 && l & mask == 0 && l & 3 == 2;
    cpu.tlb.win = if ok { ((bl + 1) << 17).min(crate::hw::MEM1_SIZE as u32) - 7 } else { 0 };
    cpu.tlb.win_gen = cpu.tlb.gen;
}

/// Translates an effective address. None means the access faults (the caller raises DSI or ISI).
#[inline]
pub fn translate<B: Bus>(cpu: &mut Cpu, ea: u32, access: Access, bus: &mut B) -> Option<u32> {
    let set = ((ea >> 12) as usize) & (TLB_SETS - 1);
    let tag = (ea >> 12) + 1;
    let gen = cpu.tlb.gen;
    let e = cpu.tlb.table(access)[set];
    if e.tag == tag && e.gen == gen {
        return Some(e.page | (ea & 0xFFF));
    }
    translate_slow(cpu, ea, access, bus, set, tag)
}

#[inline(never)]
fn translate_slow<B: Bus>(cpu: &mut Cpu, ea: u32, access: Access, bus: &mut B, set: usize, tag: u32) -> Option<u32> {
    let page = match bat(cpu, ea, access) {
        Some(Some(pa)) => pa & !0xFFF,
        Some(None) => return None,
        None => page_table(cpu, ea, access, bus)?,
    };
    let gen = cpu.tlb.gen;
    cpu.tlb.table(access)[set] = Entry { tag, gen, page };
    Some(page | (ea & 0xFFF))
}

/// Some(Some(pa)): a BAT maps it. Some(None): a BAT matches but forbids the access.
/// None: no BAT matches; go to the page table.
fn bat(cpu: &Cpu, ea: u32, access: Access) -> Option<Option<u32>> {
    let bats = if access == Access::Fetch { &cpu.ibat } else { &cpu.dbat };
    let count = if cpu.is_broadway() && cpu.hid4 & HID4_SBE != 0 { 8 } else { 4 };
    let user = cpu.msr & MSR_PR != 0;
    for [upper, lower] in bats.iter().take(count).copied() {
        // Vs (supervisor valid) is bit 1, Vp (user valid) bit 0.
        let valid = if user { upper & 1 != 0 } else { upper & 2 != 0 };
        if !valid {
            continue;
        }
        // BL: the block length mask, in 128 KB units, applied to the top 15 bits.
        let bl = (upper >> 2) & 0x7FF;
        let mask = !(bl << 17) & 0xFFFE_0000;
        if ea & mask != upper & mask {
            continue;
        }
        let pp = lower & 3;
        let allowed = match pp {
            0 => false,
            1 | 3 => access != Access::Write,
            _ => true,
        };
        if !allowed {
            return Some(None);
        }
        let pa = (lower & mask) | (ea & !mask);
        return Some(Some(pa));
    }
    None
}

/// The hashed page table walk: primary hash, then secondary. Sets R (and C on a write).
fn page_table<B: Bus>(cpu: &mut Cpu, ea: u32, access: Access, bus: &mut B) -> Option<u32> {
    let seg = cpu.sr[(ea >> 28) as usize];
    // T = 1 is a direct-store segment, which the Gekko does not support: no translation.
    if seg & 0x8000_0000 != 0 {
        return None;
    }
    // N: no-execute segment.
    if access == Access::Fetch && seg & 0x1000_0000 != 0 {
        return None;
    }
    let user = cpu.msr & MSR_PR != 0;
    let key = if user { seg & 0x2000_0000 != 0 } else { seg & 0x4000_0000 != 0 };
    let vsid = seg & 0x00FF_FFFF;
    let page_index = (ea >> 12) & 0xFFFF;
    let api = page_index >> 10;
    let htaborg = cpu.sdr1 & 0xFFFF_0000;
    let htabmask = cpu.sdr1 & 0x1FF;
    let primary = (vsid & 0x7FFFF) ^ page_index;
    for (h, hash) in [(0u32, primary), (1, !primary & 0x7FFFF)] {
        let pteg = (htaborg & 0xFE00_0000)
            | ((((htaborg >> 16) & 0x1FF) | ((hash >> 10) & htabmask)) << 16)
            | ((hash & 0x3FF) << 6);
        for i in 0..8 {
            let addr = pteg + i * 8;
            let w0 = bus.read32(addr);
            if w0 & 0x8000_0000 == 0 {
                continue;
            }
            if (w0 >> 7) & 0x00FF_FFFF != vsid || (w0 >> 6) & 1 != h || w0 & 0x3F != api {
                continue;
            }
            let mut w1 = bus.read32(addr + 4);
            let pp = w1 & 3;
            let allowed = match (key, pp) {
                (false, 3) => access != Access::Write,
                (false, _) => true,
                (true, 0) => false,
                (true, 2) => true,
                (true, _) => access != Access::Write,
            };
            if !allowed {
                return None;
            }
            let old = w1;
            w1 |= 0x100; // R: referenced
            if access == Access::Write {
                w1 |= 0x80; // C: changed
            }
            if w1 != old {
                bus.write32(addr + 4, w1);
            }
            return Some(w1 & 0xFFFF_F000);
        }
    }
    None
}
