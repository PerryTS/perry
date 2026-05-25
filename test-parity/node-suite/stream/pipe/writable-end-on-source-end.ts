import { Readable, PassThrough } from "node:stream";
// pipe() — destination receives end() when source ends (autoEnd default).
const r = Readable.from(["a"]);
const dst = new PassThrough();
let dstEnded = false;
dst.on("end", () => (dstEnded = true));
dst.on("data", () => {});
r.pipe(dst);
setImmediate(() => {
  setImmediate(() => {
    console.log("dst ended on src end:", dstEnded);
  });
});
