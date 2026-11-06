import { Readable } from "node:stream";
// readable.drop(n) returns a Readable that skips the first n chunks.
const out = await Readable.from([1, 2, 3, 4, 5]).drop(2).toArray();
console.log("after drop 2:", out.join(","));
