import { Readable } from "node:stream";
// After full async-iteration completes, a second for-await yields nothing
// (the underlying stream is drained / ended).
const r = Readable.from(["a", "b"]);
const first: string[] = [];
for await (const v of r) first.push(String(v));
const second: string[] = [];
for await (const v of r) second.push(String(v));
console.log("first:", first.join(","));
console.log("second count:", second.length);
