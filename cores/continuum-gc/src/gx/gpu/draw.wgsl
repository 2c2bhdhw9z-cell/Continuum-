// Continuum GC's GPU drawer: the exact CPU drawer (gx/draw.rs `raster_triangle`, gx/tev.rs
// `shade`, gx/texture.rs `sample_decoded`, gx/efb.rs `blend`), ported line by line to two compute
// kernels so the phone's graphics chip draws the pixels instead of its processor cores.
//
// Why compute and not the GPU's own triangle hardware: the GameCube's pixel pipeline is integer
// arithmetic the fixed-function blender cannot copy (8-bit blend factors with its own rounding,
// logic operations Metal does not have, 24-bit depth Apple GPUs do not have, early depth with the
// alpha test after it, three EFB pixel formats). Here every pixel is owned by exactly one thread,
// which applies the primitives to it in the order the program sent them, exactly as the CPU
// drawer's helper threads do; the EFB is the same 640x528 colour and depth words the CPU code
// reads for copies, clears and peeks.
//
// Two kernels per chunk of triangles (gpu/mod.rs `flush`):
//   setup:  one thread per triangle: fixed-point vertices, winding, bounding box, edges and the
//           attribute planes, exactly as `raster_triangle` works them out.
//   raster: one workgroup per 16x16 tile of the EFB, 64 threads, 4 pixels each. The tile's
//           triangles are picked out in order (64 at a time), then drawn one after another.
//
// Integers are exact. Floating point follows the CPU code's formulas; values are worked out per
// pixel rather than stepped along the row, so a colour or depth can differ by one in the last
// place (gpu/mod.rs tests measure it).

const EFB_W: i32 = 640;
const EFB_PIXELS: u32 = 337920u; // 640 * 528; the depth words follow the colour words

// A triangle as the CPU hands it over: [state, 0, then 3 vertices of VERT_WORDS].
const TRI_WORDS: u32 = 112u;
const VERT_WORDS: u32 = 36u;    // x, y, z, 1/w, then up to 32 attributes divided by w
// A triangle after setup: edges, the origin, the state, planes.
const SETUP_WORDS: u32 = 128u;
const U_X0: u32 = 15u;
const U_Y0: u32 = 16u;
const U_STATE: u32 = 17u;
const U_Z: u32 = 19u;
const U_W: u32 = 22u;
const U_ATTR: u32 = 25u;

// The state record (gpu/mod.rs `State`).
const S_HEAD: u32 = 0u;       // stages | colour channels << 8 | coordinates << 16 | indirect << 24
const S_SC0: u32 = 1u;        // scissor left | top << 16 (signed 16-bit halves)
const S_SC1: u32 = 2u;        // scissor right | bottom << 16, exclusive
const S_PE: u32 = 3u;
const S_ALPHA: u32 = 4u;      // dst alpha | ref0 << 8 | ref1 << 16 | comp0 << 24 | comp1 << 27 | logic << 30
const S_MISC: u32 = 5u;       // fog type | ortho << 3 | ztex op << 4 | ztex fmt << 6 | need_lod << 8
const S_FOG_A: u32 = 6u;
const S_FOG_B: u32 = 7u;
const S_FOG_C: u32 = 8u;
const S_FOG_COLOR: u32 = 9u;  // r | g << 8 | b << 16
const S_ZTEX_BIAS: u32 = 10u;
const S_REGS: u32 = 11u;      // 4 registers x RGBA, signed
const S_TC_SCALE: u32 = 27u;  // (s, t) per coordinate
const S_IND_MAP: u32 = 43u;   // per indirect stage: map | coordinate << 3, 8 bits each
const S_IND_SCALE: u32 = 44u; // per indirect stage: s shift | t shift << 4, 8 bits each
const S_IND_MTX: u32 = 45u;   // 3 matrices x (m00 m01 m02 m10 m11 m12 scale)
const S_STAGES: u32 = 66u;
const STAGE_WORDS: u32 = 5u;
const S_MAPS: u32 = 146u;
const MAP_WORDS: u32 = 27u;

// PE word bits.
const PE_Z_ENABLE: u32 = 1u;
const PE_Z_UPDATE: u32 = 16u;
const PE_Z_EARLY: u32 = 32u;
const PE_BLEND: u32 = 64u;
const PE_LOGIC: u32 = 128u;
const PE_SUBTRACT: u32 = 256u;
const PE_COLOR_UPDATE: u32 = 524288u;  // 1 << 19
const PE_ALPHA_UPDATE: u32 = 1048576u; // 1 << 20
const PE_DST_ALPHA: u32 = 2097152u;    // 1 << 21

struct Params {
    first: u32,    // first triangle of the chunk (counted since the buffers were last emptied)
    count: u32,    // triangles in the chunk
    tile_x0: u32,  // the tiles the raster kernel covers (its workgroup 0 is this tile)
    tile_y0: u32,
}

var<immediate> pc: Params;

@group(0) @binding(0) var<storage, read_write> efb: array<u32>;
@group(0) @binding(1) var<storage, read> states: array<u32>;
@group(0) @binding(2) var<storage, read> tris: array<u32>;
@group(0) @binding(3) var<storage, read_write> setups: array<u32>;
@group(0) @binding(4) var<storage, read_write> boxes: array<vec4<i32>>;
@group(0) @binding(5) var<storage, read> heap: array<u32>;
@group(0) @binding(6) var<storage, read_write> counters: array<atomic<u32>>;
// Per block of 64 triangles (chunks start on a multiple of 64): the box around their boxes, so a
// tile skips a whole block it does not touch.
@group(0) @binding(7) var<storage, read_write> blocks: array<vec4<i32>>;

