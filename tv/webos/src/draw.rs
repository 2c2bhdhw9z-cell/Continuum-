//! Drawing: rounded glass cards, gradients, glows, the light that runs round buttons, text,
//! pictures, the swirl background and the game screen, all in OpenGL ES 2.
//!
//! Everything is laid out on a 1920x1080 page and scaled to whatever the TV gives the app.

use crate::gl::{self, Gl, GLint, GLuint};
use ab_glyph::{point, Font, FontVec, PxScale, ScaleFont};
use std::collections::HashMap;

pub type Rgba = [f32; 4];

pub fn rgb(hex: u32) -> Rgba {
    [((hex >> 16) & 255) as f32 / 255.0, ((hex >> 8) & 255) as f32 / 255.0, (hex & 255) as f32 / 255.0, 1.0]
}
pub fn rgba(hex: u32, a: f32) -> Rgba {
    let mut c = rgb(hex);
    c[3] = a;
    c
}
pub fn white(a: f32) -> Rgba {
    [1.0, 1.0, 1.0, a]
}
pub fn with_alpha(c: Rgba, a: f32) -> Rgba {
    [c[0], c[1], c[2], c[3] * a]
}

pub const VW: f32 = 1920.0;
pub const VH: f32 = 1080.0;

const HEADER: &str = "#ifdef GL_FRAGMENT_PRECISION_HIGH\nprecision highp float;\n#else\nprecision mediump float;\n#endif\n";

const VS: &str = "attribute vec2 a_pos; attribute vec2 a_uv; attribute vec2 a_local; attribute vec4 a_color;
uniform vec2 u_view;
varying vec2 v_uv; varying vec2 v_local; varying vec4 v_color;
void main() {
  v_uv = a_uv; v_local = a_local; v_color = a_color;
  gl_Position = vec4(a_pos.x / u_view.x * 2.0 - 1.0, 1.0 - a_pos.y / u_view.y * 2.0, 0.0, 1.0);
}";

const FS_SHAPE: &str = "varying vec2 v_local;
uniform vec2 u_half; uniform float u_radius;
uniform vec4 u_fill0; uniform vec4 u_fill1; uniform vec2 u_gdir;
uniform float u_border; uniform vec4 u_bcolor;
uniform float u_mode; uniform float u_soft;
uniform vec4 u_light; uniform vec4 u_lc1; uniform vec4 u_lc2;
float sdf(vec2 p, vec2 b, float r) { vec2 q = abs(p) - b + vec2(r); return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r; }
vec4 over(vec4 dst, vec4 src) {
  float a = src.a + dst.a * (1.0 - src.a);
  if (a <= 0.0001) return vec4(0.0);
  return vec4((src.rgb * src.a + dst.rgb * dst.a * (1.0 - src.a)) / a, a);
}
void main() {
  float d = sdf(v_local, u_half, u_radius);
  vec4 col;
  if (u_mode > 1.5) {
    // a blurred ring along the edge (a glow round a picked tile)
    float rd = abs(d + u_border * 0.5) - u_border * 0.5;
    float g = exp(-(max(rd, 0.0) * max(rd, 0.0)) / (2.0 * u_soft * u_soft));
    col = vec4(u_fill0.rgb, u_fill0.a * g);
  } else if (u_mode > 0.5) {
    // a soft shadow or glow under a shape
    float g = 1.0 - smoothstep(-u_soft, u_soft, d);
    col = vec4(u_fill0.rgb, u_fill0.a * g);
  } else {
    float inside = clamp(0.5 - d, 0.0, 1.0);
    float t = clamp(dot(v_local / max(2.0 * u_half, vec2(1.0)), u_gdir) + 0.5, 0.0, 1.0);
    vec4 fill = mix(u_fill0, u_fill1, t);
    col = vec4(fill.rgb, fill.a * inside);
    if (u_border > 0.0) {
      float bd = abs(d + u_border * 0.5) - u_border * 0.5;
      float b = clamp(0.5 - bd, 0.0, 1.0);
      col = over(col, vec4(u_bcolor.rgb, u_bcolor.a * b));
    }
    if (u_light.x > 0.5) {
      float w = u_light.z;
      float bd = abs(d + w * 0.5) - w * 0.5;
      float b = clamp(0.5 - bd, 0.0, 1.0);
      float ang = atan(v_local.y, v_local.x);
      float loc = fract((ang - u_light.y) / 6.2831853);
      vec4 c;
      if (loc < 0.35) c = mix(vec4(u_lc1.rgb, 0.75), vec4(u_lc2.rgb, 0.9), loc / 0.35);
      else if (loc < 0.5) c = mix(vec4(u_lc2.rgb, 0.9), vec4(1.0, 1.0, 1.0, 0.95), (loc - 0.35) / 0.15);
      else if (loc < 0.65) c = mix(vec4(1.0, 1.0, 1.0, 0.95), vec4(u_lc1.rgb, 0.9), (loc - 0.5) / 0.15);
      else c = mix(vec4(u_lc1.rgb, 0.9), vec4(u_lc1.rgb, 0.75), (loc - 0.65) / 0.35);
      col = over(col, vec4(c.rgb, c.a * b * u_light.w));
    }
  }
  gl_FragColor = col;
}";

const FS_RADIAL: &str = "varying vec2 v_local; uniform vec4 u_fill0; uniform vec2 u_half;
void main() {
  float r = length(v_local / u_half);
  // stops like the phone's glows: full, 0.45 at 45%, nothing at the edge
  float a = r < 0.45 ? mix(1.0, 0.45, r / 0.45) : mix(0.45, 0.0, clamp((r - 0.45) / 0.55, 0.0, 1.0));
  gl_FragColor = vec4(u_fill0.rgb, u_fill0.a * a);
}";

