import { Readable } from "node:stream";
// push(chunk) returns true while the consumer should keep pushing, false
// when the internal buffer crosses highWaterMark.
const r = new Readable({ highWaterMark: 4, read() {} });
const ret = r.push("xxxxx"); // 5 bytes > hwm
console.log("typeof:", typeof ret);
console.log("returned false:", ret === false);
r.push(null);
