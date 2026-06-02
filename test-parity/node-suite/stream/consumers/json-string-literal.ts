import { Readable } from "node:stream";
import { json } from "node:stream/consumers";
// json() of a quoted string literal.
const r = Readable.from([`"hello"`]);
const result = await json(r);
console.log("result:", result);
console.log("is hello:", result === "hello");
