import { Readable } from "node:stream";
// Default highWaterMark for byte streams is 16384 (16 KiB).
const r = new Readable({ read() {} });
console.log("default highWaterMark:", r.readableHighWaterMark);
