//! The graphics engine ("GX" in the homebrew SDK): Flipper's command processor (CP), transform
//! unit (XF), setup and rasteriser, texture units, colour combiner (TEV) and pixel engine (PE),
//! drawing into the embedded framebuffer (EFB) and copying out to main memory.
//!
//! Written from scratch from public hardware notes (YAGCD) and from what the homebrew SDK
//! (libogc) writes to the hardware. This is the exact CPU drawer: the yardstick a GPU drawer will
//! be checked against (gamecube.json, G6).
//!
//! How data arrives: the CPU's write-gather pipe sends 32-byte bursts to the processor
//! interface (PI), which writes them into the FIFO in main memory. When the FIFO is linked (the
//! usual "immediate mode"), the command processor reads it straight away and every command is
//! carried out at once, so drawing is finished by the time the CPU's store completes.
//!
//! Layout: `mod.rs` registers, FIFO and the command parser; `vertex.rs` vertex reading and the
//! transform unit; `raster.rs` clipping, triangle setup and rasterising; `tev.rs` the combiner,
//! alpha test and fog; `texture.rs` texture formats and sampling; `efb.rs` the pixel engine and
//! EFB copies.

use serde::{Deserialize, Serialize};

mod draw;
mod efb;
mod raster;
mod tev;
mod texture;
mod vertex;

pub use efb::{EFB_HEIGHT, EFB_WIDTH};

/// Physical memory as the graphics engine's DMA sees it.
pub struct Ram<'a> {
    pub mem1: &'a mut [u8],
    pub mem2: &'a mut [u8],
}

impl Ram<'_> {
    #[inline]
    fn locate(&self, pa: u32, n: usize) -> Option<(bool, usize)> {
        let a = pa as usize;
        if a + n <= self.mem1.len() {
            return Some((false, a));
        }
        if !self.mem2.is_empty() && (0x1000_0000..0x1000_0000 + self.mem2.len()).contains(&a) {
            let o = a - 0x1000_0000;
            if o + n <= self.mem2.len() {
                return Some((true, o));
            }
        }
        None
    }

    #[inline]
    pub fn read8(&self, pa: u32) -> u8 {
        let pa = pa & 0x1FFF_FFFF;
        match self.locate(pa, 1) {
            Some((false, o)) => self.mem1[o],
            Some((true, o)) => self.mem2[o],
            None => 0,
        }
    }

    #[inline]
    pub fn read16(&self, pa: u32) -> u16 {
        let pa = pa & 0x1FFF_FFFF;
        match self.locate(pa, 2) {
            Some((false, o)) => u16::from_be_bytes([self.mem1[o], self.mem1[o + 1]]),
            Some((true, o)) => u16::from_be_bytes([self.mem2[o], self.mem2[o + 1]]),
            None => ((self.read8(pa) as u16) << 8) | self.read8(pa + 1) as u16,
        }
    }

    #[inline]
    pub fn read32(&self, pa: u32) -> u32 {
        let pa = pa & 0x1FFF_FFFF;
        match self.locate(pa, 4) {
            Some((false, o)) => u32::from_be_bytes(self.mem1[o..o + 4].try_into().unwrap()),
            Some((true, o)) => u32::from_be_bytes(self.mem2[o..o + 4].try_into().unwrap()),
            None => ((self.read16(pa) as u32) << 16) | self.read16(pa + 2) as u32,
        }
    }

    /// The bytes at `pa .. pa + n` when they are one piece of RAM (for hashing a texture).
    pub fn slice(&self, pa: u32, n: usize) -> Option<&[u8]> {
        match self.locate(pa & 0x1FFF_FFFF, n) {
            Some((false, o)) => Some(&self.mem1[o..o + n]),
            Some((true, o)) => Some(&self.mem2[o..o + n]),
            None => None,
        }
    }

    pub fn read_into(&self, pa: u32, out: &mut [u8]) {
        let pa = pa & 0x1FFF_FFFF;
        match self.locate(pa, out.len()) {
            Some((false, o)) => out.copy_from_slice(&self.mem1[o..o + out.len()]),
            Some((true, o)) => out.copy_from_slice(&self.mem2[o..o + out.len()]),
            None => {
                for (k, b) in out.iter_mut().enumerate() {
                    *b = self.read8(pa + k as u32);
                }
            }
        }
    }

    pub fn write(&mut self, pa: u32, data: &[u8]) {
        let pa = pa & 0x1FFF_FFFF;
        match self.locate(pa, data.len()) {
            Some((false, o)) => self.mem1[o..o + data.len()].copy_from_slice(data),
            Some((true, o)) => self.mem2[o..o + data.len()].copy_from_slice(data),
            None => {
                for (k, b) in data.iter().enumerate() {
                    if let Some((m2, o)) = self.locate(pa + k as u32, 1) {
                        if m2 {
                            self.mem2[o] = *b;
                        } else {
                            self.mem1[o] = *b;
                        }
                    }
                }
            }
        }
    }
}

