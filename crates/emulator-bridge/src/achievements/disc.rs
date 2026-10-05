//! Reading CHD and CSO disc images for RetroAchievements' hashing.
//!
//! rcheevos identifies a disc game by hashing files inside it (PlayStation: `SYSTEM.CNF` and the
//! boot executable; Dreamcast: `IP.BIN` and the boot file on track 3; PSP: `PARAM.SFO` and
//! `EBOOT.BIN`). It knows how to walk the disc but not how to open every container: its built-in
//! reader handles `.cue`/`.bin`, `.gdi` and plain `.iso`, and leaves the rest to the frontend
//! through a `rc_hash_cdreader` (rc_hash.h line 83). RetroArch supplies one for CHD
//! (`cheevos.c`, `rc_hash_handle_chd_open_track`). This is Continuum's, for CHD and CSO; anything
//! else goes on to rcheevos' own reader, see `cdreader.rs`.
//!
//! The shape: each container is turned into a [`TrackSource`], a flat run of one track's sectors
//! exactly as a `.bin` file would hold them, and a [`Track`] over that runs rcheevos' own `.bin`
//! logic (`cdreader_determine_sector_size` and `cdreader_read_sector` in rhash/cdreader.c), ported
//! line for line. So a CHD track is read the way the `.bin` it was made from would have been, and
//! the absolute sector numbers rcheevos asks for come out of the sector headers the same way.
//!
//! - **CHD**: through the `chd` crate already used by import (every CD codec). Tracks come from the
//!   `CHT2`, `CHTR` or `CHGD` metadata in order, each padded to four frames, as RetroArch's
//!   `chd_stream.c` counts them; a `DVD ` CHD (PSP, PS2) is one 2048-byte track.
//! - **CSO** (compressed ISO, PSP): the `CISO` block index, each block raw deflate or stored.
//!   Version 2 blocks compressed with LZ4 are refused with a sentence, as is ZSO.
//!
//! Not here: PlayStation `.pbp` (a PSP-made PS1 eboot). rcheevos only hashes `.pbp` as a PSP file
//! (the whole file, `rc_hash_psp`); a PS1 eboot is an encrypted, compressed image rcheevos cannot
//! walk and RetroArch does not hash either.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// One track's sectors laid end to end, like a `.bin` file.
pub trait TrackSource: Send {
    fn len(&self) -> u64;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Reads up to `out.len()` bytes at `offset`; returns how many were read (0 past the end).
    fn read_at(&mut self, offset: u64, out: &mut [u8]) -> usize;
}

/// `RC_HASH_CDTRACK_FIRST_DATA` and friends, rc_hash.h lines 62 to 65.
pub const TRACK_FIRST_DATA: u32 = u32::MAX;
pub const TRACK_LAST: u32 = u32::MAX - 1;
pub const TRACK_LARGEST: u32 = u32::MAX - 2;
pub const TRACK_FIRST_OF_SECOND_SESSION: u32 = u32::MAX - 3;

/// A track with its sector layout worked out. A port of `rc_hash_cdrom_track_t`'s reading half.
pub struct Track {
    source: Box<dyn TrackSource>,
    sector_size: u64,
    header_size: u64,
    first_sector: i32,
}

const SYNC: [u8; 12] = [
    0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00,
];

/// `cdreader_get_sector`: a raw sector header's MSF as a sector index, minus the 150-sector lead-in.
fn header_sector(header: &[u8; 32]) -> i32 {
    let bcd = |b: u8| i32::from(b >> 4) * 10 + i32::from(b & 0x0F);
    ((bcd(header[12]) * 60) + bcd(header[13])) * 75 + bcd(header[14]) - 150
}

impl Track {
    /// `cdreader_determine_sector_size`, then the size-based fallback of `cdreader_open_bin_track`.
    pub fn new(mut source: Box<dyn TrackSource>) -> Option<Self> {
        const TOC_SECTOR: u64 = 16;
        let mut header = [0u8; 32];
        let at = |source: &mut Box<dyn TrackSource>, size: u64, header: &mut [u8; 32]| {
            *header = [0; 32];
            source.read_at(TOC_SECTOR * size, header) == header.len()
        };
        let mut layout = None;
        if at(&mut source, 2352, &mut header) && header[..12] == SYNC {
            let head = if &header[25..30] == b"CD001" { 24 } else { 16 };
            layout = Some((2352, head, header_sector(&header) - TOC_SECTOR as i32));
        } else if at(&mut source, 2336, &mut header) && header[..12] == SYNC {
            let head = if &header[25..30] == b"CD001" { 24 } else { 16 };
            layout = Some((2336, head, header_sector(&header) - TOC_SECTOR as i32));
        } else if at(&mut source, 2048, &mut header) && &header[1..6] == b"CD001" {
            layout = Some((2048, 0, 0));
        }
        let (sector_size, header_size, first_sector) = match layout {
            Some(found) => found,
            None => {
                let size = source.len();
                if size % 2352 == 0 {
                    (2352, 24, 0)
                } else if size % 2048 == 0 {
                    (2048, 0, 0)
                } else if size % 2336 == 0 {
                    (2336, 8, 0)
                } else {
                    return None;
                }
            }
        };
        Some(Self {
            source,
            sector_size,
            header_size,
            first_sector,
        })
    }

    /// `cdreader_first_track_sector` (no pregap: a source already starts at the track's data).
    pub fn first_sector(&self) -> u32 {
        self.first_sector as u32
    }

    /// `cdreader_read_sector`: `requested` bytes of user data from absolute `sector` onward,
    /// 2048 bytes per sector.
    pub fn read_sector(&mut self, sector: u32, out: &mut [u8]) -> usize {
        const RAW_DATA: usize = 2048;
        if sector < self.first_sector as u32 {
            return 0;
        }
        let mut position =
            u64::from(sector - self.first_sector as u32) * self.sector_size + self.header_size;
        let mut total = 0usize;
        let mut remaining = out.len();
        while remaining > RAW_DATA {
            let read = self
                .source
                .read_at(position, &mut out[total..total + RAW_DATA]);
            total += read;
            if read < RAW_DATA {
                return total;
            }
            position += self.sector_size;
            remaining -= RAW_DATA;
        }
        total
            + self
                .source
                .read_at(position, &mut out[total..total + remaining])
    }
}

