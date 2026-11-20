import { Readable } from "node:stream";
// Readable.from(Buffer) yields the buffer as a single chunk.
const r = Readable.from(Buffer.from("data"));
const out: Buffer[] = [];
for await (const c of r) out.push(c as Buffer);
console.log("chunks:", out.length);
console.log("joined:", Buffer.concat(out).toString());
