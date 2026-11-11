import { Readable } from "node:stream";
// unshift() after the stream has ended is documented as a no-op /
// recoverable; in practice Node emits 'error' (ERR_STREAM_PUSH_AFTER_EOF).
const r = new Readable({ read() {} });
r.push("a");
r.push(null);
let errMsg: string | null = null;
r.on("error", (err) => (errMsg = err && err.message));
r.on("data", () => {});
r.on("end", () => {
  try {
    r.unshift("late");
  } catch (e: any) {
    errMsg = e.message;
  }
  setImmediate(() => console.log("err:", errMsg));
});
