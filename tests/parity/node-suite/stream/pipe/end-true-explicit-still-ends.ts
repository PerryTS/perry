import { Readable, PassThrough } from "node:stream";
// pipe(dst, {end: true}) — same as default; dst is ended.
const r = Readable.from(["a"]);
const dst = new PassThrough();
let dstEnded = false;
dst.on("end", () => (dstEnded = true));
dst.on("data", () => {});
r.pipe(dst, { end: true });
setImmediate(() => {
  setImmediate(() => console.log("dst ended (end:true explicit):", dstEnded));
});
