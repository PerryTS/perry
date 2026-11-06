import { Readable } from "node:stream";
// Calling pause() between two async-iter steps shouldn't stop iteration
// (the iterator pulls regardless of the flowing flag).
const r = Readable.from(["a", "b", "c"]);
const out: string[] = [];
for await (const v of r) {
  out.push(String(v));
  r.pause();
}
console.log("iterated:", out.join(","));
console.log("count:", out.length);
