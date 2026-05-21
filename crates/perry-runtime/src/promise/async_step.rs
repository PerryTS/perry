//! Async-step driver: `js_promise_resolved[_then]`, `js_async_step_chain`,
//! `js_async_step_done`, the async-step thunk cache, and
//! `Array.fromAsync`. These are the fast paths the async-to-generator
//! transform calls per `await`.

use super::*;
#[no_mangle]
pub extern "C" fn js_promise_resolved(value: f64) -> *mut Promise {
    bump(&MT_PROMISE_RESOLVED_COUNT);
    let value = adapt_foreign_promise_value(value);

    // FAST PATH: NaN-boxed primitives (numbers, undefined, null, bool,
    // raw f64s) are not pointers to thenables/promises. We can build a
    // pre-fulfilled promise and skip the `is_promise` + `assimilate`
    // probes — both are slow in the steady state of the async-to-
    // generator pattern (`Promise.resolve(<primitive>).then(...)` per
    // await). The probes still run on real pointers below.
    if is_definitely_primitive(value) {
        let promise = js_promise_new();
        unsafe {
            (*promise).state = PromiseState::Fulfilled;
            (*promise).value = value;
        }
        return promise;
    }
    let promise = js_promise_new();
    if js_value_is_promise(value) != 0 {
        bump(&MT_INNER_PROMISE_UNWRAP_COUNT);
        let inner = crate::value::js_nanbox_get_pointer(value) as *mut Promise;
        if !inner.is_null() && inner != promise {
            js_promise_resolve_with_promise(promise, inner);
            return promise;
        }
    }

    // Issue #586: ECMAScript thenable assimilation. The async-to-generator
    // transform rewrites every `await x` into `Promise.resolve(x).then(...)`
    // — which means thenable assimilation has to happen here, not in the
    // codegen-side `Expr::Await` lowering. `js_assimilate_thenable` returns
    // a fresh Promise wrapper that follows the thenable's `.then(resolve,
    // reject)` callbacks; chain its eventual state into our outer promise
    // via the same `js_promise_resolve_with_promise` pattern as the real-
    // Promise arm above. Drizzle's `QueryPromise` (`then` triggers the SQL
    // round-trip) is the load-bearing motivating case (#488).
    let assim = js_assimilate_thenable(value);
    if assim.to_bits() != value.to_bits() && js_value_is_promise(assim) != 0 {
        let inner = crate::value::js_nanbox_get_pointer(assim) as *mut Promise;
        if !inner.is_null() && inner != promise {
            js_promise_resolve_with_promise(promise, inner);
            return promise;
        }
    }

    js_promise_resolve(promise, value);
    promise
}

