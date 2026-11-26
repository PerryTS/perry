//! Async-step driver: `js_promise_resolved[_then]`, `js_async_step_chain`,
//! `js_async_step_done`, the async-step thunk cache, and
//! `Array.fromAsync`. These are the fast paths the async-to-generator
//! transform calls per `await`.

use super::*;

// ── PERRY_TRACE_ASYNC: lost-continuation diagnostics ──────────────────────
// Opt-in (`PERRY_TRACE_ASYNC=1`). Logs each `await`-SUSPEND on a *pending*
// promise (with a backtrace of the await site) and each SETTLE of a
// suspended-on promise. A SUSPEND with no matching SETTLE is a LOST
// continuation: the awaited promise never settled, so the async fn never
// resumes — the "the async request handler never returns / the response
// evaporates" signature. Find it by diffing the unmatched-await backtraces of
// a working run against a hanging one; the SUSPEND line carries the site.
// A no-op (one cached bool check) when the env var is unset.
crate::perry_thread_local! {
    static TRACE_ASYNC_ON: std::cell::Cell<i8> = const { std::cell::Cell::new(-1) };
    static TRACE_ASYNC_SEQ: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static TRACE_ASYNC_AWAITED: std::cell::RefCell<std::collections::HashSet<usize>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

#[inline]
fn trace_async_enabled() -> bool {
    TRACE_ASYNC_ON.with(|c| {
        let v = c.get();
        if v >= 0 {
            v == 1
        } else {
            let e = std::env::var("PERRY_TRACE_ASYNC").as_deref() == Ok("1");
            c.set(if e { 1 } else { 0 });
            e
        }
    })
}

/// An async fn is suspending, awaiting `inner` (a pending promise). Record it
/// and log the await site so a never-settled `inner` can be identified.
pub(crate) fn trace_async_suspend(inner: *const Promise) {
    if inner.is_null() || !trace_async_enabled() {
        return;
    }
    let id = TRACE_ASYNC_SEQ.with(|c| {
        let n = c.get().wrapping_add(1);
        c.set(n);
        n
    });
    TRACE_ASYNC_AWAITED.with(|s| s.borrow_mut().insert(inner as usize));
    eprintln!(
        "[ASYNC-SUSP #{id} P={inner:p}]\n{}",
        std::backtrace::Backtrace::force_capture()
    );
}

/// A promise settled. If an async fn was suspended awaiting it, note the resume.
pub(crate) fn trace_async_settle(promise: *const Promise, how: &str) {
    if promise.is_null() || !trace_async_enabled() {
        return;
    }
    let was = TRACE_ASYNC_AWAITED.with(|s| s.borrow_mut().remove(&(promise as usize)));
    if was {
        eprintln!("[ASYNC-SETTLE {how} P={promise:p}]");
    }
}

/// Inverse of `then::arg_to_closure` — reboxes an already-unboxed
/// `ClosurePtr` handler arg back into a boxed `f64` JS value so it can be
/// forwarded to `js_promise_then_checked`. Null (the "no handler" sentinel)
/// becomes `undefined`, matching what `IsCallable(x) is false` resolves to
/// on the `_checked` entry's own tag check.
/// #7497: NaN-box a possibly-null promise / closure pointer so it can be parked
/// in a `RuntimeHandleScope`, and read it back. NaN-boxed rather than
/// `root_raw_*_ptr` because reading a raw handle back needs `get_raw_*_ptr`,
/// which `scripts/raw_handle_debt.py` counts.
#[inline]
fn boxed_promise_or_undef(p: *mut Promise) -> f64 {
    if p.is_null() {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        crate::value::js_nanbox_pointer(p as i64)
    }
}

#[inline]
fn boxed_closure_or_undef(c: ClosurePtr) -> f64 {
    if c.is_null() {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        crate::value::js_nanbox_pointer(c as i64)
    }
}

#[inline]
fn unboxed_promise(h: &crate::gc::RuntimeHandle<'_>) -> *mut Promise {
    let v = h.get_nanbox_f64();
    if v.to_bits() == crate::value::TAG_UNDEFINED {
        std::ptr::null_mut()
    } else {
        crate::value::js_nanbox_get_pointer(v) as *mut Promise
    }
}

#[inline]
fn unboxed_closure(h: &crate::gc::RuntimeHandle<'_>) -> ClosurePtr {
    let v = h.get_nanbox_f64();
    if v.to_bits() == crate::value::TAG_UNDEFINED {
        std::ptr::null()
    } else {
        crate::value::js_nanbox_get_pointer(v) as ClosurePtr
    }
}

/// #7497 (CodeRabbit): shared, rooted tail for `js_async_step_chain`'s three
/// suspend paths. Each of them crosses `build_async_step_thunks` (two closure
/// allocations) and, on two of them, `js_promise_resolved` as well, while
/// holding the awaited promise, the two fresh thunks and `trap_next` — and
/// `then_backpatch_result` both STORES into the thunks and RETURNS `trap_next`.
/// `awaited_in` is the already-known awaited promise (the pending-inner path);
/// pass null to have `make_awaited` produce it after the thunks exist.
fn suspend_on_awaited(
    step: ClosurePtr,
    trap_next: *mut Promise,
    box_activation: *mut crate::r#box::AsyncBoxActivation,
    awaited_in: *mut Promise,
    make_awaited: impl FnOnce() -> *mut Promise,
) -> *mut Promise {
    let scope = crate::gc::RuntimeHandleScope::new();
    let step_h = scope.root_nanbox_f64(boxed_closure_or_undef(step));
    let trap_h = scope.root_nanbox_f64(boxed_promise_or_undef(trap_next));
    let awaited_in_h = scope.root_nanbox_f64(boxed_promise_or_undef(awaited_in));
    let (fulfill, reject) = build_async_step_thunks(
        unboxed_closure(&step_h),
        unboxed_promise(&trap_h),
        box_activation,
    );
    let fulfill_h = scope.root_nanbox_f64(boxed_closure_or_undef(fulfill));
    let reject_h = scope.root_nanbox_f64(boxed_closure_or_undef(reject));
    let awaited = if unboxed_promise(&awaited_in_h).is_null() {
        make_awaited()
    } else {
        unboxed_promise(&awaited_in_h)
    };
    let awaited_h = scope.root_nanbox_f64(boxed_promise_or_undef(awaited));
    then_backpatch_result(
        unboxed_promise(&awaited_h),
        unboxed_closure(&fulfill_h),
        unboxed_closure(&reject_h),
        unboxed_promise(&trap_h),
    )
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
        // A `settled` hook can collect, so the promise is reread after it.
        if crate::v8::promise_hooks_active() || crate::async_hook_slots::promise_hooks_active() {
            let scope = crate::gc::RuntimeHandleScope::new();
            let promise = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(promise as i64));
            js_promise_resolve(
                crate::value::js_nanbox_get_pointer(promise.get_nanbox_f64()) as *mut Promise,
                value,
            );
            return crate::value::js_nanbox_get_pointer(promise.get_nanbox_f64()) as *mut Promise;
        }
        unsafe {
            (*promise).state = PromiseState::Fulfilled;
            (*promise).value = value;
        }
        return promise;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_h = scope.root_nanbox_f64(value);

    // Issue #2823: `Promise.resolve(p)` MUST return `p` itself when `p` is
    // already a native Promise (constructor === Promise). The spec defines
    // Promise.resolve to short-circuit and return the argument unchanged in
    // that case — object identity and pending-promise sharing are
    // observable (`Promise.resolve(p) === p`). Perry only constructs native
    // `Promise` instances, so a GC_TYPE_PROMISE value always satisfies the
    // "constructor is Promise" check. Return the existing pointer directly
    // instead of allocating a fresh wrapper and chaining to it.
    if js_value_is_promise(value_h.get_nanbox_f64()) != 0 {
        let inner = crate::value::js_nanbox_get_pointer(value_h.get_nanbox_f64()) as *mut Promise;
        if !inner.is_null() {
            // PromiseResolve(%Promise%, x) performs Get(x, "constructor")
            // before its identity short-cut, and only that Get decides: x is
            // reused exactly when the answer is %Promise%. An own accessor can
            // throw, and an own non-intrinsic constructor prevents returning x
            // itself. A recorded prototype answers through its chain (the
            // generic getter per hop, accessors with x as receiver): a user
            // prototype below Promise.prototype answers %Promise%, so x is
            // reused and a `then` there is never called; a `class X extends
            // Promise` answers X and takes the thenable path. A `then` on the
            // chain plays no part.
            let addr = inner as usize;
            let same_intrinsic_constructor =
                if crate::object::prototype_chain::object_static_prototype(addr).is_some()
                    && !super::promise_has_own_constructor(addr)
                {
                    let key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
                    // Allocating the key can move x: re-read it from its root.
                    let addr =
                        crate::value::js_nanbox_get_pointer(value_h.get_nanbox_f64()) as usize;
                    let ctor = if key.is_null() {
                        None
                    } else {
                        crate::object::prototype_chain::resolve_inherited_field(addr, key)
                    };
                    let intrinsic = crate::object::builtin_constructor_value("Promise");
                    ctor.is_some_and(|c| c.bits() == intrinsic.to_bits())
                } else if super::promise_has_own_constructor(addr) {
                    let ctor = unsafe {
                        crate::object::exotic_expando::exotic_get_own_property(
                            addr,
                            crate::object::exotic_expando::ExoticKind::Promise,
                            "constructor",
                            value_h.get_nanbox_f64(),
                        )
                    }
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                    let ctor_h = scope.root_nanbox_f64(ctor);
                    let intrinsic = crate::object::builtin_constructor_value("Promise");
                    ctor_h.get_nanbox_f64().to_bits() == intrinsic.to_bits()
                } else {
                    true
                };
            if same_intrinsic_constructor {
                return crate::value::js_nanbox_get_pointer(value_h.get_nanbox_f64())
                    as *mut Promise;
            }
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
    // Assimilating reads `then` (a getter can run) and enqueues a job: either
    // can collect, so the promise is reread after it.
    let promise = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(promise as i64));
    super::assimilate::promise_resolve_assimilating(
        crate::value::js_nanbox_get_pointer(promise.get_nanbox_f64()) as *mut Promise,
        value_h.get_nanbox_f64(),
    );
    crate::value::js_nanbox_get_pointer(promise.get_nanbox_f64()) as *mut Promise
}

/// Run the spec PromiseResolve path behind an exception boundary.  Async
/// iterator/generator algorithms turn an abrupt constructor getter into a
/// rejected result promise instead of throwing synchronously to their caller.
pub fn js_promise_resolved_catching(value: f64) -> Result<*mut Promise, f64> {
    crate::exception::catch_js_throw(|| js_promise_resolved(value))
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
    // Every path allocates before it hands the handlers and `value` on
    // (`js_promise_new`, `capture_context`, the `then` key the inherited-then
    // probe interns, `js_promise_resolved`, which can also run a user
    // `constructor` or `then` getter), and a copying minor there moves them:
    // each path reads them back from these handles after its last such call.
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_h = scope.root_nanbox_f64(value);
    let fulfilled_h = scope.root_nanbox_f64(boxed_closure_or_undef(on_fulfilled));
    let rejected_h = scope.root_nanbox_f64(boxed_closure_or_undef(on_rejected));

    // The primitive fast path below bypasses `js_promise_new`/`then`, so it
    // would never fire `v8.promiseHooks` (#3139). When hooks are active, route
    // through the real resolve+then path instead.
    if crate::v8::promise_hooks_active() || crate::async_hook_slots::promise_hooks_active() {
        bump(&MT_FAST_PATH_MISS);
        let p1 = js_promise_resolved(value);
        return js_promise_then(
            p1,
            unboxed_closure(&fulfilled_h),
            unboxed_closure(&rejected_h),
        );
    }

    if is_definitely_primitive(value) {
        bump(&MT_FAST_PATH_HIT);
        // FAST PATH (primitive) — skip Promise.resolve()'s allocation
        // entirely. The callback runs once during the next microtask
        // drain via the `Task::Inline` arm.
        let next_h = scope.root_nanbox_f64(boxed_promise_or_undef(js_promise_new()));
        let context = capture_context();
        TASK_QUEUE.with(|q| {
            q.borrow_mut().enqueue_promise_job(Task::Inline(
                unboxed_closure(&fulfilled_h),
                value,
                unboxed_promise(&next_h),
                true,
                context,
            ));
        });
        crate::event_pump::js_notify_promise_progress();
        // No rejection handler is attached: it would only matter if `value`
        // were a Promise, which it isn't here.
        return unboxed_promise(&next_h);
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
                || super::then::promise_inherited_user_then(inner_addr).is_some()
            {
                bump(&MT_FAST_PATH_MISS);
                // `Promise.resolve(inner)` is `inner` only when its
                // `constructor` is %Promise%; otherwise it is a new promise
                // whose PromiseResolveThenableJob calls `inner.then`, so an
                // abrupt `then` (a non-object `constructor` failing
                // SpeciesConstructor) rejects it instead of throwing here
                // (#13556, test262 Promise/resolve/arg-uniq-ctor).
                let resolved = js_promise_resolved(value_h.get_nanbox_f64());
                let promise_val = box_promise_ptr(resolved);
                let result = super::js_promise_then_checked(
                    promise_val,
                    fulfilled_h.get_nanbox_f64(),
                    rejected_h.get_nanbox_f64(),
                );
                return crate::value::js_nanbox_get_pointer(result) as *mut Promise;
            }
            bump(&MT_FAST_PATH_HIT);
            return js_promise_then(
                crate::value::js_nanbox_get_pointer(value_h.get_nanbox_f64()) as *mut Promise,
                unboxed_closure(&fulfilled_h),
                unboxed_closure(&rejected_h),
            );
        }
    }

    bump(&MT_FAST_PATH_MISS);
    // Pointer-tagged but not a Promise — could be a thenable. Take
    // the unfused path so the assimilation probes can run on it.
    let p1 = js_promise_resolved(value_h.get_nanbox_f64());
    js_promise_then(
        p1,
        unboxed_closure(&fulfilled_h),
        unboxed_closure(&rejected_h),
    )
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
        // #7497 (CodeRabbit): `js_promise_new_with_parent` allocates, and the two
        // thunks it is about to be STORED into — plus the promise the handlers
        // are attached to — are live across it. This is the publishing shape:
        // without the re-read, a copying minor here writes a from-space `result`
        // into two closures the collector goes on maintaining.
        let scope = crate::gc::RuntimeHandleScope::new();
        let awaited_h = scope.root_nanbox_f64(boxed_promise_or_undef(awaited));
        let fulfill_h = scope.root_nanbox_f64(boxed_closure_or_undef(fulfill));
        let reject_h = scope.root_nanbox_f64(boxed_closure_or_undef(reject));
        let result_h = scope.root_nanbox_f64(boxed_promise_or_undef(
            super::then::js_promise_new_with_parent(unboxed_promise(&awaited_h)),
        ));
        let result = unboxed_promise(&result_h);
        crate::closure::closure_set_capture_ref(
            unboxed_closure(&fulfill_h) as *mut crate::closure::ClosureHeader,
            1,
            result as i64,
        );
        crate::closure::closure_set_capture_ref(
            unboxed_closure(&reject_h) as *mut crate::closure::ClosureHeader,
            1,
            result as i64,
        );
        super::then::js_promise_attach_handlers(
            unboxed_promise(&awaited_h),
            unboxed_closure(&fulfill_h),
            unboxed_closure(&reject_h),
        );
        unboxed_promise(&result_h)
    } else {
        // #6728: `trap_next` non-null means this is a SUBSEQUENT `await` (the
        // 2nd or later) in the activation — the result promise already exists
        // and the resume thunks already carry it (capture slot 1, set by
        // `build_async_step_thunks`). Attach the thunks to `awaited` WITHOUT a
        // chained then-result promise, exactly like the first-await
        // (`trap_next.is_null()`) branch above, and return `trap_next` (which is
        // what the reuse fast paths in `js_async_step_chain` return too).
        //
        // A plain `js_promise_then` here mints an intermediate then-result
        // promise. When the resumed step later THROWS — a `throw` reached after
        // two-or-more awaits — its fulfill thunk returns the step body's fresh
        // `Promise.reject(e)` (see `forward_swallowed_rejection`), and that
        // then-result promise ADOPTS the rejection, becoming a REJECTED promise
        // with NO handler attached. The orphan surfaces a spurious
        // "Uncaught (in promise)" (and exits the process non-zero) even though a
        // surrounding `try/catch` already handled the throw via `trap_next`.
        // The first-await path never hit this because it uses `attach_handlers`
        // (`next` = null, nothing to propagate the thunk's rejected return to);
        // extend the same shape to every await so multi-await async functions
        // that throw reject exactly one promise — the activation result.
        super::then::js_promise_attach_handlers(awaited, fulfill, reject);
        trap_next
    }
}

