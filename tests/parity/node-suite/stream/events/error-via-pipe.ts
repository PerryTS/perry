import { Readable, PassThrough } from "node:stream";
// emit('error') on a source while piped — destination receives the error
// via Node's internal error propagation in pipeline (NOT just pipe).
const src = new Readable({ read() {} });
const dst = new PassThrough();
let srcErr: string | null = null;
let dstErr: string | null = null;
src.on("error", (e) => (srcErr = e && e.message));
dst.on("error", (e) => (dstErr = e && e.message));
src.on("data", () => {});
dst.on("data", () => {});
src.pipe(dst);
src.emit("error", new Error("manual-error"));
setImmediate(() => {
  setImmediate(() => {
    console.log("src err:", srcErr);
    console.log("dst err:", dstErr);
  });
});