fn fl(w: u32) -> f32 {
    return bitcast<f32>(w);
}

fn si(w: u32) -> i32 {
    return bitcast<i32>(w);
}

// Signed 16-bit half of a word.
fn lo16(w: u32) -> i32 {
    return bitcast<i32>(w << 16u) >> 16u;
}

fn hi16(w: u32) -> i32 {
    return bitcast<i32>(w) >> 16u;
}

// Float to int as Rust's `as` does it for the values that matter (toward zero, clamped).
fn toi(v: f32) -> i32 {
    return i32(clamp(v, -1073741824.0, 1073741824.0));
}

// Rust's `(v * 16.0).round()`: halves away from zero (WGSL's round goes to even).
fn snap(v: f32) -> i32 {
    let t = v * 16.0;
    var r = round(t);
    let f = t - trunc(t);
    if (abs(f) == 0.5) {
        r = trunc(t) + sign(t);
    }
    return toi(r);
}

// a * b - c * d + e exactly, for |a|, |c| < 2^18 and |b|, |d| < 2^18: (hi, lo), the value being
// hi * 512 + lo. The CPU drawer uses 64-bit integers; WGSL has none.
fn mul2(a: i32, b: i32, c: i32, d: i32, e: i32) -> vec2<i32> {
    let bh = b >> 9u;
    let bl = b & 511;
    let dh = d >> 9u;
    let dl = d & 511;
    return vec2<i32>(a * bh - c * dh, a * bl - c * dl + e);
}

// The sign (1, 0, -1) of hi * 512 + lo.
fn sign2(v: vec2<i32>) -> i32 {
    if (abs(v.x) >= 2097152) {
        return sign(v.x);
    }
    return sign(v.x * 512 + v.y);
}

fn ceil16(a: i32) -> i32 {
    return -((-a) >> 4u);
}

// ------------------------------------------------------------------------------------- setup

fn vtx(tb: u32, k: u32, w: u32) -> f32 {
    return fl(tris[tb + 2u + k * VERT_WORDS + w]);
}

const EMPTY: vec4<i32> = vec4<i32>(32767, 32767, -32768, -32768);

var<workgroup> block_box: array<atomic<i32>, 4>;

@compute @workgroup_size(64)
fn setup(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(local_invocation_index) li: u32, @builtin(workgroup_id) wg: vec3<u32>) {
    if (li == 0u) {
        atomicStore(&block_box[0], 32767);
        atomicStore(&block_box[1], 32767);
        atomicStore(&block_box[2], -32768);
        atomicStore(&block_box[3], -32768);
    }
    workgroupBarrier();
    let b = triangle_setup(gid.x);
    if (b.x <= b.z) {
        atomicMin(&block_box[0], b.x);
        atomicMin(&block_box[1], b.y);
        atomicMax(&block_box[2], b.z);
        atomicMax(&block_box[3], b.w);
    }
    workgroupBarrier();
    if (li == 0u) {
        blocks[pc.first / 64u + wg.x] = vec4<i32>(atomicLoad(&block_box[0]), atomicLoad(&block_box[1]), atomicLoad(&block_box[2]), atomicLoad(&block_box[3]));
    }
}

// One triangle's setup; returns its box (EMPTY when it draws nothing).
fn triangle_setup(i: u32) -> vec4<i32> {
    if (i >= pc.count) {
        return EMPTY;
    }
    let t = pc.first + i;
    let tb = t * TRI_WORDS;
    let st = tris[tb];
    let head = states[st + S_HEAD];
    let nattr = 8u + 3u * ((head >> 16u) & 15u);
    var v = array<u32, 3>(0u, 1u, 2u);
    var x = array<i32, 3>(snap(vtx(tb, 0u, 0u)), snap(vtx(tb, 1u, 0u)), snap(vtx(tb, 2u, 0u)));
    var y = array<i32, 3>(snap(vtx(tb, 0u, 1u)), snap(vtx(tb, 1u, 1u)), snap(vtx(tb, 2u, 1u)));
    var area = mul2(x[1] - x[0], y[2] - y[0], y[1] - y[0], x[2] - x[0], 0);
    let sg = sign2(area);
    if (sg == 0) {
        boxes[t] = EMPTY;
        return EMPTY;
    }
    if (sg < 0) {
        v = array<u32, 3>(0u, 2u, 1u);
        x = array<i32, 3>(x[0], x[2], x[1]);
        y = array<i32, 3>(y[0], y[2], y[1]);
        area = -area;
    }
    let minx = min(x[0], min(x[1], x[2]));
    let maxx = max(x[0], max(x[1], x[2]));
    let miny = min(y[0], min(y[1], y[2]));
    let maxy = max(y[0], max(y[1], y[2]));
    let sc0 = states[st + S_SC0];
    let sc1 = states[st + S_SC1];
    // Pixels whose centres (16 * px + 8) are inside the box, and inside the scissor.
    let px0 = max(ceil16(minx - 8), lo16(sc0));
    let px1 = min((maxx - 8) >> 4u, lo16(sc1) - 1);
    let py0 = max(ceil16(miny - 8), hi16(sc0));
    let py1 = min((maxy - 8) >> 4u, hi16(sc1) - 1);
    if (px0 > px1 || py0 > py1) {
        boxes[t] = EMPTY;
        return EMPTY;
    }
    let bx = vec4<i32>(px0, py0, px1, py1);
    boxes[t] = bx;
    let sb = t * SETUP_WORDS;
    // Edge e runs from vertex e to vertex e + 1; top-left fill rule.
    for (var e = 0u; e < 3u; e++) {
        let k = (e + 1u) % 3u;
        let dx = x[k] - x[e];
        let dy = y[k] - y[e];
        let top_left = (dy == 0 && dx > 0) || dy < 0;
        let o = sb + 5u * e;
        setups[o] = bitcast<u32>(dx);
        setups[o + 1u] = bitcast<u32>(dy);
        setups[o + 2u] = bitcast<u32>(select(-1, 0, top_left));
        setups[o + 3u] = bitcast<u32>(x[e]);
        setups[o + 4u] = bitcast<u32>(y[e]);
    }
    // Attribute planes: value(px, py) = base + ddx * (px - x0) + ddy * (py - y0), in pixels.
    let fa = f32(area.x) * 512.0 + f32(area.y);
    let inv_area = 256.0 / fa;
    let x0 = f32(x[0]) / 16.0;
    let y0 = f32(y[0]) / 16.0;
    let e1x = f32(x[1] - x[0]) / 16.0;
    let e1y = f32(y[1] - y[0]) / 16.0;
    let e2x = f32(x[2] - x[0]) / 16.0;
    let e2y = f32(y[2] - y[0]) / 16.0;
    setups[sb + U_X0] = bitcast<u32>(x0);
    setups[sb + U_Y0] = bitcast<u32>(y0);
    setups[sb + U_STATE] = st;
    setups[sb + 18u] = nattr;
    // z (word 2 of a vertex), 1/w (word 3), then the attributes (words 4..).
    for (var k = 0u; k < nattr + 2u; k++) {
        let a0 = vtx(tb, v[0], 2u + k);
        let d1 = vtx(tb, v[1], 2u + k) - a0;
        let d2 = vtx(tb, v[2], 2u + k) - a0;
        let o = sb + U_Z + 3u * k;
        setups[o] = bitcast<u32>(a0);
        setups[o + 1u] = bitcast<u32>((d1 * e2y - d2 * e1y) * inv_area);
        setups[o + 2u] = bitcast<u32>((d2 * e1x - d1 * e2x) * inv_area);
    }
    return bx;
}

