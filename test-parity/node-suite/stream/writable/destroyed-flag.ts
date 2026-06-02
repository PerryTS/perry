import { Writable } from "node:stream";
// Writable.destroyed flips true after destroy().
const w = new Writable({ write(_c, _e, cb) { cb(); } });
console.log("before:", w.destroyed);
w.destroy();
console.log("after destroy:", w.destroyed);
