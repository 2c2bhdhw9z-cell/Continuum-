//! `RETRO_ENVIRONMENT_SET_MEMORY_MAPS`: the core's own description of the console's address space.
//!
//! `retro_get_memory_data` hands out at most four flat buffers (save RAM, RTC, system RAM, video
//! RAM). Several consoles have more RAM than that, in several places: the GBA has 32 KB of
//! internal work RAM at `$03000000` and 256 KB of external work RAM at `$02000000`, and mGBA's
//! `SYSTEM_RAM` is only one of them. A core that knows this publishes a memory map: a list of
//! descriptors, each saying "emulated addresses matching `start` under `select` live at `ptr +
//! offset`, with the `disconnect` bits ignored". RetroAchievements and RetroArch's own RAM search
//! read memory through that map when a core publishes one.
//!
//! What is here, all pure Rust so it is tested on the host:
//!
//! - [`MemoryDescriptor`]: an owned copy of one `retro_memory_descriptor`. libretro.h:1526 says the
//!   frontend must keep its own copy, strings included, so nothing here points at the core's table.
//! - [`preprocess`]: RetroArch's `mmap_preprocess_descriptors` (runloop.c), which fills in a zero
//!   `select` or `len` and disconnects address bits that can never reach the buffer. RetroArch runs
//!   it on every table at `SET_MEMORY_MAPS` time, before rcheevos ever sees the table, so running
//!   it here is what makes the achievement mapping below give RetroArch's answer.
//! - [`find`]: rcheevos' `rc_libretro_memory_get_descriptor` (rc_libretro.c), which resolves one
//!   emulated address to a descriptor and an offset. Used by the achievement mapper and by pokes
//!   made from a search over a mapped region.
//! - [`searchable`]: the distinct writable regions of a map, for the RAM search's region picker.
//!
//! Numbers and layouts are checked against `libretro.h` (fetched into `.work/hdr/` by
//! `scripts/fetch-libretro-headers.sh`); each cites its line.

use std::ffi::{c_char, c_uint, c_void, CStr};

/// `#define RETRO_MEMDESC_CONST (1 << 0)`, libretro.h:4141. Read-only memory (ROM).
pub const MEMDESC_CONST: u64 = 1 << 0;
/// `#define RETRO_MEMDESC_BIGENDIAN (1 << 1)`, libretro.h:4147.
pub const MEMDESC_BIGENDIAN: u64 = 1 << 1;
/// `#define RETRO_MEMDESC_SYSTEM_RAM (1 << 2)`, libretro.h:4152.
pub const MEMDESC_SYSTEM_RAM: u64 = 1 << 2;
/// `#define RETRO_MEMDESC_SAVE_RAM (1 << 3)`, libretro.h:4158.
pub const MEMDESC_SAVE_RAM: u64 = 1 << 3;
/// `#define RETRO_MEMDESC_VIDEO_RAM (1 << 4)`, libretro.h:4164.
pub const MEMDESC_VIDEO_RAM: u64 = 1 << 4;

/// `struct retro_memory_descriptor`, libretro.h:4223. Field order is the layout.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RetroMemoryDescriptor {
    pub flags: u64,
    pub ptr: *mut c_void,
    pub offset: usize,
    pub start: usize,
    pub select: usize,
    pub disconnect: usize,
    pub len: usize,
    pub addrspace: *const c_char,
}

/// `struct retro_memory_map`, libretro.h:4427.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RetroMemoryMap {
    pub descriptors: *const RetroMemoryDescriptor,
    pub num_descriptors: c_uint,
}

/// More descriptors than any core publishes (snes9x2010's ceiling, `MAX_MAPS`, is 128). A table
/// claiming more is treated as corrupt rather than walked.
pub const MAX_DESCRIPTORS: usize = 1024;

/// An owned copy of one `retro_memory_descriptor`.
///
/// The host pointer is kept as an address (`usize`) rather than a raw pointer so the core that
/// owns it stays `Send`; it is only turned back into a pointer at the point of a read or write,
/// between frames, under the engine lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryDescriptor {
    pub flags: u64,
    /// Host address of the buffer, 0 when the core gave none ("nothing is mapped here").
    pub ptr: usize,
    pub offset: usize,
    pub start: usize,
    pub select: usize,
    pub disconnect: usize,
    pub len: usize,
    /// `len` as the core declared it, before [`preprocess`] filled in a zero.
    pub declared_len: usize,
    pub addrspace: String,
}

