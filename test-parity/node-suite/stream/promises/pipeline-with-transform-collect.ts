import { Readable, Transform } from "node:stream";
import { pipeline } from "node:stream/promises";
// promises.pipeline with a transform + collecting async-fn sink.
const src = Readable.from(["a", "b"]);
const upper = new Transform({ transform(c, _e, cb) { cb(null, String(c).toUpperCase()); } });
const collected: string[] = [];
await pipeline(src, upper, async function (source: AsyncIterable<any>) {
  for await (const v of source) collected.push(String(v));
});
console.log("collected:", collected.join(","));
