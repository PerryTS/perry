//! Not-callable error path: `throw_not_callable` plus its #922 runaway-loop
//! circuit breaker and the counter reset hook the async-step driver calls.

/// Issue #648: calling a value that isn't a function (most commonly the
/// result of a property lookup that returned undefined, e.g.
/// `obj.missingFn()`) must throw a TypeError that user code can catch via
/// `try { ... } catch`. Pre-fix every `js_closure_callN` (and the `_array`
/// / `_apply_with_spread` dispatch entry points) silently returned
/// TAG_UNDEFINED when `func_ptr` failed validation, which let
/// `obj.missingFn(1, 2)` quietly evaluate to `undefined` and continue —
/// the single biggest leverage source of cascading parity-test failures
/// (`test_parity_timers` hung forever waiting on `timers.setTimeout` which
/// silently no-op'd; `test_parity_os`/`tls`/`perf_hooks`/`http2`
/// truncated mid-script when an unimplemented binding silently no-op'd).
/// Now we throw via the existing `js_throw_type_error_not_a_function`
/// machinery, which routes through Perry's exception system so a
/// surrounding `try`/`catch` catches it (per #596).
// Issue #922 circuit breaker. Track consecutive `throw_not_callable`
// invocations on the current thread; abort if the count crosses the
// runaway bound. Mirrors the `record_warn_null_ptr` pattern in
// `object.rs` — production gscmaster-api Fastify route handlers
// (#921/#922) entered a 5.7M-iteration loop where every async-step
// catch arm re-fired the same TypeError, and the per-step-closure
// reentry guard at `promise.rs::ASYNC_STEP_GUARD` missed it because
// the loop alternated between two step closures. With this fixed
// upper bound the loop terminates in milliseconds with a single
// useful stderr line, instead of 5.7M `TypeError: value is not a
// function at <anonymous>` lines that drown out the diagnostic.
const THROW_NOT_CALLABLE_ABORT_LIMIT: u64 = 100_000;

thread_local! {
    static THROW_NOT_CALLABLE_COUNT: std::cell::Cell<u64>
        = const { std::cell::Cell::new(0) };
}

#[cold]
#[inline(never)]
pub fn throw_not_callable() -> ! {
    let count = THROW_NOT_CALLABLE_COUNT.with(|c| {
        let n = c.get().saturating_add(1);
        c.set(n);
        n
    });
    if count >= THROW_NOT_CALLABLE_ABORT_LIMIT {
        eprintln!(
            "[PERRY ABORT] throw_not_callable: detected runaway TypeError loop ({}+ consecutive 'value is not a function' throws -- issue #922 circuit breaker). Common cause: an async function throws across an await boundary inside try/catch where the catch arm re-enters the same await. Convert to a result-tag pattern (see issue #921 workaround). To find the offending callsite: recompile with --debug-symbols and run under a debugger -- set a breakpoint on js_throw_type_error_not_a_function.",
            THROW_NOT_CALLABLE_ABORT_LIMIT
        );
        std::process::abort();
    }
    crate::error::js_throw_type_error_not_a_function(std::ptr::null(), 0, b"value".as_ptr(), 5)
}

/// Reset the throw_not_callable counter — called by the async-step
/// driver whenever a non-error `is_error=false` step dispatches, which
/// signals progress (the catch arm advanced past the bad await). Lives
/// here so the thread-local is private to this module.
///
/// This exists as a `pub fn` (not `extern "C"`) — it's an internal
/// runtime-side reset called from `promise.rs::js_promise_run_microtasks`.
pub(crate) fn reset_throw_not_callable_counter() {
    THROW_NOT_CALLABLE_COUNT.with(|c| c.set(0));
}