/// Fused fast path for `Promise.resolve(value).then(cb_f, cb_e)` —
/// the steady-state shape of the async-to-generator transform's
/// per-`await` lowering. The naive sequence is:
///
///   p1 = js_promise_resolved(value)              // alloc Promise #1
///   p2 = js_promise_then(p1, cb_f, cb_e)         // alloc Promise #2
///   return p2
///
/// Per-await we pay 2 Promise allocations + 2 TASK_QUEUE round-trips
/// (push to queue, pop, dispatch cb_f). For a 200k-await benchmark
/// that's 400k allocations — the dominant per-microtask cost.
///
/// The fast path:
///   if `value` is a primitive (no possibility of being a thenable
///   or another Promise), allocate ONE promise (`next`), enqueue an
///   INLINE-callback task carrying `(cb_f, value, next)`, and return
///   `next`. Skips Promise #1 entirely. The microtask runner's
///   `Task::Inline` arm dispatches the callback and propagates its
///   return value to `next` exactly as the legacy two-promise path
///   would have.
///
/// Falls back to the unfused sequence when `value` is a real Promise
/// or could be a thenable — those need the proper assimilation path.
#[no_mangle]
pub extern "C" fn js_promise_resolved_then(
    value: f64,
    on_fulfilled: ClosurePtr,
    on_rejected: ClosurePtr,
) -> *mut Promise {
    if is_definitely_primitive(value) {
        bump(&MT_FAST_PATH_HIT);
        // FAST PATH (primitive) — skip Promise.resolve()'s allocation
        // entirely. The callback runs once during the next microtask
        // drain via the `Task::Inline` arm.
        let next = js_promise_new();
        TASK_QUEUE.with(|q| {
            q.borrow_mut().push_back(Task::Inline(
                on_fulfilled,
                value,
                next,
                true,
                capture_context(),
            ));
        });
        crate::event_pump::js_notify_main_thread();
        // Suppress the rejection-handler bookkeeping: it would only
        // matter if `value` were a Promise, which it isn't here.
        let _ = on_rejected;
        return next;
    }

    // FAST PATH (already-a-Promise) — `Promise.resolve(P)` is the
    // identity per spec when P is a real Promise (the same constructor
    // is used). The async-to-generator transform's per-`await` lowering
    // routes through `Promise.resolve(__step_r.value).then(...)`; in
    // the steady state `__step_r.value` is a Promise that the user's
    // `await <expr>` produced, so the wrap-then-unwrap is wasted work.
    // Skip the wrapper allocation: directly chain `.then()` off the
    // existing promise.
    if js_value_is_promise(value) != 0 {
        bump(&MT_FAST_PATH_HIT);
        let inner = crate::value::js_nanbox_get_pointer(value) as *mut Promise;
        if !inner.is_null() {
            return js_promise_then(inner, on_fulfilled, on_rejected);
        }
    }

    bump(&MT_FAST_PATH_MISS);
    // Pointer-tagged but not a Promise — could be a thenable. Take
    // the unfused path so the assimilation probes can run on it.
    let p1 = js_promise_resolved(value);
    js_promise_then(p1, on_fulfilled, on_rejected)
}

