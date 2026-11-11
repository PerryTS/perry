import { Writable } from "node:stream";
// new Writable({ write }) constructs a writable stream instance.
const w = new Writable({ write(_c, _e, cb) { cb(); } });
console.log("instance:", w instanceof Writable);
console.log("writable flag:", w.writable);
console.log("write fn:", typeof w.write === "function");
console.log("end fn:", typeof w.end === "function");
