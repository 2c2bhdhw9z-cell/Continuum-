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
//! 2. Write the instructions.
//! 3. `sys_icache_invalidate`, which is **not optional on arm64**. The instruction cache does not
//!    observe data writes, so without it the processor may execute whatever was in that cache line
//!    before. Omitting it appears to work in the simulator and crashes on device, intermittently.
//!
//! There is deliberately no `pthread_jit_write_protect_np` in that list. It is macOS-only, does
//! not exist in the iOS SDK, and referencing it fails to LINK rather than failing at runtime,
//! which is how this probe learned it. See the note in `ios_aarch64`.
//!
//! The probe writes a function that returns 42 and calls it, twice: once asking for `MAP_JIT` and,
//! if that is refused, once without it. Returning 42 cannot happen by accident, and reporting
//! WHICH attempt succeeded is more useful than a yes or no, because it tells a future recompiler
//! port what to ask for.

/// What the probe found, as a sentence for the diagnostics HUD.
///
/// A string rather than an enum, because every outcome is something a person reads once and
/// nothing branches on. The caller shows it and does not interpret it.
/// # This one is safe to call at startup, and the other one is not
///
/// THE APP USED TO RUN THE FULL PROBE WHEN IT LAUNCHED, AND THAT MADE THE APP UNLAUNCHABLE.
/// Executing a page you just wrote is exactly what iOS kills a process for when the
/// dynamic-codesigning entitlement is not actually in force, and whether it is in force depends
/// on how the copy was signed and installed rather than on anything in this source. TrollStore
/// preserves those entitlements; a free developer account and the installers built on it do not.
/// So on those installs the app opened and was killed before it could draw the very line the
/// probe existed to print, which also silently blocked every other on-device test for several
/// builds.
///
/// This function therefore only asks whether the kernel will MAP such a page. That is a real
/// answer — if the mapping is refused, no recompiler can run — and `mmap` reports refusal by
/// returning a value rather than by having the process killed. Nothing is written and nothing is
/// executed. See [`describe_execution`] for the rest of the answer, which is now something the
/// owner asks for deliberately.
pub fn describe() -> String {
    #[cfg(all(target_os = "ios", target_arch = "aarch64"))]
    {
        ios_aarch64::probe_mapping()
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

/// The full answer: map a page, write a function into it, and CALL it.
///
/// **This can get the process killed, and that is not a bug in it.** Executing a page the process
/// just wrote is precisely the operation iOS forbids without the dynamic-codesigning entitlement
/// being honoured, and the kernel's answer to a forbidden execute is SIGKILL rather than an error
/// code. There is no way to ask "would this be allowed" other than doing it.
///
/// So this is never called on the launch path. It is reached only from an explicit control that
/// says what may happen, because an app that closes when you press a clearly labelled button is a
/// diagnosis, while an app that closes when you open it is just broken.
pub fn describe_execution() -> String {
    #[cfg(all(target_os = "ios", target_arch = "aarch64"))]
    {
        ios_aarch64::probe_execution()
    }
    #[cfg(not(all(target_os = "ios", target_arch = "aarch64")))]
    {
        "JIT: not probed, this build is not iOS arm64".to_string()
    }
}

#[cfg(all(target_os = "ios", target_arch = "aarch64"))]
mod ios_aarch64 {
    use core::ffi::{c_char, c_void};

    // `csops` below is resolved at RUNTIME through `dlsym` rather than declared for the linker.
    //
    // It is a private symbol. Declaring it in this block would make the LINKER responsible for
    // finding it, and a missing symbol there fails the build — which cannot be caught locally,
    // because `cargo check` does not link and the only linker for this target is CI. That exact
    // mistake cost two red builds with `pthread_jit_write_protect_np`. `dlsym` moves the question
    // to runtime, where a missing symbol is a `None` the read-out can report.
    extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    /// `RTLD_DEFAULT` on Darwin: search every loaded image in the normal order.
    const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

    /// `csops` operation for "give me this process's code signing status word".
    const CS_OPS_STATUS: u32 = 0;
    /// The process may be attached to by a debugger. THE ENTITLEMENT THAT DECIDES WHETHER JIT IS
    /// POSSIBLE, because JIT on iOS comes from being debugged and nothing may debug a process
    /// without it. Carried only by a development provisioning profile.
    const CS_GET_TASK_ALLOW: u32 = 0x0000_0004;
    /// A debugger is attached right now, which is when executable memory is actually permitted.
    const CS_DEBUGGED: u32 = 0x1000_0000;

    /// This process's code signing status word, or `None` if it could not be read.
    fn signing_status() -> Option<u32> {
        type Csops = unsafe extern "C" fn(i32, u32, *mut c_void, usize) -> i32;
        // SAFETY: the name is a NUL-terminated literal, and the resolved pointer is called with
        // exactly the signature `csops` is documented to have.
        unsafe {
            let symbol = dlsym(RTLD_DEFAULT, c"csops".as_ptr());
            if symbol.is_null() {
                return None;
            }
            let csops: Csops = core::mem::transmute(symbol);
            let mut status: u32 = 0;
            let rc = csops(
                0, // 0 means this process
                CS_OPS_STATUS,
                &mut status as *mut u32 as *mut c_void,
                core::mem::size_of::<u32>(),
            );
            if rc == 0 {
                Some(status)
            } else {
                None
            }
        }
    }

    /// What the signature says about this installed copy, as a sentence fragment.
    ///
    /// This is the half of the JIT question that has nothing to do with running code, and it is the
    /// half that is actually actionable: `get-task-allow` comes from the provisioning profile the
    /// copy was signed with, so a "no" here is fixed by re-signing with a development identity
    /// rather than by changing anything in this repository.
    pub fn signing_summary() -> String {
        match signing_status() {
            None => "signing flags unreadable".to_string(),
            Some(status) => {
                let debuggable = status & CS_GET_TASK_ALLOW != 0;
                let debugged = status & CS_DEBUGGED != 0;
                match (debuggable, debugged) {
                    (false, _) => "get-task-allow is MISSING, so nothing can attach a debugger \
                                   and no recompiler can run. Re-sign with a DEVELOPMENT \
                                   certificate and profile, not a distribution one"
                        .to_string(),
                    (true, false) => "get-task-allow is present, so this copy can receive JIT \
                                      once a debugger attaches. Nothing is attached yet: that is \
                                      StikDebug's job"
                        .to_string(),
                    (true, true) => "get-task-allow is present AND a debugger is attached, which \
                                     is the state a recompiler needs"
                        .to_string(),
                }
            }
        }
    }

    // Declared here rather than taking a dependency on `libc` for four symbols. The values are
    // from Apple's `sys/mman.h` and are stable ABI: changing them would break every compiled
    // binary on the platform, so they are not a version risk.
    const PROT_READ: i32 = 0x01;
    const PROT_WRITE: i32 = 0x02;
    const PROT_EXEC: i32 = 0x04;
    const MAP_PRIVATE: i32 = 0x0002;
    const MAP_ANON: i32 = 0x1000;
    /// The flag that asks for a mapping the hardened runtime will allow to be executable.
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
        fn sys_icache_invalidate(start: *mut c_void, len: usize);
    }

    // NOTE THE ABSENCE OF `pthread_jit_write_protect_np`, WHICH IS THE MOST USEFUL THING THIS
    // PROBE HAS ALREADY ESTABLISHED, and it cost two red builds to learn.
    //
    // That function is `macos(11.0)` only. It does not exist in the iOS SDK, and referencing it
    // fails to LINK for aarch64-apple-ios rather than failing at runtime. Apple Silicon macOS
    // refuses to have a page writable and executable at the same instant and needs the toggle to
    // choose; iOS with the JIT entitlement does not work that way, so there is nothing to toggle.
    //
    // parallel-n64's own header said so and I did not read it literally enough. Its comment is
    // "only necessary on macOS ARM because Apple restrictions on MAP_JIT pages". That is a
    // statement about which platform needs the call, not a note about where it happened to be
    // tested, and it means that core's arm64 recompiler needs LESS adaptation for iOS than for
    // the Mac it was written on, not more.

    /// `mov w0, #42` then `ret`, as arm64 machine code.
    ///
    /// The shortest function whose return value cannot happen by accident. A page that was never
    /// written, or never made executable, either traps or returns whatever was already there.
    ///
    /// `MOVZ W0, #42` is `0x52800000 | (42 << 5)`, and `RET` is `0xD65F03C0`. Written as bytes so
    /// the order in memory is explicit rather than depending on how a `u32` is laid out.
    const RETURN_42: [u8; 8] = [
        0x40, 0x05, 0x80, 0x52, // mov w0, #42
        0xC0, 0x03, 0x5F, 0xD6, // ret
    ];

    const LEN: usize = 4096;

    /// Asks only whether an executable page can be MAPPED. Never writes, never executes.
    ///
    /// Safe to call anywhere, including at launch, because every outcome here is a return value.
    /// It is also genuinely informative in the negative: a refused mapping means no recompiler can
    /// run at all. A successful mapping is necessary but not sufficient, which the wording says.
    pub fn probe_mapping() -> String {
        // The signing half comes first, because it is the one with an action attached to it. A
        // mapping result is interesting; a missing `get-task-allow` is a thing to go and fix.
        let signing = signing_summary();
        let with_jit = can_map(MAP_PRIVATE | MAP_ANON | MAP_JIT);
        let plain = can_map(MAP_PRIVATE | MAP_ANON);
        let mapping = match (with_jit, plain) {
            (true, _) => "executable pages map with MAP_JIT",
            (false, true) => "MAP_JIT refused, plain executable mapping accepted",
            (false, false) => "no executable page could be mapped at all",
        };
        format!("JIT: {signing}. Mapping: {mapping}.")
    }

    /// One mapping attempt, immediately released. Nothing is written to the page.
    fn can_map(flags: i32) -> bool {
        // SAFETY: a fresh anonymous mapping, never dereferenced, unmapped before returning.
        unsafe {
            let page = mmap(
                core::ptr::null_mut(),
                LEN,
                PROT_READ | PROT_WRITE | PROT_EXEC,
                flags,
                -1,
                0,
            );
            if page.is_null() || page as isize == -1 {
                return false;
            }
            munmap(page, LEN);
            true
        }
    }

    pub fn probe_execution() -> String {
        // TRIED TWO WAYS, AND REPORTING WHICH ONE WORKED IS THE POINT. A yes or no would say
        // whether a recompiler is possible; naming the mechanism says what a recompiler has to
        // ASK FOR, which is the thing the port actually needs to know. On a sideloaded build the
        // answer depends on how the app was signed and which entitlements survived, so it is a
        // property of the installed copy rather than of this source.
        match attempt(MAP_PRIVATE | MAP_ANON | MAP_JIT) {
            Ok(()) => {
                return "JIT: working with MAP_JIT. An executable page was mapped, written, \
                        invalidated and called."
                    .to_string()
            }
            Err(with_jit) => {
                // Without MAP_JIT. Worth trying, because a plain read-write-execute mapping is
                // permitted on some configurations and is all a recompiler needs; if this is the
                // one that works, the port simply does not pass the flag.
                match attempt(MAP_PRIVATE | MAP_ANON) {
                    Ok(()) => "JIT: working WITHOUT MAP_JIT, a plain executable mapping was \
                               accepted."
                        .to_string(),
                    Err(plain) => format!(
                        "JIT: NOT AVAILABLE. With MAP_JIT: {with_jit}. Without it: {plain}. No \
                         recompiler can run, so N64 is not possible on this installed build."
                    ),
                }
            }
        }
    }

    /// One attempt with a given set of mmap flags. `Ok(())` means a page was written and executed
    /// and returned the expected answer.
    fn attempt(flags: i32) -> Result<(), String> {
        // SAFETY: a fresh anonymous mapping, used only through the pointer returned, unmapped on
        // every exit path, never aliased. The function written into it takes no arguments and
        // returns a `u32`, which is the signature it is called through.
        unsafe {
            let page = mmap(
                core::ptr::null_mut(),
                LEN,
                PROT_READ | PROT_WRITE | PROT_EXEC,
                flags,
                -1,
                0,
            );
            // `mmap` reports failure as `MAP_FAILED`, which is `-1` rather than null.
            if page.is_null() || page as isize == -1 {
                return Err("the page could not be mapped".to_string());
            }

            core::ptr::copy_nonoverlapping(RETURN_42.as_ptr(), page as *mut u8, RETURN_42.len());

            // NOT OPTIONAL ON ARM64. The instruction cache does not observe the write above, so
            // skipping this executes whatever that cache line held before. It appears to work in
            // the simulator and crashes on device, intermittently, which is the worst failure
            // mode this sequence has.
            sys_icache_invalidate(page, RETURN_42.len());

            let entry: extern "C" fn() -> u32 = core::mem::transmute(page);
            let answer = entry();
            munmap(page, LEN);

            if answer == 42 {
                Ok(())
            } else {
                // Reached only if the page executed something other than what was written, which
                // in practice means the cache invalidation did not take.
                Err(format!("the page ran but returned {answer} instead of 42"))
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
