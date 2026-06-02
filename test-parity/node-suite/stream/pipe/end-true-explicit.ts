import { Readable, PassThrough } from "node:stream";
// pipe(dst, { end: true }) — explicit form of the default; destination
// IS ended when source ends.
const r = Readable.from(["a"]);
const dst = new PassThrough();
let dstEnded = false;
dst.on("end", () => (dstEnded = true));
dst.on("data", () => {});
r.pipe(dst, { end: true });
setImmediate(() => {
  setImmediate(() => console.log("dst ended:", dstEnded));
});
