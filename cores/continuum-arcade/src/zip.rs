//! Reading a zip: its central directory (each file's name, size and CRC32) and its files, stored
//! or deflated. Arcade sets come as zips; a set is recognised from the directory alone, so only the
//! chips a board loads are ever unpacked.
//!
//! Our own code, from PKWARE's APPNOTE.TXT (the layout, including the ZIP64 records large-file
//! tools write) and RFC 1951 (deflate), so the core needs no outside crate.

/// One file in the zip, as its central directory describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The name as stored, folders included ("pacman/pacman.6e" in some merged sets).
    pub name: String,
    /// The unpacked size.
    pub size: u64,
    pub crc: u32,
    method: u16,
    packed: u64,
    /// Where its local header is.
    header: u64,
    encrypted: bool,
}

impl Entry {
    /// The name without any folder, lower case: what a chip is called.
    pub fn base_name(&self) -> String {
        self.name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&self.name)
            .to_ascii_lowercase()
    }
}

/// A zip held in memory.
pub struct Zip {
    data: Vec<u8>,
    entries: Vec<Entry>,
}

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(d: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(d.get(at..at + 8)?.try_into().ok()?))
}

const END_OF_DIRECTORY: u32 = 0x0605_4B50;
const ZIP64_END: u32 = 0x0606_4B50;
const ZIP64_LOCATOR: u32 = 0x0706_4B50;
const DIRECTORY_ENTRY: u32 = 0x0201_4B50;
const LOCAL_HEADER: u32 = 0x0403_4B50;

