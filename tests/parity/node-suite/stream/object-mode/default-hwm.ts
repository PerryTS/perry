import { Readable } from "node:stream";
// In objectMode the default highWaterMark is 16 (number of objects, not bytes).
const r = new Readable({ objectMode: true, read() {} });
console.log("objectMode hwm default:", r.readableHighWaterMark);
