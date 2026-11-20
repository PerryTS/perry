import { Readable, PassThrough } from "node:stream";
// pipe() does NOT auto-propagate source errors to destination (different
// behavior from pipeline()). Source's 'error' fires; destination remains
// writable unless `{ end: false }` is overridden.
let dstErrors = 0;
let srcErrors = 0;
const src = new Readable({ read() {} });
const dst = new PassThrough();
src.on("error", () => srcErrors++);
dst.on("error", () => dstErrors++);
src.on("data", () => {});
dst.on("data", () => {});
src.pipe(dst);
src.destroy(new Error("src-fail"));
setImmediate(() => {
  setImmediate(() => {
    console.log("src errors:", srcErrors);
    console.log("dst errors:", dstErrors);
  });
});
