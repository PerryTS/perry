import { Readable } from "node:stream";
// read() returns null when no buffered data is available.
const r = new Readable({ read() {} });
console.log("empty read:", r.read());
