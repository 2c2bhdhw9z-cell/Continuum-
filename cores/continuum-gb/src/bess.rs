//! BESS — Best Effort Save State 1.0 (SameBoy's `BESS.md`).
//!
//! A BESS footer is appended to a save state so another BESS-compliant emulator
//! (SameBoy, BGB, mGBA, Boytacean) can read the state as a portable "best effort"
//! state, while cBoy still loads its own native state from the front of the file.
//! The footer is:
//!
//! ```text
//! [native cBoy state][raw buffers][NAME][INFO][CORE][END][u32 offset to first block]["BESS"]
//! ```
//!
//! The raw buffers (WRAM, VRAM, MBC RAM, OAM, HRAM, palettes) are the CORE block's
//! payloads, stored outside the blocks so the native state's own copies can be
//! reused. Their offsets in the CORE block are absolute, from the file's start.
//!
//! Loading: cBoy's native magic still comes first, so a cBoy state (with or without
//! a BESS footer) loads exactly as before — the footer is trailing data the native
//! decoder ignores. A file that has a BESS footer but no cBoy magic is a foreign
//! state; `find_bess` identifies it (emulator name, model) so the caller can say so
//! instead of reporting a corrupt file. A full best-effort import of a foreign
//! CORE block (rebuilding the machine from registers + buffers) is not done here.

use crate::gb::{GameBoy, Model};
use crate::ppu::CgbRev;

/// The BESS footer magic, the last four bytes of a file that has one.
pub const BESS_MAGIC: &[u8; 4] = b"BESS";

/// What `find_bess` found: the emulator that wrote the state (NAME block, if any),
/// the model the CORE block names (if any), and cBoy's own transient state (the
/// `CBOY` block, if any): whether the CPU was stopped, the M-cycles left in a
/// speed-switch halt, and the Color's scratch bytes. The transient state is not
/// part of the native layout, so it rides in the footer instead of changing the
/// format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BessInfo {
    pub name: Option<String>,
    pub model: Option<String>,
    pub cboy: Option<(bool, u16, [u8; 3])>,
}

/// Find a BESS footer at the end of `data`. Returns the info from its NAME and
/// CORE blocks, or `None` when there is no well-formed footer.
pub fn find_bess(data: &[u8]) -> Option<BessInfo> {
    if data.len() < 8 || &data[data.len() - 4..] != BESS_MAGIC {
        return None;
    }
    let first = u32::from_le_bytes(data[data.len() - 8..data.len() - 4].try_into().ok()?) as usize;
    if first >= data.len() {
        return None;
    }
    let mut info = BessInfo { name: None, model: None, cboy: None };
    let mut at = first;
    let mut saw_end = false;
    // A footer is a handful of small blocks; refuse to walk the whole file.
    for _ in 0..32 {
        if at + 8 > data.len() {
            return None;
        }
        let id = &data[at..at + 4];
        let len = u32::from_le_bytes(data[at + 4..at + 8].try_into().ok()?) as usize;
        let body = at + 8;
        if body + len > data.len() {
            return None;
        }
        match id {
            b"NAME" => {
                info.name = Some(String::from_utf8_lossy(&data[body..body + len]).into_owned());
            }
            b"CORE" => {
                if len >= 8 {
                    let raw = &data[body + 4..body + 8];
                    info.model = Some(String::from_utf8_lossy(raw).into_owned());
                }
            }
            b"CBOY" => {
                if len >= 6 {
                    let stopped = data[body] != 0;
                    let halt = u16::from_le_bytes(data[body + 1..body + 3].try_into().ok()?);
                    let scratch = [data[body + 3], data[body + 4], data[body + 5]];
                    info.cboy = Some((stopped, halt, scratch));
                }
            }
            b"END " => {
                if len != 0 {
                    return None;
                }
                saw_end = true;
                break;
            }
            _ => {}
        }
        at = body + len;
    }
    if saw_end { Some(info) } else { None }
}

