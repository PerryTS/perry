import { Readable } from "node:stream";
// In objectMode, highWaterMark defaults to 16 (count of objects, not bytes).
const r = new Readable({ objectMode: true, read() {} });
console.log("hwm:", r.readableHighWaterMark);
console.log("is 16:", r.readableHighWaterMark === 16);
