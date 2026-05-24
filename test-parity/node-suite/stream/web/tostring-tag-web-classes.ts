import { ReadableStream, WritableStream, TransformStream } from "node:stream/web";
// Web stream classes expose Symbol.toStringTag for runtime identification
// (so Object.prototype.toString.call(rs) === "[object ReadableStream]").
const rs = new ReadableStream();
const ws = new WritableStream();
const ts = new TransformStream();
console.log("R:", Object.prototype.toString.call(rs));
console.log("W:", Object.prototype.toString.call(ws));
console.log("T:", Object.prototype.toString.call(ts));
