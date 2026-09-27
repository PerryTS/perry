//! Thread-exit release of process-global address-keyed state (#11471).
//!
//! When a thread exits, [`Arena::drop`](super::Arena) hands its arena blocks
//! back to the allocator, and another thread's arena may reuse the same
//! addresses. A process-global table keyed by (or holding) an address inside
//! those blocks then describes a DIFFERENT object: #11319 (closure side
//! tables), #11462 (`tls.rs` roots) and #11470 (typed-array kind caches) each
//! shipped as a flake before anyone traced it here.
//!
//! This module is the one place those tables hear about a thread exit.
//! `Arena::drop` calls [`release_freed_ranges`] with the blocks it is about to
//! free; that runs every hook registered through
//! [`register_thread_exit_range_hook`]. A hook drops every entry whose key or
//! stored value lies in [`FreedRanges`].
//!
//! # Rules for a hook
//!
//! It runs inside a TLS destructor of the exiting thread:
//!
//! * no thread-local state (it may already be gone), no GC allocation, no
//!   collection;
//! * process-global locks are fine (the tables it touches are exactly those),
//!   but take them with `lock()`/`try_lock()` and tolerate poisoning;
//! * it must not call back into JS.
//!
//! `scripts/thread_exit_address_globals.py` requires every process-global
//! static that can hold an arena address to carry a verdict; a
//! `thread_exit_invalidated` verdict names its hook, and the gate checks the
//! hook is registered here or called by `Arena::drop`.

use std::sync::{Mutex, PoisonError};

/// The address ranges `[start, end)` of the arena blocks an exiting thread is
/// about to free, sorted by start. Arena blocks never overlap, so membership
/// is one binary search.
pub struct FreedRanges {
    sorted: Vec<(usize, usize)>,
}

impl FreedRanges {
    pub fn new(ranges: &[(usize, usize)]) -> Self {
        let mut sorted: Vec<(usize, usize)> = ranges
            .iter()
            .copied()
            .filter(|&(start, end)| start < end)
            .collect();
        sorted.sort_unstable_by_key(|&(start, _)| start);
        Self { sorted }
    }

    pub fn is_empty(&self) -> bool {
        self.sorted.is_empty()
    }

    /// Does `addr` lie inside one of the freed blocks?
    #[inline]
    pub fn contains(&self, addr: usize) -> bool {
        if addr == 0 {
            return false;
        }
        let after = self.sorted.partition_point(|&(start, _)| start <= addr);
        after > 0 && addr < self.sorted[after - 1].1
    }

    /// Does this 64-bit word name freed memory, read either as a NaN-boxed
    /// heap reference (pointer, string or bigint tag) or as a raw address
    /// (closure pointers and `*mut Promise` are stored raw in several
    /// tables)? A plain number never decodes to a block address unless it is
    /// one, so erring toward "names it" can only drop an entry that pointed at
    /// the dead thread's heap anyway.
    #[inline]
    pub fn holds_bits(&self, bits: u64) -> bool {
        let tag = bits >> 48;
        if matches!(tag, 0x7FFD | 0x7FFF | 0x7FFA) {
            return self.contains((bits & 0x0000_FFFF_FFFF_FFFF) as usize);
        }
        tag == 0 && self.contains(bits as usize)
    }

    /// [`holds_bits`](Self::holds_bits) for an `f64`-typed JS value.
    #[inline]
    pub fn holds_value(&self, value: f64) -> bool {
        self.holds_bits(value.to_bits())
    }

    /// [`holds_bits`](Self::holds_bits) for an `i64`-typed word.
    #[inline]
    pub fn holds_i64(&self, word: i64) -> bool {
        self.holds_bits(word as u64)
    }
}

/// Hooks for process-global tables, in this crate or downstream
/// (`perry-stdlib`). Function pointers only: no heap address lives here.
static RANGE_HOOKS: Mutex<Vec<fn(&FreedRanges)>> = Mutex::new(Vec::new());

/// Register `hook` to run whenever a thread's arena frees its blocks.
/// Idempotent per function. Call it before the table's first insert (a
/// `Once` on the insert path is the usual shape), so no entry predates it.
pub fn register_thread_exit_range_hook(hook: fn(&FreedRanges)) {
    let mut hooks = RANGE_HOOKS.lock().unwrap_or_else(PoisonError::into_inner);
    if !hooks
        .iter()
        .any(|existing| std::ptr::fn_addr_eq(*existing, hook))
    {
        hooks.push(hook);
    }
}

/// Run every registered hook over `ranges`. Called by `Arena::drop`.
pub(crate) fn release_freed_ranges(ranges: &[(usize, usize)]) {
    let freed = FreedRanges::new(ranges);
    if freed.is_empty() {
        return;
    }
    // Copied out so a hook may itself take locks without holding this one.
    let hooks: Vec<fn(&FreedRanges)> = RANGE_HOOKS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    for hook in hooks {
        hook(&freed);
    }
}

#[cfg(test)]
mod tests {
    use super::FreedRanges;

    #[test]
    fn membership_and_value_decoding() {
        let freed = FreedRanges::new(&[(0x2000, 0x3000), (0x1000, 0x1800)]);
        assert!(freed.contains(0x1000));
        assert!(freed.contains(0x17ff));
        assert!(!freed.contains(0x1800));
        assert!(freed.contains(0x2fff));
        assert!(!freed.contains(0x3000));
        assert!(!freed.contains(0));
        assert!(freed.holds_bits(0x7FFD_0000_0000_2010));
        assert!(freed.holds_bits(0x7FFF_0000_0000_1010));
        assert!(freed.holds_bits(0x2010));
        assert!(!freed.holds_bits(0x7FFD_0000_0000_4000));
        assert!(!freed.holds_value(1.5));
        assert!(!freed.holds_bits(0x7FFE_0000_0000_2010), "an int32 is not an address");
    }
}
