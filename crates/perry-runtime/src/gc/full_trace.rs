//! Full-heap trace scope shared by weak/ephemeron-style runtime owners.
//!
//! Minors cannot infer whether an old owner is live because they deliberately
//! do not trace the whole old generation. Runtime registries which become weak
//! only for a full trace use this scope to distinguish those collections from
//! non-copying minors without coupling their lifetime rules to one another.

use std::cell::Cell;

crate::perry_thread_local! {
    static FULL_TRACE_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn begin_full_trace() {
    FULL_TRACE_ACTIVE.with(|active| {
        assert!(!active.replace(true), "full trace already active");
    });
    crate::proxy::gc_begin_full_trace();
}

pub(crate) fn finish_full_trace() {
    crate::proxy::gc_finish_full_trace();
    FULL_TRACE_ACTIVE.with(|active| {
        assert!(active.replace(false), "no full trace active");
    });
}

#[inline(always)]
pub(crate) fn full_trace_active() -> bool {
    FULL_TRACE_ACTIVE.with(Cell::get)
}
