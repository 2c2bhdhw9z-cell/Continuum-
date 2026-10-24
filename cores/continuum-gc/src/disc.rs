//! Game discs. A `Disc` is anything that reads bytes at an offset of the plain disc image; plain
//! ISO/GCM files are the first kind, and the compressed formats (RVZ, WIA, GCZ, CISO, WBFS) will
//! each be another, so the rest of the emulator never knows which it has.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

pub const GC_MAGIC: u32 = 0xC233_9F3D;
pub const WII_MAGIC: u32 = 0x5D1C_9EA3;

pub trait Disc: Send {
    /// Fills `buf` from `offset` in the plain image. Bytes past the end read as zero.
    fn read(&mut self, offset: u64, buf: &mut [u8]) -> std::io::Result<()>;
    fn size(&self) -> u64;
}

/// A plain disc image file (.iso / .gcm).
pub struct PlainImage {
    file: File,
    size: u64,
}

impl PlainImage {
    pub fn open(path: &str) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        Ok(PlainImage { file, size })
    }
}

impl Disc for PlainImage {
    fn read(&mut self, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
        buf.fill(0);
        if offset >= self.size {
            return Ok(());
        }
        self.file.seek(SeekFrom::Start(offset))?;
        let n = ((self.size - offset) as usize).min(buf.len());
        self.file.read_exact(&mut buf[..n])
    }
    fn size(&self) -> u64 {
        self.size
    }
}

/// A disc held in memory (tests, and small homebrew images).
pub struct MemoryImage(pub Vec<u8>);

impl Disc for MemoryImage {
    fn read(&mut self, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
        buf.fill(0);
        let start = offset.min(self.0.len() as u64) as usize;
        let n = (self.0.len() - start).min(buf.len());
        buf[..n].copy_from_slice(&self.0[start..start + n]);
        Ok(())
    }
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Console {
    GameCube,
    Wii,
}

/// The fields of the disc header (boot.bin and bi2.bin) the emulator needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// Six characters: system, game, region, maker (for example "GALE01").
    pub game_id: String,
    pub disc_number: u8,
    pub version: u8,
    pub console: Console,
    pub title: String,
    /// GameCube only (Wii discs keep these inside the encrypted partition).
    pub dol_offset: u32,
    pub fst_offset: u32,
    pub fst_size: u32,
    /// Region code from bi2.bin: 0 Japan, 1 USA, 2 PAL, 4 Korea.
    pub region: u32,
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
}

impl Header {
    pub fn read(disc: &mut dyn Disc) -> Result<Header, String> {
        let mut b = vec![0u8; 0x2440];
        disc.read(0, &mut b).map_err(|e| e.to_string())?;
        let console = if be32(&b, 0x1C) == GC_MAGIC {
            Console::GameCube
        } else if be32(&b, 0x18) == WII_MAGIC {
            Console::Wii
        } else {
            return Err("not a GameCube or Wii disc".into());
        };
        let text = |s: &[u8]| {
            let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
            String::from_utf8_lossy(&s[..end]).trim().to_string()
        };
        Ok(Header {
            game_id: text(&b[0..6]),
            disc_number: b[6],
            version: b[7],
            console,
            title: text(&b[0x20..0x400]),
            dol_offset: be32(&b, 0x420),
            fst_offset: be32(&b, 0x424),
            fst_size: be32(&b, 0x428),
            region: be32(&b, 0x458),
        })
    }
}

/// The apploader at 0x2440: the small program the real console runs to load a game.
#[derive(Debug, Clone, PartialEq)]
pub struct Apploader {
    pub entry: u32,
    pub code: Vec<u8>,
}

impl Apploader {
    pub fn read(disc: &mut dyn Disc) -> Result<Apploader, String> {
        let mut h = [0u8; 0x20];
        disc.read(0x2440, &mut h).map_err(|e| e.to_string())?;
        let entry = be32(&h, 0x10);
        let size = be32(&h, 0x14) + be32(&h, 0x18);
        if size == 0 || size > 0x40_0000 {
            return Err("no apploader".into());
        }
        let mut code = vec![0u8; size as usize];
        disc.read(0x2460, &mut code).map_err(|e| e.to_string())?;
        Ok(Apploader { entry, code })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_gamecube_header() {
        let mut img = vec![0u8; 0x2440];
        img[0..6].copy_from_slice(b"TEST01");
        img[0x1C..0x20].copy_from_slice(&GC_MAGIC.to_be_bytes());
        img[0x20..0x29].copy_from_slice(b"Test Disc");
        img[0x420..0x424].copy_from_slice(&0x1_0000u32.to_be_bytes());
        img[0x458..0x45C].copy_from_slice(&1u32.to_be_bytes());
        let h = Header::read(&mut MemoryImage(img)).unwrap();
        assert_eq!(h.game_id, "TEST01");
        assert_eq!(h.console, Console::GameCube);
        assert_eq!(h.title, "Test Disc");
        assert_eq!(h.dol_offset, 0x1_0000);
        assert_eq!(h.region, 1);
    }

    #[test]
    fn refuses_something_else() {
        assert!(Header::read(&mut MemoryImage(vec![0; 0x3000])).is_err());
    }
}
