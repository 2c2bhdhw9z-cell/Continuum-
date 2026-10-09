//! Which system is this file for, when the extension does not say.
//!
//! A `.cue` can be a PlayStation, Sega CD, Saturn, PC Engine CD or DOS disc; a `.zip` can be one
//! cartridge, an arcade romset or a DOS game; a lone `.bin` can be a Mega Drive cartridge, an
//! Atari 2600 cartridge or a CD track. So the import looks inside:
//!
//! - **Discs** (`.cue`, `.iso`, `.bin`, `.img`, `.ccd`, `.mdf`, `.chd`): the first data sector of
//!   the first data track carries `SEGA SEGASATURN` (Saturn), `SEGA SEGAKATANA` (Dreamcast) or
//!   `SEGADISCSYSTEM` / `SEGA MEGA DRIVE` / `SEGA GENESIS` (Sega CD). The ISO 9660 primary volume
//!   descriptor at sector 16 names `PLAYSTATION` in its system identifier, and sector 4 carries
//!   Sony's licence text. A PSP disc is an ISO 9660 volume whose root holds `PSP_GAME` or
//!   `UMD_DATA.BIN`. A PC Engine CD has `PC Engine CD-ROM SYSTEM` in the first sectors of its first
//!   DATA track, which is usually track 2 because track 1 is the "this is not a music CD" warning.
//! - **`.gdi` and `.cdi`** are only ever Dreamcast.
//! - **`.chd`**: the metadata says GD-ROM (`CHGD`, `CHGT`: Dreamcast) or CD (`CHT2`, `CHTR`,
//!   `CHCD`), and for a CD the first data track's first hunk is decompressed (the `chd` crate does
//!   every CD codec in pure Rust) and read exactly like a raw disc.
//! - **Cartridge `.bin`**: `SEGA` at 0x100 (Mega Drive, `SEGA 32X` for the 32X), `ATARI7800` at 1,
//!   the Nintendo logo of a Game Boy or GBA, the N64 byte-order magic, `NES\x1A`, and last of all
//!   the Atari 2600's handful of exact ROM sizes, which is only a guess.
//! - **Archives** are listed, not unpacked: one ROM inside means that ROM's system, a `.exe`,
//!   `.com` or `.bat` means DOS, Amiga disk images mean Amiga, and a known romset name or a zip of
//!   rom chips means arcade. Arcade, DOS and Amiga keep the archive whole, because those cores load
//!   the archive itself.
//!
//! Every answer carries a confidence. At [`SURE`] or above the app routes without asking; below it
//! the app asks the user once with a system picker and remembers the answer per file.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::archive;

/// At or above this, the answer is used without asking.
pub const SURE: u8 = 80;

/// What detection concluded about one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    /// A shared system id (`ps1`, `segacd`, ...), or empty when unknown.
    pub system: String,
    /// 0 to 100. 0 means nothing was found.
    pub confidence: u8,
    /// One plain sentence: what was seen, or why nothing was.
    pub reason: String,
    /// The systems this file could plausibly be, for the picker, most likely first. Contains
    /// `system` when that is set.
    pub candidates: Vec<String>,
    /// True when the core wants the archive itself (arcade, DOS, Amiga), so it must be copied
    /// as is rather than unpacked.
    pub keep_archive: bool,
}

impl Detection {
    fn found(system: &str, confidence: u8, reason: impl Into<String>) -> Self {
        Self {
            system: system.to_string(),
            confidence,
            reason: reason.into(),
            candidates: vec![system.to_string()],
            keep_archive: false,
        }
    }

    fn unknown(reason: impl Into<String>, candidates: &[&str]) -> Self {
        Self {
            system: String::new(),
            confidence: 0,
            reason: reason.into(),
            candidates: candidates.iter().map(|s| s.to_string()).collect(),
            keep_archive: false,
        }
    }

    fn with_candidates(mut self, extra: &[&str]) -> Self {
        for c in extra {
            if !self.candidates.iter().any(|x| x == c) {
                self.candidates.push(c.to_string());
            }
        }
        self
    }

    /// Whether the app may route on this without asking.
    pub fn is_sure(&self) -> bool {
        !self.system.is_empty() && self.confidence >= SURE
    }
}

/// Systems a disc image of unknown kind could be, for the picker.
pub const DISC_CANDIDATES: &[&str] = &["ps1", "segacd", "saturn", "pcecd", "dreamcast", "psp", "dos", "amiga"];
/// Systems an archive of unknown kind could be.
pub const ARCHIVE_CANDIDATES: &[&str] = &["arcade", "dos", "amiga"];
/// Systems a cartridge `.bin` of unknown kind could be.
pub const BIN_CANDIDATES: &[&str] = &["genesis", "atari2600", "atari7800", "atari5200", "sega32x", "ps1"];

/// The system an extension names on its own, or None when it is shared or unknown.
///
/// Detection only. Which CORE runs a system is CoreCatalog's job in the app; this only answers
/// "what console is a `.sfc`", which no core choice can change.
pub fn system_for_extension(ext: &str) -> Option<&'static str> {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    Some(match ext.as_str() {
        "nes" | "unf" | "unif" => "nes",
        "fds" => "fds",
        "sfc" | "smc" | "swc" | "fig" => "snes",
        "gb" | "sgb" => "gb",
        "gbc" => "gbc",
        "gba" | "agb" => "gba",
        "sms" => "sms",
        "gg" => "gg",
        "sg" => "sg1000",
        "md" | "gen" | "smd" => "genesis",
        "32x" => "sega32x",
        "pce" => "tg16",
        "sgx" => "sgx",
        "nds" => "ds",
        "n64" | "z64" | "v64" => "n64",
        "3ds" | "3dsx" | "cci" | "cxi" => "n3ds",
        "cso" => "psp",
        "a26" => "atari2600",
        "a78" => "atari7800",
        "a52" => "atari5200",
        "lnx" => "lynx",
        "j64" | "jag" => "jaguar",
        "ws" | "wsc" => "wswan",
        "ngp" | "ngc" => "ngp",
        "min" => "pokemini",
        "vb" | "vboy" => "vb",
        "adf" | "adz" | "dms" | "ipf" | "hdf" | "hdz" | "lha" | "uae" | "rp9" => "amiga",
        "d64" | "d71" | "d81" | "g64" | "t64" | "tap" | "prg" | "crt" | "p00" => "c64",
        "exe" | "com" | "bat" | "dosz" => "dos",
        "wad" => "doom",
        "swf" => "flash",
        "jar" | "jad" => "j2me",
        "gcm" | "gcz" | "rvz" => "gamecube",
        "wbfs" | "wad2" => "wii",
        "sis" | "sisx" | "n-gage" => "symbian",
        "gdi" | "cdi" => "dreamcast",
        _ => return None,
    })
}

/// Detects the system of a file on disk, reading only what it needs (headers, a few sectors,
/// an archive's directory). Never panics; an unreadable file is an unknown with the reason.
pub fn detect_path(path: &Path) -> Detection {
    detect_depth(path, 0)
}

