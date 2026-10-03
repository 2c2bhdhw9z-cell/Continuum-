//! Turning a core's memory into the flat address space RetroAchievements uses.
//!
//! An achievement condition says "the byte at $0075". That address is in RetroAchievements' own
//! numbering for the console (rcheevos' `rc_console_memory_regions`), not an offset into any one
//! libretro buffer: on a SNES, $000000-$01FFFF is work RAM and $020000 onwards is cartridge RAM,
//! which libretro exposes as two separate regions (`SYSTEM_RAM` and `SAVE_RAM`).
//!
//! A port of rcheevos' own `rc_libretro.c` (`rc_libretro_memory_init` and the three paths under it),
//! which is what RetroArch runs. Ported rather than compiled because `rc_libretro.c` needs
//! `libretro.h` at compile time, which this repository fetches on demand and does not commit.
//!
//! - **With a memory map** (`RETRO_ENVIRONMENT_SET_MEMORY_MAPS`, see [`crate::memory_maps`]): each
//!   console region's `real_address` (the console's own bus address, `$03000000` for the GBA's
//!   internal work RAM) is looked up in the core's descriptors. This is what makes the GBA right:
//!   mGBA's `SYSTEM_RAM` is only one of its two work RAMs, but its map has both.
//! - **Without one**: the console's regions are served from `SYSTEM_RAM` and `SAVE_RAM` in order,
//!   which is the right answer wherever the console's achievement RAM is exactly those two
//!   buffers back to back (NES, Game Boy, Master System, Mega Drive, SNES on snes9x, PlayStation).
//! - **No console table at all**: system RAM, then save RAM.

/// rcheevos' `RC_MEMORY_TYPE_*`, rc_consoles.h line 110 onwards. An enum in C starting at zero.
pub const RC_MEMORY_TYPE_SYSTEM_RAM: u8 = 0;
pub const RC_MEMORY_TYPE_SAVE_RAM: u8 = 1;
pub const RC_MEMORY_TYPE_VIDEO_RAM: u8 = 2;
pub const RC_MEMORY_TYPE_UNUSED: u8 = 6;

use crate::memory_maps::{self, MemoryDescriptor};

/// One region of a console's achievement address space, as rcheevos describes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsoleRegion {
    pub start: u32,
    pub end: u32,
    /// The console's own address of `start` (`rc_memory_region_t::real_address`). Only the
    /// memory-map path reads it.
    pub real: u32,
    pub kind: u8,
}

/// One contiguous block of the flat address space: either real memory or a null filler that reads
/// as nothing (rcheevos treats a null block as "stop here").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MappedBlock {
    pub data: *const u8,
    pub size: usize,
}

impl MappedBlock {
    fn is_null(&self) -> bool {
        self.data.is_null()
    }
}

/// rcheevos' `RC_LIBRETRO_MAX_MEMORY_REGIONS` is 32. The same ceiling here, so a pathological
/// console table cannot grow the list without bound.
const MAX_BLOCKS: usize = 32;

fn register(blocks: &mut Vec<MappedBlock>, data: *const u8, size: usize) {
    if size == 0 {
        return;
    }
    if let Some(last) = blocks.last_mut() {
        // Extend a null block with more null, or a real block with the memory right after it.
        if data.is_null() && last.is_null() {
            last.size += size;
            return;
        }
        if !data.is_null() && !last.is_null() && last.data.wrapping_add(last.size) == data {
            last.size += size;
            return;
        }
    }
    if blocks.len() == MAX_BLOCKS {
        return;
    }
    blocks.push(MappedBlock { data, size });
}

/// The libretro region a console region is served from. Mirrors
/// `rc_libretro_memory_console_region_to_ram_type`.
fn libretro_region_for(kind: u8) -> u32 {
    match kind {
        RC_MEMORY_TYPE_SAVE_RAM => crate::memory::MEMORY_SAVE_RAM,
        RC_MEMORY_TYPE_VIDEO_RAM => crate::memory::MEMORY_VIDEO_RAM,
        _ => crate::memory::MEMORY_SYSTEM_RAM,
    }
}

