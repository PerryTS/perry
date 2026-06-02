import { Writable } from "node:stream";
// If writev is defined, cork+uncork-batched writes go through writev rather
// than individual write calls.
let writevCalls = 0;
let writeCalls = 0;
const w = new Writable({
  write(_c, _e, cb) {
    writeCalls++;
    cb();
  },
  writev(_chunks, cb) {
    writevCalls++;
    cb();
  },
});
w.cork();
w.write("a");
w.write("b");
w.write("c");
w.uncork();
w.end();
w.on("finish", () => {
  console.log("writev calls:", writevCalls);
  console.log("write calls:", writeCalls);
  console.log("writev took the batch:", writevCalls > 0);
});