/// Whether this reader, rather than rcheevos' own, should open `path`.
pub fn handles(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".chd") || lower.ends_with(".cso")
}

/// Opens one track of a CHD or CSO. `None` means rcheevos is told the track could not be opened,
/// and the reason is logged.
pub fn open(path: &str, track: u32) -> Option<Track> {
    let lower = path.to_ascii_lowercase();
    let source: Result<Box<dyn TrackSource>, String> = if lower.ends_with(".chd") {
        chd_track(Path::new(path), track).map(|t| Box::new(t) as Box<dyn TrackSource>)
    } else if lower.ends_with(".cso") {
        if track > 1 && track < TRACK_FIRST_OF_SECOND_SESSION {
            Err("a CSO is one data track".into())
        } else if track == TRACK_FIRST_OF_SECOND_SESSION {
            Err("a CSO has no second session".into())
        } else {
            Cso::open(Path::new(path)).map(|c| Box::new(c) as Box<dyn TrackSource>)
        }
    } else {
        Err("not a CHD or CSO".into())
    };
    match source {
        Ok(source) => {
            let track_handle = Track::new(source);
            if track_handle.is_none() {
                log::warn!("achievements: {path}: could not work out the sector size");
            }
            track_handle
        }
        Err(why) => {
            log::warn!("achievements: {path} track {track}: {why}");
            None
        }
    }
}

// ---------------------------------------------------------------------------------------- CHD

/// One track out of a CHD's metadata, as `chdstream_get_meta` reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChdTrackMeta {
    pub number: u32,
    pub kind: String,
    pub frames: u32,
    pub pregap: u32,
    pub pregap_stored: bool,
}

/// Parses `TRACK:1 TYPE:MODE2_RAW SUBTYPE:NONE FRAMES:1234 PREGAP:150 PGTYPE:VMODE2_RAW ...`, the
/// text of a `CHT2`, `CHTR` or `CHGD` entry.
pub fn parse_track(text: &str) -> Option<ChdTrackMeta> {
    let mut track = ChdTrackMeta {
        number: 0,
        kind: String::new(),
        frames: 0,
        pregap: 0,
        pregap_stored: false,
    };
    for field in text.trim_end_matches('\0').split_whitespace() {
        let Some((key, value)) = field.split_once(':') else {
            continue;
        };
        match key {
            "TRACK" => track.number = value.parse().ok()?,
            "TYPE" => track.kind = value.to_string(),
            "FRAMES" => track.frames = value.parse().ok()?,
            "PREGAP" => track.pregap = value.parse().unwrap_or(0),
            "PGTYPE" => track.pregap_stored = value.starts_with('V'),
            _ => {}
        }
    }
    (track.number > 0).then_some(track)
}

/// Where one track sits in a CHD: its first frame (after a stored pregap), its frame count, and
/// how many bytes of each frame are sector data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackPlace {
    pub first_frame: u64,
    pub frames: u64,
    pub frame_size: u32,
}

/// Frames are padded to a multiple of four per track (`padding_frames`, TRACK_PAD).
fn padded(frames: u32) -> u64 {
    u64::from(frames).div_ceil(4) * 4
}

/// Bytes of sector data in each frame of a track type, as `chdstream_open` sizes them, with the
/// MODE2 forms given their real sizes.
fn frame_size(kind: &str, unit_bytes: u32) -> u32 {
    match kind {
        "MODE1_RAW" | "MODE2_RAW" | "AUDIO" => 2352,
        "MODE1" | "MODE2_FORM1" => 2048,
        "MODE2" | "MODE2_FORM_MIX" => 2336,
        "MODE2_FORM2" => 2324,
        _ => unit_bytes.min(2352),
    }
}

/// Picks the track rcheevos asked for out of the list, the way `chdstream_find_track` does, and
/// says where it is. `tracks` are in metadata order.
pub fn place_track(tracks: &[ChdTrackMeta], wanted: u32, unit_bytes: u32) -> Option<TrackPlace> {
    let index = match wanted {
        TRACK_FIRST_DATA => tracks.iter().position(|t| t.kind != "AUDIO")?,
        TRACK_LAST => tracks.len().checked_sub(1)?,
        TRACK_LARGEST => tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind != "AUDIO")
            .max_by(|a, b| a.1.frames.cmp(&b.1.frames).then(b.0.cmp(&a.0)))
            .map(|(i, _)| i)?,
        // RetroArch's CHD reader has no session information either.
        TRACK_FIRST_OF_SECOND_SESSION => return None,
        number => tracks.iter().position(|t| t.number == number)?,
    };
    let offset: u64 = tracks[..index].iter().map(|t| padded(t.frames)).sum();
    let track = &tracks[index];
    let skip = if track.pregap_stored {
        u64::from(track.pregap.min(track.frames))
    } else {
        0
    };
    Some(TrackPlace {
        first_frame: offset + skip,
        frames: u64::from(track.frames) - skip,
        frame_size: frame_size(&track.kind, unit_bytes),
    })
}

const fn tag(b: &[u8; 4]) -> u32 {
    ((b[0] as u32) << 24) | ((b[1] as u32) << 16) | ((b[2] as u32) << 8) | b[3] as u32
}

struct ChdSource {
    chd: chd::Chd<File>,
    place: TrackPlace,
    unit_bytes: u32,
    hunk_bytes: u32,
    cached: Option<(u32, Vec<u8>)>,
    compressed: Vec<u8>,
}

/// Hunks bigger than this are refused. A CD CHD's hunk is 19,584 bytes (eight 2,448-byte frames)
/// and a DVD one a few KB. Both the `chd` crate and [`ChdSource::hunk`] allocate a whole hunk up
/// front, so without a ceiling a broken header could ask for 4 GiB at once. The crate already
/// refuses 16 MiB and over in v1 to v4 headers; it does not check v5 headers at all.
const MAX_CHD_HUNK: u32 = 16 << 20;

