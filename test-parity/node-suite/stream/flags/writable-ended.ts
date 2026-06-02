import { Writable } from "node:stream";
// writableEnded becomes true once end() has been called.
const w = new Writable({ write(_c, _e, cb) { cb(); } });
console.log("before:", w.writableEnded);
w.end();
console.log("after end():", w.writableEnded);
