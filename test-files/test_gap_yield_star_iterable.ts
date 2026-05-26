// Test: yield* over an iterable that exposes a resolvable Symbol.iterator
// (#1831). `yield* X` resolves `X[Symbol.iterator]()` when X is iterable, and
// uses X directly when it is already an iterator (a generator). Validated
// byte-for-byte against `node --experimental-strip-types`.
//
// Scope: object-literal / own-property iterables + generator-call delegation
// (regression). Class/prototype-defined Symbol.iterator member access is a
// separate runtime gap (#1838); arrays are a separate sub-case.

// --- yield* over an object literal with [Symbol.iterator] ---
const range3 = {
  [Symbol.iterator]() {
    let i = 0;
    return {
      next: () => (i < 3 ? { value: i++, done: false } : { value: undefined, done: true }),
    };
  },
};
function* h() {
  yield* range3 as any;
  yield 9;
}
console.log("obj-iterable:", [...h()].join(",")); // 0,1,2,9

// --- the iterator object's own values come through with the trailing yield ---
const letters = {
  [Symbol.iterator]() {
    const arr = ["x", "y"];
    let i = 0;
    return { next: () => (i < arr.length ? { value: arr[i++], done: false } : { value: undefined, done: true }) };
  },
};
function* j() {
  yield "start";
  yield* letters as any;
  yield "end";
}
console.log("mixed:", [...j()].join(",")); // start,x,y,end

// --- regression: yield* over a generator CALL still works (no Symbol.iterator
//     on a perry generator object → js_get_iterator returns it unchanged) ---
function* inner() {
  yield 1;
  yield 2;
}
function* outer() {
  yield* inner();
  yield 3;
}
console.log("gencall:", [...outer()].join(",")); // 1,2,3

console.log("ALL YIELD-STAR-ITERABLE TESTS PASSED");
