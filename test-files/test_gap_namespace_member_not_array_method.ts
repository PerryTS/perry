// #321 / #24: a method call on a *module namespace* whose member happens to be
// named `map` / `filter` / `find` / ... was mis-lowered to the Array intrinsic
// (`NS.map(x, f)` → `Expr::ArrayMap { array: NS, callback: x }`), so it
// returned `[]` and never called the member. This is exactly effect's
// `import * as Effect from "effect/Effect"; Effect.map(eff, f)` — where
// `Effect.map` is `export const map = core.map`.
//
// Fix: skip the array-intrinsic fold when the receiver identifier is a module
// namespace import (`namespace_import_locals`). Real imported arrays (named
// value imports, or `NS.items.map(...)`) still fold.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

import * as NS from "./_helpers/ns_member_fns_mod.ts";
import { items } from "./_helpers/ns_member_fns_mod.ts";

// (1) namespace member functions named like array methods — must call the
//     member, NOT lower to an array op.
console.log("NS.map(5, *2):", NS.map(5, (n) => n * 2)); // 10
console.log("NS.filter(5, >3):", NS.filter(5, (n) => n > 3)); // true
console.log("NS.find(7):", NS.find(7)); // 8

// (2) a real array reached *through* the namespace (`NS.items`) still maps.
console.log("NS.items.map(+1):", NS.items.map((n) => n + 1).join(",")); // 11,21,31

// (3) a directly-imported array (named value import) still array-maps — guard
//     must not over-fire on non-namespace imports.
console.log("items.filter(>15):", items.filter((n) => n > 15).join(",")); // 20,30
console.log("items.map(*2):", items.map((n) => n * 2).join(",")); // 20,40,60
