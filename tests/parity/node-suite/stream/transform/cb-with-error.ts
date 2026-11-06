import { Transform } from "node:stream";
// transform() passing an Error to its callback emits 'error' on the stream.
const t = new Transform({
  transform(_c, _e, cb) { cb(new Error("nope")); },
});
let msg = "";
t.on("error", (e) => (msg = (e as Error).message));
t.write("x");
setImmediate(() => console.log("err:", msg));
