import { PassThrough } from "node:stream";
import { pipeline } from "node:stream/promises";
// `stream/promises` exposes pipeline as a Promise-returning helper used
// pervasively with `await`.
const src = new PassThrough();
const sink = new PassThrough();
const out: string[] = [];
sink.on("data", (c) => out.push(String(c)));
src.write("await-");
src.write("works");
src.end();
await pipeline(src, sink);
console.log("joined:", out.join(""));