// ------------------------------------------------------------------------------------- raster

var<workgroup> flags: array<u32, 64>;
var<workgroup> picked: array<u32, 64>;
var<workgroup> picked_count: u32;
var<workgroup> skip_block: u32;

@compute @workgroup_size(64)
fn raster(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) li: u32) {
    let tx = i32((pc.tile_x0 + wg.x) * 16u);
    let ty = i32((pc.tile_y0 + wg.y) * 16u);
    let lx = i32(li & 15u);
    let ly = i32(li >> 4u);
    var drawn = 0u;
    for (var base = 0u; base < pc.count; base += 64u) {
        // A block of 64 triangles none of which touches this tile is skipped whole.
        if (li == 0u) {
            let bb = blocks[(pc.first + base) / 64u];
            skip_block = select(0u, 1u, bb.x > tx + 15 || bb.z < tx || bb.y > ty + 15 || bb.w < ty);
        }
        if (workgroupUniformLoad(&skip_block) != 0u) {
            continue;
        }
        // Which of the block's triangles touch this tile, kept in order.
        let t = pc.first + base + li;
        var hit = 0u;
        if (base + li < pc.count) {
            let b = boxes[t];
            if (b.x <= tx + 15 && b.z >= tx && b.y <= ty + 15 && b.w >= ty) {
                hit = 1u;
            }
        }
        flags[li] = hit;
        workgroupBarrier();
        if (li == 0u) {
            var n = 0u;
            for (var k = 0u; k < 64u; k++) {
                if (flags[k] != 0u) {
                    picked[n] = pc.first + base + k;
                    n += 1u;
                }
            }
            picked_count = n;
        }
        let n = workgroupUniformLoad(&picked_count);
        for (var k = 0u; k < n; k++) {
            drawn += triangle(picked[k], tx + lx, ty + ly);
        }
        workgroupBarrier();
    }
    atomicAdd(&counters[0], drawn);
}

// This thread's four pixels of the tile (one column, every fourth row) for one triangle.
fn triangle(t: u32, px: i32, py0: i32) -> u32 {
    let b = boxes[t];
    if (px < b.x || px > b.z) {
        return 0u;
    }
    let sb = t * SETUP_WORDS;
    var drawn = 0u;
    for (var r = 0; r < 16; r += 4) {
        let py = py0 + r;
        if (py < b.y || py > b.w) {
            continue;
        }
        if (inside(sb, px, py) && pixel(sb, px, py)) {
            drawn += 1u;
        }
    }
    return drawn;
}

fn inside(sb: u32, px: i32, py: i32) -> bool {
    let cx = px * 16 + 8;
    let cy = py * 16 + 8;
    for (var e = 0u; e < 3u; e++) {
        let o = sb + 5u * e;
        let dx = si(setups[o]);
        let dy = si(setups[o + 1u]);
        // dx * (cy - y) - dy * (cx - x) + bias >= 0
        if (sign2(mul2(dx, cy - si(setups[o + 4u]), dy, cx - si(setups[o + 3u]), si(setups[o + 2u]))) < 0) {
            return false;
        }
    }
    return true;
}

fn plane(o: u32, fx: f32, fy: f32) -> f32 {
    return fl(setups[o]) + fl(setups[o + 1u]) * fx + fl(setups[o + 2u]) * fy;
}

