import { Readable } from "node:stream";
// pause() on a source that has push()'ed data — no 'data' fires until resume.
const r = new Readable({ read() {} });
let count = 0;
r.pause();
r.on("data", () => count++);
r.push("a");
r.push("b");
setImmediate(() => {
  console.log("data emitted while paused:", count);
  console.log("flowing:", r.readableFlowing);
});
