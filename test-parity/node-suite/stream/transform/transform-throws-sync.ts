import { Transform } from "node:stream";
// A synchronous throw inside transform() — Node catches and emits 'error'.
const t = new Transform({
  transform(_c, _e, _cb) {
    throw new Error("sync-throw");
  },
});
let errMsg: string | null = null;
t.on("error", (err) => (errMsg = err && err.message));
t.on("data", () => {});
t.write("x");
setImmediate(() => console.log("err:", errMsg));
