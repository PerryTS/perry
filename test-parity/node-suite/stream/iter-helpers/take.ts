import { Readable } from "node:stream";
// readable.take(n) returns a Readable that yields only the first n chunks.
const out = await Readable.from([1, 2, 3, 4, 5]).take(2).toArray();
console.log("first 2:", out.join(","));