/// Specialized version of `js_promise_resolved_then` for the async-step
/// driver's per-`await` lowering: equivalent to
/// `Promise.resolve(value).then(v => step(v, false), e => step(e, true))`
/// but skips the `then_v_arrow` / `then_e_arrow` wrapper closures
/// entirely by carrying `step_closure` directly through the task queue
/// and invoking it as a 2-arg call at dispatch time.
///
/// Primary perf wins:
///   - One fewer closure allocation per await on the primitive-value
///     fast path (was: `Task::Inline` carrying `then_v_arrow`; is now
///     `Task::AsyncStep` carrying `step_closure`).
///   - One fewer closure dispatch per microtask (was: `then_v_arrow`
///     body called `step_closure`; is now `step_closure` is invoked
///     directly by the runner).
#[no_mangle]
pub extern "C" fn js_async_step_chain(value: f64, step_closure: ClosurePtr) -> *mut Promise {
    // PR #1004 followup: if `value` is a JS_HANDLE_TAG handle to a V8
    // Promise (the common case for `await <V8-fallback-call>(...)` —
    // e.g. `await new SignJWT(...).sign(key)` in jose), convert it to
    // a native pending Promise before any of the dispatch logic looks
    // at it. Without this, `is_definitely_primitive(value)` returns
    // true for the handle (only POINTER_TAG values are non-primitive),
    // so the V8 Promise gets enqueued as the resolution value directly
    // — the next async step then sees the unresolved Promise object
    // as the `await` result, and code that expects the inner value
    // (e.g. jose's `jwtVerify(jwt, key)` where `jwt` should be a
    // string) observes `[object Promise]` instead. The other arms of
    // this function already handle native pending Promises correctly
    // via the `js_value_is_promise` + thunk path.
    let value = adapt_foreign_promise_value(value);

    // Reuse predicate. `next` reuse is sound only when AsyncStepChain
    // is being called from the body of the SAME step closure that the
    // runner is currently dispatching. Two readers of INLINE_TRAP pose
    // risk:
    //   - A nested `await` where step body's __next runs user TS that
    //     calls another async fn whose outer wrapper invokes its own
    //     step. That inner step's AsyncStepChain receives a DIFFERENT
    //     step closure → fails the gate → allocates a fresh Promise
    //     that becomes the inner async fn's user-facing return value.
    //   - The very first call from the outer wrapper (no microtask
    //     active yet) → INLINE_TRAP is empty → fails the gate.
    let trap = INLINE_TRAP.with(|c| c.get());
    let can_reuse = !trap.trap_next.is_null() && trap.current_step == step_closure as usize;
    let trap_next = trap.trap_next;

    let (next, queued_value, is_error) = if is_definitely_primitive(value) {
        // Primitive value: enqueue Task::AsyncStep directly.
        bump(&MT_FAST_PATH_HIT);
        (
            if can_reuse {
                bump(&MT_STEP_CHAIN_REUSE_HIT);
                trap_next
            } else {
                bump(&MT_STEP_CHAIN_REUSE_MISS);
                js_promise_new()
            },
            value,
            false,
        )
    } else if js_value_is_promise(value) != 0 {
        let inner = crate::value::js_nanbox_get_pointer(value) as *mut Promise;
        if !inner.is_null() {
            let inner_state = unsafe { (*inner).state };
            match inner_state {
                PromiseState::Fulfilled => {
                    // Inner already settled with a primitive (the steady
                    // state for `await Promise.resolve(<primitive>)`).
                    // Enqueue Task::AsyncStep with the unwrapped value;
                    // skip the legacy thunk-alloc + js_promise_then path.
                    let unwrapped = unsafe { (*inner).value };
                    (
                        if can_reuse {
                            bump(&MT_STEP_CHAIN_REUSE_HIT);
                            trap_next
                        } else {
                            bump(&MT_STEP_CHAIN_REUSE_MISS);
                            js_promise_new()
                        },
                        unwrapped,
                        false,
                    )
                }
                PromiseState::Rejected => {
                    let reason = unsafe { (*inner).reason };
                    (
                        if can_reuse {
                            bump(&MT_STEP_CHAIN_REUSE_HIT);
                            trap_next
                        } else {
                            bump(&MT_STEP_CHAIN_REUSE_MISS);
                            js_promise_new()
                        },
                        reason,
                        true,
                    )
                }
                PromiseState::Pending => {
                    // Inner still pending — fall back to the thunk
                    // path. We can't enqueue a Task::AsyncStep until
                    // the inner settles; install fulfill/reject thunks
                    // that will queue the right Task when called.
                    bump(&MT_STEP_CHAIN_REUSE_MISS);
                    let (fulfill, reject) = build_async_step_thunks(step_closure);
                    return js_promise_then(inner, fulfill, reject);
                }
            }
        } else {
            bump(&MT_STEP_CHAIN_REUSE_MISS);
            let (fulfill, reject) = build_async_step_thunks(step_closure);
            let p = js_promise_resolved(value);
            return js_promise_then(p, fulfill, reject);
        }
    } else {
        // Pointer-tagged but not a Promise (thenable etc.). Take the
        // fully-general path so assimilation runs.
        bump(&MT_STEP_CHAIN_REUSE_MISS);
        let (fulfill, reject) = build_async_step_thunks(step_closure);
        let p = js_promise_resolved(value);
        return js_promise_then(p, fulfill, reject);
    };

    TASK_QUEUE.with(|q| {
        q.borrow_mut().push_back(Task::AsyncStep(
            step_closure,
            queued_value,
            next,
            is_error,
            capture_context(),
        ));
    });
    crate::event_pump::js_notify_main_thread();
    next
}

/// Done-case companion to `js_async_step_chain`. Replaces the
/// `Promise.resolve(value)` allocation in the state-machine terminal
/// branch when step is dispatched from inside the microtask runner.
///
/// Fast path (steady state): INLINE_TRAP_NEXT is non-null (runner
/// stashed it before calling step) and CURRENT_STEP_CLOSURE matches
/// `step_closure` (proves we're resolving for THIS step, not an
/// outer activation we'd corrupt). Resolve `trap_next` with `value`
/// in-place and return it; step returns the same Promise to the
/// runner which fires the self-chain check and skips the propagate
/// hop. Net effect: zero new Promise allocations on the done path.
///
/// Slow path: trap_next is null (initial entry to a no-await async
/// function) or step_closure doesn't match (nested async-fn call,
/// where the outer activation's `next` must NOT be settled here).
/// Fall back to `js_promise_resolved(value)`.
#[no_mangle]
pub extern "C" fn js_async_step_done(value: f64, step_closure: ClosurePtr) -> *mut Promise {
    // PR #1004 followup (sibling to js_async_step_chain): adapt a
    // JS_HANDLE_TAG V8 Promise into a native Promise before storing it
    // as the resolution value, so `async function f() { return
    // v8Promise; }` produces a Promise that resolves to the inner
    // value (per ES spec for an async fn returning a thenable) instead
    // of a Promise whose resolution value is the unresolved V8 Promise
    // handle.
    let value = adapt_foreign_promise_value(value);

    let trap = INLINE_TRAP.with(|c| c.get());
    if !trap.trap_next.is_null() && trap.current_step == step_closure as usize {
        bump(&MT_STEP_DONE_REUSE_HIT);
        js_promise_resolve(trap.trap_next, value);
        trap.trap_next
    } else {
        bump(&MT_STEP_DONE_REUSE_MISS);
        js_promise_resolved(value)
    }
}

