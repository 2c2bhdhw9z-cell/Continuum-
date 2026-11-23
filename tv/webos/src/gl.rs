//! OpenGL ES 2 functions, looked up through SDL (the TV's GPU driver provides them).

#![allow(non_snake_case, dead_code, clippy::upper_case_acronyms)]

use std::ffi::{c_char, c_void, CString};

pub type GLenum = u32;
pub type GLuint = u32;
pub type GLint = i32;
pub type GLsizei = i32;
pub type GLfloat = f32;
pub type GLboolean = u8;
pub type GLbitfield = u32;

pub const COLOR_BUFFER_BIT: GLbitfield = 0x4000;
pub const BLEND: GLenum = 0x0BE2;
pub const SCISSOR_TEST: GLenum = 0x0C11;
pub const DEPTH_TEST: GLenum = 0x0B71;
pub const CULL_FACE: GLenum = 0x0B44;
pub const SRC_ALPHA: GLenum = 0x0302;
pub const ONE_MINUS_SRC_ALPHA: GLenum = 0x0303;
pub const ONE: GLenum = 1;
pub const TEXTURE_2D: GLenum = 0x0DE1;
pub const TEXTURE0: GLenum = 0x84C0;
pub const TEXTURE_MIN_FILTER: GLenum = 0x2801;
pub const TEXTURE_MAG_FILTER: GLenum = 0x2800;
pub const TEXTURE_WRAP_S: GLenum = 0x2802;
pub const TEXTURE_WRAP_T: GLenum = 0x2803;
pub const NEAREST: GLint = 0x2600;
pub const LINEAR: GLint = 0x2601;
pub const CLAMP_TO_EDGE: GLint = 0x812F;
pub const RGBA: GLenum = 0x1908;
pub const RGB: GLenum = 0x1907;
pub const ALPHA: GLenum = 0x1906;
pub const LUMINANCE: GLenum = 0x1909;
pub const UNSIGNED_BYTE: GLenum = 0x1401;
pub const FLOAT: GLenum = 0x1406;
pub const UNPACK_ALIGNMENT: GLenum = 0x0CF5;
pub const PACK_ALIGNMENT: GLenum = 0x0D05;
pub const ARRAY_BUFFER: GLenum = 0x8892;
pub const STREAM_DRAW: GLenum = 0x88E0;
pub const STATIC_DRAW: GLenum = 0x88E4;
pub const TRIANGLES: GLenum = 0x0004;
pub const TRIANGLE_STRIP: GLenum = 0x0005;
pub const VERTEX_SHADER: GLenum = 0x8B31;
pub const FRAGMENT_SHADER: GLenum = 0x8B30;
pub const COMPILE_STATUS: GLenum = 0x8B81;
pub const LINK_STATUS: GLenum = 0x8B82;
pub const INFO_LOG_LENGTH: GLenum = 0x8B84;
pub const RENDERER: GLenum = 0x1F01;
pub const VERSION: GLenum = 0x1F02;
pub const FRAMEBUFFER: GLenum = 0x8D40;
pub const COLOR_ATTACHMENT0: GLenum = 0x8CE0;
pub const FRAMEBUFFER_COMPLETE: GLenum = 0x8CD5;

macro_rules! gl_fns {
    ($($name:ident : fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        pub struct Gl {
            $(pub $name: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)*
        }
        impl Gl {
            pub fn load(get: &dyn Fn(&str) -> *mut c_void) -> Result<Gl, String> {
                Ok(Gl {
                    $($name: {
                        let p = get(concat!("gl", stringify!($name)));
                        if p.is_null() {
                            return Err(format!("no gl{}", stringify!($name)));
                        }
                        unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn($($arg),*) $(-> $ret)?>(p) }
                    },)*
                })
            }
        }
    };
}

