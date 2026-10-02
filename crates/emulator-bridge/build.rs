fn main() {
    // Only needed when the native libretro host is compiled: that is the sole caller of the
    // GET_LOG_INTERFACE trampoline. Host unit tests without `native-core` skip this.
    if std::env::var_os("CARGO_FEATURE_NATIVE_CORE").is_none() {
        return;
    }

    println!("cargo:rerun-if-changed=src/cores/retro_log_shim.c");
    cc::Build::new()
        .file("src/cores/retro_log_shim.c")
        .warnings(true)
        .compile("continuum_retro_log_shim");
}
