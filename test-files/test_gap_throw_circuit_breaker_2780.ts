// Gap test for issue #2780 — the `throw_not_callable` circuit breaker must
// NOT count CAUGHT throws toward its abort threshold.
//
// Calling a non-callable value (e.g. `(undefined as any)()`) raises a
// `TypeError: value is not a function`. In Node that is an ordinary
// catchable error, so a loop that throws-and-catches it any number of times
// completes normally. Perry's `#922` anti-runaway guard (in
// `crates/perry-runtime/src/closure/dispatch.rs`) used to count EVERY such
// throw — caught or not — and `[PERRY ABORT]`ed the whole process once the
// count crossed 100,000. Real workloads (route handlers, effect-style retry
// loops) that legitimately throw-and-catch a non-callable many times tripped
// it and the process died.
//
// The fix: a throw that reaches an open `try` (`try_depth > 0` in
// `js_throw`) is being caught, so it is not a runaway — it resets the
// counter. The breaker still aborts a genuinely UNCAUGHT runaway throw
// (those hit the `try_depth == 0` path). 200,000 caught throws must now
// complete and print the count, byte-identical to Node.

function notAFunction(): any {
    return undefined;
}

function loop(): number {
    const fn = notAFunction();
    let caught = 0;
    for (let i = 0; i < 200000; i++) {
        try {
            fn(i);
        } catch (_e) {
            caught++;
        }
    }
    return caught;
}

console.log("loop-start");
const caught = loop();
console.log("loop-end caught=" + caught);