/// An awaited promise goes through PromiseResolve(%Promise%, p), which performs
/// Get(p, "constructor") before reusing p. A promise carrying an own constructor
/// (data or accessor) must therefore take the general path that reads it. A null
/// metadata owner proves there is no own property, so the steady state costs one load.
#[inline]
fn promise_reads_own_constructor(value: f64) -> bool {
    let inner = crate::value::js_nanbox_get_pointer(value) as *const Promise;
    if inner.is_null() || unsafe { crate::object::cell_meta::promise_cell_meta(inner).is_null() } {
        return false;
    }
    super::promise_has_own_constructor(inner as usize)
}

#[no_mangle]
pub extern "C" fn js_async_step_chain(value: f64, step_closure: ClosurePtr) -> *mut Promise {
    if let Some(next) = super::settled_step::try_enqueue(value, step_closure) {
        return next;
    }
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
    // #7497: `step_closure` is a GC-heap closure that this function stores into
    // a `Task::AsyncStep` at the end, after `adapt_foreign_promise_value`,
    // `js_promise_new`, `build_async_step_thunks` and `js_promise_resolved` have
    // all had a chance to collect. Pre-fix it rode through all of them in a
    // register, so the ENQUEUED task could carry a pre-collection address — and
    // the microtask runner, which correctly re-reads everything it pops, then
    // faithfully dispatched that dead pointer. `PERRY_GC_PROTECT_FROMSPACE=1`
    // reports it at `call_async_step_direct`, one frame away from the producer.
    let step_scope = crate::gc::RuntimeHandleScope::new();
    let step_handle = step_scope.root_nanbox_f64(if step_closure.is_null() {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        crate::value::js_nanbox_pointer(step_closure as i64)
    });
    let rooted_step = || -> ClosurePtr {
        let v = step_handle.get_nanbox_f64();
        if v.to_bits() == crate::value::TAG_UNDEFINED {
            std::ptr::null()
        } else {
            crate::value::js_nanbox_get_pointer(v) as ClosurePtr
        }
    };
    let value = adapt_foreign_promise_value(value);
    let step_closure = rooted_step();

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

    let async_hooks_active = crate::async_hook_slots::promise_hooks_active();
    let (next, queued_value, is_error, direct_trigger_async_id) = if is_definitely_primitive(value)
    {
        // Primitive value: enqueue Task::AsyncStep directly.
        bump(&MT_FAST_PATH_HIT);
        // The optimized path skips the `PromiseResolve(%Promise%, value)`
        // Promise that Node creates for `await value`. That Promise is still
        // observable through async_hooks, and the continuation Promise must
        // name it as its trigger. Materialize it only while Promise hooks are
        // active; the normal no-hook fast path remains allocation-free.
        let awaited = if async_hooks_active {
            js_promise_resolved(value)
        } else {
            std::ptr::null_mut()
        };
        let awaited_async_id = if awaited.is_null() {
            0
        } else {
            unsafe { (*awaited).async_id }
        };
        (
            if can_reuse {
                bump(&MT_STEP_CHAIN_REUSE_HIT);
                trap_next
            } else if !awaited.is_null() {
                bump(&MT_STEP_CHAIN_REUSE_MISS);
                super::then::js_promise_new_with_parent(awaited)
            } else {
                bump(&MT_STEP_CHAIN_REUSE_MISS);
                js_promise_new()
            },
            value,
            false,
            awaited_async_id,
        )
    } else if js_value_is_promise(value) != 0
        && super::then::promise_inherited_user_then(
            (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize,
        )
        .is_none()
        && !promise_reads_own_constructor(value)
    {
        let inner = crate::value::js_nanbox_get_pointer(value) as *mut Promise;
        if !inner.is_null() {
            let inner_state = unsafe { (*inner).state };
            let inner_async_id = unsafe { (*inner).async_id };
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
                        } else if async_hooks_active {
                            bump(&MT_STEP_CHAIN_REUSE_MISS);
                            super::then::js_promise_new_with_parent(inner)
                        } else {
                            bump(&MT_STEP_CHAIN_REUSE_MISS);
                            js_promise_new()
                        },
                        unwrapped,
                        false,
                        inner_async_id,
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
                        } else if async_hooks_active {
                            bump(&MT_STEP_CHAIN_REUSE_MISS);
                            super::then::js_promise_new_with_parent(inner)
                        } else {
                            bump(&MT_STEP_CHAIN_REUSE_MISS);
                            js_promise_new()
                        },
                        reason,
                        true,
                        inner_async_id,
                    )
                }
                PromiseState::Pending => {
                    // Inner still pending — fall back to the thunk
                    // path. We can't enqueue a Task::AsyncStep until
                    // the inner settles; install fulfill/reject thunks
                    // that will queue the right Task when called.
                    bump(&MT_STEP_CHAIN_REUSE_MISS);
                    trace_async_suspend(inner);
                    return suspend_on_awaited(
                        rooted_step(),
                        trap_next,
                        trap.box_activation,
                        inner,
                        || std::ptr::null_mut(),
                    );
                }
            }
        } else {
            bump(&MT_STEP_CHAIN_REUSE_MISS);
            return suspend_on_awaited(
                rooted_step(),
                trap_next,
                trap.box_activation,
                std::ptr::null_mut(),
                || js_promise_resolved(value),
            );
        }
    } else {
        // Pointer-tagged but not a Promise (thenable etc.). Take the
        // fully-general path so assimilation runs.
        bump(&MT_STEP_CHAIN_REUSE_MISS);
        return suspend_on_awaited(
            rooted_step(),
            trap_next,
            trap.box_activation,
            std::ptr::null_mut(),
            || js_promise_resolved(value),
        );
    };

    // Native snapshot/queue allocations cannot collect. Keep the ordinary
    // collecting path below for async_hooks resource initialization.
    if !async_hooks_active {
        return super::settled_step::enqueue(
            rooted_step(),
            queued_value,
            next,
            is_error,
            trap.box_activation,
        );
    }

    // #7497: `capture_context()` allocates, and `next` / `queued_value` are the
    // OTHER two GC values this task carries, so root them across it too and take
    // every address from a handle at the push.
    let next_handle = step_scope.root_nanbox_f64(if next.is_null() {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        crate::value::js_nanbox_pointer(next as i64)
    });
    let queued_value_handle = step_scope.root_nanbox_f64(queued_value);
    let context = capture_context();
    let next = {
        let v = next_handle.get_nanbox_f64();
        if v.to_bits() == crate::value::TAG_UNDEFINED {
            std::ptr::null_mut()
        } else {
            crate::value::js_nanbox_get_pointer(v) as *mut Promise
        }
    };
    let (step_async_id, step_trigger_id) = if next.is_null() {
        (0, 0)
    } else if can_reuse && async_hooks_active {
        let resource = crate::value::js_nanbox_pointer(next as i64);
        let trigger_async_id = if direct_trigger_async_id != 0 {
            direct_trigger_async_id
        } else {
            crate::async_hook_slots::execution_async_id_u64()
        };
        let ids = crate::async_hook_slots::init_resource_with_trigger(
            "PROMISE",
            resource,
            false,
            trigger_async_id,
        );
        (ids.async_id, ids.trigger_async_id)
    } else {
        unsafe { ((*next).async_id, (*next).trigger_async_id) }
    };
    let next = {
        let value = next_handle.get_nanbox_f64();
        if value.to_bits() == crate::value::TAG_UNDEFINED {
            std::ptr::null_mut()
        } else {
            crate::value::js_nanbox_get_pointer(value) as *mut Promise
        }
    };
    super::settled_step::publish(
        rooted_step(),
        queued_value_handle.get_nanbox_f64(),
        next,
        is_error,
        context,
        trap.box_activation,
        step_async_id,
        step_trigger_id,
    );
    crate::event_pump::js_notify_promise_progress();
    // A registered native host wake may re-enter before returning.
    unboxed_promise(&next_handle)
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
/// Codegen entry for the return value of an await-less async fn (the
/// transform leaves those un-converted; `Stmt::Return` wraps the value
/// directly). Differs from `js_promise_resolved` in one spec-visible way: an
/// async fn always returns a FRESH promise, and `return <promise>` ADOPTS the
/// inner via the two-tick job (V8 hop parity; hops.js
/// `asyncfn-return-resolved-promise` = t0 t1 t2 X in Node) — the #2823
/// `Promise.resolve(p) === p` identity does not apply to async returns.
#[no_mangle]
pub extern "C" fn js_async_fn_result(value: f64) -> *mut Promise {
    let value = adapt_foreign_promise_value(value);
    if js_value_is_promise(value) != 0 {
        let inner = crate::value::js_nanbox_get_pointer(value) as *mut Promise;
        if !inner.is_null() {
            let fresh = js_promise_new();
            super::assimilate::enqueue_native_adoption_job(fresh, inner);
            return fresh;
        }
    }
    js_promise_resolved(value)
}

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
    crate::r#box::finish_async_box_activation(trap.box_activation);
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
        // #7497: `resolve_trap_next_with_adoption` settles the promise, which
        // can enqueue jobs and allocate — and the RETURN VALUE of this function
        // is that same promise. Returning the pre-call copy hands the async
        // state machine a from-space `GC_TYPE_PROMISE`;
        // `PERRY_GC_PROTECT_FROMSPACE=1` faults on it here under the
        // auto-optimize link.
        let scope = crate::gc::RuntimeHandleScope::new();
        let target_h = scope.root_nanbox_f64(boxed_promise_or_undef(trap.trap_next));
        let value_h = scope.root_nanbox_f64(value);
        // Pass the RE-READ address, not `trap.trap_next`: leaving the pre-root
        // copy nameable is exactly the shape this whole PR is about, and the
        // rooting above is not a licence to keep using the old binding.
        resolve_trap_next_with_adoption(unboxed_promise(&target_h), value_h.get_nanbox_f64());
        unboxed_promise(&target_h)
    } else {
        bump(&MT_STEP_DONE_REUSE_MISS);
        // An async fn always returns a FRESH promise — `js_promise_resolved`'s
        // #2823 identity short-circuit (Promise.resolve(p) === p) must not
        // apply to `return <promise>` from an async fn, which instead adopts
        // the inner promise via the two-tick job (V8 hop parity; hops.js
        // `asyncfn-return-resolved-promise` = t0 t1 t2 X in Node).
        if js_value_is_promise(value) != 0 {
            let inner = crate::value::js_nanbox_get_pointer(value) as *mut Promise;
            if !inner.is_null() {
                // #7497: `js_promise_new` allocates and `inner` is its adoption
                // source, so root `inner` across it and re-read; likewise the
                // fresh promise across `enqueue_native_adoption_job`.
                let scope = crate::gc::RuntimeHandleScope::new();
                let inner_h = scope.root_nanbox_f64(value);
                let fresh_h =
                    scope.root_nanbox_f64(crate::value::js_nanbox_pointer(js_promise_new() as i64));
                super::assimilate::enqueue_native_adoption_job(
                    crate::value::js_nanbox_get_pointer(fresh_h.get_nanbox_f64()) as *mut Promise,
                    crate::value::js_nanbox_get_pointer(inner_h.get_nanbox_f64()) as *mut Promise,
                );
                return crate::value::js_nanbox_get_pointer(fresh_h.get_nanbox_f64())
                    as *mut Promise;
            }
        }
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
    // Native Promise: adopt via the native job — `return <promise>` from an
    // async fn is observable two ticks later in V8 (hop parity; see
    // `enqueue_native_adoption_job`).
    if js_value_is_promise(value) != 0 {
        let inner = crate::value::js_nanbox_get_pointer(value) as *mut Promise;
        if !inner.is_null() && inner != target {
            super::assimilate::enqueue_native_adoption_job(target, inner);
            return;
        }
    }
    // User thenable (e.g. Drizzle QueryPromise, #586): assimilate, then
    // chain the wrapper promise into `target`.
    //
    // #7497: `js_assimilate_thenable` runs the user `then` getter and allocates,
    // and `target` is the RECEIVER of both settlements below. Root it (and the
    // value) across the call and re-read.
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_h = scope.root_nanbox_f64(boxed_promise_or_undef(target));
    let value_h = scope.root_nanbox_f64(value);
    let assim = js_assimilate_thenable(value_h.get_nanbox_f64());
    let target = unboxed_promise(&target_h);
    let value = value_h.get_nanbox_f64();
    if assim.to_bits() != value.to_bits() && js_value_is_promise(assim) != 0 {
        let inner = crate::value::js_nanbox_get_pointer(assim) as *mut Promise;
        if !inner.is_null() && inner != target {
            super::assimilate::enqueue_native_adoption_job(target, inner);
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
    call_async_step_body(
        ptr,
        f64::from_bits(0x7FFC_0000_0000_0001), // TAG_UNDEFINED
        f64::from_bits(0x7FFC_0000_0000_0003), // TAG_FALSE
        std::ptr::null_mut(),
        crate::r#box::new_async_box_activation(),
    )
}

/// #6709. Entry point for an async-generator activation. Like
/// `js_async_first_call`, but the caller supplies the resume `value` and an
/// `is_error` flag instead of the hard-coded `(undefined, false)`.
///
/// Each `gen.next(v)` / `gen.throw(e)` / `.return(v)` on a lowered async
/// generator starts a fresh async-step activation that resumes the SHARED
/// state machine (the generator's `__state`/`__done`/`__sent` boxes persist
/// across calls). The activation's step body reads its resumed value from the
/// first arg (delivered to `__sent`) and routes an error (`is_error = true`) at
/// the current suspend state; it returns either an already-settled Promise
/// (synchronous yield/completion), a fresh pending Promise (the body suspended
/// at an inner `await` via `js_async_step_chain`), or a rejected Promise (an
/// uncaught throw). The `trap_next = null` reset mirrors `js_async_first_call`
/// so a nested async fn/gen the body invokes cannot prematurely settle THIS
/// activation's result promise; `prev` is restored on exit for composition.
#[no_mangle]
pub extern "C" fn js_async_generator_resume(
    step_closure_nanbox: f64,
    value: f64,
    is_error: f64,
) -> f64 {
    let ptr = crate::value::js_nanbox_get_pointer(step_closure_nanbox)
        as *mut crate::closure::ClosureHeader;
    call_async_step_body(
        ptr,
        value,
        is_error,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    )
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ASYNC_GENERATOR_RESUME: extern "C" fn(f64, f64, f64) -> f64 =
    js_async_generator_resume;

// Thread-local single-slot cache for async-step thunks. Keyed by the
// step closure pointer. When the same step closure is used across
// multiple promise-of-promise awaits (the simple-probe shape), we
// return the cached thunks; otherwise we allocate. The thunks are
// GC-rooted via `ASYNC_STEP_THUNK_CACHE_SCANNER` so they survive
// collection until evicted by a different step closure.
crate::perry_thread_local! {
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
    box_activation: *mut crate::r#box::AsyncBoxActivation,
) -> (ClosurePtr, ClosurePtr) {
    use crate::closure::{js_closure_alloc, js_closure_set_capture_f64};
    let box_activation_id = crate::r#box::async_box_activation_id(box_activation);
    let key = step_closure as usize;
    let cached = LAST_ASYNC_STEP_THUNKS.with(|c| c.get());
    if cached.0 == key && !cached.1.is_null() && !cached.2.is_null() {
        // #5485: refresh the captured activation `trap_next`. The step
        // closure (cache key) is per-activation, so this is normally the
        // same pointer; refreshing keeps us correct if the slot is reused.
        crate::closure::closure_set_capture_ref(cached.1, 1, trap_next as i64);
        crate::closure::closure_set_capture_ref(cached.2, 1, trap_next as i64);
        crate::closure::js_closure_set_capture_ptr(cached.1, 2, box_activation as i64);
        crate::closure::js_closure_set_capture_ptr(cached.2, 2, box_activation as i64);
        js_closure_set_capture_f64(cached.1, 3, box_activation_id as f64);
        js_closure_set_capture_f64(cached.2, 3, box_activation_id as f64);
        return (cached.1 as ClosurePtr, cached.2 as ClosurePtr);
    }
    // Capture layout: [step_closure_ptr, activation_trap_next_ptr,
    // stable_box_activation_ptr, generation]. Token storage is never freed;
    // pointer + generation rejects a capture after the token is recycled.
    let fulfill = js_closure_alloc(crate::fn_info!(async_step_fulfill_thunk, 1), 4);
    crate::closure::closure_set_capture_ref(fulfill, 0, step_closure as i64);
    crate::closure::closure_set_capture_ref(fulfill, 1, trap_next as i64);
    crate::closure::js_closure_set_capture_ptr(fulfill, 2, box_activation as i64);
    js_closure_set_capture_f64(fulfill, 3, box_activation_id as f64);
    let reject = js_closure_alloc(crate::fn_info!(async_step_reject_thunk, 1), 4);
    crate::closure::closure_set_capture_ref(reject, 0, step_closure as i64);
    crate::closure::closure_set_capture_ref(reject, 1, trap_next as i64);
    crate::closure::js_closure_set_capture_ptr(reject, 2, box_activation as i64);
    js_closure_set_capture_f64(reject, 3, box_activation_id as f64);
    LAST_ASYNC_STEP_THUNKS.with(|c| c.set((key, fulfill, reject)));
    (fulfill as ClosurePtr, reject as ClosurePtr)
}

// Thunks for the promise/thenable fallback in js_async_step_chain.
// Capture layout: [step_closure_ptr]
extern "C" fn async_step_fulfill_thunk(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let step =
        crate::closure::closure_capture_ref(closure, 0) as *const crate::closure::ClosureHeader;
    // #5485: the activation's own trap_next, captured when this thunk was
    // built (at the `await`), not the ambient INLINE_TRAP.trap_next at
    // resume time — which may belong to an UNRELATED outer activation and,
    // preserved here, let this resumed step's js_async_step_done reuse-gate
    // settle that outer activation's result promise prematurely with this
    // (intermediate await) value. Restoring the captured per-activation
    // value keeps same-activation settlement working while preventing the
    // cross-activation leak.
    let captured_trap_next = crate::closure::closure_capture_ref(closure, 1) as *mut Promise;
    let box_activation = crate::closure::js_closure_get_capture_ptr(closure, 2)
        as *mut crate::r#box::AsyncBoxActivation;
    let box_activation_id = crate::closure::js_closure_get_capture_f64(closure, 3) as u64;
    let box_activation = crate::r#box::find_async_box_activation(box_activation, box_activation_id);
    let false_bits = f64::from_bits(0x7FFC_0000_0000_0003);
    call_async_step_body(step, value, false_bits, captured_trap_next, box_activation)
}

/// Invoke an async step while keeping every pointer needed after the call in
/// mutable runtime roots. The step body is arbitrary user code and may relocate
/// the activation promise as well as both pointers saved from the ambient
/// `INLINE_TRAP`; re-read all three before restoring or forwarding.
#[inline]
fn call_async_step_body(
    step: ClosurePtr,
    value: f64,
    is_error: f64,
    captured_trap_next: *mut Promise,
    box_activation: *mut crate::r#box::AsyncBoxActivation,
) -> f64 {
    crate::r#box::retain_async_box_activation(box_activation);
    if !box_activation.is_null() {
        push_async_box_execution_ref(box_activation);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let captured_h = scope.root_nanbox_f64(boxed_promise_or_undef(captured_trap_next));

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
            box_activation,
        });
        old
    });
    let prev_trap_h = scope.root_nanbox_f64(boxed_promise_or_undef(prev.trap_next));
    let prev_step_h =
        scope.root_nanbox_f64(boxed_closure_or_undef(prev.current_step as ClosurePtr));

    // `step` is needed only by this call and is never used afterward. Any
    // future post-call use must root it and re-read its relocated address.
    let result = crate::closure::js_closure_call2(
        step,
        crate::closure::plain_call_receiver(),
        value,
        is_error,
    );
    let result_h = scope.root_nanbox_f64(result);
    INLINE_TRAP.with(|c| {
        c.set(InlineTrap {
            trap_next: unboxed_promise(&prev_trap_h),
            current_step: unboxed_closure(&prev_step_h) as usize,
            box_activation: prev.box_activation,
        })
    });
    forward_swallowed_rejection(result_h.get_nanbox_f64(), unboxed_promise(&captured_h));
    let result = result_h.get_nanbox_f64();
    if !box_activation.is_null() {
        pop_async_box_execution_ref(box_activation);
    }
    crate::r#box::release_async_box_activation(box_activation);
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
pub(super) fn forward_swallowed_rejection(result: f64, trap_next: *mut Promise) {
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
    _this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let step =
        crate::closure::closure_capture_ref(closure, 0) as *const crate::closure::ClosureHeader;
    // #5485: restore the captured per-activation trap_next (see
    // async_step_fulfill_thunk for the full rationale).
    let captured_trap_next = crate::closure::closure_capture_ref(closure, 1) as *mut Promise;
    let box_activation = crate::closure::js_closure_get_capture_ptr(closure, 2)
        as *mut crate::r#box::AsyncBoxActivation;
    let box_activation_id = crate::closure::js_closure_get_capture_f64(closure, 3) as u64;
    let box_activation = crate::r#box::find_async_box_activation(box_activation, box_activation_id);
    let true_bits = f64::from_bits(0x7FFC_0000_0000_0004);
    call_async_step_body(step, value, true_bits, captured_trap_next, box_activation)
}

const AFA_RESULT_PROMISE: u32 = 0;
const AFA_RESULT_ARRAY: u32 = 1;
/// The array-like source; `undefined` on the iterator path.
const AFA_ARRAY_LIKE: u32 = 2;
/// The async iterator; `undefined` on the array-like path.
const AFA_ITERATOR_VALUE: u32 = 3;
const AFA_REJECT_CLOSURE: u32 = 4;
const AFA_MAP_FN: u32 = 5;
const AFA_THIS_ARG: u32 = 6;
const AFA_INDEX: u32 = 7;
const AFA_VALUE_CLOSURE: u32 = 8;
const AFA_MAPPED_CLOSURE: u32 = 9;
/// The result `Construct(C)` built for a constructor receiver; `undefined`
/// when the result is the plain `AFA_RESULT_ARRAY` accumulator.
const AFA_CONSTRUCTED: u32 = 10;
/// `LengthOfArrayLike` of the array-like source, read once before `Construct`.
const AFA_LENGTH: u32 = 11;
/// Rejects the result after closing the iterator: the mapped-value handler.
const AFA_ABRUPT_CLOSURE: u32 = 12;
const AFA_STATE_CAPTURES: u32 = 13;

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

/// `GetMethod(items, @@<name>)` and, when present, `GetIteratorFromMethod`.
/// Throws when the method is not callable or its result is not an object.
fn call_iterator_method(items: f64, name: &str) -> Option<f64> {
    let sym = crate::symbol::well_known_symbol(name);
    if sym.is_null() {
        return None;
    }
    // A getter or the method itself is user code.
    let scope = crate::gc::RuntimeHandleScope::new();
    let items = scope.root_nanbox_f64(items);
    let sym = f64::from_bits(crate::value::JSValue::pointer(sym as *const u8).bits());
    let method =
        unsafe { crate::symbol::js_object_get_symbol_property(items.get_nanbox_f64(), sym) };
    if crate::collection_iter::is_null_or_undefined(method) {
        return None;
    }
    if !crate::collection_iter::is_callable(method) {
        crate::collection_iter::throw_type_error(&format!("Symbol.{name} is not a function"));
    }
    let iterator = unsafe {
        crate::closure::native_call_value_this(
            method,
            crate::closure::JsThis::from_f64(items.get_nanbox_f64()),
            std::ptr::null(),
            0,
        )
    };
    if !crate::collection_iter::is_entry_object(iterator) {
        crate::collection_iter::throw_type_error(&format!(
            "Result of the Symbol.{name} method is not an object"
        ));
    }
    Some(iterator)
}

/// A built-in iterable whose `@@iterator` dispatches by class id, so the
/// symbol read above answers `undefined` for it.
fn is_intrinsic_sync_iterable(items: f64) -> bool {
    let raw = crate::value::js_nanbox_get_pointer(items) as usize;
    crate::value::JSValue::from_bits(items.to_bits()).is_any_string()
        || crate::array::js_array_is_array(items).to_bits() == crate::value::TAG_TRUE
        || crate::map::is_registered_map(raw)
        || crate::set::is_registered_set(raw)
        || crate::array::is_builtin_iterator_class_id(raw)
        || crate::object::fixed_generator_state(items).is_some()
}

/// The async iterator `Array.fromAsync` drains (a sync iterable's wrapped by
/// `CreateAsyncFromSyncIterator`), or `None` for an array-like. Each
/// iterator method is read once.
fn iterator_value_for_from_async(items: f64) -> Option<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let items = scope.root_nanbox_f64(items);
    if let Some(iter) = call_iterator_method(items.get_nanbox_f64(), "asyncIterator") {
        return Some(iter);
    }
    if crate::object::is_async_generator_instance_value(items.get_nanbox_f64()) {
        return Some(items.get_nanbox_f64());
    }
    let iter = call_iterator_method(items.get_nanbox_f64(), "iterator").or_else(|| {
        is_intrinsic_sync_iterable(items.get_nanbox_f64())
            .then(|| crate::symbol::js_get_iterator(items.get_nanbox_f64()))
    })?;
    Some(crate::array::async_from_sync_wrap_iterator(iter))
}

