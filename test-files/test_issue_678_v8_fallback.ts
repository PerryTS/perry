// Issue #678: when a TS module imports a name from a module that lands on
// the V8 fallback (e.g. yoga-layout pulled in by ink, or any `.js` outside
// `perry.compilePackages`), the codegen used to emit a bare
// `perry_fn_<src>__<name>` extern call against a symbol that doesn't exist
// — the V8 module never emits native symbols. The linker then failed with
// `Undefined symbols: _perry_fn_..._<name>`.
//
// This regression exercises the V8 fallback end-to-end: a `.js` module
// (V8-routed because it isn't in `compilePackages`), imported by a TS
// entry, called with normal arguments. The HIR-level
// `transform_js_imports` rewrites the obvious shapes to `JsCallFunction`,
// and the codegen-level `js_call_v8_export` bridge handles anything left
// over so the link always succeeds.
//
// Acceptance: byte-for-byte parity with `node --experimental-strip-types`.

import { greet, add } from "./fixtures/issue_678_v8/mod.js";

const g = greet("perry");
console.log("greet:", g);

const sum = add(2, 3);
console.log("add:", sum);
