import { Readable } from "node:stream";
import { json } from "node:stream/consumers";
// json() of a bare number literal.
const r = Readable.from(["42"]);
const result = await json(r);
console.log("result:", result);
console.log("is 42:", result === 42);