fn detect_depth(path: &Path, depth: u32) -> Detection {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("this file");
    let result = match ext.as_str() {
        "cue" => detect_cue(path),
        "gdi" | "cdi" => Ok(Detection::found("dreamcast", 99, format!("{name} is a .{ext}, which only the Dreamcast uses"))),
        "chd" => detect_chd(path),
        "iso" | "img" | "mdf" => detect_image(path),
        "ccd" => detect_ccd(path),
        "bin" => detect_bin(path),
        "pbp" => detect_pbp(path),
        "m3u" if depth < 2 => detect_m3u(path, depth),
        "zip" | "7z" => detect_archive(path),
        _ => match system_for_extension(&ext) {
            Some(system) => Ok(Detection::found(system, 99, format!(".{ext} is only ever {system}"))),
            None => Ok(Detection::unknown(format!(".{ext} is not a format Continuum knows"), &[])),
        },
    };
    result.unwrap_or_else(|e| Detection::unknown(format!("{name} could not be read: {e}"), DISC_CANDIDATES))
}

// MARK: - Sectors

/// 12-byte CD sync pattern at the start of every raw data sector.
const SYNC: [u8; 12] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];

/// Where the 2048 user bytes start inside one stored sector.
fn user_offset(sector: &[u8], declared_mode2_2336: bool) -> usize {
    if sector.len() >= 16 && sector[..12] == SYNC {
        // Mode byte at 15: mode 1 user data follows the 16-byte header, mode 2 form 1 has an
        // 8-byte subheader after it.
        if sector[15] == 2 { 24 } else { 16 }
    } else if declared_mode2_2336 {
        8
    } else {
        0
    }
}

/// Anything that can hand out the 2048 user bytes of a data track's sectors.
trait Sectors {
    fn user_data(&mut self, lba: u32) -> Option<Vec<u8>>;
}

/// A data track stored in a plain file at a fixed sector size.
struct FileTrack {
    file: File,
    start: u64,
    sector_size: u32,
    mode2_2336: bool,
}

impl Sectors for FileTrack {
    fn user_data(&mut self, lba: u32) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; self.sector_size as usize];
        self.file
            .seek(SeekFrom::Start(self.start + lba as u64 * self.sector_size as u64))
            .ok()?;
        read_full(&mut self.file, &mut buf).ok()?;
        let off = user_offset(&buf, self.mode2_2336);
        let end = (off + 2048).min(buf.len());
        Some(buf[off..end].to_vec())
    }
}

fn read_full(file: &mut impl Read, buf: &mut [u8]) -> std::io::Result<()> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = file.read(&mut buf[filled..])?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "short read"));
        }
        filled += n;
    }
    Ok(())
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}

/// What the first data track's sectors say. None when nothing recognisable was found.
fn classify_data_track(track: &mut dyn Sectors) -> Option<Detection> {
    let first = track.user_data(0)?;
    if first.starts_with(b"SEGA SEGASATURN") {
        return Some(Detection::found("saturn", 98, "the disc header says SEGA SEGASATURN"));
    }
    if first.starts_with(b"SEGA SEGAKATANA") {
        return Some(Detection::found("dreamcast", 98, "the disc header says SEGA SEGAKATANA"));
    }
    if first.starts_with(b"SEGADISCSYSTEM")
        || first.starts_with(b"SEGABOOTDISC")
        || first.starts_with(b"SEGA MEGA DRIVE")
        || first.starts_with(b"SEGA GENESIS")
    {
        return Some(Detection::found("segacd", 97, "the disc header is a Sega CD boot header"));
    }
    if let Some(pvd) = track.user_data(16) {
        if pvd.len() >= 190 && pvd[0] == 1 && &pvd[1..6] == b"CD001" {
            let system_id = String::from_utf8_lossy(&pvd[8..40]).trim().to_string();
            if contains(system_id.as_bytes(), b"PSP GAME") || has_psp_root(track, &pvd) {
                return Some(Detection::found("psp", 97, "the disc's root folder holds PSP_GAME or UMD_DATA.BIN"));
            }
            if contains(system_id.as_bytes(), b"PLAYSTATION") {
                return Some(Detection::found("ps1", 95, "the volume's system id says PLAYSTATION"));
            }
            if contains(system_id.as_bytes(), b"CDTV") || contains(system_id.as_bytes(), b"CD32")
                || contains(system_id.as_bytes(), b"AMIGA")
            {
                return Some(Detection::found("amiga", 75, format!("the volume's system id says {system_id}")));
            }
        }
    }
    if let Some(licence) = track.user_data(4) {
        if contains(&licence, b"Sony Computer Entertainment") {
            return Some(Detection::found("ps1", 90, "sector 4 carries the PlayStation licence text"));
        }
    }
    None
}

/// Whether the ISO 9660 root directory names PSP_GAME or UMD_DATA.BIN.
fn has_psp_root(track: &mut dyn Sectors, pvd: &[u8]) -> bool {
    // Root directory record at offset 156 of the PVD: extent LBA at +2 (LE), data length at +10.
    let root = &pvd[156..190];
    let lba = u32::from_le_bytes([root[2], root[3], root[4], root[5]]);
    let len = u32::from_le_bytes([root[10], root[11], root[12], root[13]]);
    let sectors = len.div_ceil(2048).clamp(1, 8);
    for i in 0..sectors {
        let Some(dir) = track.user_data(lba + i) else { return false };
        if contains(&dir, b"PSP_GAME") || contains(&dir, b"UMD_DATA.BIN") {
            return true;
        }
    }
    false
}

fn has_pce_signature(track: &mut dyn Sectors) -> bool {
    (0..3).any(|lba| {
        track
            .user_data(lba)
            .is_some_and(|data| contains(&data, b"PC Engine CD-ROM SYSTEM"))
    })
}

/// Classifies a disc from its data tracks, in disc order.
fn classify_disc(mut data_tracks: Vec<Box<dyn Sectors>>, had_audio_first: bool, what: &str) -> Detection {
    if let Some(first) = data_tracks.first_mut() {
        if let Some(found) = classify_data_track(first.as_mut()) {
            return found.with_candidates(DISC_CANDIDATES);
        }
    }
    for track in data_tracks.iter_mut() {
        if has_pce_signature(track.as_mut()) {
            return Detection::found("pcecd", 95, "a data track says PC Engine CD-ROM SYSTEM")
                .with_candidates(DISC_CANDIDATES);
        }
    }
    if data_tracks.is_empty() {
        return Detection::unknown(format!("{what} has no data track to read"), DISC_CANDIDATES);
    }
    let hint = if had_audio_first { " (track 1 is audio, which PC Engine CD discs often are)" } else { "" };
    Detection::unknown(format!("{what} carries no console signature Continuum knows{hint}"), DISC_CANDIDATES)
}

// MARK: - Cue sheets

