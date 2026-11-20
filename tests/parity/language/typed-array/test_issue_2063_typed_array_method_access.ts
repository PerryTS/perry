// Issue #2063: TypedArray instance method access must NOT take the
// integer-indexed element fast path. Pre-fix, `ta[m]` for any string key
// `fptosi`'d the NaN-boxed string to index 0, so `typeof ta.copyWithin`
// (and every other method name) was "number" and numeric strings like
// `ta["2"]` returned element 0 instead of element 2.
//
// We assert `typeof ta[m] !== "number"` (rather than printing the typeof
// directly) so the test is byte-identical to Node regardless of whether the
// method is reified to a function value — reification is tracked separately
// in #2059. The point of THIS test is that the index fast path no longer
// shadows the property with a stray element value.

const a = new Int8Array([10, 20, 30, 40]);

// 1) Method-name keys (computed) no longer read a number.
const methods = [
  "copyWithin", "fill", "subarray", "slice", "set",
  "indexOf", "map", "filter", "reduce", "join",
];
for (const m of methods) {
  console.log(m, typeof a[m] !== "number");
}

// 2) Canonical numeric-index string keys read the correct element.
console.log('a["0"]:', a["0"]);
console.log('a["2"]:', a["2"]);
const k2 = "2";
console.log("a[k2]:", a[k2]);
const k0 = "0";
console.log("a[k0]:", a[k0]);

// 3) Out-of-range numeric string → undefined (not element 0).
const k9 = "9";
console.log("a[k9]:", a[k9]);

// 4) Plain numeric indices and .length are unaffected by the fix.
console.log("a[0]:", a[0], "a[3]:", a[3], "len:", a.length);

// 5) Float64Array behaves the same.
const f = new Float64Array([1.5, 2.5, 3.5]);
console.log('f["1"]:', f["1"]);
console.log("f.fill not number:", typeof f["fill"] !== "number");