// One pixel through the rest of `raster_triangle`: depth, attributes, the combiner, the depth
// test and blending. True when it counts as drawn (the CPU drawer's pixel count).
fn pixel(sb: u32, px: i32, py: i32) -> bool {
    let st = setups[sb + U_STATE];
    let fx = (f32(px) + 0.5) - fl(setups[sb + U_X0]);
    let fy = (f32(py) + 0.5) - fl(setups[sb + U_Y0]);
    let idx = u32(py * EFB_W + px);
    let z = plane(sb + U_Z, fx, fy);
    let zi = u32(clamp(z, 0.0, 16777215.0));
    let pe = states[st + S_PE];
    let early = (pe & PE_Z_EARLY) != 0u;
    if (early && (pe & PE_Z_ENABLE) != 0u && !depth_test(pe, idx, zi)) {
        return false;
    }
    let iw = plane(sb + U_W, fx, fy);
    var w = 0.0;
    if (iw != 0.0) {
        w = 1.0 / iw;
    }
    let head = states[st + S_HEAD];
    let num_chans = max((head >> 8u) & 3u, 1u);
    let num_tex = (head >> 16u) & 15u;
    var color: array<vec4<i32>, 2>;
    for (var c = 0u; c < num_chans; c++) {
        let o = sb + U_ATTR + 12u * c;
        let v = vec4<f32>(plane(o, fx, fy), plane(o + 3u, fx, fy), plane(o + 6u, fx, fy), plane(o + 9u, fx, fy));
        color[c] = vec4<i32>(clamp(v * w, vec4<f32>(0.0), vec4<f32>(255.0)));
    }
    var tex: array<vec2<f32>, 8>;
    var lods: array<f32, 8>;
    let need = (states[st + S_MISC] >> 8u) & 255u;
    for (var t = 0u; t < num_tex; t++) {
        let o = sb + U_ATTR + 3u * (8u + 3u * t);
        let sw = plane(o, fx, fy);
        let tw = plane(o + 3u, fx, fy);
        var qw = plane(o + 6u, fx, fy);
        if (qw == 0.0) {
            qw = 1e-20;
        }
        let ss = fl(states[st + S_TC_SCALE + 2u * t]);
        let ts = fl(states[st + S_TC_SCALE + 2u * t + 1u]);
        let iq = 1.0 / qw;
        tex[t] = vec2<f32>(sw * iq * ss, tw * iq * ts);
        if ((need & (1u << t)) != 0u) {
            let sx = fl(setups[o + 1u]);
            let sy = fl(setups[o + 2u]);
            let tx = fl(setups[o + 4u]);
            let ty = fl(setups[o + 5u]);
            let qx = fl(setups[o + 7u]);
            let qy = fl(setups[o + 8u]);
            let iq2 = iq * iq;
            let dudx = (sx * qw - sw * qx) * iq2 * ss;
            let dudy = (sy * qw - sw * qy) * iq2 * ss;
            let dvdx = (tx * qw - tw * qx) * iq2 * ts;
            let dvdy = (ty * qw - tw * qy) * iq2 * ts;
            let rho = max(dudx * dudx + dvdx * dvdx, dudy * dudy + dvdy * dvdy);
            if (rho > 0.0) {
                lods[t] = 0.5 * log2(rho);
            } else {
                lods[t] = -10.0;
            }
        }
    }
    let shaded = shade(st, zi, color, tex, lods);
    if (shaded.alive == 0u) {
        return false;
    }
    if (!early && !depth_test(pe, idx, shaded.z)) {
        return true;
    }
    efb[idx] = blend(pe, states[st + S_ALPHA] & 255u, efb[idx], shaded.color);
    return true;
}

fn z_pass(func: u32, z: u32, old: u32) -> bool {
    switch func {
        case 0u: { return false; }
        case 1u: { return z < old; }
        case 2u: { return z == old; }
        case 3u: { return z <= old; }
        case 4u: { return z > old; }
        case 5u: { return z != old; }
        case 6u: { return z >= old; }
        default: { return true; }
    }
}

fn depth_test(pe: u32, idx: u32, z: u32) -> bool {
    if ((pe & PE_Z_ENABLE) == 0u) {
        return true;
    }
    if (!z_pass((pe >> 1u) & 7u, z, efb[EFB_PIXELS + idx])) {
        return false;
    }
    if ((pe & PE_Z_UPDATE) != 0u) {
        efb[EFB_PIXELS + idx] = z;
    }
    return true;
}

// ------------------------------------------------------------------------------------- TEV

struct Shaded {
    color: vec4<i32>,
    z: u32,
    alive: u32,
}

// One combiner channel: d +/- lerp(a, b, c), plus bias, times scale (tev.rs `combine`).
fn combine(a0: i32, b0: i32, c0: i32, d: i32, bias: u32, sub: bool, scale: u32) -> i32 {
    let a = a0 & 255;
    let b = b0 & 255;
    var c = c0 & 255;
    c = c + (c >> 7u);
    let lerp = (a * (256 - c) + b * c + 128) >> 8u;
    var v = d + lerp;
    if (sub) {
        v = d - lerp;
    }
    if (bias == 1u) {
        v += 128;
    } else if (bias == 2u) {
        v -= 128;
    }
    if (scale == 1u) {
        v = v * 2;
    } else if (scale == 2u) {
        v = v * 4;
    } else if (scale == 3u) {
        v = v >> 1u;
    }
    return v;
}

// The comparison modes (bias 3), tev.rs `compare`.
fn compare(mode: u32, a: vec3<i32>, b: vec3<i32>, ch: u32) -> bool {
    let a8 = a & vec3<i32>(255);
    let b8 = b & vec3<i32>(255);
    switch mode {
        case 0u: { return a8.x > b8.x; }
        case 1u: { return a8.x == b8.x; }
        case 2u: { return ((a8.y << 8u) | a8.x) > ((b8.y << 8u) | b8.x); }
        case 3u: { return ((a8.y << 8u) | a8.x) == ((b8.y << 8u) | b8.x); }
        case 4u: { return ((a8.z << 16u) | (a8.y << 8u) | a8.x) > ((b8.z << 16u) | (b8.y << 8u) | b8.x); }
        case 5u: { return ((a8.z << 16u) | (a8.y << 8u) | a8.x) == ((b8.z << 16u) | (b8.y << 8u) | b8.x); }
        case 6u: { return a8[ch] > b8[ch]; }
        default: { return a8[ch] == b8[ch]; }
    }
}

