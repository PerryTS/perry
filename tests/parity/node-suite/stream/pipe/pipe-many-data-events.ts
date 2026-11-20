import { Readable, PassThrough } from "node:stream";
// Pipe many chunks — all reach destination in order.
const items = Array.from({ length: 20 }, (_, i) => `chunk${i}`);
const r = Readable.from(items);
const dst = new PassThrough();
const out: string[] = [];
dst.on("data", (c) => out.push(String(c)));
r.pipe(dst);
dst.on("end", () => {
  console.log("count:", out.length);
  console.log("first:", out[0]);
  console.log("last:", out[out.length - 1]);
});
