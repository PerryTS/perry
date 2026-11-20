import { Readable } from "node:stream";
import { buffer } from "node:stream/consumers";
// buffer() concatenates multiple chunks into one Buffer.
const r = Readable.from([Buffer.from("ab"), Buffer.from("cd"), Buffer.from("ef")]);
const result = await buffer(r);
console.log("length:", result.length);
console.log("content:", result.toString("utf8"));