/// The command processor's view of the FIFO in main memory (all physical, 32-byte aligned).
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct CpFifo {
    pub base: u32,
    pub end: u32,
    pub hi: u32,
    pub lo: u32,
    pub dist: u32,
    pub wptr: u32,
    pub rptr: u32,
    pub bp: u32,
}

// CP control register bits.
const CR_READ: u16 = 1 << 0;
const CR_BP: u16 = 1 << 1;
const CR_HI_INT: u16 = 1 << 2;
const CR_LO_INT: u16 = 1 << 3;
const CR_LINK: u16 = 1 << 4;
const CR_BP_INT: u16 = 1 << 5;
// CP status register bits.
const SR_HI: u16 = 1 << 0;
const SR_LO: u16 = 1 << 1;
const SR_READ_IDLE: u16 = 1 << 2;
const SR_CMD_IDLE: u16 = 1 << 3;
const SR_BP: u16 = 1 << 4;

// PE interrupt register bits (PE 0x0A).
const PE_TOKEN_EN: u16 = 1 << 0;
const PE_FINISH_EN: u16 = 1 << 1;
const PE_TOKEN_INT: u16 = 1 << 2;
const PE_FINISH_INT: u16 = 1 << 3;

/// Counters for development read-outs (not part of save states).
#[derive(Default, Clone, Debug)]
pub struct Stats {
    pub commands: u64,
    pub primitives: u64,
    pub vertices: u64,
    pub triangles: u64,
    pub pixels: u64,
    pub efb_copies: u64,
    pub xfb_copies: u64,
    pub display_lists: u64,
    pub unknown_opcodes: u64,
    pub last_unknown: u8,
    /// Time spent drawing (development read-out).
    pub draw_time: std::time::Duration,
    /// Time the CPU's thread spent waiting for (and helping with) the helpers' pixels.
    pub wait_time: std::time::Duration,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Gx {
    /// Drawing helper threads (draw.rs). First, so that it is dropped (finishing its queue)
    /// before the EFB it draws into.
    #[serde(skip)]
    draw: draw::Threads,
    // ---- CP registers (CPU side, 0x0C000000)
    pub cp_sr: u16,
    pub cp_cr: u16,
    pub cp_token: u16,
    pub fifo: CpFifo,
    /// Performance counters and other CP registers nothing reads meaningfully.
    pub cp_misc: Vec<u16>,

    // ---- CP internal registers (loaded through the FIFO with command 0x08)
    pub matidx_a: u32,
    pub matidx_b: u32,
    pub vcd_lo: u32,
    pub vcd_hi: u32,
    pub vat_a: [u32; 8],
    pub vat_b: [u32; 8],
    pub vat_c: [u32; 8],
    pub array_base: [u32; 16],
    pub array_stride: [u32; 16],

    // ---- XF: matrix and light memory (0x0000-0x07FF) and registers (0x1000-0x10FF)
    pub xfmem: Vec<u32>,
    pub xfregs: Vec<u32>,

    // ---- BP registers (setup, rasteriser, texture, TEV, PE)
    pub bp: Vec<u32>,
    bp_mask: u32,
    /// TEV colour registers (PREV, REG0-2) as signed 11-bit RGBA, and the konstant colours.
    pub tev_regs: [[i16; 4]; 4],
    pub tev_konst: [[u8; 4]; 4],

    // ---- PE registers (CPU side, 0x0C001000)
    pub pe_regs: [u16; 16],

    // ---- memories
    #[serde(with = "big_bytes")]
    pub tmem: Vec<u8>,
    #[serde(with = "big_u32s")]
    pub efb_color: Vec<u32>,
    #[serde(with = "big_u32s")]
    pub efb_depth: Vec<u32>,

    /// FIFO bytes read but not yet a whole command.
    queue: Vec<u8>,
    /// Decoded textures (rebuilt on demand; never in a save state).
    #[serde(skip)]
    pub(crate) tex_cache: std::collections::HashMap<texture::TexKey, texture::Cached>,
    /// Moves on whenever cached textures may have gone stale (a texture-cache invalidate, a
    /// palette or TMEM load, an EFB copy to a texture). A cached texture from an older epoch is
    /// checked against the bytes it was decoded from before it is used, and decoded again only
    /// if they changed (`texture::Gx::decoded`).
    pub(crate) tex_epoch: u64,
    /// Bytes the decoded textures in `tex_cache` take (kept under `texture::TEX_CACHE_BYTES`).
    #[serde(skip)]
    pub(crate) tex_cache_bytes: usize,
    #[serde(skip)]
    pub stats: Stats,
}

mod big_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        v.serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        Vec::deserialize(d)
    }
}

