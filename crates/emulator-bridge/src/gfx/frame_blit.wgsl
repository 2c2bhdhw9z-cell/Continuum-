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
// Post-process looks (scanlines, CRT, LCD grid, dot matrix, sharp bilinear) and the rotation live in
// `fs_main`, selected by the `post` block, so switching look is a uniform write, not a pipeline.

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

// The post-process block: which look, its strengths, the rotation and the target's size.
//
// Three vec4s, after the screen array, so nothing before it moves: `screens` still starts 16 bytes
// in and every placement test in renderer.rs reads exactly the bytes it always did.
struct Post {
    // x: effect (0 none, 1 smooth, 2 sharp bilinear, 3 scanlines, 4 CRT, 5 LCD grid,
    //    6 dot matrix). y: quarter turns counter-clockwise (SET_ROTATION plus the user's). zw: 0.
    mode: vec4<u32>,
    // x: brightness multiplier. y: CRT curvature. z: scanline or grid strength. w: CRT mask.
    params: vec4<f32>,
    // xy: the target's size in pixels. z: CRT vignette. w: 0.
    target_size: vec4<f32>,
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
    post: Post,
};

@group(0) @binding(0) var<uniform> blit: Blit;
@group(0) @binding(1) var frame_texture: texture_2d<f32>;
@group(0) @binding(2) var frame_sampler: sampler;

const EFFECT_NONE: u32 = 0u;
const EFFECT_SMOOTH: u32 = 1u;
const EFFECT_SHARP_BILINEAR: u32 = 2u;
const EFFECT_SCANLINES: u32 = 3u;
const EFFECT_CRT: u32 = 4u;
const EFFECT_LCD: u32 = 5u;
const EFFECT_DOT_MATRIX: u32 = 6u;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    // Where in the quad this fragment is, 0..1, top-left origin, as DISPLAYED (before rotation).
    @location(0) local: vec2<f32>,
    // This screen's source rect: xy scale, zw offset, in texture coordinates.
    @location(1) @interpolate(flat) source: vec4<f32>,
    // Output pixels per source texel along each displayed axis. Sharp bilinear and the grids use it.
    @location(2) @interpolate(flat) scale: vec2<f32>,
};

// Displayed quad position -> unrotated position. Rotation r turns the IMAGE r quarter turns
// counter-clockwise (libretro.h:735), so the inverse is applied to the lookup.
fn unrotate(p: vec2<f32>, r: u32) -> vec2<f32> {
    switch (r % 4u) {
        case 1u: { return vec2<f32>(1.0 - p.y, p.x); }
        case 2u: { return vec2<f32>(1.0 - p.x, 1.0 - p.y); }
        case 3u: { return vec2<f32>(p.y, 1.0 - p.x); }
        default: { return p; }
    }
}

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
    // with the smaller v. The source rect itself is applied in the fragment stage now, after the
    // rotation and the CRT curve, which both work in displayed quad space.
    out.local = vec2<f32>(corner.x * 0.5 + 0.5, 0.5 - corner.y * 0.5);
    out.source = screen.source;
    // Quad size in pixels over source texels, with the axes swapped for a quarter turn.
    let quad_px = abs(screen.dest.xy) * blit.post.target_size.xy;
    var texels = abs(screen.source.xy) * blit.frame_size;
    if (blit.post.mode.y % 2u == 1u) {
        texels = texels.yx;
    }
    out.scale = max(quad_px / max(texels, vec2<f32>(1.0, 1.0)), vec2<f32>(1.0, 1.0));
    return out;
}

