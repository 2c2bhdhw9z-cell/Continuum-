// Composites one or more emulator screens onto the swapchain.
//
// Deliberately minimal: two triangles per screen, one texture sample, no vertex buffers
// (positions come from `vertex_index`). Aspect correction, integer scaling and the screen
// layout all arrive as uniform data, so changing any of them is a buffer write rather than a
// pipeline rebuild.
//
// ## Why this is instanced rather than one quad
//
// It was one quad until the hardware-render work needed it not to be. Two things are coming
// that a single fullscreen quad cannot express:
//
//   - The DS and the 3DS hand over ONE framebuffer containing TWO screens, stacked. Presenting
//     them means drawing two different regions of one texture to two different places, which is
//     the same draw with different numbers rather than a second pass.
//   - A hardware-rendered core draws into a texture we hand it, and that texture is not
//     necessarily the shape of the window it ends up in.
//
// So each screen is one instance, carrying where it comes from and where it goes. With one
// screen and an identity source rect this is byte-for-byte the geometry the single-quad version
// produced, which is what lets the nine systems already shipping stay untouched.
//
// Phase 1b hooks: shader-based CRT masks, scanlines and xBR-class upscalers all belong in
// `fs_main` behind additional pipeline variants.

// Matches `MAX_SCREENS` in renderer.rs. Four rather than two: the DS and 3DS need two, and a
// fixed-size uniform array costs 32 bytes per unused slot, which is not worth a second layout
// to reclaim.
const MAX_SCREENS: u32 = 4u;

// One screen's placement, packed into two vec4s rather than four vec2s.
//
// PACKED FOR ALIGNMENT, NOT FOR SIZE. WGSL requires an array element in the uniform address
// space to be 16-byte aligned, and a struct of vec2s has 8-byte alignment, which either fails
// to compile or silently acquires padding that the Rust side then disagrees with. A vec4 is
// 16-byte aligned by definition, so this shape cannot drift out of step with `ScreenUniform`.
struct Screen {
    // xy: clip-space scale of the quad. zw: clip-space translation.
    dest: vec4<f32>,
    // xy: scale applied to the source texture coordinate. zw: its translation.
    // Identity is (1, 1, 0, 0), which samples the whole texture.
    source: vec4<f32>,
};

struct Blit {
    // Source framebuffer size in pixels; available for pixel-snapping effects.
    frame_size: vec2<f32>,
    // How many entries of `screens` are live. The draw issues exactly this many instances, so
    // an out-of-range instance cannot be reached, and this is here for the fragment stage and
    // for anyone reading a capture of the buffer.
    screen_count: u32,
    _padding: u32,
    screens: array<Screen, 4>,
};

@group(0) @binding(0) var<uniform> blit: Blit;
@group(0) @binding(1) var frame_texture: texture_2d<f32>;
@group(0) @binding(2) var frame_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    // Two triangles covering NDC. A `var` (not `const`) so the dynamic index is legal in WGSL.
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
    );

    let corner = corners[vertex_index];
    // Clamped rather than trusted. The draw call already bounds the instance count, so this is
    // unreachable today; it costs one instruction and removes a class of out-of-bounds read
    // that would otherwise depend on a draw call somewhere else staying correct.
    let screen = blit.screens[min(instance_index, MAX_SCREENS - 1u)];

    var out: VertexOutput;
    out.clip_position = vec4<f32>(corner * screen.dest.xy + screen.dest.zw, 0.0, 1.0);
    // NDC is y-up, texture space is y-down: flip v, or every frame renders upside down. The
    // flip happens BEFORE the source rect is applied, so a source offset is expressed in
    // ordinary top-down texture coordinates and the top screen of a stacked pair is the one
    // with the smaller v.
    let base_uv = vec2<f32>(corner.x * 0.5 + 0.5, 0.5 - corner.y * 0.5);
    out.uv = base_uv * screen.source.xy + screen.source.zw;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let rgb = textureSample(frame_texture, frame_sampler, in.uv).rgb;
    // Force opaque: cores emit XRGB where the alpha byte is garbage, and a premultiplied-alpha
    // canvas would show it as translucency.
    return vec4<f32>(rgb, 1.0);
}
