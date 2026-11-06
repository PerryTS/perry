import { Readable } from "node:stream";
import { text } from "node:stream/consumers";
// text() on pure-ASCII Buffer chunks.
const r = Readable.from([Buffer.from("hello"), Buffer.from("world")]);
const result = await text(r);
console.log("result:", result);
console.log("length:", result.length);
