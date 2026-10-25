//! WGSL to SPIR-V for the GPU drawer's kernels (draw.wgsl beside this file), shared by
//! examples/shaders.rs (which writes src/gx/gpu/draw.spv, the file the core embeds) and the GPU
//! tests (which check that file is up to date). A development tool: naga is only a
//! dev-dependency, so the phone build never compiles a shader.

/// The SPIR-V for `wgsl`, as bytes (little-endian words).
pub fn compile(wgsl: &str) -> Result<Vec<u8>, String> {
    let module = naga::front::wgsl::parse_str(wgsl).map_err(|e| e.emit_to_string(wgsl))?;
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::IMMEDIATES)
        .validate(&module)
        .map_err(|e| e.emit_to_string(wgsl))?;
    let mut options = naga::back::spv::Options { lang_version: (1, 0), flags: naga::back::spv::WriterFlags::empty(), ..Default::default() };
    // Every index into a buffer or an array is clamped into it: a drawing bug can draw the wrong
    // thing, never read or write outside a buffer (which on a phone stops the graphics chip).
    options.bounds_check_policies = naga::proc::BoundsCheckPolicies {
        index: naga::proc::BoundsCheckPolicy::Restrict,
        buffer: naga::proc::BoundsCheckPolicy::Restrict,
        image_load: naga::proc::BoundsCheckPolicy::Restrict,
        binding_array: naga::proc::BoundsCheckPolicy::Restrict,
    };
    options.zero_initialize_workgroup_memory = naga::back::spv::ZeroInitializeWorkgroupMemoryMode::None;
    options.force_loop_bounding = false;
    let words = naga::back::spv::write_vec(&module, &info, &options, None).map_err(|e| e.to_string())?;
    Ok(words.iter().flat_map(|w| w.to_le_bytes()).collect())
}
