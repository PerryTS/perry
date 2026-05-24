import { Transform } from "node:stream";
// _flush is called once after end() and before 'end' event — exactly once.
let flushCount = 0;
const t = new Transform({
  transform(c, _e, cb) { cb(null, c); },
  flush(cb) {
    flushCount++;
    cb();
  },
});
t.on("data", () => {});
t.on("end", () => {
  console.log("flush count:", flushCount);
  console.log("called once:", flushCount === 1);
});
t.write("x");
t.end();
