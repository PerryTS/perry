//! Weak-to-strong READ barrier (#7900).
//!
//! # The hole this closes
//!
//! A budgeted cycle runs its one-time `FinalRootRemark` and then keeps opening
//! mutator windows while `AtomicFinalize` is still sliced — the full path's
//! `RememberedSetRebuild`, and (since #7892) the weak-holder loop itself. The
//! collector's soundness argument for those windows is "the incremental mark
//! barrier shades every store, and mid-cycle allocations are born black". Both
//! mechanisms only observe values the mutator **writes** or **creates**.
//!
//! `WeakRef.deref()` and `WeakMap.get()` do neither. They take a white object —
//! white *by construction*, because weak edges are deliberately excluded from
//! the strong trace — and hand it to compiled code as a strong local. That is a
//! white-to-strong transition through a pure READ. The remark has already run,
//! so no later root scan can discover the new reference; the next weak slice
//! sees the target unmarked, tombstones it, and the sweep reclaims memory the
//! mutator is still holding.
//!
//! # The barrier
//!
//! Every weak read shades the value words it hands out, exactly as a store
//! barrier would have shaded them on the way into the heap. Consequences:
//!
//! * the pending weak decision sees `GC_FLAG_MARKED` and keeps the slot
//!   (`weak_target_should_clear` is a mark-set predicate), so the target is not
//!   tombstoned mid-turn — which is also what the spec's `AddToKeptObjects`
//!   requires of `WeakRef.deref`;
//! * the shade pushes a mark seed, and the pre-sweep drains (the minor arm of
//!   `RememberedSetRebuild`, the full arm of `DisableBarrier`, and `step_sweep`'s
//!   gap drain) trace the target's children, so marking it does not leave a
//!   marked-but-untraced object with white children;
//! * outside a cycle it is inert. That matters: a stray mark laid down with no
//!   cycle in flight reads as "already live" to the NEXT cycle's trace. The
//!   inertness is pinned by `weak_read_barrier_is_inert_outside_a_cycle`.
//!
//! Cost is one relaxed load of the process-wide barrier-active count on a path
//! that is already a linear scan (`WeakMap`) or a by-name field read
//! (`WeakRef`). It is not a hot path.
//!
//! # Scope
//!
//! Only the budgeted (incremental) collector has post-remark mutator windows.
//! The copied minor processes its weak registry inside one uninterruptible
//! step, and synchronous cycles pass an unbounded budget, so neither can
//! interleave a read — but the barrier is unconditional rather than
//! phase-gated, because "which subphase is parked" is not observable from a
//! runtime helper and a phase-gated barrier is one reordering away from being
//! wrong again.

/// Shade one value word handed from a weak slot to the mutator.
///
/// Returns `true` when this call actually marked a previously-white object,
/// which is what the tests assert to prove the barrier's subject was live.
#[inline]
pub(super) fn weak_read_barrier(value_bits: u64) -> bool {
    let shaded = crate::gc::gc_weak_read_shade(value_bits);
    #[cfg(test)]
    if shaded {
        super::test_support::note_weak_read_barrier_shade();
    }
    shaded
}

/// Convenience wrapper for the `f64`-typed FFI returns: shade, then pass the
/// value straight through.
#[inline]
pub(super) fn weak_read_barrier_f64(value_bits: u64) -> f64 {
    weak_read_barrier(value_bits);
    f64::from_bits(value_bits)
}
