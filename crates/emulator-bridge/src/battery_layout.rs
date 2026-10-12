//! Battery saves in the shape each core expects, and the save files other cores left behind.
//!
//! Pure functions; `bridge.rs` calls them on the way into a core (`restore_battery_save`) and on
//! the way out (`battery_save`), so the file on disk keeps one layout whichever core plays it.
//!
//! - **Padding.** A file longer than the save RAM whose extra bytes are all `0xFF` (Genesis Plus
//!   GX's 64 KB `.srm` written before a game's first frame, a RetroArch file) is cut to size. For
//!   bsnes an all-`0x00` tail is cut as well, the padding other SNES emulators write.
//! - **Mega Drive cartridge SRAM on BlastEm.** Genesis Plus GX (and so every Mega Drive save made
//!   before the licence swap) writes a 64 KB window indexed by the 68K address, `a & 0xFFFF`.
//!   BlastEm keeps only the bytes the cartridge really has: odd bytes, even bytes, or 16-bit
//!   words in host order. The file stays in the Genesis Plus GX layout, so it still syncs with a
//!   phone on an older build and still loads in RetroArch; the engine converts at the boundary.
//! - **Files other cores wrote.** Mesen 2 kept NES saves as `<game>.sav` in the system folder,
//!   and Genesis Plus GX kept the Sega CD's backup RAM as `scd_U/E/J.brm` there. Those are the
//!   candidates a first load can pick up.

use std::path::{Path, PathBuf};

/// Which bytes of the cartridge's SRAM window the cartridge actually has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SramLane {
    /// Odd 68K addresses only (header flags `$18`).
    Odd,
    /// Even 68K addresses only (`$10`).
    Even,
    /// Both: 16-bit SRAM (`$00`).
    Word,
    /// No `RA` header: an EEPROM cartridge BlastEm knows from its own list, linear from 0.
    Linear,
}

/// A Mega Drive cartridge's save RAM, as its header describes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MdSram {
    pub lane: SramLane,
    /// First 68K address, even (`BE32($1B4) & $00FFFFFE`).
    pub start: u32,
    /// Last 68K address, odd (`BE32($1B8) | 1`).
    pub end: u32,
}

/// How the engine shapes a battery save for the running core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BatteryLayout {
    /// Byte for byte, with only the padding rule.
    #[default]
    Raw,
    /// BlastEm on a Mega Drive cartridge: Genesis Plus GX's layout on disk.
    MegaDriveGpgx(MdSram),
}

/// The Genesis Plus GX window: `a & 0xFFFF`.
const GPGX_WINDOW: usize = 0x10000;
/// With no header, a save this size or smaller is an EEPROM laid out from 0 in both cores.
const LINEAR_MAX: usize = 0x2000;

