import { Readable } from "node:stream";
// listenerCount(eventName) returns the number of registered listeners
// for the given event (excludes future once()-removed wrappers count).
const r = new Readable({ read() {} });
r.on("data", () => {});
r.on("data", () => {});
r.once("end", () => {});
console.log("data count:", r.listenerCount("data"));
console.log("end count:", r.listenerCount("end"));
console.log("error count:", r.listenerCount("error"));
