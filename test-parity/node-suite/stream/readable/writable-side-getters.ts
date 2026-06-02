import { Duplex } from "node:stream";
// Duplex exposes writable getters: writableEnded, writableHighWaterMark.
const d = new Duplex({ read() {}, write(_c, _e, cb) { cb(); } });
console.log("writableEnded initial:", d.writableEnded);
console.log("writableHighWaterMark:", typeof d.writableHighWaterMark);
console.log("writableLength:", typeof d.writableLength);
console.log("writableObjectMode:", d.writableObjectMode);
