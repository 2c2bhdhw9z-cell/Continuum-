//! Reading a 7z archive: its header (each file's name, size and CRC32) and its files, packed with
//! LZMA, LZMA2 or stored. Arcade sets come as 7z files too (7-Zip's own default is LZMA2 with the
//! header itself LZMA-packed; torrent7z's sets are LZMA).
//!
//! Our own code, written from Igor Pavlov's description of the 7z format (7zFormat.txt) and his
//! LZMA specification (lzma-specification.txt), both in the public domain, and from the xz
//! format's description of LZMA2's chunks. Every number in the file is checked before it is used:
//! a damaged or hostile archive is refused with a reason, never a panic.

/// The first six bytes of every 7z archive.
pub const SIGNATURE: [u8; 6] = [b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C];

pub fn is_7z(d: &[u8]) -> bool {
    d.len() >= 32 && d[..6] == SIGNATURE
}

/// One file in the archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub name: String,
    pub size: u64,
    pub crc: Option<u32>,
    /// Which folder (packed stream) holds it and where in that folder's unpacked bytes; `None`
    /// for an empty file.
    pub folder: Option<usize>,
    pub offset: u64,
}

/// How one folder is packed: one coder (or, for an archive this reader doesn't follow, more).
#[derive(Clone, Debug)]
struct Coder {
    id: Vec<u8>,
    props: Vec<u8>,
    ins: u64,
    outs: u64,
}

#[derive(Clone, Debug)]
struct Folder {
    coders: Vec<Coder>,
    /// Where its packed bytes are in the archive, and how many.
    pack_at: u64,
    pack_len: u64,
    unpacked: u64,
    crc: Option<u32>,
}

/// The archive's directory.
#[derive(Clone, Debug)]
pub struct Listing {
    pub files: Vec<File>,
    folders: Vec<Folder>,
}

// ---- reading the header -------------------------------------------------------------------

struct Reader<'a> {
    d: &'a [u8],
    at: usize,
}

const CUT: &str = "the archive's header is cut off";

impl<'a> Reader<'a> {
    fn byte(&mut self) -> Result<u8, String> {
        let b = *self.d.get(self.at).ok_or(CUT)?;
        self.at += 1;
        Ok(b)
    }

    fn bytes(&mut self, n: u64) -> Result<&'a [u8], String> {
        let n = usize::try_from(n).map_err(|_| CUT)?;
        let end = self.at.checked_add(n).ok_or(CUT)?;
        let s = self.d.get(self.at..end).ok_or(CUT)?;
        self.at = end;
        Ok(s)
    }

    /// 7z's variable-length number: the first byte's leading one bits say how many more bytes
    /// follow (little-endian), its other bits are the top of the value.
    fn number(&mut self) -> Result<u64, String> {
        let first = self.byte()?;
        let mut mask = 0x80u8;
        let mut value = 0u64;
        for i in 0..8 {
            if first & mask == 0 {
                let high = u64::from(first & mask.wrapping_sub(1));
                return Ok(value | high << (8 * i));
            }
            value |= u64::from(self.byte()?) << (8 * i);
            mask >>= 1;
        }
        Ok(value)
    }

    /// A count that has to fit in memory: no more than the header has bytes.
    fn count(&mut self) -> Result<usize, String> {
        let n = self.number()?;
        if n > self.d.len() as u64 {
            return Err("the archive's header counts more than it holds".into());
        }
        Ok(n as usize)
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().expect("four bytes")))
    }

    /// A bit per item, the first in each byte's top bit.
    fn bits(&mut self, n: usize) -> Result<Vec<bool>, String> {
        let mut v = Vec::with_capacity(n);
        let mut b = 0u8;
        for i in 0..n {
            if i % 8 == 0 {
                b = self.byte()?;
            }
            v.push(b & (0x80 >> (i % 8)) != 0);
        }
        Ok(v)
    }

    /// "All defined" or a bit per item, then a CRC32 for each defined one.
    fn digests(&mut self, n: usize) -> Result<Vec<Option<u32>>, String> {
        let all = self.byte()?;
        let defined = if all != 0 { vec![true; n] } else { self.bits(n)? };
        defined.into_iter().map(|d| if d { self.u32().map(Some) } else { Ok(None) }).collect()
    }

    /// Skips a property this reader doesn't need: its size, then that many bytes.
    fn skip_property(&mut self) -> Result<(), String> {
        let n = self.number()?;
        self.bytes(n)?;
        Ok(())
    }
}

