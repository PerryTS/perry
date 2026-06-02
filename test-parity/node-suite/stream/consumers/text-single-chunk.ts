import { Readable } from "node:stream";
import { text } from "node:stream/consumers";
// text() on a single-chunk string stream.
const r = Readable.from(["solo"]);
const result = await text(r);
console.log("result:", result);
console.log("matches:", result === "solo");
