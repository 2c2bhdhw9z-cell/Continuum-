//! The JIT at run time (gamecube.json G8): the fast interpreter's blocks compiled to ARM64,
//! run with blocks jumping straight into each other.
//!
//! Only in the standard mapping every game sets up (main memory at 0x80000000 through the
//! first instruction and data BATs, translation on). There the compiled code's inline memory
//! path is exactly the MMU's answer, and a block's effective address fixes its physical one,
//! so links between blocks stay right for as long as both blocks exist. Everything else
//! (exception entry with translation off, unusual BATs), idle loops (fast-forwarded by the
//! fast interpreter) and blocks that read the time stay in the fast interpreter.
//!
//! The fast interpreter's block cache stays in charge: it finds blocks, builds them, drops
//! them when their code changes, and decides when a compiled block may run (`FastCpu`). This
//! file only holds the generated code and the links between blocks.

use super::compile::{compile, Inst, JitCtx, TABLE_EMPTY, TABLE_SIZE, UNLINKED};
use crate::cpu::fast_ops::Op;
use crate::cpu::{Cpu, MSR_DR, MSR_IR};
use crate::hw::{Hw, MEM1_SIZE};
use std::collections::HashMap;

/// Where generated code lives and how it runs: the phone's code memory, or (on the x86 test
/// machine) an ARM64 emulator.
pub trait Backend: Send {
    /// The size of the code area, in instructions.
    fn capacity(&self) -> usize;
    /// Writes instructions at instruction `at` and makes them ready to run.
    fn write(&mut self, at: usize, words: &[u32]);
    /// The address instruction `at` runs from (what generated code jumps to).
    fn address(&self, at: usize) -> u64;
    /// The address generated code calls to run one instruction's handler.
    fn fallback(&self) -> u64;
    /// Runs the code at instruction `at` as `fn(cpu, hw, ctx, budget) -> budget left`.
    ///
    /// # Safety
    /// The code there must have come from the compiler, and the pointers must be live.
    unsafe fn call(&mut self, at: usize, cpu: *mut Cpu, hw: *mut Hw, ctx: *const JitCtx, budget: u64) -> u64;
}

/// Runs one instruction's fast-interpreter handler: what generated code calls for anything it
/// does not do itself.
///
/// # Safety
/// Called only by generated code, with the pointers it was given and an `Op` of a live block.
pub unsafe extern "C" fn fallback(cpu: *mut Cpu, hw: *mut Hw, op: *const Op) {
    // SAFETY: as above; nothing else holds these while generated code runs.
    unsafe {
        let op = &*op;
        (op.f)(&mut *cpu, &mut *hw, op);
    }
}

/// A compiled block: the effective address it was compiled for, and where its code starts.
#[derive(Clone, Copy, Debug)]
pub struct Native {
    pub ea: u32,
    pub entry: usize,
}

#[derive(Default, Clone, Debug)]
pub struct JitStats {
    pub compiled: u64,
    pub native_instructions: u64,
    pub handler_instructions: u64,
    pub calls: u64,
    pub links: u64,
    pub resets: u64,
}

pub struct Jit {
    backend: Box<dyn Backend>,
    /// Instructions of the code area in use.
    used: usize,
    ctx: Box<JitCtx>,
    /// Compiled blocks by effective address: their chained entry.
    entries: HashMap<u32, usize>,
    /// The mapping all current code was compiled for.
    mapping: Mapping,
    /// The indirect-branch table generated code reads (`JitCtx::table`).
    table: Vec<[u64; 2]>,
    /// Exits waiting for a block at an effective address to be compiled.
    pending: HashMap<u32, Vec<usize>>,
    /// Exits jumping straight into the block at an effective address.
    incoming: HashMap<u32, Vec<usize>>,
    pub stats: JitStats,
}

/// The mapping compiled code relies on: main memory at 0x80000000 through the first
/// instruction and data BATs (as every game sets up), translation on. Its key (the two BAT
/// pairs) and how much of main memory each BAT covers from 0x80000000.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Mapping {
    pub key: [u32; 4],
    /// Bytes from 0x80000000 that instruction fetch maps to physical 0 onwards.
    pub fetch: u32,
    /// Bytes from 0x80000000 that data accesses map to physical 0 onwards, reads and writes.
    pub data: u32,
}

impl Mapping {
    /// The mapping now, or None when the CPU is not in one compiled code can use.
    #[inline]
    pub fn of(cpu: &Cpu) -> Option<Mapping> {
        if cpu.msr & (MSR_IR | MSR_DR) != (MSR_IR | MSR_DR) {
            return None;
        }
        let fetch = bat_window(cpu.ibat[0], false)?;
        let data = bat_window(cpu.dbat[0], true)?;
        Some(Mapping { key: [cpu.ibat[0][0], cpu.ibat[0][1], cpu.dbat[0][0], cpu.dbat[0][1]], fetch, data })
    }

    /// Whether code at an effective address can be compiled and linked to in this mapping.
    #[inline]
    pub fn compilable(&self, ea: u32) -> bool {
        ea.wrapping_sub(0x8000_0000) < self.fetch && ea & 3 == 0
    }
}

