import { PassThrough, pipeline } from "node:stream";
// pipeline(asyncIterable, dst, cb) — async iterable as the source (not a stream).
async function* gen() {
  yield "x";
  yield "y";
}
const dst = new PassThrough();
const out: string[] = [];
dst.on("data", (c) => out.push(String(c)));
pipeline(gen(), dst, (err: any) => {
  console.log("err:", err);
  console.log("out:", out.join(","));
});
