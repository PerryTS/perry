import { Readable } from "node:stream";
// read() on a destroyed Readable returns null (not the buffered data).
const r = new Readable({ read() {} });
r.push("buffered");
r.destroy();
console.log("read after destroy:", r.read());
