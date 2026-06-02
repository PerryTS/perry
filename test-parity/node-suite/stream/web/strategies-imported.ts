import {
  ReadableStream,
  CountQueuingStrategy,
  ByteLengthQueuingStrategy,
} from "node:stream/web";
// Both queuing strategy classes are exported from node:stream/web and
// can be passed to ReadableStream constructor's 2nd arg.
const c = new CountQueuingStrategy({ highWaterMark: 5 });
const b = new ByteLengthQueuingStrategy({ highWaterMark: 1024 });
console.log("count hwm:", c.highWaterMark);
console.log("byte hwm:", b.highWaterMark);
const rs = new ReadableStream({ start(c) { c.close(); } }, c);
console.log("rs constructed:", rs instanceof ReadableStream);
