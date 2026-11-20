// epic #1785 / #1758: a module-level closure that forward-references a
// module-level `const`, where a *nested* local of the same name exists in
// another function.
//
// The module-level forward-declaration pass pre-registers a LocalId for the
// module `const`. The bug: a nested local of the same name
// (`function helper() { const zipWith = ... }`) hit the same
// `pre_registered_module_vars` reuse branch and CONSUMED the module var's id —
// so the nested local and the module binding shared one id, the real module
// value landed on a fresh id, and the sibling closure (`merge`) that
// forward-referenced the module name resolved to the wrong (uninitialised)
// slot → `value is not a function`.
//
// effect's `layer.merge` (`dual(2, (self, that) => zipWith(self, that, ...))`,
// referencing the module `zipWith` exported later at L1191) broke this way
// because a *local* `zipWith` (L1180, inside another fn) precedes it — which
// blocked the entire `import { Effect } from "effect"` barrel.
//
// Fix: gate the `pre_registered_module_vars` id-reuse on MODULE scope
// (`scope_depth == 0 && inside_block_scope == 0`); a nested local gets a fresh
// id. Compared byte-for-byte against `node --experimental-strip-types`.

// (1) the core shape: merge forward-refs module `zipWith`; helper has a local.
const merge = (self: number, that: number) => zipWith(self, that);
function helper() {
  const zipWith = 5; // local shadow, declared BEFORE the module const
  return zipWith;
}
const zipWith = (a: number, b: number) => a + b;
console.log("(1) merge(1,2):", merge(1, 2));
console.log("(1) helper():", helper());

// (2) via a dual-style indirection (effect's exact shape).
function dual(_arity: number, body: any): any {
  return function (a: any, b: any) {
    if (arguments.length >= 2) return body(a, b);
    return (s: any) => body(s, a);
  };
}
const combine = dual(2, (self: any, that: any) => joiner(self, that));
function other() {
  const joiner = "shadow";
  return joiner;
}
const joiner = (a: any, b: any) => `${a}+${b}`;
console.log("(2) combine(x,y):", combine("x", "y"));
console.log("(2) combine(y) curried:", combine("y")("x"));
console.log("(2) other():", other());

// (3) the nested local still works independently (not clobbered).
function scope3() {
  const val = 99;
  return val;
}
const val = (n: number) => n * 2;
console.log("(3) scope3():", scope3(), "| module val(21):", val(21));
