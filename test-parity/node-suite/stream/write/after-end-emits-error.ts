import { Writable } from "node:stream";
// Calling write() after end() emits 'error' (ERR_STREAM_WRITE_AFTER_END).
const w = new Writable({ write(_c, _e, cb) { cb(); } });
let errored = false;
w.on("error", () => (errored = true));
w.end("a");
w.write("b");
setImmediate(() => console.log("write-after-end errored:", errored));