impl MemoryDescriptor {
    /// A descriptor with every field zero, for building tables in tests and by hand.
    pub fn new(ptr: usize, start: usize, len: usize) -> Self {
        Self {
            flags: 0,
            ptr,
            offset: 0,
            start,
            select: 0,
            disconnect: 0,
            len,
            declared_len: len,
            addrspace: String::new(),
        }
    }

    /// Where byte 0 of this descriptor lives in the host, or 0 when it has no buffer.
    pub fn host(&self) -> usize {
        if self.ptr == 0 {
            0
        } else {
            self.ptr.wrapping_add(self.offset)
        }
    }

    pub fn is_const(&self) -> bool {
        self.flags & MEMDESC_CONST != 0
    }
}

/// Copies a core's table out of its memory and preprocesses it the way RetroArch does.
///
/// # Safety
///
/// `map` must be null or point at a `retro_memory_map` whose `descriptors` array has
/// `num_descriptors` entries, each `addrspace` null or NUL-terminated, as libretro.h:1524 requires.
pub unsafe fn copy_from_core(map: *const RetroMemoryMap) -> Vec<MemoryDescriptor> {
    if map.is_null() {
        return Vec::new();
    }
    let map = unsafe { &*map };
    let count = map.num_descriptors as usize;
    if map.descriptors.is_null() || count == 0 || count > MAX_DESCRIPTORS {
        return Vec::new();
    }
    let raw = unsafe { std::slice::from_raw_parts(map.descriptors, count) };
    let mut table: Vec<MemoryDescriptor> = raw
        .iter()
        .map(|d| MemoryDescriptor {
            flags: d.flags,
            ptr: d.ptr as usize,
            offset: d.offset,
            start: d.start,
            select: d.select,
            disconnect: d.disconnect,
            len: d.len,
            declared_len: d.len,
            addrspace: if d.addrspace.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(d.addrspace) }
                    .to_string_lossy()
                    .into_owned()
            },
        })
        .collect();
    // RetroArch ignores the result too: a table it cannot fully normalise is still used, with the
    // descriptors after the bad one left as the core wrote them, and `find` copes with that.
    let _ = preprocess(&mut table);
    table
}

// ---- RetroArch's helpers (runloop.c `mmap_add_bits_down`, `mmap_inflate`, `mmap_reduce`,
// `mmap_highest_bit`), on `usize` like the originals' `size_t`.

fn add_bits_down(mut n: usize) -> usize {
    n |= n >> 1;
    n |= n >> 2;
    n |= n >> 4;
    n |= n >> 8;
    n |= n >> 16;
    #[cfg(target_pointer_width = "64")]
    {
        n |= n >> 32;
    }
    n
}

fn inflate(mut addr: usize, mut mask: usize) -> usize {
    while mask != 0 {
        let tmp = (mask.wrapping_sub(1)) & !mask;
        addr = ((addr & !tmp) << 1) | (addr & tmp);
        mask &= mask - 1;
    }
    addr
}

fn reduce(mut addr: usize, mut mask: usize) -> usize {
    while mask != 0 {
        let tmp = (mask.wrapping_sub(1)) & !mask;
        addr = (addr & tmp) | ((addr >> 1) & !tmp);
        mask = (mask & (mask - 1)) >> 1;
    }
    addr
}

fn highest_bit(n: usize) -> usize {
    let n = add_bits_down(n);
    n ^ (n >> 1)
}

