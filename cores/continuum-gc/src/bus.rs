//! What the CPU sees on the other side of its address translation: physical memory and devices.
//!
//! The GameCube's processor is big-endian, so every multi-byte access is big-endian here. The
//! CPU only ever hands this trait PHYSICAL addresses: translation (BATs and the page table) is
//! the CPU's job (`cpu::mmu`).

pub trait Bus {
    fn read8(&mut self, pa: u32) -> u8;
    fn read16(&mut self, pa: u32) -> u16;
    fn read32(&mut self, pa: u32) -> u32;
    fn write8(&mut self, pa: u32, v: u8);
    fn write16(&mut self, pa: u32, v: u16);
    fn write32(&mut self, pa: u32, v: u32);

    fn read64(&mut self, pa: u32) -> u64 {
        ((self.read32(pa) as u64) << 32) | self.read32(pa.wrapping_add(4)) as u64
    }
    fn write64(&mut self, pa: u32, v: u64) {
        self.write32(pa, (v >> 32) as u32);
        self.write32(pa.wrapping_add(4), v as u32);
    }

    /// The CPU's write-gather pipe (Gekko's WPAR): 32-byte bursts to the graphics FIFO. A bus
    /// without one treats them as ordinary writes.
    fn gather_pipe_burst(&mut self, pa: u32, data: &[u8; 32]) {
        for (i, b) in data.iter().enumerate() {
            self.write8(pa.wrapping_add(i as u32), *b);
        }
    }

    /// The CPU executed `icbi` or flushed its instruction cache: code at `pa` may have changed.
    fn invalidate_code(&mut self, _pa: u32, _len: u32) {}
}

/// Plain flat memory for tests: `size` bytes from physical 0, everything else reads 0.
pub struct FlatRam {
    pub mem: Vec<u8>,
}

impl FlatRam {
    pub fn new(size: usize) -> Self {
        FlatRam { mem: vec![0; size] }
    }
    fn idx(&self, pa: u32, n: usize) -> Option<usize> {
        let i = pa as usize;
        (i + n <= self.mem.len()).then_some(i)
    }
    pub fn load(&mut self, pa: u32, bytes: &[u8]) {
        let i = pa as usize;
        self.mem[i..i + bytes.len()].copy_from_slice(bytes);
    }
}

impl Bus for FlatRam {
    fn read8(&mut self, pa: u32) -> u8 {
        self.idx(pa, 1).map_or(0, |i| self.mem[i])
    }
    fn read16(&mut self, pa: u32) -> u16 {
        self.idx(pa, 2).map_or(0, |i| u16::from_be_bytes([self.mem[i], self.mem[i + 1]]))
    }
    fn read32(&mut self, pa: u32) -> u32 {
        self.idx(pa, 4).map_or(0, |i| u32::from_be_bytes(self.mem[i..i + 4].try_into().unwrap()))
    }
    fn write8(&mut self, pa: u32, v: u8) {
        if let Some(i) = self.idx(pa, 1) {
            self.mem[i] = v;
        }
    }
    fn write16(&mut self, pa: u32, v: u16) {
        if let Some(i) = self.idx(pa, 2) {
            self.mem[i..i + 2].copy_from_slice(&v.to_be_bytes());
        }
    }
    fn write32(&mut self, pa: u32, v: u32) {
        if let Some(i) = self.idx(pa, 4) {
            self.mem[i..i + 4].copy_from_slice(&v.to_be_bytes());
        }
    }
}
