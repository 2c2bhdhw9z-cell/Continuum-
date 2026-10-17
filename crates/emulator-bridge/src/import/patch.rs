//! Game patches: fan translations, hacks and fixes in the three formats players actually trade,
//! IPS, BPS and UPS (owner, 16 October 2026: Manic has a ROM patcher, so Continuum needs one).
//!
//! Continuum's own implementation, written from the formats themselves:
//!
//! - **IPS**: "PATCH", then records of a 3-byte offset and a 2-byte length followed by that many
//!   bytes, or a length of 0 followed by a 2-byte run and one byte to repeat; "EOF" ends it,
//!   optionally followed by a 3-byte size to cut the result to. It carries no checksum, so the only
//!   check is that it is well formed.
//! - **UPS**: "UPS1", the input and output sizes as variable-length numbers, then hunks of "skip
//!   this many bytes, then XOR these bytes in up to a zero byte", and 12 bytes of CRC32: input,
//!   output, and the patch itself. It works both ways: applied to its output it gives back its
//!   input.
//! - **BPS**: "BPS1", the two sizes, a metadata block, then commands that build the output from
//!   the input (same place), from the patch, or by copying from anywhere in the input or in the
//!   output so far, and the same three CRC32s.
//!
//! The patch is always applied to a COPY: the caller names a new file, and the original game is
//! never touched. A BPS or UPS patch made for the other version of a game is refused by its
//! checksum, with one plain line, instead of producing a game that crashes. A SNES game with a
//! 512-byte copier header is tried without it when the checksum says so.

use std::path::Path;

/// What applying a patch did, for the line the player reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patched {
    pub format: &'static str,
    pub bytes: usize,
    /// True when a SNES copier header had to come off for the checksum to match.
    pub header_removed: bool,
}

/// Why a patch could not be applied, in the player's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchError {
    /// Not IPS, BPS or UPS.
    UnknownFormat,
    /// Cut short or self-contradictory, or its own checksum is wrong.
    Damaged,
    /// Made for a different version of the game (its input checksum doesn't match).
    WrongGame,
    Io(String),
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownFormat => {
                write!(f, "That isn't a patch Continuum can use. It takes .ips, .bps and .ups files.")
            }
            Self::Damaged => write!(f, "That patch file looks damaged. Try downloading it again."),
            Self::WrongGame => write!(
                f,
                "That patch is for a different version of this game. Check the patch's notes for \
                 the version it needs."
            ),
            Self::Io(reason) => write!(f, "The patch couldn't be applied: {reason}"),
        }
    }
}

