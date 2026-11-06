// #321 / #27: arrays did not expose `Symbol.iterator` as a property.
// `Symbol.iterator in arr` was `false` and `arr[Symbol.iterator]` read back a
// *number* (the symbol value `fptosi`'d to a garbage index on the array
// numeric fast path). effect's `Predicate.isIterable(x)` is
// `Symbol.iterator in x`, so `isIterable([...])` was false — which made
// `Effect.all`'s predicate-`dual` `forEach` take its data-last branch and
// return a function instead of the combined effect.
//
// Fix: the symbol resolver exposes the array's iterator for the well-known
// iterator symbol (a bound callable), and the array IndexGet fast path routes
// symbol keys to the resolver instead of the numeric load. So
// `Symbol.iterator in arr` is true and `arr[Symbol.iterator]` is a function.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

const arr = [1, 2, 3];

// (1) presence via `in` (effect's isIterable shape).
console.log("Symbol.iterator in arr:", Symbol.iterator in arr);
console.log("Symbol.iterator in []:", Symbol.iterator in ([] as number[]));

// (2) the property is a function.
console.log("typeof arr[Symbol.iterator]:", typeof arr[Symbol.iterator]);

// (3) a hasProperty-style guard (effect's Predicate.hasProperty).
const hasProperty = (self: any, key: any): boolean =>
  (typeof self === "object" || typeof self === "function") &&
  self !== null &&
  key in self;
console.log("hasProperty(arr, Symbol.iterator):", hasProperty(arr, Symbol.iterator));

// (4) a string key is still absent (guard against over-broad `in`).
console.log("'foo' in arr:", "foo" in arr);
console.log("0 in arr:", 0 in arr);

// (5) for-of and spread still work (no regression to array iteration).
let sum = 0;
for (const n of arr) sum += n;
console.log("for-of sum:", sum);
console.log("spread:", [...arr].join(","));

// (6) a plain object without Symbol.iterator reports false.
console.log("Symbol.iterator in {}:", Symbol.iterator in ({} as any));
