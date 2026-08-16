//! Resumable ("sliced") weak processing for budgeted GC cycles.
//!
//! # Why this module exists (#7903)
//!
//! `js_gc_step_us` and the mutator-assist paths advertise a *time* budget, but
//! they can only check elapsed time **between** work units. Any work unit whose
//! cost is unbounded therefore makes the advertised budget a fiction: the step
//! overshoots by however long that one unit ran, and no amount of tightening the
//! microsecond budget can help.
//!
//! Weak processing used to charge **one work unit per registered holder**. A
//! `FinalizationRegistry` is one holder — but its record array is arbitrarily
//! long, and [`super::process_finreg_after_mark`] walked all of it inside that
//! single unit. One registry holding a million registrations was therefore one
//! atomic, heap-sized "work unit" behind a time-budgeted API.
//!
//! This module charges **one work unit per record** and keeps a cursor *into*
//! the record array, so a large registry is spread across as many steps as it
//! needs and every step honours its budget.
//!
//! # The correctness constraint this has to preserve
//!
//! The previous code's atomicity was not accidental. Its comment read:
//!
//! > A FinalizationRegistry is one holder/work unit; its record array stays
//! > atomic so unregistering cannot interleave with and reorder an in-progress
//! > registry scan.
//!
//! That hazard is real. Between two steps the mutator runs, and
//! `FinalizationRegistry.prototype.unregister` **rebuilds** the entries array
//! without the matching records — every index after a removed element shifts
//! down. A naive resumed cursor would skip exactly as many records as were
//! removed before it, and a skipped record is a weak slot that never gets
//! tombstoned: on a non-moving budgeted cycle its target is swept and the slot
//! is left dangling.
//!
//! So the cursor is validated, not trusted. Alongside the record index we keep
//! the **identity** of the array it indexes: the value word of the registry's
//! `entries` field plus that array's length. Both mutation paths change one of
//! them — `unregister` installs a freshly built array (new value word),
//! `register` pushes (new length, and usually a new word too). On resume the
//! identity is re-read and compared; a mismatch means the indices we hold are
//! meaningless, and the registry's scan restarts from 0 against the new array.
//!
//! Restarting is safe because a rescan is idempotent. The first pass writes
//! `undefined` into a collected record's target slot and `false` into its
//! pending flag after enqueueing, so a second pass over the same record sees a
//! target that is no longer a collectable pointer and a pending flag that is no
//! longer set — it enqueues nothing and clears nothing twice.
//!
//! # The hard bound
//!
//! Restart-on-mutation alone is livelock-shaped: a mutator that touches the
//! registry in every window would restart the scan forever. So restarts are
//! capped at [`MAX_REGISTRY_RESTARTS`]; past that the registry is finished in
//! one atomic pass and *charged as such* in the telemetry
//! (`registry_atomic_finishes`). The bound this module offers is therefore
//! explicit rather than implied: per-step weak work is at most
//! `budget + (the one atomic finish that a pathological mutator can force,
//! at most once per registry per cycle)`.

use super::{
    dispatch_weak_holder, resolve_weak_holder_full, FullCycleLiveness, HolderDisposition,
    ObjectHeader, CLASS_ID_FINALIZATION_REGISTRY, WEAK_HOLDERS,
};

/// How many times one registry's scan may restart because the mutator changed
/// its entries array under us before we stop slicing it and finish atomically.
///
/// Four is not tuned — it is small enough that the atomic fallback is reachable
/// in a test and large enough that ordinary `unregister` traffic never reaches
/// it. What matters is that the number is finite, so the phase has a stated
/// worst case instead of an unbounded retry loop.
const MAX_REGISTRY_RESTARTS: u32 = 4;

/// A cursor into one FinalizationRegistry's record array.
///
/// The identity it carries ([`super::FinregEntriesIdentity`]) is two words, both
/// re-readable from the registry object without dereferencing anything held
/// across a mutator window. That matters: the cursor survives a return to the
/// mutator, so it must not cache a raw `*mut ArrayHeader` — it caches the
/// *value word* and re-validates it through `WeakLiveness::as_live_array` on
/// every resume, exactly as the unsliced code did on every call.
struct RegistryCursor {
    /// The holder's current address (post-`resolve_weak_holder_full`).
    holder: usize,
    identity: super::FinregEntriesIdentity,
    /// Next record index to scan.
    next: usize,
    restarts: u32,
}

/// Resumable full/fallback weak processing. The holder registry is snapshotted
/// once, then each call consumes at most `budget` work units — where a unit is
/// one holder resolved **or one FinalizationRegistry record scanned**. This
/// makes the work O(registered weak holders + registered records) with a
/// per-step ceiling, rather than O(all arena objects) with a per-holder ceiling
/// that one large registry could blow through.
///
/// Snapshotting is intentional: budgeted cycles are non-moving, while
/// synchronous moving cycles pass an unlimited budget and cannot expose a
/// mutator window. Holders allocated after the snapshot are allocate-black and
/// therefore cannot lose a target in the current cycle; the next collection
/// processes them.
pub(crate) struct FullWeakProcessingState {
    holders: Vec<usize>,
    cursor: usize,
    /// Set when a step ran out of budget partway through a registry's records.
    registry: Option<RegistryCursor>,
}

