import { Readable } from "node:stream";
// Readable.from(map) iterates a Map's [key, value] entries.
const m = new Map([["a", 1], ["b", 2]]);
const out: any[] = [];
for await (const entry of Readable.from(m)) out.push(entry);
console.log("count:", out.length);
console.log("first-key:", out[0] && out[0][0]);
