import { Readable } from "node:stream";
// read(0) is a special case — it doesn't consume the buffer but triggers
// internal '_read' (returns an empty Buffer or null).
const r = new Readable({ read() {} });
r.push("data");
const ret = r.read(0);
console.log("returned:", ret === null || (Buffer.isBuffer(ret) && ret.length === 0));
r.push(null);
