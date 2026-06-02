// #39 (#321): a data-last (pipeable) namespace export named `sort` —
// `export const sort = dual(2, body)`, exactly effect's `Array.sort` — was
// mis-lowered to the `js_array_sort` intrinsic when called as `NS.sort(cmp)`.
// `sort` (and `reverse`/`entries`/`keys`/`values`/`splice`/`fill`/...) are NOT
// in the array-method "overlapping" set, so the existing imported-array
// namespace guard didn't cover them; the call fell through to
// `try_array_only_methods`, whose `"sort" if !args.is_empty()` arm folded
// `NS.sort(cmp)` into `Expr::ArraySort { array: NS, comparator: cmp }`. The
// compiled binary then ran `js_array_sort(NS, cmp)`, which returned the
// namespace object itself (`typeof` "object") instead of the curried
// `(self) => sorted` closure — so `pipe(bounds, NS.sort(cmp), ...)` threw
// "value is not a function". This surfaced in effect's metric histogram path
// (`MetricRegistryImpl.getHistogram` → `Arr.sort(number.Order)`) at fiber exit,
// breaking every `Effect.runSync(...)` fiber case.
//
// Fix: in `try_array_only_methods`, bail (treat the receiver as not-an-array)
// for ALL method names when the receiver identifier is a module namespace
// import (`namespace_import_locals`), so the call dispatches the namespace
// export and the data-last curried form is returned.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

import * as NS from "./_helpers/ns_member_fns_mod.ts";

// Data-last form: one arg returns a unary closure.
const sortFn = NS.sort((a: number, b: number) => (a === b ? 0 : a < b ? -1 : 1));
console.log("typeof NS.sort:", typeof NS.sort); // function
console.log("typeof sortFn:", typeof sortFn); // function
console.log("sortFn([3,1,2]):", JSON.stringify(sortFn([3, 1, 2]))); // [1,2,3]

// Data-first form: two args sort directly.
console.log(
  "NS.sort([5,4,6], asc):",
  JSON.stringify(NS.sort([5, 4, 6], (a: number, b: number) => (a === b ? 0 : a < b ? -1 : 1)))
); // [4,5,6]

// Used inside a pipe, exactly like effect's metric histogram path.
const pipe = (a: any, ab?: any, bc?: any): any => {
  if (bc) return bc(ab(a));
  if (ab) return ab(a);
  return a;
};
const result = pipe(
  [30, 10, 20],
  NS.sort((a: number, b: number) => (a === b ? 0 : a < b ? -1 : 1)),
  (arr: number[]) => arr.map((n) => n / 10)
);
console.log("pipe(sort,map):", JSON.stringify(result)); // [1,2,3]
