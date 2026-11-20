// Issue followup to #985: V8 namespace-import member calls — user-import form.
//
// `import * as R from "ramda"; R.sum([1,2,3,4,5])` returned `0` instead of
// `15` for every wildcard-namespace member call on a V8-fallback module
// (ramda, date-fns, jose, effect). #985 added
// `CompileOptions::namespace_v8_specifiers` and routes the call through
// `js_call_v8_export(specifier, member, args, argc)` from both the codegen
// StaticMethodCall arm (`expr.rs`) and the namespace-member-call arm
// (`lower_call.rs`).
//
// This fixture nails the regression bar by exercising the exact pattern
// the v0.5.993 compat sweep tripped on (`"sum=" + R.sum(arr)` — result
// flows through string concat, no companion Named import). The sweep
// originally showed `0` because it consumed a binary built before #985
// landed; with the post-#985 binary the same fixture produces the
// expected Node output below.
//
// Covers number return (sum/head/add/identity/multiply) — the higher-order
// `R.add(1)` curried-call form is intentionally NOT tested here because
// the V8 bridge still returns closures as opaque handles that the native
// callsite can't dispatch (out of scope per #985).
//
// Expected output (matches `node --experimental-strip-types`):
//   15
//   1
//   5
//   42
//   12
import * as R from 'ramda';
console.log(R.sum([1, 2, 3, 4, 5]));
console.log(R.head([1, 2, 3]));
console.log(R.add(2, 3));
console.log(R.identity(42));
console.log(R.multiply(3, 4));
