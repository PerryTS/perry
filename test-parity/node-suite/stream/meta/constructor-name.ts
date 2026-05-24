import { Readable, Writable, Duplex, Transform, PassThrough } from "node:stream";
// Stream class constructor.name reflects the class identity.
console.log("R:", new Readable({ read() {} }).constructor.name);
console.log("W:", new Writable({ write(_c, _e, cb) { cb(); } }).constructor.name);
console.log("D:", new Duplex({ read() {}, write(_c, _e, cb) { cb(); } }).constructor.name);
console.log("T:", new Transform({ transform(c, _e, cb) { cb(null, c); } }).constructor.name);
console.log("P:", new PassThrough().constructor.name);
