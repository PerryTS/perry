import { Readable } from "node:stream";
// readable.toArray() (Node 17+) consumes the stream and resolves with the
// array of chunks.
const arr = await Readable.from(["a", "b", "c"]).toArray();
console.log("length:", arr.length);
console.log("joined:", arr.join(","));