/// `Array.fromAsync(input, mapFn?, thisArg?)` — Node 22+ static method.
///
/// Returns a Promise that resolves to an Array. Mapped results are awaited
/// before appending; an array-like's elements are also awaited before
/// mapping, while an iterable's values arrive through the async iterator
/// protocol (a sync iterable's through `CreateAsyncFromSyncIterator`).
#[no_mangle]
pub extern "C" fn js_array_from_async(input: f64, map_fn: f64, this_arg: f64) -> f64 {
    array_from_async_full(undefined_value(), input, map_fn, this_arg)
}

/// [`js_array_from_async`] called with `c` as its receiver: a constructor
/// `c` builds the result (`Construct(C)` for an iterable, `Construct(C,
/// «len»)` for an array-like), anything else leaves it a plain array.
pub(crate) fn array_from_async_full(c: f64, input: f64, map_fn: f64, this_arg: f64) -> f64 {
    use crate::array::js_array_alloc;
    use crate::closure::{js_closure_alloc, js_closure_set_capture_f64};

    if let Some(rejected) = array_from_async_nullish_error(input) {
        return rejected;
    }
    let map_fn = match resolve_from_async_map_fn(map_fn) {
        Ok(Some(_)) => map_fn,
        Ok(None) => undefined_value(),
        Err(reason) => return boxed_promise(js_promise_rejected(reason)),
    };

    // The iterator probes and `Construct(C)` run user code.
    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_nanbox_f64(input);
    let map_fn = scope.root_nanbox_f64(map_fn);
    let this_arg = scope.root_nanbox_f64(this_arg);
    let c = scope.root_nanbox_f64(c);
    // GetMethod and the array-like reads see a primitive source's wrapper.
    let source = input.get_nanbox_f64();
    let input = if crate::array::boxed_primitive_source(source, source.to_bits()) {
        scope.root_nanbox_f64(crate::object::js_object_coerce(input.get_nanbox_f64()))
    } else {
        input
    };
    let source = crate::exception::catch_js_throw(|| {
        match iterator_value_for_from_async(input.get_nanbox_f64()) {
            Some(iter) => (Some(iter), None),
            None => (
                None,
                Some(crate::array::array_like_length(input.get_nanbox_f64())),
            ),
        }
    });
    let (iter_value, len) = match source {
        Ok(source) => source,
        Err(reason) => return boxed_promise(js_promise_rejected(reason)),
    };
    let c_value = c.get_nanbox_f64();
    if len.is_some_and(|len| len > u32::MAX as usize)
        && !crate::object::js_value_is_constructor(c_value)
    {
        let msg = crate::string::js_string_from_bytes(b"Invalid array length".as_ptr(), 20);
        let err = crate::error::js_rangeerror_new(msg);
        return boxed_promise(js_promise_rejected(crate::value::js_nanbox_pointer(
            err as i64,
        )));
    }
    let len = len.map(|len| len as f64);
    let iter_value = scope.root_nanbox_f64(iter_value.unwrap_or_else(undefined_value));
    let c = c.get_nanbox_f64();
    let constructed = if crate::object::js_value_is_constructor(c) {
        let args = len.as_slice();
        match crate::exception::catch_js_throw(|| unsafe {
            crate::object::js_new_function_construct(c, args.as_ptr(), args.len())
        }) {
            Ok(target) => target,
            Err(reason) => return boxed_promise(js_promise_rejected(reason)),
        }
    } else {
        undefined_value()
    };
    let constructed = scope.root_nanbox_f64(constructed);

    // Every allocation below can collect, and the first step runs user code:
    // each new cell is rooted and re-read after the allocations that follow.
    let root = |cell: *mut u8| scope.root_nanbox_f64(crate::value::js_nanbox_pointer(cell as i64));
    let result_promise = root(js_promise_new() as *mut u8);
    let result_arr = root(js_array_alloc(0) as *mut u8);
    let state = root(js_closure_alloc(
        crate::fn_info!(array_from_async_step, 1),
        AFA_STATE_CAPTURES,
    ) as *mut u8);
    let reject_closure =
        root(js_closure_alloc(crate::fn_info!(array_from_async_reject, 1), 1) as *mut u8);
    let value_closure =
        root(js_closure_alloc(crate::fn_info!(array_from_async_value_step, 1), 1) as *mut u8);
    let mapped_closure =
        root(js_closure_alloc(crate::fn_info!(array_from_async_mapped_step, 1), 1) as *mut u8);
    let abrupt_closure =
        root(js_closure_alloc(crate::fn_info!(array_from_async_abrupt_step, 1), 1) as *mut u8);
    let cell = |handle: &crate::gc::RuntimeHandle<'_>| {
        crate::value::js_nanbox_get_pointer(handle.get_nanbox_f64())
    };
    let closure = |handle| cell(handle) as *mut crate::closure::ClosureHeader;

    let state_ref = cell(&state);
    crate::closure::closure_set_capture_ref(closure(&reject_closure), 0, cell(&result_promise));
    crate::closure::closure_set_capture_ref(closure(&value_closure), 0, state_ref);
    crate::closure::closure_set_capture_ref(closure(&mapped_closure), 0, state_ref);
    crate::closure::closure_set_capture_ref(closure(&abrupt_closure), 0, state_ref);

    let array_like = if len.is_some() {
        input.get_nanbox_f64()
    } else {
        undefined_value()
    };
    let st = closure(&state);
    crate::closure::closure_set_capture_ref(st, AFA_RESULT_PROMISE, cell(&result_promise));
    crate::closure::closure_set_capture_ref(st, AFA_RESULT_ARRAY, cell(&result_arr));
    js_closure_set_capture_f64(st, AFA_ARRAY_LIKE, array_like);
    js_closure_set_capture_f64(st, AFA_LENGTH, len.unwrap_or(0.0));
    crate::closure::closure_set_capture_ref(st, AFA_ABRUPT_CLOSURE, cell(&abrupt_closure));
    js_closure_set_capture_f64(st, AFA_ITERATOR_VALUE, iter_value.get_nanbox_f64());
    crate::closure::closure_set_capture_ref(st, AFA_REJECT_CLOSURE, cell(&reject_closure));
    js_closure_set_capture_f64(st, AFA_MAP_FN, map_fn.get_nanbox_f64());
    js_closure_set_capture_f64(st, AFA_THIS_ARG, this_arg.get_nanbox_f64());
    js_closure_set_capture_f64(st, AFA_INDEX, 0.0);
    js_closure_set_capture_f64(st, AFA_CONSTRUCTED, constructed.get_nanbox_f64());
    crate::closure::closure_set_capture_ref(st, AFA_VALUE_CLOSURE, cell(&value_closure));
    crate::closure::closure_set_capture_ref(st, AFA_MAPPED_CLOSURE, cell(&mapped_closure));

    if len.is_none() {
        array_from_async_call_next(st);
    } else {
        array_from_async_array_next(st);
    }

    result_promise.get_nanbox_f64()
}

