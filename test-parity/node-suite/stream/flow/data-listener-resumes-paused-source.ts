import { Readable } from "node:stream";
// Source paused; adding 'data' listener should NOT auto-resume (per Node docs).
const r = Readable.from(["a", "b"]);
r.pause();
console.log("paused before listener:", !r.readableFlowing);
let dataCount = 0;
r.on("data", () => dataCount++);
setImmediate(() => {
  console.log("data count while paused:", dataCount);
});
