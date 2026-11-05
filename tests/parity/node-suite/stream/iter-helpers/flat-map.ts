import { Readable } from "node:stream";
// readable.flatMap(fn) returns a stream of chunks flattened from fn(chunk).
const out = await Readable.from([1, 2, 3]).flatMap((n: number) => [n, n * 10]).toArray();
console.log("joined:", out.join(","));
