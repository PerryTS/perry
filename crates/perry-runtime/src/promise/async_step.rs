//! Async-step driver: `js_promise_resolved[_then]`, `js_async_step_chain`,
//! `js_async_step_done`, the async-step thunk cache, and
//! `Array.fromAsync`. These are the fast paths the async-to-generator
//! transform calls per `await`.

use super::*;

/// Inverse of `then::arg_to_closure` — reboxes an already-unboxed
/// `ClosurePtr` handler arg back into a boxed `f64` JS value so it can be
/// forwarded to `js_promise_then_checked`. Null (the "no handler" sentinel)
/// becomes `undefined`, matching what `IsCallable(x) is false` resolves to
/// on the `_checked` entry's own tag check.
#[inline]
fn closure_ptr_to_arg(ptr: ClosurePtr) -> f64 {
    if ptr.is_null() {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        f64::from_bits(crate::value::JSValue::pointer(ptr as *const u8).bits())
    }
}

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
        // When lifecycle hooks are active, go through the real resolve path so
        // `settled`/`before`/`after` fire (instead of poking the state field).
        if crate::v8::promise_hooks_active() || crate::async_hooks::hooks_active() {
            js_promise_resolve(promise, value);
            return promise;
        }
        unsafe {
            (*promise).state = PromiseState::Fulfilled;
            (*promise).value = value;
        }
        return promise;
    }
    // Issue #2823: `Promise.resolve(p)` MUST return `p` itself when `p` is
    // already a native Promise (constructor === Promise). The spec defines
    // Promise.resolve to short-circuit and return the argument unchanged in
    // that case — object identity and pending-promise sharing are
    // observable (`Promise.resolve(p) === p`). Perry only constructs native
    // `Promise` instances, so a GC_TYPE_PROMISE value always satisfies the
    // "constructor is Promise" check. Return the existing pointer directly
    // instead of allocating a fresh wrapper and chaining to it.
    if js_value_is_promise(value) != 0 {
        let inner = crate::value::js_nanbox_get_pointer(value) as *mut Promise;
        if !inner.is_null() {
            return inner;
        }
    }
    let promise = js_promise_new();

    // Issue #586: ECMAScript thenable assimilation. The async-to-generator
    // transform rewrites every `await x` into `Promise.resolve(x).then(...)`
    // — which means thenable assimilation has to happen here, not in the
    // codegen-side `Expr::Await` lowering. `promise_resolve_assimilating`
    // implements the spec PromiseResolveThenableJob: a thenable's `.then` is
    // invoked from a SCHEDULED microtask, never synchronously during resolve.
    // This matters for `Promise.race`/`Promise.any` over a thenable, where Node
    // does not call the thenable's `then` until the job runs (so the count of
    // synchronous `then` invocations stays 0). Primitives (fast path above) and
    // native Promises (short-circuit above) never reach here, so the per-await
    // steady state is untouched; only real thenables (drizzle's `QueryPromise`,
    // object literals with `then`) defer by one microtask — which the await
    // loop drains, leaving the resolved value identical.
    super::assimilate::promise_resolve_assimilating(promise, value);
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
    // The primitive fast path below bypasses `js_promise_new`/`then`, so it
    // would never fire `v8.promiseHooks` (#3139). When hooks are active, route
    // through the real resolve+then path instead.
    if crate::v8::promise_hooks_active() {
        bump(&MT_FAST_PATH_MISS);
        let p1 = js_promise_resolved(value);
        return js_promise_then(p1, on_fulfilled, on_rejected);
    }

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
        let inner = crate::value::js_nanbox_get_pointer(value) as *mut Promise;
        if !inner.is_null() {
            // #5849: an own "then"/"constructor" expando on `inner` (e.g. a
            // native Promise whose `.then` was reassigned) must still be
            // invoked via `Invoke`/SpeciesConstructor semantics — the raw
            // `js_promise_then` below operates directly on the native
            // reaction slots and would silently ignore the override
            // (test262 resolve/resolve-prms-cstm-then). `promise_has_own_*`
            // bail out on a single cheap thread-local flag when no expando
            // has ever been installed on ANY value, so this costs nothing
            // in the steady-state await path this fast path exists for.
            let inner_addr = inner as usize;
            if super::promise_has_own_property(inner_addr, "then")
                || super::promise_has_own_constructor(inner_addr)
            {
                bump(&MT_FAST_PATH_MISS);
                let promise_val = box_promise_ptr(inner);
                let fulfilled_val = closure_ptr_to_arg(on_fulfilled);
                let rejected_val = closure_ptr_to_arg(on_rejected);
                let result =
                    super::js_promise_then_checked(promise_val, fulfilled_val, rejected_val);
                return crate::value::js_nanbox_get_pointer(result) as *mut Promise;
            }
            bump(&MT_FAST_PATH_HIT);
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
///
/// #5437: register the resume thunks on `awaited` AND keep the machine's
/// result promise consistent so `js_async_step_done`'s reuse gate can
/// settle it. When this machine was entered via `js_async_first_call`
/// (which nulls `trap_next` per #691), a plain `js_promise_then` here
/// yields a result promise that `done` cannot match (its reuse gate needs
/// a non-null `trap_next`), so `done` mints a fresh promise and ORPHANS
/// the result the caller awaits → the awaiter hangs (the Next.js SSR
/// render deadlock). Allocate the result explicitly, back-patch the
/// thunks' captured `trap_next` (capture slot 1) to it, and attach the
/// handlers WITHOUT a chained `next` (a chained `next` would make a
/// mid-chain await self-resolve → "Chaining cycle detected").
fn then_backpatch_result(
    awaited: *mut Promise,
    fulfill: ClosurePtr,
    reject: ClosurePtr,
    trap_next: *mut Promise,
) -> *mut Promise {
    if trap_next.is_null() {
        let result = super::then::js_promise_new_with_parent(awaited);
        crate::closure::js_closure_set_capture_ptr(
            fulfill as *mut crate::closure::ClosureHeader,
            1,
            result as i64,
        );
        crate::closure::js_closure_set_capture_ptr(
            reject as *mut crate::closure::ClosureHeader,
            1,
            result as i64,
        );
        super::then::js_promise_attach_handlers(awaited, fulfill, reject);
        result
    } else {
        js_promise_then(awaited, fulfill, reject)
    }
}

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
                    // `await`ing an already-rejected promise delivers its
                    // reason to this async step (which re-throws it into the
                    // suspended function body, where a surrounding try/catch
                    // can catch it) — so the rejection IS handled. The
                    // Pending arm below routes through `js_promise_then`, which
                    // marks the rejection handled; this fast path reads the
                    // reason field directly, so it must mark it explicitly or
                    // the program-end detector reports a spurious
                    // "Uncaught (in promise)" for a rejection that `await`
                    // already consumed (and exits non-zero).
                    crate::promise::mark_rejection_handled(inner);
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
                    let (fulfill, reject) = build_async_step_thunks(step_closure, trap_next);
                    return then_backpatch_result(inner, fulfill, reject, trap_next);
                }
            }
        } else {
            bump(&MT_STEP_CHAIN_REUSE_MISS);
            let (fulfill, reject) = build_async_step_thunks(step_closure, trap_next);
            let p = js_promise_resolved(value);
            return then_backpatch_result(p, fulfill, reject, trap_next);
        }
    } else {
        // Pointer-tagged but not a Promise (thenable etc.). Take the
        // fully-general path so assimilation runs.
        bump(&MT_STEP_CHAIN_REUSE_MISS);
        let (fulfill, reject) = build_async_step_thunks(step_closure, trap_next);
        let p = js_promise_resolved(value);
        return then_backpatch_result(p, fulfill, reject, trap_next);
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
        // Issue #4828: an async fn whose tail is `return <promise>` must
        // ADOPT the returned promise/thenable's eventual state into its
        // own result promise (`trap_next`), not store the promise object
        // as the literal resolution value. The slow path below already
        // does this via `js_promise_resolved`; the in-place reuse fast
        // path bypassed it with a raw `js_promise_resolve`, so awaiting
        // the outer fn yielded the inner Promise object itself (typeof
        // "object", JSON.stringify === "", every property undefined).
        // Mirror `js_promise_resolved`'s adoption probes here, but settle
        // the existing `trap_next` instead of allocating a fresh promise
        // so the runner's self-chain fast path still fires.
        resolve_trap_next_with_adoption(trap.trap_next, value);
        trap.trap_next
    } else {
        bump(&MT_STEP_DONE_REUSE_MISS);
        js_promise_resolved(value)
    }
}

