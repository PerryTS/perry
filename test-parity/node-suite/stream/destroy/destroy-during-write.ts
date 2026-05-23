import { Writable } from "node:stream";
// Destroying a stream during a write() callback surfaces via the error chain.
const w = new Writable({
  write(_c, _e, cb) {
    (this as any).destroy(new Error("mid-write"));
    cb();
  },
});
let msg = "";
w.on("error", (e) => (msg = (e as Error).message));
w.write("x");
setImmediate(() => console.log("msg:", msg));