mod big_u32s {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &Vec<u32>, s: S) -> Result<S::Ok, S::Error> {
        v.serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u32>, D::Error> {
        Vec::deserialize(d)
    }
}

impl Default for Gx {
    fn default() -> Self {
        Self::new()
    }
}

/// How many bytes a command needs before it can run, or None if not known yet.
enum Need {
    Bytes(usize),
    More,
}

impl Gx {
    pub fn new() -> Self {
        let mut bp = vec![0; 0x100];
        // The line/point setup's default width (6/6 of a pixel).
        bp[0x22] = 0x0000_0606;
        Gx {
            draw: draw::Threads::default(),
            cp_sr: SR_READ_IDLE | SR_CMD_IDLE,
            cp_cr: 0,
            cp_token: 0,
            fifo: CpFifo::default(),
            cp_misc: vec![0; 0x40],
            matidx_a: 0,
            matidx_b: 0,
            vcd_lo: 0,
            vcd_hi: 0,
            vat_a: [0; 8],
            vat_b: [0; 8],
            vat_c: [0; 8],
            array_base: [0; 16],
            array_stride: [0; 16],
            xfmem: vec![0; 0x800],
            xfregs: vec![0; 0x100],
            bp,
            bp_mask: 0x00FF_FFFF,
            tev_regs: [[0; 4]; 4],
            tev_konst: [[0; 4]; 4],
            pe_regs: [0; 16],
            tmem: vec![0; 1 << 20],
            efb_color: vec![0; efb::EFB_WIDTH * efb::EFB_HEIGHT],
            efb_depth: vec![0x00FF_FFFF; efb::EFB_WIDTH * efb::EFB_HEIGHT],
            queue: Vec::new(),
            tex_cache: Default::default(),
            tex_epoch: 0,
            tex_cache_bytes: 0,
            stats: Stats::default(),
        }
    }

    /// Forgets every decoded texture.
    pub(crate) fn clear_tex_cache(&mut self) {
        self.tex_cache.clear();
        self.tex_cache_bytes = 0;
    }

    // ------------------------------------------------------------------ drawing threads

    /// Draws on `count` helper threads (other processor cores) from now on; 0 or 1 draws on
    /// the calling thread. The picture is exactly the same either way.
    pub fn set_draw_threads(&mut self, count: usize) {
        if count.max(1) != self.draw.count().max(1) {
            self.sync();
            self.draw = draw::Threads::start(count);
        }
    }

    pub fn draw_threads(&self) -> usize {
        self.draw.count()
    }

    /// Waits for everything queued to be drawn into the EFB.
    pub fn sync(&mut self) {
        let t = std::time::Instant::now();
        let n = self.draw.wait();
        self.stats.pixels += n;
        self.stats.wait_time += t.elapsed();
    }

    /// `sync` for a shared borrow (save states); the pixel count is added at the next `sync`.
    pub fn wait_idle(&self) {
        // Pixels counted here are dropped from the development counter; nothing else.
        let _ = self.draw.wait();
    }

    // ------------------------------------------------------------------ interrupts