/// Settle `target` with `value`, adopting `value`'s state when it is a
/// native Promise or an assimilable thenable (ECMAScript "Promise
/// Resolve Functions" thenable-adoption). Used by the `js_async_step_done`
/// reuse fast path so `return <promise>` from an async fn unwraps the
/// inner promise into the outer result promise (Issue #4828). Mirrors the
/// adoption arms of `js_promise_resolved`, but resolves an existing
/// `target` rather than constructing a new one.
fn resolve_trap_next_with_adoption(target: *mut Promise, value: f64) {
    // Primitives can never be a promise/thenable — settle directly.
    if is_definitely_primitive(value) {
        js_promise_resolve(target, value);
        return;
    }
    // Native Promise: chain `target` to follow its eventual state.
    if js_value_is_promise(value) != 0 {
        let inner = crate::value::js_nanbox_get_pointer(value) as *mut Promise;
        if !inner.is_null() && inner != target {
            js_promise_resolve_with_promise(target, inner);
            return;
        }
    }
    // User thenable (e.g. Drizzle QueryPromise, #586): assimilate, then
    // chain the wrapper promise into `target`.
    let assim = js_assimilate_thenable(value);
    if assim.to_bits() != value.to_bits() && js_value_is_promise(assim) != 0 {
        let inner = crate::value::js_nanbox_get_pointer(assim) as *mut Promise;
        if !inner.is_null() && inner != target {
            js_promise_resolve_with_promise(target, inner);
            return;
        }
    }
    // Plain object / non-thenable value: store as-is.
    js_promise_resolve(target, value);
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
fn build_async_step_thunks(
    step_closure: ClosurePtr,
    trap_next: *mut Promise,
) -> (ClosurePtr, ClosurePtr) {
    use crate::closure::{js_closure_alloc, js_closure_set_capture_ptr};
    let key = step_closure as usize;
    let cached = LAST_ASYNC_STEP_THUNKS.with(|c| c.get());
    if cached.0 == key && !cached.1.is_null() && !cached.2.is_null() {
        // #5485: refresh the captured activation `trap_next`. The step
        // closure (cache key) is per-activation, so this is normally the
        // same pointer; refreshing keeps us correct if the slot is reused.
        js_closure_set_capture_ptr(cached.1, 1, trap_next as i64);
        js_closure_set_capture_ptr(cached.2, 1, trap_next as i64);
        return (cached.1 as ClosurePtr, cached.2 as ClosurePtr);
    }
    // Capture layout: [step_closure_ptr, activation_trap_next_ptr] (#5485).
    let fulfill = js_closure_alloc(async_step_fulfill_thunk as *const u8, 2);
    js_closure_set_capture_ptr(fulfill, 0, step_closure as i64);
    js_closure_set_capture_ptr(fulfill, 1, trap_next as i64);
    let reject = js_closure_alloc(async_step_reject_thunk as *const u8, 2);
    js_closure_set_capture_ptr(reject, 0, step_closure as i64);
    js_closure_set_capture_ptr(reject, 1, trap_next as i64);
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
    // #5485: the activation's own trap_next, captured when this thunk was
    // built (at the `await`), not the ambient INLINE_TRAP.trap_next at
    // resume time — which may belong to an UNRELATED outer activation and,
    // preserved here, let this resumed step's js_async_step_done reuse-gate
    // settle that outer activation's result promise prematurely with this
    // (intermediate await) value. Restoring the captured per-activation
    // value keeps same-activation settlement working while preventing the
    // cross-activation leak.
    let captured_trap_next = crate::closure::js_closure_get_capture_ptr(closure, 1) as *mut Promise;
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
            trap_next: captured_trap_next,
            current_step: step as usize,
        });
        old
    });
    let result = crate::closure::js_closure_call2(step, value, false_bits);
    INLINE_TRAP.with(|c| c.set(prev));
    forward_swallowed_rejection(result, captured_trap_next);
    result
}

