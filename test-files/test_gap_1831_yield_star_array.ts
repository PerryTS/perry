// #1831 (array sub-case): `yield*` over a plain Array must iterate the
// values, not infinite-loop over an empty/undefined iterator result.
//
// The object-iterable sub-case (`yield* { [Symbol.iterator]() {…} }`) was
// fixed in #1839; arrays were intentionally left as a follow-up because
// `js_get_iterator` returned arrays unchanged. With the runtime
// `ARRAY_ITERATOR_CLASS_ID` from #321 in place, `js_get_iterator(arr)` can
// now return `array_values_iter(arr)` — a real `.next()`-bearing iterator
// object that `js_native_call_method` dispatches.
//
// Validated byte-for-byte against `node --experimental-strip-types`.

function* g() {
  yield* [1, 2, 3] as any;
  yield 4;
}
console.log("array:", [...g()].join(","));

// Object iterable still works (#1839 regression check).
const iterable = {
  [Symbol.iterator]() {
    let i = 0;
    return {
      next: () => (i < 2 ? { value: i++, done: false } : { value: undefined, done: true }),
    };
  },
};
function* h() {
  yield* iterable as any;
  yield 9;
}
console.log("object:", [...h()].join(","));

// Mixed: arrays + object iterable + nested generator.
function* inner() {
  yield "a";
  yield "b";
}
function* mixed() {
  yield* [1, 2] as any;
  yield* inner();
  yield* ["x", "y"] as any;
  yield "end";
}
console.log("mixed:", [...mixed()].join(","));

// Empty array — must terminate cleanly.
function* empty() {
  yield* [] as any;
  yield 1;
  yield* [99] as any;
}
console.log("empty:", [...empty()].join(","));

// String elements + spread into a let-bound array.
function* strs() {
  yield* ["x", "y"] as any;
}
const out: string[] = [...strs()];
console.log("strs:", out.join(","));

// Regression: for-of / spread over a plain array still use their fast
// paths (don't reach js_get_iterator), so unchanged behavior.
const arr = [10, 20, 30];
const fo: number[] = [];
for (const v of arr) fo.push(v);
console.log("for-of:", fo.join(","));
console.log("spread:", [...arr].join(","));

console.log("ALL 1831 YIELD-STAR-OVER-ARRAY TESTS PASSED");
