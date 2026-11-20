import { Writable } from "node:stream";
// The writev(chunks, cb) option lets user code receive batched writes when
// the stream is corked (or otherwise has queued multiple chunks).
let batches = 0;
const w = new Writable({
  write(_c, _e, cb) { cb(); },
  writev(_chunks, cb) { batches++; cb(); },
});
w.on("finish", () => console.log("writev batches:", batches));
w.cork();
w.write("a");
w.write("b");
w.write("c");
process.nextTick(() => w.uncork());
w.end();