/// #5941: a thunk-resumed step that exits through its internal catch arm
/// (a `throw` after the await, or an awaited rejection with no matching
/// catch route) returns a FRESH rejected promise
/// (`return Promise.reject(e)` in the step body's catch) — historically
/// DISCARDED by the thunks, so the machine's real result promise (the
/// captured per-activation `trap_next`, #5485) stayed Pending and every
/// awaiter of the async fn hung instead of observing the throw. On the
/// first_call path the wrapper returns the rejected promise to the caller
/// directly, so only the resumed path leaked. Forward the rejection into
/// the activation's result. The normal resume paths return `trap_next`
/// itself (chain/done reuse) and are skipped by the identity check;
/// `js_async_step_done`'s non-reuse arm returning a user
/// `return <rejected promise>` also lands here, which matches spec (the
/// async fn's result rejects with the inner reason).
fn forward_swallowed_rejection(result: f64, trap_next: *mut Promise) {
    if trap_next.is_null() {
        return;
    }
    let bits = result.to_bits();
    if bits & 0xFFFF_0000_0000_0000 != 0x7FFD_0000_0000_0000 {
        return;
    }
    if js_value_is_promise(result) == 0 {
        return;
    }
    let ret = (bits & 0x0000_FFFF_FFFF_FFFF) as *mut Promise;
    if ret.is_null() || ret == trap_next {
        return;
    }
    unsafe {
        if (*ret).state != PromiseState::Rejected || (*trap_next).state != PromiseState::Pending {
            return;
        }
        let reason = (*ret).reason;
        // The rejection is consumed by forwarding it into the result —
        // the discarded wrapper must not be reported as unhandled.
        crate::promise::mark_rejection_handled(ret);
        js_promise_reject(trap_next, reason);
    }
}

