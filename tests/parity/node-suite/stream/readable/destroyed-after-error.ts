import { Readable } from "node:stream";
// After 'error' event, destroyed flag is true.
const r = new Readable({ read() {} });
r.on("error", () => {
  setImmediate(() => {
    console.log("destroyed after error:", r.destroyed);
  });
});
r.destroy(new Error("err-then-destroyed"));
