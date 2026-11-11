import { Readable } from "node:stream";
// Pushed data accumulates in the buffer; readableLength reflects total
// buffered bytes after multiple pushes.
const r = new Readable({ highWaterMark: 16, read() {} });
r.push("ab");
r.push("cd");
r.push("ef");
console.log("length:", r.readableLength);
console.log("under hwm:", r.readableLength < r.readableHighWaterMark);
