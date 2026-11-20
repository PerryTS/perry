import { Readable } from "node:stream";
// A user-supplied highWaterMark overrides the default.
const r = new Readable({ highWaterMark: 1234, read() {} });
console.log("custom hwm:", r.readableHighWaterMark);