/// Hunks a CHD may have: a 16 GiB disc in 2 KB hunks, which is twice a dual-layer DVD cut into
/// single sectors. The crate expands the whole hunk map into memory at open, 12 bytes a hunk for a
/// compressed CHD, and that map is RLE-coded, so its stored size says nothing about how big it
/// unpacks to. This caps the unpacked map at 96 MiB.
const MAX_CHD_HUNKS: u64 = 1 << 23;

/// Metadata entries read before giving up. A CD has at most 99 tracks, one entry each, plus a few
/// others. Each entry names the next by offset, so a corrupt one can point back at itself, and
/// collecting that chain would grow without end.
const MAX_CHD_METADATA: usize = 1024;

/// Checks a CHD header against the real file before the `chd` crate opens it.
///
/// The crate validates v1 to v4 headers and reads their maps entry by entry, so those stop at the
/// end of the file on their own. A v5 header is taken on trust: opening allocates the whole hunk
/// map from the header's sizes and a hunk-sized buffer for each codec, before anything is read. So
/// those sizes are checked here first, and a file that lies about them is refused with a reason
/// instead of being handed the allocation it asked for. The metadata chain is checked for every
/// version that has one; see [`check_chd_metadata`]. Anything else is left for the crate to judge.
fn check_chd_header<R: Read + Seek>(reader: &mut R) -> Result<(), String> {
    let file_len = reader.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
    let mut header = [0u8; 64];
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|e| e.to_string())?;
    if reader.read_exact(&mut header).is_err() {
        // Too short for a v3 to v5 header; the crate says why.
        return Ok(());
    }
    let be32 = |at: usize| u32::from_be_bytes(header[at..at + 4].try_into().unwrap_or_default());
    let be64 = |at: usize| u64::from_be_bytes(header[at..at + 8].try_into().unwrap_or_default());
    if &header[..8] != b"MComprHD" {
        return Ok(());
    }
    // Where the first metadata entry is (chd.h): offset 36 in a v3 or v4 header, 48 in a v5 one.
    // Versions 1 and 2 have no metadata.
    let version = be32(12);
    let first_metadata = match version {
        3 | 4 => be64(36),
        5 => be64(48),
        _ => return Ok(()),
    };
    check_chd_metadata(reader, first_metadata)?;
    if version != 5 {
        return Ok(());
    }
    // The rest of the v5 layout: compressors at 16, logical size at 32, map offset at 40, hunk
    // size at 56.
    let uncompressed = be32(16) == 0;
    let logical_bytes = be64(32);
    let map_offset = be64(40);
    let hunk_bytes = be32(56);
    if hunk_bytes > MAX_CHD_HUNK {
        return Err(format!(
            "the CHD header asks for {hunk_bytes}-byte hunks, more than any disc uses"
        ));
    }
    if hunk_bytes == 0 {
        // Refused by the crate, which also guards its own division.
        return Ok(());
    }
    let hunks = logical_bytes.div_ceil(u64::from(hunk_bytes));
    if hunks > MAX_CHD_HUNKS {
        return Err("the CHD says it is larger than any disc".into());
    }
    // The hunk itself is only capped, not held to the file's length: a compressed hunk unpacks to
    // more than it stores, and an uncompressed CHD may leave an all-zero hunk out entirely. The
    // map, though, is read straight out of the file, so it has to be there.
    let past_the_end = || "the CHD's hunk map runs past the end of the file".to_string();
    let map_end = if uncompressed {
        // Stored as is, four bytes a hunk.
        map_offset.checked_add(hunks * 4)
    } else {
        // A 16-byte header whose first field is how many bytes of coded map follow it. The crate
        // allocates that many before reading them.
        if map_offset.checked_add(16).is_none_or(|end| end > file_len) {
            return Err(past_the_end());
        }
        let mut stored = [0u8; 4];
        reader
            .seek(SeekFrom::Start(map_offset))
            .and_then(|_| reader.read_exact(&mut stored))
            .map_err(|_| past_the_end())?;
        map_offset
            .checked_add(16)
            .and_then(|start| start.checked_add(u64::from(u32::from_be_bytes(stored))))
    };
    if map_end.is_none_or(|end| end > file_len) {
        return Err(past_the_end());
    }
    Ok(())
}

/// Follows the metadata chain from `first`, reading only each entry's 16-byte header, and refuses
/// one that does not end within [`MAX_CHD_METADATA`] entries.
///
/// Needed before the crate opens the file, not just in `chd_track`: for a v3 or v4 file the crate
/// walks this chain itself while opening (to guess the sector size) and collects every entry, so
/// a chain that points back at itself would grow that list until the app was killed. An entry
/// that cannot be read is where the crate stops too, so that is the end of the chain, not a loop.
fn check_chd_metadata<R: Read + Seek>(reader: &mut R, first: u64) -> Result<(), String> {
    let mut at = first;
    for _ in 0..MAX_CHD_METADATA {
        if at == 0 {
            return Ok(());
        }
        let mut entry = [0u8; 16];
        let read = reader
            .seek(SeekFrom::Start(at))
            .and_then(|_| reader.read_exact(&mut entry));
        if read.is_err() {
            return Ok(());
        }
        // Tag (4 bytes), flags and length (4), then the next entry's offset, 0 for none.
        at = u64::from_be_bytes(entry[8..16].try_into().unwrap_or_default());
    }
    if at == 0 {
        Ok(())
    } else {
        Err("the CHD's metadata list never ends".into())
    }
}