impl FullWeakProcessingState {
    pub(crate) fn new() -> Self {
        let holders = WEAK_HOLDERS.with(|holders| holders.borrow().iter().copied().collect());
        #[cfg(test)]
        super::test_support::reset_full_weak_processing_work_units();
        Self {
            holders,
            cursor: 0,
            registry: None,
        }
    }

    fn holders_drained(&self) -> bool {
        self.cursor == self.holders.len() && self.registry.is_none()
    }

    /// Process up to `budget` work units. Returns true when this cycle's weak
    /// processing is complete.
    pub(crate) fn step(
        &mut self,
        valid_ptrs: &crate::gc::ValidPointerSet,
        minor_only: bool,
        enqueue_callbacks: bool,
        budget: usize,
    ) -> bool {
        if budget == 0 {
            return self.holders_drained();
        }
        let liveness = FullCycleLiveness {
            valid_ptrs,
            minor_only,
        };
        let mut remaining = budget;
        let mut records_this_step = 0usize;

        // An in-flight registry always gets the budget first: leaving it parked
        // while new holders are resolved would let the number of open cursors
        // grow, and only one can be represented.
        if let Some(mut cursor) = self.registry.take() {
            let finished = advance_registry(
                &mut cursor,
                &liveness,
                enqueue_callbacks,
                &mut remaining,
                &mut records_this_step,
            );
            if !finished {
                self.registry = Some(cursor);
                note_step_records(records_this_step, true);
                return false;
            }
        }

        while remaining > 0 && self.cursor < self.holders.len() {
            let addr = self.holders[self.cursor];
            self.cursor += 1;
            remaining -= 1;
            #[cfg(test)]
            super::test_support::note_full_weak_processing_work_unit();
            match unsafe { resolve_weak_holder_full(valid_ptrs, addr, minor_only) } {
                HolderDisposition::Drop => {
                    WEAK_HOLDERS.with(|holders| {
                        holders.borrow_mut().remove(&addr);
                    });
                }
                HolderDisposition::Keep => {}
                HolderDisposition::Process(current) => {
                    let obj = current as *mut ObjectHeader;
                    if unsafe { (*obj).class_id } == CLASS_ID_FINALIZATION_REGISTRY {
                        let Some(identity) =
                            (unsafe { super::finreg_entries_identity(obj, &liveness) })
                        else {
                            continue;
                        };
                        let mut cursor = RegistryCursor {
                            holder: current,
                            identity,
                            next: 0,
                            restarts: 0,
                        };
                        let finished = advance_registry(
                            &mut cursor,
                            &liveness,
                            enqueue_callbacks,
                            &mut remaining,
                            &mut records_this_step,
                        );
                        if !finished {
                            self.registry = Some(cursor);
                            note_step_records(records_this_step, true);
                            return false;
                        }
                    } else {
                        unsafe { dispatch_weak_holder(obj, &liveness, enqueue_callbacks) };
                    }
                }
            }
        }
        note_step_records(records_this_step, false);
        self.holders_drained()
    }
}

/// Scan records from `cursor` until the registry is exhausted or `remaining`
/// hits zero. Returns true when the registry is fully scanned.
fn advance_registry(
    cursor: &mut RegistryCursor,
    liveness: &FullCycleLiveness<'_>,
    enqueue_callbacks: bool,
    remaining: &mut usize,
    records_this_step: &mut usize,
) -> bool {
    let registry = cursor.holder as *mut ObjectHeader;
    loop {
        // Re-derive the entries array on every resume. A mismatch means the
        // mutator restructured the array between steps and our index is stale.
        let Some(identity) = (unsafe { super::finreg_entries_identity(registry, liveness) }) else {
            // The array died or stopped being an array: nothing left to scan.
            return true;
        };
        if identity != cursor.identity {
            crate::gc::instruments::note_weak_registry_restart();
            if cursor.restarts >= MAX_REGISTRY_RESTARTS {
                // Hard bound: stop slicing this registry and finish it in one
                // atomic pass, accounted as such.
                crate::gc::instruments::note_weak_registry_atomic_finish();
                let scanned = unsafe {
                    super::process_finreg_record_range(
                        registry,
                        liveness,
                        enqueue_callbacks,
                        0,
                        identity.len,
                    )
                };
                *records_this_step = records_this_step.saturating_add(scanned);
                *remaining = remaining.saturating_sub(scanned);
                return true;
            }
            cursor.restarts += 1;
            cursor.identity = identity;
            cursor.next = 0;
        }
        if cursor.next >= cursor.identity.len {
            return true;
        }
        if *remaining == 0 {
            return false;
        }
        let take = (cursor.identity.len - cursor.next).min(*remaining);
        let scanned = unsafe {
            super::process_finreg_record_range(
                registry,
                liveness,
                enqueue_callbacks,
                cursor.next,
                take,
            )
        };
        cursor.next += take;
        *records_this_step = records_this_step.saturating_add(scanned);
        *remaining -= take;
        if cursor.next >= cursor.identity.len {
            return true;
        }
        if *remaining == 0 {
            return false;
        }
    }
}

#[inline]
fn note_step_records(records: usize, sliced: bool) {
    crate::gc::instruments::note_weak_step_records(records as u64, sliced);
}
