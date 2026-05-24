import { Readable, Transform, PassThrough, pipeline } from "node:stream";
// Three-stage pipeline with mid-stage error — src and sink both destroyed.
const src = Readable.from(["x"]);
const mid = new Transform({
  transform(_c, _e, cb) {
    cb(new Error("mid-fail"));
  },
});
const sink = new PassThrough();
sink.on("data", () => {});
let errMsg: string | null = null;
pipeline(src, mid, sink, (err: any) => {
  errMsg = err && err.message;
});
setImmediate(() => {
  setImmediate(() => {
    console.log("err:", errMsg);
    console.log("src destroyed:", src.destroyed);
    console.log("sink destroyed:", sink.destroyed);
  });
});
