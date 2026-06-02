import { Readable } from "node:stream";
import { text } from "node:stream/consumers";
// text() concatenates multiple chunks into one string.
const r = Readable.from(["hello", " ", "world"]);
const result = await text(r);
console.log("result:", result);
console.log("matches:", result === "hello world");
