import { Transform } from "node:stream";
// Transform extends Duplex with a transform(chunk, enc, cb) callback.
const t = new Transform({ transform(c, _e, cb) { cb(null, c); } });
console.log("instance:", t instanceof Transform);
console.log("readable:", t.readable);
console.log("writable:", t.writable);