    pub fn irq_cp(&self) -> bool {
        let (sr, cr) = (self.cp_sr, self.cp_cr);
        (sr & SR_HI != 0 && cr & CR_HI_INT != 0) || (sr & SR_LO != 0 && cr & CR_LO_INT != 0) || (sr & SR_BP != 0 && cr & CR_BP_INT != 0)
    }

    pub fn irq_token(&self) -> bool {
        let r = self.pe_regs[5];
        r & PE_TOKEN_INT != 0 && r & PE_TOKEN_EN != 0
    }

    pub fn irq_finish(&self) -> bool {
        let r = self.pe_regs[5];
        r & PE_FINISH_INT != 0 && r & PE_FINISH_EN != 0
    }

    fn update_watermarks(&mut self) {
        let f = &self.fifo;
        self.cp_sr &= !(SR_HI | SR_LO);
        if f.dist > f.hi {
            self.cp_sr |= SR_HI;
        }
        if f.dist < f.lo {
            self.cp_sr |= SR_LO;
        }
    }

    // ------------------------------------------------------------------ CP registers (CPU side)

    pub fn cp_read(&mut self, off: u32) -> u16 {
        let f = &self.fifo;
        let lo = |v: u32| v as u16;
        let hi = |v: u32| (v >> 16) as u16;
        match off & 0x7E {
            0x00 => self.cp_sr | SR_READ_IDLE | SR_CMD_IDLE,
            0x02 => self.cp_cr,
            0x04 => 0,
            0x0E => self.cp_token,
            0x20 => lo(f.base),
            0x22 => hi(f.base),
            0x24 => lo(f.end),
            0x26 => hi(f.end),
            0x28 => lo(f.hi),
            0x2A => hi(f.hi),
            0x2C => lo(f.lo),
            0x2E => hi(f.lo),
            0x30 => lo(f.dist),
            0x32 => hi(f.dist),
            0x34 => lo(f.wptr),
            0x36 => hi(f.wptr),
            0x38 => lo(f.rptr),
            0x3A => hi(f.rptr),
            0x3C => lo(f.bp),
            0x3E => hi(f.bp),
            o => self.cp_misc[(o / 2) as usize & 0x3F],
        }
    }

    pub fn cp_write(&mut self, ram: &mut Ram, off: u32, v: u16) {
        let set_lo = |r: &mut u32, v: u16| *r = (*r & 0xFFFF_0000) | (v as u32 & 0xFFE0);
        let set_hi = |r: &mut u32, v: u16| *r = (*r & 0xFFFF) | ((v as u32 & 0x3FFF) << 16);
        let f = &mut self.fifo;
        match off & 0x7E {
            0x00 => {}
            0x02 => {
                self.cp_cr = v;
                // Turning the breakpoint off clears its status.
                if v & CR_BP == 0 {
                    self.cp_sr &= !SR_BP;
                }
            }
            0x04 => {
                // Clear: bit 0 the overflow status, bit 1 the underflow status.
                if v & 1 != 0 {
                    self.cp_sr &= !SR_HI;
                }
                if v & 2 != 0 {
                    self.cp_sr &= !SR_LO;
                }
                return;
            }
            0x0E => self.cp_token = v,
            0x20 => set_lo(&mut f.base, v),
            0x22 => set_hi(&mut f.base, v),
            0x24 => set_lo(&mut f.end, v),
            0x26 => set_hi(&mut f.end, v),
            0x28 => f.hi = (f.hi & 0xFFFF_0000) | (v as u32 & 0xFFE0),
            0x2A => f.hi = (f.hi & 0xFFFF) | ((v as u32 & 0x3FFF) << 16),
            0x2C => f.lo = (f.lo & 0xFFFF_0000) | (v as u32 & 0xFFE0),
            0x2E => f.lo = (f.lo & 0xFFFF) | ((v as u32 & 0x3FFF) << 16),
            0x30 => f.dist = (f.dist & 0xFFFF_0000) | (v as u32 & 0xFFE0),
            0x32 => f.dist = (f.dist & 0xFFFF) | ((v as u32 & 0x3FFF) << 16),
            0x34 => set_lo(&mut f.wptr, v),
            0x36 => set_hi(&mut f.wptr, v),
            0x38 => {
                set_lo(&mut f.rptr, v);
                self.queue.clear();
            }
            0x3A => {
                set_hi(&mut f.rptr, v);
                self.queue.clear();
            }
            0x3C => set_lo(&mut f.bp, v),
            0x3E => set_hi(&mut f.bp, v),
            o => {
                self.cp_misc[(o / 2) as usize & 0x3F] = v;
                return;
            }
        }
        self.update_watermarks();
        self.run(ram);
    }

