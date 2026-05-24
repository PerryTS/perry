import { Readable } from "node:stream";
// Readable.from([1, undefined, 3]) — undefined as a value in objectMode
// has special meaning (it signals end). In non-objectMode...?
// In Node, the iterator pushes each yielded value; undefined push terminates the stream.
const r = Readable.from([1, undefined, 3]);
const out: any[] = [];
r.on("data", (c) => out.push(c));
r.on("end", () => {
  console.log("collected:", out.length);
  console.log("values:", out.join(","));
});
