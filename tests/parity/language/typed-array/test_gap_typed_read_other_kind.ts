// A read site typed for one typed-array kind reading cells of other kinds
// (owners, views, a shared buffer), with integral, fractional, negative-zero,
// NaN, string and out-of-bounds keys, and non-typed-array receivers.
function sum(a: Float32Array): number {
  let s = 0;
  for (let i = 0; i < a.length; i++) s += a[i];
  return s;
}
function at(a: Float32Array, k: any): any {
  return a[k];
}
function show(v: any): string {
  return typeof v === "bigint" ? v + "n" : String(v);
}
const recvs: any[] = [
  new Float32Array([1.5, 2.5, 3.5]),
  new Int32Array([-1, 2, 3]),
  new Uint8Array([250, 2, 3]),
  new Uint8ClampedArray([255, 0, 9]),
  new Int16Array([-300, 7, 8]),
  new Uint32Array([4000000000, 1, 2]),
  new Float64Array([0.1, 0.2, 0.3]),
  new Int8Array(new ArrayBuffer(8), 2, 3).fill(-5),
  new Float64Array(new SharedArrayBuffer(24)).fill(2.25),
  new BigInt64Array([-7n, 8n, 9n]),
];
for (const r of recvs) {
  const keys: any[] = [0, 2, -0, 1.5, NaN, -1, 3, "1", "x"];
  console.log(r.constructor.name, typeof r[0] === "bigint" ? "big" : sum(r), keys.map((k) => show(at(r, k))).join(","));
}
console.log("array", sum([1, 2, 3] as any), show(at([4, 5] as any, 1)), show(at({ 1: "o" } as any, 1)));
