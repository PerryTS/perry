//! Suppression windows (`gc_suppress`, `GcSuppressScope`) and the trigger
//! check a window owes when it closes.

use super::*;

pub fn gc_suppress() {
    if !gen_gc_enabled()
        && crate::arena::arena_in_use_bytes() >= gc_tiny_parse_full_gc_in_use_trigger_dyn_bytes()
    {
        crate::arena::arena_start_fresh_general_block();
    }
    // Issue #745: snapshot arena_total at suppress-start so the
    // matching `gc_bump_malloc_trigger` can size the suppressed
    // window's parse growth and gate the bytes-trigger bump on it.
    GC_PRE_SUPPRESS_BYTES.with(|c| c.set(crate::arena::arena_total_bytes()));
    GC_FLAGS.with(|f| f.set(f.get() | GC_FLAG_SUPPRESSED));
}

/// Resume GC triggers after suppression.
///
/// A trigger check turned away inside the window is dropped here, not run:
/// every `gc_suppress` caller accounts for the window's growth itself right
/// after this call (see [`GC_FLAG_TRIGGER_OWED`]).
pub fn gc_unsuppress() {
    // The SNAP census holds collection off until its boundary (an address
    // names one object for the whole run); a parse window's unsuppress must
    // not end that hold.
    if crate::snap_census::holds_collection() {
        return;
    }
    GC_FLAGS.with(|f| f.set(f.get() & !(GC_FLAG_SUPPRESSED | GC_FLAG_TRIGGER_OWED)));
}

/// True while GC triggers are suppressed (see [`gc_suppress`]).
pub(crate) fn gc_is_suppressed() -> bool {
    GC_FLAGS.with(|f| f.get()) & GC_FLAG_SUPPRESSED != 0
}

/// #6759 Phase C2: RAII no-move window for a TINY allocation. While alive,
/// every GC trigger is blocked (`GC_FLAG_SUPPRESSED` gates `gc_check_trigger`,
/// the budgeted stepper, and the safepoint moving minor), so no heap object
/// can move and raw pointers held anywhere up the caller stack stay valid
/// across the allocation.
///
/// Unlike [`gc_suppress`] this sets ONLY the flag: it skips the fresh-block /
/// pre-suppress-bytes bookkeeping that exists for JSON.parse's multi-megabyte
/// suppressed windows, because the intended use is a few dozen bytes (e.g. an
/// `ObjectMeta` record) where that accounting is pure overhead. Nesting-safe:
/// the previous suppression state is restored on drop, so a scope opened
/// inside an outer `gc_suppress` window does not unsuppress it early.
///
/// A trigger check that an allocation inside the scope reached is OWED, not
/// dropped: [`gc_check_trigger_evaluate`] records it in
/// [`GC_FLAG_TRIGGER_OWED`] and the outermost scope runs it as it drops,
/// which is an allocation point like any other (the allocation-point arms
/// defer to a precise safepoint or collect non-moving under the conservative
/// scan). Without that, a trigger whose arena block boundary falls inside a
/// scope is lost until the next boundary: a loop of small Maps whose every
/// block boundary landed in `ensure_capacity`'s scope ran with no minor at
/// all and grew the nursery without bound.
pub(crate) struct GcSuppressScope {
    was_suppressed: bool,
}

impl GcSuppressScope {
    pub(crate) fn new() -> Self {
        let was_suppressed = gc_is_suppressed();
        if !was_suppressed {
            GC_FLAGS.with(|f| f.set(f.get() | GC_FLAG_SUPPRESSED));
            #[cfg(test)]
            window_exit_probe::COLLECTIONS_AT_WINDOW_ENTRY
                .with(|c| c.set(super::progress::gc_total_collection_count()));
        }
        GcSuppressScope { was_suppressed }
    }
}

/// Tests only: the collection count when the last outermost
/// [`GcSuppressScope`] closed, read BEFORE the check it owes runs. A window's
/// test reads it to tell "collected inside the window" from "the owed check
/// collected as the window closed".
#[cfg(test)]
pub(crate) mod window_exit_probe {
    std::thread_local! {
        pub(crate) static COLLECTIONS_AT_WINDOW_EXIT: std::cell::Cell<Option<u64>> =
            const { std::cell::Cell::new(None) };
        /// The collection count when the current outermost window opened.
        pub(crate) static COLLECTIONS_AT_WINDOW_ENTRY: std::cell::Cell<u64> =
            const { std::cell::Cell::new(0) };
        /// Set when an outermost window closes with a count other than the
        /// one it opened with: a collection ran INSIDE that window. A test
        /// clears it, runs builds made of several windows, and reads it.
        pub(crate) static WINDOW_SAW_COLLECTION: std::cell::Cell<bool> =
            const { std::cell::Cell::new(false) };
    }
}

impl Drop for GcSuppressScope {
    fn drop(&mut self) {
        if !self.was_suppressed {
            #[cfg(test)]
            window_exit_probe::COLLECTIONS_AT_WINDOW_EXIT
                .with(|c| c.set(Some(super::progress::gc_total_collection_count())));
            #[cfg(test)]
            {
                let now = super::progress::gc_total_collection_count();
                if window_exit_probe::COLLECTIONS_AT_WINDOW_ENTRY.with(std::cell::Cell::get) != now
                {
                    window_exit_probe::WINDOW_SAW_COLLECTION.with(|c| c.set(true));
                }
            }
            let flags = GC_FLAGS.with(|f| {
                let flags = f.get();
                f.set(flags & !(GC_FLAG_SUPPRESSED | GC_FLAG_TRIGGER_OWED));
                flags
            });
            if flags & GC_FLAG_TRIGGER_OWED != 0 {
                run_owed_trigger_check();
            }
        }
    }
}

/// The trigger check a [`GcSuppressScope`] turned away, run as the outermost
/// scope drops. Not while unwinding: a panic leaves the heap in whatever state
/// the scope was protecting, exactly as [`flush_deferred_gc_request`] skips it.
#[cold]
#[inline(never)]
fn run_owed_trigger_check() {
    if std::thread::panicking() {
        return;
    }
    gc_check_trigger();
}