fn chd_track(path: &Path, wanted: u32) -> Result<ChdSource, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    check_chd_header(&mut file)?;
    let mut chd = chd::Chd::open(file, None).map_err(|e| format!("not a readable CHD: {e}"))?;
    if chd.header().has_parent() {
        return Err("this CHD needs its parent CHD".into());
    }
    let unit_bytes = chd.header().unit_bytes();
    let hunk_bytes = chd.header().hunk_size();
    if unit_bytes == 0 || hunk_bytes == 0 || hunk_bytes < unit_bytes {
        return Err("the CHD header has no usable sector size".into());
    }
    // The upper bound again, for every header version, because `ChdSource::hunk` allocates
    // `hunk_bytes` for each hunk it reads and this is the one place a `ChdSource` is made.
    if hunk_bytes > MAX_CHD_HUNK {
        return Err(format!(
            "the CHD header asks for {hunk_bytes}-byte hunks, more than any disc uses"
        ));
    }
    // Bounded here as well as in `check_chd_metadata`, because this is the list actually held, and
    // a bound that lived only in a separate walk would be one edit away from not applying.
    let refs: Vec<_> = chd.metadata_refs().take(MAX_CHD_METADATA + 1).collect();
    if refs.len() > MAX_CHD_METADATA {
        return Err("the CHD's metadata list never ends".into());
    }
    let mut by_tag: [Vec<ChdTrackMeta>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    let mut is_dvd = false;
    for r in refs {
        let t = chd::metadata::MetadataTag::metatag(&r);
        let slot = if t == tag(b"CHT2") {
            0
        } else if t == tag(b"CHTR") {
            1
        } else if t == tag(b"CHGD") {
            2
        } else {
            if t == tag(b"DVD ") {
                is_dvd = true;
            }
            continue;
        };
        if let Ok(meta) = r.read(chd.inner()) {
            if let Some(track) = parse_track(&String::from_utf8_lossy(&meta.value)) {
                by_tag[slot].push(track);
            }
        }
    }
    let place = if let Some(tracks) = by_tag.iter().find(|list| !list.is_empty()) {
        place_track(tracks, wanted, unit_bytes).ok_or("the CHD has no such track")?
    } else if is_dvd || unit_bytes == 2048 {
        if !matches!(wanted, 1 | TRACK_FIRST_DATA | TRACK_LAST | TRACK_LARGEST) {
            return Err("a DVD CHD is one track".into());
        }
        let total = chd.header().logical_bytes() / u64::from(unit_bytes);
        TrackPlace {
            first_frame: 0,
            frames: total,
            frame_size: unit_bytes,
        }
    } else {
        return Err("the CHD is neither a CD, a GD-ROM nor a DVD".into());
    };
    Ok(ChdSource {
        chd,
        place,
        unit_bytes,
        hunk_bytes,
        cached: None,
        compressed: Vec::new(),
    })
}

impl ChdSource {
    /// One hunk, decoded. The buffer is `hunk_bytes` long because the crate insists on exactly
    /// that; `chd_track`, the only constructor, has already held it to [`MAX_CHD_HUNK`].
    fn hunk(&mut self, number: u32) -> Option<&[u8]> {
        if self.cached.as_ref().map(|c| c.0) != Some(number) {
            let mut out = vec![0u8; self.hunk_bytes as usize];
            let mut hunk = self.chd.hunk(number).ok()?;
            hunk.read_hunk_in(&mut self.compressed, &mut out).ok()?;
            self.cached = Some((number, out));
        }
        self.cached.as_ref().map(|c| c.1.as_slice())
    }
}

impl TrackSource for ChdSource {
    fn len(&self) -> u64 {
        self.place.frames * u64::from(self.place.frame_size)
    }

    fn read_at(&mut self, offset: u64, out: &mut [u8]) -> usize {
        let frame_size = u64::from(self.place.frame_size);
        let frames_per_hunk = u64::from(self.hunk_bytes / self.unit_bytes);
        let end = (offset + out.len() as u64).min(self.len());
        let mut at = offset;
        let mut written = 0usize;
        while at < end {
            let frame = at / frame_size;
            let within = (at % frame_size) as usize;
            let amount = ((frame_size as usize - within) as u64).min(end - at) as usize;
            let chd_frame = self.place.first_frame + frame;
            let hunk_number = (chd_frame / frames_per_hunk) as u32;
            let hunk_offset = ((chd_frame % frames_per_hunk) * u64::from(self.unit_bytes)) as usize;
            let Some(hunk) = self.hunk(hunk_number) else {
                break;
            };
            let Some(bytes) = hunk.get(hunk_offset + within..hunk_offset + within + amount) else {
                break;
            };
            out[written..written + amount].copy_from_slice(bytes);
            written += amount;
            at += amount as u64;
        }
        written
    }
}

// ---------------------------------------------------------------------------------------- CSO

/// A `CISO` compressed ISO.
pub struct Cso<R> {
    reader: R,
    total: u64,
    block_size: u32,
    align: u8,
    version: u8,
    index: Vec<u32>,
    cached: Option<(u32, Vec<u8>)>,
}

impl Cso<File> {
    pub fn open(path: &Path) -> Result<Self, String> {
        Self::from_reader(File::open(path).map_err(|e| e.to_string())?)
    }
}

/// Blocks bigger than this are refused: the index would otherwise let a broken file ask for any
/// allocation it likes. Real CSOs use 2 KB blocks; some tools write up to 128 KB.
const MAX_CSO_BLOCK: u32 = 1 << 20;