fn alpha_test(f: u32, a: i32, r: i32) -> bool {
    switch f {
        case 0u: { return false; }
        case 1u: { return a < r; }
        case 2u: { return a == r; }
        case 3u: { return a <= r; }
        case 4u: { return a > r; }
        case 5u: { return a != r; }
        case 6u: { return a >= r; }
        default: { return true; }
    }
}

fn wrapc(v: f32, w: u32) -> f32 {
    if (w == 0u) {
        return v;
    }
    if (w == 6u) {
        return 0.0;
    }
    let size = f32(256u >> (w - 1u));
    let r = v % size;
    if (r < 0.0) {
        return r + size;
    }
    return r;
}

fn present(st: u32, m: u32) -> bool {
    return (states[st + S_MAPS + m * MAP_WORDS + 1u] & 32768u) != 0u;
}

// Runs the TEV, the alpha test, the z texture and fog for one pixel (tev.rs `shade`).
fn shade(st: u32, zin: u32, color: array<vec4<i32>, 2>, tex: array<vec2<f32>, 8>, lods: array<f32, 8>) -> Shaded {
    var res: Shaded;
    let head = states[st + S_HEAD];
    var rows: array<vec4<i32>, 10>;
    for (var k = 0u; k < 4u; k++) {
        let o = st + S_REGS + 4u * k;
        rows[k] = vec4<i32>(si(states[o]), si(states[o + 1u]), si(states[o + 2u]), si(states[o + 3u]));
    }
    rows[7] = vec4<i32>(255);
    rows[8] = vec4<i32>(128);
    // Indirect texture lookups, one per indirect stage.
    let num_ind = head >> 24u;
    let imap = states[st + S_IND_MAP];
    let iscale = states[st + S_IND_SCALE];
    var ind: array<vec3<i32>, 4>;
    for (var i = 0u; i < min(num_ind, 4u); i++) {
        let m = (imap >> (8u * i)) & 7u;
        let tc = (imap >> (8u * i + 3u)) & 7u;
        let ss = (iscale >> (8u * i)) & 15u;
        let ts = (iscale >> (8u * i + 4u)) & 15u;
        let t = tex[tc];
        var s = vec4<u32>(0u);
        if (present(st, m)) {
            s = sample(st, m, t.x / f32(1u << ss), t.y / f32(1u << ts), lods[tc] - f32(max(ss, ts)));
        }
        ind[i] = vec3<i32>(i32(s.w), i32(s.z), i32(s.y));
    }
    var last_tex = vec4<i32>(0);
    var prev_off = vec2<f32>(0.0);
    var bump_alpha = 0;
    var cdst = 0u;
    var adst = 0u;
    let nstages = head & 255u;
    for (var n = 0u; n < nstages; n++) {
        let o = st + S_STAGES + n * STAGE_WORDS;
        let w0 = states[o];
        let w1 = states[o + 1u];
        let w2 = states[o + 2u];
        let w3 = states[o + 3u];
        let cmd = states[o + 4u];
        let texmap = (w3 >> 16u) & 7u;
        let tc = (w3 >> 19u) & 7u;
        let tex_on = ((w3 >> 22u) & 1u) != 0u;
        let ras = (w3 >> 23u) & 7u;
        // Indirect offset for this stage's coordinates.
        var uv = tex[tc];
        if (cmd != 0u || num_ind > 0u) {
            let bt = cmd & 3u;
            let fmt = (cmd >> 2u) & 3u;
            let bias = (cmd >> 4u) & 7u;
            let bs = (cmd >> 7u) & 3u;
            let mid = (cmd >> 9u) & 15u;
            let sw = (cmd >> 13u) & 7u;
            let tw = (cmd >> 16u) & 7u;
            let add_prev = (cmd & 1048576u) != 0u;
            var c = vec3<i32>(0);
            if (bt < num_ind) {
                c = ind[bt];
            }
            var mask = 255;
            var bias_v = -128;
            if (fmt == 1u) {
                mask = 31;
                bias_v = 1;
            } else if (fmt == 2u) {
                mask = 15;
                bias_v = 1;
            } else if (fmt == 3u) {
                mask = 7;
                bias_v = 1;
            }
            var ba = 0;
            if (bs == 1u) {
                ba = c.x;
            } else if (bs == 2u) {
                ba = c.y;
            } else if (bs == 3u) {
                ba = c.z;
            }
            bump_alpha = ba & ~mask & 248;
            c = c & vec3<i32>(mask);
            if ((bias & 1u) != 0u) {
                c.x += bias_v;
            }
            if ((bias & 2u) != 0u) {
                c.y += bias_v;
            }
            if ((bias & 4u) != 0u) {
                c.z += bias_v;
            }
            var off = vec2<f32>(0.0);
            if (mid >= 1u && mid <= 3u) {
                let mb = st + S_IND_MTX + (mid - 1u) * 7u;
                let f = vec3<f32>(c);
                let sc = fl(states[mb + 6u]);
                off = vec2<f32>(
                    (fl(states[mb]) * f.x + fl(states[mb + 1u]) * f.y + fl(states[mb + 2u]) * f.z) * sc,
                    (fl(states[mb + 3u]) * f.x + fl(states[mb + 4u]) * f.y + fl(states[mb + 5u]) * f.z) * sc);
            } else if (mid >= 5u && mid <= 7u) {
                let sc = fl(states[st + S_IND_MTX + (mid - 5u) * 7u + 6u]);
                off = vec2<f32>(uv.x * f32(c.x) * sc / 256.0, uv.y * f32(c.x) * sc / 256.0);
            } else if (mid >= 9u && mid <= 11u) {
                let sc = fl(states[st + S_IND_MTX + (mid - 9u) * 7u + 6u]);
                off = vec2<f32>(uv.x * f32(c.y) * sc / 256.0, uv.y * f32(c.y) * sc / 256.0);
            }
            uv = vec2<f32>(wrapc(uv.x, sw), wrapc(uv.y, tw));
            var total = off;
            if (add_prev) {
                total += prev_off;
            }
            uv += total;
            prev_off = total;
        }
        var texc = vec4<i32>(0);
        if (tex_on) {
            var s = vec4<u32>(0u);
            if (present(st, texmap)) {
                s = sample(st, texmap, uv.x, uv.y, lods[tc]);
            }
            let sz = w1 >> 24u;
            texc = vec4<i32>(i32(s[sz & 3u]), i32(s[(sz >> 2u) & 3u]), i32(s[(sz >> 4u) & 3u]), i32(s[(sz >> 6u) & 3u]));
        }
        last_tex = texc;
        var ras_raw = vec4<i32>(0);
        if (ras == 0u) {
            ras_raw = color[0];
        } else if (ras == 1u) {
            ras_raw = color[1];
        } else if (ras == 5u) {
            ras_raw = vec4<i32>(bump_alpha);
        } else if (ras == 6u) {
            ras_raw = vec4<i32>(bump_alpha | (bump_alpha >> 5u));
        }
        let rz = (w1 >> 16u) & 255u;
        rows[4] = texc;
        rows[5] = vec4<i32>(ras_raw[rz & 3u], ras_raw[(rz >> 2u) & 3u], ras_raw[(rz >> 4u) & 3u], ras_raw[(rz >> 6u) & 3u]);
        rows[6] = vec4<i32>(i32(w2 & 255u), i32((w2 >> 8u) & 255u), i32((w2 >> 16u) & 255u), i32(w2 >> 24u));
        // Colour operands: row (4 bits) and whether it is that row's alpha (bit 4), 5 bits each.
        var cin: array<vec3<i32>, 4>;
        for (var k = 0u; k < 4u; k++) {
            let sel = (w0 >> (5u * k)) & 31u;
            let r = rows[sel & 15u];
            if ((sel & 16u) != 0u) {
                cin[k] = vec3<i32>(r.w);
            } else {
                cin[k] = r.xyz;
            }
        }
        let aa = rows[w1 & 15u].w;
        let ab = rows[(w1 >> 4u) & 15u].w;
        let ac = rows[(w1 >> 8u) & 15u].w;
        let ad = rows[(w1 >> 12u) & 15u].w;
        let c_bias = w3 & 3u;
        let c_sub = ((w3 >> 2u) & 1u) != 0u;
        let c_clamp = ((w3 >> 3u) & 1u) != 0u;
        let c_scale = (w3 >> 4u) & 3u;
        cdst = (w3 >> 6u) & 3u;
        let a_bias = (w3 >> 8u) & 3u;
        let a_sub = ((w3 >> 10u) & 1u) != 0u;
        let a_clamp = ((w3 >> 11u) & 1u) != 0u;
        let a_scale = (w3 >> 12u) & 3u;
        adst = (w3 >> 14u) & 3u;
        var col = vec3<i32>(0);
        if (c_bias == 3u) {
            let mode = (c_scale << 1u) | u32(c_sub);
            for (var ch = 0u; ch < 3u; ch++) {
                var add = 0;
                if (compare(mode, cin[0], cin[1], ch)) {
                    add = cin[2][ch] & 255;
                }
                col[ch] = cin[3][ch] + add;
            }
        } else {
            for (var ch = 0u; ch < 3u; ch++) {
                col[ch] = combine(cin[0][ch], cin[1][ch], cin[2][ch], cin[3][ch], c_bias, c_sub, c_scale);
            }
        }
        var alpha = 0;
        if (a_bias == 3u) {
            let mode = (a_scale << 1u) | u32(a_sub);
            var ok = false;
            if (mode >= 6u) {
                if (mode == 6u) {
                    ok = (aa & 255) > (ab & 255);
                } else {
                    ok = (aa & 255) == (ab & 255);
                }
            } else {
                ok = compare(mode, cin[0], cin[1], 0u);
            }
            alpha = ad;
            if (ok) {
                alpha += ac & 255;
            }
        } else {
            alpha = combine(aa, ab, ac, ad, a_bias, a_sub, a_scale);
        }
        if (c_clamp) {
            col = clamp(col, vec3<i32>(0), vec3<i32>(255));
        } else {
            col = clamp(col, vec3<i32>(-1024), vec3<i32>(1023));
        }
        if (a_clamp) {
            alpha = clamp(alpha, 0, 255);
        } else {
            alpha = clamp(alpha, -1024, 1023);
        }
        rows[cdst] = vec4<i32>(col, rows[cdst].w);
        rows[adst].w = alpha;
    }
    var out = vec4<i32>(rows[cdst].xyz, rows[adst].w) & vec4<i32>(255);

    // Alpha test.
    let aw = states[st + S_ALPHA];
    let t0 = alpha_test((aw >> 24u) & 7u, out.w, i32((aw >> 8u) & 255u));
    let t1 = alpha_test((aw >> 27u) & 7u, out.w, i32((aw >> 16u) & 255u));
    let logic = aw >> 30u;
    var passed = t0 == t1;
    if (logic == 0u) {
        passed = t0 && t1;
    } else if (logic == 1u) {
        passed = t0 || t1;
    } else if (logic == 2u) {
        passed = t0 != t1;
    }
    if (!passed) {
        res.alive = 0u;
        return res;
    }

    // Z texture.
    let misc = states[st + S_MISC];
    var z = zin;
    let ztex_op = (misc >> 4u) & 3u;
    if (ztex_op != 0u) {
        let t = vec4<u32>(last_tex);
        let fmt = (misc >> 6u) & 3u;
        var zt = (t.x << 16u) | (t.y << 8u) | t.z;
        if (fmt == 0u) {
            zt = t.w;
        } else if (fmt == 1u) {
            zt = (t.w << 8u) | t.x;
        }
        zt = (zt + states[st + S_ZTEX_BIAS]) & 16777215u;
        if (ztex_op == 1u) {
            z = min(z + zt, 16777215u);
        } else {
            z = zt;
        }
    }

    // Fog.
    let fog_type = misc & 7u;
    if (fog_type >= 2u) {
        let zf = f32(zin) / 16777215.0;
        let fog_a = fl(states[st + S_FOG_A]);
        var ze = 0.0;
        if (((misc >> 3u) & 1u) != 0u) {
            ze = fog_a * zf;
        } else {
            let d = fl(states[st + S_FOG_B]) - zf;
            if (abs(d) >= 1e-12) {
                ze = fog_a / d;
            }
        }
        let fv = clamp(ze - fl(states[st + S_FOG_C]), 0.0, 1.0);
        var f = fv;
        if (fog_type == 4u) {
            f = 1.0 - exp2(-8.0 * fv);
        } else if (fog_type == 5u) {
            f = 1.0 - exp2(-8.0 * fv * fv);
        } else if (fog_type == 6u) {
            f = exp2(-8.0 * (1.0 - fv));
        } else if (fog_type == 7u) {
            f = exp2(-8.0 * (1.0 - fv) * (1.0 - fv));
        }
        let fi = i32(clamp(f, 0.0, 1.0) * 256.0);
        let fc = states[st + S_FOG_COLOR];
        let fog = vec3<i32>(i32(fc & 255u), i32((fc >> 8u) & 255u), i32((fc >> 16u) & 255u));
        out = vec4<i32>((out.xyz * (256 - fi) + fog * fi) >> vec3<u32>(8u), out.w);
    }
    res.color = out;
    res.z = z;
    res.alive = 1u;
    return res;
}