/// Append a BESS footer (raw buffers, then NAME/INFO/CORE/END blocks, then the
/// footer) to a native cBoy state in `out`.
pub fn append_bess(out: &mut Vec<u8>, gb: &GameBoy) {
    // The raw buffers, in the order the CORE block lists them. Offsets are absolute.
    let mut off = |out: &mut Vec<u8>, bytes: &[u8]| -> (u32, u32) {
        if bytes.is_empty() {
            return (0, 0);
        }
        let at = out.len() as u32;
        out.extend_from_slice(bytes);
        (bytes.len() as u32, at)
    };
    let (ram_len, ram_off) = off(out, &gb.bus.wram);
    let (vram_len, vram_off) = off(out, &gb.bus.ppu.vram);
    let (mbc_len, mbc_off) = off(out, &gb.bus.cart.ram);
    let (oam_len, oam_off) = off(out, &gb.bus.ppu.oam);
    let (hram_len, hram_off) = off(out, &gb.bus.hram);
    let cgb = gb.bus.cgb;
    let (bg_len, bg_off) = if cgb { off(out, &gb.bus.ppu.bg_pal) } else { (0, 0) };
    let (obj_len, obj_off) = if cgb { off(out, &gb.bus.ppu.obj_pal) } else { (0, 0) };

    let first_block = out.len() as u32;
    // NAME: the emulator and version, so another emulator knows whose native
    // format the front of the file is.
    block(out, b"NAME", format!("cBoy {}", env!("CARGO_PKG_VERSION")).as_bytes());
    // INFO: the ROM title and global checksum, so a loader can check the game.
    let rom = &gb.bus.cart.rom;
    let mut info = Vec::with_capacity(0x12);
    let title = rom.get(0x134..0x144).unwrap_or(&[]);
    info.extend_from_slice(title);
    info.resize(0x10, 0);
    let checksum = rom.get(0x14E..0x150).unwrap_or(&[]);
    info.extend_from_slice(checksum);
    info.resize(0x12, 0);
    block(out, b"INFO", &info);
    // CORE: version, model, CPU registers, execution state, I/O registers, and
    // the buffer sizes and offsets above.
    let mut core = vec![0u8; 0xD0];
    core[0..2].copy_from_slice(&1u16.to_le_bytes());
    core[2..4].copy_from_slice(&1u16.to_le_bytes());
    core[4..8].copy_from_slice(&model_id(gb));
    let cpu = &gb.cpu;
    let put16 = |core: &mut Vec<u8>, at: usize, v: u16| core[at..at + 2].copy_from_slice(&v.to_le_bytes());
    put16(&mut core, 0x08, cpu.pc);
    put16(&mut core, 0x0A, (u16::from(cpu.a) << 8) | u16::from(cpu.f & 0xF0));
    put16(&mut core, 0x0C, cpu.bc());
    put16(&mut core, 0x0E, cpu.de());
    put16(&mut core, 0x10, cpu.hl());
    put16(&mut core, 0x12, cpu.sp);
    core[0x14] = u8::from(cpu.ime);
    core[0x15] = gb.bus.ie;
    core[0x16] = if cpu.halted { 1 } else { 0 };
    for i in 0..128 {
        core[0x18 + i] = gb.bus.raw_read(0xFF00 + i as u16);
    }
    let put32 = |core: &mut Vec<u8>, at: usize, v: u32| core[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put32(&mut core, 0x98, ram_len);
    put32(&mut core, 0x9C, ram_off);
    put32(&mut core, 0xA0, vram_len);
    put32(&mut core, 0xA4, vram_off);
    put32(&mut core, 0xA8, mbc_len);
    put32(&mut core, 0xAC, mbc_off);
    put32(&mut core, 0xB0, oam_len);
    put32(&mut core, 0xB4, oam_off);
    put32(&mut core, 0xB8, hram_len);
    put32(&mut core, 0xBC, hram_off);
    put32(&mut core, 0xC0, bg_len);
    put32(&mut core, 0xC4, bg_off);
    put32(&mut core, 0xC8, obj_len);
    put32(&mut core, 0xCC, obj_off);
    block(out, b"CORE", &core);
    // CBOY: cBoy's own transient state, which the native layout does not carry:
    // whether the CPU was stopped, the M-cycles left in a speed-switch halt, and
    // the Color's scratch bytes ($FF72/$FF73/$FF75). Another BESS reader ignores
    // an unknown block, so this stays interoperable.
    let mut cboy = vec![u8::from(gb.cpu.stopped), (gb.cpu.speed_halt & 0xFF) as u8, (gb.cpu.speed_halt >> 8) as u8];
    cboy.extend_from_slice(&gb.bus.scratch);
    block(out, b"CBOY", &cboy);
    // END: zero length, the last block.
    block(out, b"END ", &[]);
    out.extend_from_slice(&first_block.to_le_bytes());
    out.extend_from_slice(BESS_MAGIC);
}

/// One BESS block: a four-letter identifier, a little-endian length, then the body.
fn block(out: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(id);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
}

/// The BESS model identifier: family, model, CPU revision, padding space.
fn model_id(gb: &GameBoy) -> [u8; 4] {
    match gb.console {
        Model::Cgb => {
            let rev = match gb.bus.ppu.cgb_rev {
                CgbRev::C => b'C',
                CgbRev::D => b'D',
                CgbRev::E => b'E',
            };
            [b'C', b'C', rev, b' ']
        }
        Model::Agb => [b'C', b'A', b' ', b' '],
        // DMG, DMG0, MGB and anything else in the original family: no revision.
        _ => [b'G', b'D', b' ', b' '],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bess_round_trip_identifies_the_writer() {
        let mut gb = GameBoy::new(vec![0u8; 0x8000]).unwrap();
        let state = gb.save_state();
        assert_eq!(&state[state.len() - 4..], BESS_MAGIC);
        let info = find_bess(&state).expect("a BESS footer");
        assert!(info.name.as_deref().unwrap_or("").starts_with("cBoy "));
        assert_eq!(info.model.as_deref(), Some("GD  "));
        // And the state still loads, footer and all.
        gb.load_state(&state).unwrap();
    }

    #[test]
    fn bess_cboy_block_round_trips_a_stopped_cpu() {
        let mut gb = GameBoy::new(vec![0u8; 0x8000]).unwrap();
        gb.cpu.stopped = true;
        gb.cpu.speed_halt = 1234;
        gb.bus.scratch = [0x5A, 0xA5, 0x70];
        let state = gb.save_state();
        let info = find_bess(&state).expect("a BESS footer");
        assert_eq!(info.cboy, Some((true, 1234, [0x5A, 0xA5, 0x70])));
        let mut other = GameBoy::new(vec![0u8; 0x8000]).unwrap();
        other.load_state(&state).unwrap();
        assert!(other.cpu.stopped);
        assert_eq!(other.cpu.speed_halt, 1234);
        assert_eq!(other.bus.scratch, [0x5A, 0xA5, 0x70]);
        // And the round trip is exact: saving the loaded machine gives the same bytes.
        assert_eq!(other.save_state(), state);
    }

    #[test]
    fn bess_foreign_footer_is_detected_not_mistaken_for_a_cboy_state() {
        // A minimal foreign BESS file: NAME + CORE + END + footer, no cBoy magic.
        let mut f = Vec::new();
        let first = 0u32;
        block(&mut f, b"NAME", b"SameBoy 1.0.0");
        let mut core = vec![0u8; 0xD0];
        core[0..2].copy_from_slice(&1u16.to_le_bytes());
        core[2..4].copy_from_slice(&1u16.to_le_bytes());
        core[4..8].copy_from_slice(b"CCE ");
        block(&mut f, b"CORE", &core);
        block(&mut f, b"END ", &[]);
        f.extend_from_slice(&first.to_le_bytes());
        f.extend_from_slice(BESS_MAGIC);
        let info = find_bess(&f).expect("a BESS footer");
        assert_eq!(info.name.as_deref(), Some("SameBoy 1.0.0"));
        assert_eq!(info.model.as_deref(), Some("CCE "));
        let mut gb = GameBoy::new(vec![0u8; 0x8000]).unwrap();
        assert!(gb.load_state(&f).is_err());
    }
}