/// One TRACK line of a cue sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CueTrack {
    pub number: u32,
    /// The mode word as written: `AUDIO`, `MODE1/2048`, `MODE1/2352`, `MODE2/2352`, `MODE2/2336`.
    pub mode: String,
    /// INDEX 01 position in frames (75 per second) from the start of its FILE.
    pub index1: u32,
}

impl CueTrack {
    pub fn is_audio(&self) -> bool {
        self.mode.eq_ignore_ascii_case("AUDIO")
    }

    pub fn sector_size(&self) -> u32 {
        self.mode
            .rsplit('/')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(2352)
    }
}

/// One FILE line of a cue sheet and the tracks under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CueFile {
    pub name: String,
    pub tracks: Vec<CueTrack>,
}

/// Parses a cue sheet's FILE / TRACK / INDEX 01 lines. Unknown lines are ignored.
pub fn parse_cue(text: &str) -> Vec<CueFile> {
    let mut files: Vec<CueFile> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("FILE ") {
            let rest = line[5..].trim();
            let name = if let Some(stripped) = rest.strip_prefix('"') {
                stripped.split('"').next().unwrap_or("").to_string()
            } else {
                // Unquoted: everything up to the last word (the file type).
                match rest.rsplit_once(' ') {
                    Some((n, _)) => n.trim().to_string(),
                    None => rest.to_string(),
                }
            };
            files.push(CueFile { name, tracks: Vec::new() });
        } else if upper.starts_with("TRACK ") {
            let mut parts = line.split_whitespace().skip(1);
            let number = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
            let mode = parts.next().unwrap_or("MODE1/2352").to_string();
            if let Some(file) = files.last_mut() {
                file.tracks.push(CueTrack { number, mode, index1: 0 });
            }
        } else if upper.starts_with("INDEX ") {
            let mut parts = line.split_whitespace().skip(1);
            let index: u32 = parts.next().and_then(|n| n.parse().ok()).unwrap_or(99);
            if index == 1 {
                let frames = parts.next().map(parse_msf).unwrap_or(0);
                if let Some(track) = files.last_mut().and_then(|f| f.tracks.last_mut()) {
                    track.index1 = frames;
                }
            }
        }
    }
    files
}

/// `mm:ss:ff` to frames.
fn parse_msf(text: &str) -> u32 {
    let parts: Vec<u32> = text.split(':').filter_map(|p| p.trim().parse().ok()).collect();
    match parts.as_slice() {
        [m, s, f] => (m * 60 + s) * 75 + f,
        _ => 0,
    }
}

/// Finds a sibling file by name, case-insensitively, the way a cue's FILE line is meant.
fn sibling(dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = dir.join(name);
    if direct.exists() {
        return Some(direct);
    }
    let base = Path::new(name).file_name()?.to_str()?.to_ascii_lowercase();
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.to_ascii_lowercase() == base)
    })
}

fn detect_cue(path: &Path) -> std::io::Result<Detection> {
    let text = String::from_utf8_lossy(&std::fs::read(path)?).into_owned();
    let dir = path.parent().unwrap_or(Path::new("."));
    let files = parse_cue(&text);
    let mut tracks: Vec<Box<dyn Sectors>> = Vec::new();
    let mut had_audio_first = false;
    let mut missing = Vec::new();
    for (fi, file) in files.iter().enumerate() {
        for (ti, track) in file.tracks.iter().enumerate() {
            if fi == 0 && ti == 0 && track.is_audio() {
                had_audio_first = true;
            }
            if track.is_audio() {
                continue;
            }
            match sibling(dir, &file.name).and_then(|p| File::open(p).ok()) {
                Some(handle) => tracks.push(Box::new(FileTrack {
                    file: handle,
                    start: track.index1 as u64 * track.sector_size() as u64,
                    sector_size: track.sector_size(),
                    mode2_2336: track.sector_size() == 2336,
                })),
                None => missing.push(file.name.clone()),
            }
        }
    }
    if tracks.is_empty() && !missing.is_empty() {
        return Ok(Detection::unknown(
            format!("the cue sheet names {} but it is not beside it", missing.join(", ")),
            DISC_CANDIDATES,
        ));
    }
    Ok(classify_disc(tracks, had_audio_first, "the cue sheet"))
}

// MARK: - Plain images

/// Opens a single-file disc image, finding its sector size from the sync pattern.
fn open_image(path: &Path) -> std::io::Result<Option<FileTrack>> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut head = vec![0u8; 2448 + 16];
    let got = file.read(&mut head)?;
    head.truncate(got);
    if head.len() >= 12 && head[..12] == SYNC {
        let size = if head.len() >= 2352 + 12 && head[2352..2364] == SYNC {
            2352
        } else if head.len() >= 2448 + 12 && head[2448..2460] == SYNC {
            2448
        } else {
            2352
        };
        return Ok(Some(FileTrack { file, start: 0, sector_size: size, mode2_2336: false }));
    }
    // A cooked 2048-byte image: ISO 9660 PVD at sector 16, or a Sega header at 0.
    if len >= 2048 {
        return Ok(Some(FileTrack { file, start: 0, sector_size: 2048, mode2_2336: false }));
    }
    Ok(None)
}

/// GameCube and Wii discs carry a magic word in their header.
fn nintendo_disc_magic(path: &Path) -> Option<Detection> {
    let mut file = File::open(path).ok()?;
    let mut head = [0u8; 0x20];
    read_full(&mut file, &mut head).ok()?;
    if head[0x18..0x1C] == [0x5D, 0x1C, 0x9E, 0xA3] {
        return Some(Detection::found("wii", 97, "the disc header carries the Wii magic word"));
    }
    if head[0x1C..0x20] == [0xC2, 0x33, 0x9F, 0x3D] {
        return Some(Detection::found("gamecube", 97, "the disc header carries the GameCube magic word"));
    }
    None
}

fn detect_image(path: &Path) -> std::io::Result<Detection> {
    if let Some(found) = nintendo_disc_magic(path) {
        return Ok(found);
    }
    match open_image(path)? {
        Some(track) => Ok(classify_disc(vec![Box::new(track)], false, "the disc image")),
        None => Ok(Detection::unknown("the image is too small to be a disc", DISC_CANDIDATES)),
    }
}

/// CloneCD: the .ccd is a description and the sectors are in the .img beside it.
fn detect_ccd(path: &Path) -> std::io::Result<Detection> {
    let text = String::from_utf8_lossy(&std::fs::read(path)?).into_owned();
    let dir = path.parent().unwrap_or(Path::new("."));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let Some(img) = sibling(dir, &format!("{stem}.img")) else {
        return Ok(Detection::unknown(format!("{stem}.img is not beside the .ccd"), DISC_CANDIDATES));
    };
    // [TRACK n] MODE=0 (audio) / 1 / 2, then INDEX 1=<lba>.
    let mut data_starts: Vec<u32> = Vec::new();
    let mut had_audio_first = false;
    let mut mode: Option<u32> = None;
    let mut first_track = true;
    for line in text.lines().map(str::trim) {
        if line.to_ascii_uppercase().starts_with("[TRACK") {
            mode = None;
        } else if let Some(v) = line.strip_prefix("MODE=") {
            mode = v.trim().parse().ok();
            if first_track && mode == Some(0) {
                had_audio_first = true;
            }
            first_track = false;
        } else if let Some(v) = line.strip_prefix("INDEX 1=") {
            if let (Some(m), Ok(lba)) = (mode, v.trim().parse::<u32>()) {
                if m != 0 {
                    data_starts.push(lba);
                }
            }
        }
    }
    if data_starts.is_empty() {
        data_starts.push(0);
    }
    let mut tracks: Vec<Box<dyn Sectors>> = Vec::new();
    for lba in data_starts {
        tracks.push(Box::new(FileTrack {
            file: File::open(&img)?,
            start: lba as u64 * 2352,
            sector_size: 2352,
            mode2_2336: false,
        }));
    }
    Ok(classify_disc(tracks, had_audio_first, "the CloneCD image"))
}

