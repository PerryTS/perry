import { Writable } from "node:stream";
// write() returns false once the internal buffer exceeds highWaterMark;
// the 'drain' event fires when the buffer is empty again.
const w = new Writable({
  highWaterMark: 4,
  write(_c, _e, cb) { setImmediate(cb); },
});
w.on("drain", () => console.log("drain fired"));
const ok = w.write("hello");
console.log("write returned:", ok);
w.end();
