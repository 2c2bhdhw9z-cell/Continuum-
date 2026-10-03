//! Turning a core's memory regions into the flat address space RetroAchievements uses.
//!
//! An achievement condition says "the byte at $0075". That address is in RetroAchievements' own
//! numbering for the console (rcheevos' `rc_console_memory_regions`), not an offset into any one
//! libretro buffer: on a SNES, $000000-$01FFFF is work RAM and $020000 onwards is cartridge RAM,
//! which libretro exposes as two separate regions (`SYSTEM_RAM` and `SAVE_RAM`).
//!
//! This is a port of the "unmapped memory" path of rcheevos' own `rc_libretro.c`
//! (`rc_libretro_memory_init_from_unmapped_memory` and `..._without_regions`), the path RetroArch
//! takes for every core that does not publish `RETRO_ENVIRONMENT_SET_MEMORY_MAPS`. It is ported
//! rather than compiled because `rc_libretro.c` needs `libretro.h` at compile time, which this
//! repository fetches on demand and does not commit.
//!
//! KNOWN GAP, said plainly: cores that DO publish memory maps (snes9x, Genesis Plus GX, mGBA and
//! others) are mapped by RetroArch through those maps instead. Continuum does not accept
//! `SET_MEMORY_MAPS` yet, so for those cores this fallback is used. For consoles whose achievement
//! RAM is exactly the core's `SYSTEM_RAM` followed by `SAVE_RAM` (NES, Game Boy, Master System,
//! Mega Drive, PlayStation work RAM) that is the same answer. For consoles with several disjoint
//! RAM blocks behind one libretro region (the GBA's IWRAM and EWRAM) an address can land in the
//! wrong block, and those achievements will not trigger correctly until memory maps are honoured.

/// rcheevos' `RC_MEMORY_TYPE_*`, rc_consoles.h line 110 onwards. An enum in C starting at zero.
pub const RC_MEMORY_TYPE_SYSTEM_RAM: u8 = 0;
pub const RC_MEMORY_TYPE_SAVE_RAM: u8 = 1;
pub const RC_MEMORY_TYPE_VIDEO_RAM: u8 = 2;
pub const RC_MEMORY_TYPE_UNUSED: u8 = 6;

/// One region of a console's achievement address space, as rcheevos describes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsoleRegion {
    pub start: u32,
    pub end: u32,
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

/// Builds the block list for one console.
///
/// `core_region(id)` answers a libretro region id with the core's buffer, or `None`.
pub fn build(
    console: &[ConsoleRegion],
    core_region: impl Fn(u32) -> Option<(*const u8, usize)>,
) -> Vec<MappedBlock> {
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
        ConsoleRegion { start, end, kind }
    }

    #[test]
    fn no_console_table_means_system_then_save() {
        let system = [1u8; 4];
        let save = [2u8; 2];
        let blocks = build(&[], |id| match id {
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
        let blocks = build(&console, |id| match id {
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
        let blocks = build(&console, |id| match id {
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
        let blocks = build(&console, |id| {
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
        let blocks = build(&console, |_| None);
        let mut out = [0u8; 1];
        assert_eq!(unsafe { read(&blocks, 0, &mut out) }, 0);
        assert_eq!(total_size(&blocks), 8);
    }

    #[test]
    fn reads_past_the_end_return_zero() {
        let system = [1u8; 4];
        let blocks = build(&[], |id| {
            (id == MEMORY_SYSTEM_RAM).then_some((system.as_ptr(), system.len()))
        });
        let mut out = [0u8; 2];
        assert_eq!(unsafe { read(&blocks, 100, &mut out) }, 0);
        assert_eq!(unsafe { read(&blocks, 3, &mut out) }, 1);
    }
}
