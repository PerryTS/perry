// Followup to #957 / PR #959 — `Function('return this')()` + bare
// `RegExp(pattern[, flags])` recognisers.
//
// Real lodash 4 builds two stubborn module-init expressions that, before
// this commit, both threw `TypeError: value is not a function` at module
// init and left `_` undefined:
//
//   1. `var root = freeGlobal || freeSelf || Function('return this')();`
//      The inner `Function('return this')` call was a `Call { callee:
//      GlobalGet(0), args: [String("return this")] }` (the `Function`
//      ident resolved to the no-resolution sentinel). The double-call
//      dispatched through `js_closure_call1` on a null closure handle
//      → throw.
//
//   2. `var reHasEscapedHtml = RegExp(reEscapedHtml.source);` (and
//      ~5 sibling `RegExp(...)` bare-call sites). The `RegExp`
//      ident lowered the same way as `Function` (GlobalGet(0)
//      sentinel), so the call form crashed identically.
//
// Both are now AST-shape recognised at HIR lower time. (1) folds to
// `Expr::GlobalThisExpr` (lazy `js_get_global_this()` singleton — same
// object `globalThis[X] = V` already writes to under #611). (2) folds
// to a new `Expr::RegExpDynamic { pattern, flags }` that lowers to the
// same `js_regexp_new(pattern_handle, flags_handle)` the static `/foo/g`
// arm uses. `new RegExp(<non-literal>)` was rerouted the same way (the
// literal-only `Expr::RegExp` arm in `expr_new.rs` previously fell
// through to generic class-instantiation when the pattern was an Expr).

// ── (1) `Function('return this')()` evaluates to the global singleton.
const g: any = Function("return this")();
console.log("global_is_object:", typeof g === "object" && g !== null);
// Identity: the runtime caches the singleton so two double-call sites
// resolve to the same JS object.
console.log("global_identity:", Function("return this")() === g);
// Whitespace + trailing semicolon variants the recognizer also accepts.
console.log("global_with_semi:", typeof Function("return this;")() === "object");
console.log("global_with_ws:", typeof Function("  return this  ")() === "object");

// Writes through the singleton are visible on subsequent reads.
(g as { [k: string]: number })["__perry_test_957_b"] = 7;
console.log(
    "global_write_then_read:",
    (g as { [k: string]: number })["__perry_test_957_b"],
);

// ── (2) `RegExp(pattern)` + `RegExp(pattern, flags)` bare function-call.
const r1 = RegExp("ab+c");
console.log("regexp_call_basic:", r1.test("xabbbc"));

const r2 = RegExp("ab+c", "g");
console.log("regexp_call_with_flags:", r2.test("xabbbc"));

// Dynamic pattern from another variable (the lodash shape).
const pat = "ab+c";
const r3 = RegExp(pat);
console.log("regexp_call_dynamic_pattern:", r3.test("xabbbc"));

// The literal lodash shape — `RegExp(otherRegex.source)` — the exact
// AST that hit the GlobalGet(0) callee bug in the original repro.
const litRegex = /xyz/g;
const r4 = RegExp(litRegex.source);
console.log("regexp_from_source:", r4.test("xyz"));

// `new RegExp(<non-literal>)` rerouted through the same dynamic arm.
const r5 = new RegExp(pat);
console.log("new_regexp_dynamic:", r5.test("xabbbc"));

const r6 = new RegExp(pat, "g");
console.log("new_regexp_dynamic_with_flags:", r6.test("xabbbc"));

// `new RegExp(literal)` (the path that already worked via `Expr::RegExp`)
// keeps working — guard against accidentally regressing.
const r7 = new RegExp("xyz");
console.log("new_regexp_literal:", r7.test("xyz"));
