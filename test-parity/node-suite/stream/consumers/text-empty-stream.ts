import { Readable } from "node:stream";
import { text } from "node:stream/consumers";
// text() on an empty stream returns ''.
const r = Readable.from([]);
const result = await text(r);
console.log("result:", JSON.stringify(result));
console.log("is empty:", result === "");