/// #691 Phase 2 helper. Returns the currently-dispatching step
/// closure as a raw `*mut ClosureHeader`. Codegen NaN-boxes the
/// result. Lets the async-to-generator transform emit
/// `Expr::CurrentStepClosure` inside the step body in place of a
/// captured `step_id` self-reference — saves one `js_box_alloc` per
/// async-fn invocation and shrinks the step closure by one capture
/// slot. Safe to call only from inside a step body (or from any code
/// known to run inside `Task::AsyncStep` dispatch or
/// `js_async_first_call`); returns null otherwise.
#[no_mangle]
pub extern "C" fn js_get_current_step_closure() -> *mut crate::closure::ClosureHeader {
    let trap = INLINE_TRAP.with(|c| c.get());
    trap.current_step as *mut crate::closure::ClosureHeader
}

/// #691 Phase 2 helper. Invoke a freshly-built step closure for the
/// very first state transition of an async-fn activation. The wrapper
/// emits this in place of `__step(undefined, false)` so that
/// `CURRENT_STEP_CLOSURE` TLS is set before the body runs — without
/// this setup, `js_get_current_step_closure` inside the body would
/// observe whatever the previous `Task::AsyncStep` left (or null on
/// cold entry). Saves and restores the previous trap state so nested
/// async-fn calls compose correctly.
///
/// Takes the closure NaN-boxed (the HIR caller passes it via a
/// regular Expr) and returns the closure's own return value
/// (typically a Promise pointer NaN-boxed by the step body).
#[no_mangle]
pub extern "C" fn js_async_first_call(step_closure_nanbox: f64) -> f64 {
    let ptr = crate::value::js_nanbox_get_pointer(step_closure_nanbox)
        as *mut crate::closure::ClosureHeader;
    // CRITICAL: clear `trap_next` for the inner activation. The previous
    // implementation preserved `old.trap_next` so the inner step would
    // "compose correctly" — but that allowed the inner async fn's
    // `js_async_step_chain` to satisfy `can_reuse = trap_next != null
    // && current_step == step_closure` (current_step was just set to
    // `ptr`, the new inner step), causing the inner's first state
    // transition to reuse and settle the OUTER activation's `trap_next`
    // promise prematurely with the inner's intermediate value. Visible
    // symptom: `async function tC() { try { await Promise.reject(e); }
    // catch (e) { const r = await innerAsync(); return "wrap: " + r; } }`
    // — tC.then fires with the inner's value (e.g. "helpC") instead of
    // tC's actual wrapped return value ("wrap: helpC"). Path-dependent:
    // top-level `await innerAsync()` (no try/catch) happens to work
    // because the outer step's continuation overwrites the inner's
    // premature settlement with the correct value; the busy-wait
    // path used by Expr::Await inside __async_throw catch handlers
    // doesn't have this overwrite, so the inner's premature settlement
    // is the final state of tC's promise.
    //
    // Forcing `trap_next = null` here makes the inner's
    // `js_async_step_chain` fail the gate, allocate its own next
    // Promise, and only chain through that — leaving the outer
    // `trap_next` untouched. The outer's chain reuse on its OWN
    // resumption is unaffected (this restore at function exit puts
    // `prev` back).
    let prev = INLINE_TRAP.with(|c| {
        let old = c.get();
        c.set(InlineTrap {
            trap_next: std::ptr::null_mut(),
            current_step: ptr as usize,
        });
        old
    });
    let result = {
        crate::closure::js_closure_call2(
            ptr,
            f64::from_bits(0x7FFC_0000_0000_0001), // TAG_UNDEFINED
            f64::from_bits(0x7FFC_0000_0000_0003), // TAG_FALSE
        )
    };
    INLINE_TRAP.with(|c| c.set(prev));
    result
}