/// RetroArch's `mmap_preprocess_descriptors`, line for line. Returns false where RetroArch does,
/// with the descriptors before that point already rewritten, which is also what RetroArch keeps.
pub fn preprocess(table: &mut [MemoryDescriptor]) -> bool {
    let mut top_addr: usize = 1;
    for desc in table.iter() {
        if desc.select != 0 {
            top_addr |= desc.select;
        } else {
            top_addr |= desc.start.wrapping_add(desc.len).wrapping_sub(1);
        }
    }
    top_addr = add_bits_down(top_addr);

    for desc in table.iter_mut() {
        if desc.select == 0 {
            if desc.len == 0 {
                return false;
            }
            if desc.len & (desc.len - 1) != 0 {
                return false;
            }
            desc.select = top_addr & !inflate(add_bits_down(desc.len - 1), desc.disconnect);
        }
        if desc.len == 0 {
            desc.len = add_bits_down(reduce(top_addr & !desc.select, desc.disconnect)).wrapping_add(1);
        }
        if desc.start & !desc.select != 0 {
            return false;
        }
        let highest_reachable = inflate(desc.len.wrapping_sub(1), desc.disconnect);
        while highest_bit(top_addr & !desc.select & !desc.disconnect) > highest_bit(highest_reachable) {
            desc.disconnect |= highest_bit(top_addr & !desc.select & !desc.disconnect);
        }
    }
    true
}

/// rcheevos' `rc_libretro_memory_get_descriptor`: the first descriptor claiming `real_address`,
/// and the offset into it. Address arithmetic is 32-bit, as in the original.
pub fn find(table: &[MemoryDescriptor], real_address: u32) -> Option<(usize, usize)> {
    for (index, desc) in table.iter().enumerate() {
        if desc.select == 0 {
            // Explicit range.
            let address = real_address as u64;
            let start = desc.start as u64;
            if address >= start && address < start.saturating_add(desc.len as u64) {
                return Some((index, (address - start) as usize));
            }
        } else if ((desc.start as u64 ^ real_address as u64) & desc.select as u64) == 0 {
            let mut reduced = real_address.wrapping_sub(desc.start as u32);
            let mut disconnect = desc.disconnect as u32;
            while disconnect != 0 {
                let tmp = disconnect.wrapping_sub(1) & !disconnect;
                reduced = (reduced & tmp) | ((reduced >> 1) & !tmp);
                disconnect = (disconnect & (disconnect - 1)) >> 1;
            }
            if (reduced as usize) < desc.len {
                return Some((index, reduced as usize));
            }
        }
    }
    None
}

/// The first descriptor whose DECLARED range (`start` to `start + len`) holds the address.
///
/// The fallback for writes. RetroArch's preprocessing can leave a descriptor whose `select` no
/// longer covers all of its own length (Genesis Plus GX's Sega CD PRG RAM at `$80020000` is one:
/// its start is not aligned to its size), so [`find`] misses the top of it. rcheevos never notices,
/// because it resolves one address per block and takes the whole length from there, but a poke
/// made from a search over that region must still land.
fn find_declared(table: &[MemoryDescriptor], real_address: u32) -> Option<(usize, usize)> {
    let address = real_address as usize;
    table.iter().enumerate().find_map(|(index, desc)| {
        let end = desc.start.checked_add(desc.declared_len)?;
        (desc.ptr != 0 && address >= desc.start && address < end).then(|| (index, address - desc.start))
    })
}

/// One region of a memory map the RAM search can run over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchableRegion {
    /// Index of the descriptor in the table.
    pub index: usize,
    /// "IWRAM", "System RAM", ...: the core's address space name, else what its flags say.
    pub name: String,
    /// The console's own address of byte 0.
    pub start: usize,
    pub len: usize,
    pub host: usize,
}

/// The largest region offered to the search. Two snapshots are taken of it, so this is a memory
/// ceiling as much as a sanity check; 32 MB is more RAM than any system here has in one block.
pub const MAX_SEARCH_REGION: usize = 32 * 1024 * 1024;

/// The writable, distinct regions of a map, in the core's order.
///
/// Skipped: descriptors with no buffer, read-only ones (ROM, BIOS), mirrors (a second descriptor
/// over host memory already listed), and ones with no size at all. A zero `len` the core left to
/// be "bounded by select and disconnect" (libretro.h:4303) is taken as preprocessing worked it out,
/// which is how snes9x2010 describes work RAM.
pub fn searchable(table: &[MemoryDescriptor]) -> Vec<SearchableRegion> {
    let mut out: Vec<SearchableRegion> = Vec::new();
    for (index, desc) in table.iter().enumerate() {
        let host = desc.host();
        let len = if desc.declared_len > 0 {
            desc.declared_len
        } else if desc.select != 0 {
            desc.len
        } else {
            0
        };
        if host == 0 || desc.is_const() || len == 0 || len > MAX_SEARCH_REGION {
            continue;
        }
        let overlaps = out
            .iter()
            .any(|r| host < r.host.wrapping_add(r.len) && r.host < host.wrapping_add(len));
        if overlaps {
            continue;
        }
        let name = if !desc.addrspace.is_empty() {
            desc.addrspace.clone()
        } else if desc.flags & MEMDESC_SYSTEM_RAM != 0 {
            "System RAM".to_owned()
        } else if desc.flags & MEMDESC_SAVE_RAM != 0 {
            "Save RAM".to_owned()
        } else if desc.flags & MEMDESC_VIDEO_RAM != 0 {
            "Video RAM".to_owned()
        } else {
            "Memory".to_owned()
        };
        out.push(SearchableRegion {
            index,
            name,
            start: desc.start,
            len,
            host,
        });
    }
    out
}

