// Issue #678: consumer imports through a `export { default as X } from
// "./inner.ts"` re-export rename. Pre-fix the codegen formed
// `perry_fn_<inner>__renamedArrow` even though `inner.ts` emits the
// symbol under `default`, and the linker failed with `Undefined
// symbols: _perry_fn_..._renamedArrow`.
//
// The fix tracks the origin export name across the re-export chain so
// every `perry_fn_<src>__<suffix>` construction site picks the correct
// suffix; this file exercises the call-site, the value-as-arg site, and
// the plain non-rename path.

import { renamedArrow, renamedPlain } from "./fixtures/issue_678_pkg/index.ts";

// Call site through the rename — origin export is `default` (a const
// arrow), consumer wrote `renamedArrow`.
console.log("arrow-call:", renamedArrow(41));

// Value-as-arg shape: hand the imported binding to another function so
// the codegen takes the `js_closure_alloc_singleton(@__perry_wrap_*)`
// path. Without the origin-name plumb-through, the wrapper symbol
// referenced here would also point at `__renamedArrow` and link-fail.
function apply(fn: (n: number) => number, value: number): number {
  return fn(value);
}
console.log("arrow-as-value:", apply(renamedArrow, 99));

// Sanity check the non-default rename path: `export { plain as
// renamedPlain }` still resolves to `perry_fn_<inner>__plain` rather
// than `__renamedPlain`.
console.log("plain-call:", renamedPlain(5));

console.log("done");