// Thread-local single-slot cache for async-step thunks. Keyed by the
// step closure pointer. When the same step closure is used across
// multiple promise-of-promise awaits (the simple-probe shape), we
// return the cached thunks; otherwise we allocate. The thunks are
// GC-rooted via `ASYNC_STEP_THUNK_CACHE_SCANNER` so they survive
// collection until evicted by a different step closure.
thread_local! {
    pub(super) static LAST_ASYNC_STEP_THUNKS: std::cell::Cell<(usize, *mut crate::closure::ClosureHeader, *mut crate::closure::ClosureHeader)> =
        const { std::cell::Cell::new((0, std::ptr::null_mut(), std::ptr::null_mut())) };
}

/// GC root scanner for the LAST_ASYNC_STEP_THUNKS cache.
pub fn scan_async_step_thunk_cache(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_async_step_thunk_cache_mut(&mut visitor);
}

pub fn scan_async_step_thunk_cache_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    LAST_ASYNC_STEP_THUNKS.with(|c| {
        let (mut key, mut fulfill, mut reject) = c.get();
        let mut changed = visitor.visit_metadata_usize_slot(&mut key);
        changed |= visitor.visit_raw_mut_ptr_slot(&mut fulfill);
        changed |= visitor.visit_raw_mut_ptr_slot(&mut reject);
        if changed {
            c.set((key, fulfill, reject));
        }
    });
}

/// Build (fulfill, reject) thunks for the async-step promise-chain
/// fallback. Uses LAST_ASYNC_STEP_THUNKS as a single-slot cache —
/// hits 100% in the simple-probe shape (one step closure across all
/// awaits) while degrading gracefully (no cache overhead beyond the
/// cell read/write) when many distinct step closures interleave (the
/// Promise.all-of-N shape).
fn build_async_step_thunks(step_closure: ClosurePtr) -> (ClosurePtr, ClosurePtr) {
    let key = step_closure as usize;
    let cached = LAST_ASYNC_STEP_THUNKS.with(|c| c.get());
    if cached.0 == key && !cached.1.is_null() && !cached.2.is_null() {
        return (cached.1 as ClosurePtr, cached.2 as ClosurePtr);
    }
    use crate::closure::{js_closure_alloc, js_closure_set_capture_ptr};
    let fulfill = js_closure_alloc(async_step_fulfill_thunk as *const u8, 1);
    js_closure_set_capture_ptr(fulfill, 0, step_closure as i64);
    let reject = js_closure_alloc(async_step_reject_thunk as *const u8, 1);
    js_closure_set_capture_ptr(reject, 0, step_closure as i64);
    LAST_ASYNC_STEP_THUNKS.with(|c| c.set((key, fulfill, reject)));
    (fulfill as ClosurePtr, reject as ClosurePtr)
}

// Thunks for the promise/thenable fallback in js_async_step_chain.
// Capture layout: [step_closure_ptr]
extern "C" fn async_step_fulfill_thunk(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let step = crate::closure::js_closure_get_capture_ptr(closure, 0)
        as *const crate::closure::ClosureHeader;
    let false_bits = f64::from_bits(0x7FFC_0000_0000_0003);
    // #691 Phase 2: when this thunk is invoked from the pending-Promise
    // fallback in js_async_step_chain (await of a still-pending inner),
    // the runtime arrives here via Task::Inline dispatch which does NOT
    // set INLINE_TRAP.current_step. Step bodies now read self from that
    // TLS via Expr::CurrentStepClosure, so we MUST set it before
    // entering the step. Save/restore for nested-async composition.
    let prev = INLINE_TRAP.with(|c| {
        let old = c.get();
        c.set(InlineTrap {
            trap_next: old.trap_next,
            current_step: step as usize,
        });
        old
    });
    let result = crate::closure::js_closure_call2(step, value, false_bits);
    INLINE_TRAP.with(|c| c.set(prev));
    result
}

