import { Readable } from "node:stream";
// Readable.from with a moderately large array — iteration counts correct.
const arr = Array.from({ length: 50 }, (_, i) => i);
const r = Readable.from(arr);
const out: number[] = [];
for await (const v of r) out.push(v as number);
console.log("count:", out.length);
console.log("first:", out[0], "last:", out[out.length - 1]);
console.log("sum:", out.reduce((a, b) => a + b, 0));
