import { Writable, isWritable } from "node:stream";
// stream.isWritable(stream) — true for a fresh Writable.
const w = new Writable({ write(_c, _e, cb) { cb(); } });
console.log("isWritable:", isWritable(w));
console.log("is true:", isWritable(w) === true);
