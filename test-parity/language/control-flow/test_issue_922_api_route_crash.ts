// Regression test for issue #922 — every /api/* request crashes the
// process with an infinite loop of:
//
//   [WARN_NULL_PTR] js_object_set_field: null POINTER_TAG at obj=... field_index=0 class_id=0
//   TypeError: value is not a function
//       at <anonymous>
//   [WARN_NULL_PTR] js_object_set_field: null POINTER_TAG at obj=... field_index=0 class_id=0
//   TypeError: value is not a function
//       at <anonymous>
//   ... (5.7M+ lines, PM2 declares the process errored)
//
// The user's gscmaster-api Fastify service entered this loop on every
// /api/* hit after re-compiling under a fresh Perry. The May 10 binary
// of the byte-identical source worked; a May 15+ rebuild looped.
//
// The infinite loop has two co-occurring root causes:
//
//   1. `js_object_set_field` saw a null POINTER_TAG (0x7FFD_0000_0000_0000)
//      coming through as the value, emitted ONE `eprintln!` per write, and
//      let the caller continue with an undefined slot. The downstream call
//      site then read that slot, dispatched as a function, and
//      `throw_not_callable` re-raised. Codegen for the user's specific
//      pattern (async fastify route handler with `throw` across `await`
//      inside `try`/`catch`) re-entered the same write+throw sequence
//      forever — no bound, no abort.
//
//   2. The async-step driver's `ASYNC_STEP_GUARD` from #712 only counted
//      consecutive same-`step_closure` errors; the production loop
//      alternated between two step closures, so the counter reset every
//      other dispatch and never tripped the 10K bound.
//
// Fix (this PR):
//
//   * `js_object_set_field` rate-limits the [WARN_NULL_PTR] log lines to
//     64 occurrences (then prints a one-time "suppressed" notice) AND
//     aborts the process with a clear diagnostic line after 100K
//     consecutive same-site writes (issue #922 circuit breaker).
//   * `throw_not_callable` aborts after 100K consecutive
//     `value is not a function` throws on the same thread.
//   * `ASYNC_STEP_GUARD` now counts ANY consecutive `is_error=true`
//     dispatches, not just same-closure ones, so the production loop
//     trips at 10K iterations and rejects the chain.
//   * A non-error step dispatch resets the `throw_not_callable` counter,
//     so legitimate cumulative throws across a long-running process
//     don't false-positive.
//
// This test exercises the throw_not_callable circuit breaker directly:
// call an undefined function inside a `try`/`catch` 200K times in a sync
// loop. These are ordinary catchable `TypeError`s, so in Node the loop
// completes and prints `loop-end caught=200000`.
//
// Issue #2780 fix: the `throw_not_callable` circuit breaker no longer
// counts CAUGHT throws toward its 100K abort threshold. `js_throw` resets
// the counter whenever the throw reaches an open `try` (`try_depth > 0`),
// so this loop now completes byte-identically to Node instead of hitting
// `[PERRY ABORT]` at iteration 100K. The breaker still aborts genuinely
// *uncaught* runaway throw loops (`try_depth == 0`), and the async-step
// guards in `promise/microtasks.rs` still cover unbounded async re-entry.
//
// Expected (Node + Perry): two lines —
//   loop-start
//   loop-end caught=200000

function makeUndefinedFn(): any {
    return undefined;
}

function exerciseCircuitBreaker(): number {
    const fn = makeUndefinedFn();
    let caught = 0;
    // Tight loop calling an undefined function. The fix's circuit breaker
    // aborts at 100K iterations; without the fix the loop runs to
    // completion (200K iterations, 200K caught exceptions, ~0.5s).
    for (let i = 0; i < 200000; i++) {
        try {
            fn(i);
        } catch (_e) {
            caught++;
        }
    }
    return caught;
}

// With the circuit breaker: this never returns — the process aborts.
// We log the loop start so the test harness can verify the binary
// reached the loop body before aborting.
console.log("loop-start");
const n = exerciseCircuitBreaker();
console.log("loop-end caught=" + n);
