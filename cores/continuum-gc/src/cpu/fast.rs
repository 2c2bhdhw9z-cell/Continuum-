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
}

impl FastCpu {
    pub fn new() -> Self {
        FastCpu { blocks: Vec::new(), free: Vec::new(), index: Map::default(), lookup: vec![(0, 0); LOOKUP], pages: Map::default(), stats: FastStats::default() }
    }

    /// Drops every cached block.
    pub fn clear(&mut self) {
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
        let block = Block { ops };
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

    /// Runs one block (or takes one interrupt), at most `budget` instructions. Returns the
    /// cycles used and the physical page of a newly built block (for the code-page map).
    pub fn run(&mut self, cpu: &mut Cpu, hw: &mut Hw, budget: u64) -> (u64, Option<u32>) {
        if cpu.check_interrupts() {
            return (1, None);
        }
        let pc = cpu.pc;
        let pa = if cpu.msr & MSR_IR == 0 {
            pc
        } else {
            match super::mmu::translate(cpu, pc, Access::Fetch, hw) {
                Some(pa) => pa,
                // Let the plain path raise the ISI exactly as it would.
                None => return (cpu.step(hw), None),
            }
        };
        let slot = ((pa >> 2) as usize) & (LOOKUP - 1);
        let mut built = None;
        let idx = if self.lookup[slot].0 == pa + 1 {
            self.lookup[slot].1
        } else {
            let idx = match self.index.get(&pa) {
                Some(&i) => i,
                None => {
                    built = Some(pa >> 12);
                    self.build(hw, pa)
                }
            };
            self.lookup[slot] = (pa + 1, idx);
            idx
        };
        self.stats.blocks_run += 1;
        let ops = &self.blocks[idx as usize].as_ref().expect("live block").ops;
        let limit = (ops.len() as u64).min(budget.max(1)) as usize;
        cpu.exit = false;
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
        cpu.advance_time(n as u64);
        (n as u64, built)
    }
}
