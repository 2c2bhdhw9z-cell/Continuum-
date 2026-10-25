//! Compiles the GPU drawer's kernels: src/gx/gpu/draw.wgsl to src/gx/gpu/draw.spv, which the core
//! embeds. Run after changing the WGSL (the GPU tests fail while the two disagree):
//!
//! cargo run --example shaders           write draw.spv
//! cargo run --example shaders -- check  only say whether draw.spv is up to date
#[path = "../src/gx/gpu/shader_build.rs"]
mod shader_build;

fn main() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/gx/gpu");
    let wgsl = std::fs::read_to_string(format!("{dir}/draw.wgsl")).expect("draw.wgsl");
    let spv = match shader_build::compile(&wgsl) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let path = format!("{dir}/draw.spv");
    if std::env::args().nth(1).as_deref() == Some("check") {
        let same = std::fs::read(&path).is_ok_and(|old| old == spv);
        eprintln!("draw.spv {}", if same { "is up to date" } else { "is OUT OF DATE" });
        std::process::exit(if same { 0 } else { 1 });
    }
    std::fs::write(&path, &spv).expect("write draw.spv");
    eprintln!("wrote {path} ({} bytes)", spv.len());
}