/// The bytes of one descriptor.
///
/// # Safety
///
/// The descriptor must come from the core's current table, the core must not be running, and
/// `len` bytes at its host address must be live, which libretro.h:4238 promises for the session.
pub unsafe fn descriptor_bytes<'a>(desc: &MemoryDescriptor, len: usize) -> Option<&'a [u8]> {
    let host = desc.host();
    (host != 0 && len != 0).then(|| unsafe { std::slice::from_raw_parts(host as *const u8, len) })
}

/// Writes `bytes` at a console address through the map. Refuses, and writes nothing, when the
/// address is unmapped, read-only, has no buffer or the write would leave the descriptor.
///
/// # Safety
///
/// As [`descriptor_bytes`], and nothing else may be borrowing the core's memory.
pub unsafe fn write_through(table: &[MemoryDescriptor], address: u32, bytes: &[u8]) -> bool {
    let Some((index, offset)) = find(table, address).or_else(|| find_declared(table, address)) else {
        return false;
    };
    let desc = &table[index];
    let host = desc.host();
    let Some(end) = offset.checked_add(bytes.len()) else {
        return false;
    };
    if host == 0 || desc.is_const() || end > desc.len {
        return false;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), (host + offset) as *mut u8, bytes.len());
    }
    true
}

#[cfg(test)]
pub mod fixtures {
    //! Descriptor tables shaped exactly like the ones real cores publish, read from their sources.
    use super::*;

    /// One fake console's memory: the buffers the descriptors point into.
    pub struct Buffers {
        pub blocks: Vec<Vec<u8>>,
    }

    impl Buffers {
        pub fn ptr(&self, index: usize) -> usize {
            self.blocks[index].as_ptr() as usize
        }
    }

    fn filled(len: usize, seed: u8) -> Vec<u8> {
        (0..len).map(|i| seed.wrapping_add((i % 251) as u8)).collect()
    }

    /// mGBA's `_setupMaps` for the GBA (libretro/mgba `src/platform/libretro/libretro.c`), with
    /// the constants from `include/mgba/internal/gba/memory.h`. Buffers: 0 IWRAM, 1 EWRAM, 2 save,
    /// 3 ROM, 4 BIOS, 5 VRAM, 6 palette, 7 OAM, 8 I/O. `save_len` 0 is a cartridge with no save.
    pub fn mgba_gba(rom_len: usize, save_len: usize) -> (Buffers, Vec<RetroMemoryDescriptor>) {
        let buffers = Buffers {
            blocks: vec![
                filled(0x8000, 1),
                filled(0x40000, 2),
                filled(save_len.max(1), 3),
                filled(rom_len, 4),
                filled(0x4000, 5),
                filled(0x18000, 6),
                filled(0x400, 7),
                filled(0x400, 8),
                filled(0x400, 9),
            ],
        };
        let d = |flags: u64, ptr: usize, start: usize, select: usize, len: usize| RetroMemoryDescriptor {
            flags,
            ptr: ptr as *mut c_void,
            offset: 0,
            start,
            select,
            disconnect: 0,
            len,
            addrspace: std::ptr::null(),
        };
        let save_ptr = if save_len > 0 { buffers.ptr(2) } else { 0 };
        let table = vec![
            d(MEMDESC_SYSTEM_RAM, buffers.ptr(0), 0x0300_0000, 0xFF00_0000, 0x8000),
            d(MEMDESC_SYSTEM_RAM, buffers.ptr(1), 0x0200_0000, 0xFF00_0000, 0x40000),
            d(0, save_ptr, 0x0E00_0000, 0, save_len),
            d(MEMDESC_CONST, buffers.ptr(3), 0x0800_0000, 0, rom_len),
            d(MEMDESC_CONST, buffers.ptr(3), 0x0A00_0000, 0, rom_len),
            d(MEMDESC_CONST, buffers.ptr(3), 0x0C00_0000, 0, rom_len),
            d(MEMDESC_CONST, buffers.ptr(4), 0, 0, 0x4000),
            d(0, buffers.ptr(5), 0x0600_0000, 0xFF00_0000, 0x18000),
            d(0, buffers.ptr(6), 0x0500_0000, 0xFF00_0000, 0x400),
            d(0, buffers.ptr(7), 0x0700_0000, 0xFF00_0000, 0x400),
            d(0, buffers.ptr(8), 0x0400_0000, 0, 0x400),
        ];
        (buffers, table)
    }

