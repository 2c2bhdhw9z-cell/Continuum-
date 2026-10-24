//! Our own ARM64 JIT (gamecube.json G8).

pub mod a64;
pub mod compile;
#[cfg(target_arch = "aarch64")]
pub mod native;
pub mod runtime;
#[cfg(test)]
mod tests;
#[cfg(feature = "unicorn")]
pub mod unicorn;

/// Code memory for the JIT: the real thing on an ARM64 processor; on the x86 test machine the
/// ARM64 emulator when built with the `unicorn` feature; otherwise none.
pub fn backend(bytes: usize) -> Option<Box<dyn runtime::Backend>> {
    #[cfg(target_arch = "aarch64")]
    {
        native::NativeBackend::new(bytes).map(|b| Box::new(b) as Box<dyn runtime::Backend>)
    }
    #[cfg(all(not(target_arch = "aarch64"), feature = "unicorn"))]
    {
        Some(Box::new(unicorn::UnicornBackend::new(bytes / 4)))
    }
    #[cfg(all(not(target_arch = "aarch64"), not(feature = "unicorn")))]
    {
        let _ = bytes;
        None
    }
}