fn array_from_async_is_iterator_path(state: *const crate::closure::ClosureHeader) -> bool {
    crate::closure::js_closure_get_capture_f64(state, AFA_ITERATOR_VALUE).to_bits()
        != crate::value::TAG_UNDEFINED
}

fn array_from_async_reject_result(state: *const crate::closure::ClosureHeader, reason: f64) {
    let result_promise = crate::closure::closure_capture_ref(state, AFA_RESULT_PROMISE);
    js_promise_reject(result_promise as *mut Promise, reason);
}

/// `Await(Call(next, iterator))`, continuing in [`array_from_async_step`].
fn array_from_async_call_next(state: *const crate::closure::ClosureHeader) {
    let iter_value = crate::closure::js_closure_get_capture_f64(state, AFA_ITERATOR_VALUE);
    // `next` is user code.
    let scope = crate::gc::RuntimeHandleScope::new();
    let state_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(state as i64));
    let next_result = crate::exception::catch_js_throw(|| unsafe {
        let result = crate::object::js_native_call_method(
            iter_value,
            b"next".as_ptr() as *const i8,
            4,
            std::ptr::null(),
            0,
        );
        js_promise_resolved(result)
    });
    let state = crate::value::js_nanbox_get_pointer(state_h.get_nanbox_f64())
        as *const crate::closure::ClosureHeader;
    match next_result {
        Ok(awaited) => {
            let reject = crate::closure::closure_capture_ref(state, AFA_REJECT_CLOSURE)
                as *const crate::closure::ClosureHeader;
            js_promise_then(awaited, state, reject);
        }
        Err(reason) => array_from_async_reject_result(state, reason),
    }
}

