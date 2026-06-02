import { Writable } from "node:stream";
// When sink.write callback is invoked with an Error, the stream emits 'error'.
const w = new Writable({
  write(_c, _e, cb) {
    cb(new Error("sink-rejection"));
  },
});
let errMsg: string | null = null;
w.on("error", (err) => (errMsg = err && err.message));
w.write("x");
setImmediate(() => console.log("err:", errMsg));
