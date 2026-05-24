import { Readable } from "node:stream";
// removeAllListeners() without an event name clears EVERY listener for
// every event on the emitter.
const r = new Readable({ read() {} });
r.on("data", () => {});
r.on("end", () => {});
r.on("error", () => {});
r.removeAllListeners();
console.log("event names left:", r.eventNames().length);
