import { Readable } from "node:stream";
// new Readable() with NO options at all uses defaults — readable flag true,
// readableHighWaterMark = 16384, objectMode false.
const r = new Readable();
console.log("readable:", r.readable);
console.log("objectMode:", r.readableObjectMode);
console.log("hwm:", r.readableHighWaterMark);
