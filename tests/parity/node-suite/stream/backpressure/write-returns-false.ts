import { Writable } from "node:stream";
// When the internal buffer exceeds highWaterMark, write() returns false so
// callers can wait for 'drain'.
const w = new Writable({
  highWaterMark: 1,
  write(_c, _e, cb) { setImmediate(cb); },
});
const a = w.write("xx");
console.log("write returned:", typeof a, a);
w.end();
