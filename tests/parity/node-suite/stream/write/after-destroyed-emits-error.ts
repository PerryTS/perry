import { Writable } from "node:stream";
// Calling write() on a destroyed Writable emits 'error' (ERR_STREAM_DESTROYED).
const w = new Writable({ write(_c, _e, cb) { cb(); } });
let errored = false;
w.on("error", () => (errored = true));
w.destroy();
w.write("after");
setImmediate(() => console.log("write-after-destroy errored:", errored));