const FS_RING: &str = "varying vec2 v_local; uniform vec4 u_ring; uniform vec4 u_c1; uniform vec4 u_c2;
vec3 hue(float h) { vec3 k = clamp(abs(fract(h + vec3(0.0, 2.0/3.0, 1.0/3.0)) * 6.0 - 3.0) - 1.0, 0.0, 1.0); return k; }
void main() {
  // u_ring: radius, width, start angle, mode (0 rainbow, 1 two colours, 2 one colour)
  float d = length(v_local);
  float a = clamp(0.5 - abs(d - (u_ring.x - u_ring.y * 0.5)) + u_ring.y * 0.5, 0.0, 1.0);
  float loc = fract((atan(v_local.y, v_local.x) - u_ring.z) / 6.2831853);
  vec3 c;
  if (u_ring.w < 0.5) c = hue(loc);
  else if (u_ring.w < 1.5) c = mix(u_c1.rgb, u_c2.rgb, 0.5 + 0.5 * sin(loc * 6.2831853));
  else c = u_c1.rgb;
  gl_FragColor = vec4(c, a);
}";

const FS_TEXT: &str = "varying vec2 v_uv; varying vec4 v_color; uniform sampler2D u_tex;
void main() { gl_FragColor = vec4(v_color.rgb, v_color.a * texture2D(u_tex, v_uv).a); }";

const FS_IMAGE: &str = "varying vec2 v_uv; varying vec2 v_local; varying vec4 v_color;
uniform sampler2D u_tex; uniform vec2 u_half; uniform float u_radius; uniform float u_bgr;
float sdf(vec2 p, vec2 b, float r) { vec2 q = abs(p) - b + vec2(r); return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r; }
void main() {
  vec4 t = texture2D(u_tex, v_uv);
  if (u_bgr > 0.5) t = vec4(t.b, t.g, t.r, 1.0);
  float inside = clamp(0.5 - sdf(v_local, u_half, u_radius), 0.0, 1.0);
  gl_FragColor = vec4(t.rgb * v_color.rgb, t.a * v_color.a * inside);
}";

const FS_SWIRL: &str = "varying vec2 v_uv; uniform sampler2D u_tex; uniform vec2 u_size;
vec4 tap(vec2 p) { return texture2D(u_tex, p); }
void main() {
  // smooth (cubic B-spline) scaling of the small colour grid: four filtered taps
  vec2 st = v_uv * u_size - 0.5;
  vec2 i = floor(st); vec2 f = st - i;
  vec2 f2 = f * f; vec2 f3 = f2 * f;
  vec2 w0 = (-f3 + 3.0 * f2 - 3.0 * f + 1.0) / 6.0;
  vec2 w1 = (3.0 * f3 - 6.0 * f2 + 4.0) / 6.0;
  vec2 w2 = (-3.0 * f3 + 3.0 * f2 + 3.0 * f + 1.0) / 6.0;
  vec2 w3 = f3 / 6.0;
  vec2 s0 = w0 + w1; vec2 s1 = w2 + w3;
  vec2 t0 = (i - 0.5 + w1 / s0) / u_size;
  vec2 t1 = (i + 1.5 + w3 / s1) / u_size;
  vec4 c = s0.y * (s0.x * tap(vec2(t0.x, t0.y)) + s1.x * tap(vec2(t1.x, t0.y)))
         + s1.y * (s0.x * tap(vec2(t0.x, t1.y)) + s1.x * tap(vec2(t1.x, t1.y)));
  // a speck of noise, so a slow gradient never shows steps (bands) on the TV
  float n = fract(52.9829189 * fract(dot(gl_FragCoord.xy, vec2(0.06711056, 0.00583715))));
  gl_FragColor = vec4(c.rgb + (n - 0.5) / 255.0, 1.0);
}";

/// The swirl's colour field (swirl.rs `field`), one grid point per pixel of a small texture,
/// worked out on the graphics chip so the menus cost the processor nothing. Rows are the TV's
/// height, columns its width (the field turned a quarter, as on the processor).
const FS_FIELD: &str = "uniform vec4 u_t;      // t, tall, mix, over
uniform vec2 u_grid;   // cols, rows
uniform vec3 u_base;
uniform vec3 u_whirl[2];
uniform vec3 u_cloud[8];  // x, y, colour
uniform float u_nclouds;
uniform vec3 u_pal[4];    // lightness, chroma, hue
uniform float u_npal;
float gam(float c) { c = clamp(c, 0.0, 1.0); return c <= 0.0031308 ? c * 12.92 : 1.055 * pow(c, 1.0 / 2.4) - 0.055; }
void main() {
  float t = u_t.x; float tall = u_t.y; float over = u_t.w;
  float i = floor(gl_FragCoord.y);
  float j = floor(gl_FragCoord.x);
  float qx = -over + (1.0 + 2.0 * over) * i / u_grid.x;
  float qy = (-over + (1.0 + 2.0 * over) * j / u_grid.y) * tall;
  for (int v = 0; v < 2; v++) {
    vec3 w = u_whirl[v];
    float dx = qx - w.x; float dy = qy - w.y;
    float a = w.z * exp(-(dx * dx + dy * dy) / 0.12);
    float ca = cos(a); float sa = sin(a);
    qx = w.x + dx * ca - dy * sa;
    qy = w.y + dx * sa + dy * ca;
  }
  float px = qx + 0.05 * sin(qy * 3.1 + t * 0.37) + 0.025 * sin(qy * 5.3 - t * 0.29 + 1.3);
  float py = qy + 0.05 * sin(qx * 2.7 - t * 0.31 + 2.1) + 0.025 * sin(qx * 4.9 + t * 0.23);
  float falloff = 1.0 / (2.0 * 0.22 * 0.22);
  float total = 0.0; float l = 0.0; float ch = 0.0; float hu = 0.0;
  for (int c = 0; c < 4; c++) {
    if (float(c) >= u_npal) break;
    float wsum = 0.0;
    for (int s = 0; s < 8; s++) {
      if (float(s) >= u_nclouds) break;
      vec3 cl = u_cloud[s];
      if (abs(cl.z - float(c)) < 0.5) {
        float dx = px - cl.x; float dy = py - cl.y;
        wsum += exp(-(dx * dx + dy * dy) * falloff);
      }
    }
    float v = pow(wsum + 1e-4, 1.8);
    total += v;
    l += v * u_pal[c].x; ch += v * u_pal[c].y; hu += v * u_pal[c].z;
  }
  l /= total; ch /= total; hu /= total;
  vec3 lab = vec3(l, ch * cos(hu), ch * sin(hu));
  float l0 = lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z;
  float m0 = lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z;
  float s0 = lab.x - 0.0894841775 * lab.y - 1.2914855480 * lab.z;
  float L = l0 * l0 * l0; float M = m0 * m0 * m0; float S = s0 * s0 * s0;
  vec3 rgb = vec3(gam(4.0767416621 * L - 3.3077115913 * M + 0.2309699292 * S),
                  gam(-1.2684380046 * L + 2.6097574011 * M - 0.3413193965 * S),
                  gam(-0.0041960863 * L - 0.7034186147 * M + 1.7076147010 * S));
  vec3 outc = u_base * (1.0 - u_t.z) + rgb * u_t.z;
  gl_FragColor = vec4(clamp(outc, 0.0, 1.0), 1.0);
}";

