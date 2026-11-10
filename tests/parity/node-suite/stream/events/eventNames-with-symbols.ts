import { Readable } from "node:stream";
import { errorMonitor } from "node:events";
// eventNames includes Symbol event keys.
const r = new Readable({ read() {} });
r.on("data", () => {});
r.on(errorMonitor, () => {});
const names = r.eventNames();
console.log("length:", names.length);
console.log("has data:", names.includes("data"));
console.log("has symbol:", names.includes(errorMonitor));
