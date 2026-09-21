//! A bounded ring of save states, so holding a button can walk time backwards.
//!
//! Rewind is save states used as a tape rather than as bookmarks: snapshot every few
//! frames, keep as many as a memory budget allows, and to rewind, load the newest one and
//! throw it away. Walking backwards is therefore last-in-first-out, and the depth of the
//! tape in seconds is `snapshots * interval / fps` rather than anything this module can
//! know by itself.
//!
//! Two properties drive the whole design.
//!
//! **A byte budget, not a snapshot count.** Save-state sizes differ by two orders of
//! magnitude across the systems Continuum runs - roughly 13 KB for the NES against a
//! megabyte or more for the PlayStation - so "keep 600 snapshots" is ten seconds of rewind
//! on one system and an out-of-memory kill on another. iOS terminates a process that grows
//! too large rather than paging it out, which makes an unbounded count the difference
//! between a feature and a crash. The budget is the promise; how much rewind it buys you
//! varies by system, and that is the honest way round.
//!
//! **No allocation in the steady state.** At a snapshot every six frames a 500 KB state is
//! 5 MB of allocation and 5 MB of free per second, forever, while a game is running. So
//! evicted buffers are not dropped: they go to a free list and are handed back out for the
//! next snapshot. After the tape fills once, pushing a snapshot is a `memcpy` into memory
//! this module already owns.
//!
//! Nothing here knows what a save state contains. It is bytes with a budget, which is why
//! it needs no core, no session and no renderer, and can be unit tested on its own.

use std::collections::VecDeque;

use crate::error::BridgeError;

/// Upper bound on retained-but-unused buffers.
///
/// The free list exists to absorb the steady state, where one buffer is evicted per
/// snapshot pushed, so it only ever needs to be a couple deep. It grows beyond that only
/// when the budget is lowered or the tape is cleared, and holding every buffer from a
/// discarded tape would defeat the point of having a budget at all.
const MAX_POOLED_BUFFERS: usize = 4;

/// A fixed-memory tape of save states, newest last.
#[derive(Debug, Default)]
pub struct RewindBuffer {
    /// `0` disables rewind entirely and is the default.
    budget_bytes: usize,
    /// Oldest at the front, newest at the back. Rewinding pops the back.
    snapshots: VecDeque<Vec<u8>>,
    /// Evicted buffers kept for reuse. See [`MAX_POOLED_BUFFERS`].
    pool: Vec<Vec<u8>>,
    /// Running total of `snapshots`, maintained incrementally so the budget check does
    /// not walk the deque on every push.
    bytes: usize,
    /// How many snapshots have been evicted to stay inside the budget. Diagnostic only:
    /// a steadily climbing figure is the tape working as intended, not a fault.
    evicted: u64,
}

