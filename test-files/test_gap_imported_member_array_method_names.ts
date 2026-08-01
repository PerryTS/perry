// #7154: a NAMED value import whose member happens to be called `map` /
// `filter` / `slice` / `keys` / ... was folded to the Array intrinsic on the
// strength of the METHOD NAME alone.
//
// `try_imported_array_methods` built an `Expr::ExternFuncRef` for any imported
// identifier and then matched on the method name, so
//
//     import { z } from "zod";
//     z.map(keyType, valueType)          // zod: export function map(k, v)
//
// lowered to `Expr::ArrayMap { array: z, callback: keyType }` — the second
// argument was dropped and argument #1 landed in Array#map's callback slot, so
// the binary threw `TypeError: object is not a function` out of
// `js_validate_array_callback` instead of building a `ZodMap`. Only `join`
// (#420, drizzle's `sql.join(list)`) and `sort` (semver's `sort(list)`) had
// ever been given the "is it actually an array?" guard; the other ~18 arms
// rewrote the call unconditionally. The #321 namespace guard did not cover this
// because `z` is a named import, not `import * as z`.
//
// Fix: fold only when the imported binding is statically typed as an array;
// otherwise fall through to the generic call path, which invokes the member
// itself with every argument.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

import { ns, arr, nums } from "./_helpers/imported_member_array_names_mod.ts";

// (1) An imported non-array whose members are NAMED like array methods must
//     call its own members, with all arguments, and return their values.
console.log("ns.map:", ns.map(1, 2));
console.log("ns.filter:", ns.filter(1, 2));
console.log("ns.find:", ns.find(1));
console.log("ns.forEach:", ns.forEach(1));
console.log("ns.reduce:", ns.reduce(1, 2));
console.log("ns.reduceRight:", ns.reduceRight(1, 2));
console.log("ns.slice:", ns.slice(1, 2));
console.log("ns.includes:", ns.includes(1));
console.log("ns.indexOf:", ns.indexOf(1));
console.log("ns.join:", ns.join(1));
console.log("ns.sort:", ns.sort(1));
console.log("ns.keys:", ns.keys());
console.log("ns.values:", ns.values());
console.log("ns.entries:", ns.entries());
console.log("ns.flat:", ns.flat());
console.log("ns.with:", ns.with(1, 2));
console.log("ns.toSorted:", ns.toSorted(1));
console.log("ns.toReversed:", ns.toReversed());
console.log("ns.toSpliced:", ns.toSpliced(1, 2));

// (2) Real imported arrays must still behave like arrays — the guard must not
//     over-fire in the other direction.
console.log("arr.map:", JSON.stringify(arr.map((x) => x * 2)));
console.log("arr.filter:", JSON.stringify(arr.filter((x) => x > 1)));
console.log("arr.find:", JSON.stringify(arr.find((x) => x === 2)));
console.log("arr.slice:", JSON.stringify(arr.slice(1)));
console.log("arr.includes:", JSON.stringify(arr.includes(2)));
console.log("arr.indexOf:", JSON.stringify(arr.indexOf(2)));
console.log("arr.join:", JSON.stringify(arr.join("-")));
console.log("arr.reduce:", JSON.stringify(arr.reduce((a, b) => a + b, 0)));
console.log("arr.reduceRight:", JSON.stringify(arr.reduceRight((a, b) => a + b, 0)));
console.log("arr.keys:", JSON.stringify([...arr.keys()]));
console.log("arr.values:", JSON.stringify([...arr.values()]));
console.log("arr.entries:", JSON.stringify([...arr.entries()]));
console.log("nums.toSorted:", JSON.stringify(nums.toSorted((a, b) => b - a)));
console.log("nums.toReversed:", JSON.stringify(nums.toReversed()));
console.log("nums.toSpliced:", JSON.stringify(nums.toSpliced(1, 1)));
console.log("nums.with:", JSON.stringify(nums.with(0, 99)));
console.log("nums.flat:", JSON.stringify(nums.flat()));
console.log("nums.sort:", JSON.stringify([...nums].sort((a, b) => b - a)));
let acc = "";
arr.forEach((x) => {
  acc += x;
});
console.log("arr.forEach:", JSON.stringify(acc));
