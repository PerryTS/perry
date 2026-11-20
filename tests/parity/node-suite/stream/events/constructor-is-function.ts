import { Readable, Writable } from "node:stream";
// Stream classes are functions.
console.log("Readable typeof:", typeof Readable);
console.log("Writable typeof:", typeof Writable);
const r = new Readable({ read() {} });
console.log("r.constructor typeof:", typeof r.constructor);
console.log("r.constructor.name:", r.constructor.name);
