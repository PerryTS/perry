import { PassThrough } from "node:stream";
// Streams inherit the full EventEmitter management surface.
const p = new PassThrough();
const fn = () => {};
p.on("data", fn);
console.log("listenerCount:", p.listenerCount("data"));
console.log("eventNames includes data:", p.eventNames().includes("data"));
p.removeListener("data", fn);
console.log("after remove:", p.listenerCount("data"));
