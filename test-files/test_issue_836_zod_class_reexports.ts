// Issue #836 — cross-module value re-exports unresolved at link time,
// surfaced by the `perry.compilePackages: ["zod"]` repro. Two sub-bugs
// landed in this PR:
//
//   A) `export const $ZodCheck = ...` — the producer side ran the
//      exported name through `sanitize()`, which rewrites `$` to `_`,
//      so the value-getter symbol was emitted at
//      `perry_fn_<src>___ZodCheck`. The consumer side built the symbol
//      with `import_origin_suffix()`, which returns the name VERBATIM,
//      so it referenced `perry_fn_<src>__$ZodCheck` and link-failed.
//      Same mismatch hits `__perry_wrap_perry_fn_<src>__$ZodCheck`,
//      built when the binding is passed as a value.
//
//   B) `import * as z from "./external.ts"; export { z };` — the
//      `Export::Named { local: "z", exported: "z" }` entry was skipped
//      by every wrapper-emission loop (`local==exported` AND `z` is a
//      namespace import, not a HIR function). Consumers that read `z`
//      as a value link-failed on
//      `__perry_wrap_perry_fn_<index_ts>__z`.
//
// This file exercises the link-side fixes for both. Runtime semantics
// of `import * as z; export { z };` are a separate concern — the
// namespace-import-as-re-export currently materializes `z` as
// `undefined` at runtime, tracked separately. The bar for this PR is
// that the link succeeds.
//
// Out of scope (separate bug — needs a follow-up PR):
//   * `export default function () {...}` (anonymous default) is dropped
//     entirely by the HIR lowerer today, so `_perry_fn_<src>__default`
//     is never emitted. zod's `v4/locales/en.ts` triggers this; the
//     remaining `_perry_fn_<en_ts>__default` undefined-symbol after
//     this PR comes from that path.

import { $ZodCheck, $ZodCheckStringFormat } from "./fixtures/issue_836_pkg/external.ts";
import { z } from "./fixtures/issue_836_pkg/index.ts";

// Sub-bug A as a property read of a `$`-prefixed const re-exported
// through a barrel. Pre-fix: linker couldn't find
// `_perry_fn_..._checks_ts__$ZodCheck`.
console.log("ZodCheck.kind:", $ZodCheck.kind);
console.log("ZodCheck.validate(5):", $ZodCheck.validate(5));
console.log("ZodCheck.validate(-1):", $ZodCheck.validate(-1));

console.log("ZodCheckStringFormat.kind:", $ZodCheckStringFormat.kind);
console.log("ZodCheckStringFormat.validate('hi'):", $ZodCheckStringFormat.validate("hi"));

// Sub-bug A taken as a VALUE — exercises the
// `__perry_wrap_perry_fn_<src>__$ZodCheck` closure-wrapper alias.
// Pre-fix the linker missed the wrapper symbol.
function describe(obj: any): string {
  return typeof obj;
}
console.log("typeof ZodCheck:", describe($ZodCheck));

// Sub-bug B — `import * as z; export { z };`. Read `z` as a value so
// the codegen takes the `js_closure_alloc_singleton(@__perry_wrap_...__z)`
// path. Pre-fix the linker missed the wrapper symbol. The actual
// `typeof z` Perry reports today is "function" (not "object" like
// node) because namespace re-exports through `export { z }` still
// materialize as a closure handle — that's a separate runtime bug,
// out of scope here. We just need the load to NOT segfault.
const z_val = z;
console.log("z bound:", z_val !== null && z_val !== undefined ? "yes" : "no");

console.log("done");
