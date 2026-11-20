import { Readable } from "node:stream";
// readable.read(n) returns up to n bytes (Buffer) from the internal buffer.
const r = new Readable({ read() {} });
r.push(Buffer.from("abcdef"));
r.push(null);
const chunk = r.read(3);
console.log("chunk length:", chunk && chunk.length);
console.log("first byte:", chunk && chunk[0]);
