import { Readable, isErrored } from "node:stream";
// isErrored(stream) — true after destroy(err).
const r = new Readable({ read() {} });
r.on("error", () => {});
console.log("before:", isErrored(r));
r.destroy(new Error("boom"));
setImmediate(() => {
  console.log("after:", isErrored(r));
});
