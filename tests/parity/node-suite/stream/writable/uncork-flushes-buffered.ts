import { Writable } from "node:stream";
// cork() buffers writes; uncork() flushes them. Implementation detail:
// the buffered writes are processed in order on uncork.
const received: string[] = [];
const w = new Writable({
  write(c, _e, cb) {
    received.push(String(c));
    cb();
  },
});
w.cork();
w.write("a");
w.write("b");
w.write("c");
console.log("during cork:", received.length);
w.uncork();
w.end();
w.on("finish", () => {
  console.log("after uncork:", received.join(","));
});
