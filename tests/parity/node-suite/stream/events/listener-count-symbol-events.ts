import { Readable } from "node:stream";
import { errorMonitor } from "node:events";
// listenerCount works with Symbol event names too (e.g., errorMonitor).
const r = new Readable({ read() {} });
r.on(errorMonitor, () => {});
r.on(errorMonitor, () => {});
console.log("symbol count:", r.listenerCount(errorMonitor));
console.log("string-event count for data:", r.listenerCount("data"));
