import { Writable } from "node:stream";
// writableNeedDrain flips true after a write that returned false (i.e., the
// buffer crossed highWaterMark); flips back to false after 'drain'.
const w = new Writable({
  highWaterMark: 1,
  write(_c, _e, cb) { setImmediate(cb); },
});
w.write("xx");
console.log("needDrain after over-write:", w.writableNeedDrain);
w.end();