/// Builds the block list for one console. Mirrors `rc_libretro_memory_init`.
///
/// `map` is the core's memory map (empty when it published none) and `core_region(id)` answers a
/// libretro region id with the core's buffer, or `None`.
pub fn build(
    console: &[ConsoleRegion],
    map: &[MemoryDescriptor],
    core_region: impl Fn(u32) -> Option<(*const u8, usize)>,
) -> Vec<MappedBlock> {
    if !console.is_empty() && !map.is_empty() {
        return build_from_map(console, map);
    }
    let mut blocks = Vec::new();
    if console.is_empty() {
        // No console table: system RAM, then save RAM.
        for id in [crate::memory::MEMORY_SYSTEM_RAM, crate::memory::MEMORY_SAVE_RAM] {
            if let Some((data, size)) = core_region(id) {
                register(&mut blocks, data, size);
            }
        }
        return blocks;
    }

    let last_end = console.last().map_or(0, |r| r.end);
    let mut padding = false;
    for (i, region) in console.iter().enumerate() {
        let region_size = (region.end as usize).saturating_sub(region.start as usize) + 1;
        let libretro = libretro_region_for(region.kind);

        if region.kind == RC_MEMORY_TYPE_UNUSED
            && region_size >= 0x10000
            && !padding
            && last_end > 0x0100_0000
        {
            // Large unused gaps in a big address space are alignment padding: the memory either
            // side of them is not contiguous in the libretro buffer, so stop mapping here.
            padding = true;
        }

        // The first console region served by the same libretro buffer is where that buffer starts.
        let base = console[..=i]
            .iter()
            .find(|r| libretro_region_for(r.kind) == libretro)
            .map_or(0, |r| r.start);
        let offset = (region.start - base) as usize;

        let (data, available) = if padding {
            (std::ptr::null(), region_size)
        } else {
            core_region(libretro).unwrap_or((std::ptr::null(), 0))
        };
        let (data, available) = if offset < available {
            let data = if data.is_null() {
                data
            } else {
                data.wrapping_add(offset)
            };
            (data, available - offset)
        } else {
            (std::ptr::null(), 0)
        };

        if region_size > available {
            register(&mut blocks, data, available);
            register(&mut blocks, std::ptr::null(), region_size - available);
        } else {
            register(&mut blocks, data, region_size);
        }
    }
    blocks
}

/// `rc_libretro_memory_init_from_memory_map`, line for line.
fn build_from_map(console: &[ConsoleRegion], map: &[MemoryDescriptor]) -> Vec<MappedBlock> {
    let mut blocks = Vec::new();
    for region in console {
        let mut remaining = (region.end as usize).saturating_sub(region.start as usize) + 1;
        let mut real_address = region.real;
        let mut disconnect_size: u32 = 0;
        while remaining > 0 {
            let Some((index, offset)) = memory_maps::find(map, real_address) else {
                if disconnect_size != 0 && remaining > disconnect_size as usize {
                    register(&mut blocks, std::ptr::null(), disconnect_size as usize);
                    remaining -= disconnect_size as usize;
                    real_address = real_address.wrapping_add(disconnect_size);
                    disconnect_size = 0;
                    continue;
                }
                register(&mut blocks, std::ptr::null(), remaining);
                break;
            };
            let desc = &map[index];
            let data: *const u8 = if desc.ptr == 0 {
                std::ptr::null()
            } else {
                (desc.host() as *const u8).wrapping_add(offset)
            };
            let mut desc_size = desc.len.saturating_sub(offset);
            if desc.disconnect != 0 && desc_size > desc.disconnect {
                // The largest block readable before the lowest disconnected bit flips.
                let low = desc.disconnect as u32;
                disconnect_size = low & low.wrapping_neg();
                desc_size = (disconnect_size.wrapping_sub(real_address & disconnect_size.wrapping_sub(1)))
                    as usize;
            }
            if remaining > desc_size {
                if desc_size == 0 {
                    register(&mut blocks, std::ptr::null(), remaining);
                    remaining = 0;
                } else {
                    register(&mut blocks, data, desc_size);
                    remaining -= desc_size;
                    real_address = real_address.wrapping_add(desc_size as u32);
                }
            } else {
                register(&mut blocks, data, remaining);
                remaining = 0;
            }
        }
    }
    blocks
}

