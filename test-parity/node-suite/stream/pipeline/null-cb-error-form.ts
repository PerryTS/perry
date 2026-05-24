import { Readable, PassThrough, pipeline } from "node:stream";
// pipeline(src, dst, cb) — on success, cb is called with (null) or (undefined).
const src = Readable.from(["a"]);
const dst = new PassThrough();
dst.on("data", () => {});
pipeline(src, dst, (err) => {
  console.log("err truthy:", !!err);
  console.log("err type:", typeof err);
});
