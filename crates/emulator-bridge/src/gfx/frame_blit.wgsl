// Presents one emulator framebuffer to the swapchain.
//
// Deliberately minimal: a fullscreen-ish quad, one texture sample, no vertex
// buffers (positions come from `vertex_index`). Aspect correction and integer
// scaling arrive as a clip-space scale/offset in the uniform, so changing the
// scaling mode is a 32-byte buffer write, not a pipeline rebuild.
//
// Phase 1b hooks: shader-based CRT masks, scanlines and xBR-class upscalers all
// belong in `fs_main` behind additional pipeline variants.

struct Blit {
    // Clip-space scale applied to the quad. Letterboxing shrinks one axis.
    scale: vec2<f32>,
    // Clip-space translation. Non-zero only for off-centre integer scaling.
    offset: vec2<f32>,
    // Source framebuffer size in pixels; available for pixel-snapping effects.
    frame_size: vec2<f32>,
    _padding: vec2<f32>,
};

@group(0) @binding(0) var<uniform> blit: Blit;
@group(0) @binding(1) var frame_texture: texture_2d<f32>;
@group(0) @binding(2) var frame_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    // Two triangles covering NDC. A `var` (not `const`) so the dynamic index is
    // legal in WGSL.
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
    );

    let corner = corners[vertex_index];

    var out: VertexOutput;
    out.clip_position = vec4<f32>(corner * blit.scale + blit.offset, 0.0, 1.0);
    // NDC is y-up, texture space is y-down: flip v, or every frame renders
    // upside down.
    out.uv = vec2<f32>(corner.x * 0.5 + 0.5, 0.5 - corner.y * 0.5);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let rgb = textureSample(frame_texture, frame_sampler, in.uv).rgb;
    // Force opaque: cores emit XRGB where the alpha byte is garbage, and a
    // premultiplied-alpha canvas would show it as translucency.
    return vec4<f32>(rgb, 1.0);
}