/// How many bytes from 0x80000000 a BAT pair maps to physical 0 onwards (at most all of main
/// memory), valid at both privilege levels and allowing reads (and writes, for `write`); None
/// when it does not. The MMU uses the first BAT that matches, so then this one decides every
/// access there.
#[inline]
fn bat_window(b: [u32; 2], write: bool) -> Option<u32> {
    let (u, l) = (b[0], b[1]);
    let bl = (u >> 2) & 0x7FF;
    let pp_ok = if write { l & 3 == 2 } else { l & 3 != 0 };
    // BL must be a run of low ones (a power-of-two block); the block must start at
    // 0x80000000 and map to physical 0.
    let mask = !(bl << 17) & 0xFFFE_0000;
    if u & 3 != 3 || bl & (bl + 1) != 0 || u & mask != 0x8000_0000 || l & mask != 0 || !pp_ok {
        return None;
    }
    Some(((bl + 1) << 17).min(MEM1_SIZE as u32))
}

/// Where an effective address sits in the indirect-branch table.
fn table_index(ea: u32) -> usize {
    (ea >> 2) as usize & (TABLE_SIZE - 1)
}

/// `b` from instruction `from` to instruction `to`.
fn branch(from: usize, to: usize) -> u32 {
    0x1400_0000 | ((to as i64 - from as i64) as u32 & 0x03FF_FFFF)
}

impl Jit {
    pub fn new(backend: Box<dyn Backend>) -> Jit {
        Jit {
            backend,
            used: 0,
            ctx: Box::new(JitCtx { mem1: 0, code_pages: 0, fallback: 0, mem_limit: 0, table: 0 }),
            entries: HashMap::new(),
            mapping: Mapping::default(),
            table: vec![[TABLE_EMPTY, 0]; TABLE_SIZE],
            pending: HashMap::new(),
            incoming: HashMap::new(),
            stats: JitStats::default(),
        }
    }

    /// The mapping current code was compiled for.
    pub fn mapping(&self) -> Mapping {
        self.mapping
    }

    /// After a `reset`: the mapping the code compiled from now on is for.
    pub fn set_mapping(&mut self, m: Mapping) {
        debug_assert!(self.entries.is_empty());
        self.mapping = m;
    }

    /// Forgets all generated code (the caller forgets every `Native` it holds).
    pub fn reset(&mut self) {
        self.used = 0;
        self.table.iter_mut().for_each(|e| *e = [TABLE_EMPTY, 0]);
        self.entries.clear();
        self.pending.clear();
        self.incoming.clear();
        self.stats.resets += 1;
    }

    /// Compiles the block at effective address `ea`. None when the code area is full: then
    /// `reset` and forget every `Native`.
    pub fn compile(&mut self, ea: u32, ops: &[Op]) -> Option<Native> {
        let insts: Vec<Inst> = ops.iter().enumerate().map(|(k, op)| Inst { ea: ea.wrapping_add(4 * k as u32), op }).collect();
        let c = compile(&insts, true);
        if self.used + c.code.len() > self.backend.capacity() {
            return None;
        }
        let at = self.used;
        let mut code = c.code;
        let mut incoming: Vec<(u32, usize)> = Vec::new();
        for &(site, target) in &c.links {
            let abs = at + site;
            if !self.mapping.compilable(target) {
                continue;
            }
            if let Some(&inner) = self.entries.get(&target) {
                code[site] = branch(abs, inner);
                incoming.push((target, abs));
            } else {
                self.pending.entry(target).or_default().push(abs);
            }
        }
        self.backend.write(at, &code);
        self.used += code.len();
        for (target, abs) in incoming {
            self.incoming.entry(target).or_default().push(abs);
            self.stats.links += 1;
        }
        let inner = at + c.inner;
        self.entries.insert(ea, inner);
        self.table[table_index(ea)] = [ea as u64, self.backend.address(inner)];
        // Exits that were waiting for this block (its own loop back included).
        if let Some(sites) = self.pending.remove(&ea) {
            for &s in &sites {
                self.backend.write(s, &[branch(s, inner)]);
            }
            self.stats.links += sites.len() as u64;
            self.incoming.entry(ea).or_default().extend(sites);
        }
        self.stats.compiled += 1;
        self.stats.native_instructions += c.native as u64;
        self.stats.handler_instructions += (ops.len() - c.native) as u64;
        Some(Native { ea, entry: at })
    }

    /// The block compiled for `ea` is gone: exits that jumped into it return instead (and
    /// wait for it to be compiled again).
    pub fn forget(&mut self, ea: u32) {
        if self.entries.remove(&ea).is_none() {
            return;
        }
        let t = &mut self.table[table_index(ea)];
        if t[0] == ea as u64 {
            *t = [TABLE_EMPTY, 0];
        }
        if let Some(sites) = self.incoming.remove(&ea) {
            for &s in &sites {
                self.backend.write(s, &[UNLINKED]);
            }
            self.pending.entry(ea).or_default().extend(sites);
        }
    }

    /// Runs compiled code from `n` for at most `budget` instructions (at least the whole first
    /// block). Returns how many ran.
    pub fn run(&mut self, n: Native, cpu: &mut Cpu, hw: &mut Hw, budget: u64) -> u64 {
        self.ctx.mem1 = hw.mem1.as_mut_ptr() as u64;
        self.ctx.code_pages = hw.code_lines.as_ptr() as u64;
        self.ctx.fallback = self.backend.fallback();
        // Accesses up to 8 bytes long starting below this are inside the data window.
        self.ctx.mem_limit = (self.mapping.data - 7) as u64;
        self.ctx.table = self.table.as_ptr() as u64;
        self.stats.calls += 1;
        // SAFETY: `n` came from `compile` since the last reset, so its code is whole; the
        // pointers are live for the call and nothing else uses them meanwhile.
        let left = unsafe { self.backend.call(n.entry, cpu, hw, &*self.ctx, budget) };
        budget - left
    }

    /// How much of the code area is in use, in bytes.
    pub fn used_bytes(&self) -> usize {
        self.used * 4
    }
}