gl_fns! {
    Viewport: fn(GLint, GLint, GLsizei, GLsizei);
    ClearColor: fn(GLfloat, GLfloat, GLfloat, GLfloat);
    Clear: fn(GLbitfield);
    Enable: fn(GLenum);
    Disable: fn(GLenum);
    BlendFunc: fn(GLenum, GLenum);
    Scissor: fn(GLint, GLint, GLsizei, GLsizei);
    GenTextures: fn(GLsizei, *mut GLuint);
    DeleteTextures: fn(GLsizei, *const GLuint);
    BindTexture: fn(GLenum, GLuint);
    ActiveTexture: fn(GLenum);
    TexImage2D: fn(GLenum, GLint, GLint, GLsizei, GLsizei, GLint, GLenum, GLenum, *const c_void);
    TexSubImage2D: fn(GLenum, GLint, GLint, GLint, GLsizei, GLsizei, GLenum, GLenum, *const c_void);
    TexParameteri: fn(GLenum, GLenum, GLint);
    PixelStorei: fn(GLenum, GLint);
    CreateShader: fn(GLenum) -> GLuint;
    ShaderSource: fn(GLuint, GLsizei, *const *const c_char, *const GLint);
    CompileShader: fn(GLuint);
    GetShaderiv: fn(GLuint, GLenum, *mut GLint);
    GetShaderInfoLog: fn(GLuint, GLsizei, *mut GLsizei, *mut c_char);
    CreateProgram: fn() -> GLuint;
    AttachShader: fn(GLuint, GLuint);
    BindAttribLocation: fn(GLuint, GLuint, *const c_char);
    LinkProgram: fn(GLuint);
    GetProgramiv: fn(GLuint, GLenum, *mut GLint);
    GetProgramInfoLog: fn(GLuint, GLsizei, *mut GLsizei, *mut c_char);
    UseProgram: fn(GLuint);
    GetUniformLocation: fn(GLuint, *const c_char) -> GLint;
    Uniform1i: fn(GLint, GLint);
    Uniform1f: fn(GLint, GLfloat);
    Uniform2f: fn(GLint, GLfloat, GLfloat);
    Uniform3f: fn(GLint, GLfloat, GLfloat, GLfloat);
    Uniform4f: fn(GLint, GLfloat, GLfloat, GLfloat, GLfloat);
    Uniform4fv: fn(GLint, GLsizei, *const GLfloat);
    Uniform3fv: fn(GLint, GLsizei, *const GLfloat);
    GenBuffers: fn(GLsizei, *mut GLuint);
    BindBuffer: fn(GLenum, GLuint);
    BufferData: fn(GLenum, isize, *const c_void, GLenum);
    VertexAttribPointer: fn(GLuint, GLint, GLenum, GLboolean, GLsizei, *const c_void);
    EnableVertexAttribArray: fn(GLuint);
    DrawArrays: fn(GLenum, GLint, GLsizei);
    GetString: fn(GLenum) -> *const u8;
    GetError: fn() -> GLenum;
    ReadPixels: fn(GLint, GLint, GLsizei, GLsizei, GLenum, GLenum, *mut c_void);
    GenFramebuffers: fn(GLsizei, *mut GLuint);
    BindFramebuffer: fn(GLenum, GLuint);
    FramebufferTexture2D: fn(GLenum, GLenum, GLenum, GLuint, GLint);
    CheckFramebufferStatus: fn(GLenum) -> GLenum;
    Finish: fn();
}

impl Gl {
    pub fn string(&self, name: GLenum) -> String {
        unsafe {
            let p = (self.GetString)(name);
            if p.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(p as *const c_char).to_string_lossy().into_owned()
            }
        }
    }

    fn shader(&self, kind: GLenum, src: &str) -> Result<GLuint, String> {
        unsafe {
            let s = (self.CreateShader)(kind);
            let c = CString::new(src).unwrap();
            let p = c.as_ptr();
            (self.ShaderSource)(s, 1, &p, std::ptr::null());
            (self.CompileShader)(s);
            let mut ok = 0;
            (self.GetShaderiv)(s, COMPILE_STATUS, &mut ok);
            if ok == 0 {
                let mut buf = vec![0u8; 2048];
                let mut n = 0;
                (self.GetShaderInfoLog)(s, 2048, &mut n, buf.as_mut_ptr() as *mut c_char);
                return Err(String::from_utf8_lossy(&buf[..n.max(0) as usize]).into_owned());
            }
            Ok(s)
        }
    }

    /// Compile a program whose vertex attributes are `attribs` in order (location 0, 1, ...).
    pub fn program(&self, vs: &str, fs: &str, attribs: &[&str]) -> Result<GLuint, String> {
        unsafe {
            let v = self.shader(VERTEX_SHADER, vs).map_err(|e| format!("vertex: {e}"))?;
            let f = self.shader(FRAGMENT_SHADER, fs).map_err(|e| format!("fragment: {e}"))?;
            let p = (self.CreateProgram)();
            (self.AttachShader)(p, v);
            (self.AttachShader)(p, f);
            for (i, a) in attribs.iter().enumerate() {
                let c = CString::new(*a).unwrap();
                (self.BindAttribLocation)(p, i as GLuint, c.as_ptr());
            }
            (self.LinkProgram)(p);
            let mut ok = 0;
            (self.GetProgramiv)(p, LINK_STATUS, &mut ok);
            if ok == 0 {
                let mut buf = vec![0u8; 2048];
                let mut n = 0;
                (self.GetProgramInfoLog)(p, 2048, &mut n, buf.as_mut_ptr() as *mut c_char);
                return Err(String::from_utf8_lossy(&buf[..n.max(0) as usize]).into_owned());
            }
            Ok(p)
        }
    }

    pub fn uniform(&self, prog: GLuint, name: &str) -> GLint {
        let c = CString::new(name).unwrap();
        unsafe { (self.GetUniformLocation)(prog, c.as_ptr()) }
    }

    pub fn texture(&self, filter: GLint) -> GLuint {
        unsafe {
            let mut t = 0;
            (self.GenTextures)(1, &mut t);
            (self.BindTexture)(TEXTURE_2D, t);
            (self.TexParameteri)(TEXTURE_2D, TEXTURE_MIN_FILTER, filter);
            (self.TexParameteri)(TEXTURE_2D, TEXTURE_MAG_FILTER, filter);
            (self.TexParameteri)(TEXTURE_2D, TEXTURE_WRAP_S, CLAMP_TO_EDGE);
            (self.TexParameteri)(TEXTURE_2D, TEXTURE_WRAP_T, CLAMP_TO_EDGE);
            t
        }
    }
}
