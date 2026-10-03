//! The classic RAM search, over `SYSTEM_RAM`.
//!
//! Start a search and every address is a candidate. Play a little, then filter: "it went down",
//! "it did not change", "it is now 3". Each filter keeps the candidates that pass and drops the
//! rest, and after a few rounds what is left is the address of the lives counter. That address
//! then becomes a [`super::poke::Poke`].
//!
//! ## Two snapshots, and why "changed" is not "not equal"
//!
//! Every filter compares the CURRENT value either with a number the user typed or with a snapshot.
//! There are two snapshots, and the distinction is what keeps the filters from being duplicates:
//!
//! - `previous` is the RAM as it was at the last filter (or the start). "Greater than previous",
//!   "equal to previous" and so on compare with it, and every filter then moves it forward to now,
//!   which is what lets a search be stepped: "went down", play, "went down again".
//! - `start` is the RAM as it was when the search began, and is never moved. "Changed" and
//!   "unchanged" compare with it, so "unchanged" keeps addresses that are the same as at the start
//!   however many rounds have passed in between.
//!
//! ## Memory
//!
//! The candidate list is `None` until the first filter, meaning "every address": a PlayStation's
//! 2 MB of RAM is two million candidates, and materialising them only to keep most of them after
//! one filter would be eight megabytes of `u32` for nothing. After the first filter it is a sorted
//! list of the survivors. The two snapshots are one copy of the region each.
//!
//! Values are little endian. See the note on endianness in [`super::poke`].

/// How wide a value the search reads at each address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchWidth {
    Bits8,
    Bits16,
    Bits32,
}

impl SearchWidth {
    pub const fn bytes(self) -> usize {
        match self {
            SearchWidth::Bits8 => 1,
            SearchWidth::Bits16 => 2,
            SearchWidth::Bits32 => 4,
        }
    }

    pub const fn max_value(self) -> u32 {
        match self {
            SearchWidth::Bits8 => 0xFF,
            SearchWidth::Bits16 => 0xFFFF,
            SearchWidth::Bits32 => u32::MAX,
        }
    }
}

/// One filter. See the module note for which snapshot each one compares with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchFilter {
    EqualToPrevious,
    NotEqualToPrevious,
    GreaterThanPrevious,
    LessThanPrevious,
    /// Different from the value when the search started.
    Changed,
    /// The same as the value when the search started.
    Unchanged,
    EqualTo(u32),
    NotEqualTo(u32),
    GreaterThan(u32),
    LessThan(u32),
    /// Exactly `n` more than the previous value, wrapping at the width.
    IncreasedBy(u32),
    /// Exactly `n` less than the previous value, wrapping at the width.
    DecreasedBy(u32),
}

impl SearchFilter {
    fn keeps(self, now: u32, previous: u32, start: u32, mask: u32) -> bool {
        match self {
            SearchFilter::EqualToPrevious => now == previous,
            SearchFilter::NotEqualToPrevious => now != previous,
            SearchFilter::GreaterThanPrevious => now > previous,
            SearchFilter::LessThanPrevious => now < previous,
            SearchFilter::Changed => now != start,
            SearchFilter::Unchanged => now == start,
            SearchFilter::EqualTo(value) => now == value,
            SearchFilter::NotEqualTo(value) => now != value,
            SearchFilter::GreaterThan(value) => now > value,
            SearchFilter::LessThan(value) => now < value,
            SearchFilter::IncreasedBy(delta) => now == previous.wrapping_add(delta) & mask,
            SearchFilter::DecreasedBy(delta) => now == previous.wrapping_sub(delta) & mask,
        }
    }

    /// The typed number, if this filter has one.
    fn operand(self) -> Option<u32> {
        match self {
            SearchFilter::EqualTo(v)
            | SearchFilter::NotEqualTo(v)
            | SearchFilter::GreaterThan(v)
            | SearchFilter::LessThan(v)
            | SearchFilter::IncreasedBy(v)
            | SearchFilter::DecreasedBy(v) => Some(v),
            _ => None,
        }
    }
}

