import { Readable } from "node:stream";
// read() before any push is non-flowing — should return null
// because there's no buffered data yet.
const r = new Readable({ read() {} });
const first = r.read();
console.log("read before push:", first);
console.log("is null:", first === null);