extern "C" fn async_step_reject_thunk(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let step = crate::closure::js_closure_get_capture_ptr(closure, 0)
        as *const crate::closure::ClosureHeader;
    // #5485: restore the captured per-activation trap_next (see
    // async_step_fulfill_thunk for the full rationale).
    let captured_trap_next = crate::closure::js_closure_get_capture_ptr(closure, 1) as *mut Promise;
    let true_bits = f64::from_bits(0x7FFC_0000_0000_0004);
    // #691 Phase 2: see async_step_fulfill_thunk — same TLS-setup
    // requirement on the rejection path.
    let prev = INLINE_TRAP.with(|c| {
        let old = c.get();
        c.set(InlineTrap {
            trap_next: captured_trap_next,
            current_step: step as usize,
        });
        old
    });
    let result = crate::closure::js_closure_call2(step, value, true_bits);
    INLINE_TRAP.with(|c| c.set(prev));
    forward_swallowed_rejection(result, captured_trap_next);
    result
}

const AFA_RESULT_PROMISE: u32 = 0;
const AFA_RESULT_ARRAY: u32 = 1;
const AFA_SOURCE_ARRAY: u32 = 2;
const AFA_ITERATOR_VALUE: u32 = 3;
const AFA_REJECT_CLOSURE: u32 = 4;
const AFA_MAP_FN: u32 = 5;
const AFA_THIS_ARG: u32 = 6;
const AFA_INDEX: u32 = 7;
const AFA_VALUE_CLOSURE: u32 = 8;
const AFA_MAPPED_CLOSURE: u32 = 9;
const AFA_STATE_CAPTURES: u32 = 10;

#[inline]
fn undefined_value() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[inline]
fn boxed_promise(promise: *mut Promise) -> f64 {
    crate::value::js_nanbox_pointer(promise as i64)
}

fn string_header_to_owned(sp: *const crate::string::StringHeader) -> String {
    if sp.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*sp).byte_len as usize;
        let bytes = (sp as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(bytes, len)).into_owned()
    }
}

fn type_error_value(message: &str) -> f64 {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    f64::from_bits(crate::value::JSValue::pointer(err as *const u8).bits())
}

fn rejected_type_error_promise(message: &str) -> f64 {
    boxed_promise(js_promise_rejected(type_error_value(message)))
}

fn array_from_async_nullish_error(input: f64) -> Option<f64> {
    match input.to_bits() {
        crate::value::TAG_NULL => Some(rejected_type_error_promise(
            "Cannot read properties of null (reading 'Symbol(Symbol.asyncIterator)')",
        )),
        crate::value::TAG_UNDEFINED => Some(rejected_type_error_promise(
            "Cannot read properties of undefined (reading 'Symbol(Symbol.asyncIterator)')",
        )),
        _ => None,
    }
}

