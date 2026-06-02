import { Readable } from "node:stream";
// `await r.toArray()` returns a plain Array that can be spread.
const r = Readable.from([1, 2, 3]);
const arr = await (r as any).toArray();
console.log("is array:", Array.isArray(arr));
const spread = [...arr, 99];
console.log("spread:", spread.join(","));
