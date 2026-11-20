import { Readable, PassThrough } from "node:stream";
// After source.pipe(dst) completes, source no longer listed in dst._writableState pipe.
const r = Readable.from(["x"]);
const dst = new PassThrough();
dst.on("data", () => {});
r.pipe(dst);
dst.on("end", () => {
  setImmediate(() => {
    console.log("source destroyed:", r.destroyed);
    console.log("dst destroyed:", dst.destroyed);
  });
});
