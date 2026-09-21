//! Does this app actually have a working JIT on this device?
//!
//! ## Why this exists, and why it is not part of any core
//!
//! Every N64 core needs a recompiler: the interpreter is far too slow to be playable, which is
//! the opposite of the PlayStation, where the interpreter is fine and is what ships today. So
//! N64 depends on writing instructions into memory at runtime and jumping to them, and on iOS
//! that is not something an app may simply do. It needs the `allow-jit` entitlement, and it
//! needs a specific sequence that has no equivalent on other platforms.
//!
//! **The entitlements being present is not the same as the JIT working, and this project has
//! never executed that path even once.** `native/ios/Continuum.entitlements` has carried
//! `allow-jit` and `allow-unsigned-executable-memory` since the first build, and nothing has
//! ever allocated an executable page to find out whether they take effect. On a sideloaded
//! build the entitlements are only as good as the signature that carried them, so this is a
//! question about the installed app rather than about the source.
//!
//! ## Why a probe rather than switching a core's recompiler on
//!
//! The obvious experiment is to rebuild the PlayStation core with its dynarec enabled and see
//! whether it still runs. That was the plan, and reading the cores made it clear it would not
//! have answered the question:
//!
//! - `pcsx_rearmed` contains no reference to `MAP_JIT`, `pthread_jit_write_protect_np` or
//!   `sys_icache_invalidate` anywhere. Its `Makefile.libretro` sets `DYNAREC = 0` for
//!   `ios-arm64`, and that is not an App Store concession: the recompiler has no Apple support
//!   to enable. Forcing it on would have built, then failed at the first executable page, and
//!   the failure would have looked like a broken core rather than a missing port.
//! - `mupen64plus-libretro-nx` has none either.
//! - `parallel-n64` **does**, in
//!   `mupen64plus-core/src/device/r4300/new_dynarec/arm64/apple_jit_protect.h`, whose own
//!   comment says it is "only necessary on macOS ARM". So the code exists and runs on Apple
//!   Silicon Macs, while that core's iOS block sets `WITH_DYNAREC=` empty.
//!
//! Which means the honest cheap experiment is not in a core at all. It is this: perform the
//! exact sequence in our own process, in about forty lines, and report what happened. If it
//! fails here it will fail in any recompiler, and we learn that from a HUD line rather than
//! from a core that crashes for one of five possible reasons.
//!
//! ## The sequence, and the part that is not optional
//!
//! 1. `mmap` with `MAP_JIT`, requesting read, write and execute.
//! 2. `pthread_jit_write_protect_np(0)` to make the page writable on this thread. Apple
//!    hardware does not allow a page to be writable and executable at the same instant, so this
//!    toggles which one it is, per thread.
//! 3. Write the instructions.
//! 4. `pthread_jit_write_protect_np(1)` to make it executable again.
//! 5. `sys_icache_invalidate`, which is **not optional on arm64**. The instruction cache does
//!    not observe data writes, so without it the processor may execute whatever was in that
//!    cache line before. Omitting it appears to work in the simulator and crashes on device,
//!    intermittently, which is the worst failure mode this sequence has.
//!
//! The probe writes a function that returns 42 and then calls it. Returning 42 proves all five
//! steps worked, because there is no way to get that answer without the page being both written
//! and executed.

/// What the probe found, as a sentence for the diagnostics HUD.
///
/// A string rather than an enum, because every outcome is something a person reads once and
/// nothing branches on. The caller shows it and does not interpret it.
pub fn describe() -> String {
    #[cfg(all(target_os = "ios", target_arch = "aarch64"))]
    {
        ios_aarch64::probe()
    }
    #[cfg(not(all(target_os = "ios", target_arch = "aarch64")))]
    {
        // The host test machine, and also the macOS build CI does purely to generate the Swift
        // bindings. GATED ON `target_os = "ios"` RATHER THAN `target_vendor = "apple"`, which is
        // a distinction that cost one red build: macOS arm64 is an Apple arm64 target, so the
        // wider gate pulled this module into that build, where `pthread_jit_write_protect_np`
        // needs macOS 11 and the host build's deployment target is lower. The link failed for a
        // symbol nothing on that target would ever have called.
        //
        // Said plainly rather than pretending to have run, because a probe that reported success
        // where it had done nothing would be worse than useless.
        "JIT: not probed, this build is not iOS arm64".to_string()
    }
}

