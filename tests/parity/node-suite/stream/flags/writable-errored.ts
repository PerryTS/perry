import { Writable } from "node:stream";
// writable.errored stores the Error passed to destroy(err) (or null).
const w = new Writable({ write(_c, _e, cb) { cb(); } });
w.on("error", () => {});
console.log("errored before:", w.errored);
w.destroy(new Error("write-fail"));
setImmediate(() => {
  console.log("errored typeof:", typeof w.errored);
  console.log("errored message:", w.errored ? (w.errored as Error).message : null);
});
