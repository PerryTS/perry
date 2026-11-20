import { Readable, PassThrough } from "node:stream";
// Source emits error before any data — pipe handler still fires.
const src = new Readable({ read() {} });
const dst = new PassThrough();
let dstDataCount = 0;
let srcErrCount = 0;
dst.on("data", () => dstDataCount++);
src.on("error", () => srcErrCount++);
dst.on("error", () => {});
src.pipe(dst);
src.destroy(new Error("immediate"));
setImmediate(() => {
  setImmediate(() => {
    console.log("dst data:", dstDataCount);
    console.log("src error fired:", srcErrCount);
  });
});
