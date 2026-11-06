import { Transform, isReadable, isWritable } from "node:stream";
// A Transform is both readable and writable.
const t = new Transform({ transform(c, _e, cb) { cb(null, c); } });
console.log("isReadable:", isReadable(t));
console.log("isWritable:", isWritable(t));
console.log("both true:", isReadable(t) === true && isWritable(t) === true);
