import { Writable } from "node:stream";
// After write() returns false, the 'drain' event fires once the buffer is
// empty again — the standard backpressure-recovery cycle.
const w = new Writable({
  highWaterMark: 1,
  write(_c, _e, cb) { setImmediate(cb); },
});
const ok1 = w.write("a"); // expect false
w.on("drain", () => {
  const ok2 = w.write("b");
  console.log("after drain ok:", ok2);
  w.end();
});
console.log("first write ok:", ok1);
