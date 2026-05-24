import { PassThrough, Transform, Duplex, Readable, Writable } from "node:stream";
// PassThrough extends Transform → Duplex → Readable + Writable.
const p = new PassThrough();
console.log("instanceof PassThrough:", p instanceof PassThrough);
console.log("instanceof Transform:", p instanceof Transform);
console.log("instanceof Duplex:", p instanceof Duplex);
console.log("instanceof Readable:", p instanceof Readable);
console.log("instanceof Writable:", p instanceof Writable);
