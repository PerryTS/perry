import { Readable } from "node:stream";
// destroy() returns the stream itself for chaining.
const r = new Readable({ read() {} });
const ret = r.destroy();
console.log("returns self:", ret === r);