    /// PI wrote a 32-byte burst into the FIFO; when linked, the CP's write pointer follows.
    pub fn pi_burst(&mut self, ram: &mut Ram, new_wptr: u32) {
        if self.cp_cr & CR_LINK == 0 {
            return;
        }
        self.fifo.wptr = new_wptr;
        self.fifo.dist = self.fifo.dist.wrapping_add(32);
        self.update_watermarks();
        self.run(ram);
    }

    /// The command processor reads everything between its read and write pointers.
    fn run(&mut self, ram: &mut Ram) {
        if self.cp_cr & CR_READ == 0 {
            return;
        }
        let mut block = [0u8; 32];
        let mut guard = 0u32;
        while self.fifo.rptr != self.fifo.wptr {
            if self.cp_cr & CR_BP != 0 && self.fifo.rptr == self.fifo.bp {
                self.cp_sr |= SR_BP;
                break;
            }
            ram.read_into(self.fifo.rptr, &mut block);
            self.queue.extend_from_slice(&block);
            if self.fifo.rptr == (self.fifo.end & !31) {
                self.fifo.rptr = self.fifo.base;
            } else {
                self.fifo.rptr = self.fifo.rptr.wrapping_add(32);
            }
            self.fifo.dist = self.fifo.dist.wrapping_sub(32);
            guard += 1;
            if guard > (1 << 22) {
                // A FIFO whose pointers make no sense: stop rather than spin forever.
                self.fifo.rptr = self.fifo.wptr;
                break;
            }
            // Run whole commands as they arrive, so huge FIFOs do not pile up.
            if self.queue.len() >= 4096 {
                self.drain(ram);
            }
        }
        self.drain(ram);
        if self.fifo.rptr == self.fifo.wptr {
            self.fifo.dist = 0;
        }
        self.update_watermarks();
    }

    /// Runs every whole command waiting in the queue.
    fn drain(&mut self, ram: &mut Ram) {
        let q = std::mem::take(&mut self.queue);
        let used = self.parse(ram, &q, 0);
        self.queue = q;
        self.queue.drain(..used);
    }

    /// Runs whole commands from `data`; returns how many bytes were used.
    fn parse(&mut self, ram: &mut Ram, data: &[u8], depth: u32) -> usize {
        let mut p = 0;
        while p < data.len() {
            let need = match self.command_size(data, p) {
                Need::Bytes(n) => n,
                Need::More => break,
            };
            if p + need > data.len() {
                break;
            }
            self.execute(ram, &data[p..p + need], depth);
            p += need;
        }
        p
    }

    fn command_size(&self, d: &[u8], p: usize) -> Need {
        let op = d[p];
        match op {
            0x00 | 0x48 | 0x44 => Need::Bytes(1),
            0x08 => Need::Bytes(6),
            0x10 => {
                if d.len() < p + 5 {
                    return Need::More;
                }
                let w = u32::from_be_bytes(d[p + 1..p + 5].try_into().unwrap());
                Need::Bytes(5 + 4 * (((w >> 16) & 0xF) as usize + 1))
            }
            0x20 | 0x28 | 0x30 | 0x38 | 0x61 => Need::Bytes(5),
            0x40 => Need::Bytes(9),
            0x80..=0xBF => {
                if d.len() < p + 3 {
                    return Need::More;
                }
                let count = u16::from_be_bytes([d[p + 1], d[p + 2]]) as usize;
                Need::Bytes(3 + count * self.vertex_size((op & 7) as usize))
            }
            _ => Need::Bytes(1),
        }
    }

