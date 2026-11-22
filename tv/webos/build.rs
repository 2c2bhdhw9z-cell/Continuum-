// The libretro log callback is a C variadic function (printf style). Stable Rust cannot define
// one, so a few lines of C format the message and hand Rust a finished line.
fn main() {
    println!("cargo:rerun-if-changed=src/log_shim.c");
    cc::Build::new().file("src/log_shim.c").warnings(false).compile("continuum_log_shim");
}
