import { Readable } from "node:stream";
// Readable.isReadable(stream) reports whether the stream is still readable.
const r = new Readable({ read() {} });
console.log("isReadable:", Readable.isReadable(r));