impl<R: Read + Seek> Cso<R> {
    pub fn from_reader(mut reader: R) -> Result<Self, String> {
        let mut header = [0u8; 24];
        reader
            .read_exact(&mut header)
            .map_err(|_| "too short to be a CSO".to_string())?;
        if &header[..4] == b"ZISO" {
            return Err(
                "this is a ZSO (LZ4), which is not supported; use a CSO, CHD or ISO".into(),
            );
        }
        if &header[..4] != b"CISO" {
            return Err("not a CSO (no CISO header)".into());
        }
        let header_size = u32::from_le_bytes(header[4..8].try_into().unwrap_or_default());
        let total = u64::from_le_bytes(header[8..16].try_into().unwrap_or_default());
        let block_size = u32::from_le_bytes(header[16..20].try_into().unwrap_or_default());
        let version = header[20];
        let align = header[21];
        if block_size == 0 || block_size > MAX_CSO_BLOCK || align > 31 {
            return Err(format!(
                "the CSO header is not sane (block size {block_size})"
            ));
        }
        let blocks = total.div_ceil(u64::from(block_size));
        if blocks > (1 << 26) {
            return Err("the CSO says it is larger than any disc".into());
        }
        // Version 1 files often store 0 here; the index always follows the 24-byte header then.
        let index_at = if header_size >= 24 {
            u64::from(header_size)
        } else {
            24
        };
        // The index is stored in the file, four bytes a block, so it can never be longer than the
        // file is. Checked against the real length BEFORE allocating: the block cap above still
        // allows a 256 MiB index, and a corrupt header claiming that much used to have the whole
        // buffer allocated and only then found the file was a few bytes long. On a phone that
        // allocation alone can get the app killed.
        let index_bytes = (blocks + 1) * 4;
        let file_len = reader
            .seek(SeekFrom::End(0))
            .map_err(|e| e.to_string())?;
        if index_at
            .checked_add(index_bytes)
            .is_none_or(|end| end > file_len)
        {
            return Err("the CSO's block index runs past the end of the file".into());
        }
        reader
            .seek(SeekFrom::Start(index_at))
            .map_err(|e| e.to_string())?;
        let mut raw = vec![0u8; index_bytes as usize];
        reader
            .read_exact(&mut raw)
            .map_err(|_| "the CSO's block index is cut short".to_string())?;
        let index = raw
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        Ok(Self {
            reader,
            total,
            block_size,
            align,
            version,
            index,
            cached: None,
        })
    }

    fn block(&mut self, number: u32) -> Option<&[u8]> {
        if self.cached.as_ref().map(|c| c.0) != Some(number) {
            let entry = *self.index.get(number as usize)?;
            let next = *self.index.get(number as usize + 1)?;
            let flag = entry & 0x8000_0000 != 0;
            let start = u64::from(entry & 0x7FFF_FFFF) << self.align;
            let stop = u64::from(next & 0x7FFF_FFFF) << self.align;
            let stored = stop.checked_sub(start)?.min(u64::from(MAX_CSO_BLOCK) * 2) as usize;
            let mut data = vec![0u8; stored];
            self.reader.seek(SeekFrom::Start(start)).ok()?;
            let mut got = 0;
            while got < stored {
                match self.reader.read(&mut data[got..]) {
                    Ok(0) => break,
                    Ok(n) => got += n,
                    Err(_) => return None,
                }
            }
            data.truncate(got);
            let block_size = self.block_size as usize;
            // Version 1: the high bit means stored. Version 2: a block as big as the block size is
            // stored, and the high bit means LZ4 rather than deflate.
            let plain = if self.version >= 2 {
                if !flag && data.len() >= block_size {
                    true
                } else if flag {
                    log::warn!(
                        "achievements: CSO v2 block {number} is LZ4, which is not supported"
                    );
                    return None;
                } else {
                    false
                }
            } else {
                flag
            };
            let block = if plain {
                data.truncate(block_size);
                data
            } else {
                miniz_oxide::inflate::decompress_to_vec_with_limit(&data, block_size).ok()?
            };
            self.cached = Some((number, block));
        }
        self.cached.as_ref().map(|c| c.1.as_slice())
    }
}

impl<R: Read + Seek + Send> TrackSource for Cso<R> {
    fn len(&self) -> u64 {
        self.total
    }

    fn read_at(&mut self, offset: u64, out: &mut [u8]) -> usize {
        let block_size = u64::from(self.block_size);
        let end = (offset + out.len() as u64).min(self.total);
        let mut at = offset;
        let mut written = 0usize;
        while at < end {
            let number = (at / block_size) as u32;
            let within = (at % block_size) as usize;
            let Some(block) = self.block(number) else {
                break;
            };
            let amount = (block.len().saturating_sub(within) as u64).min(end - at) as usize;
            if amount == 0 {
                break;
            }
            out[written..written + amount].copy_from_slice(&block[within..within + amount]);
            written += amount;
            at += amount as u64;
        }
        written
    }
}

