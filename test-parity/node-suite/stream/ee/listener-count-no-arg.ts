import { PassThrough } from "node:stream";
// listenerCount(eventName) returns a number; listenerCount() with no arg
// (newer Node) returns total listener count across all events.
const p = new PassThrough();
p.on("data", () => {});
p.on("end", () => {});
console.log("typeof count(data):", typeof p.listenerCount("data"));
console.log("count('data'):", p.listenerCount("data"));
