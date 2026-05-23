import { Transform } from "node:stream";
// An exception thrown inside transform() surfaces as a stream 'error'.
const t = new Transform({
  transform(_c, _e, _cb) { throw new Error("transform-failed"); },
});
let msg = "";
t.on("error", (e) => (msg = (e as Error).message));
t.write("x");
setImmediate(() => console.log("error msg:", msg));
