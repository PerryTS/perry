import { Readable } from "node:stream";
// removeAllListeners('error') — removes ALL error listeners.
const r = new Readable({ read() {} });
r.on("error", () => {});
r.on("error", () => {});
console.log("before:", r.listenerCount("error"));
r.removeAllListeners("error");
console.log("after:", r.listenerCount("error"));
