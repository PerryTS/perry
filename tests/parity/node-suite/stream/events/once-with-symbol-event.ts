import { Readable } from "node:stream";
import { errorMonitor } from "node:events";
// once() can subscribe to Symbol events; the listener fires once then
// auto-removes.
const r = new Readable({ read() {} });
let count = 0;
r.once(errorMonitor, () => count++);
r.on("error", () => {});
r.destroy(new Error("first"));
setImmediate(() => {
  console.log("first count:", count);
  console.log("listenerCount(errorMonitor):", r.listenerCount(errorMonitor));
});