/// An `.smd` file (a 512-byte header, then 16 KB blocks with odd bytes first) as plain ROM bytes.
/// Applies when the extension is `smd` or bytes 8-9 are `AA BB`; anything else comes back as is.
pub fn deinterleave_smd(ext: &str, file: &[u8]) -> Vec<u8> {
    let is_smd = ext.eq_ignore_ascii_case("smd") || (file.len() > 9 && file[8] == 0xAA && file[9] == 0xBB);
    if !is_smd || file.len() < 0x200 + 0x4000 {
        return file.to_vec();
    }
    let body = &file[0x200..];
    let mut out = vec![0u8; body.len() / 0x4000 * 0x4000];
    for (block, chunk) in body.chunks_exact(0x4000).enumerate() {
        let o = block * 0x4000;
        for i in 0..0x2000 {
            out[o + 2 * i + 1] = chunk[i];
            out[o + 2 * i] = chunk[0x2000 + i];
        }
    }
    out
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// The SRAM a Mega Drive header declares at `$1B0`, or `None` with no `RA` there (or flags no
/// cartridge uses).
pub fn md_sram_from_header(head: &[u8]) -> Option<MdSram> {
    if head.len() < 0x1BC || &head[0x1B0..0x1B2] != b"RA" {
        return None;
    }
    let lane = match head[0x1B2] & 0x18 {
        0x18 => SramLane::Odd,
        0x10 => SramLane::Even,
        0x00 => SramLane::Word,
        _ => return None,
    };
    let start = be32(&head[0x1B4..]) & 0x00FF_FFFE;
    let end = be32(&head[0x1B8..]) | 1;
    if end <= start {
        return None;
    }
    Some(MdSram { lane, start, end })
}

/// The layout for `core_id` running a file with extension `ext` whose first bytes are `head`
/// (already de-interleaved). Only BlastEm on a Mega Drive cartridge converts; a 32X cartridge, a
/// Sega CD disc and every other core keep their bytes.
pub fn layout_for(core_id: &str, ext: &str, head: &[u8]) -> BatteryLayout {
    if core_id != "blastem" || !matches!(ext.to_ascii_lowercase().as_str(), "md" | "gen" | "smd" | "bin") {
        return BatteryLayout::Raw;
    }
    if head.len() < 0x110 {
        return BatteryLayout::Raw;
    }
    let console = &head[0x100..0x110];
    let says = |needle: &[u8]| console.windows(needle.len()).any(|w| w == needle);
    if !says(b"SEGA") || says(b"32X") {
        return BatteryLayout::Raw;
    }
    match md_sram_from_header(head) {
        // BlastEm has no SRAM there at all (a mapper register range), so nothing to convert.
        Some(s) if s.start >= 0x80_0000 => BatteryLayout::Raw,
        // Wider than Genesis Plus GX's window: GPGX itself aliased it, so keep BlastEm's bytes.
        Some(s) if (s.end - s.start + 1) as usize > GPGX_WINDOW => BatteryLayout::Raw,
        Some(s) => BatteryLayout::MegaDriveGpgx(s),
        None if has_ra(head) => BatteryLayout::Raw,
        None => BatteryLayout::MegaDriveGpgx(MdSram { lane: SramLane::Linear, start: 0, end: 0 }),
    }
}

fn has_ra(head: &[u8]) -> bool {
    head.len() >= 0x1B2 && &head[0x1B0..0x1B2] == b"RA"
}

/// The 68K addresses of BlastEm's live bytes, in order. `None` for `Linear`.
fn live_addresses(sram: &MdSram) -> Option<Vec<u32>> {
    let span = sram.end - sram.start + 1;
    Some(match sram.lane {
        SramLane::Odd => (0..span / 2).map(|k| sram.start + 1 + 2 * k).collect(),
        SramLane::Even => (0..span / 2).map(|k| sram.start + 2 * k).collect(),
        // Little-endian host words: the low byte is the odd (second) 68K byte.
        SramLane::Word => (0..span / 2).flat_map(|k| [sram.start + 2 * k + 1, sram.start + 2 * k]).collect(),
        SramLane::Linear => return None,
    })
}

/// A Genesis Plus GX battery file as BlastEm's live save RAM of `live_size` bytes. Refused when
/// the result would not be exactly that size, so the file is never half-loaded.
pub fn gpgx_to_live(file: &[u8], sram: &MdSram, live_size: usize) -> Result<Vec<u8>, String> {
    let out = match live_addresses(sram) {
        Some(addrs) => {
            let g = |a: u32| file.get(a as usize & 0xFFFF).copied().unwrap_or(0xFF);
            addrs.into_iter().map(g).collect::<Vec<u8>>()
        }
        None => {
            if live_size > LINEAR_MAX {
                return Err(format!(
                    "this cartridge's save is {live_size} bytes with no size in its header, so it can't be matched up"
                ));
            }
            if file.len() > live_size && file[live_size..].iter().any(|b| *b != 0xFF) {
                return Err(format!(
                    "that save is {} bytes and this game's save is {live_size}",
                    file.len()
                ));
            }
            let mut out = file[..file.len().min(live_size)].to_vec();
            out.resize(live_size, 0xFF);
            out
        }
    };
    if out.len() != live_size {
        return Err(format!(
            "that save is laid out for {} bytes and this game's save is {live_size}",
            out.len()
        ));
    }
    Ok(out)
}

/// BlastEm's live save RAM back as a Genesis Plus GX file: `(end & 0xFFFF) + 1` bytes, `0xFF`
/// where the cartridge has no byte. The exact inverse of [`gpgx_to_live`].
pub fn live_to_gpgx(live: &[u8], sram: &MdSram) -> Vec<u8> {
    let Some(addrs) = live_addresses(sram) else {
        return live.to_vec();
    };
    let len = addrs.iter().map(|a| (*a as usize & 0xFFFF) + 1).max().unwrap_or(0);
    let mut out = vec![0xFF; len.max((sram.end as usize & 0xFFFF) + 1)];
    for (byte, a) in live.iter().zip(addrs) {
        out[a as usize & 0xFFFF] = *byte;
    }
    out
}

/// `data` cut to `size` when everything past `size` is padding: `0xFF` for any core, or `0x00`
/// for bsnes. Anything else comes back unchanged, so the caller's size rules still apply.
pub fn fit_padded<'a>(data: &'a [u8], size: usize, core_id: &str) -> &'a [u8] {
    if data.len() <= size || size == 0 {
        return data;
    }
    let tail = &data[size..];
    if tail.iter().all(|b| *b == 0xFF) || (core_id == "bsnes" && tail.iter().all(|b| *b == 0x00)) {
        &data[..size]
    } else {
        data
    }
}

