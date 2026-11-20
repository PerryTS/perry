import { Writable } from "node:stream";
// writableLength reflects buffered bytes pending in the writable queue.
const w = new Writable({
  highWaterMark: 100,
  write(_c, _e, cb) { setImmediate(cb); }, // delay so writes accumulate
});
console.log("initial:", w.writableLength);
w.write("ab");
w.write("cd");
console.log("after 2 writes:", w.writableLength);
w.end();
