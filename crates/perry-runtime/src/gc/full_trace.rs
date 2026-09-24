//! Full-heap trace scope shared by weak/ephemeron-style runtime owners.
//!
//! Minors cannot infer whether an old owner is live because they deliberately
//! do not trace the whole old generation. Runtime registries which become weak
//! only for a full trace use this scope to distinguish those collections from
//! non-copying minors without coupling their lifetime rules to one another.

use std::cell::Cell;
use std::ffi::c_void;

crate::perry_thread_local! {
    static FULL_TRACE_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn begin_full_trace() {
    FULL_TRACE_ACTIVE.with(|active| {
        assert!(!active.replace(true), "full trace already active");
    });
    crate::proxy::gc_begin_full_trace();
    FETCH_TRACE.with(|hook| {
        if let Some(hook) = hook.get() {
            (hook.phase)(0);
        }
    });
}

pub(crate) fn finish_full_trace() {
    FETCH_TRACE.with(|hook| {
        if let Some(hook) = hook.get() {
            (hook.phase)(1);
        }
    });
    crate::proxy::gc_finish_full_trace();
    FULL_TRACE_ACTIVE.with(|active| {
        assert!(active.replace(false), "no full trace active");
    });
}

#[inline(always)]
pub(crate) fn full_trace_active() -> bool {
    FULL_TRACE_ACTIVE.with(Cell::get)
}

// Provider callbacks use only C ABI data so separately linked stdlib images
// participate in the host collector, just like mutable-root scanners.
type Mark = extern "C" fn(u64, *mut c_void);
type Observe = extern "C" fn(u64, Mark, *mut c_void) -> bool;
#[derive(Clone, Copy)]
struct FetchTrace {
    phase: extern "C" fn(u32),
    observe: Observe,
}
crate::perry_thread_local! {
    static FETCH_TRACE: Cell<Option<FetchTrace>> = const { Cell::new(None) };
}

#[no_mangle]
pub extern "C" fn perry_ffi_gc_register_fetch_trace(phase: extern "C" fn(u32), observe: Observe) {
    FETCH_TRACE.with(|hook| hook.set(Some(FetchTrace { phase, observe })));
}

#[inline]
pub(crate) fn handle_trace_active() -> bool {
    crate::proxy::gc_full_trace_active()
        || (full_trace_active() && FETCH_TRACE.with(|hook| hook.get().is_some()))
}

pub(crate) fn observe_handle(bits: u64, valid_ptrs: &super::ValidPointerSet) -> bool {
    if crate::proxy::gc_observe_traced_value(bits, valid_ptrs) {
        return true;
    }
    if !full_trace_active() {
        return false;
    }
    extern "C" fn mark(bits: u64, ctx: *mut c_void) {
        let valid_ptrs = unsafe { &*(ctx as *const super::ValidPointerSet) };
        super::try_mark_value_or_raw(bits, valid_ptrs);
    }
    FETCH_TRACE.with(|hook| match hook.get() {
        Some(hook) => (hook.observe)(bits, mark, valid_ptrs as *const _ as *mut c_void),
        None => false,
    })
}

pub(crate) fn abort_full_trace() {
    let _ = FETCH_TRACE.try_with(|hook| {
        if let Some(hook) = hook.get() {
            (hook.phase)(2);
        }
    });
    let _ = FULL_TRACE_ACTIVE.try_with(|active| active.set(false));
    crate::proxy::gc_abort_full_trace();
}

/// Query whether a mutable-root scan is the weak-owner marking phase.
///
/// # Safety
/// `ctx` must be the live visitor context passed to a mutable-root scanner.
#[no_mangle]
pub unsafe extern "C" fn perry_ffi_gc_root_visitor_is_full_mark(ctx: *mut c_void) -> bool {
    full_trace_active() && (&*(ctx as *const super::RuntimeRootVisitor<'_>)).is_mark_phase()
}

/// Native registry pressure requests a full trace at the next safe poll;
/// allocating a numeric handle itself must never collect under table locks.
#[no_mangle]
pub extern "C" fn perry_ffi_gc_request_handle_collection() {
    super::policy::GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
    super::policy::set_safepoint_pending(true);
}