extern "C" fn async_step_reject_thunk(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let step = crate::closure::js_closure_get_capture_ptr(closure, 0)
        as *const crate::closure::ClosureHeader;
    let true_bits = f64::from_bits(0x7FFC_0000_0000_0004);
    // #691 Phase 2: see async_step_fulfill_thunk — same TLS-setup
    // requirement on the rejection path.
    let prev = INLINE_TRAP.with(|c| {
        let old = c.get();
        c.set(InlineTrap {
            trap_next: old.trap_next,
            current_step: step as usize,
        });
        old
    });
    let result = crate::closure::js_closure_call2(step, value, true_bits);
    INLINE_TRAP.with(|c| c.set(prev));
    result
}

/// `Array.fromAsync(input)` — Node 22+ static method.
///
/// Returns a Promise that resolves to an Array. Two input shapes:
///   1. **Array**: each element is awaited (if it's a Promise) and the
///      results are collected. Equivalent to `Promise.all(input)`.
///   2. **Async iterator** (object with a `.next()` method): we call
///      `.next()` repeatedly via the closure-chained .then() pattern,
///      pushing each `value` until `done` is true, then resolve the
///      output Promise with the collected array.
///
/// `input` is the NaN-boxed input value. Returns a NaN-boxed Promise
/// pointer (POINTER_TAG) so the caller's `await` can unwrap it.
#[no_mangle]
pub extern "C" fn js_array_from_async(input: f64) -> f64 {
    use crate::array::{js_array_alloc, ArrayHeader};
    use crate::closure::{js_closure_alloc, js_closure_set_capture_ptr};
    use crate::value::js_nanbox_get_pointer;

    // Strip NaN-box to get the raw pointer.
    let raw_ptr = js_nanbox_get_pointer(input) as usize;
    if raw_ptr == 0 {
        // null/undefined input — resolve to empty array
        let empty = js_array_alloc(0);
        unsafe {
            (*empty).length = 0;
        }
        let arr_f64 = crate::value::js_nanbox_pointer(empty as i64);
        let p = js_promise_resolved(arr_f64);
        return crate::value::js_nanbox_pointer(p as i64);
    }

    // Path 1: input is an Array. Reuse Promise.all behavior — js_promise_all
    // handles a mix of promise and non-promise elements correctly.
    unsafe {
        let gc_header =
            (raw_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
            let arr_ptr = raw_ptr as *const ArrayHeader;
            let p = js_promise_all(arr_ptr);
            return crate::value::js_nanbox_pointer(p as i64);
        }
    }

    // Path 2: async iterator (or any other object). Allocate a result
    // Promise and an empty result Array, then kick off the .next() chain.
    let result_promise = js_promise_new();
    let result_arr = js_array_alloc(0);
    unsafe {
        (*result_arr).length = 0;
    }

    // Build the recursive .next() handler closure. Captures:
    //   [0] result_promise (Promise to resolve at the end)
    //   [1] result_arr (Array to push each value into)
    //   [2] iter object (raw pointer; we re-NaN-box on .next() call)
    let chain_closure = js_closure_alloc(array_from_async_step as *const u8, 3);
    js_closure_set_capture_ptr(chain_closure, 0, result_promise as i64);
    js_closure_set_capture_ptr(chain_closure, 1, result_arr as i64);
    js_closure_set_capture_ptr(chain_closure, 2, raw_ptr as i64);

    // Kick off the first .next() call. The handler returns the iter result
    // (or undefined for done) — we wire it through `.then(chain_closure)`
    // which will recurse.
    unsafe {
        array_from_async_call_next(raw_ptr, chain_closure);
    }

    crate::value::js_nanbox_pointer(result_promise as i64)
}

