import { Readable } from "node:stream";
// Readable.from(Uint8Array) — TypedArrays are iterable, yields each byte.
// (Differs from Buffer which short-circuits to single chunk in Node.)
const arr = new Uint8Array([1, 2, 3]);
const r = Readable.from(arr);
const out: any[] = [];
for await (const v of r) out.push(v);
console.log("count:", out.length);
console.log("first:", out[0]);
