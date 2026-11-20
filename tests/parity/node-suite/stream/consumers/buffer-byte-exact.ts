import { Readable } from "node:stream";
import { buffer } from "node:stream/consumers";
// buffer() preserves exact byte sequence across chunks.
const r = Readable.from([
  Buffer.from([0x01, 0x02]),
  Buffer.from([0x03, 0x04]),
]);
const buf = await buffer(r);
console.log("length:", buf.length);
console.log("bytes:", Array.from(buf).join(","));