/// One surviving address, with what it holds now and what it held at the last filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchHit {
    pub address: u32,
    pub current: u32,
    pub previous: u32,
}

/// A search in progress.
#[derive(Debug, Clone)]
pub struct RamSearch {
    width: SearchWidth,
    /// Step between candidate addresses: 1, or the width when the search is aligned.
    step: usize,
    start: Vec<u8>,
    previous: Vec<u8>,
    /// `None` means every address (see the module note). Sorted ascending once materialised.
    candidates: Option<Vec<u32>>,
    filters_applied: u32,
}

/// Reads one little-endian value of `width` at `offset`, or `None` past the end.
pub fn read_value(ram: &[u8], offset: usize, width: SearchWidth) -> Option<u32> {
    let end = offset.checked_add(width.bytes())?;
    let bytes = ram.get(offset..end)?;
    Some(match width {
        SearchWidth::Bits8 => u32::from(bytes[0]),
        SearchWidth::Bits16 => u32::from(u16::from_le_bytes([bytes[0], bytes[1]])),
        SearchWidth::Bits32 => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
    })
}

impl RamSearch {
    /// Starts a search over `ram`, which is copied as both snapshots.
    ///
    /// `aligned` steps candidates by the width, which is what a 16 or 32 bit value on a 16 or 32
    /// bit machine almost always is. Unaligned is right for the 8-bit systems, where a 16-bit
    /// counter can start at any byte.
    pub fn start(ram: &[u8], width: SearchWidth, aligned: bool) -> Result<Self, String> {
        if ram.len() < width.bytes() {
            return Err(format!(
                "system RAM is {} byte(s), too small to search for {}-byte values",
                ram.len(),
                width.bytes()
            ));
        }
        if u32::try_from(ram.len()).is_err() {
            return Err("system RAM is larger than 4 GB, which a search cannot address".into());
        }
        Ok(Self {
            width,
            step: if aligned { width.bytes() } else { 1 },
            start: ram.to_vec(),
            previous: ram.to_vec(),
            candidates: None,
            filters_applied: 0,
        })
    }

    pub fn width(&self) -> SearchWidth {
        self.width
    }

    pub fn filters_applied(&self) -> u32 {
        self.filters_applied
    }

    /// How many addresses a fresh search over a region of this length starts with.
    fn all_count(&self) -> usize {
        let len = self.previous.len();
        let width = self.width.bytes();
        if len < width {
            0
        } else {
            (len - width) / self.step + 1
        }
    }

    /// How many candidates are left.
    pub fn count(&self) -> usize {
        match &self.candidates {
            Some(list) => list.len(),
            None => self.all_count(),
        }
    }

    /// Applies one filter against `ram`, which is the region as it is NOW, and moves the
    /// `previous` snapshot forward to it. Returns how many candidates survive.
    ///
    /// A region whose length changed since the start (a different game on the same core would
    /// have been a new search, so this is a core resizing mid-session) is not refused: candidates
    /// past the new end are dropped, because there is nothing there to compare.
    pub fn filter(&mut self, ram: &[u8], filter: SearchFilter) -> Result<usize, String> {
        let mask = self.width.max_value();
        if let Some(operand) = filter.operand() {
            if operand > mask {
                return Err(format!(
                    "{operand} does not fit in a {}-bit value; the largest is {mask}",
                    self.width.bytes() * 8
                ));
            }
        }
        let width = self.width;
        let (start, previous) = (&self.start, &self.previous);
        let keep = |address: u32| -> bool {
            let offset = address as usize;
            let (Some(now), Some(prev), Some(first)) = (
                read_value(ram, offset, width),
                read_value(previous, offset, width),
                read_value(start, offset, width),
            ) else {
                return false;
            };
            filter.keeps(now, prev, first, mask)
        };
        let survivors: Vec<u32> = match self.candidates.take() {
            Some(list) => list.into_iter().filter(|&a| keep(a)).collect(),
            None => {
                let count = self.all_count();
                (0..count)
                    .map(|i| (i * self.step) as u32)
                    .filter(|&a| keep(a))
                    .collect()
            }
        };
        let count = survivors.len();
        self.candidates = Some(survivors);
        self.previous = ram.to_vec();
        self.filters_applied += 1;
        Ok(count)
    }

