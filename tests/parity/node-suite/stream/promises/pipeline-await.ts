import { Readable, Writable } from "node:stream";
import { pipeline } from "node:stream/promises";
// `stream/promises` exposes pipeline as a Promise-returning helper used
// pervasively with `await`.
const out: string[] = [];
const src = Readable.from(["await-", "works"]);
const sink = new Writable({
  write(chunk, _enc, cb) { out.push(String(chunk)); cb(); },
});
await pipeline(src, sink);
console.log("joined:", out.join(""));
