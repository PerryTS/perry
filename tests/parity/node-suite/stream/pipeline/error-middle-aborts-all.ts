import { Readable, Transform, Writable, pipeline } from "node:stream";
// An error in a middle transform — source AND sink should be destroyed.
let srcClosed = false;
let sinkClosed = false;
const src = new Readable({ read() {} });
const mid = new Transform({ transform(_c, _e, cb) { cb(new Error("mid-fail")); } });
const sink = new Writable({ write(_c, _e, cb) { cb(); } });
src.on("close", () => (srcClosed = true));
sink.on("close", () => (sinkClosed = true));
pipeline(src, mid, sink, () => {});
src.push("x");
setImmediate(() => {
  setImmediate(() => {
    console.log("src destroyed:", srcClosed);
    console.log("sink destroyed:", sinkClosed);
  });
});
