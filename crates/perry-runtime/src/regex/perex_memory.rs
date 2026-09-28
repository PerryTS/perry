//! Operation-owned native scratch, bounded by the operation's `MemoryBudget`.
//! No GC pointer may be stored in these buffers.
//!
//! This scratch is NOT reported to the collector as external side bytes
//! (#11549). Everything here is freed by the operation that allocated it,
//! when that operation returns or unwinds; no collection can ever reclaim a
//! byte of it, and no collection is needed for it to be released. Reporting it
//! told the old-reclaim pacing the opposite: every per-call buffer's release
//! landed in `GC_EXTERNAL_SIDE_DRAINED_SINCE_FULL`, which is held as pressure
//! until the next FULL collection, so a regex loop that took the owned search
//! path (a program with more registers than the lent cell holds) was paced by
//! phantom bytes into a budgeted full mark-sweep every few hundred calls.
//!
//! What bounds it instead is the budget: every buffer, inline charge and
//! reservation is checked against the operation's hard limit
//! (`perex_api::SCRATCH_BYTES`) before it exists, so one operation can never
//! hold more than that. Storage whose size follows the SUBJECT rather than
//! that limit (`perex_replace_direct::Spans`, `perex_replace_storage`'s
//! native piece records) is not scratch in this sense and is still reported.

use std::cell::Cell;
use std::ops::{Deref, DerefMut};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StorageError {
    Limit,
    Allocation,
}

/// One operation's hard scratch/result-metadata limit. Simultaneous old/new
/// buffers during a search rebuffer count against the same limit.
pub(crate) struct MemoryBudget {
    limit: usize,
    live: Cell<usize>,
    peak: Cell<usize>,
}

impl MemoryBudget {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            limit,
            live: Cell::new(0),
            peak: Cell::new(0),
        }
    }

    #[cfg(test)]
    pub(crate) fn live_bytes(&self) -> usize {
        self.live.get()
    }
    #[cfg(test)]
    pub(crate) fn peak_bytes(&self) -> usize {
        self.peak.get()
    }

    fn check(&self, extra: usize) -> Result<usize, StorageError> {
        self.live
            .get()
            .checked_add(extra)
            .filter(|&n| n <= self.limit)
            .ok_or(StorageError::Limit)
    }
}

/// Bytes charged to an operation's limit for storage it holds without
/// allocating, such as match slots kept inline. The limit and peak see them as
/// they would a buffer's; the collector is not told, since nothing is on its
/// heap or the native heap.
pub(crate) struct Charge<'a> {
    budget: &'a MemoryBudget,
    bytes: usize,
}
impl<'a> Charge<'a> {
    pub(crate) fn new(budget: &'a MemoryBudget, bytes: usize) -> Result<Self, StorageError> {
        let live = budget.check(bytes)?;
        budget.live.set(live);
        budget.peak.set(budget.peak.get().max(live));
        Ok(Self { budget, bytes })
    }
}
impl Drop for Charge<'_> {
    fn drop(&mut self) {
        self.budget.live.set(self.budget.live.get() - self.bytes);
    }
}

/// Charge a stable native allocation the caller owns, such as a replacer
/// call's argument slots, to the operation's limit. Its GC-bearing slots are
/// registered with the shadow stack separately; like every other buffer here
/// it is released by the operation, not by a collection, so the collector is
/// not told about it.
pub(super) struct Reservation<'a> {
    budget: &'a MemoryBudget,
    bytes: usize,
}
impl<'a> Reservation<'a> {
    pub(super) fn new(budget: &'a MemoryBudget, bytes: usize) -> Result<Self, StorageError> {
        let live = budget.check(bytes)?;
        budget.live.set(live);
        budget.peak.set(budget.peak.get().max(live));
        Ok(Self { budget, bytes })
    }
}
impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        self.budget.live.set(self.budget.live.get() - self.bytes);
    }
}

/// Stable initialized native allocation, charged to the operation's limit for
/// as long as it lives. Creating one never collects.
pub(crate) struct Buffer<'a, T: Copy + Default> {
    data: Vec<T>,
    budget: &'a MemoryBudget,
    bytes: usize,
}

impl<'a, T: Copy + Default> Buffer<'a, T> {
    pub(crate) fn new(budget: &'a MemoryBudget, count: usize) -> Result<Self, StorageError> {
        let requested = count
            .checked_mul(std::mem::size_of::<T>())
            .ok_or(StorageError::Limit)?;
        budget.check(requested)?;
        let mut data = Vec::new();
        data.try_reserve_exact(count)
            .map_err(|_| StorageError::Allocation)?;
        let bytes = data
            .capacity()
            .checked_mul(std::mem::size_of::<T>())
            .ok_or(StorageError::Limit)?;
        let live = budget.check(bytes)?;
        data.resize(count, T::default());
        budget.live.set(live);
        budget.peak.set(budget.peak.get().max(live));
        Ok(Self {
            data,
            budget,
            bytes,
        })
    }
}

impl<T: Copy + Default> Deref for Buffer<'_, T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.data
    }
}

impl<T: Copy + Default> DerefMut for Buffer<'_, T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.data
    }
}

impl<T: Copy + Default> Drop for Buffer<'_, T> {
    fn drop(&mut self) {
        self.budget.live.set(self.budget.live.get() - self.bytes);
    }
}
