import { Readable } from "node:stream";
import { buffer } from "node:stream/consumers";
// stream/consumers.buffer(stream) consumes a Readable and resolves with a
// single concatenated Buffer.
const r = Readable.from([Buffer.from("ab"), Buffer.from("cd")]);
const buf = await buffer(r);
console.log("length:", buf.length);
console.log("content:", buf.toString());
