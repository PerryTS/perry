// #1840: for-of and spread over a non-generator class `[Symbol.iterator]()`
// must drive the runtime iterator protocol (`expr[Symbol.iterator]().next()`
// loop), not fall through to the indexed-array path.
//
// Sibling of test_gap_class_symbol_iterator.ts (#1838 — member access / `in`
// / `yield*` resolution). The lowering paths are different: for-of routes
// untyped receivers through `js_for_of_to_array` → `js_get_iterator`, and
// spread (`[...x]`) routes through `js_array_clone_for_spread` →
// `js_iterator_to_array(js_get_iterator(...))`. This test locks both paths
// so a future refactor of either can't silently zero-iterate again.
//
// Validated byte-for-byte against `node --experimental-strip-types`.

class Range {
  n: number;
  constructor(n: number) {
    this.n = n;
  }
  [Symbol.iterator]() {
    let i = 0;
    const n = this.n;
    return {
      next: () => (i < n ? { value: i++, done: false } : { value: undefined, done: true }),
    };
  }
}

// --- spread over a class instance ---
console.log("class spread:", [...new Range(3)]);

// --- for-of over a class instance ---
const fo: number[] = [];
for (const x of new Range(3)) fo.push(x);
console.log("class for-of:", fo);

// --- single-statement (brace-less) for-of body ---
const fo1: number[] = [];
for (const x of new Range(2)) fo1.push(x * 10);
console.log("class for-of (no braces):", fo1);

// --- spread over a class instance held in an untyped local ---
const u: any = new Range(2);
console.log("any spread:", [...u]);
const fo2: number[] = [];
for (const x of u) fo2.push(x);
console.log("any for-of:", fo2);

// --- spread over a plain object literal with [Symbol.iterator] ---
const objIter = {
  [Symbol.iterator]() {
    let i = 0;
    return {
      next: () => (i < 3 ? { value: i++, done: false } : { value: undefined, done: true }),
    };
  },
};
console.log("obj spread:", [...(objIter as any)]);
const fo3: number[] = [];
for (const x of objIter as any) fo3.push(x);
console.log("obj for-of:", fo3);

// --- inherited class iterator drives for-of + spread ---
class Sub extends Range {}
console.log("sub spread:", [...new Sub(2)]);
const fo4: number[] = [];
for (const x of new Sub(2)) fo4.push(x);
console.log("sub for-of:", fo4);

console.log("ALL 1840 ITERATOR FOR-OF + SPREAD TESTS PASSED");
