import { Readable } from "node:stream";
// setEncoding(enc) returns the stream itself for chaining.
const r = new Readable({ read() {} });
const ret = r.setEncoding("utf8");
console.log("returns self:", ret === r);
