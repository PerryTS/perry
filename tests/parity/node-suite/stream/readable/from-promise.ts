import { Readable } from "node:stream";
// Readable.from(Promise) resolves the promise and yields the value as a chunk.
const r = Readable.from(Promise.resolve("resolved-value"));
const out: string[] = [];
for await (const c of r) out.push(String(c));
console.log("joined:", out.join("|"));
