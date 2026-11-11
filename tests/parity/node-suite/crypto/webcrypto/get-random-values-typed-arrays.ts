// #12819: `crypto.getRandomValues(x)` only worked for Uint8Array/Buffer. For
// any other integer TypedArray it left the array untouched and returned a
// different object, so destructuring the result threw. Per Web Crypto it
// accepts every integer TypedArray, fills it in place and returns the same
// object.
import nodeCrypto from "node:crypto";

// The issue's own repro, verbatim.
const a = new Int32Array(4);
const r = crypto.getRandomValues(a);
console.log(r === a, Object.prototype.toString.call(r), a[0] !== 0);
const u = crypto.getRandomValues(new Uint8Array(4));
console.log(u.length, u instanceof Uint8Array);

// The destructuring form from the issue.
const [d0, d1, d2, d3] = crypto.getRandomValues(new Int32Array(4));
console.log("destructured:", typeof d0, typeof d1, typeof d2, typeof d3);

function filled(view: ArrayLike<number | bigint>): boolean {
  for (let i = 0; i < view.length; i++) if (view[i] != 0) return true;
  return false;
}

// Every integer TypedArray kind, through the global and the `node:crypto`
// namespace.
const kinds: [string, () => ArrayLike<number | bigint>][] = [
  ["Int8Array", () => new Int8Array(16)],
  ["Uint8Array", () => new Uint8Array(16)],
  ["Uint8ClampedArray", () => new Uint8ClampedArray(16)],
  ["Int16Array", () => new Int16Array(16)],
  ["Uint16Array", () => new Uint16Array(16)],
  ["Int32Array", () => new Int32Array(16)],
  ["Uint32Array", () => new Uint32Array(16)],
  ["BigInt64Array", () => new BigInt64Array(16)],
  ["BigUint64Array", () => new BigUint64Array(16)],
];
for (const [name, make] of kinds) {
  const g = make();
  const n = make();
  console.log(
    name,
    crypto.getRandomValues(g as any) === g && filled(g),
    nodeCrypto.getRandomValues(n as any) === n && filled(n),
  );
}

// Buffer stays a working control.
const buf = Buffer.alloc(16);
console.log("Buffer:", crypto.getRandomValues(buf) === buf, filled(buf));
