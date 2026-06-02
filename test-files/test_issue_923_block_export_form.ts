// Issue #923 — `const x = ...; export { x };` (block-export form) failed
// to link with `undefined reference to __perry_wrap_perry_fn_<src>__pool`,
// while the inline-export form `export const x = ...` linked fine. The
// two are interchangeable in ECMAScript; Perry should treat them as such.
//
// Root cause is the same family as #836 — the block-export form lowers
// to `Export::Named { local: "pool", exported: "pool" }` for a non-
// function local, which the existing wrapper-emission loops in codegen
// skipped (the `local != exported` rename loop and the
// `local == exported && local is namespace import` no-op loop both
// missed the case where `local == exported` and `local` is a plain
// module-level const). The fix extends the codegen no-op-wrapper
// emission to also cover plain const-locals exported via a block.
//
// The bar for this regression test is purely link-time: the binary
// must compile, link, and run. The runtime value is also asserted so
// we know the producer-side getter (`perry_fn_<src>__pool`) and
// closure wrapper still agree on what the binding points at.

import { pool, poolName } from "./fixtures/issue_923_pkg/producer.ts";

// Read `pool` as a property — exercises the value-getter symbol.
console.log("pool.name:", pool.name);
console.log("pool.tag:", pool.tag);

// Read `pool` as a value — exercises the closure-wrapper symbol
// (`__perry_wrap_perry_fn_<src>__pool`), which was the missing
// symbol called out in the link error.
function describe(obj: any): string {
  return typeof obj;
}
console.log("typeof pool:", describe(pool));

// Sibling export still works (sanity check that we didn't break the
// regular `export function` path).
console.log("poolName():", poolName());

console.log("done");