extern "C" fn array_from_async_reject(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    reason: f64,
) -> f64 {
    let result_promise = crate::closure::closure_capture_ref(closure, 0) as *mut Promise;
    if !result_promise.is_null() {
        js_promise_reject(result_promise, reason);
    }
    0.0
}

/// `.then(...)` handler invoked once per `.next()` resolution: finishes on
/// `done`, otherwise maps or appends `value`.
extern "C" fn array_from_async_step(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    iter_result: f64,
) -> f64 {
    // Building the error and reading `done` / `value` (maybe getters) collect.
    let scope = crate::gc::RuntimeHandleScope::new();
    let state_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(closure as i64));
    if !crate::collection_iter::is_entry_object(iter_result) {
        let message = format!(
            "Iterator result {} is not an object",
            render_from_async_callback(iter_result)
        );
        let error = type_error_value(&message);
        let state = crate::value::js_nanbox_get_pointer(state_h.get_nanbox_f64())
            as *const crate::closure::ClosureHeader;
        array_from_async_reject_result(state, error);
        return 0.0;
    }
    let result_h = scope.root_nanbox_f64(iter_result);
    let read = |name: &[u8]| {
        let key = crate::string::js_string_from_bytes_longlived(name.as_ptr(), name.len() as u32);
        let obj = crate::value::js_nanbox_get_pointer(result_h.get_nanbox_f64())
            as *const crate::object::ObjectHeader;
        f64::from_bits(crate::object::js_object_get_field_by_name(obj, key).bits())
    };
    let step = crate::exception::catch_js_throw(|| {
        (crate::value::js_is_truthy(read(b"done")) == 0).then(|| read(b"value"))
    });
    let state = crate::value::js_nanbox_get_pointer(state_h.get_nanbox_f64())
        as *const crate::closure::ClosureHeader;
    match step {
        Ok(Some(value)) => array_from_async_map_or_push(state, value),
        Ok(None) => array_from_async_finish(state),
        Err(reason) => array_from_async_reject_result(state, reason),
    }
    0.0
}

