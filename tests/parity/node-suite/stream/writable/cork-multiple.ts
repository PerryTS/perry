import { Writable } from "node:stream";
// cork() can be called multiple times; uncork() must be called an equal
// number of times to actually flush.
const seen: number[] = [];
const w = new Writable({
  write(_c, _e, cb) { seen.push(1); cb(); },
});
w.cork();
w.cork();
w.write("a");
w.write("b");
w.uncork(); // still corked
console.log("after first uncork:", seen.length);
w.uncork(); // now flushes
process.nextTick(() => {
  console.log("after second uncork:", seen.length);
  w.end();
});
