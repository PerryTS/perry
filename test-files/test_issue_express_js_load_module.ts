// Regression test for express V8-fallback `[js_load_module] FAILED to load`.
//
// Symptom on main pre-fix (express smoke):
//
//   $ perry main.ts -o out && ./out
//   [js_load_module] FAILED to load '/.../debug/src/index.js':
//     Cannot find module 'supports-color' in node_modules
//   TypeError: value is not a function
//
// Root cause: Perry's CJS→ESM wrapper (`wrap_commonjs` in
// `crates/perry-jsruntime/src/modules.rs`) hoists every `require("...")`
// call to a top-of-file `import * as _req_N from "..."` so the wrapped
// IIFE can do synchronous lookups. That static import means a bare
// specifier that can't be resolved in node_modules (a missing OPTIONAL
// peer dep like `supports-color` in `debug`, gated by `try/catch`)
// aborts module loading at instantiation time instead of throwing a
// catchable JS error at the require() callsite — opposite of Node.js
// semantics, where `require()` of an absent optional dep throws
// `MODULE_NOT_FOUND` synchronously and can be swallowed.
//
// Fix: `resolve()` substitutes a synthetic `perry-missing:<spec>` stub
// specifier for unresolvable bare imports; `load()` returns an ESM
// module that exports `__perry_missing: true` + `__perry_specifier`;
// `wrap_commonjs` emits a per-require-case marker check that throws
// `MODULE_NOT_FOUND` from inside the wrapper's `require()` body so
// user-level `try/catch` catches it. Top-level user imports (entry-point
// `js_load_module` calls) still hard-error so genuinely missing modules
// aren't silently masked.
//
// This test exercises the soft-throw path end-to-end via a fixture .js
// module that wraps `require("non-existent-optional-pkg-xyz")` in
// try/catch and reports whether it landed on the catch arm.

import { colorLevel, label } from "./fixtures/issue_express_js_load_module/optional_dep.js";

console.log("label:", label);
console.log("colorLevel:", colorLevel);
console.log("OK");
