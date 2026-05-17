// Regression for #901 — pino smoke threw
//
//     TypeError: Cannot read properties of undefined (reading 'ASC')
//
// during `import pino from "pino"` at v0.5.941. The trigger is two
// same-file default imports of *different* modules.
//
// Perry's CJS-to-ESM wrap at `crates/perry/src/commands/compile/cjs_wrap.rs`
// hoists every `require("./X")` to an `import _req_N from "./X"`, so
// pino.js's
//
//     const { DEFAULT_LEVELS, SORTING_ORDER } = require('./lib/constants')
//     const { createArgsNormalizer, ... } = require('./lib/tools')
//
// became
//
//     import _req_9 from './lib/constants';
//     import _req_10 from './lib/tools';
//
// Both default imports lowered through HIR's
// `crates/perry-hir/src/lower.rs`'s `ImportSpecifier::Default` arm,
// which called `ctx.register_imported_func(local, "default")` —
// recording the literal string `"default"` as the imported name for
// BOTH `_req_9` and `_req_10`. Subsequent value-position references
// (e.g. inside the IIFE: `require('./lib/constants')` returns
// `_req_9`) lowered via `lookup_imported_func(name)` to
// `Expr::ExternFuncRef { name: "default" }` regardless of which
// default the source code referenced.
//
// The codegen's `import_function_prefixes` map at
// `crates/perry-codegen/src/codegen.rs` is keyed by the
// `ExternFuncRef.name`; it's a flat `HashMap<String, String>`. Two
// default imports in the same file both inserted `"default" →
// prefix_X`, and HashMap insert overwrites — whichever module's
// prefix landed last won. Both `_req_9` and `_req_10` then routed to
// the SAME source module's `perry_fn_<src>__default` symbol at
// codegen.
//
// Pino's IIFE thus saw:
//   - `require('./lib/constants')` → returned `_req_10` (= tools'
//     exports object: `{ createArgsNormalizer, asChindings, ... }`).
//   - `const { SORTING_ORDER } = require('./lib/constants')` →
//     destructured a non-existent `SORTING_ORDER` from tools'
//     namespace → bound `undefined`.
//   - `levelComparison: SORTING_ORDER.ASC` → reading `.ASC` on
//     `undefined` → the spec-mandated TypeError above.
//
// Two-part fix:
//
//   1. `crates/perry-hir/src/lower.rs` —
//      `ImportSpecifier::Default` now calls
//      `register_imported_func(local, local)` (identity registration)
//      instead of `register_imported_func(local, "default")`. The
//      local name is unique per import site, so every `_req_N`
//      reference lowers to `ExternFuncRef { name: "_req_N" }` —
//      distinct keys, no map collision.
//
//   2. `crates/perry/src/commands/compile.rs` — after each
//      `import_function_prefixes` insert pair, when the spec is
//      `Default`, also insert `local_name → <resolved_origin_name>`
//      (defaulting to `"default"`) into
//      `import_function_origin_names`. The codegen's
//      `import_origin_suffix()` lookup at every
//      `perry_fn_<src>__<suffix>` construction site then resolves
//      `_req_N` → "default" so the emitted symbol matches what the
//      source module actually defines (`perry_fn_<src>__default`).
//
// This pure-TS minimal repro exercises the same HIR + CLI path that
// the CJS wrap output drives: two `import X from "./mod"` defaults
// in one file, two different modules each defining `export default
// <obj>`, and the importer reading a per-default property. Pre-fix
// both `a.value` and `b.value` printed `BBB` (the SECOND module's
// value — its prefix overwrote the first under the shared "default"
// key). Post-fix `a.value` is `AAA`, `b.value` is `BBB`. The pino
// CJS-wrap shape collapses to exactly this pattern after `cjs_wrap`
// hoists the requires, so this test exercises the same code path
// without dragging in pino's downstream Node-API dependencies
// (`tracingChannel`, `MAX_STRING_LENGTH`, etc.).

import a from "./test_issue_pino_sorting_order_undefined_a";
import b from "./test_issue_pino_sorting_order_undefined_b";

console.log("a.value:", a.value);
console.log("b.value:", b.value);
console.log("a.tag:", a.tag);
console.log("b.tag:", b.tag);

// Mirror pino's failing access pattern: read a property on the
// destructured default-import's own object. Pre-fix both destructures
// collided so reading `a.tag` would hit `b`'s tag (or undefined for
// keys only the first module defined). Post-fix both reads resolve
// to their own module's object.
const { value: aValue } = a;
const { value: bValue } = b;
console.log("destructured aValue:", aValue);
console.log("destructured bValue:", bValue);