#[cfg(all(target_os = "ios", target_arch = "aarch64"))]
mod ios_aarch64 {
    use core::ffi::c_void;

    // Declared here rather than taking a dependency on `libc` for five symbols. The values are
    // from Apple's `sys/mman.h` and are stable ABI: changing them would break every compiled
    // binary on the platform, so they are not a version risk.
    const PROT_READ: i32 = 0x01;
    const PROT_WRITE: i32 = 0x02;
    const PROT_EXEC: i32 = 0x04;
    const MAP_PRIVATE: i32 = 0x0002;
    const MAP_ANON: i32 = 0x1000;
    /// The flag that makes an executable mapping legal at all under the hardened runtime.
    const MAP_JIT: i32 = 0x0800;

    unsafe extern "C" {
        fn mmap(
            addr: *mut c_void,
            len: usize,
            prot: i32,
            flags: i32,
            fd: i32,
            offset: i64,
        ) -> *mut c_void;
        fn munmap(addr: *mut c_void, len: usize) -> i32;
        /// `0` allows writes on this thread, `1` restores execute protection. Per thread, not
        /// per page, which is why the toggle brackets the write rather than the mapping.
        fn pthread_jit_write_protect_np(enabled: i32);
        fn sys_icache_invalidate(start: *mut c_void, len: usize);
    }

    /// `mov w0, #42` then `ret`, as arm64 machine code.
    ///
    /// Chosen because it is the shortest function whose return value cannot happen by accident.
    /// A page that was never written, or never made executable, cannot produce 42: it either
    /// traps or returns whatever was already there.
    ///
    /// `MOVZ W0, #42` is `0x52800000 | (42 << 5)`, and `RET` is `0xD65F03C0`. Written as bytes
    /// rather than words so the order in memory is explicit and does not depend on how a `u32`
    /// happens to be laid out.
    const RETURN_42: [u8; 8] = [
        0x40, 0x05, 0x80, 0x52, // mov w0, #42
        0xC0, 0x03, 0x5F, 0xD6, // ret
    ];

    pub fn probe() -> String {
        // One page is plenty for eight bytes, and `mmap` rounds up regardless.
        const LEN: usize = 4096;

        // SAFETY: a fresh anonymous mapping, used only through the pointer returned, unmapped
        // on every exit path below, and never aliased. The function written into it takes no
        // arguments and returns a `u32`, which is the signature it is called through.
        unsafe {
            let page = mmap(
                core::ptr::null_mut(),
                LEN,
                PROT_READ | PROT_WRITE | PROT_EXEC,
                MAP_PRIVATE | MAP_ANON | MAP_JIT,
                -1,
                0,
            );
            // `mmap` reports failure as `MAP_FAILED`, which is `-1` rather than null.
            if page.is_null() || page as isize == -1 {
                return "JIT: REFUSED, an executable page could not be mapped. The allow-jit \
                        entitlement is missing from the installed app, or the signature did not \
                        carry it."
                    .to_string();
            }

            // Writable on this thread. Without this the store below faults, because the page is
            // executable and Apple hardware will not have it both ways at once.
            pthread_jit_write_protect_np(0);
            core::ptr::copy_nonoverlapping(RETURN_42.as_ptr(), page as *mut u8, RETURN_42.len());
            pthread_jit_write_protect_np(1);

            // NOT OPTIONAL. See the module note: the instruction cache does not observe the
            // write above, so skipping this executes whatever that cache line held before.
            sys_icache_invalidate(page, RETURN_42.len());

            let entry: extern "C" fn() -> u32 = core::mem::transmute(page);
            let answer = entry();
            munmap(page, LEN);

            if answer == 42 {
                "JIT: working. An executable page was mapped, written and called.".to_string()
            } else {
                // Reached only if the page executed something other than what was written,
                // which in practice means the cache invalidation did not take.
                format!(
                    "JIT: WRONG ANSWER, the page executed but returned {answer} instead of 42, \
                     so it ran something other than what was written."
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_probe_reports_something_on_every_target() {
        // On the host this asserts only that it does not crash and says it did not run, which is
        // the point: a probe that claimed success without executing anything would be worse than
        // having none. The real answer can only come from a device.
        let line = super::describe();
        assert!(line.starts_with("JIT:"), "unexpected probe line: {line}");
        #[cfg(not(all(target_os = "ios", target_arch = "aarch64")))]
        assert!(line.contains("not probed"));
    }
}