impl Zip {
    /// Reads the directory. Fails on anything that isn't a zip, with a reason for the log.
    pub fn parse(data: Vec<u8>) -> Result<Zip, String> {
        let d = &data;
        // The end record is the last thing in the file, followed only by a comment of up to 64 KB.
        let lowest = d.len().saturating_sub(22 + 0xFFFF);
        let end = (lowest..=d.len().saturating_sub(22))
            .rev()
            .find(|&at| u32_at(d, at) == Some(END_OF_DIRECTORY))
            .ok_or("not a zip (no end-of-directory record)")?;
        let mut count = u64::from(u16_at(d, end + 10).ok_or("cut off")?);
        let mut dir_len = u64::from(u32_at(d, end + 12).ok_or("cut off")?);
        let mut dir_at = u64::from(u32_at(d, end + 16).ok_or("cut off")?);
        // ZIP64: the 32-bit fields are all ones and the real values are in a second record that a
        // locator just before the end record points to.
        let zip64 = count == 0xFFFF || dir_len == 0xFFFF_FFFF || dir_at == 0xFFFF_FFFF;
        if zip64 && end >= 20 && u32_at(d, end - 20) == Some(ZIP64_LOCATOR) {
            let rec = u64_at(d, end - 20 + 8).ok_or("cut off")? as usize;
            if u32_at(d, rec) != Some(ZIP64_END) {
                return Err("a ZIP64 locator that points at nothing".into());
            }
            count = u64_at(d, rec + 32).ok_or("cut off")?;
            dir_len = u64_at(d, rec + 40).ok_or("cut off")?;
            dir_at = u64_at(d, rec + 48).ok_or("cut off")?;
        }
        let (mut at, stop) = (dir_at as usize, (dir_at + dir_len) as usize);
        if stop > d.len() {
            return Err("the directory runs past the end of the file".into());
        }
        let mut entries = Vec::new();
        for _ in 0..count {
            if u32_at(d, at) != Some(DIRECTORY_ENTRY) {
                return Err("a damaged directory".into());
            }
            let flags = u16_at(d, at + 8).ok_or("cut off")?;
            let method = u16_at(d, at + 10).ok_or("cut off")?;
            let crc = u32_at(d, at + 16).ok_or("cut off")?;
            let mut packed = u64::from(u32_at(d, at + 20).ok_or("cut off")?);
            let mut size = u64::from(u32_at(d, at + 24).ok_or("cut off")?);
            let name_len = usize::from(u16_at(d, at + 28).ok_or("cut off")?);
            let extra_len = usize::from(u16_at(d, at + 30).ok_or("cut off")?);
            let note_len = usize::from(u16_at(d, at + 32).ok_or("cut off")?);
            let mut header = u64::from(u32_at(d, at + 42).ok_or("cut off")?);
            let name_at = at + 46;
            let name = d.get(name_at..name_at + name_len).ok_or("cut off")?;
            let name = String::from_utf8_lossy(name).into_owned();
            // A ZIP64 extra field holds, in this order, whichever of the sizes and offset are
            // all ones in the entry itself.
            let mut x = name_at + name_len;
            let extra_end = x + extra_len;
            while x + 4 <= extra_end {
                let id = u16_at(d, x).ok_or("cut off")?;
                let len = usize::from(u16_at(d, x + 2).ok_or("cut off")?);
                if id == 0x0001 {
                    let mut f = x + 4;
                    for field in [&mut size, &mut packed, &mut header] {
                        if *field == 0xFFFF_FFFF && f + 8 <= x + 4 + len {
                            *field = u64_at(d, f).ok_or("cut off")?;
                            f += 8;
                        }
                    }
                }
                x += 4 + len;
            }
            if !name.ends_with('/') {
                entries.push(Entry {
                    name,
                    size,
                    crc,
                    method,
                    packed,
                    header,
                    encrypted: flags & 1 != 0,
                });
            }
            at = extra_end + note_len;
        }
        Ok(Zip { data, entries })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Unpacks entry `i`, and checks its CRC32 against the directory's.
    pub fn read(&self, i: usize) -> Result<Vec<u8>, String> {
        let e = self.entries.get(i).ok_or("no such file in the zip")?;
        if e.encrypted {
            return Err(format!("{} is encrypted", e.name));
        }
        let d = &self.data;
        let h = e.header as usize;
        if u32_at(d, h) != Some(LOCAL_HEADER) {
            return Err(format!("{}: a damaged file header", e.name));
        }
        let name_len = usize::from(u16_at(d, h + 26).ok_or("cut off")?);
        let extra_len = usize::from(u16_at(d, h + 28).ok_or("cut off")?);
        let start = h + 30 + name_len + extra_len;
        let packed = d
            .get(start..start + e.packed as usize)
            .ok_or_else(|| format!("{} is cut off", e.name))?;
        let out = match e.method {
            0 => packed.to_vec(),
            8 => inflate(packed, e.size as usize).map_err(|why| format!("{}: {why}", e.name))?,
            m => return Err(format!("{} is packed a way this core can't unpack (method {m})", e.name)),
        };
        if out.len() as u64 != e.size {
            return Err(format!("{} unpacks to the wrong size", e.name));
        }
        if crc32(&out) != e.crc {
            return Err(format!("{} is damaged (its CRC doesn't match)", e.name));
        }
        Ok(out)
    }
}

/// Writes a plain zip of stored (unpacked) files. Used by the tests to build sets from cArcade's
/// own test programs.
#[doc(hidden)]
pub fn write_stored(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut dir = Vec::new();
    for (name, data) in files {
        let at = out.len() as u32;
        let crc = crc32(data);
        let fixed = |sig: u32, v: &mut Vec<u8>| {
            v.extend(sig.to_le_bytes());
            if sig == DIRECTORY_ENTRY {
                v.extend(20u16.to_le_bytes()); // made by
            }
            v.extend(20u16.to_le_bytes()); // needed
            v.extend(0u16.to_le_bytes()); // flags
            v.extend(0u16.to_le_bytes()); // stored
            v.extend(0u32.to_le_bytes()); // time and date
            v.extend(crc.to_le_bytes());
            v.extend((data.len() as u32).to_le_bytes());
            v.extend((data.len() as u32).to_le_bytes());
            v.extend((name.len() as u16).to_le_bytes());
            v.extend(0u16.to_le_bytes()); // extra
        };
        fixed(LOCAL_HEADER, &mut out);
        out.extend(name.as_bytes());
        out.extend_from_slice(data);
        fixed(DIRECTORY_ENTRY, &mut dir);
        dir.extend([0u8; 8]); // comment length, disk, internal and external attributes
        dir.extend([0u8; 2]);
        dir.extend(at.to_le_bytes());
        dir.extend(name.as_bytes());
    }
    let dir_at = out.len() as u32;
    out.extend(&dir);
    out.extend(END_OF_DIRECTORY.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((dir.len() as u32).to_le_bytes());
    out.extend(dir_at.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

// ---- CRC32 ------------------------------------------------------------------------------------

/// The CRC32 every zip uses (IEEE 802.3: polynomial 04C11DB7h, bits taken low first, so EDB88320h
/// reflected), one byte at a time from a table.
const CRC_TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[n] = c;
        n += 1;
    }
    t
};

pub fn crc32(data: &[u8]) -> u32 {
    !data.iter().fold(!0u32, |c, &b| {
        CRC_TABLE[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8)
    })
}

// ---- Inflate (RFC 1951) ------------------------------------------------------------------------

/// The deflate stream's bits, taken from each byte's lowest bit up.
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
    buf: u32,
    have: u32,
}

impl Bits<'_> {
    fn need(&mut self, n: u32) -> Result<(), String> {
        while self.have < n {
            let b = *self.data.get(self.at).ok_or("the packed data is cut off")?;
            self.at += 1;
            self.buf |= u32::from(b) << self.have;
            self.have += 8;
        }
        Ok(())
    }

