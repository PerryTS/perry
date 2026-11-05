// Regression coverage for issue #236.
// Three independent bugs surface from the same OP repro:
//   (1) `.then(console.log)` used to hang the chained promise forever
//       — the on_fulfilled was a NULL ClosurePtr sentinel and js_promise_*
//       only pushed to the microtask queue when the callback was non-null,
//       so propagation to `next` never fired.
//   (2) `console.log` passed as a value used to lower to the `0.0` sentinel,
//       so even after the propagation fix the body silently dropped.
//       Fix routes `console.log`-as-value through a runtime-allocated
//       singleton closure that thunks into js_console_log_dynamic.
//   (3) Anything that rejected with a bare string-pointer NaN-boxed with
//       POINTER_TAG printed as `Uncaught exception: [object Object]` —
//       the printer read the StringHeader's byte_len as object_type and
//       fell into the generic-object arm. Promise reject sites in fetch
//       now allocate a real Error so the printer takes the Error arm.
//
// (1) and (2) are exercised here. (3) needs a network call to surface and
// is covered by the manual repro in /tmp/issue_236.ts (kept out of the
// test suite so CI doesn't depend on api.github.com being reachable).

async function makeData(): Promise<string> {
    return "hello world";
}

console.log("start");

// Pre-fix this hung forever — chain never settled.
// Post-fix it both prints "hello world" (singleton-closure dispatch) and
// completes (microtask propagation).
await makeData().then(console.log);

console.log("between");

// Variant: Promise.resolve directly with .then(console.log).
await Promise.resolve(42).then(console.log);

console.log("after");
