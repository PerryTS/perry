//! Software prefetch for the collector's pointer-chasing loops.
//!
//! # Why this exists
//!
//! On a promotion-heavy workload the copying minor's cost is not arithmetic,
//! it is DRAM latency over object headers. `gc-handoff/bench/retain.ts` keeps
//! 3 M records live; a single minor promotes ~750 k of them and touches each
//! header from three separate passes — the remembered-set dirty scan (which
//! classifies the child address), the mark drain (which re-reads the header to
//! walk its slots), and `clear_marks`. Between passes the cohort is ~54 MB, so
//! every pass starts cold and each header read is a full memory round trip.
//!
//! All three loops know their next targets several iterations ahead: the drain
//! and `clear_marks` walk a `Vec<*mut GcHeader>`, and the dirty scan walks a
//! contiguous slot range whose values decode to the addresses it is about to
//! classify. Issuing a prefetch for the entry `PREFETCH_DISTANCE` ahead
//! overlaps those round trips instead of serialising them.
//!
//! # Safety
//!
//! `prfm` / `_mm_prefetch` are architecturally faultless: an unmapped,
//! misaligned or nonsensical address is a no-op, never a signal. That is what
//! makes it usable on a *candidate* address (the dirty scan prefetches slot
//! values before they have been proven to be pointers) where a speculative
//! load would be a use-after-free waiting to happen.

/// How far ahead the collector's loops prefetch.
///
/// Deliberately modest: the loops do real work per element (a classify, a slot
/// walk), so the latency to hide is one DRAM access, not many. Too large a
/// distance evicts the line before it is used.
pub(super) const PREFETCH_DISTANCE: usize = 8;

/// Issue a non-faulting prefetch-for-read of the cache line holding `addr`.
///
/// A zero address is skipped only because it is the common "no entry" encoding
/// in the slots this is used on; every other value, valid or not, is safe to
/// hand to the instruction.
#[inline(always)]
pub(super) fn prefetch_read(addr: usize) {
    if addr == 0 {
        return;
    }
    #[cfg(target_arch = "aarch64")]
    // SAFETY: `prfm` has no architectural memory effect and cannot fault.
    unsafe {
        core::arch::asm!(
            "prfm pldl1keep, [{addr}]",
            addr = in(reg) addr,
            options(nostack, preserves_flags, readonly),
        );
    }
    #[cfg(target_arch = "x86_64")]
    // SAFETY: `prefetcht0` has no architectural memory effect and cannot fault.
    unsafe {
        core::arch::x86_64::_mm_prefetch(addr as *const i8, core::arch::x86_64::_MM_HINT_T0);
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = addr;
    }
}

/// Prefetch the object a NaN-boxed heap value points at.
///
/// Mirrors `CopyingPointerSet::decode_bits`'s tagged arm only — the raw-pointer
/// candidate arm needs a classification to be meaningful and a wrong guess
/// there would prefetch noise. Returns without touching memory for anything
/// that is not a tagged heap pointer.
#[inline(always)]
pub(super) fn prefetch_boxed_child(bits: u64) {
    use crate::value::{BIGINT_TAG, POINTER_MASK, POINTER_TAG, STRING_TAG, TAG_MASK};
    let tag = bits & TAG_MASK;
    if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
        let addr = (bits & POINTER_MASK) as usize;
        // The classifier's first read is the GC header, which sits below the
        // user pointer — prefetch that line, not the payload's.
        prefetch_read(addr.saturating_sub(super::GC_HEADER_SIZE));
    }
}