const FS_GAME: &str = "varying vec2 v_uv; uniform sampler2D u_tex; uniform vec2 u_size; uniform float u_grid; uniform float u_sharp;
void main() {
  vec2 uv = v_uv;
  if (u_sharp > 0.5) {
    // Sharp, even pixels at any scale: each console pixel's own colour right across it, and a
    // blend no wider than one TV pixel where two meet (measured from the nearest seam).
    vec2 px = uv * u_size;
    vec2 fw = max(fwidth(px), vec2(0.0001));
    vec2 seam = floor(px + 0.5);
    uv = (seam + clamp((px - seam) / fw, -0.5, 0.5)) / u_size;
  }
  vec4 t = texture2D(u_tex, uv);
  vec3 c = vec3(t.b, t.g, t.r);
  if (u_grid > 0.0) {
    vec2 g = fract(v_uv * u_size);
    float line = smoothstep(0.0, 0.18, g.x) * smoothstep(0.0, 0.18, g.y);
    c *= mix(1.0, 0.55 + 0.45 * line, u_grid);
  }
  gl_FragColor = vec4(c, 1.0);
}";

struct Prog {
    id: GLuint,
    u: HashMap<&'static str, GLint>,
}

/// One shape's look.
#[derive(Clone, Copy)]
pub struct Shape {
    pub fill0: Rgba,
    pub fill1: Rgba,
    /// gradient direction (x, y) in the shape's own size; (1,1) is top left to bottom right
    pub gdir: [f32; 2],
    pub border: f32,
    pub bcolor: Rgba,
    /// the light running round the edge: Some((start angle in radians, line width, first, second, strength))
    pub light: Option<(f32, f32, Rgba, Rgba, f32)>,
}

impl Shape {
    pub fn fill(c: Rgba) -> Shape {
        Shape { fill0: c, fill1: c, gdir: [0.0, 0.0], border: 0.0, bcolor: [0.0; 4], light: None }
    }
    pub fn gradient(a: Rgba, b: Rgba) -> Shape {
        Shape { fill0: a, fill1: b, gdir: [0.7071, 0.7071], border: 0.0, bcolor: [0.0; 4], light: None }
    }
    pub fn vertical(a: Rgba, b: Rgba) -> Shape {
        Shape { fill0: a, fill1: b, gdir: [0.0, 1.0], border: 0.0, bcolor: [0.0; 4], light: None }
    }
    pub fn border(mut self, w: f32, c: Rgba) -> Shape {
        self.border = w;
        self.bcolor = c;
        self
    }
    pub fn light(mut self, angle: f32, width: f32, a: Rgba, b: Rgba) -> Shape {
        self.light = Some((angle, width, a, b, 1.0));
        self
    }
}

pub struct Texture {
    pub id: GLuint,
    pub w: u32,
    pub h: u32,
}

struct Metrics {
    width: usize,
    height: usize,
    xmin: f32,
    ymin: f32,
    advance_width: f32,
}

struct Glyph {
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    w: f32,
    h: f32,
    xoff: f32,
    yoff: f32,
    adv: f32,
}

pub const LIGHT: usize = 0;
pub const REGULAR: usize = 1;
pub const MEDIUM: usize = 2;
pub const BOLD: usize = 3;

const ATLAS: usize = 1024;

struct Atlas {
    tex: GLuint,
    x: usize,
    y: usize,
    row_h: usize,
    glyphs: HashMap<(usize, u32, char), Option<Glyph>>,
}

pub struct Draw {
    pub gl: Gl,
    vbo: GLuint,
    shape: Prog,
    radial: Prog,
    ring: Prog,
    text: Prog,
    image: Prog,
    swirl: Prog,
    game: Prog,
    field: Option<Prog>,
    field_fbo: Option<(GLuint, Texture)>,
    fonts: Vec<FontVec>,
    atlas: Atlas,
    text_verts: Vec<f32>,
    pub scale: f32,
    pub width: i32,
    pub height: i32,
    clip: Vec<[f32; 4]>,
}

fn quad(out: &mut Vec<f32>, x0: f32, y0: f32, x1: f32, y1: f32, uv: [f32; 4], local: [f32; 4], c: Rgba) {
    let v = |x: f32, y: f32, u: f32, vv: f32, lx: f32, ly: f32| [x, y, u, vv, lx, ly, c[0], c[1], c[2], c[3]];
    let a = v(x0, y0, uv[0], uv[1], local[0], local[1]);
    let b = v(x1, y0, uv[2], uv[1], local[2], local[1]);
    let cc = v(x0, y1, uv[0], uv[3], local[0], local[3]);
    let d = v(x1, y1, uv[2], uv[3], local[2], local[3]);
    for p in [a, b, cc, b, d, cc] {
        out.extend_from_slice(&p);
    }
}