// ------------------------------------------------------------------------------------- textures

fn wrap(c: i32, size: u32, mode: u32) -> u32 {
    let s = i32(size);
    let pow2 = (size & (size - 1u)) == 0u;
    if (mode == 1u) {
        if (pow2) {
            return u32(c & (s - 1));
        }
        let r = c % s;
        return u32(select(r, r + s, r < 0));
    }
    if (mode == 2u) {
        var p = 0;
        if (pow2) {
            p = c & (2 * s - 1);
        } else {
            let r = c % (2 * s);
            p = select(r, r + 2 * s, r < 0);
        }
        return u32(select(p, 2 * s - 1 - p, p >= s));
    }
    return u32(clamp(c, 0, s - 1));
}

fn texel(off: u32, w: u32, x: u32, y: u32) -> vec4<u32> {
    let p = heap[off + y * w + x];
    return vec4<u32>(p & 255u, (p >> 8u) & 255u, (p >> 16u) & 255u, p >> 24u);
}

fn level_sample(mb: u32, level: u32, flags_w: u32, u0: f32, v0: f32, linear: bool) -> vec4<u32> {
    let off = states[mb + 5u + 2u * level];
    let wh = states[mb + 6u + 2u * level];
    let w = wh & 65535u;
    let h = wh >> 16u;
    let inv = 1.0 / f32(1u << level);
    let u = u0 * inv;
    let v = v0 * inv;
    let ws = flags_w & 3u;
    let wt = (flags_w >> 2u) & 3u;
    if (!linear) {
        return texel(off, w, wrap(toi(floor(u)), w, ws), wrap(toi(floor(v)), h, wt));
    }
    let uu = u - 0.5;
    let vv = v - 0.5;
    let fx = floor(uu);
    let fy = floor(vv);
    let ax = u32((uu - fx) * 128.0);
    let ay = u32((vv - fy) * 128.0);
    let x0 = toi(fx);
    let y0 = toi(fy);
    let xa = wrap(x0, w, ws);
    let xb = wrap(x0 + 1, w, ws);
    let ya = wrap(y0, h, wt);
    let yb = wrap(y0 + 1, h, wt);
    let top = texel(off, w, xa, ya) * (128u - ax) + texel(off, w, xb, ya) * ax;
    let bot = texel(off, w, xa, yb) * (128u - ax) + texel(off, w, xb, yb) * ax;
    return (top * (128u - ay) + bot * ay + vec4<u32>(8192u)) >> vec4<u32>(14u);
}