    /// Genesis Plus GX's `set_memory_maps` for the Sega CD (`libretro/libretro.c`): 68K RAM, PRG
    /// RAM at a virtual `$80020000` (`SCD_BIT`), word RAM. Its only memory map; the Mega Drive
    /// publishes none. Buffers: 0 work RAM, 1 PRG RAM, 2 word RAM.
    pub fn gpgx_segacd() -> (Buffers, Vec<RetroMemoryDescriptor>, Vec<std::ffi::CString>) {
        let buffers = Buffers {
            blocks: vec![filled(0x10000, 10), filled(0x80000, 20), filled(0x40000, 30)],
        };
        let names: Vec<std::ffi::CString> = ["68KRAM", "PRGRAM", "WORDRAM"]
            .iter()
            .map(|n| std::ffi::CString::new(*n).unwrap())
            .collect();
        let d = |ptr: usize, start: usize, len: usize, name: &std::ffi::CString| RetroMemoryDescriptor {
            flags: MEMDESC_SYSTEM_RAM,
            ptr: ptr as *mut c_void,
            offset: 0,
            start,
            select: 0,
            disconnect: 0,
            len,
            addrspace: name.as_ptr(),
        };
        let table = vec![
            d(buffers.ptr(0), 0xFF_0000, 0x10000, &names[0]),
            d(buffers.ptr(1), (1usize << 31) | 0x02_0000, 0x80000, &names[1]),
            d(buffers.ptr(2), 0x20_0000, 0x40000, &names[2]),
        ];
        (buffers, table, names)
    }

    /// snes9x2010's map for a LoROM game (`memmap.c` `MAP_LIBRETRO`, after `S9xAppendMapping`
    /// merges): work RAM at $7E0000 as one 128 KB descriptor (the two 64 KB banks merged), the low
    /// 8 KB mirror in banks $00-$3F and $80-$BF, cartridge SRAM at $700000 with $8000 disconnected,
    /// and ROM. The current libretro snes9x publishes NO map (checked: its libretro.cpp never calls
    /// SET_MEMORY_MAPS), so this stands for the snes9x family's shape, not the core Continuum ships.
    /// Buffers: 0 work RAM, 1 SRAM, 2 ROM.
    pub fn snes9x2010_lorom() -> (Buffers, Vec<RetroMemoryDescriptor>) {
        let buffers = Buffers {
            blocks: vec![filled(0x20000, 40), filled(0x2000, 50), filled(0x10_0000, 60)],
        };
        let lib = |flags: u64, ptr: usize, offset: usize, disconnect: usize, len: usize,
                   bank_s: usize, bank_e: usize, addr_s: usize, addr_e: usize| {
            let start = bank_s << 16 | addr_s;
            let select = (start ^ (bank_e << 16 | addr_e)) ^ 0xFF_FFFF;
            RetroMemoryDescriptor {
                flags,
                ptr: ptr as *mut c_void,
                offset,
                start,
                select,
                disconnect,
                len,
                addrspace: std::ptr::null(),
            }
        };
        let mut wram = lib(0, buffers.ptr(0), 0, 0xFF_0000, 0, 0x7E, 0x7E, 0, 0xFFFF);
        // The merge of the $7F bank into the $7E one: `a->select &= ~len` with len 0x10000.
        wram.select &= !0x1_0000;
        wram.disconnect &= !0x1_0000;
        let table = vec![
            wram,
            lib(0, buffers.ptr(0), 0, 0xFF_0000, 0, 0x00, 0x3F, 0x0000, 0x1FFF),
            lib(0, buffers.ptr(0), 0, 0xFF_0000, 0, 0x80, 0xBF, 0x0000, 0x1FFF),
            lib(0, buffers.ptr(1), 0, 0x8000, 0x2000, 0x70, 0x7F, 0x0000, 0x7FFF),
            lib(MEMDESC_CONST, buffers.ptr(2), 0, 0x8000, 0x10_0000, 0x00, 0x3F, 0x8000, 0xFFFF),
        ];
        (buffers, table)
    }

