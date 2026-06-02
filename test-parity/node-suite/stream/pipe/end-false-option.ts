import { Readable, PassThrough } from "node:stream";
// pipe(dst, { end: false }) does NOT end the destination when the source
// ends. The destination remains writable.
const r = Readable.from(["a", "b"]);
const dst = new PassThrough();
let dstEnded = false;
dst.on("end", () => (dstEnded = true));
dst.on("data", () => {});
r.pipe(dst, { end: false });
r.on("end", () => {
  setImmediate(() => {
    console.log("dst ended:", dstEnded);
    console.log("dst still writable:", dst.writable);
    dst.end();
  });
});
