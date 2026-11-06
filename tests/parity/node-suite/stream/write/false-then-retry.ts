import { Writable } from "node:stream";
// When write() returns false (buffer over HWM) the next write is queued —
// it eventually reaches _write after drain.
let writes = 0;
const w = new Writable({
  highWaterMark: 1,
  write(_c, _e, cb) { writes++; setImmediate(cb); },
});
const a = w.write("a");
const b = w.write("b");
console.log("a:", a, "b:", b);
w.end(() => console.log("writes:", writes));
