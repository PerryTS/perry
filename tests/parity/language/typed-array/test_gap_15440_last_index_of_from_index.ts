// %TypedArray%.prototype.lastIndexOf: a fromIndex below -length searches
// nothing and answers -1, for every element kind and receiver typing (#15440).
const kinds: any[] = [Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array,
  Int32Array, Uint32Array, Float32Array, Float64Array];
const froms: any[] = [-10, -4, -Infinity, -3, -2.9, -1, -0, 0, 1.7, 2, 10, Infinity, NaN, "1", null];
for (const K of kinds) {
  const a = new K([7, 8, 7]);
  console.log(K.name, froms.map((f) => `${String(f)}:${a.lastIndexOf(7, f)}`).join(" "));
}
const u8 = new Uint8Array([7, 8, 7]);
console.log("static", u8.lastIndexOf(7, -10), u8.lastIndexOf(7, -4), u8.lastIndexOf(7, -Infinity), u8.lastIndexOf(7, -3));
const anyU8: any = new Uint8Array([7, 8, 7]);
console.log("any", anyU8.lastIndexOf(7, -10), anyU8.lastIndexOf(7, -4), anyU8.lastIndexOf(7, -3));
console.log("subarray", u8.subarray(1).lastIndexOf(7, -3), u8.subarray(1).lastIndexOf(7, -2));
console.log("call", Uint8Array.prototype.lastIndexOf.call(u8, 7, -10));
