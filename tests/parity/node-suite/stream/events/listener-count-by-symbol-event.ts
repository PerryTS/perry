import { Readable } from "node:stream";
import { errorMonitor } from "node:events";
// listenerCount with a Symbol event name works.
const r = new Readable({ read() {} });
console.log("before:", r.listenerCount(errorMonitor));
r.on(errorMonitor, () => {});
r.on(errorMonitor, () => {});
console.log("after 2:", r.listenerCount(errorMonitor));