impl Draw {
    pub fn new(gl: Gl, fonts: Vec<FontVec>, width: i32, height: i32) -> Result<Draw, String> {
        let attribs = ["a_pos", "a_uv", "a_local", "a_color"];
        let mk = |fs: &str, names: &[&'static str]| -> Result<Prog, String> {
            // Full precision first: this TV's driver leaves GL_FRAGMENT_PRECISION_HIGH out, and
            // half precision overflows on 1080p coordinates (black patches in the swirl).
            let ext = if fs.contains("fwidth") { "#extension GL_OES_standard_derivatives : enable\n" } else { "" };
            let id = match gl.program(VS, &format!("{ext}precision highp float;\n{fs}"), &attribs) {
                Ok(id) => id,
                Err(e) => {
                    crate::logln!("full precision refused ({e}); using the driver's default");
                    gl.program(VS, &format!("{ext}{HEADER}{fs}"), &attribs)?
                }
            };
            let mut u = HashMap::new();
            for n in names.iter().chain(["u_view", "u_tex"].iter()) {
                u.insert(*n, gl.uniform(id, n));
            }
            Ok(Prog { id, u })
        };
        let shape = mk(
            FS_SHAPE,
            &["u_half", "u_radius", "u_fill0", "u_fill1", "u_gdir", "u_border", "u_bcolor", "u_mode", "u_soft", "u_light", "u_lc1", "u_lc2"],
        )
        .map_err(|e| format!("shape shader: {e}"))?;
        let radial = mk(FS_RADIAL, &["u_fill0", "u_half"]).map_err(|e| format!("radial shader: {e}"))?;
        let ring = mk(FS_RING, &["u_ring", "u_c1", "u_c2"]).map_err(|e| format!("ring shader: {e}"))?;
        let text = mk(FS_TEXT, &[]).map_err(|e| format!("text shader: {e}"))?;
        let image = mk(FS_IMAGE, &["u_half", "u_radius", "u_bgr"]).map_err(|e| format!("image shader: {e}"))?;
        let swirl = mk(FS_SWIRL, &["u_size"]).map_err(|e| format!("swirl shader: {e}"))?;
        let field = match mk(FS_FIELD, &["u_t", "u_grid", "u_base", "u_whirl", "u_cloud", "u_nclouds", "u_pal", "u_npal"]) {
            Ok(p) => Some(p),
            Err(e) => {
                crate::logln!("the swirl's field shader didn't build ({e}); the processor works it out");
                None
            }
        };
        let game = match mk(FS_GAME, &["u_size", "u_grid", "u_sharp"]) {
            Ok(p) => p,
            Err(e) => {
                crate::logln!("game shader with derivatives failed ({e}); using the plain one");
                let plain = FS_GAME.replace("vec2 fw = max(fwidth(px), vec2(0.0001));", "vec2 fw = vec2(0.25);");
                mk(&plain, &["u_size", "u_grid", "u_sharp"]).map_err(|e| format!("game shader: {e}"))?
            }
        };
        let mut vbo = 0;
        unsafe { (gl.GenBuffers)(1, &mut vbo) };
        let tex = gl.texture(gl::LINEAR);
        unsafe {
            (gl.PixelStorei)(gl::UNPACK_ALIGNMENT, 1);
            let zero = vec![0u8; ATLAS * ATLAS];
            (gl.TexImage2D)(gl::TEXTURE_2D, 0, gl::ALPHA as i32, ATLAS as i32, ATLAS as i32, 0, gl::ALPHA, gl::UNSIGNED_BYTE, zero.as_ptr() as *const _);
        }
        Ok(Draw {
            gl,
            vbo,
            shape,
            radial,
            ring,
            text,
            image,
            swirl,
            game,
            field,
            field_fbo: None,
            fonts,
            atlas: Atlas { tex, x: 0, y: 0, row_h: 0, glyphs: HashMap::new() },
            text_verts: Vec::new(),
            scale: width as f32 / VW,
            width,
            height,
            clip: Vec::new(),
        })
    }

    pub fn begin(&mut self, bg: Rgba) {
        let gl = &self.gl;
        unsafe {
            (gl.Viewport)(0, 0, self.width, self.height);
            (gl.Disable)(gl::DEPTH_TEST);
            (gl.Disable)(gl::CULL_FACE);
            (gl.Enable)(gl::BLEND);
            (gl.BlendFunc)(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
            (gl.ClearColor)(bg[0], bg[1], bg[2], 1.0);
            (gl.Clear)(gl::COLOR_BUFFER_BIT);
        }
    }

    pub fn end(&mut self) {
        self.flush_text();
    }

    fn use_prog(&self, which: &Prog) {
        let gl = &self.gl;
        unsafe {
            (gl.UseProgram)(which.id);
            (gl.Uniform2f)(which.u["u_view"], VW, VH);
            (gl.Uniform1i)(which.u["u_tex"], 0);
        }
    }

    fn submit(&self, verts: &[f32]) {
        let gl = &self.gl;
        unsafe {
            (gl.BindBuffer)(gl::ARRAY_BUFFER, self.vbo);
            (gl.BufferData)(gl::ARRAY_BUFFER, std::mem::size_of_val(verts) as isize, verts.as_ptr() as *const _, gl::STREAM_DRAW);
            let stride = 10 * 4;
            let offs = [(0u32, 2, 0usize), (1, 2, 8), (2, 2, 16), (3, 4, 24)];
            for (loc, n, off) in offs {
                (gl.EnableVertexAttribArray)(loc);
                (gl.VertexAttribPointer)(loc, n, gl::FLOAT, 0, stride, off as *const _);
            }
            (gl.DrawArrays)(gl::TRIANGLES, 0, (verts.len() / 10) as i32);
        }
    }

    // ---- clipping (scroll areas) ----

    pub fn push_clip(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.flush_text();
        let r = if let Some(p) = self.clip.last() {
            let x0 = x.max(p[0]);
            let y0 = y.max(p[1]);
            let x1 = (x + w).min(p[0] + p[2]);
            let y1 = (y + h).min(p[1] + p[3]);
            [x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0)]
        } else {
            [x, y, w, h]
        };
        self.clip.push(r);
        self.apply_clip();
    }

