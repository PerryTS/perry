import { Readable } from "node:stream";
// unshift() should increase readableLength (it adds buffered bytes).
const r = new Readable({ read() {} });
r.push("hello");
const before = r.readableLength;
r.unshift("xx");
const after = r.readableLength;
console.log("before:", before);
console.log("after:", after);
console.log("grew:", after > before);
