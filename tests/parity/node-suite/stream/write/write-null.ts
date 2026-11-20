import { Writable } from "node:stream";
// write(null) is an error in non-object mode; Node emits 'error' with
// ERR_STREAM_NULL_VALUES.
const w = new Writable({ write(_c, _e, cb) { cb(); } });
let errored = false;
w.on("error", () => (errored = true));
try {
  w.write(null as any);
} catch {
  errored = true;
}
setImmediate(() => console.log("rejected:", errored));