    pub fn pop_clip(&mut self) {
        self.flush_text();
        self.clip.pop();
        self.apply_clip();
    }

    fn apply_clip(&self) {
        let gl = &self.gl;
        unsafe {
            if let Some(r) = self.clip.last() {
                let s = self.scale;
                (gl.Enable)(gl::SCISSOR_TEST);
                let y = self.height as f32 - (r[1] + r[3]) * s;
                (gl.Scissor)((r[0] * s) as i32, y as i32, (r[2] * s).ceil() as i32, (r[3] * s).ceil() as i32);
            } else {
                (gl.Disable)(gl::SCISSOR_TEST);
            }
        }
    }

    // ---- shapes ----

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, s: &Shape) {
        self.flush_text();
        let p = &self.shape;
        self.use_prog(p);
        let gl = &self.gl;
        let r = radius.min(w * 0.5).min(h * 0.5).max(0.0);
        unsafe {
            (gl.Uniform2f)(p.u["u_half"], w * 0.5, h * 0.5);
            (gl.Uniform1f)(p.u["u_radius"], r);
            (gl.Uniform4fv)(p.u["u_fill0"], 1, s.fill0.as_ptr());
            (gl.Uniform4fv)(p.u["u_fill1"], 1, s.fill1.as_ptr());
            (gl.Uniform2f)(p.u["u_gdir"], s.gdir[0], s.gdir[1]);
            (gl.Uniform1f)(p.u["u_border"], s.border);
            (gl.Uniform4fv)(p.u["u_bcolor"], 1, s.bcolor.as_ptr());
            (gl.Uniform1f)(p.u["u_mode"], 0.0);
            (gl.Uniform1f)(p.u["u_soft"], 0.0);
            match s.light {
                Some((angle, width, a, b, k)) => {
                    (gl.Uniform4f)(p.u["u_light"], 1.0, angle, width, k);
                    (gl.Uniform4fv)(p.u["u_lc1"], 1, a.as_ptr());
                    (gl.Uniform4fv)(p.u["u_lc2"], 1, b.as_ptr());
                }
                None => (gl.Uniform4f)(p.u["u_light"], 0.0, 0.0, 0.0, 0.0),
            }
        }
        let mut v = Vec::with_capacity(60);
        let (hw, hh) = (w * 0.5 + 1.0, h * 0.5 + 1.0);
        let (cx, cy) = (x + w * 0.5, y + h * 0.5);
        quad(&mut v, cx - hw, cy - hh, cx + hw, cy + hh, [0.0; 4], [-hw, -hh, hw, hh], [1.0; 4]);
        self.submit(&v);
    }

    /// A soft shadow or glow the shape of a rounded rectangle.
    pub fn glow(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, soft: f32, c: Rgba) {
        self.soft_shape(x, y, w, h, radius, soft, 0.0, c, 1.0);
    }

    /// A blurred stroke along a rounded rectangle's edge.
    pub fn glow_ring(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, width: f32, soft: f32, c: Rgba) {
        self.soft_shape(x, y, w, h, radius, soft, width, c, 2.0);
    }

    #[allow(clippy::too_many_arguments)]
    fn soft_shape(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, soft: f32, width: f32, c: Rgba, mode: f32) {
        self.flush_text();
        let p = &self.shape;
        self.use_prog(p);
        let gl = &self.gl;
        let r = radius.min(w * 0.5).min(h * 0.5).max(0.0);
        unsafe {
            (gl.Uniform2f)(p.u["u_half"], w * 0.5, h * 0.5);
            (gl.Uniform1f)(p.u["u_radius"], r);
            (gl.Uniform4fv)(p.u["u_fill0"], 1, c.as_ptr());
            (gl.Uniform1f)(p.u["u_border"], width);
            (gl.Uniform1f)(p.u["u_mode"], mode);
            (gl.Uniform1f)(p.u["u_soft"], soft.max(0.5));
            (gl.Uniform4f)(p.u["u_light"], 0.0, 0.0, 0.0, 0.0);
        }
        let m = soft * 3.0 + 2.0;
        let (hw, hh) = (w * 0.5 + m, h * 0.5 + m);
        let (cx, cy) = (x + w * 0.5, y + h * 0.5);
        let mut v = Vec::with_capacity(60);
        quad(&mut v, cx - hw, cy - hh, cx + hw, cy + hh, [0.0; 4], [-hw, -hh, hw, hh], [1.0; 4]);
        self.submit(&v);
    }

    /// An oval glow that fades out from its middle.
    pub fn radial(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: Rgba) {
        self.flush_text();
        let p = &self.radial;
        self.use_prog(p);
        unsafe {
            (self.gl.Uniform4fv)(p.u["u_fill0"], 1, c.as_ptr());
            (self.gl.Uniform2f)(p.u["u_half"], rx, ry);
        }
        let mut v = Vec::with_capacity(60);
        quad(&mut v, cx - rx, cy - ry, cx + rx, cy + ry, [0.0; 4], [-rx, -ry, rx, ry], [1.0; 4]);
        self.submit(&v);
    }

    /// A turning ring (the one next to the CONTINUUM name). mode: 0 rainbow, 1 two colours, 2 one.
    #[allow(clippy::too_many_arguments)]
    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, width: f32, angle: f32, mode: f32, c1: Rgba, c2: Rgba) {
        self.flush_text();
        let p = &self.ring;
        self.use_prog(p);
        unsafe {
            (self.gl.Uniform4f)(p.u["u_ring"], r, width, angle, mode);
            (self.gl.Uniform4fv)(p.u["u_c1"], 1, c1.as_ptr());
            (self.gl.Uniform4fv)(p.u["u_c2"], 1, c2.as_ptr());
        }
        let m = r + 2.0;
        let mut v = Vec::with_capacity(60);
        quad(&mut v, cx - m, cy - m, cx + m, cy + m, [0.0; 4], [-m, -m, m, m], [1.0; 4]);
        self.submit(&v);
    }

    // ---- pictures ----

    pub fn texture_rgba(&self, w: u32, h: u32, pixels: &[u8], filter: GLint) -> Texture {
        let gl = &self.gl;
        let id = gl.texture(filter);
        unsafe {
            (gl.PixelStorei)(gl::UNPACK_ALIGNMENT, 1);
            (gl.TexImage2D)(gl::TEXTURE_2D, 0, gl::RGBA as i32, w as i32, h as i32, 0, gl::RGBA, gl::UNSIGNED_BYTE, pixels.as_ptr() as *const _);
        }
        Texture { id, w, h }
    }

    pub fn update_texture(&self, t: &mut Texture, w: u32, h: u32, pixels: &[u8]) {
        let gl = &self.gl;
        unsafe {
            (gl.BindTexture)(gl::TEXTURE_2D, t.id);
            (gl.PixelStorei)(gl::UNPACK_ALIGNMENT, 1);
            if t.w != w || t.h != h {
                (gl.TexImage2D)(gl::TEXTURE_2D, 0, gl::RGBA as i32, w as i32, h as i32, 0, gl::RGBA, gl::UNSIGNED_BYTE, pixels.as_ptr() as *const _);
                t.w = w;
                t.h = h;
            } else {
                (gl.TexSubImage2D)(gl::TEXTURE_2D, 0, 0, 0, w as i32, h as i32, gl::RGBA, gl::UNSIGNED_BYTE, pixels.as_ptr() as *const _);
            }
        }
    }

    pub fn delete_texture(&self, t: &Texture) {
        unsafe { (self.gl.DeleteTextures)(1, &t.id) };
    }

    pub fn set_filter(&self, t: &Texture, filter: GLint) {
        let gl = &self.gl;
        unsafe {
            (gl.BindTexture)(gl::TEXTURE_2D, t.id);
            (gl.TexParameteri)(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, filter);
            (gl.TexParameteri)(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, filter);
        }
    }

    /// A picture in a rounded frame. `tint` multiplies it (alpha fades it).
    #[allow(clippy::too_many_arguments)]
    pub fn image(&mut self, t: &Texture, x: f32, y: f32, w: f32, h: f32, radius: f32, tint: Rgba, bgr: bool) {
        self.flush_text();
        let p = &self.image;
        self.use_prog(p);
        let gl = &self.gl;
        unsafe {
            (gl.ActiveTexture)(gl::TEXTURE0);
            (gl.BindTexture)(gl::TEXTURE_2D, t.id);
            (gl.Uniform2f)(p.u["u_half"], w * 0.5, h * 0.5);
            (gl.Uniform1f)(p.u["u_radius"], radius.min(w * 0.5).min(h * 0.5));
            (gl.Uniform1f)(p.u["u_bgr"], if bgr { 1.0 } else { 0.0 });
        }
        let mut v = Vec::with_capacity(60);
        quad(&mut v, x, y, x + w, y + h, [0.0, 0.0, 1.0, 1.0], [-w * 0.5, -h * 0.5, w * 0.5, h * 0.5], tint);
        self.submit(&v);
    }

    /// The swirl background from its small colour grid, drawn over `x,y,w,h`.
    pub fn swirl(&mut self, t: &Texture, x: f32, y: f32, w: f32, h: f32) {
        self.flush_text();
        let p = &self.swirl;
        self.use_prog(p);
        unsafe {
            (self.gl.ActiveTexture)(gl::TEXTURE0);
            (self.gl.BindTexture)(gl::TEXTURE_2D, t.id);
            (self.gl.Uniform2f)(p.u["u_size"], t.w as f32, t.h as f32);
        }
        let mut v = Vec::with_capacity(60);
        quad(&mut v, x, y, x + w, y + h, [0.0, 0.0, 1.0, 1.0], [0.0; 4], [1.0; 4]);
        self.submit(&v);
    }

    /// The swirl worked out on the graphics chip (into a small texture, one point per pixel)
    /// and drawn over `x,y,w,h` like the processor's version. False when this driver can't.
    #[allow(clippy::too_many_arguments)]
    pub fn swirl_gpu(&mut self, p: &crate::swirl::FieldParams, base: [f32; 3], over: f32, x: f32, y: f32, w: f32, h: f32) -> bool {
        if self.field.is_none() {
            return false;
        }
        self.flush_text();
        let (tw, th) = (p.rows + 1, p.cols + 1);
        let gl = &self.gl;
        if self.field_fbo.as_ref().map(|(_, t)| t.w != tw || t.h != th).unwrap_or(true) {
            let tex = self.texture_rgba(tw, th, &vec![0u8; (tw * th * 4) as usize], gl::LINEAR);
            let mut fbo = 0;
            let ok = unsafe {
                (gl.GenFramebuffers)(1, &mut fbo);
                (gl.BindFramebuffer)(gl::FRAMEBUFFER, fbo);
                (gl.FramebufferTexture2D)(gl::FRAMEBUFFER, gl::COLOR_ATTACHMENT0, gl::TEXTURE_2D, tex.id, 0);
                let st = (gl.CheckFramebufferStatus)(gl::FRAMEBUFFER);
                (gl.BindFramebuffer)(gl::FRAMEBUFFER, 0);
                st == gl::FRAMEBUFFER_COMPLETE
            };
            if !ok {
                crate::logln!("no off-screen drawing on this driver; the processor works the swirl out");
                self.field = None;
                return false;
            }
            self.field_fbo = Some((fbo, tex));
        }
        let fbo = self.field_fbo.as_ref().unwrap().0;
        let prog = self.field.as_ref().unwrap();
        let u = |n: &str| prog.u[n];
        let mut clouds = [0f32; 24];
        for (i, c) in p.clouds.iter().take(8).enumerate() {
            clouds[i * 3..i * 3 + 3].copy_from_slice(c);
        }
        let mut pal = [0f32; 12];
        for (i, c) in p.pal.iter().take(4).enumerate() {
            pal[i * 3..i * 3 + 3].copy_from_slice(c);
        }
        let whirls = [p.whirls[0][0], p.whirls[0][1], p.whirls[0][2], p.whirls[1][0], p.whirls[1][1], p.whirls[1][2]];
        unsafe {
            (gl.BindFramebuffer)(gl::FRAMEBUFFER, fbo);
            (gl.Viewport)(0, 0, tw as i32, th as i32);
            (gl.Disable)(gl::BLEND);
            (gl.Disable)(gl::SCISSOR_TEST);
            (gl.UseProgram)(prog.id);
            (gl.Uniform2f)(u("u_view"), tw as f32, th as f32);
            (gl.Uniform4f)(u("u_t"), p.t, VW / VH, p.mix, over);
            (gl.Uniform2f)(u("u_grid"), p.cols as f32, p.rows as f32);
            (gl.Uniform3f)(u("u_base"), base[0], base[1], base[2]);
            (gl.Uniform3fv)(u("u_whirl"), 2, whirls.as_ptr());
            (gl.Uniform3fv)(u("u_cloud"), 8, clouds.as_ptr());
            (gl.Uniform1f)(u("u_nclouds"), p.clouds.len().min(8) as f32);
            (gl.Uniform3fv)(u("u_pal"), 4, pal.as_ptr());
            (gl.Uniform1f)(u("u_npal"), p.pal.len().min(4) as f32);
        }
        let mut v = Vec::with_capacity(60);
        quad(&mut v, 0.0, 0.0, tw as f32, th as f32, [0.0; 4], [0.0; 4], [1.0; 4]);
        self.submit(&v);
        unsafe {
            (gl.BindFramebuffer)(gl::FRAMEBUFFER, 0);
            (gl.Viewport)(0, 0, self.width, self.height);
            (gl.Enable)(gl::BLEND);
        }
        self.apply_clip();
        // now draw it smoothly scaled, exactly like the processor-made grid
        let p2 = &self.swirl;
        self.use_prog(p2);
        let t = &self.field_fbo.as_ref().unwrap().1;
        unsafe {
            (self.gl.ActiveTexture)(gl::TEXTURE0);
            (self.gl.BindTexture)(gl::TEXTURE_2D, t.id);
            (self.gl.Uniform2f)(p2.u["u_size"], t.w as f32, t.h as f32);
        }
        let mut v = Vec::with_capacity(60);
        // a framebuffer's first row is its bottom: flip so row 0 (the top of the field) is on top
        quad(&mut v, x, y, x + w, y + h, [0.0, 0.0, 1.0, 1.0], [0.0; 4], [1.0; 4]);
        self.submit(&v);
        true
    }

    /// The game's picture. `sharp` keeps pixels square; `grid` (0 to 1) draws an LCD grid.
    #[allow(clippy::too_many_arguments)]
    pub fn game(&mut self, t: &Texture, src_w: u32, src_h: u32, x: f32, y: f32, w: f32, h: f32, sharp: bool, grid: f32) {
        self.flush_text();
        let p = &self.game;
        self.use_prog(p);
        unsafe {
            (self.gl.ActiveTexture)(gl::TEXTURE0);
            (self.gl.BindTexture)(gl::TEXTURE_2D, t.id);
            (self.gl.Uniform2f)(p.u["u_size"], t.w as f32, t.h as f32);
            (self.gl.Uniform1f)(p.u["u_grid"], grid);
            (self.gl.Uniform1f)(p.u["u_sharp"], if sharp { 1.0 } else { 0.0 });
        }
        let u1 = src_w as f32 / t.w as f32;
        let v1 = src_h as f32 / t.h as f32;
        let mut v = Vec::with_capacity(60);
        quad(&mut v, x, y, x + w, y + h, [0.0, 0.0, u1, v1], [0.0; 4], [1.0; 4]);
        self.submit(&v);
    }

    // ---- text ----

    fn glyph(&mut self, font: usize, px: u32, ch: char) -> Option<&Glyph> {
        let key = (font, px, ch);
        if !self.atlas.glyphs.contains_key(&key) {
            let f = &self.fonts[font.min(self.fonts.len() - 1)];
            // sizes are per em, like the phone's points
            let scale = f.pt_to_px_scale(px as f32 * 0.75).unwrap_or(PxScale::from(px as f32));
            let gid = f.glyph_id(ch);
            let adv = f.as_scaled(scale).h_advance(gid);
            let outline = f.outline_glyph(gid.with_scale_and_position(scale, point(0.0, 0.0)));
            let drawn = outline.map(|og| {
                let b = og.px_bounds();
                let (w, h) = (b.width().ceil() as usize, b.height().ceil() as usize);
                let mut bm = vec![0u8; w * h];
                og.draw(|x, y, c| {
                    let i = y as usize * w + x as usize;
                    if i < bm.len() {
                        bm[i] = (c.clamp(0.0, 1.0) * 255.0) as u8;
                    }
                });
                (w, h, b.min.x, b.min.y, bm)
            });
            let g = match drawn {
                None | Some((0, _, _, _, _)) | Some((_, 0, _, _, _)) => {
                    Some(Glyph { u0: 0.0, v0: 0.0, u1: 0.0, v1: 0.0, w: 0.0, h: 0.0, xoff: 0.0, yoff: 0.0, adv })
                }
                Some((mw, mh, xmin, ymin, bitmap)) => {
                let m = Metrics { width: mw, height: mh, xmin, ymin, advance_width: adv };
                let (gw, gh) = (m.width + 1, m.height + 1);
                if self.atlas.x + gw > ATLAS {
                    self.atlas.x = 0;
                    self.atlas.y += self.atlas.row_h;
                    self.atlas.row_h = 0;
                }
                if self.atlas.y + gh > ATLAS {
                    // full: start again (the text already queued is drawn first)
                    self.flush_text();
                    self.atlas.glyphs.clear();
                    self.atlas.x = 0;
                    self.atlas.y = 0;
                    self.atlas.row_h = 0;
                }
                let (ax, ay) = (self.atlas.x, self.atlas.y);
                unsafe {
                    let gl = &self.gl;
                    (gl.BindTexture)(gl::TEXTURE_2D, self.atlas.tex);
                    (gl.PixelStorei)(gl::UNPACK_ALIGNMENT, 1);
                    (gl.TexSubImage2D)(gl::TEXTURE_2D, 0, ax as i32, ay as i32, m.width as i32, m.height as i32, gl::ALPHA, gl::UNSIGNED_BYTE, bitmap.as_ptr() as *const _);
                }
                self.atlas.x += gw;
                self.atlas.row_h = self.atlas.row_h.max(gh);
                let a = ATLAS as f32;
                Some(Glyph {
                    u0: ax as f32 / a,
                    v0: ay as f32 / a,
                    u1: (ax + m.width) as f32 / a,
                    v1: (ay + m.height) as f32 / a,
                    w: m.width as f32,
                    h: m.height as f32,
                    xoff: m.xmin,
                    yoff: m.ymin,
                    adv: m.advance_width,
                })
                }
            };
            self.atlas.glyphs.insert(key, g);
        }
        self.atlas.glyphs.get(&key).and_then(|g| g.as_ref())
    }

    /// Width of a line of text on the 1920x1080 page.
    pub fn measure(&mut self, s: &str, size: f32, font: usize, tracking: f32) -> f32 {
        let px = (size * self.scale).round().max(1.0) as u32;
        let mut w = 0.0;
        for ch in s.chars() {
            if let Some(g) = self.glyph(font, px, ch) {
                w += g.adv / self.scale + tracking;
            }
        }
        w - if s.is_empty() { 0.0 } else { tracking }
    }

    /// Draw a line of text. `y` is the top of the line box (size tall). Returns its width.
    #[allow(clippy::too_many_arguments)]
    pub fn text(&mut self, s: &str, x: f32, y: f32, size: f32, font: usize, c: Rgba, tracking: f32) -> f32 {
        let scale = self.scale;
        let px = (size * scale).round().max(1.0) as u32;
        let ascent = {
            let f = &self.fonts[font.min(self.fonts.len() - 1)];
            let scale = f.pt_to_px_scale(px as f32 * 0.75).unwrap_or(PxScale::from(px as f32));
            f.as_scaled(scale).ascent()
        };
        // baseline in device pixels, centred in the line box
        let line_h = size * scale;
        let base = (y * scale + (line_h - px as f32 * 1.0) * 0.5 + ascent * 0.92).round();
        let mut pen = (x * scale).round();
        let start = pen;
        let mut verts = std::mem::take(&mut self.text_verts);
        for ch in s.chars() {
            let g = match self.glyph(font, px, ch) {
                Some(g) => (g.u0, g.v0, g.u1, g.v1, g.w, g.h, g.xoff, g.yoff, g.adv),
                None => continue,
            };
            let (u0, v0, u1, v1, gw, gh, xoff, yoff, adv) = g;
            if gw > 0.0 {
                let gx = pen + xoff;
                let gy = base + yoff;
                quad(&mut verts, gx / scale, gy / scale, (gx + gw) / scale, (gy + gh) / scale, [u0, v0, u1, v1], [0.0; 4], c);
            }
            pen += adv + tracking * scale;
        }
        self.text_verts = verts;
        (pen - start) / scale - if s.is_empty() { 0.0 } else { tracking }
    }

    /// Text cut to fit `max` wide, with an ellipsis.
    #[allow(clippy::too_many_arguments)]
    pub fn text_fit(&mut self, s: &str, x: f32, y: f32, size: f32, font: usize, c: Rgba, max: f32) -> f32 {
        if self.measure(s, size, font, 0.0) <= max {
            return self.text(s, x, y, size, font, c, 0.0);
        }
        let chars: Vec<char> = s.chars().collect();
        let mut n = chars.len();
        while n > 0 {
            let t: String = chars[..n].iter().collect::<String>().trim_end().to_string() + "…";
            if self.measure(&t, size, font, 0.0) <= max {
                return self.text(&t, x, y, size, font, c, 0.0);
            }
            n -= 1;
        }
        0.0
    }

    /// Text centred on `cx`.
    pub fn text_center(&mut self, s: &str, cx: f32, y: f32, size: f32, font: usize, c: Rgba) {
        let w = self.measure(s, size, font, 0.0);
        self.text(s, cx - w * 0.5, y, size, font, c, 0.0);
    }

    /// Wrap text into lines no wider than `max`.
    pub fn wrap(&mut self, s: &str, size: f32, font: usize, max: f32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut cur = String::new();
        for word in s.split_whitespace() {
            let trial = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
            if self.measure(&trial, size, font, 0.0) > max && !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
            } else {
                cur = trial;
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
        lines
    }

    pub fn flush_text(&mut self) {
        if self.text_verts.is_empty() {
            return;
        }
        let p = &self.text;
        self.use_prog(p);
        unsafe {
            (self.gl.ActiveTexture)(gl::TEXTURE0);
            (self.gl.BindTexture)(gl::TEXTURE_2D, self.atlas.tex);
        }
        let verts = std::mem::take(&mut self.text_verts);
        self.submit(&verts);
        self.text_verts = verts;
        self.text_verts.clear();
    }
}