/// `Await(? Get(arrayLike, k))` for the next index, or the finish once `k`
/// reaches the length read before `Construct`.
fn array_from_async_array_next(state: *const crate::closure::ClosureHeader) {
    let index = crate::closure::js_closure_get_capture_f64(state, AFA_INDEX);
    if index >= crate::closure::js_closure_get_capture_f64(state, AFA_LENGTH) {
        array_from_async_finish(state);
        return;
    }
    let array_like = crate::closure::js_closure_get_capture_f64(state, AFA_ARRAY_LIKE);
    // An element getter is user code.
    let scope = crate::gc::RuntimeHandleScope::new();
    let state_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(state as i64));
    let value = crate::exception::catch_js_throw(|| {
        crate::object::js_object_get_index_polymorphic(array_like.to_bits() as i64, index)
    });
    let state = crate::value::js_nanbox_get_pointer(state_h.get_nanbox_f64())
        as *const crate::closure::ClosureHeader;
    match value {
        Ok(value) => array_from_async_await_value(state, value),
        Err(reason) => array_from_async_reject_result(state, reason),
    }
}

/// Resolves the result promise with the collected values: the accumulator,
/// or the constructed result once `Set(A, "length", k, true)` closed it.
fn array_from_async_finish(closure: *const crate::closure::ClosureHeader) {
    let result_promise = crate::closure::closure_capture_ref(closure, AFA_RESULT_PROMISE);
    let constructed = crate::closure::js_closure_get_capture_f64(closure, AFA_CONSTRUCTED);
    if constructed.to_bits() == crate::value::TAG_UNDEFINED {
        let result_arr = crate::closure::closure_capture_ref(closure, AFA_RESULT_ARRAY);
        js_promise_resolve(
            result_promise as *mut Promise,
            crate::value::js_nanbox_pointer(result_arr),
        );
        return;
    }
    let len = crate::closure::js_closure_get_capture_f64(closure, AFA_INDEX) as usize;
    // A `length` setter on the result runs user code.
    let scope = crate::gc::RuntimeHandleScope::new();
    let result_promise = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(result_promise));
    let settled = crate::exception::catch_js_throw(|| {
        crate::array::finish_constructed_length(constructed, len)
    });
    let result_promise =
        crate::value::js_nanbox_get_pointer(result_promise.get_nanbox_f64()) as *mut Promise;
    match settled {
        Ok(result) => js_promise_resolve(result_promise, result),
        Err(reason) => js_promise_reject(result_promise, reason),
    }
}

