// Issue #321 — `Effect.succeed(42)` returning `undefined` on the native
// `perry.compilePackages: ["effect"]` path.
//
// REPRO SHAPE: a barrel that does `export * as Effect from "./Effect.js"`
// and a consumer that does `import { Effect } from "<pkg>"; Effect.method(...)`.
// The consumer's `Effect.succeed(42)` lowers to
// `StaticMethodCall { class_name: "Effect", method_name: "succeed" }`.
//
// PRE-FIX BUG: the StaticMethodCall arm in `crates/perry-codegen/src/expr.rs`
// emitted a direct call `perry_fn_<src>__succeed(arg)` against a 0-arg
// getter symbol (because `succeed = (v) => new EffectInst(v)` is var-shape:
// the source emits a getter returning the closure, not the body). The
// arg was silently dropped and the caller saw the closure pointer
// itself — `typeof Effect.succeed(42) === "function"`. Effect's
// `runSync(program)` then read `program._tag` off a closure pointer and
// the runtime threw `Cannot read properties of undefined (reading '_tag')`.
//
// POST-FIX: the StaticMethodCall arm classifies the member as variable-shaped,
// fetches its closure via the zero-arg getter, and dispatches through
// `js_closure_callN` with the user's args. `Effect.succeed(42)` now
// returns the EffectInst, `_tag === "Commit"`, and `runSync` returns 42.
//
// This file is byte-compared against Node — see `node --experimental-strip-types`.
// Run from the fixture directory under `test-files/fixtures/`:
//   cd test-files/fixtures/issue_321_named_namespace_reexport
//   perry ../../test_issue_321_named_namespace_reexport.ts -o out && ./out

import { Effect } from "mini-effect";
const p = Effect.succeed(42);
console.log("typeof p:", typeof p);
console.log("p?._tag:", (p as any)?._tag);
console.log("runSync:", Effect.runSync(p));