/// Reads `buffer.len()` bytes at `address`, returning how many were read. Mirrors
/// `rc_libretro_memory_read`: a read that reaches a null block stops there.
///
/// # Safety
///
/// Every non-null block must point at `size` readable bytes, which holds for blocks built by
/// [`build`] from a core's live regions until the core runs again.
pub unsafe fn read(blocks: &[MappedBlock], mut address: u32, buffer: &mut [u8]) -> u32 {
    let mut written = 0usize;
    let mut wanted = buffer.len();
    for block in blocks {
        let size = block.size;
        if (address as usize) >= size {
            address -= size as u32;
            continue;
        }
        if block.is_null() {
            break;
        }
        let available = size - address as usize;
        let take = available.min(wanted);
        let source = unsafe { std::slice::from_raw_parts(block.data.add(address as usize), take) };
        buffer[written..written + take].copy_from_slice(source);
        written += take;
        wanted -= take;
        if wanted == 0 {
            break;
        }
        address = 0;
    }
    written as u32
}

/// Total bytes of address space the blocks cover, null filler included.
pub fn total_size(blocks: &[MappedBlock]) -> usize {
    blocks.iter().map(|b| b.size).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MEMORY_SAVE_RAM, MEMORY_SYSTEM_RAM};

    fn region(start: u32, end: u32, kind: u8) -> ConsoleRegion {
        ConsoleRegion { start, end, real: 0, kind }
    }

    #[test]
    fn no_console_table_means_system_then_save() {
        let system = [1u8; 4];
        let save = [2u8; 2];
        let blocks = build(&[], &[], |id| match id {
            MEMORY_SYSTEM_RAM => Some((system.as_ptr(), system.len())),
            MEMORY_SAVE_RAM => Some((save.as_ptr(), save.len())),
            _ => None,
        });
        assert_eq!(total_size(&blocks), 6);
        let mut out = [0u8; 6];
        assert_eq!(unsafe { read(&blocks, 0, &mut out) }, 6);
        assert_eq!(out, [1, 1, 1, 1, 2, 2]);
    }

    #[test]
    fn snes_style_system_then_save_in_one_address_space() {
        // $0000-$0007 system RAM, $0008-$000B save RAM.
        let console = [
            region(0, 7, RC_MEMORY_TYPE_SYSTEM_RAM),
            region(8, 11, RC_MEMORY_TYPE_SAVE_RAM),
        ];
        let system: Vec<u8> = (0..8).collect();
        let save: Vec<u8> = (100..104).collect();
        let blocks = build(&console, &[], |id| match id {
            MEMORY_SYSTEM_RAM => Some((system.as_ptr(), system.len())),
            MEMORY_SAVE_RAM => Some((save.as_ptr(), save.len())),
            _ => None,
        });
        let mut byte = [0u8; 1];
        unsafe {
            assert_eq!(read(&blocks, 3, &mut byte), 1);
            assert_eq!(byte[0], 3);
            assert_eq!(read(&blocks, 9, &mut byte), 1);
            assert_eq!(byte[0], 101);
            // A read straddling the boundary takes from both.
            let mut two = [0u8; 2];
            assert_eq!(read(&blocks, 7, &mut two), 2);
            assert_eq!(two, [7, 100]);
        }
    }

    #[test]
    fn a_smaller_core_buffer_is_padded_and_reads_stop_at_the_pad() {
        // The console says 16 bytes of system RAM; the core only has 8.
        let console = [
            region(0, 15, RC_MEMORY_TYPE_SYSTEM_RAM),
            region(16, 19, RC_MEMORY_TYPE_SAVE_RAM),
        ];
        let system = [9u8; 8];
        let save = [5u8; 4];
        let blocks = build(&console, &[], |id| match id {
            MEMORY_SYSTEM_RAM => Some((system.as_ptr(), system.len())),
            MEMORY_SAVE_RAM => Some((save.as_ptr(), save.len())),
            _ => None,
        });
        assert_eq!(total_size(&blocks), 20);
        let mut out = [0u8; 4];
        unsafe {
            assert_eq!(read(&blocks, 10, &mut out), 0, "inside the null pad");
            assert_eq!(read(&blocks, 6, &mut out), 2, "stops where the pad starts");
            assert_eq!(read(&blocks, 16, &mut out), 4);
        }
        assert_eq!(out, [5, 5, 5, 5]);
    }

    #[test]
    fn two_console_regions_over_one_buffer_take_consecutive_offsets() {
        // Like the GBA: two system-RAM regions, both served from SYSTEM_RAM, the second at an
        // offset equal to its distance from the first one's start.
        let console = [
            region(0, 3, RC_MEMORY_TYPE_SYSTEM_RAM),
            region(4, 11, RC_MEMORY_TYPE_SYSTEM_RAM),
        ];
        let system: Vec<u8> = (0..12).collect();
        let blocks = build(&console, &[], |id| {
            (id == MEMORY_SYSTEM_RAM).then_some((system.as_ptr(), system.len()))
        });
        // Contiguous memory merges into one block.
        assert_eq!(blocks.len(), 1);
        let mut out = [0u8; 12];
        assert_eq!(unsafe { read(&blocks, 0, &mut out) }, 12);
        assert_eq!(out.to_vec(), system);
    }

    #[test]
    fn a_missing_core_region_reads_as_nothing() {
        let console = [region(0, 7, RC_MEMORY_TYPE_SYSTEM_RAM)];
        let blocks = build(&console, &[], |_| None);
        let mut out = [0u8; 1];
        assert_eq!(unsafe { read(&blocks, 0, &mut out) }, 0);
        assert_eq!(total_size(&blocks), 8);
    }

    #[test]
    fn reads_past_the_end_return_zero() {
        let system = [1u8; 4];
        let blocks = build(&[], &[], |id| {
            (id == MEMORY_SYSTEM_RAM).then_some((system.as_ptr(), system.len()))
        });
        let mut out = [0u8; 2];
        assert_eq!(unsafe { read(&blocks, 100, &mut out) }, 0);
        assert_eq!(unsafe { read(&blocks, 3, &mut out) }, 1);
    }

    // ---- With a memory map. The console tables are rcheevos' own (consoleinfo.c), copied here
    // so these run without the C library; client.rs checks the live tables carry the same reals.

    fn real(start: u32, end: u32, real: u32, kind: u8) -> ConsoleRegion {
        ConsoleRegion { start, end, real, kind }
    }

    fn gba_console() -> Vec<ConsoleRegion> {
        vec![
            real(0x00000, 0x07FFF, 0x0300_0000, RC_MEMORY_TYPE_SYSTEM_RAM),
            real(0x08000, 0x47FFF, 0x0200_0000, RC_MEMORY_TYPE_SYSTEM_RAM),
            real(0x48000, 0x57FFF, 0x0E00_0000, RC_MEMORY_TYPE_SAVE_RAM),
        ]
    }

    fn byte_at(blocks: &[MappedBlock], address: u32) -> Option<u8> {
        let mut out = [0u8; 1];
        (unsafe { read(blocks, address, &mut out) } == 1).then_some(out[0])
    }

    #[test]
    fn gba_through_mgbas_map_reads_iwram_ewram_and_save() {
        let (buffers, raw) = crate::memory_maps::fixtures::mgba_gba(0x80_0000, 0x8000);
        let map = crate::memory_maps::fixtures::copy(&raw);
        // mGBA's SYSTEM_RAM is EWRAM only, which is what the old fallback served everything from.
        let ewram = (buffers.blocks[1].as_ptr(), buffers.blocks[1].len());
        let blocks = build(&gba_console(), &map, |id| (id == MEMORY_SYSTEM_RAM).then_some(ewram));
        assert_eq!(total_size(&blocks), 0x58000);
        assert_eq!(byte_at(&blocks, 0x0000), Some(buffers.blocks[0][0]), "IWRAM first");
        assert_eq!(byte_at(&blocks, 0x7FFF), Some(buffers.blocks[0][0x7FFF]));
        assert_eq!(byte_at(&blocks, 0x8000), Some(buffers.blocks[1][0]), "then EWRAM");
        assert_eq!(byte_at(&blocks, 0x47FFF), Some(buffers.blocks[1][0x3FFFF]));
        assert_eq!(byte_at(&blocks, 0x48000), Some(buffers.blocks[2][0]), "then the save");
        // A 32 KB save in a 64 KB window: the rest is filler, as in RetroArch.
        assert_eq!(byte_at(&blocks, 0x50000), None);

        // And the fallback this replaces would have answered $0000 with EWRAM's first byte.
        let fallback = build(&gba_console(), &[], |id| (id == MEMORY_SYSTEM_RAM).then_some(ewram));
        assert_eq!(byte_at(&fallback, 0x0000), Some(buffers.blocks[1][0]));
        assert_ne!(buffers.blocks[0][0], buffers.blocks[1][0]);
    }

    #[test]
    fn segacd_through_gpgx_map_reads_all_of_prg_ram() {
        let (buffers, raw, _names) = crate::memory_maps::fixtures::gpgx_segacd();
        let map = crate::memory_maps::fixtures::copy(&raw);
        let console = [
            real(0x00000, 0x0FFFF, 0x00FF_0000, RC_MEMORY_TYPE_SYSTEM_RAM),
            real(0x10000, 0x8FFFF, 0x8002_0000, RC_MEMORY_TYPE_SYSTEM_RAM),
            real(0x90000, 0xAFFFF, 0x0020_0000, RC_MEMORY_TYPE_SYSTEM_RAM),
        ];
        let blocks = build(&console, &map, |_| None);
        assert_eq!(byte_at(&blocks, 0x00010), Some(buffers.blocks[0][0x10]));
        assert_eq!(byte_at(&blocks, 0x10000), Some(buffers.blocks[1][0]));
        // The top of PRG RAM, which `find` alone would miss: the block is taken whole.
        assert_eq!(byte_at(&blocks, 0x8FFFF), Some(buffers.blocks[1][0x7FFFF]));
        assert_eq!(byte_at(&blocks, 0x90004), Some(buffers.blocks[2][4]));
    }

    #[test]
    fn snes_through_a_snes9x2010_map_reads_work_ram_and_leaves_cart_ram_unmapped() {
        let (buffers, raw) = crate::memory_maps::fixtures::snes9x2010_lorom();
        let map = crate::memory_maps::fixtures::copy(&raw);
        let console = [
            real(0x00000, 0x1FFFF, 0x07E_0000, RC_MEMORY_TYPE_SYSTEM_RAM),
            real(0x20000, 0x9FFFF, 0x100_0000, RC_MEMORY_TYPE_SAVE_RAM),
        ];
        let blocks = build(&console, &map, |_| None);
        assert_eq!(byte_at(&blocks, 0x10001), Some(buffers.blocks[0][0x10001]));
        // rcheevos puts SNES cartridge RAM at a made-up $1000000, outside any SNES address, so a
        // core's map cannot reach it. RetroArch reads nothing there either.
        assert_eq!(byte_at(&blocks, 0x20000), None);
    }

    #[test]
    fn a_map_that_covers_nothing_is_all_filler() {
        let map = vec![crate::memory_maps::MemoryDescriptor::new(0x1000, 0x4000_0000, 0x100)];
        let blocks = build(&gba_console(), &map, |_| None);
        assert_eq!(total_size(&blocks), 0x58000);
        assert_eq!(byte_at(&blocks, 0), None);
    }
}
