import { Readable } from "node:stream";
// Once a Readable has been fully consumed, re-iterating yields no chunks
// (it's not reusable).
const r = Readable.from(["a", "b"]);
const first: string[] = [];
for await (const c of r) first.push(String(c));
const second: string[] = [];
for await (const c of r) second.push(String(c));
console.log("first:", first.join(","));
console.log("second-empty:", second.length === 0);