    fn execute(&mut self, ram: &mut Ram, c: &[u8], depth: u32) {
        self.stats.commands += 1;
        let be32 = |o: usize| u32::from_be_bytes(c[o..o + 4].try_into().unwrap());
        match c[0] {
            0x00 | 0x48 | 0x44 => {}
            0x08 => self.cp_internal_write(c[1], be32(2)),
            0x10 => {
                let w = be32(1);
                let addr = w & 0xFFFF;
                let n = ((w >> 16) & 0xF) + 1;
                for k in 0..n {
                    self.xf_write(addr + k, be32(5 + 4 * k as usize));
                }
            }
            0x20 | 0x28 | 0x30 | 0x38 => {
                // Indexed XF load from arrays 12-15: index (16 bits), length-1 (4), address (12).
                let w = be32(1);
                let array = 12 + ((c[0] as usize - 0x20) >> 3);
                let index = w >> 16;
                let n = ((w >> 12) & 0xF) + 1;
                let addr = w & 0xFFF;
                let src = self.array_base[array].wrapping_add(index.wrapping_mul(self.array_stride[array]));
                for k in 0..n {
                    let v = ram.read32(src + 4 * k);
                    self.xf_write(addr + k, v);
                }
            }
            0x40 => {
                let addr = be32(1) & 0x1FFF_FFE0;
                let size = be32(5) as usize;
                if depth < 2 && size > 0 && size < (32 << 20) {
                    self.stats.display_lists += 1;
                    let mut list = vec![0u8; size];
                    ram.read_into(addr, &mut list);
                    self.parse(ram, &list, depth + 1);
                }
            }
            0x61 => self.bp_write(ram, c[1], be32(1) & 0x00FF_FFFF),
            0x80..=0xBF => {
                let prim = (c[0] >> 3) & 7;
                let fmt = (c[0] & 7) as usize;
                let count = u16::from_be_bytes([c[1], c[2]]) as usize;
                self.stats.primitives += 1;
                let t = std::time::Instant::now();
                self.draw(ram, prim, fmt, count, &c[3..]);
                self.stats.draw_time += t.elapsed();
            }
            op => {
                self.stats.unknown_opcodes += 1;
                self.stats.last_unknown = op;
            }
        }
    }

    // ------------------------------------------------------------------ register files

    fn cp_internal_write(&mut self, reg: u8, v: u32) {
        let i = (reg & 0xF) as usize;
        match reg & 0xF0 {
            0x30 => self.matidx_a = v,
            0x40 => self.matidx_b = v,
            0x50 => self.vcd_lo = v,
            0x60 => self.vcd_hi = v,
            0x70 => self.vat_a[i & 7] = v,
            0x80 => self.vat_b[i & 7] = v,
            0x90 => self.vat_c[i & 7] = v,
            0xA0 => self.array_base[i] = v & 0x1FFF_FFFF,
            0xB0 => self.array_stride[i] = v & 0xFF,
            _ => {}
        }
    }

    pub fn xf_write(&mut self, addr: u32, v: u32) {
        if addr < 0x1000 {
            if let Some(m) = self.xfmem.get_mut(addr as usize) {
                *m = v;
            }
        } else if addr < 0x1100 {
            self.xfregs[(addr - 0x1000) as usize] = v;
        }
    }

    pub fn xf_float(&self, addr: usize) -> f32 {
        f32::from_bits(self.xfmem[addr & 0x7FF])
    }

    pub fn xfreg(&self, addr: u32) -> u32 {
        self.xfregs[(addr - 0x1000) as usize & 0xFF]
    }

