import { Readable, Transform } from "node:stream";
import { pipeline } from "node:stream/promises";
// 5-stage promises-pipeline.
const src = Readable.from(["a"]);
const t1 = new Transform({ transform(c, _e, cb) { cb(null, String(c) + "1"); } });
const t2 = new Transform({ transform(c, _e, cb) { cb(null, String(c) + "2"); } });
const t3 = new Transform({ transform(c, _e, cb) { cb(null, String(c) + "3"); } });
const collected: string[] = [];
async function* sink(source: AsyncIterable<any>) {
  for await (const v of source) collected.push(String(v));
}
await pipeline(src, t1, t2, t3, sink as any);
console.log("result:", collected.join(","));