fn render_from_async_callback(value: f64) -> String {
    string_header_to_owned(crate::value::js_jsvalue_to_string(value))
}

fn resolve_from_async_map_fn(map_fn: f64) -> Result<Option<*const crate::ClosureHeader>, f64> {
    if map_fn.to_bits() == crate::value::TAG_UNDEFINED {
        return Ok(None);
    }
    let raw = crate::value::js_nanbox_get_pointer(map_fn) as usize;
    if raw >= 0x10000 && crate::closure::is_closure_ptr(raw) {
        return Ok(Some(raw as *const crate::ClosureHeader));
    }
    Err(type_error_value(&format!(
        "{} is not a function",
        render_from_async_callback(map_fn)
    )))
}

fn array_value_ptr(input: f64) -> Option<*const crate::array::ArrayHeader> {
    let raw = crate::value::js_nanbox_get_pointer(input) as usize;
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    unsafe {
        let gc = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        ((*gc).obj_type == crate::gc::GC_TYPE_ARRAY)
            .then_some(raw as *const crate::array::ArrayHeader)
    }
}

fn iterator_value_for_from_async(input: f64) -> Option<f64> {
    if let Some(iter) = crate::array::call_symbol_async_iterator_for_flat_map(input) {
        return Some(iter);
    }
    if crate::object::is_async_generator_instance_value(input) {
        return Some(input);
    }
    let iter = crate::symbol::js_get_iterator(input);
    let raw = crate::value::js_nanbox_get_pointer(iter) as usize;
    if iter.to_bits() != input.to_bits()
        || crate::array::is_builtin_iterator_class_id(raw)
        || crate::array::has_iterator_next(iter)
    {
        return Some(crate::array::async_from_sync_wrap_iterator(iter));
    }
    None
}

/// `Array.fromAsync(input, mapFn?, thisArg?)` — Node 22+ static method.
///
/// Returns a Promise that resolves to an Array. Source values are awaited
/// before optional mapping, and mapped results are awaited before appending.
/// Direct async-iterator objects keep the existing chained `.next()` path;
/// sync iterables and array-like inputs are first materialized with the same
/// conversion stack as `Array.from`.
#[no_mangle]
pub extern "C" fn js_array_from_async(input: f64, map_fn: f64, this_arg: f64) -> f64 {
    use crate::array::{js_array_alloc, ArrayHeader};
    use crate::closure::{
        js_closure_alloc, js_closure_set_capture_f64, js_closure_set_capture_ptr,
    };

    if let Some(rejected) = array_from_async_nullish_error(input) {
        return rejected;
    }
    let map_fn = match resolve_from_async_map_fn(map_fn) {
        Ok(Some(_)) => map_fn,
        Ok(None) => undefined_value(),
        Err(reason) => return boxed_promise(js_promise_rejected(reason)),
    };

    let iter_value = iterator_value_for_from_async(input);
    let source_arr = if iter_value.is_some() {
        std::ptr::null_mut()
    } else if let Some(arr) = array_value_ptr(input) {
        arr as *mut ArrayHeader
    } else {
        crate::array::js_array_clone(
            crate::value::js_nanbox_get_pointer(input) as *const ArrayHeader
        )
    };
    let iter_value = iter_value.unwrap_or_else(undefined_value);

    let result_promise = js_promise_new();
    let result_arr = js_array_alloc(0);
    unsafe {
        (*result_arr).length = 0;
    }

    let state = js_closure_alloc(array_from_async_step as *const u8, AFA_STATE_CAPTURES);
    let reject_closure = js_closure_alloc(array_from_async_reject as *const u8, 1);
    let value_closure = js_closure_alloc(array_from_async_value_step as *const u8, 1);
    let mapped_closure = js_closure_alloc(array_from_async_mapped_step as *const u8, 1);

    js_closure_set_capture_ptr(reject_closure, 0, result_promise as i64);
    js_closure_set_capture_ptr(value_closure, 0, state as i64);
    js_closure_set_capture_ptr(mapped_closure, 0, state as i64);

    js_closure_set_capture_ptr(state, AFA_RESULT_PROMISE, result_promise as i64);
    js_closure_set_capture_ptr(state, AFA_RESULT_ARRAY, result_arr as i64);
    js_closure_set_capture_ptr(state, AFA_SOURCE_ARRAY, source_arr as i64);
    js_closure_set_capture_f64(state, AFA_ITERATOR_VALUE, iter_value);
    js_closure_set_capture_ptr(state, AFA_REJECT_CLOSURE, reject_closure as i64);
    js_closure_set_capture_f64(state, AFA_MAP_FN, map_fn);
    js_closure_set_capture_f64(state, AFA_THIS_ARG, this_arg);
    js_closure_set_capture_f64(state, AFA_INDEX, 0.0);
    js_closure_set_capture_ptr(state, AFA_VALUE_CLOSURE, value_closure as i64);
    js_closure_set_capture_ptr(state, AFA_MAPPED_CLOSURE, mapped_closure as i64);

    if source_arr.is_null() {
        array_from_async_call_next(iter_value, state, reject_closure);
    } else {
        array_from_async_array_next(state);
    }

    boxed_promise(result_promise)
}