    fn take(&mut self, n: u32) -> Result<u32, String> {
        if n == 0 {
            return Ok(0);
        }
        self.need(n)?;
        let v = self.buf & ((1 << n) - 1);
        self.buf >>= n;
        self.have -= n;
        Ok(v)
    }

    /// Drops the bits left in the current byte (a stored block starts on a byte).
    fn skip_to_byte(&mut self) {
        let drop = self.have % 8;
        self.buf >>= drop;
        self.have -= drop;
    }
}

/// A canonical Huffman code (RFC 1951 3.2.2): how many codes there are of each length, and the
/// symbols in code order. Decoded a bit at a time: codes of one length are consecutive numbers, so
/// a code read so far is a symbol of this length when it falls below the next length's first code.
struct Code {
    count: [u16; 16],
    symbols: Vec<u16>,
}

impl Code {
    fn new(lengths: &[u8]) -> Result<Code, String> {
        let mut count = [0u16; 16];
        for &l in lengths {
            count[usize::from(l)] += 1;
        }
        count[0] = 0;
        // More codes than a length can hold is no code at all; fewer is allowed (a one-symbol or
        // unused distance code).
        let mut room: i32 = 1;
        for &c in &count[1..] {
            room = room * 2 - i32::from(c);
            if room < 0 {
                return Err("a damaged Huffman table".into());
            }
        }
        let mut first = [0u16; 16];
        for l in 1..16 {
            first[l] = first[l - 1] + count[l - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (s, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[usize::from(first[usize::from(l)])] = s as u16;
                first[usize::from(l)] += 1;
            }
        }
        Ok(Code { count, symbols })
    }

    fn decode(&self, bits: &mut Bits) -> Result<u16, String> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= bits.take(1)? as i32;
            let n = i32::from(self.count[len]);
            if code - n < first {
                return Ok(self.symbols[(index + code - first) as usize]);
            }
            index += n;
            first = (first + n) << 1;
            code <<= 1;
        }
        Err("a code that isn't in the Huffman table".into())
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13,
];
/// The order the code length code's lengths come in (RFC 1951 3.2.7).
const LENGTH_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// Unpacks a raw deflate stream that should come to `size` bytes.
pub fn inflate(data: &[u8], size: usize) -> Result<Vec<u8>, String> {
    let mut bits = Bits {
        data,
        at: 0,
        buf: 0,
        have: 0,
    };
    let mut out = Vec::with_capacity(size);
    loop {
        let last = bits.take(1)? == 1;
        match bits.take(2)? {
            0 => {
                bits.skip_to_byte();
                let len = bits.take(16)?;
                let check = bits.take(16)?;
                if len != !check & 0xFFFF {
                    return Err("a damaged stored block".into());
                }
                for _ in 0..len {
                    out.push(bits.take(8)? as u8);
                }
            }
            1 => {
                // The fixed codes (RFC 1951 3.2.6).
                let mut lit = [8u8; 288];
                lit[144..256].fill(9);
                lit[256..280].fill(7);
                let lit = Code::new(&lit)?;
                let dist = Code::new(&[5u8; 30])?;
                block(&mut bits, &mut out, &lit, &dist, size)?;
            }
            2 => {
                let nlit = bits.take(5)? as usize + 257;
                let ndist = bits.take(5)? as usize + 1;
                let nlen = bits.take(4)? as usize + 4;
                let mut lens = [0u8; 19];
                for &i in &LENGTH_ORDER[..nlen] {
                    lens[i] = bits.take(3)? as u8;
                }
                let lens = Code::new(&lens)?;
                let mut lengths = Vec::with_capacity(nlit + ndist);
                while lengths.len() < nlit + ndist {
                    let (value, times) = match lens.decode(&mut bits)? {
                        l @ 0..=15 => (l as u8, 1),
                        16 => {
                            let prev = *lengths.last().ok_or("a repeat with nothing before it")?;
                            (prev, 3 + bits.take(2)?)
                        }
                        17 => (0, 3 + bits.take(3)?),
                        _ => (0, 11 + bits.take(7)?),
                    };
                    for _ in 0..times {
                        lengths.push(value);
                    }
                }
                if lengths.len() > nlit + ndist {
                    return Err("code lengths run past the table".into());
                }
                if lengths[256] == 0 {
                    return Err("a block with no end code".into());
                }
                let lit = Code::new(&lengths[..nlit])?;
                let dist = Code::new(&lengths[nlit..])?;
                block(&mut bits, &mut out, &lit, &dist, size)?;
            }
            _ => return Err("a block of an unknown kind".into()),
        }
        if last {
            return Ok(out);
        }
    }
}