// MARK: - .bin

/// A cue sheet beside this .bin that names it, if any.
fn cue_naming(path: &Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("cue"))
            && std::fs::read(p).ok().is_some_and(|bytes| {
                parse_cue(&String::from_utf8_lossy(&bytes))
                    .iter()
                    .any(|f| Path::new(&f.name).file_name().and_then(|n| n.to_str())
                        .is_some_and(|n| n.to_ascii_lowercase() == name))
            })
    })
}

/// Cartridge headers, for a ROM whose extension says nothing.
pub fn detect_cartridge(head: &[u8], len: u64) -> Option<Detection> {
    if head.len() >= 0x108 && &head[0x100..0x104] == b"SEGA" {
        if head[0x100..].starts_with(b"SEGA 32X") {
            return Some(Detection::found("sega32x", 95, "the header at 0x100 says SEGA 32X"));
        }
        return Some(Detection::found("genesis", 95, "the header at 0x100 says SEGA"));
    }
    if head.len() >= 10 && &head[1..10] == b"ATARI7800" {
        return Some(Detection::found("atari7800", 98, "the file starts with the ATARI7800 header"));
    }
    if head.len() >= 4 && &head[..4] == b"NES\x1A" {
        return Some(Detection::found("nes", 98, "the file starts with an iNES header"));
    }
    if head.len() >= 4 && &head[..4] == b"LYNX" {
        return Some(Detection::found("lynx", 95, "the file starts with a Lynx header"));
    }
    if head.len() >= 4 {
        let magic = [head[0], head[1], head[2], head[3]];
        if magic == [0x80, 0x37, 0x12, 0x40] || magic == [0x37, 0x80, 0x40, 0x12] || magic == [0x40, 0x12, 0x37, 0x80] {
            return Some(Detection::found("n64", 97, "the file starts with the N64 boot magic"));
        }
    }
    if head.len() >= 0x150 && head[0x104..0x108] == [0xCE, 0xED, 0x66, 0x66] {
        let color = head[0x143] & 0x80 != 0;
        return Some(if color {
            Detection::found("gbc", 95, "Game Boy logo with the Color flag")
        } else {
            Detection::found("gb", 95, "the Game Boy logo is at 0x104")
        });
    }
    if head.len() >= 0xC0 && head[0x04..0x08] == [0x24, 0xFF, 0xAE, 0x51] {
        return Some(Detection::found("gba", 95, "the GBA logo is at 0x04"));
    }
    if matches!(len, 2048 | 4096 | 8192 | 12288 | 16384 | 32768 | 65536) {
        return Some(
            Detection::found("atari2600", 55, format!("{len} bytes is an Atari 2600 cartridge size, but other systems use it too"))
                .with_candidates(&["atari5200", "atari7800"]),
        );
    }
    None
}

fn detect_bin(path: &Path) -> std::io::Result<Detection> {
    if let Some(cue) = cue_naming(path) {
        let mut found = detect_cue(&cue)?;
        found.reason = format!("a cue sheet beside it names it: {}", found.reason);
        return Ok(found);
    }
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut head = vec![0u8; 0x200.min(len as usize)];
    read_full(&mut file, &mut head)?;
    let is_disc = (head.len() >= 12 && head[..12] == SYNC) || len >= 0x8006 && {
        let mut pvd = [0u8; 6];
        file.seek(SeekFrom::Start(0x8000))?;
        read_full(&mut file, &mut pvd).is_ok() && &pvd[1..6] == b"CD001"
    };
    if is_disc {
        return detect_image(path);
    }
    if let Some(found) = detect_cartridge(&head, len) {
        return Ok(found.with_candidates(BIN_CANDIDATES));
    }
    Ok(Detection::unknown("the .bin has no cartridge header and is not a disc track", BIN_CANDIDATES))
}

// MARK: - PBP, m3u

fn detect_pbp(path: &Path) -> std::io::Result<Detection> {
    let mut file = File::open(path)?;
    let mut head = [0u8; 0x28];
    read_full(&mut file, &mut head)?;
    if &head[..4] != b"\0PBP" {
        return Ok(Detection::unknown("the .pbp has no PBP header", &["ps1", "psp"]));
    }
    let psar = u32::from_le_bytes([head[0x24], head[0x25], head[0x26], head[0x27]]) as u64;
    file.seek(SeekFrom::Start(psar))?;
    let mut magic = [0u8; 12];
    if read_full(&mut file, &mut magic).is_ok()
        && (magic.starts_with(b"PSISOIMG") || magic.starts_with(b"PSTITLEIMG"))
    {
        return Ok(Detection::found("ps1", 97, "the EBOOT holds a PlayStation disc image").with_candidates(&["psp"]));
    }
    Ok(Detection::found("psp", 90, "a PSP EBOOT with no PlayStation disc inside").with_candidates(&["ps1"]))
}

fn detect_m3u(path: &Path, depth: u32) -> std::io::Result<Detection> {
    let text = String::from_utf8_lossy(&std::fs::read(path)?).into_owned();
    let dir = path.parent().unwrap_or(Path::new("."));
    let first = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'));
    match first.and_then(|name| sibling(dir, name)) {
        Some(disc) => {
            let mut found = detect_depth(&disc, depth + 1);
            found.reason = format!("the playlist's first disc: {}", found.reason);
            Ok(found)
        }
        None => Ok(Detection::unknown("the playlist's first disc is not beside it", DISC_CANDIDATES)),
    }
}

/// The files a multi-file sheet names (.cue tracks, .m3u discs, .gdi tracks, the .img and .sub of
/// a .ccd), as written. Empty for anything else. The import uses it to say which are missing.
pub fn referenced_files(path: &Path) -> Vec<String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
    let text = String::from_utf8_lossy(&bytes);
    let mut names: Vec<String> = match ext.as_str() {
        "cue" => parse_cue(&text).into_iter().map(|f| f.name).collect(),
        "m3u" => text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect(),
        "gdi" => text
            .lines()
            .skip(1)
            .filter_map(|l| {
                // "n lba type size name offset", name possibly quoted.
                if let Some(start) = l.find('"') {
                    l[start + 1..].split('"').next().map(String::from)
                } else {
                    l.split_whitespace().nth(4).map(String::from)
                }
            })
            .collect(),
        "ccd" => {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            vec![format!("{stem}.img"), format!("{stem}.sub")]
        }
        _ => Vec::new(),
    };
    let mut seen = std::collections::HashSet::new();
    names.retain(|n| seen.insert(n.to_ascii_lowercase()));
    names
}

