//! The fast interpreter (gamecube.json G7): the plain interpreter's instruction semantics,
//! without its per-instruction overhead.
//!
//! Code is cut into blocks (straight runs of instructions inside one 4 KB page, ending at a
//! branch or anything that can change translation or interrupts). Each block is fetched and
//! translated once, kept in a cache by physical address, and replayed. Every instruction still
//! runs through `Cpu::execute`, so the results are the plain interpreter's.
//!
//! Identical to the plain interpreter, instruction for instruction: a block stops early when
//! an instruction touches a hardware register (the interrupt line may change), raises an
//! exception or branches, and the caller never lets a block run past the next video line or
//! the decrementer. Interrupts are therefore checked at exactly the same instructions.
//!
//! Code that changes is dropped from the cache: `icbi`, the instruction-cache flash
//! invalidate, and any CPU store or DMA into a page holding cached code (`Hw::code_pages`).

use super::{Access, Cpu, MSR_IR};
use crate::bus::Bus;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// The most instructions one block holds.
const MAX_BLOCK: usize = 64;

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
    insts: Vec<u32>,
}

#[derive(Default)]
pub struct FastCpu {
    blocks: Map<Block>,
    /// Physical page number -> block start addresses in it.
    pages: Map<Vec<u32>>,
    pub stats: FastStats,
}

#[derive(Default, Clone, Debug)]
pub struct FastStats {
    pub blocks_built: u64,
    pub blocks_run: u64,
    pub invalidations: u64,
}

/// Whether an instruction must end a block: control flow, and anything that can change
/// address translation, the MSR, the decrementer or interrupt state.
fn ends_block(i: u32) -> bool {
    match i >> 26 {
        16 | 17 | 18 | 19 => true,
        31 => matches!((i >> 1) & 0x3FF, 146 | 210 | 242 | 306 | 467 | 370 | 566 | 598 | 982 | 854),
        _ => false,
    }
}

impl FastCpu {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drops every cached block.
    pub fn clear(&mut self) {
        self.blocks.clear();
        self.pages.clear();
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
                    self.blocks.remove(&s);
                }
            }
        }
    }

    /// Runs one block (or takes one interrupt), at most `budget` instructions. Returns the
    /// cycles used and the physical page of a newly built block (for the code-page map).
    pub fn run<B: Bus>(&mut self, cpu: &mut Cpu, bus: &mut B, budget: u64, stop: &dyn Fn(&mut B) -> bool) -> (u64, Option<u32>) {
        if cpu.check_interrupts() {
            return (1, None);
        }
        let pc = cpu.pc;
        let pa = if cpu.msr & MSR_IR == 0 {
            pc
        } else {
            match super::mmu::translate(cpu, pc, Access::Fetch, bus) {
                Some(pa) => pa,
                None => {
                    // Let the plain path raise the ISI exactly as it would.
                    return (cpu.step(bus), None);
                }
            }
        };
        let mut built = None;
        if !self.blocks.contains_key(&pa) {
            let mut insts = Vec::with_capacity(16);
            let mut a = pa;
            loop {
                let i = bus.read32(a);
                insts.push(i);
                a = a.wrapping_add(4);
                if ends_block(i) || insts.len() >= MAX_BLOCK || a & 0xFFF == 0 {
                    break;
                }
            }
            self.blocks.insert(pa, Block { insts });
            self.pages.entry(pa >> 12).or_default().push(pa);
            self.stats.blocks_built += 1;
            built = Some(pa >> 12);
        }
        self.stats.blocks_run += 1;
        let block = &self.blocks[&pa];
        let mut n = 0u64;
        let mut ea = pc;
        for &inst in &block.insts {
            if n >= budget {
                break;
            }
            cpu.begin(ea);
            ea = ea.wrapping_add(4);
            cpu.pc = ea;
            cpu.execute(inst, bus);
            cpu.advance_time(1);
            n += 1;
            if cpu.faulted() || cpu.pc != ea || cpu.dec_pending || stop(bus) {
                break;
            }
        }
        (n.max(1), built)
    }
}
