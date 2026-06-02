import { Writable } from "node:stream";
// Once a Writable has errored, subsequent write() calls emit error and
// return false.
const w = new Writable({ write(_c, _e, cb) { cb(); } });
let errors = 0;
w.on("error", () => errors++);
w.destroy(new Error("first"));
w.write("after-error");
setImmediate(() => console.log("errors:", errors));
