// Regression test for issue #967: default-import-as-value call dispatch.
//
// When `function add(a,b){…}; export default add;` is compiled as a native
// package (compilePackages), the consumer's `const fn = add; fn(2,3)` shape
// previously resolved through a no-op `__perry_wrap_perry_fn_<src>__default`
// emitted by codegen's sub-bug-B alias loop — which short-circuited any
// closure-based call dispatch and returned `undefined`.
//
// This test exercises the same shape via inline package modules so it can
// run inside the gap suite without npm install. The fix lives in
// crates/perry-codegen/src/codegen.rs at the
// `local == exported && !func_by_local_name.contains_key(...)` branch:
// when `hir.exported_functions` resolves the name back to a real HIR
// function, emit a forwarding wrapper instead of the no-op.
//
// Setup: cross-module function callable via every value-position shape
// (alias, parameter, closure capture, direct call) — each must return
// the actual function result, not `undefined`.

import add from "./test_default_import_as_value_helper.ts";

// 1. Alias to local const + call through local.
const fn = add;
console.log("alias:", fn(2, 3));

// 2. Pass as argument; called inside callee.
function apply(f: (a: number, b: number) => number, a: number, b: number): number {
  return f(a, b);
}
console.log("apply:", apply(add, 10, 20));

// 3. Closure captures the imported default.
function makeCaller(f: (a: number, b: number) => number) {
  return function (a: number, b: number) {
    return f(a, b);
  };
}
const caller = makeCaller(add);
console.log("closure:", caller(7, 8));

// 4. Direct call — still works post-fix.
console.log("direct:", add(2, 3));