// The header's property IDs.
const END: u8 = 0x00;
const HEADER: u8 = 0x01;
const ARCHIVE_PROPERTIES: u8 = 0x02;
const ADDITIONAL_STREAMS: u8 = 0x03;
const MAIN_STREAMS: u8 = 0x04;
const FILES_INFO: u8 = 0x05;
const PACK_INFO: u8 = 0x06;
const UNPACK_INFO: u8 = 0x07;
const SUBSTREAMS_INFO: u8 = 0x08;
const SIZE: u8 = 0x09;
const CRC: u8 = 0x0A;
const FOLDER: u8 = 0x0B;
const CODERS_UNPACK_SIZE: u8 = 0x0C;
const NUM_UNPACK_STREAM: u8 = 0x0D;
const EMPTY_STREAM: u8 = 0x0E;
const NAME: u8 = 0x11;
const ENCODED_HEADER: u8 = 0x17;

/// What a streams-info block says: the folders, and the files (substreams) in each.
struct Streams {
    folders: Vec<Folder>,
    /// Per folder: the sizes and CRCs of the files in it, in order.
    substreams: Vec<Vec<(u64, Option<u32>)>>,
}

fn read_streams(r: &mut Reader, base: u64) -> Result<Streams, String> {
    let mut pack_sizes: Vec<u64> = Vec::new();
    let mut pack_pos = 0u64;
    let mut folders: Vec<Folder> = Vec::new();
    let mut per_folder: Option<Vec<usize>> = None;
    let mut sub_sizes: Option<Vec<Vec<u64>>> = None;
    let mut sub_crcs: Option<Vec<Option<u32>>> = None;
    loop {
        match r.byte()? {
            END => break,
            PACK_INFO => {
                pack_pos = r.number()?;
                let n = r.count()?;
                loop {
                    match r.byte()? {
                        END => break,
                        SIZE => pack_sizes = (0..n).map(|_| r.number()).collect::<Result<_, _>>()?,
                        CRC => {
                            r.digests(n)?;
                        }
                        _ => r.skip_property()?,
                    }
                }
            }
            UNPACK_INFO => {
                if r.byte()? != FOLDER {
                    return Err("a damaged folder list".into());
                }
                let n = r.count()?;
                if r.byte()? != 0 {
                    return Err("an archive whose folders are stored elsewhere".into());
                }
                let mut outs_per_folder = Vec::new();
                for _ in 0..n {
                    let coders_n = r.count()?;
                    let mut coders = Vec::new();
                    let (mut ins, mut outs) = (0u64, 0u64);
                    for _ in 0..coders_n {
                        let flags = r.byte()?;
                        let id = r.bytes(u64::from(flags & 0x0F))?.to_vec();
                        let (ci, co) = if flags & 0x10 != 0 { (r.number()?, r.number()?) } else { (1, 1) };
                        let props = if flags & 0x20 != 0 {
                            let n = r.number()?;
                            r.bytes(n)?.to_vec()
                        } else {
                            Vec::new()
                        };
                        if flags & 0x80 != 0 {
                            return Err("an archive with alternative coders".into());
                        }
                        ins = ins.saturating_add(ci);
                        outs = outs.saturating_add(co);
                        coders.push(Coder { id, props, ins: ci, outs: co });
                    }
                    if outs == 0 || outs > 64 || ins > 64 {
                        return Err("a damaged folder".into());
                    }
                    for _ in 0..outs - 1 {
                        r.number()?;
                        r.number()?;
                    }
                    let packed = ins - (outs - 1);
                    if packed > 1 {
                        for _ in 0..packed {
                            r.number()?;
                        }
                    }
                    outs_per_folder.push(outs);
                    folders.push(Folder { coders, pack_at: 0, pack_len: 0, unpacked: 0, crc: None });
                }
                loop {
                    match r.byte()? {
                        END => break,
                        CODERS_UNPACK_SIZE => {
                            for (f, &outs) in folders.iter_mut().zip(&outs_per_folder) {
                                // The folder's result is its last coder's output (for the
                                // one-coder folders this reader unpacks, its only one).
                                for _ in 0..outs {
                                    f.unpacked = r.number()?;
                                }
                            }
                        }
                        CRC => {
                            let crcs = r.digests(folders.len())?;
                            for (f, c) in folders.iter_mut().zip(crcs) {
                                f.crc = c;
                            }
                        }
                        _ => r.skip_property()?,
                    }
                }
            }
            SUBSTREAMS_INFO => {
                let mut counts = vec![1usize; folders.len()];
                loop {
                    match r.byte()? {
                        END => break,
                        NUM_UNPACK_STREAM => {
                            for c in counts.iter_mut() {
                                *c = r.count()?;
                            }
                        }
                        SIZE => {
                            let mut all = Vec::new();
                            for (f, &c) in folders.iter().zip(&counts) {
                                let mut sizes = Vec::new();
                                let mut sum = 0u64;
                                for _ in 1..c.max(1) {
                                    let s = r.number()?;
                                    sum = sum.checked_add(s).ok_or("a damaged size")?;
                                    sizes.push(s);
                                }
                                if c > 0 {
                                    sizes.push(f.unpacked.checked_sub(sum).ok_or("a damaged size")?);
                                }
                                all.push(sizes);
                            }
                            sub_sizes = Some(all);
                        }
                        CRC => {
                            // A CRC for every file whose folder doesn't already give it.
                            let n: usize = folders
                                .iter()
                                .zip(&counts)
                                .map(|(f, &c)| if c == 1 && f.crc.is_some() { 0 } else { c })
                                .sum();
                            sub_crcs = Some(r.digests(n)?);
                        }
                        _ => r.skip_property()?,
                    }
                }
                per_folder = Some(counts);
            }
            _ => return Err("a damaged streams list".into()),
        }
    }
    // Where each folder's packed bytes are: one packed stream each, one after another.
    let mut at = base.checked_add(pack_pos).ok_or("a damaged pack position")?;
    let mut pi = 0;
    for f in folders.iter_mut() {
        let packed_streams = f.coders.iter().map(|c| c.ins).sum::<u64>() - (f.coders.iter().map(|c| c.outs).sum::<u64>() - 1);
        let len = *pack_sizes.get(pi).ok_or("a folder without its packed bytes")?;
        f.pack_at = at;
        f.pack_len = len;
        for k in 0..packed_streams {
            let s = *pack_sizes.get(pi + k as usize).ok_or("a folder without its packed bytes")?;
            at = at.checked_add(s).ok_or("a damaged pack size")?;
        }
        pi += packed_streams as usize;
    }
    let counts = per_folder.unwrap_or_else(|| vec![1; folders.len()]);
    let sizes = sub_sizes.unwrap_or_else(|| {
        folders.iter().zip(&counts).map(|(f, &c)| if c == 0 { vec![] } else { vec![f.unpacked] }).collect()
    });
    let mut crcs = sub_crcs.unwrap_or_default().into_iter();
    let mut substreams = Vec::new();
    for ((f, &c), s) in folders.iter().zip(&counts).zip(sizes) {
        let with_crc: Vec<(u64, Option<u32>)> = if c == 1 && f.crc.is_some() {
            vec![(s[0], f.crc)]
        } else {
            s.into_iter().map(|size| (size, crcs.next().flatten())).collect()
        };
        substreams.push(with_crc);
    }
    Ok(Streams { folders, substreams })
}