// texture.rs `sample_decoded`. Map words: 1 flags (wrap s, wrap t << 2, mag linear << 4, min
// linear << 5, mip << 6, levels << 8, present << 15), 2 lod bias, 3 min lod, 4 max lod, then per
// level its first texel in the heap and width | height << 16.
fn sample(st: u32, m: u32, u: f32, v: f32, lod_in: f32) -> vec4<u32> {
    let mb = st + S_MAPS + m * MAP_WORDS;
    let fw = states[mb + 1u];
    let min_lod = fl(states[mb + 3u]);
    let lod = clamp(lod_in + fl(states[mb + 2u]), min_lod, max(fl(states[mb + 4u]), min_lod));
    let magnify = lod <= 0.0;
    var linear = ((fw >> 5u) & 1u) != 0u;
    if (magnify) {
        linear = ((fw >> 4u) & 1u) != 0u;
    }
    let max_level = max((fw >> 8u) & 15u, 1u) - 1u;
    let mip = (fw >> 6u) & 3u;
    if (magnify || mip == 0u || max_level == 0u) {
        return level_sample(mb, 0u, fw, u, v, linear);
    }
    if (mip == 1u) {
        let l = u32(min(floor(lod + 0.5), f32(max_level)));
        return level_sample(mb, l, fw, u, v, linear);
    }
    let l0 = min(u32(floor(lod)), max_level);
    let l1 = min(l0 + 1u, max_level);
    let f = u32((lod - floor(lod)) * 256.0);
    let a = level_sample(mb, l0, fw, u, v, linear);
    if (l1 == l0 || f == 0u) {
        return a;
    }
    let b = level_sample(mb, l1, fw, u, v, linear);
    return (a * (256u - f) + b * f) >> vec4<u32>(8u);
}

