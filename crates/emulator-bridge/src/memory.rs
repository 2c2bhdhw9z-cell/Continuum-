//! Core memory regions: what `retro_get_memory_data` / `retro_get_memory_size` expose.
//!
//! Three features stand on this one seam: the battery save (`SAVE_RAM` is the cartridge's
//! battery-backed RAM, which the FRONTEND has to persist), RAM search and per-frame pokes
//! (`SYSTEM_RAM`), and RetroAchievements (reads `SYSTEM_RAM` and `SAVE_RAM` every frame).
//!
//! The ids are libretro's own and are machine-checked against `libretro.h` (fetched by
//! `scripts/fetch-libretro-headers.sh`) rather than remembered.

/// `#define RETRO_MEMORY_SAVE_RAM 0`, libretro.h line 510. Battery-backed cartridge RAM.
pub const MEMORY_SAVE_RAM: u32 = 0;
/// `#define RETRO_MEMORY_RTC 1`, libretro.h line 515. Real time clock state.
pub const MEMORY_RTC: u32 = 1;
/// `#define RETRO_MEMORY_SYSTEM_RAM 2`, libretro.h line 518. The console's work RAM.
pub const MEMORY_SYSTEM_RAM: u32 = 2;
/// `#define RETRO_MEMORY_VIDEO_RAM 3`, libretro.h line 521.
pub const MEMORY_VIDEO_RAM: u32 = 3;

/// A readable name for a region id, for status lines. Never empty.
pub fn region_name(id: u32) -> &'static str {
    match id {
        MEMORY_SAVE_RAM => "save RAM",
        MEMORY_RTC => "real time clock",
        MEMORY_SYSTEM_RAM => "system RAM",
        MEMORY_VIDEO_RAM => "video RAM",
        _ => "unknown memory region",
    }
}

/// Checks that `offset..offset+len` lies inside a region of `region_len` bytes.
///
/// Returns the range on success and a sentence on failure. Written with checked arithmetic
/// because both numbers come from Swift, and an overflow in the addition would otherwise wrap
/// into a range that looks valid.
pub fn checked_range(
    region_len: usize,
    offset: u64,
    len: u64,
) -> Result<std::ops::Range<usize>, String> {
    let start = usize::try_from(offset).map_err(|_| format!("offset {offset} is too large"))?;
    let count = usize::try_from(len).map_err(|_| format!("length {len} is too large"))?;
    let end = start
        .checked_add(count)
        .ok_or_else(|| format!("offset {offset} plus length {len} overflows"))?;
    if end > region_len {
        return Err(format!(
            "bytes {start}..{end} are outside a region of {region_len} byte(s)"
        ));
    }
    Ok(start..end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_libretro_h() {
        // libretro.h lines 510, 515, 518 and 521. A renumbering upstream would be an ABI break,
        // so this test is a tripwire for a typo here rather than for a change there.
        assert_eq!(MEMORY_SAVE_RAM, 0);
        assert_eq!(MEMORY_RTC, 1);
        assert_eq!(MEMORY_SYSTEM_RAM, 2);
        assert_eq!(MEMORY_VIDEO_RAM, 3);
    }

    #[test]
    fn range_checks() {
        assert_eq!(checked_range(16, 0, 16).unwrap(), 0..16);
        assert_eq!(checked_range(16, 4, 4).unwrap(), 4..8);
        assert_eq!(checked_range(16, 16, 0).unwrap(), 16..16);
        assert!(checked_range(16, 15, 2).is_err());
        assert!(checked_range(16, u64::MAX, 2).is_err());
        assert!(checked_range(16, 1, u64::MAX).is_err());
    }

    #[test]
    fn every_region_has_a_name() {
        for id in 0..5 {
            assert!(!region_name(id).is_empty());
        }
    }
}