/// Reads the archive's directory.
pub fn list(d: &[u8]) -> Result<Listing, String> {
    if !is_7z(d) {
        return Err("not a 7z archive".into());
    }
    let next_at = u64::from_le_bytes(d[12..20].try_into().expect("eight bytes"));
    let next_len = u64::from_le_bytes(d[20..28].try_into().expect("eight bytes"));
    let next_crc = u32::from_le_bytes(d[28..32].try_into().expect("four bytes"));
    let start = 32u64.checked_add(next_at).ok_or(CUT)?;
    let end = start.checked_add(next_len).ok_or(CUT)?;
    let header = usize::try_from(start)
        .ok()
        .zip(usize::try_from(end).ok())
        .and_then(|(s, e)| d.get(s..e))
        .ok_or("the archive is cut off")?;
    if crate::zip::crc32(header) != next_crc {
        return Err("the archive's header is damaged (its CRC doesn't match)".into());
    }
    let mut bytes: Vec<u8> = header.to_vec();
    // 7-Zip packs the header itself, describing where its packed bytes are with a streams list.
    for _ in 0..4 {
        let mut r = Reader { d: &bytes, at: 0 };
        match r.byte()? {
            HEADER => return read_header(&mut r),
            ENCODED_HEADER => {
                let s = read_streams(&mut r, 32)?;
                let f = s.folders.first().ok_or("an empty packed header")?;
                bytes = unpack(d, f)?;
            }
            _ => return Err("not a 7z header".into()),
        }
    }
    Err("a header packed too many times".into())
}

