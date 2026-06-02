import { Readable, PassThrough } from "node:stream";
// Stream instances expose Symbol.toStringTag through Object.prototype.toString.
const r = new Readable({ read() {} });
const p = new PassThrough();
console.log("Readable:", Object.prototype.toString.call(r));
console.log("PassThrough:", Object.prototype.toString.call(p));
