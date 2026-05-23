import { Readable } from "node:stream";
// Readable.from(string) yields the entire string as a single chunk.
const r = Readable.from("hello");
const out: string[] = [];
for await (const c of r) out.push(String(c));
console.log("joined:", out.join("|"));
