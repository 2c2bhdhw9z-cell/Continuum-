//! The fast interpreter (gamecube.json G7): instructions decoded once into small handlers
//! (fast_ops.rs), run in blocks.
//!
//! Code is cut into blocks (straight runs of instructions inside one 4 KB page, ending at a
//! branch or anything that can change translation, the MSR or interrupts). A block is fetched,
//! translated and decoded once, kept by physical address, and replayed: each instruction is
//! one call to its handler, with its operands already pulled out of the instruction word.
//!
//! Identical to the plain interpreter, instruction for instruction:
//! - interrupts are checked at the start of each block, and a block stops right after any
//!   instruction that could change the interrupt line (a hardware register access, a DMA);
//! - the caller never lets a block run past the next video line or the moment the decrementer
//!   passes zero, and instructions that read or write the time base or the decrementer always
//!   start a block, where the time is exact;
//! - time is added per block, in the same amounts.
//!
//! Code that changes is dropped from the cache: `icbi`, the instruction-cache flash
//! invalidate, and any CPU store or DMA into a page holding cached code (`Hw::code_pages`);
//! a block stops at once when that happens to its own page.

use super::fast_ops::{decode, ends_block, is_timed, Op};
use super::{Access, Cpu, MSR_IR};
use crate::hw::Hw;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// The most instructions one block holds.
const MAX_BLOCK: usize = 128;
/// Direct-mapped lookup in front of the block map.
const LOOKUP: usize = 1 << 16;

/// A small, fast hasher for u32 keys.
#[derive(Default)]
pub struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ b as u64).wrapping_mul(0x51_7cc1_b727_220a_95);
        }
    }
    fn write_u32(&mut self, i: u32) {
        self.0 = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

type Map<V> = HashMap<u32, V, BuildHasherDefault<KeyHasher>>;

struct Block {
    ops: Vec<Op>,
    /// Unique per built block, so links to a freed and reused slot are recognised as stale.
    version: u32,
    /// Links to the blocks that followed this one: (effective pc + 1, block index, version,
    /// translation generation).
    links: [(u32, u32, u32, u32); 2],
    /// A loop that only reads memory and branches back to itself: it can be fast-forwarded.
    idle: bool,
    /// Ends with an instruction that touches the time base or the decrementer: the machine
    /// must work out the next deadline again before anything else runs.
    timed: bool,
}

pub struct FastCpu {
    /// Blocks by index; freed slots are reused.
    blocks: Vec<Option<Block>>,
    free: Vec<u32>,
    /// Physical start address -> block index.
    index: Map<u32>,
    /// (physical address + 1, block index) by address hash: most lookups end here.
    lookup: Vec<(u32, u32)>,
    /// Physical page number -> block start addresses in it.
    pages: Map<Vec<u32>>,
    /// The block that ran last, to follow its links.
    last: Option<u32>,
    pub stats: FastStats,
}

impl Default for FastCpu {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default, Clone, Debug)]
pub struct FastStats {
    pub blocks_built: u64,
    pub blocks_run: u64,
    pub invalidations: u64,
    pub idle_skips: u64,
}

impl FastCpu {
    pub fn new() -> Self {
        FastCpu { blocks: Vec::new(), free: Vec::new(), index: Map::default(), lookup: vec![(0, 0); LOOKUP], pages: Map::default(), last: None, stats: FastStats::default() }
    }

    /// Drops every cached block.
    pub fn clear(&mut self) {
        self.last = None;
        self.blocks.clear();
        self.free.clear();
        self.index.clear();
        self.pages.clear();
        self.lookup.iter_mut().for_each(|e| *e = (0, 0));
    }

    /// Drops the blocks in the pages `pa .. pa + len` touches.
    pub fn invalidate(&mut self, pa: u32, len: u32) {
        if len >= 0x0100_0000 {
            self.clear();
            return;
        }
        let first = pa >> 12;
        let last = pa.wrapping_add(len.max(1) - 1) >> 12;
        for page in first..=last {
            if let Some(starts) = self.pages.remove(&page) {
                self.stats.invalidations += 1;
                self.last = None;
                for s in starts {
                    if let Some(i) = self.index.remove(&s) {
                        self.blocks[i as usize] = None;
                        self.free.push(i);
                        let slot = &mut self.lookup[((s >> 2) as usize) & (LOOKUP - 1)];
                        if slot.0 == s + 1 {
                            *slot = (0, 0);
                        }
                    }
                }
            }
        }
    }