// ------------------------------------------------------------------------------------- blending

fn unpack(c: u32) -> vec4<i32> {
    return vec4<i32>(i32(c >> 24u), i32((c >> 16u) & 255u), i32((c >> 8u) & 255u), i32(c & 255u));
}

fn pack(c: vec4<i32>) -> u32 {
    let u = vec4<u32>(c) & vec4<u32>(255u);
    return (u.x << 24u) | (u.y << 16u) | (u.z << 8u) | u.w;
}

fn quantize(c: vec4<i32>, format: u32) -> vec4<i32> {
    if (format == 1u) {
        let s = c >> vec4<u32>(2u);
        return (s << vec4<u32>(2u)) | (s >> vec4<u32>(4u));
    }
    if (format == 2u) {
        let r = c.x >> 3u;
        let g = c.y >> 2u;
        let b = c.z >> 3u;
        return vec4<i32>((r << 3u) | (r >> 2u), (g << 2u) | (g >> 4u), (b << 3u) | (b >> 2u), 255);
    }
    return vec4<i32>(c.xyz, 255);
}

fn mul8(v: i32, f: i32) -> i32 {
    return (v * (f + (f >> 7u))) >> 8u;
}

fn src_factor(sel: u32, c: u32, dst: vec4<i32>, src: vec4<i32>, dst_a: i32) -> i32 {
    switch sel {
        case 0u: { return 0; }
        case 1u: { return 255; }
        case 2u: { return dst[c]; }
        case 3u: { return 255 - dst[c]; }
        case 4u: { return src.w; }
        case 5u: { return 255 - src.w; }
        case 6u: { return dst_a; }
        default: { return 255 - dst_a; }
    }
}

fn dst_factor(sel: u32, c: u32, src: vec4<i32>, dst_a: i32) -> i32 {
    switch sel {
        case 0u: { return 0; }
        case 1u: { return 255; }
        case 2u: { return src[c]; }
        case 3u: { return 255 - src[c]; }
        case 4u: { return src.w; }
        case 5u: { return 255 - src.w; }
        case 6u: { return dst_a; }
        default: { return 255 - dst_a; }
    }
}

fn logic_op(op: u32, s: u32, d: u32) -> u32 {
    switch op {
        case 0u: { return 0u; }
        case 1u: { return s & d; }
        case 2u: { return s & ~d; }
        case 3u: { return s; }
        case 4u: { return ~s & d; }
        case 5u: { return d; }
        case 6u: { return s ^ d; }
        case 7u: { return s | d; }
        case 8u: { return ~(s | d); }
        case 9u: { return ~(s ^ d); }
        case 10u: { return ~d; }
        case 11u: { return s | ~d; }
        case 12u: { return ~s; }
        case 13u: { return ~s | d; }
        case 14u: { return ~(s & d); }
        default: { return 255u; }
    }
}

// efb.rs `blend`: the new stored value of an EFB pixel.
fn blend(pe: u32, dst_alpha: u32, dst_px: u32, src: vec4<i32>) -> u32 {
    let dst = unpack(dst_px);
    let format = (pe >> 22u) & 7u;
    let has_alpha = format == 1u;
    let dst_a = select(255, dst.w, has_alpha);
    var out = src;
    if ((pe & PE_BLEND) != 0u) {
        if ((pe & PE_SUBTRACT) != 0u) {
            out = clamp(dst - src, vec4<i32>(0), vec4<i32>(255));
        } else {
            let sf = (pe >> 9u) & 7u;
            let df = (pe >> 12u) & 7u;
            for (var c = 0u; c < 4u; c++) {
                var d = dst[c];
                if (c == 3u) {
                    d = dst_a;
                }
                out[c] = clamp(mul8(src[c], src_factor(sf, c, dst, src, dst_a)) + mul8(d, dst_factor(df, c, src, dst_a)), 0, 255);
            }
        }
    } else if ((pe & PE_LOGIC) != 0u) {
        let op = (pe >> 15u) & 15u;
        for (var c = 0u; c < 4u; c++) {
            out[c] = i32(logic_op(op, u32(src[c]) & 255u, u32(dst[c]) & 255u) & 255u);
        }
    }
    let dst_alpha_on = (pe & PE_DST_ALPHA) != 0u;
    if (dst_alpha_on) {
        out.w = i32(dst_alpha);
    }
    let q = quantize(out, format);
    var result = dst;
    if ((pe & PE_COLOR_UPDATE) != 0u) {
        result = vec4<i32>(q.xyz, result.w);
    }
    if ((pe & PE_ALPHA_UPDATE) != 0u || dst_alpha_on) {
        result.w = select(255, q.w, has_alpha);
    }
    return pack(result);
}