// MARK: - CHD

/// One track out of a CHD's CHT2 metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChdTrack {
    pub number: u32,
    pub kind: String,
    pub frames: u32,
    pub pregap: u32,
    pub pregap_stored: bool,
}

/// Parses `TRACK:1 TYPE:MODE2_RAW SUBTYPE:NONE FRAMES:1234 PREGAP:150 PGTYPE:VMODE2_RAW ...`.
pub fn parse_cht2(text: &str) -> Option<ChdTrack> {
    let mut track = ChdTrack { number: 0, kind: String::new(), frames: 0, pregap: 0, pregap_stored: false };
    for field in text.trim_end_matches('\0').split_whitespace() {
        let (key, value) = field.split_once(':')?;
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

const fn tag(b: &[u8; 4]) -> u32 {
    ((b[0] as u32) << 24) | ((b[1] as u32) << 16) | ((b[2] as u32) << 8) | b[3] as u32
}

struct ChdSectors {
    chd: chd::Chd<File>,
    unit: u32,
    hunk_size: u32,
    first_frame: u64,
    cached: Option<(u32, Vec<u8>)>,
    comp: Vec<u8>,
    mode2_2336: bool,
}

impl Sectors for ChdSectors {
    fn user_data(&mut self, lba: u32) -> Option<Vec<u8>> {
        let byte = (self.first_frame + lba as u64) * self.unit as u64;
        let hunk_num = (byte / self.hunk_size as u64) as u32;
        let within = (byte % self.hunk_size as u64) as usize;
        if self.cached.as_ref().map(|c| c.0) != Some(hunk_num) {
            let mut out = vec![0u8; self.hunk_size as usize];
            let mut hunk = self.chd.hunk(hunk_num).ok()?;
            hunk.read_hunk_in(&mut self.comp, &mut out).ok()?;
            self.cached = Some((hunk_num, out));
        }
        let data = &self.cached.as_ref()?.1;
        let sector_len = (self.unit as usize).min(2352);
        let sector = data.get(within..within + sector_len)?;
        let off = user_offset(sector, self.mode2_2336);
        Some(sector[off..(off + 2048).min(sector.len())].to_vec())
    }
}

fn detect_chd(path: &Path) -> std::io::Result<Detection> {
    let open = || -> Result<chd::Chd<File>, String> {
        chd::Chd::open(File::open(path).map_err(|e| e.to_string())?, None).map_err(|e| e.to_string())
    };
    let mut chd = match open() {
        Ok(c) => c,
        Err(e) => return Ok(Detection::unknown(format!("the CHD could not be opened: {e}"), DISC_CANDIDATES)),
    };
    if chd.header().has_parent() {
        return Ok(Detection::unknown("this CHD needs a parent CHD, so it cannot be read on its own", DISC_CANDIDATES));
    }
    let refs: Vec<_> = chd.metadata_refs().collect();
    let mut tracks = Vec::new();
    let mut is_cd = false;
    let mut is_dvd = false;
    for r in refs {
        let t = chd::metadata::MetadataTag::metatag(&r);
        if t == tag(b"CHGD") || t == tag(b"CHGT") {
            return Ok(Detection::found("dreamcast", 98, "the CHD's metadata says GD-ROM"));
        }
        if t == tag(b"DVD ") {
            is_dvd = true;
        }
        if t == tag(b"CHT2") || t == tag(b"CHTR") || t == tag(b"CHCD") {
            is_cd = true;
            if t != tag(b"CHCD") {
                if let Ok(meta) = r.read(chd.inner()) {
                    if let Some(track) = parse_cht2(&String::from_utf8_lossy(&meta.value)) {
                        tracks.push(track);
                    }
                }
            }
        }
    }
    let unit = chd.header().unit_bytes();
    let hunk_size = chd.header().hunk_size();
    if unit == 0 || hunk_size == 0 {
        return Ok(Detection::unknown("the CHD header has no sector size", DISC_CANDIDATES));
    }
    let make = |first_frame: u64, kind: &str| -> Result<ChdSectors, String> {
        Ok(ChdSectors {
            chd: open()?,
            unit,
            hunk_size,
            first_frame,
            cached: None,
            comp: Vec::new(),
            mode2_2336: kind == "MODE2" || kind == "MODE2_FORM_MIX",
        })
    };
    if is_dvd || (!is_cd && unit == 2048) {
        return Ok(match make(0, "") {
            Ok(s) => classify_disc(vec![Box::new(s)], false, "the CHD"),
            Err(e) => Detection::unknown(format!("the CHD could not be read: {e}"), DISC_CANDIDATES),
        });
    }
    if !is_cd {
        return Ok(Detection::unknown("the CHD is neither a CD nor a GD-ROM (a hard disk image?)", DISC_CANDIDATES));
    }
    tracks.sort_by_key(|t| t.number);
    if tracks.is_empty() {
        tracks.push(ChdTrack { number: 1, kind: "MODE1_RAW".into(), frames: u32::MAX, pregap: 0, pregap_stored: false });
    }
    let had_audio_first = tracks.first().is_some_and(|t| t.kind == "AUDIO");
    let mut sources: Vec<Box<dyn Sectors>> = Vec::new();
    let mut frame: u64 = 0;
    for track in &tracks {
        if track.kind != "AUDIO" {
            let start = frame + if track.pregap_stored { track.pregap as u64 } else { 0 };
            match make(start, &track.kind) {
                Ok(s) => sources.push(Box::new(s)),
                Err(e) => return Ok(Detection::unknown(format!("the CHD could not be read: {e}"), DISC_CANDIDATES)),
            }
        }
        // Each track is padded to a multiple of four frames in the CHD.
        let frames = track.frames as u64;
        frame += frames.div_ceil(4) * 4;
    }
    Ok(classify_disc(sources, had_audio_first, "the CHD"))
}

// MARK: - Archives

fn detect_archive(path: &Path) -> std::io::Result<Detection> {
    let names = archive::list(path).map_err(std::io::Error::other)?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    Ok(classify_archive(stem, &names))
}

/// Romset names common enough to be worth recognising by name alone. Lowercase, no extension.
/// Not a full list: a zip of rom chips is recognised by its contents too (see `looks_like_chips`).
const ARCADE_SETS: &[&str] = &[
    "1941", "1942", "1943", "19xx", "aof", "aof2", "aof3", "armwar", "avsp", "bublbobl", "bombjack",
    "btime", "burgert", "cadash", "captcomm", "centiped", "contra", "cybots", "ddonpach", "ddragon",
    "ddragon2", "ddsom", "ddtod", "defender", "digdug", "dino", "dkong", "dkongjr", "donpachi",
    "dstlk", "elevator", "esprade", "fatfury1", "fatfury2", "fatfursp", "ffight", "frogger",
    "galaga", "galaxian", "garou", "gauntlet", "ghouls", "gng", "gunbird", "hsf2", "invaders",
    "joust", "kinst", "knights", "kof94", "kof95", "kof96", "kof97", "kof98", "kof99", "kof2000",
    "kof2001", "kof2002", "kof2003", "lastblad", "lastbld2", "mk", "mk2", "mk3", "mmatrix",
    "mpatrol", "mslug", "mslug2", "mslug3", "mslug4", "mslug5", "mslugx", "msh", "mshvsf", "mvsc",
    "nbajam", "neogeo", "nightstr", "outrun", "pacman", "pgm", "phoenix", "punisher", "puzzloop",
    "qbert", "qsound", "rbff1", "rbff2", "rbffspec", "robotron", "rtype", "rtype2", "samsho",
    "samsho2", "samsho3", "samsho4", "samsho5", "samsh5sp", "scramble", "sf", "sf2", "sf2ce",
    "sf2hf", "sf2t", "sfa", "sfa2", "sfa3", "sfiii", "sfiii2", "sfiii3", "sgemf", "shinobi",
    "simpsons", "spf2t", "ssf2", "ssf2t", "strider", "tmnt", "tmnt2", "truxton", "twinbee",
    "vsav", "vsav2", "wof", "xmcota", "xmvsf", "zookeep",
];

/// Extensions that only ever name a playable ROM or disc entry inside an archive.
fn is_content_ext(ext: &str) -> bool {
    system_for_extension(ext).is_some()
        || matches!(ext, "cue" | "bin" | "iso" | "img" | "chd" | "ccd" | "sub" | "m3u" | "mdf" | "mds" | "toc" | "pbp" | "gdi" | "raw")
}

/// Names that ride along in archives and say nothing about the game.
fn is_noise(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    lower.starts_with("__macosx/")
        || base.starts_with('.')
        || base.ends_with(".txt")
        || base.ends_with(".nfo")
        || base.ends_with(".diz")
        || base.ends_with(".url")
        || base.ends_with(".jpg")
        || base.ends_with(".png")
        || base.ends_with(".pdf")
        || base == "thumbs.db"
}

fn ext_of(name: &str) -> String {
    let base = name.rsplit('/').next().unwrap_or(name);
    match base.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// Rom chip names: no extension, or a short board-position one such as `.1a`, `.u12`, `.ic3`,
/// `.6f`, `.p1`, `.m1`, `.c1`, or `.rom` / `.bin` among many siblings.
fn looks_like_chip(name: &str) -> bool {
    let ext = ext_of(name);
    if ext.is_empty() {
        return true;
    }
    if ext == "rom" {
        return true;
    }
    let has_digit = ext.chars().any(|c| c.is_ascii_digit());
    ext.len() <= 4 && has_digit && !is_content_ext(&ext)
}

/// Classifies an archive from its entry names alone. `stem` is the archive's own name without
/// its extension (arcade romsets are recognised by it).
pub fn classify_archive(stem: &str, names: &[String]) -> Detection {
    let entries: Vec<&String> = names.iter().filter(|n| !n.ends_with('/') && !is_noise(n)).collect();
    let keep = |mut d: Detection| {
        d.keep_archive = true;
        d.with_candidates(ARCHIVE_CANDIDATES)
    };
    if entries.is_empty() {
        return Detection::unknown("the archive holds nothing but folders and notes", ARCHIVE_CANDIDATES);
    }
    let exts: Vec<String> = entries.iter().map(|n| ext_of(n)).collect();
    if exts.iter().any(|e| matches!(e.as_str(), "exe" | "com" | "bat"))
        || entries.iter().any(|n| n.to_ascii_lowercase().ends_with("dosbox.conf"))
    {
        return keep(Detection::found("dos", 92, "the archive holds a DOS program (.exe, .com or .bat)"));
    }
    let amiga = exts.iter().any(|e| matches!(e.as_str(), "adf" | "adz" | "dms" | "ipf" | "hdf" | "lha" | "uae" | "slave"))
        || entries.iter().any(|n| n.to_ascii_lowercase().ends_with(".info"));
    if amiga {
        return keep(Detection::found("amiga", 90, "the archive holds Amiga disk or WHDLoad files"));
    }
    if ARCADE_SETS.contains(&stem.to_ascii_lowercase().as_str()) {
        return keep(Detection::found("arcade", 92, format!("{stem} is a known arcade romset name")));
    }
    let content: Vec<usize> = (0..entries.len()).filter(|&i| is_content_ext(&exts[i])).collect();
    // A single ROM (ignoring notes): that ROM's system, unpacked.
    if content.len() == 1 && entries.len() <= 2 {
        let ext = &exts[content[0]];
        if let Some(system) = system_for_extension(ext) {
            return Detection::found(system, 95, format!("the archive holds one .{ext} ROM"));
        }
    }
    let chips = entries.iter().filter(|n| looks_like_chip(n)).count();
    if entries.len() >= 2 && chips * 2 >= entries.len() && content.len() * 2 < entries.len() {
        return keep(Detection::found("arcade", 75, format!("{chips} of {} entries look like arcade rom chips", entries.len())));
    }
    // A disc set (cue plus bins, m3u plus discs): unpack, then detect the sheet once it is on disk.
    if let Some(i) = content.iter().copied().find(|&i| matches!(exts[i].as_str(), "m3u" | "cue" | "gdi" | "ccd" | "chd" | "iso" | "pbp")) {
        let ext = &exts[i];
        if let Some(system) = system_for_extension(ext) {
            return Detection::found(system, 95, format!("the archive holds a .{ext}"));
        }
        return Detection {
            system: String::new(),
            confidence: 0,
            reason: format!("the archive holds a .{ext} disc set; it is unpacked and the disc is read after"),
            candidates: DISC_CANDIDATES.iter().map(|s| s.to_string()).collect(),
            keep_archive: false,
        };
    }
    // Several ROMs for one system (a cartridge collection): that system, unpacked.
    let systems: Vec<&str> = content.iter().filter_map(|&i| system_for_extension(&exts[i])).collect();
    if let Some(first) = systems.first() {
        if systems.iter().all(|s| s == first) {
            return Detection::found(first, 90, format!("the archive holds {} {first} ROMs", systems.len()));
        }
    }
    Detection::unknown("nothing in the archive names a system", ARCHIVE_CANDIDATES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::testdir::TestDir;

    /// A raw 2352-byte mode 1 sector holding `user` (padded to 2048).
    fn raw_sector(user: &[u8], mode: u8) -> Vec<u8> {
        let mut s = vec![0u8; 2352];
        s[..12].copy_from_slice(&SYNC);
        s[15] = mode;
        let off = if mode == 2 { 24 } else { 16 };
        s[off..off + user.len()].copy_from_slice(user);
        s
    }

    fn disc(sectors: &[(u32, &[u8])], count: u32, mode: u8) -> Vec<u8> {
        let mut out = Vec::new();
        for lba in 0..count {
            let user = sectors.iter().find(|(l, _)| *l == lba).map(|(_, u)| *u).unwrap_or(&[]);
            out.extend(raw_sector(user, mode));
        }
        out
    }

    fn pvd(system_id: &str, root_lba: u32) -> Vec<u8> {
        let mut p = vec![0u8; 2048];
        p[0] = 1;
        p[1..6].copy_from_slice(b"CD001");
        let id = format!("{system_id:<32}");
        p[8..40].copy_from_slice(id.as_bytes());
        p[156 + 2..156 + 6].copy_from_slice(&root_lba.to_le_bytes());
        p[156 + 10..156 + 14].copy_from_slice(&2048u32.to_le_bytes());
        p
    }

    fn cue_for(bin: &str) -> String {
        format!("FILE \"{bin}\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n")
    }

    #[test]
    fn cue_parsing_reads_files_tracks_and_index() {
        let text = "FILE \"Game (Track 1).bin\" BINARY\r\n  TRACK 01 MODE1/2352\r\n    INDEX 01 00:00:00\r\nFILE \"Game (Track 2).bin\" BINARY\r\n  TRACK 02 AUDIO\r\n    INDEX 00 00:00:00\r\n    INDEX 01 00:02:00\r\n";
        let files = parse_cue(text);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "Game (Track 1).bin");
        assert_eq!(files[0].tracks[0].sector_size(), 2352);
        assert!(files[1].tracks[0].is_audio());
        assert_eq!(files[1].tracks[0].index1, 150);
        let unquoted = parse_cue("FILE game.bin BINARY\nTRACK 1 MODE1/2048\nINDEX 1 00:00:00");
        assert_eq!(unquoted[0].name, "game.bin");
        assert_eq!(unquoted[0].tracks[0].sector_size(), 2048);
    }

    #[test]
    fn cue_detects_saturn_segacd_dreamcast_and_ps1() {
        let dir = TestDir::new("cue");
        let cases: [(&str, Vec<u8>, &str); 4] = [
            ("sat", disc(&[(0, b"SEGA SEGASATURN SEGA ENTERPRISES")], 20, 1), "saturn"),
            ("scd", disc(&[(0, b"SEGADISCSYSTEM  ")], 20, 1), "segacd"),
            ("dc", disc(&[(0, b"SEGA SEGAKATANA SEGA ENTERPRISES")], 20, 1), "dreamcast"),
            ("psx", disc(&[(16, &pvd("PLAYSTATION", 22))], 24, 2), "ps1"),
        ];
        for (stem, bytes, want) in cases {
            dir.write(&format!("{stem}.bin"), &bytes);
            let cue = dir.write(&format!("{stem}.cue"), cue_for(&format!("{stem}.bin")).as_bytes());
            let d = detect_path(&cue);
            assert_eq!(d.system, want, "{stem}: {}", d.reason);
            assert!(d.is_sure());
        }
    }

    #[test]
    fn ps1_by_licence_text_when_the_pvd_is_silent() {
        let dir = TestDir::new("lic");
        let bytes = disc(&[(4, b"          Licensed  by          Sony Computer Entertainment Inc.")], 20, 2);
        dir.write("a.bin", &bytes);
        let cue = dir.write("a.cue", cue_for("a.bin").as_bytes());
        assert_eq!(detect_path(&cue).system, "ps1");
    }

    #[test]
    fn pc_engine_cd_is_found_on_track_two() {
        let dir = TestDir::new("pce");
        let audio = vec![0u8; 2352 * 4];
        let mut data_user = vec![0u8; 64];
        data_user[0x20..0x20 + 23].copy_from_slice(b"PC Engine CD-ROM SYSTEM");
        dir.write("t1.bin", &audio);
        dir.write("t2.bin", &disc(&[(1, &data_user)], 4, 1));
        let cue = dir.write(
            "pce.cue",
            b"FILE \"t1.bin\" BINARY\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\nFILE \"t2.bin\" BINARY\n TRACK 02 MODE1/2352\n  INDEX 01 00:00:00\n",
        );
        let d = detect_path(&cue);
        assert_eq!(d.system, "pcecd", "{}", d.reason);
    }

    #[test]
    fn psp_iso_by_root_folder() {
        let dir = TestDir::new("psp");
        let mut iso = vec![0u8; 2048 * 24];
        iso[16 * 2048..17 * 2048].copy_from_slice(&pvd("", 22));
        let root = 22 * 2048;
        iso[root + 40..root + 48].copy_from_slice(b"PSP_GAME");
        let path = dir.write("game.iso", &iso);
        let d = detect_path(&path);
        assert_eq!(d.system, "psp", "{}", d.reason);
    }

    #[test]
    fn gamecube_and_wii_magic() {
        let dir = TestDir::new("gc");
        let mut gc = vec![0u8; 4096];
        gc[0x1C..0x20].copy_from_slice(&[0xC2, 0x33, 0x9F, 0x3D]);
        assert_eq!(detect_path(&dir.write("g.iso", &gc)).system, "gamecube");
        let mut wii = vec![0u8; 4096];
        wii[0x18..0x1C].copy_from_slice(&[0x5D, 0x1C, 0x9E, 0xA3]);
        assert_eq!(detect_path(&dir.write("w.iso", &wii)).system, "wii");
    }

    #[test]
    fn unknown_disc_asks_with_candidates() {
        let dir = TestDir::new("unk");
        dir.write("x.bin", &disc(&[], 20, 1));
        let cue = dir.write("x.cue", cue_for("x.bin").as_bytes());
        let d = detect_path(&cue);
        assert!(!d.is_sure());
        assert!(d.candidates.contains(&"ps1".to_string()));
        let missing = dir.write("y.cue", cue_for("nothere.bin").as_bytes());
        assert!(detect_path(&missing).reason.contains("nothere.bin"));
    }

    #[test]
    fn gdi_and_cdi_are_dreamcast_and_referenced_files() {
        let dir = TestDir::new("gdi");
        let gdi = dir.write("g.gdi", b"3\n1 0 4 2352 track01.bin 0\n2 600 0 2352 \"track 02.raw\" 0\n3 45000 4 2352 track03.bin 0\n");
        assert_eq!(detect_path(&gdi).system, "dreamcast");
        assert_eq!(referenced_files(&gdi), vec!["track01.bin", "track 02.raw", "track03.bin"]);
        assert_eq!(detect_path(&dir.write("c.cdi", b"x")).system, "dreamcast");
    }

    #[test]
    fn cartridge_bins() {
        let dir = TestDir::new("cart");
        let mut md = vec![0u8; 0x400];
        md[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
        assert_eq!(detect_path(&dir.write("md.bin", &md)).system, "genesis");
        let mut x = vec![0u8; 0x400];
        x[0x100..0x108].copy_from_slice(b"SEGA 32X");
        assert_eq!(detect_path(&dir.write("x.bin", &x)).system, "sega32x");
        let mut a78 = vec![0u8; 0x1000 + 128];
        a78[1..10].copy_from_slice(b"ATARI7800");
        assert_eq!(detect_path(&dir.write("a.bin", &a78)).system, "atari7800");
        let d = detect_path(&dir.write("pong.bin", &vec![0xEAu8; 4096]));
        assert_eq!(d.system, "atari2600");
        assert!(!d.is_sure(), "2600 by size alone must ask");
        let d = detect_path(&dir.write("odd.bin", &vec![1u8; 5000]));
        assert!(d.system.is_empty());
    }

    #[test]
    fn bin_named_by_a_cue_follows_the_cue() {
        let dir = TestDir::new("bincue");
        dir.write("Game (Track 1).bin", &disc(&[(0, b"SEGA SEGASATURN ")], 20, 1));
        dir.write("Game.cue", cue_for("Game (Track 1).bin").as_bytes());
        assert_eq!(detect_path(&dir.path().join("Game (Track 1).bin")).system, "saturn");
    }

    #[test]
    fn m3u_follows_first_disc_and_pbp_split() {
        let dir = TestDir::new("m3u");
        dir.write("d1.bin", &disc(&[(16, &pvd("PLAYSTATION", 22))], 24, 2));
        dir.write("d1.cue", cue_for("d1.bin").as_bytes());
        let m3u = dir.write("set.m3u", b"#EXTM3U\nd1.cue\nd2.cue\n");
        assert_eq!(detect_path(&m3u).system, "ps1");
        assert_eq!(referenced_files(&m3u), vec!["d1.cue", "d2.cue"]);

        let mut pbp = vec![0u8; 0x200];
        pbp[..4].copy_from_slice(b"\0PBP");
        pbp[0x24..0x28].copy_from_slice(&0x100u32.to_le_bytes());
        pbp[0x100..0x10C].copy_from_slice(b"PSISOIMG0000");
        assert_eq!(detect_path(&dir.write("e.pbp", &pbp)).system, "ps1");
        pbp[0x100..0x10C].copy_from_slice(b"\0\0\0\0\0\0\0\0\0\0\0\0");
        assert_eq!(detect_path(&dir.write("f.pbp", &pbp)).system, "psp");
    }

    #[test]
    fn ccd_reads_the_img_beside_it() {
        let dir = TestDir::new("ccd");
        dir.write("g.img", &disc(&[(0, b"SEGADISCSYSTEM  ")], 20, 1));
        let ccd = dir.write("g.ccd", b"[CloneCD]\nVersion=3\n[TRACK 1]\nMODE=1\nINDEX 1=0\n");
        assert_eq!(detect_path(&ccd).system, "segacd");
        assert_eq!(referenced_files(&ccd), vec!["g.img", "g.sub"]);
    }

    /// An uncompressed CHD v5 of 2448-byte CD frames with one metadata entry.
    fn chd_v5(frames: &[Vec<u8>], meta_tag: &[u8; 4], meta: &str) -> Vec<u8> {
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
        let meta_bytes = {
            let mut m = Vec::new();
            m.extend_from_slice(meta_tag);
            let data = format!("{meta}\0");
            let len = data.len() as u32;
            m.push(0x01);
            m.extend_from_slice(&len.to_be_bytes()[1..]);
            m.extend_from_slice(&0u64.to_be_bytes());
            m.extend_from_slice(data.as_bytes());
            m
        };
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

    #[test]
    fn chd_cd_is_decompressed_and_read_and_gdrom_is_dreamcast() {
        let dir = TestDir::new("chd");
        let mut frames: Vec<Vec<u8>> = (0..24).map(|_| raw_sector(&[], 2)).collect();
        frames[16] = raw_sector(&pvd("PLAYSTATION", 22), 2);
        let bytes = chd_v5(&frames, b"CHT2", "TRACK:1 TYPE:MODE2_RAW SUBTYPE:NONE FRAMES:24 PREGAP:0 PGTYPE:MODE1 PGSUB:RW POSTGAP:0");
        let d = detect_path(&dir.write("game.chd", &bytes));
        assert_eq!(d.system, "ps1", "{}", d.reason);
        let gd = chd_v5(&frames, b"CHGD", "TRACK:1 TYPE:MODE1_RAW SUBTYPE:NONE FRAMES:24 PAD:0 PREGAP:0 PGTYPE:MODE1 PGSUB:RW POSTGAP:0");
        assert_eq!(detect_path(&dir.write("dc.chd", &gd)).system, "dreamcast");
        assert!(detect_path(&dir.write("bad.chd", b"MComprHD nope")).system.is_empty());
    }

    #[test]
    fn cht2_parsing() {
        let t = parse_cht2("TRACK:2 TYPE:MODE1_RAW SUBTYPE:NONE FRAMES:4000 PREGAP:150 PGTYPE:VMODE1_RAW PGSUB:RW POSTGAP:0\0").unwrap();
        assert_eq!((t.number, t.kind.as_str(), t.frames, t.pregap, t.pregap_stored), (2, "MODE1_RAW", 4000, 150, true));
        assert!(parse_cht2("garbage").is_none());
    }

    #[test]
    fn archive_classification() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let d = classify_archive("Doom", &s(&["DOOM/DOOM.EXE", "DOOM/DOOM1.WAD"]));
        assert_eq!((d.system.as_str(), d.keep_archive), ("dos", true));
        let d = classify_archive("Turrican", &s(&["Turrican.adf"]));
        assert_eq!((d.system.as_str(), d.keep_archive), ("amiga", true));
        let d = classify_archive("mslug", &s(&["201-p1.p1", "201-s1.s1"]));
        assert_eq!((d.system.as_str(), d.keep_archive), ("arcade", true));
        let d = classify_archive("unknownset", &s(&["a-1.1a", "a-2.2b", "a-3.u12", "prom.6f"]));
        assert_eq!((d.system.as_str(), d.keep_archive), ("arcade", true));
        let d = classify_archive("Zelda", &s(&["Zelda.sfc", "readme.txt"]));
        assert_eq!((d.system.as_str(), d.keep_archive, d.is_sure()), ("snes", false, true));
        let d = classify_archive("FF7", &s(&["FF7.m3u", "FF7 (Disc 1).cue", "FF7 (Disc 1).bin"]));
        assert!(d.system.is_empty() && !d.keep_archive);
        let d = classify_archive("pack", &s(&["a.gba", "b.gba", "__MACOSX/._a.gba"]));
        assert_eq!(d.system, "gba");
        let d = classify_archive("mystery", &s(&["data.dat"]));
        assert!(d.system.is_empty());
    }

    #[test]
    fn unique_extensions_are_sure() {
        let dir = TestDir::new("ext");
        let d = detect_path(&dir.write("x.sfc", b"x"));
        assert_eq!(d.system, "snes");
        assert!(d.is_sure());
        assert_eq!(system_for_extension(".ws"), Some("wswan"));
        assert_eq!(system_for_extension("cue"), None);
    }
}
