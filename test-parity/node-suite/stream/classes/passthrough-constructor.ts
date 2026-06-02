import { PassThrough } from "node:stream";
// PassThrough is a Transform that emits each input chunk unchanged.
const p = new PassThrough();
console.log("instance:", p instanceof PassThrough);
console.log("readable:", p.readable);
console.log("writable:", p.writable);
