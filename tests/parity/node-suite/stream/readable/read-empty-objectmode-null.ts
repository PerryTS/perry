import { Readable } from "node:stream";
// objectMode read on empty stream returns null.
const r = new Readable({ objectMode: true, read() {} });
console.log("read:", r.read());
console.log("is null:", r.read() === null);
