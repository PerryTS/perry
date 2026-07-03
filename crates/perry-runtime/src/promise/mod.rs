//! Promise implementation for async/await support
//!
//! This is a simplified Promise implementation for the Perry runtime.
//! It supports basic resolve/reject and then/catch chaining.
//!
//! This module is split into topical sub-modules; see siblings for the
//! per-area implementation. `mod.rs` owns the shared infrastructure
//! (instrumentation counters, thread-local task queue, async-context
//! plumbing, Promise/Task/InlineTrap types) and re-exports the public
//! API surface.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};

use crate::async_context::{
    capture_context, enter_context, restore_context, scan_snapshot_roots_mut, AsyncContextSnapshot,
};

pub mod assimilate;
pub mod async_step;
pub mod checked_dispatch;
pub mod combinators;
pub mod microtasks;
pub mod native_async;
pub mod scanners;
pub mod spec_combinators;
pub mod then;

// ─── Explicit named re-exports ────────────────────────────────────
// The full pre-split public surface. Anything new that needs to be
// FFI-visible MUST be added here so the linker still sees it.

pub use async_step::{
    js_array_from_async, js_async_first_call, js_async_step_chain, js_async_step_done,
    js_get_current_step_closure, js_promise_resolved, js_promise_resolved_then,
    scan_async_step_thunk_cache, scan_async_step_thunk_cache_mut,
};
pub use checked_dispatch::{
    js_promise_catch_checked, js_promise_closure_arg, js_promise_finally_checked,
    js_promise_then_checked,
};
pub use combinators::{
    js_assimilate_thenable, js_await_any_promise, js_is_promise, js_promise_all,
    js_promise_all_settled, js_promise_any, js_promise_new_with_executor, js_promise_race,
    js_promise_rejected, js_promise_schedule_resolve, js_promise_try, js_value_is_promise,
};
pub use microtasks::js_promise_run_microtasks;
pub use native_async::{
    js_native_async_completion_attach_handle, js_native_async_completion_cancel,
    js_native_async_completion_new, js_native_async_completion_promise,
    js_native_async_completion_reject_bits, js_native_async_completion_reject_promise_bits,
    js_native_async_completion_reject_string, js_native_async_completion_resolve_bits,
    js_native_async_completion_resolve_promise_bits, js_native_async_drop_promise_token,
    js_native_async_has_active, js_native_async_process_pending,
    scan_native_async_completion_roots_mut, NativeAsyncCompletion,
    PERRY_NATIVE_ASYNC_ALREADY_COMPLETED, PERRY_NATIVE_ASYNC_CLEANUP_ON_CANCEL,
    PERRY_NATIVE_ASYNC_CLEANUP_ON_REJECT, PERRY_NATIVE_ASYNC_CLEANUP_ON_SUCCESS,
    PERRY_NATIVE_ASYNC_INVALID, PERRY_NATIVE_ASYNC_OK, PERRY_NATIVE_ASYNC_THREAD_MAIN,
    PERRY_NATIVE_ASYNC_WRONG_THREAD,
};
pub use scanners::{js_promise_with_resolvers, scan_promise_roots, scan_promise_roots_mut};
pub(crate) use scanners::{new_promise_root_scan_state, scan_promise_roots_mut_step};
pub use spec_combinators::{
    js_promise_all_settled_spec, js_promise_all_spec, js_promise_any_spec, js_promise_race_spec,
    js_promise_reject_spec, js_promise_resolve_spec, js_promise_try_spec,
    js_promise_with_resolvers_spec,
};
pub(crate) use then::{
    box_promise_ptr, js_promise_attach_handlers, js_promise_attach_settle_listener,
    mark_rejection_handled, promise_has_own_constructor, promise_has_own_property,
    promise_proto_method, promise_prototype_catch_thunk, promise_prototype_finally_thunk,
    promise_prototype_then_thunk,
};
pub use then::{
    js_promise_bound_method, js_promise_catch, js_promise_finally, js_promise_free,
    js_promise_mark_internally_handled, js_promise_new, js_promise_reason, js_promise_reject,
    js_promise_resolve, js_promise_resolve_with_promise, js_promise_result, js_promise_state,
    js_promise_then, js_promise_value,
};

#[cfg(test)]
pub(crate) use scanners::{
    test_async_step_thunk_cache, test_clear_promise_scanner_roots, test_current_microtask_value,
    test_promise_context_keys, test_promise_scanner_snapshot, test_seed_async_step_thunk_cache,
    test_seed_many_promise_task_roots, test_seed_promise_context, test_seed_promise_scanner_roots,
    test_store_with_resolvers_result_fields, TestPromiseScannerSnapshot,
};

