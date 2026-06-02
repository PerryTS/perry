// Regression for #388: `obj?.method()` short-circuit was using strict
// equality (`obj === null`) when it should use loose equality
// (`obj == null`). The spec is clear that `?.` short-circuits on both
// null AND undefined.
//
// Pre-fix: `map.get(missingKey)?.has?.(1) ?? false` returned the
// literal string `[object Object]` because:
//   1. `map.get(missingKey)` returned `undefined`
//   2. `undefined === null` evaluated to `false`
//   3. The else branch took `undefined.has(1)` which produced
//      garbage / `[object Object]`
//   4. `?? false` saw a truthy `[object Object]` and returned it
//
// Post-fix: lower.rs's OptChain → Conditional rewrite uses
// `CompareOp::LooseEq` so the condition `obj == null` is true for
// both `null` and `undefined`, taking the `then_expr: Undefined`
// branch correctly.
const map = new Map<number, string>();
map.set(1, "one");

const array = [1, 2, 3];

console.log("map direct has", map.has(1));
console.log("map optional has", map?.has(1));
console.log("map get optional has", ({ m: map }).m?.has(1));
console.log("missing map optional has", map.get(2)?.has?.(1) ?? false);
console.log(
  "missing nested optional has",
  ({ m: new Map<number, Map<number, string>>() }).m.get(2)?.has(1) ?? false,
);
console.log("array direct join", array.join(","));
console.log("array optional join", array?.join(","));
console.log("nested optional join", ({ a: array }).a?.join(","));
