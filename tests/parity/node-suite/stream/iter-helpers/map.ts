import { Readable } from "node:stream";
// readable.map(fn) returns a Readable that yields fn(chunk) for each input.
const out = await Readable.from([1, 2, 3]).map((n: number) => n * 2).toArray();
console.log("doubled:", out.join(","));
