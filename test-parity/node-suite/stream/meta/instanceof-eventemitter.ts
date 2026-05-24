import { Readable, Writable, Duplex, Transform, PassThrough } from "node:stream";
import { EventEmitter } from "node:events";
// All Node stream classes inherit from EventEmitter.
const r = new Readable({ read() {} });
const w = new Writable({ write(_c, _e, cb) { cb(); } });
const d = new Duplex({ read() {}, write(_c, _e, cb) { cb(); } });
const t = new Transform({ transform(c, _e, cb) { cb(null, c); } });
const p = new PassThrough();
console.log("R:", r instanceof EventEmitter);
console.log("W:", w instanceof EventEmitter);
console.log("D:", d instanceof EventEmitter);
console.log("T:", t instanceof EventEmitter);
console.log("P:", p instanceof EventEmitter);
