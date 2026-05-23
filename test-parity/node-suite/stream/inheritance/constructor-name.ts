import { Readable, Writable, Duplex, Transform, PassThrough } from "node:stream";
// Each stream class's constructor.name matches its export name.
console.log("Readable:", new Readable({ read() {} }).constructor.name);
console.log("Writable:", new Writable({ write(_c, _e, cb) { cb(); } }).constructor.name);
console.log("Duplex:", new Duplex({ read() {}, write(_c, _e, cb) { cb(); } }).constructor.name);
console.log("Transform:", new Transform().constructor.name);
console.log("PassThrough:", new PassThrough().constructor.name);
