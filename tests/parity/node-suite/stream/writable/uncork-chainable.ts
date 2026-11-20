import { Writable } from "node:stream";
// cork() returns undefined (no chaining); uncork() also returns undefined.
const w = new Writable({ write(_c, _e, cb) { cb(); } });
console.log("cork return:", w.cork());
console.log("uncork return:", w.uncork());
w.end();