/// Helper that calls `iter.next()` (returning a Promise) and attaches
/// `chain_closure` as both fulfill and reject handlers. Used by the async
/// iterator path of `js_array_from_async`.
fn array_from_async_call_next(
    iter_value: f64,
    chain_closure: *const crate::closure::ClosureHeader,
    reject_closure: *const crate::closure::ClosureHeader,
) {
    let method_name = b"next";
    // Call iter.next() — returns a Promise<{value, done}> for async generators
    // or `{value, done}` directly for sync iterators.
    let next_result = unsafe {
        crate::object::js_native_call_method(
            iter_value,
            method_name.as_ptr() as *const i8,
            method_name.len(),
            std::ptr::null(),
            0,
        )
    };

    // If the result is a Promise pointer, attach the handler via .then.
    let next_ptr = crate::value::js_nanbox_get_pointer(next_result) as usize;
    if next_ptr != 0 {
        unsafe {
            let gc_header = (next_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_PROMISE {
                let next_promise = next_ptr as *mut Promise;
                js_promise_then(next_promise, chain_closure, reject_closure);
                return;
            }
        }
    }
    // Synchronous iterator path: invoke the handler directly with the
    // result so the iteration loop continues without going through .then.
    array_from_async_step(chain_closure as *const _, next_result);
}

extern "C" fn array_from_async_reject(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    let result_promise = crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    if !result_promise.is_null() {
        js_promise_reject(result_promise, reason);
    }
    0.0
}

/// `.then(...)` handler invoked once per `.next()` resolution. Reads the
/// `{value, done}` iter-result, pushes `value` into the accumulator array,
/// and either resolves the output Promise (when `done`) or schedules
/// another `.next()` call.
extern "C" fn array_from_async_step(
    closure: *const crate::closure::ClosureHeader,
    iter_result: f64,
) -> f64 {
    use crate::closure::js_closure_get_capture_ptr;

    let result_promise = js_closure_get_capture_ptr(closure, AFA_RESULT_PROMISE) as *mut Promise;
    let result_arr =
        js_closure_get_capture_ptr(closure, AFA_RESULT_ARRAY) as *mut crate::array::ArrayHeader;
    let iter_value = crate::closure::js_closure_get_capture_f64(closure, AFA_ITERATOR_VALUE);

    if result_promise.is_null() || result_arr.is_null() {
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

    array_from_async_await_value(closure, value_f64);

    let _ = iter_value;

    0.0
}

fn array_from_async_array_next(closure: *const crate::closure::ClosureHeader) {
    let source_arr = crate::closure::js_closure_get_capture_ptr(closure, AFA_SOURCE_ARRAY)
        as *const crate::array::ArrayHeader;
    let result_promise =
        crate::closure::js_closure_get_capture_ptr(closure, AFA_RESULT_PROMISE) as *mut Promise;
    let result_arr = crate::closure::js_closure_get_capture_ptr(closure, AFA_RESULT_ARRAY)
        as *mut crate::array::ArrayHeader;
    if source_arr.is_null() || result_promise.is_null() || result_arr.is_null() {
        return;
    }
    let index = crate::closure::js_closure_get_capture_f64(closure, AFA_INDEX) as usize;
    let length = unsafe { (*source_arr).length as usize };
    if index >= length {
        js_promise_resolve(
            result_promise,
            crate::value::js_nanbox_pointer(result_arr as i64),
        );
        return;
    }
    let value = crate::array::js_array_get_f64(source_arr, index as u32);
    array_from_async_await_value(closure, value);
}

fn array_from_async_await_value(closure: *const crate::closure::ClosureHeader, value: f64) {
    let value_closure = crate::closure::js_closure_get_capture_ptr(closure, AFA_VALUE_CLOSURE)
        as *const crate::closure::ClosureHeader;
    let reject = crate::closure::js_closure_get_capture_ptr(closure, AFA_REJECT_CLOSURE)
        as *const crate::closure::ClosureHeader;
    let promise = js_promise_resolved(value);
    js_promise_then(promise, value_closure, reject);
}

extern "C" fn array_from_async_value_step(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let state = crate::closure::js_closure_get_capture_ptr(closure, 0)
        as *const crate::closure::ClosureHeader;
    if !state.is_null() {
        array_from_async_map_or_push(state, value);
    }
    0.0
}

fn array_from_async_map_or_push(closure: *const crate::closure::ClosureHeader, value: f64) {
    let map_fn = crate::closure::js_closure_get_capture_f64(closure, AFA_MAP_FN);
    if map_fn.to_bits() == crate::value::TAG_UNDEFINED {
        array_from_async_push_and_continue(closure, value);
        return;
    }

    let index = crate::closure::js_closure_get_capture_f64(closure, AFA_INDEX);
    let this_arg = crate::closure::js_closure_get_capture_f64(closure, AFA_THIS_ARG);
    let args = [value, index];
    let args_arr = crate::array::js_array_from_f64(args.as_ptr(), args.len() as u32);
    let prev_this = crate::object::js_implicit_this_set(this_arg);
    let mapped_promise = js_promise_try(map_fn, args_arr);
    crate::object::js_implicit_this_set(prev_this);

    let mapped_closure = crate::closure::js_closure_get_capture_ptr(closure, AFA_MAPPED_CLOSURE)
        as *const crate::closure::ClosureHeader;
    let reject = crate::closure::js_closure_get_capture_ptr(closure, AFA_REJECT_CLOSURE)
        as *const crate::closure::ClosureHeader;
    js_promise_then(mapped_promise, mapped_closure, reject);
}

extern "C" fn array_from_async_mapped_step(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let state = crate::closure::js_closure_get_capture_ptr(closure, 0)
        as *const crate::closure::ClosureHeader;
    if !state.is_null() {
        array_from_async_push_and_continue(state, value);
    }
    0.0
}

fn array_from_async_push_and_continue(closure: *const crate::closure::ClosureHeader, value: f64) {
    let mut result_arr = crate::closure::js_closure_get_capture_ptr(closure, AFA_RESULT_ARRAY)
        as *mut crate::array::ArrayHeader;
    if result_arr.is_null() {
        return;
    }
    result_arr = crate::array::js_array_push_f64(result_arr, value);
    crate::closure::js_closure_set_capture_ptr(
        closure as *mut _,
        AFA_RESULT_ARRAY,
        result_arr as i64,
    );

    let next_index = crate::closure::js_closure_get_capture_f64(closure, AFA_INDEX) + 1.0;
    crate::closure::js_closure_set_capture_f64(closure as *mut _, AFA_INDEX, next_index);

    let source_arr = crate::closure::js_closure_get_capture_ptr(closure, AFA_SOURCE_ARRAY)
        as *const crate::array::ArrayHeader;
    if source_arr.is_null() {
        let iter_value = crate::closure::js_closure_get_capture_f64(closure, AFA_ITERATOR_VALUE);
        let reject = crate::closure::js_closure_get_capture_ptr(closure, AFA_REJECT_CLOSURE)
            as *const crate::closure::ClosureHeader;
        array_from_async_call_next(iter_value, closure, reject);
    } else {
        array_from_async_array_next(closure);
    }
}

/// Helper to allocate a static StringHeader for property-name lookups.
/// Reuses `js_string_from_bytes` so the result is GC-tracked.
fn make_static_string(bytes: &[u8]) -> *const crate::string::StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}
