//! Executables: Nintendo's DOL format (every GameCube game's main program, and most homebrew)
//! and 32-bit big-endian PowerPC ELF (homebrew built with devkitPPC).

/// One piece of an executable to copy into memory.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// Where it goes (an effective address, usually 0x80000000 and up).
    pub addr: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Executable {
    pub entry: u32,
    pub sections: Vec<Section>,
    /// Memory to zero (address, length).
    pub bss: Vec<(u32, u32)>,
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

impl Executable {
    /// Reads either format, picked by the ELF magic.
    pub fn parse(bytes: &[u8]) -> Result<Executable, String> {
        if bytes.starts_with(b"\x7FELF") {
            Self::parse_elf(bytes)
        } else {
            Self::parse_dol(bytes)
        }
    }

    pub fn parse_dol(b: &[u8]) -> Result<Executable, String> {
        if b.len() < 0x100 {
            return Err("too small for a DOL header".into());
        }
        let mut sections = Vec::new();
        for i in 0..18 {
            let off = be32(b, i * 4).unwrap() as usize;
            let addr = be32(b, 0x48 + i * 4).unwrap();
            let size = be32(b, 0x90 + i * 4).unwrap() as usize;
            if size == 0 {
                continue;
            }
            let data = b.get(off..off + size).ok_or_else(|| format!("section {i} runs past the end of the file"))?;
            sections.push(Section { addr, data: data.to_vec() });
        }
        if sections.is_empty() {
            return Err("a DOL with no sections".into());
        }
        let bss_addr = be32(b, 0xD8).unwrap();
        let bss_size = be32(b, 0xDC).unwrap();
        let entry = be32(b, 0xE0).unwrap();
        let bss = if bss_size > 0 { vec![(bss_addr, bss_size)] } else { vec![] };
        Ok(Executable { entry, sections, bss })
    }

    pub fn parse_elf(b: &[u8]) -> Result<Executable, String> {
        // 32-bit (class 1), big-endian (data 2), PowerPC (machine 20).
        if b.get(4) != Some(&1) || b.get(5) != Some(&2) {
            return Err("not a 32-bit big-endian ELF".into());
        }
        if be16(b, 18) != Some(20) {
            return Err("not a PowerPC ELF".into());
        }
        let entry = be32(b, 24).ok_or("short ELF header")?;
        let phoff = be32(b, 28).ok_or("short ELF header")? as usize;
        let phentsize = be16(b, 42).ok_or("short ELF header")? as usize;
        let phnum = be16(b, 44).ok_or("short ELF header")? as usize;
        let mut sections = Vec::new();
        let mut bss = Vec::new();
        for i in 0..phnum {
            let p = phoff + i * phentsize;
            // PT_LOAD only.
            if be32(b, p).ok_or("short program header")? != 1 {
                continue;
            }
            let off = be32(b, p + 4).ok_or("short program header")? as usize;
            let vaddr = be32(b, p + 8).ok_or("short program header")?;
            let paddr = be32(b, p + 12).ok_or("short program header")?;
            let filesz = be32(b, p + 16).ok_or("short program header")? as usize;
            let memsz = be32(b, p + 20).ok_or("short program header")?;
            // devkitPPC links at virtual addresses (0x80...); some tools leave vaddr 0.
            let addr = if vaddr != 0 { vaddr } else { paddr };
            if filesz > 0 {
                let data = b.get(off..off + filesz).ok_or("a segment runs past the end of the file")?;
                sections.push(Section { addr, data: data.to_vec() });
            }
            if memsz as usize > filesz {
                bss.push((addr + filesz as u32, memsz - filesz as u32));
            }
        }
        if sections.is_empty() {
            return Err("an ELF with nothing to load".into());
        }
        Ok(Executable { entry, sections, bss })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dol(text: &[u8], addr: u32, entry: u32) -> Vec<u8> {
        let mut b = vec![0u8; 0x100];
        b[0..4].copy_from_slice(&0x100u32.to_be_bytes());
        b[0x48..0x4C].copy_from_slice(&addr.to_be_bytes());
        b[0x90..0x94].copy_from_slice(&(text.len() as u32).to_be_bytes());
        b[0xD8..0xDC].copy_from_slice(&0x8010_0000u32.to_be_bytes());
        b[0xDC..0xE0].copy_from_slice(&0x40u32.to_be_bytes());
        b[0xE0..0xE4].copy_from_slice(&entry.to_be_bytes());
        b.extend_from_slice(text);
        b
    }

    #[test]
    fn reads_a_dol() {
        let e = Executable::parse(&dol(&[1, 2, 3, 4], 0x8000_3100, 0x8000_3100)).unwrap();
        assert_eq!(e.entry, 0x8000_3100);
        assert_eq!(e.sections, vec![Section { addr: 0x8000_3100, data: vec![1, 2, 3, 4] }]);
        assert_eq!(e.bss, vec![(0x8010_0000, 0x40)]);
    }

    #[test]
    fn refuses_a_truncated_dol() {
        let mut d = dol(&[1, 2, 3, 4], 0x8000_3100, 0x8000_3100);
        d.truncate(0x102);
        assert!(Executable::parse(&d).is_err());
    }

    #[test]
    fn reads_an_elf() {
        let mut b = vec![0u8; 0x54 + 8];
        b[0..4].copy_from_slice(b"\x7FELF");
        b[4] = 1;
        b[5] = 2;
        b[18..20].copy_from_slice(&20u16.to_be_bytes());
        b[24..28].copy_from_slice(&0x8000_4000u32.to_be_bytes());
        b[28..32].copy_from_slice(&0x34u32.to_be_bytes());
        b[42..44].copy_from_slice(&32u16.to_be_bytes());
        b[44..46].copy_from_slice(&1u16.to_be_bytes());
        let p = 0x34;
        b[p..p + 4].copy_from_slice(&1u32.to_be_bytes());
        b[p + 4..p + 8].copy_from_slice(&0x54u32.to_be_bytes());
        b[p + 8..p + 12].copy_from_slice(&0x8000_4000u32.to_be_bytes());
        b[p + 16..p + 20].copy_from_slice(&8u32.to_be_bytes());
        b[p + 20..p + 24].copy_from_slice(&0x20u32.to_be_bytes());
        b[0x54..0x5C].copy_from_slice(&[9; 8]);
        let e = Executable::parse(&b).unwrap();
        assert_eq!(e.entry, 0x8000_4000);
        assert_eq!(e.sections[0].data, vec![9; 8]);
        assert_eq!(e.bss, vec![(0x8000_4008, 0x18)]);
    }
}
