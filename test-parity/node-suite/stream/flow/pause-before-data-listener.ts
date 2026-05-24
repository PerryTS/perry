import { Readable } from "node:stream";
// Calling pause() before adding a 'data' listener prevents the stream
// from auto-flowing when the listener is later added.
const r = Readable.from(["a", "b", "c"]);
r.pause();
let count = 0;
r.on("data", () => count++);
setImmediate(() => {
  console.log("count while paused:", count);
  console.log("flowing:", r.readableFlowing);
});