// Barrel distortion around the centre, for the CRT look. `k` 0 is flat.
fn curve(p: vec2<f32>, k: f32) -> vec2<f32> {
    let c = p * 2.0 - 1.0;
    // Each axis bends by the square of the other, the way a tube face curves.
    let bent = c * (vec2<f32>(1.0, 1.0) + k * vec2<f32>(c.y * c.y, c.x * c.x));
    return bent * 0.5 + 0.5;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let effect = blit.post.mode.x;
    let rotation = blit.post.mode.y;
    let brightness = blit.post.params.x;
    let strength = clamp(blit.post.params.z, 0.0, 1.0);

    var local = in.local;
    var inside = 1.0;
    if (effect == EFFECT_CRT) {
        local = curve(local, blit.post.params.y);
        // Outside the bent tube is black. A multiplier rather than an early return keeps the sample
        // below in uniform control flow, which is what naga's validator (and Metal) want.
        let edge = step(vec2<f32>(0.0, 0.0), local) * step(local, vec2<f32>(1.0, 1.0));
        inside = edge.x * edge.y;
        local = clamp(local, vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0));
    }

    let unrotated = unrotate(local, rotation);
    var uv = unrotated * in.source.xy + in.source.zw;
    let texel = uv * blit.frame_size;
    // Scale along the SOURCE axes, so the grid follows the source pixels through a rotation.
    var scale = in.scale;
    if (rotation % 2u == 1u) {
        scale = scale.yx;
    }

    if (effect == EFFECT_SHARP_BILINEAR) {
        // Nearest inside each texel, a one-output-pixel bilinear seam between them: sharp at any
        // scale without the uneven pixel widths of plain nearest at a non-integer factor.
        let base = floor(texel);
        let f = texel - base;
        let region = vec2<f32>(0.5, 0.5) - vec2<f32>(0.5, 0.5) / scale;
        let g = clamp((f - region) / max(vec2<f32>(1.0, 1.0) - 2.0 * region, vec2<f32>(1e-4, 1e-4)), vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0));
        uv = (base + g) / max(blit.frame_size, vec2<f32>(1.0, 1.0));
    }

    // Level 0 explicitly: no derivatives, so the sample is legal whatever the branch above did.
    var rgb = textureSampleLevel(frame_texture, frame_sampler, uv, 0.0).rgb;
    let cell = fract(texel);

    if (effect == EFFECT_SCANLINES || effect == EFFECT_CRT) {
        // Bright in the middle of each source row, dark at its edges.
        let row = sin(3.14159265 * cell.y);
        rgb = rgb * mix(1.0 - strength, 1.0, row * row);
    }
    if (effect == EFFECT_CRT) {
        // Aperture grille on the OUTPUT pixels: each third of a triad favours one primary.
        let mask_strength = clamp(blit.post.params.w, 0.0, 1.0);
        let column = u32(floor(in.clip_position.x)) % 3u;
        var mask = vec3<f32>(1.0 - mask_strength, 1.0 - mask_strength, 1.0 - mask_strength);
        if (column == 0u) { mask.x = 1.0; }
        if (column == 1u) { mask.y = 1.0; }
        if (column == 2u) { mask.z = 1.0; }
        // The mask removes light; lift the result back so the picture is not just darker.
        rgb = rgb * mask * (1.0 + mask_strength * 0.6);
        // Vignette: corners fall off.
        let v = local * (vec2<f32>(1.0, 1.0) - local);
        let vignette = pow(clamp(v.x * v.y * 16.0, 0.0, 1.0), clamp(blit.post.target_size.z, 0.0, 1.0));
        rgb = rgb * vignette * inside;
    }
    if (effect == EFFECT_LCD || effect == EFFECT_DOT_MATRIX) {
        // Gap width is about one output pixel, never more than a third of the cell.
        let w = clamp(vec2<f32>(1.0, 1.0) / scale, vec2<f32>(0.04, 0.04), vec2<f32>(0.33, 0.33));
        let gx = smoothstep(vec2<f32>(0.0, 0.0), w, cell) * smoothstep(vec2<f32>(0.0, 0.0), w, vec2<f32>(1.0, 1.0) - cell);
        let grid = gx.x * gx.y;
        if (effect == EFFECT_LCD) {
            rgb = rgb * mix(1.0 - strength, 1.0, grid);
        } else {
            // A Game Boy screen: the gaps show the pale backing, and the dots are slightly round.
            let d = distance(cell, vec2<f32>(0.5, 0.5));
            let round_dot = 1.0 - smoothstep(0.42, 0.5 + w.x, d);
            let backing = vec3<f32>(0.80, 0.84, 0.72);
            rgb = mix(backing, rgb, clamp(grid * 0.6 + round_dot * 0.4, 0.0, 1.0) * strength + (1.0 - strength));
        }
    }

    rgb = clamp(rgb * brightness, vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(1.0, 1.0, 1.0));
    // Force opaque: cores emit XRGB where the alpha byte is garbage, and a premultiplied-alpha
    // canvas would show it as translucency.
    return vec4<f32>(rgb, 1.0);
}