/// Battery files another core wrote for this game, which a first load may pick up. Never
/// written or deleted by the engine.
pub fn migration_candidates(core_id: &str, ext: &str, content_name: &str, support_dir: &Path) -> Vec<PathBuf> {
    let ext = ext.to_ascii_lowercase();
    match (core_id, ext.as_str()) {
        // Mesen 2: `BatteryManager::GetBasePath`, the ROM's file name without its extension.
        ("mesen2" | "fceumm", "nes" | "unf" | "unif") => {
            let stem = Path::new(content_name).file_stem().map(|s| s.to_string_lossy().into_owned());
            stem.filter(|s| !s.is_empty()).map(|s| vec![support_dir.join(format!("{s}.sav"))]).unwrap_or_default()
        }
        // Genesis Plus GX: one backup RAM per region in the system folder.
        ("blastem", "cue" | "chd" | "iso" | "m3u") => {
            ["scd_U.brm", "scd_E.brm", "scd_J.brm"].iter().map(|n| support_dir.join(n)).collect()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 0x200-byte Mega Drive header: "SEGA MEGA DRIVE" at $100 and an RA block.
    fn header(flags: u8, start: u32, end: u32) -> Vec<u8> {
        let mut h = vec![0u8; 0x200];
        h[0x100..0x10F].copy_from_slice(b"SEGA MEGA DRIVE");
        h[0x1B0..0x1B2].copy_from_slice(b"RA");
        h[0x1B2] = 0xA0 | flags;
        h[0x1B3] = 0x20;
        h[0x1B4..0x1B8].copy_from_slice(&start.to_be_bytes());
        h[0x1B8..0x1BC].copy_from_slice(&end.to_be_bytes());
        h
    }

    /// What Genesis Plus GX wrote for the evaluation's sramtest carts (make_sramtest.py): 0x22+i
    /// at the i-th cartridge byte in 68K address order (stride 2 for odd and even, 1 for 16-bit),
    /// 0xFF everywhere else in the window, trimmed after the last written byte.
    fn gpgx_file(sram: &MdSram, count: usize) -> Vec<u8> {
        let (first, stride) = match sram.lane {
            SramLane::Odd => (sram.start + 1, 2),
            SramLane::Even => (sram.start, 2),
            _ => (sram.start, 1),
        };
        let mut file = vec![0xFF; GPGX_WINDOW];
        for i in 0..count as u32 {
            file[(first + i * stride) as usize & 0xFFFF] = 0x22 + i as u8;
        }
        let last = file.iter().rposition(|b| *b != 0xFF).unwrap();
        file.truncate(last + 1);
        file
    }

    #[test]
    fn header_lanes_and_window() {
        let odd = md_sram_from_header(&header(0x18, 0x200001, 0x203FFF)).unwrap();
        assert_eq!(odd, MdSram { lane: SramLane::Odd, start: 0x200000, end: 0x203FFF });
        assert_eq!(md_sram_from_header(&header(0x10, 0x200000, 0x203FFE)).unwrap().lane, SramLane::Even);
        let word = md_sram_from_header(&header(0x00, 0x200000, 0x20FFFF)).unwrap();
        assert_eq!((word.lane, word.end), (SramLane::Word, 0x20FFFF));
        assert_eq!(md_sram_from_header(&header(0x08, 0x200000, 0x20FFFF)), None);
        assert_eq!(md_sram_from_header(&[0u8; 0x200]), None);
    }

    #[test]
    fn only_blastem_on_a_mega_drive_cartridge_converts() {
        let h = header(0x18, 0x200001, 0x203FFF);
        assert!(matches!(layout_for("blastem", "md", &h), BatteryLayout::MegaDriveGpgx(_)));
        assert_eq!(layout_for("gearsystem", "md", &h), BatteryLayout::Raw);
        assert_eq!(layout_for("blastem", "cue", &h), BatteryLayout::Raw);
        let mut x32 = h.clone();
        x32[0x100..0x108].copy_from_slice(b"SEGA 32X");
        assert_eq!(layout_for("blastem", "32x", &x32), BatteryLayout::Raw);
        assert_eq!(layout_for("blastem", "bin", &x32), BatteryLayout::Raw);
        let mut eeprom = h.clone();
        eeprom[0x1B0..0x1BC].fill(0x20);
        assert_eq!(
            layout_for("blastem", "gen", &eeprom),
            BatteryLayout::MegaDriveGpgx(MdSram { lane: SramLane::Linear, start: 0, end: 0 })
        );
        // A header with SRAM past the cartridge space, or wider than GPGX's window: left alone.
        assert_eq!(layout_for("blastem", "md", &header(0x00, 0xA13000, 0xA13001)), BatteryLayout::Raw);
        assert_eq!(layout_for("blastem", "md", &header(0x00, 0x200000, 0x21FFFF)), BatteryLayout::Raw);
    }

    #[test]
    fn odd_even_and_word_carts_read_as_genesis_plus_gx_wrote_them() {
        for (flags, end) in [(0x18u8, 0x203FFFu32), (0x10, 0x203FFE), (0x00, 0x203FFF)] {
            let sram = md_sram_from_header(&header(flags, 0x200000 | (flags == 0x18) as u32, end)).unwrap();
            let live_size = match sram.lane { SramLane::Word => 0x4000, _ => 0x2000 };
            let file = gpgx_file(&sram, 16);
            let live = gpgx_to_live(&file, &sram, live_size).unwrap();
            assert_eq!(live.len(), live_size);
            match sram.lane {
                // Words in host order: each pair swapped against the 68K order 22 23, 24 25...
                SramLane::Word => assert_eq!(&live[..6], &[0x23, 0x22, 0x25, 0x24, 0x27, 0x26]),
                _ => assert_eq!(&live[..4], &[0x22, 0x23, 0x24, 0x25]),
            }
            // Past what GPGX wrote, the bytes are the fresh 0xFF it would have had.
            assert_eq!(live[live_size - 1], 0xFF);
        }
    }

    #[test]
    fn word_lane_byte_order_is_host_little_endian() {
        let sram = MdSram { lane: SramLane::Word, start: 0x200000, end: 0x200003 };
        // 68K bytes: $200000=AA $200001=BB $200002=CC $200003=DD
        let live = gpgx_to_live(&[0xAA, 0xBB, 0xCC, 0xDD], &sram, 4).unwrap();
        assert_eq!(live, vec![0xBB, 0xAA, 0xDD, 0xCC]);
        assert_eq!(live_to_gpgx(&live, &sram), vec![0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn the_round_trip_is_exact_over_the_window() {
        for (lane, start, end, live_size) in [
            (SramLane::Odd, 0x200000u32, 0x203FFFu32, 0x2000usize),
            (SramLane::Even, 0x200000, 0x203FFF, 0x2000),
            (SramLane::Word, 0x200000, 0x207FFF, 0x8000),
        ] {
            let sram = MdSram { lane, start, end };
            let live: Vec<u8> = (0..live_size).map(|i| (i * 31 + 7) as u8).collect();
            let file = live_to_gpgx(&live, &sram);
            assert_eq!(file.len(), (end as usize & 0xFFFF) + 1);
            assert_eq!(gpgx_to_live(&file, &sram, live_size).unwrap(), live);
            // And a full GPGX window (0xFF where the cart has no byte) survives the other way.
            assert_eq!(live_to_gpgx(&gpgx_to_live(&file, &sram, live_size).unwrap(), &sram), file);
        }
        // Odd lane: the even bytes of the file are filler.
        let sram = MdSram { lane: SramLane::Odd, start: 0x200000, end: 0x200007 };
        assert_eq!(live_to_gpgx(&[1, 2, 3, 4], &sram), vec![0xFF, 1, 0xFF, 2, 0xFF, 3, 0xFF, 4]);
    }

    #[test]
    fn a_trimmed_genesis_plus_gx_file_and_a_wrong_size_are_handled() {
        let sram = MdSram { lane: SramLane::Odd, start: 0x200000, end: 0x203FFF };
        // GPGX trimmed after its last non-FF byte: only 3 bytes on disk.
        let live = gpgx_to_live(&[0xFF, 0x41, 0xFF, 0x42], &sram, 0x2000).unwrap();
        assert_eq!(&live[..3], &[0x41, 0x42, 0xFF]);
        assert!(live[2..].iter().all(|b| *b == 0xFF));
        // BlastEm says the RAM is another size than the header gives: refused, nothing loaded.
        assert!(gpgx_to_live(&[0x41], &sram, 0x4000).is_err());
    }

    #[test]
    fn linear_eeprom_pads_and_refuses_what_it_cannot_place() {
        let linear = MdSram { lane: SramLane::Linear, start: 0, end: 0 };
        assert_eq!(gpgx_to_live(&[1, 2, 3], &linear, 8).unwrap(), vec![1, 2, 3, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
        let mut long = vec![9u8; 128];
        long.extend_from_slice(&[0xFF; 128]);
        assert_eq!(gpgx_to_live(&long, &linear, 128).unwrap(), vec![9u8; 128]);
        long[200] = 0;
        assert!(gpgx_to_live(&long, &linear, 128).is_err());
        assert!(gpgx_to_live(&[1, 2], &linear, 0x2001).is_err());
        assert_eq!(live_to_gpgx(&[5, 6], &linear), vec![5, 6]);
    }

    #[test]
    fn smd_files_are_deinterleaved() {
        let mut file = vec![0u8; 0x200 + 0x4000];
        file[8] = 0xAA;
        file[9] = 0xBB;
        for i in 0..0x2000 {
            file[0x200 + i] = 0x10; // odd bytes first
            file[0x200 + 0x2000 + i] = 0x20; // then even bytes
        }
        file[0x200] = 0x01;
        file[0x200 + 0x2000] = 0x02;
        let out = deinterleave_smd("bin", &file);
        assert_eq!(out.len(), 0x4000);
        assert_eq!(&out[..4], &[0x02, 0x01, 0x20, 0x10]);
        // A plain .md comes back untouched.
        assert_eq!(deinterleave_smd("md", &[1, 2, 3]), vec![1, 2, 3]);
    }

    #[test]
    fn padding_is_trimmed_by_its_own_rules() {
        // C1: a 64 KB file with an all-FF tail into 32 KB.
        let mut file = vec![0x11u8; 0x8000];
        file.extend_from_slice(&[0xFF; 0x8000]);
        assert_eq!(fit_padded(&file, 0x8000, "gearsystem").len(), 0x8000);
        file[0x9000] = 0x00;
        assert_eq!(fit_padded(&file, 0x8000, "gearsystem").len(), 0x10000);
        // C2: a zero tail is padding for bsnes only.
        let mut zeros = vec![0x11u8; 0x2000];
        zeros.extend_from_slice(&[0x00; 0x2000]);
        assert_eq!(fit_padded(&zeros, 0x2000, "bsnes").len(), 0x2000);
        assert_eq!(fit_padded(&zeros, 0x2000, "mesen2").len(), 0x4000);
        // Shorter or equal: untouched.
        assert_eq!(fit_padded(&[1, 2], 8, "bsnes"), &[1, 2]);
    }

    #[test]
    fn migration_candidates_are_the_files_those_cores_wrote() {
        let dir = Path::new("/support");
        assert_eq!(
            migration_candidates("mesen2", "nes", "Zelda (USA).nes", dir),
            vec![PathBuf::from("/support/Zelda (USA).sav")]
        );
        assert_eq!(migration_candidates("fceumm", "NES", "a.nes", dir), vec![PathBuf::from("/support/a.sav")]);
        assert_eq!(
            migration_candidates("blastem", "cue", "Sonic CD.cue", dir),
            ["scd_U.brm", "scd_E.brm", "scd_J.brm"].map(|n| dir.join(n)).to_vec()
        );
        assert!(migration_candidates("blastem", "md", "Sonic.md", dir).is_empty());
        assert!(migration_candidates("bsnes", "sfc", "a.sfc", dir).is_empty());
    }
}