/// CRC-32 (IEEE, the zip one), the checksum BPS and UPS carry.
pub fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for (i, slot) in table.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        table
    });
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc = table[((crc ^ byte as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// Which format a patch is, from its first bytes.
pub fn format_of(patch: &[u8]) -> Option<&'static str> {
    if patch.starts_with(b"PATCH") {
        Some("IPS")
    } else if patch.starts_with(b"BPS1") {
        Some("BPS")
    } else if patch.starts_with(b"UPS1") {
        Some("UPS")
    } else {
        None
    }
}

/// Applies `patch` to `rom` and returns the patched game.
pub fn apply(rom: &[u8], patch: &[u8]) -> Result<(Vec<u8>, Patched), PatchError> {
    let format = format_of(patch).ok_or(PatchError::UnknownFormat)?;
    let (out, header_removed) = match format {
        "IPS" => (apply_ips(rom, patch)?, false),
        "BPS" => with_header_retry(rom, |source: &[u8]| apply_bps(source, patch))?,
        _ => with_header_retry(rom, |source: &[u8]| apply_ups(source, patch))?,
    };
    let bytes = out.len();
    Ok((out, Patched { format, bytes, header_removed }))
}

/// Runs a checked patch, and when the checksum says it is for another game and the file has a
/// SNES copier header (512 bytes over a whole number of kilobytes), once more without the header.
fn with_header_retry(
    rom: &[u8],
    run: impl Fn(&[u8]) -> Result<Vec<u8>, PatchError>,
) -> Result<(Vec<u8>, bool), PatchError> {
    match run(rom) {
        Err(PatchError::WrongGame) if rom.len() % 1024 == 512 => run(&rom[512..]).map(|out| (out, true)),
        other => other.map(|out| (out, false)),
    }
}

/// Reads both files, applies the patch and writes the result to `out_path`, a new file. Refuses
/// to write over anything that is already there, so the original game can never be replaced.
pub fn apply_files(rom_path: &Path, patch_path: &Path, out_path: &Path) -> Result<Patched, PatchError> {
    if out_path.exists() {
        return Err(PatchError::Io(format!("{} already exists", out_path.display())));
    }
    let rom = std::fs::read(rom_path).map_err(|e| PatchError::Io(e.to_string()))?;
    let patch = std::fs::read(patch_path).map_err(|e| PatchError::Io(e.to_string()))?;
    let (out, patched) = apply(&rom, &patch)?;
    std::fs::write(out_path, out).map_err(|e| PatchError::Io(e.to_string()))?;
    Ok(patched)
}

// ------------------------------------------------------------------ IPS

/// The largest output an IPS patch may produce: its offsets are 3 bytes, so 16 MB plus one
/// record. Anything claiming more is damaged.
const IPS_LIMIT: usize = 0x0100_0000 + 0x1_0000;

fn apply_ips(rom: &[u8], patch: &[u8]) -> Result<Vec<u8>, PatchError> {
    let mut out = rom.to_vec();
    let mut at = 5;
    let byte = |i: usize| patch.get(i).copied().ok_or(PatchError::Damaged);
    loop {
        if patch.get(at..at + 3) == Some(b"EOF") {
            at += 3;
            break;
        }
        let offset = (byte(at)? as usize) << 16 | (byte(at + 1)? as usize) << 8 | byte(at + 2)? as usize;
        let size = (byte(at + 3)? as usize) << 8 | byte(at + 4)? as usize;
        at += 5;
        let (run, value, literal) = if size == 0 {
            let run = (byte(at)? as usize) << 8 | byte(at + 1)? as usize;
            let value = byte(at + 2)?;
            at += 3;
            (run, value, None)
        } else {
            let data = patch.get(at..at + size).ok_or(PatchError::Damaged)?;
            at += size;
            (size, 0, Some(data))
        };
        let end = offset + run;
        if end > IPS_LIMIT {
            return Err(PatchError::Damaged);
        }
        if out.len() < end {
            out.resize(end, 0);
        }
        match literal {
            Some(data) => out[offset..end].copy_from_slice(data),
            None => out[offset..end].fill(value),
        }
    }
    // The optional size to cut the result to (Lunar IPS's extension).
    if let Some(cut) = patch.get(at..at + 3) {
        let size = (cut[0] as usize) << 16 | (cut[1] as usize) << 8 | cut[2] as usize;
        if size <= out.len() {
            out.truncate(size);
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ BPS and UPS

/// The variable-length number both formats use: seven bits a byte, the high bit ends it, and
/// each further byte adds one more step so no number has two spellings.
fn read_number(patch: &[u8], at: &mut usize) -> Result<u64, PatchError> {
    let mut value: u64 = 0;
    let mut shift: u64 = 1;
    loop {
        let x = *patch.get(*at).ok_or(PatchError::Damaged)?;
        *at += 1;
        value = value
            .checked_add((x as u64 & 0x7F).checked_mul(shift).ok_or(PatchError::Damaged)?)
            .ok_or(PatchError::Damaged)?;
        if x & 0x80 != 0 {
            return Ok(value);
        }
        shift = shift.checked_shl(7).ok_or(PatchError::Damaged)?;
        value = value.checked_add(shift).ok_or(PatchError::Damaged)?;
    }
}

/// The three checksums at the end of a BPS or UPS patch: input, output, patch.
fn footer(patch: &[u8]) -> Result<(u32, u32), PatchError> {
    if patch.len() < 4 + 12 {
        return Err(PatchError::Damaged);
    }
    let word = |i: usize| u32::from_le_bytes([patch[i], patch[i + 1], patch[i + 2], patch[i + 3]]);
    let end = patch.len();
    if crc32(&patch[..end - 4]) != word(end - 4) {
        return Err(PatchError::Damaged);
    }
    Ok((word(end - 12), word(end - 8)))
}

/// Output sizes are capped at 1 GB: a number past that is a damaged patch, not a game.
const SIZE_LIMIT: u64 = 1 << 30;

fn apply_bps(source: &[u8], patch: &[u8]) -> Result<Vec<u8>, PatchError> {
    let (source_crc, target_crc) = footer(patch)?;
    let mut at = 4;
    let source_size = read_number(patch, &mut at)?;
    let target_size = read_number(patch, &mut at)?;
    let metadata = read_number(patch, &mut at)?;
    if target_size > SIZE_LIMIT || metadata > patch.len() as u64 {
        return Err(PatchError::Damaged);
    }
    at += metadata as usize;
    if source_size != source.len() as u64 || crc32(source) != source_crc {
        return Err(PatchError::WrongGame);
    }
    let end = patch.len() - 12;
    let mut out = vec![0u8; target_size as usize];
    let mut write = 0usize;
    let mut source_at: i64 = 0;
    let mut target_at: i64 = 0;
    while at < end {
        let data = read_number(patch, &mut at)?;
        let command = data & 3;
        let length = ((data >> 2) + 1) as usize;
        if write + length > out.len() {
            return Err(PatchError::Damaged);
        }
        match command {
            // From the input, at the same place.
            0 => {
                let from = source.get(write..write + length).ok_or(PatchError::Damaged)?;
                out[write..write + length].copy_from_slice(from);
            }
            // From the patch.
            1 => {
                let from = patch.get(at..at + length).ok_or(PatchError::Damaged)?;
                if at + length > end {
                    return Err(PatchError::Damaged);
                }
                out[write..write + length].copy_from_slice(from);
                at += length;
            }
            // From anywhere in the input.
            2 => {
                let step = read_number(patch, &mut at)?;
                let delta = (step >> 1) as i64;
                source_at += if step & 1 != 0 { -delta } else { delta };
                let from = usize::try_from(source_at).map_err(|_| PatchError::Damaged)?;
                let slice = source.get(from..from + length).ok_or(PatchError::Damaged)?;
                out[write..write + length].copy_from_slice(slice);
                source_at += length as i64;
            }
            // From the output so far, one byte at a time: the copy may overlap itself.
            _ => {
                let step = read_number(patch, &mut at)?;
                let delta = (step >> 1) as i64;
                target_at += if step & 1 != 0 { -delta } else { delta };
                for i in 0..length {
                    let from = usize::try_from(target_at).map_err(|_| PatchError::Damaged)?;
                    if from >= write + i {
                        return Err(PatchError::Damaged);
                    }
                    out[write + i] = out[from];
                    target_at += 1;
                }
            }
        }
        write += length;
    }
    if write != out.len() || crc32(&out) != target_crc {
        return Err(PatchError::Damaged);
    }
    Ok(out)
}

fn apply_ups(input: &[u8], patch: &[u8]) -> Result<Vec<u8>, PatchError> {
    let (crc_a, crc_b) = footer(patch)?;
    let mut at = 4;
    let size_a = read_number(patch, &mut at)?;
    let size_b = read_number(patch, &mut at)?;
    if size_a > SIZE_LIMIT || size_b > SIZE_LIMIT {
        return Err(PatchError::Damaged);
    }
    let input_crc = crc32(input);
    // UPS runs both ways: on its input it gives its output, and on its output its input.
    let (out_size, out_crc) = if input.len() as u64 == size_a && input_crc == crc_a {
        (size_b, crc_b)
    } else if input.len() as u64 == size_b && input_crc == crc_b {
        (size_a, crc_a)
    } else {
        return Err(PatchError::WrongGame);
    };
    let mut out = vec![0u8; out_size as usize];
    let shared = input.len().min(out.len());
    out[..shared].copy_from_slice(&input[..shared]);
    let end = patch.len() - 12;
    let mut write = 0usize;
    while at < end {
        let skip = read_number(patch, &mut at)?;
        write = write.checked_add(usize::try_from(skip).map_err(|_| PatchError::Damaged)?)
            .ok_or(PatchError::Damaged)?;
        loop {
            let x = *patch.get(at).ok_or(PatchError::Damaged)?;
            at += 1;
            if at > end {
                return Err(PatchError::Damaged);
            }
            if x == 0 {
                write += 1;
                break;
            }
            if write < out.len() {
                let original = input.get(write).copied().unwrap_or(0);
                out[write] = original ^ x;
            }
            write += 1;
        }
    }
    if crc32(&out) != out_crc {
        return Err(PatchError::Damaged);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let x = (value & 0x7F) as u8;
            value >>= 7;
            if value == 0 {
                out.push(0x80 | x);
                return out;
            }
            out.push(x);
            value -= 1;
        }
    }

    fn with_footer(mut body: Vec<u8>, input: &[u8], output: &[u8]) -> Vec<u8> {
        body.extend_from_slice(&crc32(input).to_le_bytes());
        body.extend_from_slice(&crc32(output).to_le_bytes());
        let own = crc32(&body);
        body.extend_from_slice(&own.to_le_bytes());
        body
    }

    #[test]
    fn crc32_is_the_zip_one() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn numbers_read_back() {
        for value in [0u64, 1, 127, 128, 255, 16_383, 16_384, 1 << 24, 123_456_789] {
            let bytes = number(value);
            let mut at = 0;
            assert_eq!(read_number(&bytes, &mut at).unwrap(), value);
            assert_eq!(at, bytes.len());
        }
    }

    #[test]
    fn ips_records_runs_growth_and_the_cut() {
        let rom = vec![0u8; 16];
        let mut patch = b"PATCH".to_vec();
        patch.extend_from_slice(&[0, 0, 2, 0, 3, 0xAA, 0xBB, 0xCC]); // 3 bytes at 2
        patch.extend_from_slice(&[0, 0, 8, 0, 0, 0, 4, 0x11]); // four 0x11 at 8
        patch.extend_from_slice(&[0, 0, 18, 0, 2, 0x55, 0x66]); // grows the file to 20
        patch.extend_from_slice(b"EOF");
        let (out, done) = apply(&rom, &patch).unwrap();
        assert_eq!(done.format, "IPS");
        assert_eq!(&out[2..5], &[0xAA, 0xBB, 0xCC]);
        assert_eq!(&out[8..12], &[0x11; 4]);
        assert_eq!(out.len(), 20);
        assert_eq!(&out[18..20], &[0x55, 0x66]);
        let mut cut = patch.clone();
        cut.extend_from_slice(&[0, 0, 10]);
        assert_eq!(apply(&rom, &cut).unwrap().0.len(), 10);
        // Cut short in the middle of a record.
        assert_eq!(apply(&rom, &patch[..12]).unwrap_err(), PatchError::Damaged);
    }

    #[test]
    fn bps_uses_all_four_commands_and_checks_the_game() {
        let source: Vec<u8> = (0..32u8).collect();
        // Output: source[0..8], "HELLO", source[20..24], then the output's last two bytes twice,
        // copied from the output itself with the copy overlapping what it writes.
        let mut target = source[0..8].to_vec();
        target.extend_from_slice(b"HELLO");
        target.extend_from_slice(&source[20..24]);
        target.extend_from_slice(&[source[22], source[23], source[22], source[23]]);
        let mut body = b"BPS1".to_vec();
        body.extend(number(source.len() as u64));
        body.extend(number(target.len() as u64));
        body.extend(number(3));
        body.extend_from_slice(b"abc");
        body.extend(number((8 - 1) << 2)); // SourceRead 8
        body.extend(number(((5 - 1) << 2) | 1)); // TargetRead 5
        body.extend_from_slice(b"HELLO");
        body.extend(number(((4 - 1) << 2) | 2)); // SourceCopy 4 from 20
        body.extend(number(20 << 1));
        body.extend(number(((4 - 1) << 2) | 3)); // TargetCopy 4 from 15, overlapping
        body.extend(number(15 << 1));
        let patch = with_footer(body, &source, &target);
        let (out, done) = apply(&source, &patch).unwrap();
        assert_eq!(out, target);
        assert_eq!(done.format, "BPS");
        assert!(!done.header_removed);

        let mut other = source.clone();
        other[0] = 99;
        assert_eq!(apply(&other, &patch).unwrap_err(), PatchError::WrongGame);

        // A SNES copier header in front: taken off, and said so.
        let big: Vec<u8> = (0..1024u32).map(|i| (i % 251) as u8).collect();
        let mut body = b"BPS1".to_vec();
        body.extend(number(1024));
        body.extend(number(1024));
        body.extend(number(0));
        body.extend(number((1024 - 1) << 2));
        let plain = with_footer(body, &big, &big);
        let mut with_header = vec![0u8; 512];
        with_header.extend_from_slice(&big);
        let (out, done) = apply(&with_header, &plain).unwrap();
        assert_eq!(out, big);
        assert!(done.header_removed);

        let mut broken = patch.clone();
        broken[6] ^= 1;
        assert_eq!(apply(&source, &broken).unwrap_err(), PatchError::Damaged);
    }

    #[test]
    fn ups_works_both_ways() {
        let a: Vec<u8> = (0..20u8).collect();
        let mut b = a.clone();
        b[3] = 0xF0;
        b[4] = 0xF1;
        b.extend_from_slice(&[7, 8]);
        let mut body = b"UPS1".to_vec();
        body.extend(number(a.len() as u64));
        body.extend(number(b.len() as u64));
        body.extend(number(3)); // skip to 3
        body.extend_from_slice(&[a[3] ^ b[3], a[4] ^ b[4], 0]);
        body.extend(number(14)); // 5 + 1 (the zero) + 14 = 20
        body.extend_from_slice(&[7, 8, 0]);
        let patch = with_footer(body, &a, &b);
        let (out, done) = apply(&a, &patch).unwrap();
        assert_eq!(done.format, "UPS");
        assert_eq!(out, b);
        assert_eq!(apply(&b, &patch).unwrap().0, a, "and back");
        assert_eq!(apply(&[1, 2, 3], &patch).unwrap_err(), PatchError::WrongGame);
    }

    #[test]
    fn anything_else_is_refused_and_files_are_never_overwritten() {
        assert_eq!(apply(b"rom", b"not a patch").unwrap_err(), PatchError::UnknownFormat);
        let dir = std::env::temp_dir().join(format!("continuum-patch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let rom = dir.join("game.gba");
        let ips = dir.join("fix.ips");
        let out = dir.join("game (fix).gba");
        std::fs::write(&rom, [0u8; 8]).unwrap();
        let mut patch = b"PATCH".to_vec();
        patch.extend_from_slice(&[0, 0, 1, 0, 1, 9]);
        patch.extend_from_slice(b"EOF");
        std::fs::write(&ips, &patch).unwrap();
        apply_files(&rom, &ips, &out).unwrap();
        assert_eq!(std::fs::read(&out).unwrap()[1], 9);
        assert_eq!(std::fs::read(&rom).unwrap(), vec![0u8; 8], "the original is untouched");
        assert!(matches!(apply_files(&rom, &ips, &out), Err(PatchError::Io(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
