import { PassThrough, pipeline } from "node:stream";
// pipeline(syncIterable, dst, cb) — array source (not stream, not async).
const dst = new PassThrough();
const out: string[] = [];
dst.on("data", (c) => out.push(String(c)));
pipeline(["a", "b", "c"], dst, (err: any) => {
  console.log("err:", err);
  console.log("out:", out.join(","));
});