    pub fn copy(table: &[RetroMemoryDescriptor]) -> Vec<MemoryDescriptor> {
        let map = RetroMemoryMap {
            descriptors: table.as_ptr(),
            num_descriptors: table.len() as c_uint,
        };
        unsafe { copy_from_core(&map) }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn layout_matches_libretro_h() {
        // Eight fields: u64, pointer, five size_t, pointer.
        let word = std::mem::size_of::<usize>();
        assert_eq!(std::mem::size_of::<RetroMemoryDescriptor>(), 8 + 7 * word);
        assert_eq!(std::mem::offset_of!(RetroMemoryDescriptor, ptr), 8);
        assert_eq!(std::mem::offset_of!(RetroMemoryDescriptor, addrspace), 8 + 6 * word);
        assert_eq!(std::mem::offset_of!(RetroMemoryMap, num_descriptors), word);
        assert_eq!(MEMDESC_CONST, 1);
        assert_eq!(MEMDESC_BIGENDIAN, 2);
        assert_eq!(MEMDESC_SYSTEM_RAM, 4);
        assert_eq!(MEMDESC_SAVE_RAM, 8);
        assert_eq!(MEMDESC_VIDEO_RAM, 16);
    }

    #[test]
    fn null_and_empty_tables_copy_to_nothing() {
        assert!(unsafe { copy_from_core(std::ptr::null()) }.is_empty());
        let map = RetroMemoryMap {
            descriptors: std::ptr::null(),
            num_descriptors: 3,
        };
        assert!(unsafe { copy_from_core(&map) }.is_empty());
    }

    #[test]
    fn mgba_iwram_and_ewram_resolve_to_their_own_buffers() {
        let (buffers, raw) = mgba_gba(0x80_0000, 0x8000);
        let table = copy(&raw);
        assert_eq!(table.len(), 11);
        // IWRAM after RetroArch's preprocessing: the 16 address bits above the 32 KB are
        // disconnected, so $03008000 mirrors $03000000 the way the hardware does.
        assert_eq!(table[0].disconnect, 0x00FF_8000);
        assert_eq!(find(&table, 0x0300_0000), Some((0, 0)));
        assert_eq!(find(&table, 0x0300_7FFF), Some((0, 0x7FFF)));
        assert_eq!(find(&table, 0x0300_8004), Some((0, 4)));
        assert_eq!(find(&table, 0x0200_0010), Some((1, 0x10)));
        assert_eq!(find(&table, 0x0E00_0002), Some((2, 2)));
        assert_eq!(find(&table, 0x0800_0000).map(|f| f.0), Some(3));
        assert_eq!(find(&table, 0x0400_0000).map(|f| f.0), Some(10));
        assert_eq!(find(&table, 0x0100_0000), None);
        assert_eq!(table[0].host(), buffers.ptr(0));
    }

    #[test]
    fn mgba_without_a_save_stops_preprocessing_where_retroarch_does() {
        let (_buffers, raw) = mgba_gba(0x60_0000, 0);
        let table = copy(&raw);
        // Descriptor 2 has no length and no select, which preprocessing refuses: everything after
        // it keeps select 0 and is matched by explicit range instead.
        assert_eq!(table[3].select, 0);
        assert_eq!(find(&table, 0x0800_0004), Some((3, 4)));
        assert_eq!(find(&table, 0x0E00_0000), None);
        assert_eq!(find(&table, 0x0300_0001), Some((0, 1)));
    }

    #[test]
    fn mgba_search_regions_skip_rom_bios_and_mirrors() {
        let (buffers, raw) = mgba_gba(0x80_0000, 0x8000);
        let regions = searchable(&copy(&raw));
        let starts: Vec<usize> = regions.iter().map(|r| r.start).collect();
        assert_eq!(
            starts,
            vec![0x0300_0000, 0x0200_0000, 0x0E00_0000, 0x0600_0000, 0x0500_0000, 0x0700_0000, 0x0400_0000]
        );
        assert_eq!(regions[0].name, "System RAM");
        assert_eq!(regions[0].len, 0x8000);
        assert_eq!(regions[1].len, 0x40000);
        assert_eq!(regions[1].host, buffers.ptr(1));
        assert_eq!(regions[3].name, "Memory");
    }

    #[test]
    fn gpgx_segacd_prg_ram_lives_at_the_virtual_scd_bit_address() {
        let (_buffers, raw, _names) = gpgx_segacd();
        let table = copy(&raw);
        assert_eq!(table[1].addrspace, "PRGRAM");
        assert_eq!(find(&table, 0x00FF_0010), Some((0, 0x10)));
        assert_eq!(find(&table, 0x8002_0000), Some((1, 0)));
        // RetroArch's preprocessing gives PRG RAM a select of $FFF80000 and then gives up on it,
        // because $80020000 is not aligned to its 512 KB: so `find` misses its top end, exactly as
        // it does in RetroArch. The achievement mapper still maps all of it (see memory_map.rs).
        assert_eq!(table[1].select, 0xFFF8_0000);
        assert_eq!(find(&table, 0x8009_FFFF), None);
        assert_eq!(find_declared(&table, 0x8009_FFFF), Some((1, 0x7FFFF)));
        assert_eq!(find(&table, 0x0020_0004), Some((2, 4)));
        let names: Vec<String> = searchable(&table).into_iter().map(|r| r.name).collect();
        assert_eq!(names, vec!["68KRAM", "PRGRAM", "WORDRAM"]);
    }

    #[test]
    fn snes9x2010_wram_mirrors_and_disconnected_sram() {
        let (_buffers, raw) = snes9x2010_lorom();
        let table = copy(&raw);
        assert_eq!(find(&table, 0x7E_0000), Some((0, 0)));
        assert_eq!(find(&table, 0x7F_0001), Some((0, 0x1_0001)));
        // Bank $00's first 8 KB is the same work RAM.
        assert_eq!(find(&table, 0x00_1FFF), Some((1, 0x1FFF)));
        assert_eq!(find(&table, 0x80_0010), Some((2, 0x10)));
        // SRAM at $70:0000, and $8000 is not an address bit for it.
        assert_eq!(find(&table, 0x70_0004), Some((3, 4)));
        // Only work RAM and SRAM are searchable: the mirrors share work RAM's buffer, ROM is const.
        let regions = searchable(&table);
        assert_eq!(regions.len(), 2, "{regions:?}");
        assert_eq!(regions[1].start, 0x70_0000);
    }

    #[test]
    fn write_through_refuses_rom_and_unmapped_addresses() {
        let (buffers, raw) = mgba_gba(0x80_0000, 0x8000);
        let table = copy(&raw);
        unsafe {
            assert!(write_through(&table, 0x0300_0010, &[0xAA, 0xBB]));
            assert!(!write_through(&table, 0x0800_0000, &[1]), "ROM is read-only");
            assert!(!write_through(&table, 0x0100_0000, &[1]), "nothing is mapped there");
            assert!(!write_through(&table, 0x0300_7FFF, &[1, 2]), "runs off the end");
        }
        assert_eq!(&buffers.blocks[0][0x10..0x12], &[0xAA, 0xBB]);
    }

    #[test]
    fn retroarch_helpers_match_their_definitions() {
        assert_eq!(add_bits_down(0x0400_0000), 0x07FF_FFFF);
        assert_eq!(highest_bit(0x00FF_FFFF), 0x0080_0000);
        assert_eq!(highest_bit(0), 0);
        // Inflate opens a gap at each mask bit; reduce closes it again.
        assert_eq!(inflate(0b11, 0b10), 0b101);
        assert_eq!(reduce(0b101, 0b10), 0b11);
    }
}