    fn build(&mut self, hw: &mut Hw, pa: u32) -> u32 {
        let mut ops = Vec::with_capacity(16);
        let mut a = pa;
        loop {
            let i = crate::bus::Bus::read32(hw, a);
            // A time-dependent instruction always starts its own block.
            if is_timed(i) && !ops.is_empty() {
                break;
            }
            ops.push(decode(i));
            a = a.wrapping_add(4);
            if ends_block(i) || is_timed(i) || ops.len() >= MAX_BLOCK || a & 0xFFF == 0 {
                break;
            }
        }
        let idle = is_idle_loop(&ops);
        let timed = ops.last().is_some_and(|o| is_timed(o.inst));
        let version = self.stats.blocks_built as u32 + 1;
        let block = Block { ops, version, links: [(0, 0, 0, 0); 2], idle, timed };
        let idx = match self.free.pop() {
            Some(i) => {
                self.blocks[i as usize] = Some(block);
                i
            }
            None => {
                self.blocks.push(Some(block));
                (self.blocks.len() - 1) as u32
            }
        };
        self.index.insert(pa, idx);
        self.pages.entry(pa >> 12).or_default().push(pa);
        self.stats.blocks_built += 1;
        idx
    }

    /// Runs blocks, one after another, for at most `budget` cycles: until then, or until
    /// something happens the machine must see first (an interrupt taken, a hardware register
    /// touched, code invalidated, the decrementer passing zero, the CPU going to sleep).
    /// Returns the cycles used.
    pub fn run(&mut self, cpu: &mut Cpu, hw: &mut Hw, budget: u64) -> u64 {
        let mut used = 0u64;
        while used < budget {
            if cpu.check_interrupts() {
                // As in the plain interpreter: taking it counts one cycle for the machine's
                // clock but none for the CPU's time base.
                self.last = None;
                return used + 1;
            }
            let (n, stop) = self.run_block(cpu, hw, budget - used);
            used += n;
            if stop || hw.irq_dirty || !hw.invalidated.is_empty() || cpu.dec_pending || cpu.msr & super::MSR_POW != 0 {
                break;
            }
        }
        used
    }

    /// Runs one block, at most `budget` instructions. Returns the cycles used, and whether the
    /// caller must stop chaining.
    fn run_block(&mut self, cpu: &mut Cpu, hw: &mut Hw, budget: u64) -> (u64, bool) {
        let pc = cpu.pc;
        let gen = cpu.tlb.gen;
        // Follow the last block's link when it still holds.
        let linked = self.last.and_then(|l| {
            let b = self.blocks[l as usize].as_ref()?;
            let k = b.links.iter().find(|k| k.0 == pc.wrapping_add(1) && k.3 == gen)?;
            let target = self.blocks[k.1 as usize].as_ref()?;
            (target.version == k.2).then_some(k.1)
        });
        let idx = match linked {
            Some(i) => i,
            None => {
                let pa = if cpu.msr & MSR_IR == 0 {
                    pc
                } else {
                    match super::mmu::translate(cpu, pc, Access::Fetch, hw) {
                        Some(pa) => pa,
                        None => {
                            // Let the plain path raise the ISI exactly as it would.
                            self.last = None;
                            return (cpu.step(hw), true);
                        }
                    }
                };
                let slot = ((pa >> 2) as usize) & (LOOKUP - 1);
                let idx = if self.lookup[slot].0 == pa + 1 {
                    self.lookup[slot].1
                } else {
                    let idx = match self.index.get(&pa) {
                        Some(&i) => i,
                        None => {
                            let i = self.build(hw, pa);
                            hw.mark_code_page(pa >> 12);
                            i
                        }
                    };
                    self.lookup[slot] = (pa + 1, idx);
                    idx
                };
                // Remember the way from the last block to this one.
                if let Some(l) = self.last {
                    let version = self.blocks[idx as usize].as_ref().map_or(0, |b| b.version);
                    if let Some(b) = self.blocks[l as usize].as_mut() {
                        let k = if b.links[0].0 == 0 || b.links[0].0 == pc.wrapping_add(1) { 0 } else { 1 };
                        if k == 1 {
                            b.links[1] = b.links[0];
                        }
                        b.links[0] = (pc.wrapping_add(1), idx, version, gen);
                    }
                }
                idx
            }
        };
        self.stats.blocks_run += 1;
        let block = self.blocks[idx as usize].as_ref().expect("live block");
        let ops = &block.ops;
        let limit = (ops.len() as u64).min(budget.max(1)) as usize;
        cpu.exit = false;
        cpu.faulted = false;
        let mut ea = pc;
        let mut n = 0usize;
        for op in &ops[..limit] {
            cpu.cur_pc = ea;
            (op.f)(cpu, hw, op);
            n += 1;
            if cpu.exit {
                break;
            }
            ea = ea.wrapping_add(4);
        }
        if !cpu.exit {
            cpu.pc = ea;
        }
        let mut cycles = n as u64;
        // An idle loop that went round once and will go round the same way until the next
        // event (it only reads memory nothing else can change before then): go round all the
        // times it would have, at once. The state after k rounds is the state after one.
        if block.idle && n == ops.len() && cpu.pc == pc && !cpu.faulted && !hw.irq_dirty && hw.invalidated.is_empty() {
            let rounds = (budget.saturating_sub(cycles)) / n as u64;
            cycles += rounds * n as u64;
            self.stats.idle_skips += rounds;
        }
        self.last = Some(idx);
        let stop = block.timed;
        cpu.advance_time(cycles);
        (cycles, stop)
    }
}

