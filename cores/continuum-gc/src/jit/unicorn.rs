//! Generated code run inside Unicorn (an ARM64 emulator) on a test machine that is not ARM64.
//! Only built with the `unicorn` feature, for tests and the comparison tools; never part of
//! the core in the app.
//!
//! The CPU state, main memory and the code-page bitmap are the host's own memory, mapped into
//! the emulator where the generated code looks for them, so generated code and handlers always
//! see the same bytes. Handler calls land on a page the emulator stops at; the host runs the
//! handler there and the code carries on.

use super::compile::JitCtx;
use super::runtime::{fallback, Backend};
use crate::cpu::fast_ops::Op;
use crate::cpu::Cpu;
use crate::hw::Hw;
use unicorn_engine::unicorn_const::{Arch, Mode, Prot};
use unicorn_engine::{RegisterARM64, Unicorn};

const CODE: u64 = 0x1_0000_0000;
const STACK: u64 = 0x2_0000_0000;
const STACK_SIZE: usize = 1 << 16;
const CPU: u64 = 0x3_0000_0000;
const CTX: u64 = 0x4_0000_0000;
const PAGES: u64 = 0x5_0000_0000;
const MEM1: u64 = 0x6_0000_0000;
const FALLBACK: u64 = 0x7_0000_0000;
const RETURN: u64 = 0x7_0000_1000;

struct Data {
    cpu: *mut Cpu,
}

pub struct UnicornBackend {
    uc: Unicorn<'static, Data>,
    code: Vec<u32>,
    _stack: Vec<u8>,
    ctx: Box<[u64; 512]>,
    /// (host address, size) mapped at CPU, MEM1 and PAGES.
    mapped: [(usize, usize); 3],
}

// SAFETY: the emulator and its buffers belong to this value alone and are used from one thread
// at a time (tools and tests only).
unsafe impl Send for UnicornBackend {}

fn round(n: usize) -> usize {
    (n + 0xFFF) & !0xFFF
}

impl UnicornBackend {
    /// A code area of `words` instructions.
    pub fn new(words: usize) -> UnicornBackend {
        let mut uc = Unicorn::new_with_data(Arch::ARM64, Mode::LITTLE_ENDIAN, Data { cpu: std::ptr::null_mut() }).expect("unicorn");
        let mut code = vec![0u32; round(words * 4) / 4];
        let mut stack = vec![0u8; STACK_SIZE];
        let mut ctx = Box::new([0u64; 512]);
        // SAFETY: each buffer is owned by the backend, never resized, and outlives `uc`.
        unsafe {
            uc.mem_map_ptr(CODE, (code.len() * 4) as u64, Prot::ALL, code.as_mut_ptr().cast()).unwrap();
            uc.mem_map_ptr(STACK, STACK_SIZE as u64, Prot::READ | Prot::WRITE, stack.as_mut_ptr().cast()).unwrap();
            uc.mem_map_ptr(CTX, 4096, Prot::READ | Prot::WRITE, ctx.as_mut_ptr().cast()).unwrap();
        }
        uc.mem_map(FALLBACK, 0x2000, Prot::ALL).unwrap();
        // `ret` at the handler address: the hook below has run the handler by then.
        uc.mem_write(FALLBACK, &0xD65F_03C0u32.to_le_bytes()).unwrap();
        uc.add_code_hook(FALLBACK, FALLBACK + 3, |uc, _addr, _size| {
            let cpu = uc.get_data().cpu;
            let hw = uc.reg_read(RegisterARM64::X1).unwrap() as *mut Hw;
            let op = uc.reg_read(RegisterARM64::X2).unwrap() as *const Op;
            // SAFETY: generated code passes the host pointers it was given, and the address
            // of an Op the compiler embedded from a live block.
            unsafe { fallback(cpu, hw, op) };
        })
        .unwrap();
        UnicornBackend { uc, code, _stack: stack, ctx, mapped: [(0, 0); 3] }
    }

    /// Maps host memory at a fixed guest address, again if it moved.
    fn map(&mut self, slot: usize, guest: u64, host: usize, size: usize) {
        let size = round(size);
        if self.mapped[slot] == (host, size) {
            return;
        }
        if self.mapped[slot].1 != 0 {
            self.uc.mem_unmap(guest, self.mapped[slot].1 as u64).unwrap();
        }
        // SAFETY: the host memory belongs to a live object the caller passed; only the part
        // the object covers is ever touched by generated code.
        unsafe { self.uc.mem_map_ptr(guest, size as u64, Prot::READ | Prot::WRITE, host as *mut _).unwrap() };
        self.mapped[slot] = (host, size);
    }
}

impl Backend for UnicornBackend {
    fn capacity(&self) -> usize {
        self.code.len()
    }

    fn write(&mut self, at: usize, words: &[u32]) {
        self.code[at..at + words.len()].copy_from_slice(words);
        let (a, b) = (CODE + 4 * at as u64, CODE + 4 * (at + words.len()) as u64);
        // Unicorn keeps translated code; drop what these instructions were part of.
        self.uc.ctl_remove_cache(a & !0xFFF, (b + 0xFFF) & !0xFFF).unwrap();
    }

    fn fallback(&self) -> u64 {
        FALLBACK
    }

    unsafe fn call(&mut self, at: usize, cpu: *mut Cpu, hw: *mut Hw, ctx: *const JitCtx, budget: u64) -> u64 {
        // SAFETY: the caller guarantees live pointers.
        let (mem1, pages, pages_len) = unsafe { ((*hw).mem1.as_mut_ptr() as usize, (*hw).code_pages.as_ptr() as usize, (*hw).code_pages.len() * 8) };
        let _ = ctx;
        self.map(0, CPU, cpu as usize, std::mem::size_of::<Cpu>());
        self.map(1, MEM1, mem1, crate::hw::MEM1_SIZE);
        self.map(2, PAGES, pages, pages_len);
        // The context as the code sees it: guest addresses.
        self.ctx[0] = MEM1;
        self.ctx[1] = PAGES;
        self.ctx[2] = FALLBACK;
        self.uc.get_data_mut().cpu = cpu;
        let regs = [(RegisterARM64::X0, CPU), (RegisterARM64::X1, hw as u64), (RegisterARM64::X2, CTX), (RegisterARM64::X3, budget), (RegisterARM64::SP, STACK + STACK_SIZE as u64), (RegisterARM64::X30, RETURN)];
        for (r, v) in regs {
            self.uc.reg_write(r, v).unwrap();
        }
        if let Err(e) = self.uc.emu_start(CODE + 4 * at as u64, RETURN, 0, 0) {
            let pc = self.uc.reg_read(RegisterARM64::PC).unwrap();
            panic!("generated code failed in the emulator: {e:?} at {pc:#x} (code word {})", (pc.wrapping_sub(CODE)) / 4);
        }
        self.uc.reg_read(RegisterARM64::X0).unwrap()
    }
}
