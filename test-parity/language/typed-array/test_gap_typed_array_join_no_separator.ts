// `ta.join()` with no separator on a typed array whose kind codegen does not
// know statically (a sort()/reverse()/map() result, a Uint8ClampedArray local)
// goes through the dynamic typed-array dispatcher. Its missing-argument default
// was NaN, so the separator printed as "NaN": `1NaN3` for `1,3`
// (competitive A/sort-4, A/typedarray-5).
const f = new Float64Array([3, 1, 10, 2]);
console.log(f.sort().join(), new Int8Array([3, -1, 10, -20]).sort().join());
console.log(new Int8Array([3, 1]).reverse().join(), new Int8Array([3, 1]).map((x) => x * 2).join());
const c = new Uint8ClampedArray([300, -5, 7]);
console.log(c.join(), c.join("-"), c.join(undefined), c.at(), c.with(0, 9).join());
console.log(new Float32Array([1.5, 2]).fill(4).join(), new Int16Array([5, 6]).subarray(1).join());