fn read_header(r: &mut Reader) -> Result<Listing, String> {
    let mut streams = Streams { folders: Vec::new(), substreams: Vec::new() };
    let mut files: Vec<File> = Vec::new();
    loop {
        match r.byte()? {
            END => break,
            ARCHIVE_PROPERTIES => loop {
                if r.byte()? == END {
                    break;
                }
                r.skip_property()?;
            },
            ADDITIONAL_STREAMS => {
                read_streams(r, 32)?;
            }
            MAIN_STREAMS => streams = read_streams(r, 32)?,
            FILES_INFO => {
                let n = r.count()?;
                let mut empty = vec![false; n];
                let mut names = vec![String::new(); n];
                loop {
                    let kind = r.byte()?;
                    if kind == END {
                        break;
                    }
                    let size = r.number()?;
                    let body = r.bytes(size)?;
                    let mut p = Reader { d: body, at: 0 };
                    match kind {
                        EMPTY_STREAM => empty = p.bits(n)?,
                        NAME => {
                            if p.byte()? != 0 {
                                return Err("an archive whose names are stored elsewhere".into());
                            }
                            let units: Vec<u16> = body[1..]
                                .chunks_exact(2)
                                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                                .collect();
                            for (slot, name) in names.iter_mut().zip(units.split(|&u| u == 0)) {
                                *slot = String::from_utf16_lossy(name);
                            }
                        }
                        // Times, attributes, padding (DUMMY) and the rest: not needed.
                        _ => {}
                    }
                }
                // Files with a stream take the folders' substreams in order.
                let mut subs = streams
                    .substreams
                    .iter()
                    .enumerate()
                    .flat_map(|(fi, s)| {
                        let mut off = 0u64;
                        s.iter()
                            .map(move |&(size, crc)| {
                                let at = off;
                                off += size;
                                (fi, at, size, crc)
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
                    .into_iter();
                for (name, is_empty) in names.into_iter().zip(empty) {
                    if is_empty {
                        files.push(File { name, size: 0, crc: Some(0), folder: None, offset: 0 });
                    } else {
                        let (fi, at, size, crc) = subs.next().ok_or("more files than packed streams")?;
                        files.push(File { name, size, crc, folder: Some(fi), offset: at });
                    }
                }
            }
            _ => return Err("a damaged header".into()),
        }
    }
    Ok(Listing { files, folders: streams.folders })
}

impl Listing {
    /// Unpacks folder `i` whole (every file in it, one after another), checking its CRC when the
    /// archive gives one.
    pub fn unpack_folder(&self, d: &[u8], i: usize) -> Result<Vec<u8>, String> {
        let f = self.folders.get(i).ok_or("no such folder in the archive")?;
        unpack(d, f)
    }
}

/// Codec IDs.
const COPY: &[u8] = &[0x00];
const LZMA: &[u8] = &[0x03, 0x01, 0x01];
const LZMA2: &[u8] = &[0x21];

fn unpack(d: &[u8], f: &Folder) -> Result<Vec<u8>, String> {
    if f.coders.len() != 1 {
        return Err("a 7z file packed with a filter this core can't unpack (try LZMA or LZMA2)".into());
    }
    let c = &f.coders[0];
    let packed = usize::try_from(f.pack_at)
        .ok()
        .zip(usize::try_from(f.pack_at.saturating_add(f.pack_len)).ok())
        .and_then(|(s, e)| d.get(s..e))
        .ok_or("the archive is cut off")?;
    let size = usize::try_from(f.unpacked).map_err(|_| "a file too big to unpack")?;
    // No arcade set comes near 512 MB unpacked: a bigger claim is a damaged or hostile archive,
    // not memory to give it.
    if size > 512 << 20 {
        return Err("a file too big to unpack".into());
    }
    let out = match c.id.as_slice() {
        COPY => packed.to_vec(),
        LZMA => {
            if c.props.len() < 5 {
                return Err("damaged LZMA settings".into());
            }
            let mut dec = Lzma::new(c.props[0])?;
            let mut out = Vec::with_capacity(size);
            dec.decode(packed, &mut out, size, 0)?;
            out
        }
        LZMA2 => lzma2(packed, size)?,
        [0x03, 0x04, 0x01] => return Err("a 7z file packed with PPMd, which this core can't unpack (try LZMA or LZMA2)".into()),
        [0x06, 0xF1, 0x07, 0x01] => return Err("an encrypted 7z file".into()),
        _ => return Err("a 7z file packed a way this core can't unpack (try LZMA or LZMA2)".into()),
    };
    if out.len() != size {
        return Err("a 7z file that unpacks to the wrong size".into());
    }
    if let Some(crc) = f.crc {
        if crate::zip::crc32(&out) != crc {
            return Err("a 7z file that is damaged (its CRC doesn't match)".into());
        }
    }
    Ok(out)
}

// ---- LZMA ------------------------------------------------------------------------------------

/// The range decoder: arithmetic decoding of bits, each with an 11-bit probability that adapts.
struct RangeDecoder<'a> {
    d: &'a [u8],
    at: usize,
    range: u32,
    code: u32,
}

const PROB_ONE: u16 = 1 << 11;
const PROB_INIT: u16 = PROB_ONE / 2;

impl<'a> RangeDecoder<'a> {
    fn new(d: &'a [u8]) -> Result<Self, String> {
        if d.len() < 5 || d[0] != 0 {
            return Err("damaged LZMA data".into());
        }
        Ok(RangeDecoder { d, at: 5, range: 0xFFFF_FFFF, code: u32::from_be_bytes([d[1], d[2], d[3], d[4]]) })
    }

    #[inline]
    fn normalize(&mut self) {
        if self.range < 1 << 24 {
            self.range <<= 8;
            // Past the end reads zeros; a stream that needs them is caught by `overrun`.
            let b = self.d.get(self.at).copied().unwrap_or(0);
            self.at += 1;
            self.code = self.code << 8 | u32::from(b);
        }
    }

    fn overrun(&self) -> bool {
        self.at > self.d.len()
    }

    #[inline]
    fn bit(&mut self, p: &mut u16) -> u32 {
        let bound = (self.range >> 11) * u32::from(*p);
        let b = if self.code < bound {
            self.range = bound;
            *p += (PROB_ONE - *p) >> 5;
            0
        } else {
            self.range -= bound;
            self.code -= bound;
            *p -= *p >> 5;
            1
        };
        self.normalize();
        b
    }

    /// Bits at a fixed half probability.
    fn direct(&mut self, n: u32) -> u32 {
        let mut v = 0u32;
        for _ in 0..n {
            self.range >>= 1;
            let b = u32::from(self.code >= self.range);
            if b == 1 {
                self.code -= self.range;
            }
            v = v << 1 | b;
            self.normalize();
        }
        v
    }

    /// `n` bits down a tree of probabilities, the top bit first.
    fn tree(&mut self, probs: &mut [u16], n: u32) -> u32 {
        let mut m = 1usize;
        for _ in 0..n {
            m = m << 1 | self.bit(&mut probs[m]) as usize;
        }
        (m - (1 << n)) as u32
    }

    /// The same, the bottom bit first.
    fn reverse(&mut self, probs: &mut [u16], n: u32) -> u32 {
        let mut m = 1usize;
        let mut v = 0u32;
        for i in 0..n {
            let b = self.bit(&mut probs[m]);
            m = m << 1 | b as usize;
            v |= b << i;
        }
        v
    }
}

/// A match's length: 2-9 from `low`, 10-17 from `mid`, 18-273 from `high`.
#[derive(Clone)]
struct LenDecoder {
    choice: u16,
    choice2: u16,
    low: [[u16; 8]; 16],
    mid: [[u16; 8]; 16],
    high: [u16; 256],
}

impl LenDecoder {
    fn new() -> Self {
        LenDecoder {
            choice: PROB_INIT,
            choice2: PROB_INIT,
            low: [[PROB_INIT; 8]; 16],
            mid: [[PROB_INIT; 8]; 16],
            high: [PROB_INIT; 256],
        }
    }

    fn decode(&mut self, rc: &mut RangeDecoder, pos_state: usize) -> u32 {
        if rc.bit(&mut self.choice) == 0 {
            return rc.tree(&mut self.low[pos_state], 3);
        }
        if rc.bit(&mut self.choice2) == 0 {
            return 8 + rc.tree(&mut self.mid[pos_state], 3);
        }
        16 + rc.tree(&mut self.high, 8)
    }
}

const STATES: usize = 12;
const END_POS_MODEL: u32 = 14;
const FULL_DISTANCES: usize = 128;

/// An LZMA decoder's model: its settings, probabilities, state and last four distances, kept from
/// one LZMA2 chunk to the next unless a chunk resets them.
struct Lzma {
    lc: u32,
    lp: u32,
    pb: u32,
    literal: Vec<u16>,
    is_match: [u16; STATES << 4],
    is_rep: [u16; STATES],
    is_rep_g0: [u16; STATES],
    is_rep_g1: [u16; STATES],
    is_rep_g2: [u16; STATES],
    is_rep0_long: [u16; STATES << 4],
    pos_slot: [[u16; 64]; 4],
    pos_special: [u16; 1 + FULL_DISTANCES - END_POS_MODEL as usize],
    align: [u16; 16],
    len: LenDecoder,
    rep_len: LenDecoder,
    state: usize,
    reps: [u32; 4],
}

impl Lzma {
    /// From the settings byte: (pb * 5 + lp) * 9 + lc.
    fn new(props: u8) -> Result<Lzma, String> {
        let mut l = Lzma {
            lc: 0,
            lp: 0,
            pb: 0,
            literal: Vec::new(),
            is_match: [PROB_INIT; STATES << 4],
            is_rep: [PROB_INIT; STATES],
            is_rep_g0: [PROB_INIT; STATES],
            is_rep_g1: [PROB_INIT; STATES],
            is_rep_g2: [PROB_INIT; STATES],
            is_rep0_long: [PROB_INIT; STATES << 4],
            pos_slot: [[PROB_INIT; 64]; 4],
            pos_special: [PROB_INIT; 1 + FULL_DISTANCES - END_POS_MODEL as usize],
            align: [PROB_INIT; 16],
            len: LenDecoder::new(),
            rep_len: LenDecoder::new(),
            state: 0,
            reps: [0; 4],
        };
        l.set_props(props)?;
        Ok(l)
    }

    fn set_props(&mut self, props: u8) -> Result<(), String> {
        if props >= 9 * 5 * 5 {
            return Err("damaged LZMA settings".into());
        }
        let p = u32::from(props);
        self.lc = p % 9;
        self.lp = p / 9 % 5;
        self.pb = p / 45;
        self.reset();
        Ok(())
    }

    /// Every probability back to a half, the state and distances to the start.
    fn reset(&mut self) {
        self.literal = vec![PROB_INIT; 0x300 << (self.lc + self.lp)];
        self.is_match = [PROB_INIT; STATES << 4];
        self.is_rep = [PROB_INIT; STATES];
        self.is_rep_g0 = [PROB_INIT; STATES];
        self.is_rep_g1 = [PROB_INIT; STATES];
        self.is_rep_g2 = [PROB_INIT; STATES];
        self.is_rep0_long = [PROB_INIT; STATES << 4];
        self.pos_slot = [[PROB_INIT; 64]; 4];
        self.pos_special = [PROB_INIT; 1 + FULL_DISTANCES - END_POS_MODEL as usize];
        self.align = [PROB_INIT; 16];
        self.len = LenDecoder::new();
        self.rep_len = LenDecoder::new();
        self.state = 0;
        self.reps = [0; 4];
    }

    /// Decodes `data` (one range-coded stream) onto `out` until it holds `until` bytes or the
    /// stream's end mark. `dict_start` is where the dictionary began in `out` (LZMA2 can reset
    /// it): no match reaches before it, and positions count from it.
    fn decode(&mut self, data: &[u8], out: &mut Vec<u8>, until: usize, dict_start: usize) -> Result<(), String> {
        let bad = || "damaged LZMA data".to_string();
        let mut rc = RangeDecoder::new(data)?;
        let pb_mask = (1usize << self.pb) - 1;
        let lp_mask = (1usize << self.lp) - 1;
        while out.len() < until {
            let pos = out.len() - dict_start;
            let pos_state = pos & pb_mask;
            let s = self.state;
            if rc.bit(&mut self.is_match[(s << 4) + pos_state]) == 0 {
                let prev = if pos > 0 { out[out.len() - 1] } else { 0 };
                let ctx = ((pos & lp_mask) << self.lc) + (usize::from(prev) >> (8 - self.lc));
                let probs = &mut self.literal[0x300 * ctx..0x300 * ctx + 0x300];
                let mut sym = 1usize;
                if s >= 7 {
                    // After a match: the byte at the last distance guides the first bits.
                    let d = self.reps[0] as usize + 1;
                    if d > pos {
                        return Err(bad());
                    }
                    let mut m = out[out.len() - d];
                    while sym < 0x100 {
                        let mb = usize::from(m >> 7);
                        m <<= 1;
                        let b = rc.bit(&mut probs[((1 + mb) << 8) + sym]) as usize;
                        sym = sym << 1 | b;
                        if mb != b {
                            break;
                        }
                    }
                }
                while sym < 0x100 {
                    sym = sym << 1 | rc.bit(&mut probs[sym]) as usize;
                }
                out.push(sym as u8);
                self.state = if s < 4 { 0 } else if s < 10 { s - 3 } else { s - 6 };
                continue;
            }
            let len;
            if rc.bit(&mut self.is_rep[s]) == 1 {
                if pos == 0 {
                    return Err(bad());
                }
                if rc.bit(&mut self.is_rep_g0[s]) == 0 {
                    if rc.bit(&mut self.is_rep0_long[(s << 4) + pos_state]) == 0 {
                        // One byte from the last distance.
                        self.state = if s < 7 { 9 } else { 11 };
                        let d = self.reps[0] as usize + 1;
                        if d > pos {
                            return Err(bad());
                        }
                        let b = out[out.len() - d];
                        out.push(b);
                        continue;
                    }
                } else {
                    let dist;
                    if rc.bit(&mut self.is_rep_g1[s]) == 0 {
                        dist = self.reps[1];
                    } else {
                        if rc.bit(&mut self.is_rep_g2[s]) == 0 {
                            dist = self.reps[2];
                        } else {
                            dist = self.reps[3];
                            self.reps[3] = self.reps[2];
                        }
                        self.reps[2] = self.reps[1];
                    }
                    self.reps[1] = self.reps[0];
                    self.reps[0] = dist;
                }
                len = self.rep_len.decode(&mut rc, pos_state);
                self.state = if s < 7 { 8 } else { 11 };
            } else {
                self.reps[3] = self.reps[2];
                self.reps[2] = self.reps[1];
                self.reps[1] = self.reps[0];
                len = self.len.decode(&mut rc, pos_state);
                self.state = if s < 7 { 7 } else { 10 };
                let slot = rc.tree(&mut self.pos_slot[len.min(3) as usize], 6);
                let dist = if slot < 4 {
                    slot
                } else {
                    let direct = (slot >> 1) - 1;
                    let base = (2 | (slot & 1)) << direct;
                    if slot < END_POS_MODEL {
                        let off = (base - slot) as usize;
                        base + rc.reverse(&mut self.pos_special[off..], direct)
                    } else {
                        let high = rc.direct(direct - 4) << 4;
                        base.wrapping_add(high).wrapping_add(rc.reverse(&mut self.align, 4))
                    }
                };
                if dist == 0xFFFF_FFFF {
                    // The end mark.
                    break;
                }
                self.reps[0] = dist;
            }
            let n = len as usize + 2;
            let d = self.reps[0] as usize + 1;
            if d > pos {
                return Err(bad());
            }
            for _ in 0..n.min(until - out.len()) {
                let b = out[out.len() - d];
                out.push(b);
            }
        }
        if rc.overrun() {
            return Err("cut-off LZMA data".into());
        }
        Ok(())
    }
}

/// LZMA2: chunks, each either stored or LZMA, which may reset the dictionary, the state, or the
/// settings.
fn lzma2(d: &[u8], size: usize) -> Result<Vec<u8>, String> {
    let bad = || "damaged LZMA2 data".to_string();
    let mut out = Vec::with_capacity(size);
    let mut at = 0usize;
    let mut dec: Option<Lzma> = None;
    let mut dict_start = 0usize;
    loop {
        let control = *d.get(at).ok_or_else(bad)?;
        at += 1;
        match control {
            0x00 => break,
            0x01 | 0x02 => {
                if control == 0x01 {
                    dict_start = out.len();
                }
                let n = usize::from(u16::from_be_bytes(d.get(at..at + 2).ok_or_else(bad)?.try_into().unwrap())) + 1;
                at += 2;
                out.extend_from_slice(d.get(at..at + n).ok_or_else(bad)?);
                at += n;
            }
            0x80..=0xFF => {
                let h = d.get(at..at + 4).ok_or_else(bad)?;
                let unpacked = (usize::from(control & 0x1F) << 16 | usize::from(h[0]) << 8 | usize::from(h[1])) + 1;
                let packed = (usize::from(h[2]) << 8 | usize::from(h[3])) + 1;
                at += 4;
                let mode = control >> 5 & 3;
                if mode == 3 {
                    dict_start = out.len();
                }
                if mode >= 2 {
                    let props = *d.get(at).ok_or_else(bad)?;
                    at += 1;
                    let p = u32::from(props);
                    if p % 9 + p / 9 % 5 > 4 {
                        return Err(bad());
                    }
                    match dec.as_mut() {
                        Some(l) => l.set_props(props)?,
                        None => dec = Some(Lzma::new(props)?),
                    }
                } else if mode == 1 {
                    dec.as_mut().ok_or_else(bad)?.reset();
                }
                let l = dec.as_mut().ok_or_else(bad)?;
                let chunk = d.get(at..at + packed).ok_or_else(bad)?;
                let until = out.len() + unpacked;
                if until > size {
                    return Err(bad());
                }
                l.decode(chunk, &mut out, until, dict_start)?;
                if out.len() != until {
                    return Err(bad());
                }
                at += packed;
            }
            _ => return Err(bad()),
        }
        if out.len() > size {
            return Err(bad());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_as_7z_writes_them() {
        let read = |b: &[u8]| Reader { d: b, at: 0 }.number().unwrap();
        assert_eq!(read(&[0x7F]), 0x7F);
        assert_eq!(read(&[0x80, 0x80]), 0x80);
        assert_eq!(read(&[0xBF, 0xFF]), 0x3FFF);
        assert_eq!(read(&[0xC0, 0x00, 0x40]), 0x4000);
        assert_eq!(read(&[0xFF, 1, 2, 3, 4, 5, 6, 7, 8]), 0x0807_0605_0403_0201);
    }

    #[test]
    fn a_damaged_archive_is_refused_not_a_panic() {
        let mut d = SIGNATURE.to_vec();
        d.extend([0, 4]);
        d.extend([0u8; 24]);
        assert!(list(&d).is_err());
        // A header claiming to be past the end.
        d[12] = 0xFF;
        assert!(list(&d).is_err());
        // Random bytes after a valid signature.
        let mut x = 0x1234_5678u32;
        for _ in 0..300 {
            let mut v = SIGNATURE.to_vec();
            v.extend([0, 4]);
            for _ in 0..200 {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                v.push(x as u8);
            }
            let _ = list(&v);
        }
    }
}
