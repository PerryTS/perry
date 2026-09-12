//! Operation-owned native scratch, charged to Perry's external-byte budget.
//! No GC pointer may be stored in these buffers. Allocation/accounting can
//! collect, so callers must release all program/subject views first.

use std::cell::Cell;
use std::ops::{Deref, DerefMut};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StorageError {
    Limit,
    Allocation,
    Abrupt(u64),
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

    pub(crate) fn live_bytes(&self) -> usize {
        self.live.get()
    }
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

/// Account a stable native allocation whose GC-bearing slots are separately
/// registered with the host's mutable root scanner before this can collect.
pub(super) struct Reservation<'a> {
    budget: &'a MemoryBudget,
    bytes: usize,
}
impl<'a> Reservation<'a> {
    pub(super) fn new(budget: &'a MemoryBudget, bytes: usize) -> Result<Self, StorageError> {
        let live = budget.check(bytes)?;
        let owned = Self { budget, bytes };
        budget.live.set(live);
        budget.peak.set(budget.peak.get().max(live));
        if bytes != 0 {
            crate::exception::catch_js_throw(|| crate::gc::gc_note_external_side_alloc(bytes))
                .map_err(|value| StorageError::Abrupt(value.to_bits()))?;
        }
        Ok(owned)
    }
}
impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        self.budget.live.set(self.budget.live.get() - self.bytes);
        if self.bytes != 0 {
            crate::gc::gc_note_external_side_free(self.bytes);
        }
    }
}

/// Stable initialized native allocation. Its accounting owner is established
/// before notifying the collector, so a collecting/unwinding notification
/// cannot strand a buffer or leave its bytes charged.
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
        let owned = Self {
            data,
            budget,
            bytes,
        };
        budget.live.set(live);
        budget.peak.set(budget.peak.get().max(live));
        if bytes != 0 {
            crate::exception::catch_js_throw(|| crate::gc::gc_note_external_side_alloc(bytes))
                .map_err(|value| StorageError::Abrupt(value.to_bits()))?;
        }
        Ok(owned)
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
        if self.bytes != 0 {
            crate::gc::gc_note_external_side_free(self.bytes);
        }
    }
}
