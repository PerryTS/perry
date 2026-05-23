import { PassThrough } from "node:stream";
// removeAllListeners(eventName) clears every listener for that event.
const p = new PassThrough();
p.on("data", () => {});
p.on("data", () => {});
console.log("before:", p.listenerCount("data"));
p.removeAllListeners("data");
console.log("after:", p.listenerCount("data"));