/// Helper that calls `iter.next()` (returning a Promise) and attaches
/// `chain_closure` as both fulfill and reject handlers. Used by the async
/// iterator path of `js_array_from_async`.
unsafe fn array_from_async_call_next(
    iter_ptr: usize,
    chain_closure: *const crate::closure::ClosureHeader,
) {
    // Re-NaN-box the iter pointer for js_native_call_method.
    let iter_f64 = crate::value::js_nanbox_pointer(iter_ptr as i64);
    let method_name = b"next";
    // Call iter.next() — returns a Promise<{value, done}> for async generators
    // or `{value, done}` directly for sync iterators.
    let next_result = crate::object::js_native_call_method(
        iter_f64,
        method_name.as_ptr() as *const i8,
        method_name.len(),
        std::ptr::null(),
        0,
    );

    // If the result is a Promise pointer, attach the handler via .then.
    let next_ptr = crate::value::js_nanbox_get_pointer(next_result) as usize;
    if next_ptr != 0 {
        let gc_header =
            (next_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_PROMISE {
            let next_promise = next_ptr as *mut Promise;
            // Use the chain_closure for both fulfill and reject. On rejection
            // we just propagate by resolving the result_promise with undefined.
            js_promise_then(
                next_promise,
                chain_closure as *const _,
                chain_closure as *const _,
            );
            return;
        }
    }
    // Synchronous iterator path: invoke the handler directly with the
    // result so the iteration loop continues without going through .then.
    array_from_async_step(chain_closure as *const _, next_result);
}

/// `.then(...)` handler invoked once per `.next()` resolution. Reads the
/// `{value, done}` iter-result, pushes `value` into the accumulator array,
/// and either resolves the output Promise (when `done`) or schedules
/// another `.next()` call.
extern "C" fn array_from_async_step(
    closure: *const crate::closure::ClosureHeader,
    iter_result: f64,
) -> f64 {
    use crate::array::{js_array_push_f64, ArrayHeader};
    use crate::closure::{js_closure_get_capture_ptr, js_closure_set_capture_ptr};

    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let mut result_arr = js_closure_get_capture_ptr(closure, 1) as *mut ArrayHeader;
    let iter_ptr = js_closure_get_capture_ptr(closure, 2) as usize;

    if result_promise.is_null() || result_arr.is_null() || iter_ptr == 0 {
        return 0.0;
    }

    // Read `done` and `value` off the iter result. The result is either an
    // object with those fields, or undefined (if next() returned undefined).
    let result_bits = iter_result.to_bits();
    let result_obj_ptr = if (result_bits >> 48) == 0x7FFD {
        (result_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::object::ObjectHeader
    } else if result_bits != 0 && result_bits <= 0x0000_FFFF_FFFF_FFFF {
        result_bits as *const crate::object::ObjectHeader
    } else {
        // Treat malformed result as `done: true`.
        std::ptr::null()
    };

    if result_obj_ptr.is_null() {
        // No more values — resolve the output promise with the collected array.
        let arr_f64 = crate::value::js_nanbox_pointer(result_arr as i64);
        js_promise_resolve(result_promise, arr_f64);
        return 0.0;
    }

    // Look up "done" and "value" fields by name.
    let done_key = make_static_string(b"done");
    let value_key = make_static_string(b"value");
    let done_jv = crate::object::js_object_get_field_by_name(result_obj_ptr, done_key);
    let value_jv = crate::object::js_object_get_field_by_name(result_obj_ptr, value_key);
    let done_f64 = f64::from_bits(done_jv.bits());
    let value_f64 = f64::from_bits(value_jv.bits());

    if crate::value::js_is_truthy(done_f64) != 0 {
        // Iteration complete — resolve with the accumulated array.
        let arr_f64 = crate::value::js_nanbox_pointer(result_arr as i64);
        js_promise_resolve(result_promise, arr_f64);
        return 0.0;
    }

    // Push the value (push_f64 may grow & return a new pointer).
    result_arr = js_array_push_f64(result_arr, value_f64);
    // Update the closure capture so subsequent steps see the (possibly
    // moved) array.
    js_closure_set_capture_ptr(closure as *mut _, 1, result_arr as i64);

    // Recurse: call iter.next() again. The same closure will be invoked
    // when the next promise resolves.
    unsafe {
        array_from_async_call_next(iter_ptr, closure);
    }

    0.0
}

/// Helper to allocate a static StringHeader for property-name lookups.
/// Reuses `js_string_from_bytes` so the result is GC-tracked.
fn make_static_string(bytes: &[u8]) -> *const crate::string::StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}