    fn bp_write(&mut self, ram: &mut Ram, reg: u8, value: u32) {
        let r = reg as usize;
        if reg == 0xFE {
            self.bp_mask = value;
            return;
        }
        let old = self.bp[r];
        let v = (old & !self.bp_mask) | (value & self.bp_mask);
        self.bp_mask = 0x00FF_FFFF;
        self.bp[r] = v;
        match reg {
            // Draw done: the finish interrupt.
            0x45 => {
                if v & 2 != 0 {
                    self.pe_regs[5] |= PE_FINISH_INT;
                }
            }
            0x47 => self.pe_regs[7] = v as u16,
            0x48 => {
                self.pe_regs[7] = v as u16;
                self.pe_regs[5] |= PE_TOKEN_INT;
            }
            // These used to throw every decoded texture away, and Animal Crossing does them all
            // the time (its N64 display-list code loads a palette for nearly every texture): on
            // the owner's iPhone (build 230) decoding textures again was a quarter of the game
            // thread. Now the cache is only marked as possibly stale, and each texture is checked
            // against its bytes the next time it is used.
            0x52 => {
                self.efb_copy(ram, v);
                if v & (1 << 14) == 0 {
                    self.tex_epoch += 1;
                }
            }
            0x63 => {
                self.preload_texture(ram, v);
                self.tex_epoch += 1;
            }
            0x65 => {
                self.load_tlut(ram, v);
                self.tex_epoch += 1;
            }
            0x66 => self.tex_epoch += 1,
            0xE0..=0xE7 => {
                // Pairs: even = red (0-10) and alpha (12-22); odd = blue (0-10) and green (12-22).
                // Bit 23 says whether this is a konstant colour or a TEV register.
                let k = (r - 0xE0) / 2;
                let lo = (v & 0x7FF) as i16;
                let hi = ((v >> 12) & 0x7FF) as i16;
                let s11 = |x: i16| (x << 5) >> 5;
                let konst = v & 0x80_0000 != 0;
                let (c0, c1) = if r & 1 == 0 { (0, 3) } else { (2, 1) };
                if konst {
                    self.tev_konst[k][c0] = lo as u8;
                    self.tev_konst[k][c1] = hi as u8;
                } else {
                    self.tev_regs[k][c0] = s11(lo);
                    self.tev_regs[k][c1] = s11(hi);
                }
            }
            _ => {}
        }
    }

    /// Texture preload into TMEM (BP 0x60-0x63).
    fn preload_texture(&mut self, ram: &mut Ram, v: u32) {
        let src = (self.bp[0x60] & 0x00FF_FFFF) << 5;
        let even = ((self.bp[0x61] & 0x7FFF) << 5) as usize;
        let odd = ((self.bp[0x62] & 0x7FFF) << 5) as usize;
        let lines = (v & 0x7FFF) as usize;
        let kind = (v >> 15) & 3;
        let mut line = [0u8; 32];
        if kind == 3 {
            // 32-bit textures: each 64-byte block's first half (AR) goes to the even bank, the
            // second (GB) to the odd bank.
            for k in 0..lines {
                ram.read_into(src + (k as u32) * 32, &mut line);
                let half = k / 2;
                let dst = if k % 2 == 0 { even + half * 32 } else { odd + half * 32 };
                if dst + 32 <= self.tmem.len() {
                    self.tmem[dst..dst + 32].copy_from_slice(&line);
                }
            }
        } else {
            for k in 0..lines {
                ram.read_into(src + (k as u32) * 32, &mut line);
                let dst = even + k * 32;
                if dst + 32 <= self.tmem.len() {
                    self.tmem[dst..dst + 32].copy_from_slice(&line);
                }
            }
        }
    }

    /// A palette load into the upper half of TMEM (BP 0x64, 0x65).
    fn load_tlut(&mut self, ram: &mut Ram, v: u32) {
        let src = (self.bp[0x64] & 0x00FF_FFFF) << 5;
        let dst = 0x80000 + ((v & 0x3FF) << 9) as usize;
        let len = (((v >> 10) & 0x7FF) as usize) * 32;
        let mut buf = vec![0u8; len];
        ram.read_into(src, &mut buf);
        let end = (dst + len).min(self.tmem.len());
        if dst < end {
            self.tmem[dst..end].copy_from_slice(&buf[..end - dst]);
        }
    }

    // ------------------------------------------------------------------ PE registers (CPU side)

    pub fn pe_read(&self, off: u32) -> u16 {
        self.pe_regs[((off & 0x1F) / 2) as usize]
    }

    pub fn pe_write(&mut self, off: u32, v: u16) {
        let i = ((off & 0x1F) / 2) as usize;
        if i == 5 {
            // Enables are stored; the two status bits clear on 1.
            let mut r = (self.pe_regs[5] & (PE_TOKEN_INT | PE_FINISH_INT)) | (v & 3);
            if v & PE_TOKEN_INT != 0 {
                r &= !PE_TOKEN_INT;
            }
            if v & PE_FINISH_INT != 0 {
                r &= !PE_FINISH_INT;
            }
            self.pe_regs[5] = r;
        } else {
            self.pe_regs[i] = v;
        }
    }
}
