import { Writable } from "node:stream";
// write() returns false once the buffered bytes cross the highWaterMark.
// With a tiny HWM and a slow sink, the second write should return false.
const w = new Writable({
  highWaterMark: 2,
  write(_c, _e, cb) {
    setImmediate(cb); // delay so writes accumulate
  },
});
const a = w.write("aa");
const b = w.write("bb");
console.log("a:", a);
console.log("b:", b);
console.log("at-least-one false:", a === false || b === false);