fn array_from_async_await_value(closure: *const crate::closure::ClosureHeader, value: f64) {
    let value_closure = crate::closure::closure_capture_ref(closure, AFA_VALUE_CLOSURE)
        as *const crate::closure::ClosureHeader;
    let reject = crate::closure::closure_capture_ref(closure, AFA_REJECT_CLOSURE)
        as *const crate::closure::ClosureHeader;
    let promise = js_promise_resolved(value);
    js_promise_then(promise, value_closure, reject);
}

extern "C" fn array_from_async_value_step(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let state =
        crate::closure::closure_capture_ref(closure, 0) as *const crate::closure::ClosureHeader;
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
    let mapped_promise = super::combinators::promise_try_this(
        map_fn,
        crate::closure::JsThis::from_f64(this_arg),
        args_arr,
    );

    let mapped_closure = crate::closure::closure_capture_ref(closure, AFA_MAPPED_CLOSURE)
        as *const crate::closure::ClosureHeader;
    let abrupt = crate::closure::closure_capture_ref(closure, AFA_ABRUPT_CLOSURE)
        as *const crate::closure::ClosureHeader;
    js_promise_then(mapped_promise, mapped_closure, abrupt);
}

extern "C" fn array_from_async_mapped_step(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let state =
        crate::closure::closure_capture_ref(closure, 0) as *const crate::closure::ClosureHeader;
    if !state.is_null() {
        array_from_async_push_and_continue(state, value);
    }
    0.0
}

fn array_from_async_push_and_continue(
    mut closure: *const crate::closure::ClosureHeader,
    value: f64,
) {
    let index = crate::closure::js_closure_get_capture_f64(closure, AFA_INDEX);
    let constructed = crate::closure::js_closure_get_capture_f64(closure, AFA_CONSTRUCTED);
    if constructed.to_bits() == crate::value::TAG_UNDEFINED {
        let mut result_arr = crate::closure::closure_capture_ref(closure, AFA_RESULT_ARRAY)
            as *mut crate::array::ArrayHeader;
        if result_arr.is_null() {
            return;
        }
        result_arr = crate::array::js_array_push_f64(result_arr, value);
        crate::closure::closure_set_capture_ref(
            closure as *mut _,
            AFA_RESULT_ARRAY,
            result_arr as i64,
        );
    } else {
        // The define reaches the result's own traps and setters: user code.
        let scope = crate::gc::RuntimeHandleScope::new();
        let state = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(closure as i64));
        let defined = crate::exception::catch_js_throw(|| {
            crate::array::define_constructed_index(constructed, index as usize, value)
        });
        closure = crate::value::js_nanbox_get_pointer(state.get_nanbox_f64())
            as *const crate::closure::ClosureHeader;
        match defined {
            Ok(result) => crate::closure::js_closure_set_capture_f64(
                closure as *mut _,
                AFA_CONSTRUCTED,
                result,
            ),
            Err(reason) => {
                array_from_async_abrupt(closure, reason);
                return;
            }
        }
    }

    let next_index = index + 1.0;
    crate::closure::js_closure_set_capture_f64(closure as *mut _, AFA_INDEX, next_index);

    if array_from_async_is_iterator_path(closure) {
        array_from_async_call_next(closure);
    } else {
        array_from_async_array_next(closure);
    }
}

extern "C" fn array_from_async_abrupt_step(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    reason: f64,
) -> f64 {
    let state =
        crate::closure::closure_capture_ref(closure, 0) as *const crate::closure::ClosureHeader;
    if !state.is_null() {
        array_from_async_abrupt(state, reason);
    }
    0.0
}

