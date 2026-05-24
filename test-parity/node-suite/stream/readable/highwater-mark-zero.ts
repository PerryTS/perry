import { Readable } from "node:stream";
// highWaterMark: 0 — push() returns false immediately because anything
// is at the limit.
const r = new Readable({ highWaterMark: 0, read() {} });
const a = r.push("x");
console.log("push(x) returned:", a);
console.log("readableHighWaterMark:", r.readableHighWaterMark);
