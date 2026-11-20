import { Readable } from "node:stream";
import { text } from "node:stream/consumers";
// text() decodes Buffer chunks as UTF-8.
const r = Readable.from([Buffer.from("hé"), Buffer.from("llo")]);
const result = await text(r);
console.log("result:", result);
console.log("length:", result.length);