/// One Huffman-coded block: literals, and copies of what came before, up to the end code.
fn block(bits: &mut Bits, out: &mut Vec<u8>, lit: &Code, dist: &Code, size: usize) -> Result<(), String> {
    loop {
        let sym = lit.decode(bits)?;
        match sym {
            0..=255 => out.push(sym as u8),
            256 => return Ok(()),
            _ => {
                let i = usize::from(sym - 257);
                if i >= 29 {
                    return Err("a length code that doesn't exist".into());
                }
                let len = usize::from(LENGTH_BASE[i]) + bits.take(u32::from(LENGTH_EXTRA[i]))? as usize;
                let d = usize::from(dist.decode(bits)?);
                if d >= 30 {
                    return Err("a distance code that doesn't exist".into());
                }
                let back = usize::from(DIST_BASE[d]) + bits.take(u32::from(DIST_EXTRA[d]))? as usize;
                if back > out.len() {
                    return Err("a copy from before the start".into());
                }
                let from = out.len() - back;
                for k in 0..len {
                    let b = out[from + k];
                    out.push(b);
                }
            }
        }
        if out.len() > size {
            return Err("it unpacks to more than its size".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn crc32_check_value() {
        // The standard check value for "123456789".
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    fn deflate(data: &[u8], level: u32) -> Vec<u8> {
        let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(level));
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    fn samples() -> Vec<Vec<u8>> {
        let mut rng = 0x1234_5678u32;
        let mut noise = || {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            rng
        };
        let random: Vec<u8> = (0..20_000).map(|_| noise() as u8).collect();
        // Like a ROM: runs, repeats far apart and some noise.
        let rom: Vec<u8> = (0..65_536u32)
            .map(|i| match i % 7 {
                0 => (i >> 8) as u8,
                1..=3 => 0xFF,
                _ => (noise() % 4) as u8,
            })
            .collect();
        vec![Vec::new(), b"a".to_vec(), vec![0; 100_000], random, rom, b"abcabcabcabcabcabd".repeat(500)]
    }

    #[test]
    fn inflate_matches_a_real_deflater_at_every_level() {
        for data in samples() {
            for level in 0..=9 {
                let packed = deflate(&data, level);
                let got = inflate(&packed, data.len()).unwrap_or_else(|e| panic!("level {level}: {e}"));
                assert!(got == data, "level {level}, {} bytes", data.len());
            }
        }
    }

    #[test]
    fn inflate_refuses_damage() {
        let data = b"the quick brown fox jumps over the lazy dog ".repeat(40);
        let packed = deflate(&data, 9);
        assert!(inflate(&packed[..packed.len() / 2], data.len()).is_err(), "cut off");
        assert!(inflate(&packed, 10).is_err(), "longer than its size");
        // A block of the reserved kind (BTYPE 3).
        assert!(inflate(&[0b0000_0111], 0).is_err());
    }

    #[test]
    fn a_zip_reads_back_stored_and_deflated() {
        let a = b"stored chip".to_vec();
        let b = (0..5000u32).map(|i| (i * 7 % 251) as u8).collect::<Vec<u8>>();
        let mut zip = write_stored(&[("a.6e", &a), ("folder/b.6f", &b)]);
        let z = Zip::parse(zip.clone()).unwrap();
        assert_eq!(z.entries().len(), 2);
        assert_eq!((z.entries()[0].name.as_str(), z.entries()[0].size, z.entries()[0].crc), ("a.6e", 11, crc32(&a)));
        assert_eq!(z.entries()[1].base_name(), "b.6f");
        assert_eq!(z.read(0).unwrap(), a);
        assert_eq!(z.read(1).unwrap(), b);
        // A damaged byte in a file is caught by its CRC.
        let at = zip.windows(a.len()).position(|w| w == a.as_slice()).unwrap();
        zip[at] ^= 1;
        assert!(Zip::parse(zip).unwrap().read(0).unwrap_err().contains("damaged"));
        assert!(Zip::parse(b"not a zip at all".to_vec()).is_err());
    }

    #[test]
    fn a_zip_made_by_a_real_zip_writer_reads_back() {
        // A deflated entry, a stored one and a folder, written by hand the way zip tools do.
        let files: [(&str, Vec<u8>, bool); 3] = [
            ("pacman.6e", (0..4096u32).map(|i| (i % 13) as u8).collect(), true),
            ("sub/", Vec::new(), false),
            ("82s123.7f", (0..32u8).collect(), false),
        ];
        let mut out = Vec::new();
        let mut dir = Vec::new();
        for (name, data, deflated) in &files {
            let body = if *deflated { deflate(data, 6) } else { data.clone() };
            let at = out.len() as u32;
            let head = |sig: u32, v: &mut Vec<u8>| {
                v.extend(sig.to_le_bytes());
                if sig == DIRECTORY_ENTRY {
                    v.extend(0x031Eu16.to_le_bytes());
                }
                v.extend(20u16.to_le_bytes());
                v.extend(0u16.to_le_bytes());
                v.extend((if *deflated { 8u16 } else { 0 }).to_le_bytes());
                v.extend(0x5A5A_5A5Au32.to_le_bytes());
                v.extend(crc32(data).to_le_bytes());
                v.extend((body.len() as u32).to_le_bytes());
                v.extend((data.len() as u32).to_le_bytes());
                v.extend((name.len() as u16).to_le_bytes());
                // An extended-timestamp extra field, as Info-ZIP writes.
                v.extend(9u16.to_le_bytes());
            };
            head(LOCAL_HEADER, &mut out);
            out.extend(name.as_bytes());
            out.extend([0x55, 0x54, 5, 0, 1, 0, 0, 0, 0]);
            out.extend(&body);
            head(DIRECTORY_ENTRY, &mut dir);
            dir.extend(0u16.to_le_bytes());
            dir.extend([0u8; 8]);
            dir.extend(at.to_le_bytes());
            dir.extend(name.as_bytes());
            dir.extend([0x55, 0x54, 5, 0, 1, 0, 0, 0, 0]);
        }
        let dir_at = out.len() as u32;
        out.extend(&dir);
        out.extend(END_OF_DIRECTORY.to_le_bytes());
        out.extend([0u8; 4]);
        out.extend(3u16.to_le_bytes());
        out.extend(3u16.to_le_bytes());
        out.extend((dir.len() as u32).to_le_bytes());
        out.extend(dir_at.to_le_bytes());
        let note = b"a zip comment";
        out.extend((note.len() as u16).to_le_bytes());
        out.extend(note);
        let z = Zip::parse(out).unwrap();
        let names: Vec<&str> = z.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["pacman.6e", "82s123.7f"], "the folder isn't a file");
        assert_eq!(z.read(0).unwrap(), files[0].1);
        assert_eq!(z.read(1).unwrap(), files[2].1);
    }
}
