// Issue #957 — `import _ from "lodash"; _.add(1, 2)` regression test.
//
// Real lodash exercises two distinct codegen bugs that, before this commit,
// caused `import _ from "lodash"` to resolve to `undefined`:
//
//   1. `(function() { ... }.call(this))` IIFE wrapper — the function body
//      never executed, so the wrap module's `module.exports = _` write was
//      silently dropped and the cjs_wrap-generated `const _cjs = (function()
//      { ...; return module.exports; })()` returned the initial empty
//      `{ exports: {} }` instead of the package's actual export.
//
//   2. `++result[key]` (`Expr::IndexUpdate`) bailed at codegen with
//      `expression IndexUpdate not yet supported`. Because Perry's
//      compile-as-package pipeline runs in `PERRY_ALLOW_UNIMPLEMENTED=1`
//      mode (linker stubs out failed modules), the entire lodash module
//      compiled to an empty init stub — so `_` never got assigned.
//
// This test exercises both fixes via the minimum CJS shape lodash uses.
// Real lodash still hits separate unrelated issues at runtime (no usable
// `global` / `Function('return this')()`); those are tracked downstream.

// ── (1) IIFE.call(this) executes its body and propagates outer-capture writes
const captured: { value: number } = { value: 0 };
(function () {
    captured.value = 42;
}.call(this));
console.log("iife_call_writes_outer:", captured.value);

// Pass args through .call() on an inline arrow function — should run the body
// with the user-supplied args and return the body's result.
console.log(
    "iife_arrow_call_with_args:",
    ((a: number, b: number) => a + b).call(null, 3, 4),
);

// ── (2) IndexUpdate: ++obj[key] / obj[key]++ / --obj[key] / ++arr[i]
const acc: { [k: string]: number } = { foo: 5, bar: 10 };
++acc["foo"];
console.log("prefix_inc_object_string_key:", acc.foo);

acc["bar"]++;
console.log("postfix_inc_object_string_key:", acc.bar);

--acc["foo"];
console.log("prefix_dec_object_string_key:", acc.foo);

const arr = [10, 20, 30];
++arr[0];
console.log("prefix_inc_array_numeric_index:", arr[0]);

arr[1]++;
console.log("postfix_inc_array_numeric_index:", arr[1]);

// Prefix returns the NEW value; postfix returns the OLD value. Confirm via
// the assignment-target's value capture below.
const counter: { [k: string]: number } = { n: 5 };
const pre = ++counter["n"];
const post = counter["n"]++;
console.log("prefix_returns_new:", pre); // 6
console.log("postfix_returns_old:", post); // 6 (after pre==6; postfix returns old 6, then n→7)
console.log("counter_final:", counter.n); // 7
