import { Readable } from "node:stream";
// once('error', fn) — listener auto-removed even if error fires.
const r = new Readable({ read() {} });
let count = 0;
r.once("error", () => count++);
r.emit("error", new Error("first"));
console.log("after first:", count);
console.log("listener count:", r.listenerCount("error"));