#[cfg(test)]
pub mod tests_support {
    /// Builds a CSO v1 of `iso` with `block` byte blocks, deflating every other block.
    pub fn cso_of(iso: &[u8], block: u32, version: u8) -> Vec<u8> {
        let blocks = iso.len().div_ceil(block as usize);
        let mut out = Vec::new();
        out.extend_from_slice(b"CISO");
        out.extend_from_slice(&24u32.to_le_bytes());
        out.extend_from_slice(&(iso.len() as u64).to_le_bytes());
        out.extend_from_slice(&block.to_le_bytes());
        out.push(version);
        out.push(0);
        out.extend_from_slice(&[0, 0]);
        let index_at = out.len();
        out.resize(index_at + (blocks + 1) * 4, 0);
        let mut entries = Vec::new();
        for (i, chunk) in iso.chunks(block as usize).enumerate() {
            let position = out.len() as u32;
            if i % 2 == 0 {
                entries.push(position);
                out.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(chunk, 6));
            } else {
                // Stored: v1 flags it; v2 says so by size alone.
                entries.push(if version >= 2 {
                    position
                } else {
                    position | 0x8000_0000
                });
                out.extend_from_slice(chunk);
            }
        }
        entries.push(out.len() as u32);
        for (i, e) in entries.iter().enumerate() {
            out[index_at + i * 4..index_at + i * 4 + 4].copy_from_slice(&e.to_le_bytes());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A plain in-memory track, standing in for a `.bin`.
    struct Bytes(Vec<u8>);

    impl TrackSource for Bytes {
        fn len(&self) -> u64 {
            self.0.len() as u64
        }
        fn read_at(&mut self, offset: u64, out: &mut [u8]) -> usize {
            let start = (offset as usize).min(self.0.len());
            let n = out.len().min(self.0.len() - start);
            out[..n].copy_from_slice(&self.0[start..start + n]);
            n
        }
    }

    fn bcd(n: u32) -> u8 {
        ((n / 10) << 4 | (n % 10)) as u8
    }

    /// A MODE1/2352 track whose first sector is absolute sector `lba`, each sector's user data
    /// filled with its own absolute number, and an ISO "CD001" at sector 16.
    fn raw_track(lba: u32, sectors: u32) -> Vec<u8> {
        let mut out = Vec::new();
        for i in 0..sectors {
            let absolute = lba + i + 150;
            let mut sector = vec![0u8; 2352];
            sector[..12].copy_from_slice(&SYNC);
            sector[12] = bcd(absolute / 75 / 60);
            sector[13] = bcd(absolute / 75 % 60);
            sector[14] = bcd(absolute % 75);
            sector[15] = 1;
            for b in &mut sector[16..16 + 2048] {
                *b = (lba + i) as u8;
            }
            if i == 16 {
                sector[16..22].copy_from_slice(b"\x01CD001");
            }
            out.extend_from_slice(&sector);
        }
        out
    }

    #[test]
    fn a_raw_track_reports_its_absolute_first_sector_and_reads_user_data() {
        let mut track = Track::new(Box::new(Bytes(raw_track(45000, 20)))).unwrap();
        assert_eq!(track.first_sector(), 45000);
        let mut out = vec![0u8; 4096];
        assert_eq!(track.read_sector(45002, &mut out), 4096);
        assert!(out[..2048].iter().all(|&b| b == (45002u32 as u8)));
        assert!(out[2048..].iter().all(|&b| b == (45003u32 as u8)));
        // Before the track: nothing, as rcheevos' reader says.
        assert_eq!(track.read_sector(44999, &mut out), 0);
    }

    #[test]
    fn a_cooked_iso_is_2048_byte_sectors_from_zero() {
        let mut iso = vec![0u8; 2048 * 20];
        iso[16 * 2048..16 * 2048 + 6].copy_from_slice(b"\x01CD001");
        iso[17 * 2048] = 0x77;
        let mut track = Track::new(Box::new(Bytes(iso))).unwrap();
        assert_eq!(track.first_sector(), 0);
        let mut out = [0u8; 1];
        assert_eq!(track.read_sector(17, &mut out), 1);
        assert_eq!(out[0], 0x77);
        assert!(Track::new(Box::new(Bytes(vec![0u8; 1001]))).is_none());
    }

    #[test]
    fn chd_metadata_parses_and_places_tracks_like_retroarch() {
        let t1 = parse_track("TRACK:1 TYPE:MODE2_RAW SUBTYPE:NONE FRAMES:1001 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0").unwrap();
        let t2 = parse_track("TRACK:2 TYPE:AUDIO SUBTYPE:NONE FRAMES:1500 PREGAP:150 PGTYPE:VAUDIO PGSUB:NONE POSTGAP:0").unwrap();
        let t3 = parse_track("TRACK:3 TYPE:MODE1_RAW SUBTYPE:NONE FRAMES:2000").unwrap();
        assert_eq!((t1.number, t1.frames, t1.pregap_stored), (1, 1001, false));
        assert!(t2.pregap_stored);
        let tracks = vec![t1, t2, t3];
        // Track 3 starts after 1001 frames padded to 1004 and 1500 (already a multiple of four).
        let third = place_track(&tracks, 3, 2448).unwrap();
        assert_eq!(third.first_frame, 1004 + 1500);
        assert_eq!(third.frame_size, 2352);
        // Track 2's stored pregap is skipped, so its data starts 150 frames in.
        let second = place_track(&tracks, 2, 2448).unwrap();
        assert_eq!((second.first_frame, second.frames), (1004 + 150, 1350));
        assert_eq!(
            place_track(&tracks, TRACK_FIRST_DATA, 2448)
                .unwrap()
                .first_frame,
            0
        );
        assert_eq!(place_track(&tracks, TRACK_LAST, 2448), Some(third));
        assert_eq!(place_track(&tracks, TRACK_LARGEST, 2448), Some(third));
        assert_eq!(
            place_track(&tracks, TRACK_FIRST_OF_SECOND_SESSION, 2448),
            None
        );
        assert_eq!(place_track(&tracks, 9, 2448), None);
        assert!(parse_track("TYPE:AUDIO").is_none());
    }

    fn sample_iso() -> Vec<u8> {
        let mut iso: Vec<u8> = (0..2048 * 24)
            .map(|i| (i / 2048) as u8 ^ (i % 7) as u8)
            .collect();
        iso[16 * 2048..16 * 2048 + 6].copy_from_slice(b"\x01CD001");
        iso
    }

    #[test]
    fn a_cso_reads_back_byte_for_byte_and_walks_like_an_iso() {
        let iso = sample_iso();
        for version in [1, 2] {
            let mut cso =
                Cso::from_reader(Cursor::new(tests_support::cso_of(&iso, 2048, version))).unwrap();
            assert_eq!(cso.len(), iso.len() as u64);
            let mut all = vec![0u8; iso.len()];
            assert_eq!(cso.read_at(0, &mut all), iso.len());
            assert_eq!(all, iso, "version {version}");
            // A read straddling a deflated and a stored block.
            let mut mid = vec![0u8; 3000];
            assert_eq!(cso.read_at(2048 * 3 + 100, &mut mid), 3000);
            assert_eq!(mid, iso[2048 * 3 + 100..2048 * 3 + 3100]);
            let mut track = Track::new(Box::new(cso)).unwrap();
            assert_eq!(track.first_sector(), 0);
            let mut sector = vec![0u8; 2048];
            assert_eq!(track.read_sector(20, &mut sector), 2048);
            assert_eq!(sector, iso[20 * 2048..21 * 2048]);
        }
    }

    #[test]
    fn bad_csos_are_refused_with_a_reason() {
        assert!(Cso::from_reader(Cursor::new(b"nope".to_vec())).is_err());
        let mut zso = tests_support::cso_of(&sample_iso(), 2048, 1);
        zso[..4].copy_from_slice(b"ZISO");
        assert!(Cso::from_reader(Cursor::new(zso))
            .err()
            .unwrap()
            .contains("ZSO"));
        let mut huge = tests_support::cso_of(&sample_iso(), 2048, 1);
        huge[16..20].copy_from_slice(&0u32.to_le_bytes());
        assert!(Cso::from_reader(Cursor::new(huge)).is_err());
        let mut cut = tests_support::cso_of(&sample_iso(), 2048, 1);
        cut.truncate(40);
        assert!(Cso::from_reader(Cursor::new(cut)).is_err());
    }

    #[test]
    fn a_cso_whose_index_is_longer_than_the_file_is_refused_before_allocating() {
        // A bare 24-byte header claiming 2^26 blocks of 2 KB: within the block cap, so the old code
        // allocated a 256 MiB index and only then found the file was over. The index has to be in
        // the file, so the real length refuses it first.
        let mut header = Vec::new();
        header.extend_from_slice(b"CISO");
        header.extend_from_slice(&24u32.to_le_bytes());
        header.extend_from_slice(&((1u64 << 26) * 2048).to_le_bytes());
        header.extend_from_slice(&2048u32.to_le_bytes());
        header.extend_from_slice(&[1, 0, 0, 0]);
        let why = Cso::from_reader(Cursor::new(header)).err().unwrap();
        assert!(why.contains("runs past the end of the file"), "{why}");
        // One byte short of a real index is refused the same way.
        let mut short = tests_support::cso_of(&sample_iso(), 2048, 1);
        short.truncate(24 + 25 * 4 - 1);
        let why = Cso::from_reader(Cursor::new(short)).err().unwrap();
        assert!(why.contains("runs past the end of the file"), "{why}");
    }

    /// Overwrites `len` big-endian bytes of a built CHD's header.
    fn patch_be(chd: &mut [u8], at: usize, value: u64, len: usize) {
        chd[at..at + len].copy_from_slice(&value.to_be_bytes()[8 - len..]);
    }

    fn a_good_chd() -> Vec<u8> {
        chd_v5(
            &frames_of(&raw_track(0, 24)),
            &[(b"CHT2", "TRACK:1 TYPE:MODE1_RAW SUBTYPE:NONE FRAMES:24 PREGAP:0 PGTYPE:MODE1 PGSUB:RW POSTGAP:0")],
        )
    }

    /// Why `chd_track` refused a CHD written to a temporary file.
    fn chd_refusal(name: &str, bytes: &[u8]) -> String {
        let dir = crate::import::testdir::TestDir::new(name);
        let path = dir.write("game.chd", bytes);
        let why = chd_track(&path, 1).err().expect("the CHD must be refused");
        assert!(open(path.to_str().unwrap(), 1).is_none());
        why
    }

    #[test]
    fn the_good_chd_passes_the_header_check() {
        assert_eq!(check_chd_header(&mut Cursor::new(a_good_chd())), Ok(()));
    }

    #[test]
    fn a_chd_hunk_over_16_mib_is_refused_before_allocating() {
        // The hunk size lives at offset 56 of a v5 header. Neither the crate (which does not check
        // v5 headers) nor `ChdSource::hunk` would have refused 32 MiB; both allocate a hunk whole.
        let mut chd = a_good_chd();
        patch_be(&mut chd, 56, 32 << 20, 4);
        let why = chd_refusal("rc-chd-hunk", &chd);
        assert!(why.contains("hunks, more than any disc uses"), "{why}");
        // Exactly 16 MiB is not over the line.
        let mut edge = a_good_chd();
        patch_be(&mut edge, 56, 16 << 20, 4);
        assert_eq!(check_chd_header(&mut Cursor::new(edge)), Ok(()));
    }

    #[test]
    fn a_chd_larger_than_any_disc_is_refused() {
        // A logical size of 2^40 bytes in 19,584-byte hunks is 56 million hunks, whose map the
        // crate would unpack into memory whole.
        let mut chd = a_good_chd();
        patch_be(&mut chd, 32, 1 << 40, 8);
        let why = chd_refusal("rc-chd-huge", &chd);
        assert!(why.contains("larger than any disc"), "{why}");
    }

    #[test]
    fn a_chd_map_past_the_end_of_the_file_is_refused() {
        // Uncompressed: the map offset moved to the last byte of the file, so four bytes a hunk
        // cannot fit after it.
        let mut chd = a_good_chd();
        let last = chd.len() as u64 - 1;
        patch_be(&mut chd, 40, last, 8);
        let why = chd_refusal("rc-chd-map", &chd);
        assert!(why.contains("hunk map runs past the end"), "{why}");
        // Compressed (a codec in the first slot): the stored map length at the map offset claims
        // 4 GiB.
        let mut coded = a_good_chd();
        patch_be(&mut coded, 16, u64::from(u32::from_be_bytes(*b"cdzl")), 4);
        let map_at = u64::from_be_bytes(coded[40..48].try_into().unwrap()) as usize;
        patch_be(&mut coded, map_at, u64::from(u32::MAX), 4);
        let why = check_chd_header(&mut Cursor::new(coded)).err().unwrap();
        assert!(why.contains("hunk map runs past the end"), "{why}");
    }

    #[test]
    fn a_chd_whose_metadata_loops_is_refused() {
        // One metadata entry whose "next" field, at bytes 8..16 of the entry, points back at
        // itself. The crate follows it forever, so collecting it would never end.
        let mut chd = a_good_chd();
        let meta_at = u64::from_be_bytes(chd[48..56].try_into().unwrap());
        patch_be(&mut chd, meta_at as usize + 8, meta_at, 8);
        let why = chd_refusal("rc-chd-loop", &chd);
        assert!(why.contains("metadata list never ends"), "{why}");
    }

    #[test]
    fn a_v4_chd_whose_metadata_loops_is_refused_before_the_crate_opens_it() {
        // A v4 header (108 bytes, first metadata entry's offset at 36) and one entry pointing at
        // itself. The crate walks a v4 chain while opening, so this has to be caught first.
        let mut v4 = vec![0u8; 108];
        v4[..8].copy_from_slice(b"MComprHD");
        patch_be(&mut v4, 8, 108, 4);
        patch_be(&mut v4, 12, 4, 4);
        patch_be(&mut v4, 36, 108, 8);
        v4.extend_from_slice(b"CHT2");
        v4.extend_from_slice(&[0x01, 0, 0, 0]);
        v4.extend_from_slice(&108u64.to_be_bytes());
        let why = check_chd_header(&mut Cursor::new(v4.clone())).err().unwrap();
        assert!(why.contains("metadata list never ends"), "{why}");
        // The same entry ending the chain is fine as far as this check goes.
        let end = v4.len() - 8;
        v4[end..].copy_from_slice(&0u64.to_be_bytes());
        assert_eq!(check_chd_header(&mut Cursor::new(v4)), Ok(()));
    }

    /// An uncompressed CHD v5 of 2448-byte CD frames with the given metadata entries, the same
    /// layout as import's test builder (`import/detect.rs`), extended to several entries.
    fn chd_v5(frames: &[Vec<u8>], metas: &[(&[u8; 4], &str)]) -> Vec<u8> {
        const HUNK: usize = 2448 * 8;
        let mut body: Vec<u8> = Vec::new();
        for f in frames {
            let mut frame = f.clone();
            frame.resize(2448, 0);
            body.extend(frame);
        }
        let hunks = body.len().div_ceil(HUNK);
        body.resize(hunks * HUNK, 0);
        let header_len = 124usize;
        let meta_off = header_len;
        let mut meta_bytes = Vec::new();
        for (i, (tag, text)) in metas.iter().enumerate() {
            let data = format!("{text}\0");
            let entry_len = 16 + data.len();
            let next = if i + 1 < metas.len() {
                (meta_off + meta_bytes.len() + entry_len) as u64
            } else {
                0
            };
            meta_bytes.extend_from_slice(*tag);
            meta_bytes.push(0x01);
            meta_bytes.extend_from_slice(&(data.len() as u32).to_be_bytes()[1..]);
            meta_bytes.extend_from_slice(&next.to_be_bytes());
            meta_bytes.extend_from_slice(data.as_bytes());
        }
        let map_off = meta_off + meta_bytes.len();
        let data_off = (map_off + hunks * 4).div_ceil(HUNK) * HUNK;
        let mut out = Vec::new();
        out.extend_from_slice(b"MComprHD");
        out.extend_from_slice(&(header_len as u32).to_be_bytes());
        out.extend_from_slice(&5u32.to_be_bytes());
        out.extend_from_slice(&[0u8; 16]);
        out.extend_from_slice(&(body.len() as u64).to_be_bytes());
        out.extend_from_slice(&(map_off as u64).to_be_bytes());
        out.extend_from_slice(&(meta_off as u64).to_be_bytes());
        out.extend_from_slice(&(HUNK as u32).to_be_bytes());
        out.extend_from_slice(&2448u32.to_be_bytes());
        out.resize(header_len, 0);
        out.extend(meta_bytes);
        for h in 0..hunks {
            out.extend_from_slice(&((data_off / HUNK + h) as u32).to_be_bytes());
        }
        out.resize(data_off, 0);
        out.extend(body);
        out
    }

    fn frames_of(track: &[u8]) -> Vec<Vec<u8>> {
        track.chunks(2352).map(<[u8]>::to_vec).collect()
    }

    #[test]
    fn a_chd_data_track_reads_like_its_bin() {
        let dir = crate::import::testdir::TestDir::new("rc-chd");
        let frames = frames_of(&raw_track(0, 24));
        let chd = chd_v5(&frames, &[(b"CHT2", "TRACK:1 TYPE:MODE1_RAW SUBTYPE:NONE FRAMES:24 PREGAP:0 PGTYPE:MODE1 PGSUB:RW POSTGAP:0")]);
        let path = dir.write("game.chd", &chd);
        let path = path.to_str().unwrap();
        for which in [1, TRACK_FIRST_DATA, TRACK_LARGEST, TRACK_LAST] {
            let mut track = open(path, which).unwrap();
            assert_eq!(track.first_sector(), 0);
            let mut out = vec![0u8; 2048];
            assert_eq!(track.read_sector(17, &mut out), 2048);
            assert!(out.iter().all(|&b| b == 17));
        }
        assert!(open(path, 2).is_none());
        assert!(open(path, TRACK_FIRST_OF_SECOND_SESSION).is_none());
    }

    #[test]
    fn a_gdrom_chd_finds_track_three_at_its_high_density_sector() {
        // Track 1 data (4 frames), track 2 audio (4 frames), track 3 data starting at 45000.
        let dir = crate::import::testdir::TestDir::new("rc-gd");
        let mut frames = frames_of(&raw_track(0, 4));
        frames.extend((0..4).map(|_| vec![0u8; 2352]));
        frames.extend(frames_of(&raw_track(45000, 20)));
        let chd = chd_v5(
            &frames,
            &[
                (b"CHGD", "TRACK:1 TYPE:MODE1_RAW SUBTYPE:NONE FRAMES:4 PAD:0 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0"),
                (b"CHGD", "TRACK:2 TYPE:AUDIO SUBTYPE:NONE FRAMES:4 PAD:0 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0"),
                (b"CHGD", "TRACK:3 TYPE:MODE1_RAW SUBTYPE:NONE FRAMES:20 PAD:0 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0"),
            ],
        );
        let path = dir.write("dc.chd", &chd);
        let mut track = open(path.to_str().unwrap(), 3).unwrap();
        assert_eq!(track.first_sector(), 45000);
        let mut out = vec![0u8; 2048];
        assert_eq!(track.read_sector(45016 + 1, &mut out), 2048);
        assert!(out.iter().all(|&b| b == (45017u32 as u8)));
    }

    #[test]
    fn only_chd_and_cso_are_ours() {
        assert!(handles("/a/Game.CHD"));
        assert!(handles("game.cso"));
        assert!(!handles("game.cue"));
        assert!(!handles("game.iso"));
        assert!(open("/does/not/exist.chd", 1).is_none());
        assert!(open("/does/not/exist.cso", 1).is_none());
    }
}
