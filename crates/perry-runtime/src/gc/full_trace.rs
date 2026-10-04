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
    // The prototype identity words emit their carrier edge once per trace.
    crate::object::shapes::note_full_trace_begin();
    crate::proxy::gc_begin_full_trace();
    if let Some(hook) = FETCH_TRACE.with(Cell::get) {
        FETCH_TRACE_ARMED.with(|armed| armed.set(true));
        (hook.phase)(0);
    }
    // Armed only when the provider has something to decide this trace, so a
    // program that never parked a reclaimable id pays nothing per word.
    if let Some(hook) = POOL_TRACE.with(Cell::get) {
        if (hook.phase)(0) {
            POOL_TRACE_ARMED.with(|armed| armed.set(true));
        }
    }
}

pub(crate) fn finish_full_trace() {
    if FETCH_TRACE_ARMED.with(|armed| armed.replace(false)) {
        if let Some(hook) = FETCH_TRACE.with(Cell::get) {
            (hook.phase)(1);
        }
    }
    if POOL_TRACE_ARMED.with(|armed| armed.replace(false)) {
        if let Some(hook) = POOL_TRACE.with(Cell::get) {
            (hook.phase)(1);
        }
    }
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
    /// True between `begin_full_trace` and its finish/abort when a Fetch
    /// provider was registered at the start: the one flag the per-value
    /// tracing paths read.
    static FETCH_TRACE_ARMED: Cell<bool> = const { Cell::new(false) };
}

/// Install this thread's Fetch handle trace provider. The stdlib calls it
/// once per mutator thread, before that thread's first handle exists.
#[no_mangle]
pub extern "C" fn perry_ffi_gc_register_fetch_trace(phase: extern "C" fn(u32), observe: Observe) {
    FETCH_TRACE.with(|hook| hook.set(Some(FetchTrace { phase, observe })));
}

/// The shared native-handle pool's weak-owner provider (#11453): the band
/// `[1, COMMON_HANDLE_BAND_END)` that perry-ffi and the stdlib common registry
/// allocate from. `phase(0)` returns whether this trace must observe words;
/// `phase(1)` finishes, `phase(2)` aborts. The provider decides, per id it
/// parked, whether any traced word named it.
#[derive(Clone, Copy)]
struct PoolTrace {
    phase: extern "C" fn(u32) -> bool,
    observe: Observe,
}
crate::perry_thread_local! {
    static POOL_TRACE: Cell<Option<PoolTrace>> = const { Cell::new(None) };
    static POOL_TRACE_ARMED: Cell<bool> = const { Cell::new(false) };
}

/// Install this thread's shared-pool handle trace provider (#11453). Called
/// once per mutator thread before that thread parks its first reclaimable id.
#[no_mangle]
pub extern "C" fn perry_ffi_gc_register_pool_handle_trace(
    phase: extern "C" fn(u32) -> bool,
    observe: Observe,
) {
    POOL_TRACE.with(|hook| hook.set(Some(PoolTrace { phase, observe })));
}

/// Whether a traced word must be offered to [`observe_handle`]: a proxy, a
/// Fetch, or a shared-pool handle trace is running on this thread.
#[inline]
pub(crate) fn handle_trace_active() -> bool {
    crate::proxy::gc_full_trace_active()
        || FETCH_TRACE_ARMED.with(Cell::get)
        || POOL_TRACE_ARMED.with(Cell::get)
}

/// Fetch handles are POINTER_TAG-boxed or raw ids inside the fetch band. Pure
/// arithmetic, so ordinary heap pointers never pay for the thread-local read
/// and cross-crate indirect call below.
#[inline(always)]
fn is_fetch_handle_word(bits: u64) -> bool {
    let id = match bits >> 48 {
        0x7FFD => bits & crate::value::POINTER_MASK,
        0 => bits,
        _ => return false,
    };
    crate::value::addr_class::is_fetch_handle_band(id as usize)
}

/// Shared-pool handles: POINTER_TAG-boxed or raw ids in `[1, 0x40000)`.
#[inline(always)]
fn is_pool_handle_word(bits: u64) -> bool {
    let id = match bits >> 48 {
        0x7FFD => bits & crate::value::POINTER_MASK,
        0 => bits,
        _ => return false,
    };
    crate::value::addr_class::is_common_handle_band(id as usize)
}

pub(crate) fn observe_handle(bits: u64, valid_ptrs: &super::ValidPointerSet) -> bool {
    if crate::proxy::gc_observe_traced_value(bits, valid_ptrs) {
        return true;
    }
    if is_fetch_handle_word(bits) {
        return observe_fetch_handle(bits, valid_ptrs);
    }
    is_pool_handle_word(bits) && observe_pool_handle(bits, valid_ptrs)
}

#[inline(never)]
fn observe_pool_handle(bits: u64, valid_ptrs: &super::ValidPointerSet) -> bool {
    if !POOL_TRACE_ARMED.with(Cell::get) {
        return false;
    }
    extern "C" fn mark(bits: u64, ctx: *mut c_void) {
        let valid_ptrs = unsafe { &*(ctx as *const super::ValidPointerSet) };
        super::try_mark_value_or_raw(bits, valid_ptrs);
    }
    POOL_TRACE.with(|hook| match hook.get() {
        Some(hook) => (hook.observe)(bits, mark, valid_ptrs as *const _ as *mut c_void),
        None => false,
    })
}

#[inline(never)]
fn observe_fetch_handle(bits: u64, valid_ptrs: &super::ValidPointerSet) -> bool {
    if !FETCH_TRACE_ARMED.with(Cell::get) {
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
    let armed = FETCH_TRACE_ARMED
        .try_with(|armed| armed.replace(false))
        .unwrap_or(false);
    if armed {
        let _ = FETCH_TRACE.try_with(|hook| {
            if let Some(hook) = hook.get() {
                (hook.phase)(2);
            }
        });
    }
    let pool_armed = POOL_TRACE_ARMED
        .try_with(|armed| armed.replace(false))
        .unwrap_or(false);
    if pool_armed {
        let _ = POOL_TRACE.try_with(|hook| {
            if let Some(hook) = hook.get() {
                (hook.phase)(2);
            }
        });
    }
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