// Cached `PERRY_MT_PROFILE` flag, populated once at process start.
// All instrumentation counter increments check this first — when
// profiling is OFF (the common case) each bump compiles down to a
// relaxed atomic load + conditional branch (~1 ns) instead of a
// relaxed atomic add (~4-5 ns on Apple Silicon). For a kernel that
// runs 200k microtasks with ~3 counter bumps each, that's ~600k
// avoided fetch_add ops ≈ 2-3 ms saved per run.
pub(crate) static MT_PROFILE_ENABLED: AtomicBool = AtomicBool::new(false);

#[inline(always)]
pub(crate) fn mt_profile_enabled() -> bool {
    MT_PROFILE_ENABLED.load(Ordering::Relaxed)
}

/// Bump an instrumentation counter only when profiling is enabled.
/// This compiles to a relaxed load + conditional jump instead of an
/// atomic RMW on the hot path. See `MT_PROFILE_ENABLED` for the
/// rationale.
#[inline(always)]
pub(crate) fn bump(counter: &AtomicU64) {
    if mt_profile_enabled() {
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

/// Return true iff `value`'s NaN-box tag indicates it cannot possibly
/// be a Promise pointer or a thenable object — i.e. a number,
/// undefined, null, true, false, or a non-pointer-tagged f64. The
/// async-to-generator transform emits `Promise.resolve(x).then(...)`
/// per `await`; in the common case `x` is one of these primitives.
/// Skipping the `is_promise` + `assimilate_thenable` probes for them
/// removes ~600 ns of work from every await steady-state iteration.
#[inline(always)]
pub(crate) fn is_definitely_primitive(value: f64) -> bool {
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
    let bits = value.to_bits();
    let tag = bits & TAG_MASK;
    // Only POINTER_TAG-tagged values can be promises or thenable
    // objects. Strings (STRING_TAG), bigints (BIGINT_TAG), int32s
    // (INT32_TAG), bool/null/undefined (TAG_xxx), and raw f64
    // numbers (no special tag) are all primitives in spec terms.
    tag != POINTER_TAG
}

// Instrumentation counters (set PERRY_MT_PROFILE=1 to print at exit).
pub static MT_RUN_COUNT: AtomicU64 = AtomicU64::new(0);
pub static MT_THENABLE_PROBE_COUNT: AtomicU64 = AtomicU64::new(0);
pub static MT_PROMISE_NEW_COUNT: AtomicU64 = AtomicU64::new(0);
pub static MT_PROMISE_THEN_COUNT: AtomicU64 = AtomicU64::new(0);
pub static MT_PROMISE_RESOLVED_COUNT: AtomicU64 = AtomicU64::new(0);
pub static MT_INNER_PROMISE_UNWRAP_COUNT: AtomicU64 = AtomicU64::new(0);
pub static MT_TIME_NS_QUEUE: AtomicU64 = AtomicU64::new(0);
pub static MT_TIME_NS_CALLBACK: AtomicU64 = AtomicU64::new(0);
pub static MT_TIME_NS_RESOLVE: AtomicU64 = AtomicU64::new(0);
pub static MT_FAST_PATH_HIT: AtomicU64 = AtomicU64::new(0);
pub static MT_FAST_PATH_MISS: AtomicU64 = AtomicU64::new(0);
/// Counter: per-await `js_async_step_chain` invocations that reused
/// the in-flight `next` Promise (via INLINE_TRAP_NEXT) and so skipped
/// the Promise allocation that the equivalent `js_promise_then`
/// (or fresh `js_promise_new`) call would have done.
pub static MT_STEP_CHAIN_REUSE_HIT: AtomicU64 = AtomicU64::new(0);
pub static MT_STEP_CHAIN_REUSE_MISS: AtomicU64 = AtomicU64::new(0);
/// Counter: per-async-fn `js_async_step_done` invocations that reused
/// the in-flight `next` Promise (via INLINE_TRAP_NEXT) and so skipped
/// the `js_promise_resolved` allocation the equivalent
/// `Promise.resolve(value)` would have done.
pub static MT_STEP_DONE_REUSE_HIT: AtomicU64 = AtomicU64::new(0);
pub static MT_STEP_DONE_REUSE_MISS: AtomicU64 = AtomicU64::new(0);

type ForeignPromiseAdapterFn = extern "C" fn(f64) -> f64;

static FOREIGN_PROMISE_ADAPTER_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Register an adapter for Promise-like values owned by another runtime.
/// perry-jsruntime uses this to turn a V8 `JS_HANDLE_TAG` Promise into a
/// native pending `Promise` before native await / Promise combinators inspect
/// the value.
#[no_mangle]
pub extern "C" fn js_register_foreign_promise_adapter(f: ForeignPromiseAdapterFn) {
    FOREIGN_PROMISE_ADAPTER_FN.store(f as *mut (), Ordering::Release);
}

pub(crate) fn adapt_foreign_promise_value(value: f64) -> f64 {
    let bits = value.to_bits();
    if (bits & crate::value::TAG_MASK) != crate::value::JS_HANDLE_TAG {
        return value;
    }

    let f = FOREIGN_PROMISE_ADAPTER_FN.load(Ordering::Acquire);
    if f.is_null() {
        return value;
    }

    unsafe {
        let func: ForeignPromiseAdapterFn = std::mem::transmute(f);
        func(value)
    }
}

extern "C" fn mt_profile_atexit() {
    if std::env::var_os("PERRY_MT_PROFILE").is_none() {
        return;
    }
    eprintln!(
        "[mt-profile] runs={} resolved={} then={} new={} unwrap={} thenable_probe={}",
        MT_RUN_COUNT.load(Ordering::Relaxed),
        MT_PROMISE_RESOLVED_COUNT.load(Ordering::Relaxed),
        MT_PROMISE_THEN_COUNT.load(Ordering::Relaxed),
        MT_PROMISE_NEW_COUNT.load(Ordering::Relaxed),
        MT_INNER_PROMISE_UNWRAP_COUNT.load(Ordering::Relaxed),
        MT_THENABLE_PROBE_COUNT.load(Ordering::Relaxed),
    );
    let q = MT_TIME_NS_QUEUE.load(Ordering::Relaxed);
    let cb = MT_TIME_NS_CALLBACK.load(Ordering::Relaxed);
    let rs = MT_TIME_NS_RESOLVE.load(Ordering::Relaxed);
    eprintln!(
        "[mt-profile] time(ms): queue={:.1} callback={:.1} resolve={:.1}",
        q as f64 / 1e6,
        cb as f64 / 1e6,
        rs as f64 / 1e6,
    );
    eprintln!(
        "[mt-profile] fast_path: hit={} miss={}",
        MT_FAST_PATH_HIT.load(Ordering::Relaxed),
        MT_FAST_PATH_MISS.load(Ordering::Relaxed),
    );
    eprintln!(
        "[mt-profile] closure: alloc={} cap_singleton_hit={} cap_singleton_miss={}",
        crate::closure::CLOSURE_ALLOC_COUNT.load(Ordering::Relaxed),
        crate::closure::CLOSURE_CAP_SINGLETON_HIT.load(Ordering::Relaxed),
        crate::closure::CLOSURE_CAP_SINGLETON_MISS.load(Ordering::Relaxed),
    );
    eprintln!(
        "[mt-profile] step_chain_reuse: hit={} miss={}",
        MT_STEP_CHAIN_REUSE_HIT.load(Ordering::Relaxed),
        MT_STEP_CHAIN_REUSE_MISS.load(Ordering::Relaxed),
    );
    eprintln!(
        "[mt-profile] step_done_reuse: hit={} miss={}",
        MT_STEP_DONE_REUSE_HIT.load(Ordering::Relaxed),
        MT_STEP_DONE_REUSE_MISS.load(Ordering::Relaxed),
    );
}

static MT_PROFILE_REG: std::sync::Once = std::sync::Once::new();
pub(crate) fn mt_profile_register() {
    MT_PROFILE_REG.call_once(|| {
        // Read PERRY_MT_PROFILE once and cache it. All `bump(&COUNTER)`
        // call sites read MT_PROFILE_ENABLED via a relaxed atomic load;
        // when unset (the common case) the counter increment is skipped
        // entirely. The atexit hook is only registered when profiling
        // is enabled — otherwise the printer would emit a zero report
        // at every process exit.
        let enabled = std::env::var_os("PERRY_MT_PROFILE").is_some();
        MT_PROFILE_ENABLED.store(enabled, Ordering::Relaxed);
        if enabled {
            unsafe {
                extern "C" {
                    fn atexit(cb: extern "C" fn()) -> i32;
                }
                atexit(mt_profile_atexit);
            }
        }
    });
}

// ─── Iter-result scratch slot (async-step driver fast path) ───────
//
// `await x` rewrites to a generator state machine whose `next()`
// closure used to allocate `{value, done}` per call (one alloc per
// await, plus two PropertyGet linear scans by the async-step driver
// that immediately consumes both fields). On a 200k-await benchmark
// that's 200k object allocs purely as a 2-field carrier between
// generator and step driver.
//
// We replace the alloc with a thread-local pair: the state machine
// writes (value, done) via `js_iter_result_set` and returns `undefined`;
// the step driver reads them back via `js_iter_result_get_value` /
// `js_iter_result_get_done`. The slot is overwritten on every
// `next()` call — safe because the async-step driver synchronously
// consumes both fields immediately after the call returns, with no
// intervening generator activity.
//
// The transform only emits these helpers for `was_plain_async`
// generators (async functions rewritten via async→generator). User-
// visible generators (`function*`) still allocate real `{value, done}`
// objects so `for...of` and external consumers see the spec shape.
thread_local! {
    static ITER_RESULT_VALUE: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
    static ITER_RESULT_VALUE_I32: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
    static ITER_RESULT_VALUE_I1: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ITER_RESULT_DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ITER_RESULT_VALUE_IS_RAW_F64: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ITER_RESULT_VALUE_IS_RAW_I32: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ITER_RESULT_VALUE_IS_RAW_I1: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub static MT_ITER_RESULT_SET_COUNT: AtomicU64 = AtomicU64::new(0);

/// Write the iter-result scratch slot. Returns `undefined` so callers
/// can `return js_iter_result_set(v, d)` from the generator's `next()`
/// state-machine without a separate trailing `return undefined`.
#[no_mangle]
pub extern "C" fn js_iter_result_set(value: f64, done: i32) -> f64 {
    bump(&MT_ITER_RESULT_SET_COUNT);
    ITER_RESULT_VALUE.with(|c| c.set(value));
    ITER_RESULT_DONE.with(|c| c.set(done != 0));
    ITER_RESULT_VALUE_IS_RAW_F64.with(|c| c.set(false));
    ITER_RESULT_VALUE_IS_RAW_I32.with(|c| c.set(false));
    ITER_RESULT_VALUE_IS_RAW_I1.with(|c| c.set(false));
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Write a raw numeric iter-result payload. The value half is not a JSValue
/// root and must not be scanned by GC while the side flag is set.
#[no_mangle]
pub extern "C" fn js_iter_result_set_f64(value: f64, done: i32) -> f64 {
    bump(&MT_ITER_RESULT_SET_COUNT);
    ITER_RESULT_VALUE.with(|c| c.set(value));
    ITER_RESULT_DONE.with(|c| c.set(done != 0));
    ITER_RESULT_VALUE_IS_RAW_F64.with(|c| c.set(true));
    ITER_RESULT_VALUE_IS_RAW_I32.with(|c| c.set(false));
    ITER_RESULT_VALUE_IS_RAW_I1.with(|c| c.set(false));
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Write a raw signed-Int32 iter-result payload. The value half is not a
/// JSValue root and must not be scanned by GC while the side flag is set.
#[no_mangle]
pub extern "C" fn js_iter_result_set_i32(value: i32, done: i32) -> f64 {
    bump(&MT_ITER_RESULT_SET_COUNT);
    ITER_RESULT_VALUE_I32.with(|c| c.set(value));
    ITER_RESULT_DONE.with(|c| c.set(done != 0));
    ITER_RESULT_VALUE_IS_RAW_F64.with(|c| c.set(false));
    ITER_RESULT_VALUE_IS_RAW_I32.with(|c| c.set(true));
    ITER_RESULT_VALUE_IS_RAW_I1.with(|c| c.set(false));
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Write a raw boolean iter-result payload. The value half is not a JSValue
/// root and must not be scanned by GC while the side flag is set.
#[no_mangle]
pub extern "C" fn js_iter_result_set_i1(value: i32, done: i32) -> f64 {
    bump(&MT_ITER_RESULT_SET_COUNT);
    ITER_RESULT_VALUE_I1.with(|c| c.set(value != 0));
    ITER_RESULT_DONE.with(|c| c.set(done != 0));
    ITER_RESULT_VALUE_IS_RAW_F64.with(|c| c.set(false));
    ITER_RESULT_VALUE_IS_RAW_I32.with(|c| c.set(false));
    ITER_RESULT_VALUE_IS_RAW_I1.with(|c| c.set(true));
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Read the value half of the iter-result scratch slot.
#[no_mangle]
pub extern "C" fn js_iter_result_get_value() -> f64 {
    if ITER_RESULT_VALUE_IS_RAW_I32.with(|c| c.get()) {
        let value = ITER_RESULT_VALUE_I32.with(|c| c.get());
        return f64::from_bits(crate::value::JSValue::int32(value).bits());
    }
    if ITER_RESULT_VALUE_IS_RAW_I1.with(|c| c.get()) {
        let value = ITER_RESULT_VALUE_I1.with(|c| c.get());
        return f64::from_bits(crate::value::JSValue::bool(value).bits());
    }
    ITER_RESULT_VALUE.with(|c| c.get())
}

/// Read the value half for numeric consumers. Raw-f64 writes return directly;
/// generic JSValue writes are coerced using ordinary JS number coercion.
#[no_mangle]
pub extern "C" fn js_iter_result_get_value_f64() -> f64 {
    if ITER_RESULT_VALUE_IS_RAW_F64.with(|c| c.get()) {
        ITER_RESULT_VALUE.with(|c| c.get())
    } else if ITER_RESULT_VALUE_IS_RAW_I32.with(|c| c.get()) {
        ITER_RESULT_VALUE_I32.with(|c| c.get()) as f64
    } else {
        crate::builtins::js_number_coerce(js_iter_result_get_value())
    }
}

/// Read the value half for signed-Int32 consumers. Raw-i32 writes return
/// directly; generic JSValue and other raw primitive writes use JS ToInt32.
#[no_mangle]
pub extern "C" fn js_iter_result_get_value_i32() -> i32 {
    if ITER_RESULT_VALUE_IS_RAW_I32.with(|c| c.get()) {
        return ITER_RESULT_VALUE_I32.with(|c| c.get());
    }
    if ITER_RESULT_VALUE_IS_RAW_I1.with(|c| c.get()) {
        return if ITER_RESULT_VALUE_I1.with(|c| c.get()) {
            1
        } else {
            0
        };
    }
    let number = if ITER_RESULT_VALUE_IS_RAW_F64.with(|c| c.get()) {
        ITER_RESULT_VALUE.with(|c| c.get())
    } else {
        crate::builtins::js_number_coerce(js_iter_result_get_value())
    };
    if !number.is_finite() {
        0
    } else {
        (number as i64) as i32
    }
}

/// Read the value half for boolean consumers. Raw-i1 writes return directly;
/// generic JSValue and other raw primitive writes use ordinary JS truthiness.
#[no_mangle]
pub extern "C" fn js_iter_result_get_value_i1() -> i32 {
    if ITER_RESULT_VALUE_IS_RAW_I1.with(|c| c.get()) {
        return if ITER_RESULT_VALUE_I1.with(|c| c.get()) {
            1
        } else {
            0
        };
    }
    crate::value::js_is_truthy(js_iter_result_get_value())
}

/// Read the done half as a NaN-boxed bool (TAG_TRUE / TAG_FALSE) so it
/// can flow into any control-flow / property context without a
/// separate conversion.
#[no_mangle]
pub extern "C" fn js_iter_result_get_done() -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let d = ITER_RESULT_DONE.with(|c| c.get());
    f64::from_bits(if d { TAG_TRUE } else { TAG_FALSE })
}

/// GC root scanner. The value half can hold pointer NaN-box values
/// (Promise pointers, object pointers from awaited values); register
/// this with the GC so they survive a collection that lands between
/// `js_iter_result_set` and the next `js_iter_result_get_value`.
pub fn scan_iter_result_root(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_iter_result_root_mut(&mut visitor);
}

pub fn scan_iter_result_root_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let is_raw_primitive = ITER_RESULT_VALUE_IS_RAW_F64.with(|c| c.get())
        || ITER_RESULT_VALUE_IS_RAW_I32.with(|c| c.get())
        || ITER_RESULT_VALUE_IS_RAW_I1.with(|c| c.get());
    if !is_raw_primitive {
        ITER_RESULT_VALUE.with(|c| {
            visitor.visit_cell_f64_slot(c);
        });
    }
}

#[used]
static KEEP_JS_ITER_RESULT_SET: extern "C" fn(f64, i32) -> f64 = js_iter_result_set;
#[used]
static KEEP_JS_ITER_RESULT_SET_F64: extern "C" fn(f64, i32) -> f64 = js_iter_result_set_f64;
#[used]
static KEEP_JS_ITER_RESULT_SET_I32: extern "C" fn(i32, i32) -> f64 = js_iter_result_set_i32;
#[used]
static KEEP_JS_ITER_RESULT_SET_I1: extern "C" fn(i32, i32) -> f64 = js_iter_result_set_i1;
#[used]
static KEEP_JS_ITER_RESULT_GET_VALUE: extern "C" fn() -> f64 = js_iter_result_get_value;
#[used]
static KEEP_JS_ITER_RESULT_GET_VALUE_F64: extern "C" fn() -> f64 = js_iter_result_get_value_f64;
#[used]
static KEEP_JS_ITER_RESULT_GET_VALUE_I32: extern "C" fn() -> i32 = js_iter_result_get_value_i32;
#[used]
static KEEP_JS_ITER_RESULT_GET_VALUE_I1: extern "C" fn() -> i32 = js_iter_result_get_value_i1;
#[used]
static KEEP_JS_ITER_RESULT_GET_DONE: extern "C" fn() -> f64 = js_iter_result_get_done;

/// Promise state
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PromiseState {
    Pending = 0,
    Fulfilled = 1,
    Rejected = 2,
}

/// Closure pointer type for promise handlers (closures, not raw function pointers)
pub type ClosurePtr = *const crate::closure::ClosureHeader;

/// A Promise represents an eventual completion (or failure) of an async operation
#[repr(C)]
pub struct Promise {
    /// Current state of the promise
    pub(crate) state: PromiseState,
    /// The resolved value (if fulfilled)
    pub(crate) value: f64,
    /// The rejection reason (if rejected)
    pub(crate) reason: f64,
    /// Closure to run when fulfilled (null if none)
    pub(crate) on_fulfilled: ClosurePtr,
    /// Closure to run when rejected (null if none)
    pub(crate) on_rejected: ClosurePtr,
    /// Next promise in the chain (for .then())
    pub(crate) next: *mut Promise,
    /// async_hooks asyncId for this Promise, 0 when hooks were inactive.
    pub(crate) async_id: u64,
    /// async_hooks triggerAsyncId captured at Promise creation.
    pub(crate) trigger_async_id: u64,
}

impl Promise {
    pub(crate) fn new() -> Self {
        Promise {
            state: PromiseState::Pending,
            value: 0.0,
            reason: 0.0,
            on_fulfilled: ptr::null(),
            on_rejected: ptr::null(),
            next: ptr::null_mut(),
            async_id: 0,
            trigger_async_id: 0,
        }
    }
}

/// One entry in the microtask queue. Two shapes:
///
/// `Promise(p, value, is_fulfilled)` — the legacy shape: we'll dispatch
/// to `(*p).on_fulfilled` or `on_rejected` depending on the bool, then
/// resolve `(*p).next` with the callback's return value.
///
/// `Inline(cb, value, next, is_fulfilled)` — the fast-path shape used
/// by `js_promise_resolved_then` when the awaited value is a primitive.
/// We've skipped allocating a source promise; the callback is carried
/// inline. Dispatch is identical from here: invoke `cb(value)` (or
/// `cb_rej(value)` — only one is non-null per entry), propagate the
/// result to `next`. Saves one Promise allocation per `await` of a
/// primitive value, which is the steady-state pattern for the async-to-
/// generator transform.
#[derive(Clone)]
pub(crate) enum Task {
    Promise(*mut Promise, f64, bool, AsyncContextSnapshot),
    PromiseAll(
        combinators::PromiseAllState,
        f64,
        bool,
        AsyncContextSnapshot,
    ),
    Inline(ClosurePtr, f64, *mut Promise, bool, AsyncContextSnapshot),
    /// `queueMicrotask(callback)` jobs share the same FIFO queue as Promise
    /// reactions. `process.nextTick` stays in the separate higher-priority
    /// queue owned by `builtins::globals`.
    Microtask {
        callback: ClosurePtr,
        context: AsyncContextSnapshot,
        async_id: u64,
        trigger_async_id: u64,
    },
    /// Direct dispatch to a 2-arg async-step closure. Equivalent to
    /// `Inline(then_v_arrow, value, next, true)` where `then_v_arrow`
    /// is a wrapper that calls `step(value, is_error)` — but skips the
    /// then_v_arrow alloc + dispatch by carrying `step_closure` and
    /// the `is_error` flag directly. Saves one closure allocation
    /// per await on the steady-state primitive-await path.
    AsyncStep(ClosurePtr, f64, *mut Promise, bool, AsyncContextSnapshot),
}

// Global task queue for pending promise callbacks. Must be FIFO per
// ECMAScript microtask semantics: `Promise.resolve(1).then(...)` and
// `Promise.resolve(2).then(...)` registered in source order must run
// their continuations in source order (1 first, then 2). Using a
// `Vec` with `.pop()` produces LIFO ordering, breaking every test
// that prints inside multiple parallel promise chains.
thread_local! {
pub(crate) static TASK_QUEUE: RefCell<std::collections::VecDeque<Task>>
        = const { RefCell::new(std::collections::VecDeque::new()) };

    // TODO: Move this snapshot into `Promise` once generational evacuation
    // becomes the default. Today promise objects are malloc-GC payloads whose
    // Rust fields are not dropped during sweep, so a side table lets us clean
    // pending snapshots from the sweep path. With `PERRY_GEN_GC_EVACUATE=1`,
    // however, promise addresses can change and this key will not be rewritten,
    // so a pre-evacuation `.then()` snapshot can be missed after settlement.
    pub(crate) static PROMISE_CONTEXTS: RefCell<PromiseContextStore> =
        RefCell::new(PromiseContextStore::default());

    pub(crate) static MICROTASK_PREV_CONTEXTS: RefCell<Vec<AsyncContextSnapshot>>
        = const { RefCell::new(Vec::new()) };

    /// Packed `(trap_next, current_step)` for the currently-dispatching
    /// inline-style microtask. Single TLS cell so the hot-path readers
    /// (`js_async_step_chain` / `js_async_step_done`) and writers
    /// (runner's `Task::Inline` / `Task::AsyncStep` arms) issue ONE
    /// `.with()` call instead of two. Each `.with()` on x86_64/aarch64
    /// macOS is ~10 ns through `__tls_get_addr` / mrs reads; the hot
    /// path used to do 2 reads in `js_async_step_chain` and 4 writes
    /// per Task::AsyncStep — packing cuts both in half.
    ///
    /// **Throw-trap routing.** If the callback throws and unwinds
    /// through the runner's outer `setjmp`, the trap reads `trap_next`
    /// to know which `next` Promise to reject with the exception.
    ///
    /// **Async-step Promise reuse.** When the callback is an
    /// async-step body and it calls `js_async_step_chain`, the chain
    /// helper reuses `trap_next` instead of allocating a fresh Promise
    /// per await — gated by `current_step` matching the step closure
    /// passed in (proves the call came from the SAME async function
    /// activation, not a NESTED one whose own `next` shouldn't be
    /// collapsed onto its parent's).
    pub(crate) static INLINE_TRAP: std::cell::Cell<InlineTrap>
        = const { std::cell::Cell::new(InlineTrap { trap_next: std::ptr::null_mut(), current_step: 0 }) };

    /// Defensive re-entry guard for the async step driver (issue #712).
    ///
    /// Tracks consecutive `is_error=true` AsyncStep dispatches from the
    /// SAME step closure. The original report (v0.5.836) produced 5.7M
    /// identical "value is not a function" lines because the throw
    /// closure's `__gen_state = post_catch_state` transitioned to a
    /// state that re-evaluated the same failing `await` expression —
    /// the catch arm re-fired, AsyncStepChain re-enqueued, repeat.
    ///
    /// A correct async state machine alternates: on a throwing await
    /// the runner gets ONE is_error=true entry per throw, immediately
    /// followed by is_error=false entries that resume the post-catch
    /// states. Programs that legitimately throw 1M+ times in a loop
    /// (e.g. `for (i of bigArr) try { await fail() } catch {}`)
    /// interleave is_error=false steps between the catches, so the
    /// consecutive count never grows beyond 1.
    ///
    /// On exceed: reject `next` with a synthesized TypeError and skip
    /// the step dispatch. This bounds the worst-case loop at
    /// `ASYNC_STEP_REENTRY_BOUND` iterations instead of unbounded.
    pub(crate) static ASYNC_STEP_GUARD: std::cell::Cell<AsyncStepGuard>
        = const { std::cell::Cell::new(AsyncStepGuard { last_closure: 0, consecutive_error_count: 0 }) };
}

/// Defensive guard state for the async step driver. See `ASYNC_STEP_GUARD`.
#[derive(Copy, Clone)]
pub(crate) struct AsyncStepGuard {
    pub last_closure: usize,
    pub consecutive_error_count: u32,
}

/// Upper bound on consecutive same-closure is_error=true AsyncStep
/// dispatches before the runner rejects the Promise as a runaway loop.
/// Picked well above any legitimate throw-in-a-loop pattern (those
/// interleave is_error=false steps so the count resets each iteration)
/// and well below the 5.7M observed in #712 — high enough to avoid
/// false positives, low enough to terminate quickly when the bug fires.
pub(crate) const ASYNC_STEP_REENTRY_BOUND: u32 = 10_000;

pub(crate) fn enqueue_queue_microtask(callback: i64) {
    let context = capture_context();
    let ids = crate::async_hooks::init_resource(
        "Microtask",
        f64::from_bits(crate::value::TAG_UNDEFINED),
        false,
    );
    TASK_QUEUE.with(|q| {
        q.borrow_mut().push_back(Task::Microtask {
            callback: callback as ClosurePtr,
            context,
            async_id: ids.async_id,
            trigger_async_id: ids.trigger_async_id,
        });
    });
    crate::event_pump::js_notify_main_thread();
}

#[derive(Default)]
pub(crate) struct PromiseContextStore {
    entries: HashMap<usize, AsyncContextSnapshot>,
    keys: Vec<usize>,
}

impl PromiseContextStore {
    pub(crate) fn insert(&mut self, key: usize, snapshot: AsyncContextSnapshot) {
        if !self.entries.contains_key(&key) {
            self.keys.push(key);
        }
        self.entries.insert(key, snapshot);
    }

    pub(crate) fn get(&self, key: &usize) -> Option<&AsyncContextSnapshot> {
        self.entries.get(key)
    }

    pub(crate) fn get_mut(&mut self, key: &usize) -> Option<&mut AsyncContextSnapshot> {
        self.entries.get_mut(key)
    }

    pub(crate) fn remove(&mut self, key: &usize) -> Option<AsyncContextSnapshot> {
        let removed = self.entries.remove(key);
        if removed.is_some() {
            if let Some(pos) = self.keys.iter().position(|candidate| candidate == key) {
                self.keys.swap_remove(pos);
            }
        }
        removed
    }

    #[cfg(test)]
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.keys.clear();
    }

    pub(crate) fn key_at(&self, index: usize) -> Option<usize> {
        self.keys.get(index).copied()
    }

    #[cfg(test)]
    pub(crate) fn keys(&self) -> impl Iterator<Item = &usize> {
        self.keys.iter()
    }

    #[cfg(test)]
    pub(crate) fn first(&self) -> Option<(usize, &AsyncContextSnapshot)> {
        self.keys
            .first()
            .and_then(|key| self.entries.get(key).map(|snapshot| (*key, snapshot)))
    }

    fn retain(&mut self, mut keep: impl FnMut(usize, &mut AsyncContextSnapshot) -> bool) {
        let mut index = 0;
        while index < self.keys.len() {
            let key = self.keys[index];
            let retain = self
                .entries
                .get_mut(&key)
                .is_some_and(|snapshot| keep(key, snapshot));
            if retain {
                index += 1;
            } else {
                self.keys.swap_remove(index);
                self.entries.remove(&key);
            }
        }
    }

    fn rekey(&mut self, old_key: usize, new_key: usize) {
        if old_key == new_key {
            return;
        }
        let Some(context) = self.entries.remove(&old_key) else {
            return;
        };
        if let Some(pos) = self.keys.iter().position(|key| *key == old_key) {
            self.keys[pos] = new_key;
        } else if !self.entries.contains_key(&new_key) {
            self.keys.push(new_key);
        }
        self.entries.insert(new_key, context);
    }
}

pub(crate) fn set_promise_callback_context(promise: *mut Promise) {
    if promise.is_null() {
        return;
    }
    let snapshot = capture_context();
    set_promise_context_snapshot(promise, snapshot);
}

pub(crate) fn set_promise_context_snapshot(promise: *mut Promise, snapshot: AsyncContextSnapshot) {
    if promise.is_null() {
        return;
    }
    PROMISE_CONTEXTS.with(|contexts| {
        contexts.borrow_mut().insert(promise as usize, snapshot);
    });
}

pub(crate) fn context_for_promise(promise: *mut Promise) -> AsyncContextSnapshot {
    if promise.is_null() {
        return capture_context();
    }
    PROMISE_CONTEXTS.with(|contexts| {
        contexts
            .borrow()
            .get(&(promise as usize))
            .cloned()
            .unwrap_or_else(capture_context)
    })
}

pub(crate) fn clear_promise_context(promise: *mut Promise) {
    if promise.is_null() {
        return;
    }
    PROMISE_CONTEXTS.with(|contexts| {
        contexts.borrow_mut().remove(&(promise as usize));
    });
}

pub(crate) fn clear_promise_context_for_gc(promise: *mut Promise) {
    clear_promise_context(promise);
}

pub(crate) fn cleanup_copied_minor_promise_contexts_for_gc() {
    PROMISE_CONTEXTS.with(|contexts| {
        let mut contexts = contexts.borrow_mut();
        let mut moved = Vec::new();
        contexts.retain(|key, _| {
            let space = crate::arena::classify_heap_space(key);
            let in_from_space = matches!(space, crate::arena::HeapSpace::NurseryEden)
                || space == crate::arena::active_survivor_space();
            if !in_from_space || key < crate::gc::GC_HEADER_SIZE {
                return true;
            }
            unsafe {
                let header =
                    (key as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*header).obj_type != crate::gc::GC_TYPE_PROMISE
                    || (*header).gc_flags & crate::gc::GC_FLAG_ARENA == 0
                {
                    return true;
                }
                if (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
                    let new_key = crate::gc::forwarding_address(header) as usize;
                    if new_key != key {
                        moved.push((key, new_key));
                    }
                    return true;
                }
            }
            false
        });
        for (old_key, new_key) in moved {
            contexts.rekey(old_key, new_key);
        }
    });
}

pub(crate) fn enter_microtask_context(snapshot: &AsyncContextSnapshot) {
    let previous = enter_context(snapshot);
    MICROTASK_PREV_CONTEXTS.with(|stack| {
        stack.borrow_mut().push(previous);
    });
}

pub(crate) fn restore_microtask_context() {
    MICROTASK_PREV_CONTEXTS.with(|stack| {
        if let Some(previous) = stack.borrow_mut().pop() {
            restore_context(previous);
        }
    });
}

pub(crate) fn restore_all_microtask_contexts() {
    MICROTASK_PREV_CONTEXTS.with(|stack| {
        let mut stack = stack.borrow_mut();
        while let Some(previous) = stack.pop() {
            restore_context(previous);
        }
    });
}

/// Packed thread-local state for the inline-microtask trap. See
/// `INLINE_TRAP` for the lifecycle and gating discussion.
#[derive(Copy, Clone)]
pub(crate) struct InlineTrap {
    pub trap_next: *mut Promise,
    pub current_step: usize,
}

impl InlineTrap {
    #[inline(always)]
    pub(crate) const fn empty() -> Self {
        InlineTrap {
            trap_next: std::ptr::null_mut(),
            current_step: 0,
        }
    }
}

/// Returns 1 iff the current thread's microtask queue has at least one
/// pending entry, 0 otherwise. Used by the codegen-emitted event-loop
/// active-handle check (#591) so the loop doesn't exit while a chained
/// `.then(...)` callback is waiting in the queue. Without this, an
/// `await` driven by perry-stdlib's `js_stdlib_process_pending`
/// resolution path could push a continuation to TASK_QUEUE in the
/// SAME body iteration that flips the active-handle counter to zero;
/// the next header check would then exit before the next body's
/// microtask drain runs.
#[no_mangle]
pub extern "C" fn js_microtasks_pending() -> i32 {
    if crate::node_submodules::diagnostics_channel_has_pending_publishes() {
        return 1;
    }
    if crate::thread::js_thread_has_pending() != 0 {
        return 1;
    }
    if crate::builtins::queued_microtasks_pending() {
        return 1;
    }
    TASK_QUEUE.with(|q| if q.borrow().is_empty() { 0 } else { 1 })
}
