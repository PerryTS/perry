import { compose, Readable, Transform } from "node:stream";
// An error in the composite should propagate back to destroy the source.
const src = new Readable({ read() {} });
const bad = new Transform({
  transform(_c, _e, cb) { cb(new Error("xform-fail")); },
});
const composed: any = compose(src, bad);
composed.on("error", () => {});
composed.on("data", () => {});
src.push("x");
setImmediate(() => {
  setImmediate(() => console.log("src destroyed after composite err:", src.destroyed));
});
