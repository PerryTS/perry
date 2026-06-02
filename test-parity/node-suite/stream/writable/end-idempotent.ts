import { Writable } from "node:stream";
// Calling end() a second time is a no-op (does not re-fire 'finish').
const w = new Writable({ write(_c, _e, cb) { cb(); } });
let finishes = 0;
w.on("finish", () => finishes++);
w.end();
w.end();
setImmediate(() => console.log("finish fires:", finishes));
