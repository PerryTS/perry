import { Writable } from "node:stream";
// end() returns the writable itself (chainable).
const w = new Writable({ write(_c, _e, cb) { cb(); } });
const returned = w.end();
console.log("end returns self:", returned === w);
