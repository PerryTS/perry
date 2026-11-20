import { Writable } from "node:stream";
// end() called twice — second call is a no-op (no error, no duplicate 'finish').
let finishCount = 0;
const w = new Writable({ write(_c, _e, cb) { cb(); } });
w.on("finish", () => finishCount++);
w.end("a");
w.end(); // no-op
setImmediate(() => console.log("finish count:", finishCount));
