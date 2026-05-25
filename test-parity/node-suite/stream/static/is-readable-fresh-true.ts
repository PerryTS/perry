import { Readable, isReadable } from "node:stream";
// isReadable is true on a fresh Readable.
const r = new Readable({ read() {} });
console.log("isReadable:", isReadable(r));
console.log("is true:", isReadable(r) === true);