    /// Up to `limit` survivors in address order, each with its current and previous value.
    ///
    /// Capped because the first screen after "start" can be two million rows, which no list can
    /// draw and nobody can read. The count says how many there really are.
    pub fn results(&self, ram: &[u8], limit: usize) -> Vec<SearchHit> {
        let hit = |address: u32| -> Option<SearchHit> {
            let offset = address as usize;
            Some(SearchHit {
                address,
                current: read_value(ram, offset, self.width)?,
                previous: read_value(&self.previous, offset, self.width)?,
            })
        };
        match &self.candidates {
            Some(list) => list.iter().filter_map(|&a| hit(a)).take(limit).collect(),
            None => (0..self.all_count())
                .map(|i| (i * self.step) as u32)
                .filter_map(hit)
                .take(limit)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ram(bytes: &[(usize, u8)], len: usize) -> Vec<u8> {
        let mut ram = vec![0u8; len];
        for &(offset, value) in bytes {
            ram[offset] = value;
        }
        ram
    }

    fn addresses(search: &RamSearch, ram: &[u8]) -> Vec<u32> {
        search
            .results(ram, usize::MAX)
            .iter()
            .map(|h| h.address)
            .collect()
    }

    #[test]
    fn reads_little_endian_at_each_width() {
        let ram = [0x01, 0x02, 0x03, 0x04, 0x05];
        assert_eq!(read_value(&ram, 0, SearchWidth::Bits8), Some(0x01));
        assert_eq!(read_value(&ram, 1, SearchWidth::Bits16), Some(0x0302));
        assert_eq!(read_value(&ram, 1, SearchWidth::Bits32), Some(0x0504_0302));
        assert_eq!(read_value(&ram, 2, SearchWidth::Bits32), None);
        assert_eq!(read_value(&ram, usize::MAX, SearchWidth::Bits16), None);
    }

    #[test]
    fn a_fresh_search_counts_every_address() {
        let r = vec![0u8; 16];
        assert_eq!(RamSearch::start(&r, SearchWidth::Bits8, false).unwrap().count(), 16);
        assert_eq!(RamSearch::start(&r, SearchWidth::Bits16, false).unwrap().count(), 15);
        assert_eq!(RamSearch::start(&r, SearchWidth::Bits16, true).unwrap().count(), 8);
        assert_eq!(RamSearch::start(&r, SearchWidth::Bits32, true).unwrap().count(), 4);
        assert_eq!(RamSearch::start(&r, SearchWidth::Bits32, false).unwrap().count(), 13);
    }

    #[test]
    fn refuses_a_region_smaller_than_the_width() {
        assert!(RamSearch::start(&[1, 2, 3], SearchWidth::Bits32, false).is_err());
        assert!(RamSearch::start(&[], SearchWidth::Bits8, false).is_err());
    }

    #[test]
    fn finds_a_lives_counter_in_three_rounds() {
        // Lives at 0x05 go 3 -> 2 -> 2 -> 1. A noisy byte at 0x09 counts frames.
        let r0 = ram(&[(5, 3), (9, 10), (12, 3)], 16);
        let mut search = RamSearch::start(&r0, SearchWidth::Bits8, false).unwrap();
        let r1 = ram(&[(5, 2), (9, 11), (12, 3)], 16);
        assert_eq!(search.filter(&r1, SearchFilter::LessThanPrevious).unwrap(), 1);
        assert_eq!(addresses(&search, &r1), vec![5]);
        let r2 = ram(&[(5, 2), (9, 12), (12, 3)], 16);
        assert_eq!(search.filter(&r2, SearchFilter::EqualToPrevious).unwrap(), 1);
        let r3 = ram(&[(5, 1), (9, 13), (12, 3)], 16);
        assert_eq!(search.filter(&r3, SearchFilter::EqualTo(1)).unwrap(), 1);
        assert_eq!(search.filters_applied(), 3);
        let hits = search.results(&r3, 10);
        assert_eq!(
            hits,
            vec![SearchHit {
                address: 5,
                current: 1,
                previous: 1
            }]
        );
    }

    #[test]
    fn greater_and_not_equal_against_previous() {
        let r0 = ram(&[(0, 5), (1, 5), (2, 5)], 4);
        let r1 = ram(&[(0, 6), (1, 4), (2, 5)], 4);
        let mut gt = RamSearch::start(&r0, SearchWidth::Bits8, false).unwrap();
        gt.filter(&r1, SearchFilter::GreaterThanPrevious).unwrap();
        assert_eq!(addresses(&gt, &r1), vec![0]);
        let mut ne = RamSearch::start(&r0, SearchWidth::Bits8, false).unwrap();
        ne.filter(&r1, SearchFilter::NotEqualToPrevious).unwrap();
        assert_eq!(addresses(&ne, &r1), vec![0, 1]);
    }

    #[test]
    fn changed_and_unchanged_compare_with_the_start_not_the_previous() {
        // Address 0 goes 1 -> 2 -> 1: it changed between rounds and ends where it started.
        // Address 1 goes 1 -> 1 -> 2: unchanged at round one, changed at round two.
        let r0 = ram(&[(0, 1), (1, 1)], 2);
        let r1 = ram(&[(0, 2), (1, 1)], 2);
        let r2 = ram(&[(0, 1), (1, 2)], 2);

        let mut unchanged = RamSearch::start(&r0, SearchWidth::Bits8, false).unwrap();
        unchanged.filter(&r1, SearchFilter::NotEqualToPrevious).unwrap();
        assert_eq!(addresses(&unchanged, &r1), vec![0]);
        unchanged.filter(&r2, SearchFilter::Unchanged).unwrap();
        // Back where it started, even though it moved twice: "unchanged" means since the start.
        assert_eq!(addresses(&unchanged, &r2), vec![0]);

        let mut changed = RamSearch::start(&r0, SearchWidth::Bits8, false).unwrap();
        changed.filter(&r1, SearchFilter::Unchanged).unwrap();
        assert_eq!(addresses(&changed, &r1), vec![1]);
        changed.filter(&r2, SearchFilter::Changed).unwrap();
        assert_eq!(addresses(&changed, &r2), vec![1]);
    }

    #[test]
    fn value_filters_at_sixteen_bits() {
        // 0x0123 at 0 (aligned), 0x0456 at 2, and an unaligned 0x0300 straddling 1..3.
        let r = vec![0x23, 0x01, 0x56, 0x04];
        let mut search = RamSearch::start(&r, SearchWidth::Bits16, true).unwrap();
        assert_eq!(search.filter(&r, SearchFilter::EqualTo(0x0456)).unwrap(), 1);
        assert_eq!(addresses(&search, &r), vec![2]);

        let mut unaligned = RamSearch::start(&r, SearchWidth::Bits16, false).unwrap();
        // 0x0123, 0x5601, 0x0456 at offsets 0, 1, 2.
        assert_eq!(unaligned.filter(&r, SearchFilter::GreaterThan(0x1000)).unwrap(), 1);
        assert_eq!(addresses(&unaligned, &r), vec![1]);

        let mut lt = RamSearch::start(&r, SearchWidth::Bits16, false).unwrap();
        assert_eq!(lt.filter(&r, SearchFilter::LessThan(0x0400)).unwrap(), 1);
        assert_eq!(addresses(&lt, &r), vec![0]);

        let mut ne = RamSearch::start(&r, SearchWidth::Bits16, false).unwrap();
        assert_eq!(ne.filter(&r, SearchFilter::NotEqualTo(0x0123)).unwrap(), 2);
    }

    #[test]
    fn thirty_two_bit_values_and_operand_range() {
        let mut r0 = vec![0u8; 8];
        r0[4..8].copy_from_slice(&100_000u32.to_le_bytes());
        let mut search = RamSearch::start(&r0, SearchWidth::Bits32, true).unwrap();
        let mut r1 = r0.clone();
        r1[4..8].copy_from_slice(&100_250u32.to_le_bytes());
        assert_eq!(search.filter(&r1, SearchFilter::IncreasedBy(250)).unwrap(), 1);
        assert_eq!(addresses(&search, &r1), vec![4]);

        let mut byte = RamSearch::start(&r0, SearchWidth::Bits8, false).unwrap();
        assert!(byte.filter(&r0, SearchFilter::EqualTo(256)).is_err());
        // A refused filter consumes nothing.
        assert_eq!(byte.filters_applied(), 0);
        assert_eq!(byte.count(), 8);
    }

    #[test]
    fn increased_and_decreased_by_wrap_at_the_width() {
        let r0 = vec![0xFF, 0x00];
        let r1 = vec![0x01, 0xFE];
        let mut up = RamSearch::start(&r0, SearchWidth::Bits8, false).unwrap();
        assert_eq!(up.filter(&r1, SearchFilter::IncreasedBy(2)).unwrap(), 1);
        assert_eq!(addresses(&up, &r1), vec![0]);
        let mut down = RamSearch::start(&r0, SearchWidth::Bits8, false).unwrap();
        assert_eq!(down.filter(&r1, SearchFilter::DecreasedBy(2)).unwrap(), 1);
        assert_eq!(addresses(&down, &r1), vec![1]);
    }

    #[test]
    fn results_are_capped_but_count_is_not() {
        let r = vec![7u8; 1000];
        let search = RamSearch::start(&r, SearchWidth::Bits8, false).unwrap();
        assert_eq!(search.count(), 1000);
        let hits = search.results(&r, 50);
        assert_eq!(hits.len(), 50);
        assert_eq!(hits[49].address, 49);
        assert!(hits.iter().all(|h| h.current == 7 && h.previous == 7));
    }

    #[test]
    fn a_shrunk_region_drops_candidates_past_the_end() {
        let r0 = vec![1u8; 8];
        let mut search = RamSearch::start(&r0, SearchWidth::Bits8, false).unwrap();
        let r1 = vec![1u8; 4];
        assert_eq!(search.filter(&r1, SearchFilter::EqualToPrevious).unwrap(), 4);
        assert_eq!(addresses(&search, &r1), vec![0, 1, 2, 3]);
    }

    #[test]
    fn filtering_to_nothing_is_a_result_not_an_error() {
        let r = vec![0u8; 4];
        let mut search = RamSearch::start(&r, SearchWidth::Bits8, false).unwrap();
        assert_eq!(search.filter(&r, SearchFilter::NotEqualToPrevious).unwrap(), 0);
        assert!(search.results(&r, 10).is_empty());
        // And a further filter over nothing stays at nothing.
        assert_eq!(search.filter(&r, SearchFilter::EqualTo(0)).unwrap(), 0);
    }

    #[test]
    fn previous_moves_forward_at_every_filter() {
        let mut search = RamSearch::start(&[10], SearchWidth::Bits8, false).unwrap();
        search.filter(&[9], SearchFilter::LessThanPrevious).unwrap();
        search.filter(&[8], SearchFilter::LessThanPrevious).unwrap();
        assert_eq!(search.count(), 1);
        let hit = search.results(&[8], 1)[0];
        assert_eq!((hit.current, hit.previous), (8, 8));
        // And a value that went back UP fails "less than previous".
        search.filter(&[9], SearchFilter::LessThanPrevious).unwrap();
        assert_eq!(search.count(), 0);
    }
}
