import { Readable } from "node:stream";
// Multiple read() calls before any push — all return null.
const r = new Readable({ read() {} });
console.log("r1:", r.read());
console.log("r2:", r.read());
console.log("r3:", r.read());
console.log("all null:", r.read() === null && r.read() === null);
