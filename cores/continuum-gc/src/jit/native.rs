//! Generated code run for real, on an ARM64 processor.
//!
//! On the iPhone the code memory comes from the Continuum app (`continuum_jit_region`, found
//! by name at run time): two addresses for the same memory, one to run code from and one to
//! write it through, which is what iOS allows once JIT is on (and the only shape it allows on
//! iOS 26 with TXM). When the app says no (JIT off), there is no JIT and the fast interpreter
//! runs instead. On ARM64 Linux (the test machine runs that under an emulator) one mapping
//! that is both writable and executable is enough.

use super::compile::JitCtx;
use super::runtime::{fallback, Backend};
use crate::cpu::Cpu;
use crate::hw::Hw;
use std::ffi::{c_char, c_void};
use std::ptr::null_mut;

extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    #[cfg(not(target_vendor = "apple"))]
    fn mmap(addr: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut c_void;
    #[cfg(not(target_vendor = "apple"))]
    fn munmap(addr: *mut c_void, len: usize) -> i32;
    #[cfg(target_vendor = "apple")]
    fn sys_icache_invalidate(start: *mut c_void, len: usize);
}

#[cfg(target_vendor = "apple")]
const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;
#[cfg(not(target_vendor = "apple"))]
const RTLD_DEFAULT: *mut c_void = null_mut();

type RegionFn = unsafe extern "C" fn(*const c_char, usize, *mut *mut c_void, *mut *mut c_void) -> bool;
type ReleaseFn = unsafe extern "C" fn(*mut c_void);

pub struct NativeBackend {
    rx: *mut u8,
    rw: *mut u8,
    bytes: usize,
    release: Option<ReleaseFn>,
    mapped: bool,
}

// SAFETY: the code memory belongs to this value alone, and the core uses it from one thread at
// a time (the machine sits behind the libretro face's lock).
unsafe impl Send for NativeBackend {}

impl NativeBackend {
    /// Code memory of `bytes` bytes, or None when this process may not run generated code.
    pub fn new(bytes: usize) -> Option<NativeBackend> {
        // SAFETY: looking up symbols by name, and calling the app's function with the
        // signature its header gives (crates/emulator-bridge/src/jit26.c).
        unsafe {
            let region = dlsym(RTLD_DEFAULT, c"continuum_jit_region".as_ptr());
            if !region.is_null() {
                let region: RegionFn = std::mem::transmute::<*mut c_void, RegionFn>(region);
                let (mut rx, mut rw) = (null_mut(), null_mut());
                if !region(c"continuum_gc".as_ptr(), bytes, &mut rx, &mut rw) || rx.is_null() || rw.is_null() {
                    return None;
                }
                let release = dlsym(RTLD_DEFAULT, c"continuum_jit_release".as_ptr());
                let release = (!release.is_null()).then(|| std::mem::transmute::<*mut c_void, ReleaseFn>(release));
                return Some(NativeBackend { rx: rx.cast(), rw: rw.cast(), bytes, release, mapped: false });
            }
            #[cfg(not(target_vendor = "apple"))]
            {
                // PROT_READ | PROT_WRITE | PROT_EXEC, MAP_PRIVATE | MAP_ANONYMOUS.
                let p = mmap(null_mut(), bytes, 7, 0x22, -1, 0);
                if p.is_null() || p as isize == -1 {
                    return None;
                }
                Some(NativeBackend { rx: p.cast(), rw: p.cast(), bytes, release: None, mapped: true })
            }
            #[cfg(target_vendor = "apple")]
            None
        }
    }
}

impl Drop for NativeBackend {
    fn drop(&mut self) {
        // SAFETY: giving back exactly what `new` got.
        unsafe {
            if let Some(release) = self.release {
                release(self.rx.cast());
            }
            #[cfg(not(target_vendor = "apple"))]
            if self.mapped {
                munmap(self.rx.cast(), self.bytes);
            }
        }
        let _ = self.mapped;
    }
}

/// Makes freshly written instructions visible to instruction fetch: clean the data cache
/// through the address they were written at, invalidate the instruction cache at the address
/// they run from.
///
/// # Safety
/// Both ranges must be mapped and be the same memory.
unsafe fn flush(rw: *mut u8, rx: *mut u8, len: usize) {
    #[cfg(target_vendor = "apple")]
    // SAFETY: the system call for exactly this.
    unsafe {
        let _ = rw;
        sys_icache_invalidate(rx.cast(), len);
    }
    #[cfg(not(target_vendor = "apple"))]
    // SAFETY: cache maintenance on mapped memory; no memory is changed.
    unsafe {
        let ctr: u64;
        std::arch::asm!("mrs {}, ctr_el0", out(reg) ctr);
        let dline = 4usize << ((ctr >> 16) & 15);
        let iline = 4usize << (ctr & 15);
        let mut p = rw as usize & !(dline - 1);
        while p < rw as usize + len {
            std::arch::asm!("dc cvau, {}", in(reg) p);
            p += dline;
        }
        std::arch::asm!("dsb ish");
        let mut p = rx as usize & !(iline - 1);
        while p < rx as usize + len {
            std::arch::asm!("ic ivau, {}", in(reg) p);
            p += iline;
        }
        std::arch::asm!("dsb ish", "isb");
    }
}

impl Backend for NativeBackend {
    fn capacity(&self) -> usize {
        self.bytes / 4
    }

    fn write(&mut self, at: usize, words: &[u32]) {
        assert!(at + words.len() <= self.capacity(), "code area overrun");
        // SAFETY: inside the area (checked above), written through its writable address.
        unsafe {
            std::ptr::copy_nonoverlapping(words.as_ptr(), self.rw.cast::<u32>().add(at), words.len());
            flush(self.rw.add(at * 4), self.rx.add(at * 4), words.len() * 4);
        }
    }

    fn fallback(&self) -> u64 {
        fallback as usize as u64
    }

    unsafe fn call(&mut self, at: usize, cpu: *mut Cpu, hw: *mut Hw, ctx: *const JitCtx, budget: u64) -> u64 {
        type Entry = unsafe extern "C" fn(*mut Cpu, *mut Hw, *const JitCtx, u64) -> u64;
        // SAFETY: the caller guarantees compiled code at `at`, which follows this signature.
        unsafe {
            let f = std::mem::transmute::<*mut u8, Entry>(self.rx.add(at * 4));
            f(cpu, hw, ctx, budget)
        }
    }
}