/// Whether a block is a pure waiting loop: loads from memory (no update forms), compares, and
/// arithmetic on registers, ending in a conditional branch back to its own start that leaves
/// CTR and LR alone, where every register the loop reads is either never written by it or
/// written earlier in the same round. Then every round computes exactly the same values from
/// the same memory, so going round k times leaves the same state as going round once.
fn is_idle_loop(ops: &[Op]) -> bool {
    if ops.len() < 2 || ops.len() > 16 {
        return false;
    }
    let last = ops[ops.len() - 1].inst;
    // bc with BO[2] set (no CTR), no link, relative, back to the block start.
    if last >> 26 != 16 || last & 3 != 0 || (last >> 21) & 4 == 0 {
        return false;
    }
    let back = (last & 0xFFFC) as i16 as i32;
    if back != -4 * (ops.len() as i32 - 1) {
        return false;
    }
    // Registers (GPR bits 0-31) and CR fields (bits 0-7) read and written so far.
    let (mut read_gpr, mut written_gpr) = (0u32, 0u32);
    let (mut read_cr, mut written_cr) = (0u32, 0u32);
    let bit = |r: u32| 1u32 << r;
    for op in &ops[..ops.len() - 1] {
        let i = op.inst;
        let (d, a, b) = ((i >> 21) & 31, (i >> 16) & 31, (i >> 11) & 31);
        let ra0 = |a: u32| if a == 0 { 0 } else { bit(a) };
        // (gprs read, gpr written, cr fields read, cr field written)
        let (rg, wg, wc): (u32, u32, u32) = match i >> 26 {
            32 | 34 | 40 | 42 | 14 | 15 => (ra0(a), bit(d), 0),
            10 | 11 => (bit(a), 0, bit(d >> 2)),
            24 | 25 => (bit(d), bit(a), 0),
            28 | 29 => (bit(d), bit(a), bit(0)),
            21 => (bit(d), bit(a), if i & 1 != 0 { bit(0) } else { 0 }),
            31 => match (i >> 1) & 0x3FF {
                0 | 32 => (bit(a) | bit(b), 0, bit(d >> 2)),
                23 | 87 | 279 => (ra0(a) | bit(b), bit(d), 0),
                28 | 444 => (bit(d) | bit(b), bit(a), if i & 1 != 0 { bit(0) } else { 0 }),
                _ => return false,
            },
            _ => return false,
        };
        // A register read before this round writes it must never be written in the round.
        read_gpr |= rg & !written_gpr;
        written_gpr |= wg;
        written_cr |= wc;
        let _ = &mut read_cr;
    }
    // The branch reads one CR bit.
    let bi = (last >> 16) & 31;
    if last & (0x10 << 21) == 0 {
        read_cr |= bit(bi / 4) & !written_cr;
    }
    read_gpr & written_gpr == 0 && read_cr & written_cr == 0
}
