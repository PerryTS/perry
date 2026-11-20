import { Readable, Writable } from "node:stream";
// When the destination errors, the source's 'unpipe' fires and downstream
// stops. Pipe alone does NOT propagate the error back to source by default.
let srcDestroyed = false;
const src = new Readable({ read() {} });
src.on("close", () => (srcDestroyed = true));
const dst = new Writable({
  write(_c, _e, cb) { cb(new Error("write-fail")); },
});
let dstErr: string | null = null;
dst.on("error", (e) => (dstErr = e && e.message));
src.pipe(dst);
src.push("x");
setImmediate(() => {
  setImmediate(() => {
    console.log("dst err:", dstErr);
    console.log("src destroyed (pipe alone shouldn't):", srcDestroyed);
  });
});
