import { Duplex } from "node:stream";
// Duplex combines Readable + Writable in a single instance.
const d = new Duplex({ read() {}, write(_c, _e, cb) { cb(); } });
console.log("instance:", d instanceof Duplex);
console.log("readable:", d.readable);
console.log("writable:", d.writable);
