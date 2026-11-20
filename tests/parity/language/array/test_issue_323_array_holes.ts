// Regression for issue #323 — `new Array(n)` slots used to read back as `0`
// instead of `undefined` because `js_array_alloc_with_length` left the element
// payload uninitialized (arena memory is zero-initialized on fresh blocks),
// and `Object.keys` / `n in arr` walked an ArrayHeader as if it were an
// ObjectHeader. Fix uses a HOLE sentinel (TAG_HOLE = 0x7FFC...0010) for
// never-written slots; reads translate HOLE → UNDEFINED so user code only
// sees `undefined`, while `Object.keys` and the `in` operator inspect slots
// directly to distinguish a hole from an explicit `arr[i] = undefined` write.

const values = new Array(4);

console.log("values.length", values.length);
console.log("values[1]", values[1]);
console.log("values[1] === undefined", values[1] === undefined);
console.log("1 in values", 1 in values);
console.log("Object.keys(values)", Object.keys(values));

const read = values[1];
console.log("read !== undefined", read !== undefined);

values[2] = undefined;
console.log("values[2]", values[2]);
console.log("values[2] === undefined", values[2] === undefined);
console.log("2 in values", 2 in values);

// Larger sizes hit the MIN_ARRAY_CAPACITY=16 padding boundary and beyond.
const big = new Array(20);
console.log("big.length", big.length);
console.log("big[0] === undefined", big[0] === undefined);
console.log("big[19] === undefined", big[19] === undefined);

// Single-arg `new Array(n)` where n=0 should produce an empty array.
const empty = new Array(0);
console.log("empty.length", empty.length);

// Function-scoped `new Array` covers the local-typed-array codegen path
// (the bounded-index fast path inside `for` loops).
function localCheck(): void {
  const a = new Array(3);
  let any = false;
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== undefined) any = true;
  }
  console.log("local any-defined:", any);
}
localCheck();