impl RewindBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the memory ceiling in bytes. `0` disables rewind and releases everything.
    ///
    /// Lowering the budget evicts oldest-first until the tape fits, which is the same
    /// direction of loss as a full tape and so needs no separate policy.
    pub fn set_budget_bytes(&mut self, budget_bytes: usize) {
        self.budget_bytes = budget_bytes;
        if budget_bytes == 0 {
            self.clear();
            // A disabled tape should not sit on a pool of megabyte buffers.
            self.pool = Vec::new();
            self.pool.shrink_to_fit();
            return;
        }
        self.evict_to_fit();
    }

    pub fn budget_bytes(&self) -> usize {
        self.budget_bytes
    }

    pub fn is_enabled(&self) -> bool {
        self.budget_bytes > 0
    }

    /// Number of snapshots currently on the tape.
    pub fn len(&self) -> usize {
        self.snapshots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    /// Bytes currently held by snapshots, excluding the free list.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn evicted(&self) -> u64 {
        self.evicted
    }

    /// Takes a snapshot by letting `fill` write directly into a pooled buffer.
    ///
    /// The closure receives a slice of exactly `size` bytes and returns how many it
    /// actually wrote, mirroring `EmulatorCore::save_state`. Writing in place is the whole
    /// point: the alternative shape, taking a `&[u8]` the caller already filled, would
    /// copy every snapshot twice.
    ///
    /// A `size` of `0` is not an error. It means the core has no save-state support, and a
    /// tape of empty snapshots would be a silent infinite loop of nothing, so it is
    /// declined. If `fill` fails the buffer returns to the pool and the tape is untouched,
    /// so a core that refuses one snapshot does not cost the history already recorded.
    pub fn push_with<F>(&mut self, size: usize, fill: F) -> Result<bool, BridgeError>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, BridgeError>,
    {
        if !self.is_enabled() || size == 0 {
            return Ok(false);
        }
        // A single state larger than the whole budget can never be retained: pushing it
        // would immediately evict itself. Refusing up front keeps the tape coherent
        // instead of thrashing once per snapshot interval.
        if size > self.budget_bytes {
            return Ok(false);
        }

        let mut buffer = self.take_pooled(size);
        let written = match fill(&mut buffer[..size]) {
            Ok(written) => written.min(size),
            Err(err) => {
                self.return_pooled(buffer);
                return Err(err);
            }
        };
        if written == 0 {
            self.return_pooled(buffer);
            return Ok(false);
        }
        buffer.truncate(written);

        self.bytes += buffer.len();
        self.snapshots.push_back(buffer);
        self.evict_to_fit();
        Ok(true)
    }

    /// Removes and returns the newest snapshot, or `None` when the tape is empty.
    ///
    /// The caller owns the buffer while loading it and should hand it back through
    /// [`RewindBuffer::recycle`] afterwards, which is what keeps rewinding allocation-free
    /// in the same way pushing is.
    pub fn pop(&mut self) -> Option<Vec<u8>> {
        let snapshot = self.snapshots.pop_back()?;
        self.bytes -= snapshot.len();
        Some(snapshot)
    }

    /// Returns a buffer from [`RewindBuffer::pop`] to the free list.
    pub fn recycle(&mut self, buffer: Vec<u8>) {
        self.return_pooled(buffer);
    }

    /// Drops the whole tape, keeping a little of it as free list.
    ///
    /// Called whenever the timeline the tape describes stops being the one being played:
    /// a reset, a load-state, a new session. Rewinding into another game's history would
    /// be worse than not being able to rewind.
    pub fn clear(&mut self) {
        while let Some(snapshot) = self.snapshots.pop_back() {
            self.return_pooled(snapshot);
        }
        self.bytes = 0;
    }

    fn take_pooled(&mut self, size: usize) -> Vec<u8> {
        match self.pool.pop() {
            Some(mut buffer) => {
                // `resize` only reallocates when this buffer came from a core with a
                // smaller state, which happens on a system switch and not in steady state.
                buffer.clear();
                buffer.resize(size, 0);
                buffer
            }
            None => vec![0u8; size],
        }
    }

    fn return_pooled(&mut self, buffer: Vec<u8>) {
        if self.pool.len() < MAX_POOLED_BUFFERS {
            self.pool.push(buffer);
        }
    }

    /// Evicts oldest-first until the tape is inside its budget.
    ///
    /// The last snapshot is never evicted even if it alone exceeds the budget, because a
    /// tape holding one state is still a usable single step back, whereas an empty one
    /// silently does nothing. `push_with` already refuses oversized states, so this is a
    /// guard for the budget being lowered underneath an existing tape.
    fn evict_to_fit(&mut self) {
        while self.bytes > self.budget_bytes && self.snapshots.len() > 1 {
            if let Some(oldest) = self.snapshots.pop_front() {
                self.bytes -= oldest.len();
                self.return_pooled(oldest);
                self.evicted += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fills with a recognisable byte so a popped snapshot can be identified.
    fn push_marked(tape: &mut RewindBuffer, size: usize, marker: u8) -> bool {
        tape.push_with(size, |dst| {
            dst.fill(marker);
            Ok(dst.len())
        })
        .expect("fill does not fail")
    }

    #[test]
    fn disabled_by_default_and_records_nothing() {
        let mut tape = RewindBuffer::new();
        assert!(!tape.is_enabled());
        assert!(!push_marked(&mut tape, 128, 1));
        assert_eq!(tape.len(), 0);
    }

    #[test]
    fn rewinds_newest_first() {
        let mut tape = RewindBuffer::new();
        tape.set_budget_bytes(1024);
        push_marked(&mut tape, 16, 1);
        push_marked(&mut tape, 16, 2);
        push_marked(&mut tape, 16, 3);

        assert_eq!(tape.pop().expect("three pushed")[0], 3);
        assert_eq!(tape.pop().expect("two left")[0], 2);
        assert_eq!(tape.pop().expect("one left")[0], 1);
        assert!(tape.pop().is_none());
        assert_eq!(tape.bytes(), 0);
    }

    #[test]
    fn evicts_oldest_to_stay_within_budget() {
        let mut tape = RewindBuffer::new();
        tape.set_budget_bytes(100);
        // Ten 30-byte snapshots into a 100-byte budget: three fit.
        for i in 0..10 {
            push_marked(&mut tape, 30, i);
        }
        assert_eq!(tape.len(), 3);
        assert!(tape.bytes() <= 100);
        assert_eq!(tape.evicted(), 7);
        // The survivors must be the three most recent, in order.
        assert_eq!(tape.pop().expect("newest")[0], 9);
        assert_eq!(tape.pop().expect("middle")[0], 8);
        assert_eq!(tape.pop().expect("oldest kept")[0], 7);
    }

    #[test]
    fn refuses_a_state_larger_than_the_whole_budget() {
        let mut tape = RewindBuffer::new();
        tape.set_budget_bytes(64);
        assert!(!push_marked(&mut tape, 128, 1));
        assert_eq!(tape.len(), 0);
    }

    #[test]
    fn declines_a_core_without_save_state_support() {
        let mut tape = RewindBuffer::new();
        tape.set_budget_bytes(1024);
        assert!(!push_marked(&mut tape, 0, 1));
        assert_eq!(tape.len(), 0);
    }

    #[test]
    fn a_failed_snapshot_leaves_history_intact() {
        let mut tape = RewindBuffer::new();
        tape.set_budget_bytes(1024);
        push_marked(&mut tape, 16, 1);

        let outcome = tape.push_with(16, |_| {
            Err(BridgeError::SaveState("core said no".into()))
        });
        assert!(outcome.is_err());
        assert_eq!(tape.len(), 1, "the earlier snapshot survives");
        assert_eq!(tape.pop().expect("still there")[0], 1);
    }

    #[test]
    fn lowering_the_budget_trims_immediately() {
        let mut tape = RewindBuffer::new();
        tape.set_budget_bytes(1024);
        for i in 0..8 {
            push_marked(&mut tape, 100, i);
        }
        assert_eq!(tape.len(), 8);

        tape.set_budget_bytes(250);
        assert_eq!(tape.len(), 2);
        assert!(tape.bytes() <= 250);
        assert_eq!(tape.pop().expect("newest survives")[0], 7);
    }

    #[test]
    fn disabling_releases_everything() {
        let mut tape = RewindBuffer::new();
        tape.set_budget_bytes(1024);
        push_marked(&mut tape, 128, 1);
        tape.set_budget_bytes(0);
        assert!(!tape.is_enabled());
        assert_eq!(tape.len(), 0);
        assert_eq!(tape.bytes(), 0);
    }

    #[test]
    fn reuses_buffers_rather_than_growing_without_bound() {
        let mut tape = RewindBuffer::new();
        tape.set_budget_bytes(100);
        for i in 0..50 {
            push_marked(&mut tape, 30, i);
        }
        // Three on the tape, and the free list is capped however many were evicted.
        assert_eq!(tape.len(), 3);
        assert!(tape.pool.len() <= MAX_POOLED_BUFFERS);
    }

    #[test]
    fn a_truncated_write_is_recorded_at_its_real_length() {
        let mut tape = RewindBuffer::new();
        tape.set_budget_bytes(1024);
        // A core reporting a size of 64 but writing only 20 bytes.
        tape.push_with(64, |dst| {
            dst[..20].fill(7);
            Ok(20)
        })
        .expect("fill does not fail");
        assert_eq!(tape.bytes(), 20);
        assert_eq!(tape.pop().expect("pushed").len(), 20);
    }

    #[test]
    fn keeps_a_single_snapshot_when_the_budget_is_lowered_below_it() {
        let mut tape = RewindBuffer::new();
        tape.set_budget_bytes(1024);
        push_marked(&mut tape, 500, 1);
        // One step back is still worth more than nothing.
        tape.set_budget_bytes(100);
        assert_eq!(tape.len(), 1);
    }
}