/// Rejects the result with `reason`. On the iterator path `AsyncIteratorClose`
/// runs first: `return()` is called and its result awaited, and a failure of
/// either is dropped in favor of `reason`.
fn array_from_async_abrupt(state: *const crate::closure::ClosureHeader, reason: f64) {
    if !array_from_async_is_iterator_path(state) {
        array_from_async_reject_result(state, reason);
        return;
    }
    let iter_value = crate::closure::js_closure_get_capture_f64(state, AFA_ITERATOR_VALUE);
    let scope = crate::gc::RuntimeHandleScope::new();
    let state_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(state as i64));
    let reason_h = scope.root_nanbox_f64(reason);
    let returned = crate::exception::catch_js_throw(|| unsafe {
        let result = crate::object::js_native_call_method(
            iter_value,
            b"return".as_ptr() as *const i8,
            6,
            std::ptr::null(),
            0,
        );
        js_promise_resolved(result)
    });
    let awaited = match returned {
        Ok(awaited) => scope.root_nanbox_f64(crate::value::js_nanbox_pointer(awaited as i64)),
        Err(_) => {
            let state = crate::value::js_nanbox_get_pointer(state_h.get_nanbox_f64())
                as *const crate::closure::ClosureHeader;
            array_from_async_reject_result(state, reason_h.get_nanbox_f64());
            return;
        }
    };
    let rethrow = crate::closure::js_closure_alloc(crate::fn_info!(array_from_async_rethrow, 1), 2);
    let state = crate::value::js_nanbox_get_pointer(state_h.get_nanbox_f64())
        as *const crate::closure::ClosureHeader;
    let result_promise = crate::closure::closure_capture_ref(state, AFA_RESULT_PROMISE);
    crate::closure::closure_set_capture_ref(rethrow, 0, result_promise);
    crate::closure::js_closure_set_capture_f64(rethrow, 1, reason_h.get_nanbox_f64());
    let awaited = crate::value::js_nanbox_get_pointer(awaited.get_nanbox_f64()) as *mut Promise;
    js_promise_then(awaited, rethrow, rethrow);
}

extern "C" fn array_from_async_rethrow(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    _value: f64,
) -> f64 {
    let result_promise = crate::closure::closure_capture_ref(closure, 0) as *mut Promise;
    js_promise_reject(
        result_promise,
        crate::closure::js_closure_get_capture_f64(closure, 1),
    );
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::perry_thread_local! {
        // Test-only pre-forwarding addresses intentionally have no mutable-root
        // scanner: rewriting them would defeat this relocation fixture. The
        // test also mutates INLINE_TRAP and must run in the runtime suite's
        // required single-threaded mode (`RUST_TEST_THREADS=1`).
        static RELOCATION_CASE: std::cell::Cell<[usize; 7]> = const {
            std::cell::Cell::new([0; 7])
        };
    }

    unsafe fn copy_promise_to_old(source: *mut Promise) -> *mut Promise {
        let destination = crate::arena::arena_alloc_gc_old(
            std::mem::size_of::<Promise>(),
            std::mem::align_of::<Promise>(),
            crate::gc::GC_TYPE_PROMISE,
        ) as *mut Promise;
        std::ptr::copy_nonoverlapping(source, destination, 1);
        destination
    }

    unsafe fn copy_closure_to_old(
        source: *const crate::closure::ClosureHeader,
    ) -> *mut crate::closure::ClosureHeader {
        let size = crate::closure::closure_payload_size(
            (*source).capture_count as usize + crate::closure::closure_own_slot_count(source),
        );
        let destination = crate::arena::arena_alloc_gc_old(
            size,
            std::mem::align_of::<crate::closure::ClosureHeader>(),
            crate::gc::GC_TYPE_CLOSURE,
        ) as *mut crate::closure::ClosureHeader;
        std::ptr::copy_nonoverlapping(source as *const u8, destination as *mut u8, size);
        destination
    }

    unsafe fn forward(source: *mut u8, destination: *mut u8) {
        let header = source.sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        crate::gc::set_forwarding_address(header, destination);
    }

    extern "C" fn relocating_step(
        _closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
        _value: f64,
        _is_error: f64,
    ) -> f64 {
        let [captured_from, captured_to, previous_from, previous_to, step_from, step_to, rejected] =
            RELOCATION_CASE.with(|case| case.get());
        unsafe {
            forward(captured_from as *mut u8, captured_to as *mut u8);
            forward(previous_from as *mut u8, previous_to as *mut u8);
            forward(step_from as *mut u8, step_to as *mut u8);
        }

        crate::gc::test_rewrite_runtime_handles_for_forwarded_objects();

        // The real promise-root scanner rewrites the currently installed trap
        // during a copying collection. Only the saved `prev` value is outside
        // that scanner, which is the lifetime this test isolates.
        INLINE_TRAP.with(|slot| {
            let mut active = slot.get();
            active.trap_next = captured_to as *mut Promise;
            slot.set(active);
        });

        crate::value::js_nanbox_pointer(rejected as i64)
    }

    struct TrapGuard {
        original_trap: InlineTrap,
        forwarded_sources: [(*mut u8, usize); 3],
    }

    impl Drop for TrapGuard {
        fn drop(&mut self) {
            unsafe {
                for (source, first_word) in self.forwarded_sources {
                    // GC_STORE_AUDIT(POINTER_FREE): test restores Promise state
                    // or a native closure function word, never a GC edge.
                    source.cast::<usize>().write(first_word);
                    let header = source.sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
                    (*header).gc_flags &= !crate::gc::GC_FLAG_FORWARDED;
                }
            }
            INLINE_TRAP.with(|slot| slot.set(self.original_trap));
            RELOCATION_CASE.with(|case| case.set([0; 7]));
        }
    }

    #[test]
    fn resumed_step_rereads_captured_and_saved_trap_roots_after_relocation() {
        unsafe {
            let captured_from = js_promise_new();
            let captured_to = copy_promise_to_old(captured_from);
            let previous_from = js_promise_new();
            let previous_to = copy_promise_to_old(previous_from);
            let previous_step_from =
                crate::closure::js_closure_alloc(crate::fn_info!(relocating_step, 2), 0);
            let previous_step_to = copy_closure_to_old(previous_step_from);
            let rejected = js_promise_rejected(73.0);

            let step = crate::closure::js_closure_alloc(crate::fn_info!(relocating_step, 2), 0);
            let thunk =
                crate::closure::js_closure_alloc(crate::fn_info!(async_step_fulfill_thunk, 1), 4);
            crate::closure::closure_set_capture_ref(thunk, 0, step as i64);
            crate::closure::closure_set_capture_ref(thunk, 1, captured_from as i64);
            crate::closure::closure_set_capture_word(thunk, 2, 0);
            crate::closure::js_closure_set_capture_f64(thunk, 3, 0.0);

            let original = INLINE_TRAP.with(|slot| {
                let original = slot.get();
                slot.set(InlineTrap {
                    trap_next: previous_from,
                    current_step: previous_step_from as usize,
                    box_activation: std::ptr::null_mut(),
                });
                original
            });
            let forwarded_sources = [
                captured_from as *mut u8,
                previous_from as *mut u8,
                previous_step_from as *mut u8,
            ];
            let forwarded_sources_with_words = forwarded_sources.map(|source| {
                let first_word = source.cast::<usize>().read();
                (source, first_word)
            });
            let guard = TrapGuard {
                original_trap: original,
                forwarded_sources: forwarded_sources_with_words,
            };
            RELOCATION_CASE.with(|case| {
                case.set([
                    captured_from as usize,
                    captured_to as usize,
                    previous_from as usize,
                    previous_to as usize,
                    previous_step_from as usize,
                    previous_step_to as usize,
                    rejected as usize,
                ])
            });

            async_step_fulfill_thunk(thunk, crate::closure::plain_call_receiver(), 41.0);

            assert_eq!(
                (*captured_to).state,
                PromiseState::Rejected,
                "rejection forwarding must target the relocated activation promise"
            );
            let restored = INLINE_TRAP.with(|slot| slot.get());
            assert_eq!(
                restored.trap_next, previous_to,
                "the restored trap promise must use its relocated address"
            );
            assert_eq!(
                restored.current_step, previous_step_to as usize,
                "the restored step closure must use its relocated address"
            );

            drop(guard);
            for (source, first_word) in forwarded_sources_with_words {
                assert_eq!(
                    source.cast::<usize>().read(),
                    first_word,
                    "test teardown must restore the source object's first payload word"
                );
                let header = source.sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
                assert_eq!(
                    (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED,
                    0,
                    "test teardown must clear the synthetic forwarding flag"
                );
            }
        }
    }
}
